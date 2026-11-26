//! The Java→Rust native bridge and the v1/v2 RPC handlers.

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use ferrite_jvm::NativeMethod;
use ferrite_jvm::jni::objects::{JObject, JString};
use ferrite_jvm::jni::{self, JNIEnv};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::config::{PluginHostConfig, ServerInfo};
use crate::protocol::{self, CancelRequest, RpcError, ScheduleRequest};
use crate::scheduler::{CancelOutcome, Scheduler, TaskId};
use crate::state::{
    BlockGetRequest, BlockSetRequest, EntityDropChanceSetRequest, EntityEquipmentSetRequest,
    HostStateProvider, MAX_PLUGIN_CHANNEL_LEN, MAX_PLUGIN_MESSAGE_BYTES, OpenInventoryItem,
    PlayerInventorySetRequest, UuidRequest,
};

/// The native state installed for the current host.
///
/// A previous failed start can leave an old state behind; [`install`] replaces it so a retry
/// does not keep a stale scheduler.
static CURRENT: Mutex<Option<Arc<NativeState>>> = Mutex::new(None);

#[derive(Default)]
struct PluginChannels {
    incoming: HashMap<String, HashSet<String>>,
    outgoing: HashMap<String, HashSet<String>>,
}

pub(crate) struct NativeState {
    api_version: String,
    server: ServerInfo,
    plugins_dir: String,
    data_dir: String,
    scheduler: Arc<Scheduler>,
    known_plugins: Mutex<HashMap<String, String>>,
    plugin_channels: RwLock<PluginChannels>,
    provider: RwLock<Option<Arc<dyn HostStateProvider>>>,
    /// Bitmask of events with at least one registered Java listener (`u64::MAX` until synced).
    event_mask: AtomicU64,
    stopped: AtomicBool,
}

impl NativeState {
    pub(crate) fn new(
        config: &PluginHostConfig,
        scheduler: Arc<Scheduler>,
        plugins: impl IntoIterator<Item = String>,
    ) -> Self {
        let mut known_plugins = HashMap::new();
        for name in plugins {
            known_plugins.insert(name.to_ascii_lowercase(), name);
        }
        Self {
            api_version: config.api_version.clone(),
            server: config.server.clone(),
            plugins_dir: protocol::path_string(&config.plugins_dir),
            data_dir: protocol::path_string(&config.data_dir),
            scheduler,
            known_plugins: Mutex::new(known_plugins),
            plugin_channels: RwLock::new(PluginChannels::default()),
            provider: RwLock::new(None),
            event_mask: AtomicU64::new(u64::MAX),
            stopped: AtomicBool::new(false),
        }
    }

    /// Lock-free check whether any Java plugin currently listens to `name`.
    #[inline]
    pub(crate) fn has_event_listener(&self, name: &str) -> bool {
        let Some(bit) = protocol::event_bit(name) else {
            return false;
        };
        (self.event_mask.load(Ordering::Relaxed) & bit) != 0
    }

    pub(crate) fn set_provider(&self, provider: Arc<dyn HostStateProvider>) {
        *self
            .provider
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(provider);
    }

    fn provider(&self) -> Result<Arc<dyn HostStateProvider>, RpcError> {
        self.provider
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .ok_or_else(|| {
                RpcError::new(
                    "state_unavailable",
                    crate::state::StateError::unavailable().message,
                )
            })
    }

    fn provider_ops(&self) -> Vec<&'static str> {
        self.provider
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .map(|provider| provider.supported_ops())
            .unwrap_or_default()
    }

    fn provider_v3_ops(&self) -> Vec<&'static str> {
        self.provider
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .map(|provider| provider.supported_v3_ops())
            .unwrap_or_default()
    }

    /// Whether a provider is installed, independent of which ops it advertises.
    fn has_provider(&self) -> bool {
        self.provider
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some()
    }

    pub(crate) fn tick_provider(&self) {
        let provider = self
            .provider
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if let Some(provider) = provider
            && catch_unwind(AssertUnwindSafe(|| provider.tick())).is_err()
        {
            tracing::error!("the plugin state provider panicked during tick");
        }
    }

    pub(crate) fn is_incoming_channel_registered(&self, channel: &str) -> bool {
        let channels = self
            .plugin_channels
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        channels
            .incoming
            .values()
            .any(|registered| registered.contains(channel))
    }

    pub(crate) fn clear_all_channels(&self) {
        let mut channels = self
            .plugin_channels
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        channels.incoming.clear();
        channels.outgoing.clear();
    }

    fn ensure_known_plugin(&self, plugin: &str) -> Result<String, RpcError> {
        let known = self
            .known_plugins
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        known
            .get(&plugin.to_ascii_lowercase())
            .cloned()
            .ok_or_else(|| RpcError::new("plugin_not_found", format!("unknown plugin '{plugin}'")))
    }

    fn validate_channel(channel: &str) -> Result<(), RpcError> {
        if channel.is_empty() || channel.len() > MAX_PLUGIN_CHANNEL_LEN {
            return Err(RpcError::invalid(format!(
                "plugin channel must be 1..={MAX_PLUGIN_CHANNEL_LEN} bytes"
            )));
        }
        Ok(())
    }

    pub(crate) fn shutdown_provider(&self) {
        self.clear_all_channels();
        let provider = self
            .provider
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(provider) = provider
            && catch_unwind(AssertUnwindSafe(|| provider.shutdown())).is_err()
        {
            tracing::error!("the plugin state provider panicked during shutdown");
        }
    }

    pub(crate) fn register_plugin(&self, name: &str) {
        self.known_plugins
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(name.to_ascii_lowercase(), name.to_owned());
    }

    pub(crate) fn unregister_plugin(&self, name: &str) {
        let key = name.to_ascii_lowercase();
        self.known_plugins
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&key);
        let mut channels = self
            .plugin_channels
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        channels.incoming.remove(&key);
        channels.outgoing.remove(&key);
    }

    pub(crate) fn mark_stopped(&self) {
        self.stopped.store(true, Ordering::SeqCst);
    }

    fn known_plugin(&self, name: &str) -> Option<String> {
        self.known_plugins
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&name.to_ascii_lowercase())
            .cloned()
    }

    pub(crate) fn handle_call(&self, op: &str, request: &Value) -> Result<Value, RpcError> {
        if self.stopped.load(Ordering::SeqCst) {
            return Err(RpcError::new(
                "shutdown",
                "the plugin host is shutting down",
            ));
        }
        match op {
            "host.ping" => Ok(json!({
                "pong": true,
                "contract_version": crate::CONTRACT_VERSION,
            })),
            "host.info" => Ok(json!({
                "contract_version": crate::CONTRACT_VERSION,
                "api_version": self.api_version.as_str(),
                "server": {
                    "name": self.server.name.as_str(),
                    "version": self.server.version.as_str(),
                    "motd": self.server.motd.as_str(),
                    "max_players": self.server.max_players,
                    "online_mode": self.server.online_mode,
                    "view_distance": self.server.view_distance,
                },
                "plugins_dir": self.plugins_dir.as_str(),
                "data_dir": self.data_dir.as_str(),
                "capabilities": {
                    "v1_ops": ["host.ping", "host.info", "scheduler.schedule", "scheduler.cancel"],
                    "v2_ops": self.provider_ops(),
                    "v3_ops": self.provider_v3_ops(),
                    "state_provider": self.has_provider(),
                },
            })),
            "scheduler.schedule" => {
                let request: ScheduleRequest = serde_json::from_value(request.clone())
                    .map_err(|error| RpcError::invalid(error.to_string()))?;
                let plugin = self.known_plugin(&request.plugin).ok_or_else(|| {
                    RpcError::new(
                        "plugin_not_found",
                        format!("unknown plugin '{}'", request.plugin),
                    )
                })?;
                if request.handle <= 0 {
                    return Err(RpcError::invalid("handle must be a positive integer"));
                }
                let task = self.scheduler.schedule_java(
                    &plugin,
                    request.handle,
                    request.delay_ticks,
                    request.period_ticks,
                    request.is_async,
                );
                Ok(json!({"task_id": task.get()}))
            }
            "scheduler.cancel" => {
                let request: CancelRequest = serde_json::from_value(request.clone())
                    .map_err(|error| RpcError::invalid(error.to_string()))?;
                let plugin = self.known_plugin(&request.plugin).ok_or_else(|| {
                    RpcError::new(
                        "plugin_not_found",
                        format!("unknown plugin '{}'", request.plugin),
                    )
                })?;
                match self
                    .scheduler
                    .cancel_for_plugin(&plugin, TaskId::new(request.task_id))
                {
                    CancelOutcome::Cancelled => Ok(json!({"cancelled": true})),
                    CancelOutcome::NotFound => Ok(json!({"cancelled": false})),
                    CancelOutcome::NotOwner => Err(RpcError::new(
                        "not_owner",
                        format!(
                            "task {} is not owned by plugin '{}'",
                            request.task_id, plugin
                        ),
                    )),
                }
            }
            "world.block_get" => {
                let request: BlockGetRequest = parse_v2(request)?;
                let provider = self.provider()?;
                let snapshot = provider.block_get(&request).map_err(RpcError::from)?;
                serde_json::to_value(snapshot)
                    .map_err(|error| RpcError::new("invalid_response", error.to_string()))
            }
            "world.block_set_async" => {
                let request: BlockSetRequest = parse_v2(request)?;
                let provider = self.provider()?;
                let accepted = provider.block_set_async(&request).map_err(RpcError::from)?;
                serde_json::to_value(accepted)
                    .map_err(|error| RpcError::new("invalid_response", error.to_string()))
            }
            "world.chunk_loaded" => {
                let request: ChunkLoadedRequest = parse_v2(request)?;
                let provider = self.provider()?;
                let loaded = provider
                    .chunk_loaded(&request.world, request.x, request.z)
                    .map_err(RpcError::from)?;
                Ok(json!({
                    "world": request.world,
                    "x": request.x,
                    "z": request.z,
                    "loaded": loaded,
                }))
            }
            "player.state_get" => {
                let request: UuidRequest = parse_v2(request)?;
                self.provider()?
                    .player_state_get(&request.uuid)
                    .map_err(RpcError::from)
            }
            "player.inventory_get" => {
                let request: UuidRequest = parse_v2(request)?;
                let snapshot = self
                    .provider()?
                    .player_inventory_get(&request.uuid)
                    .map_err(RpcError::from)?;
                serde_json::to_value(snapshot)
                    .map_err(|error| RpcError::new("invalid_response", error.to_string()))
            }
            "player.inventory_set_async" => {
                let request: PlayerInventorySetRequest = parse_v2(request)?;
                let accepted = self
                    .provider()?
                    .player_inventory_set_async(&request)
                    .map_err(RpcError::from)?;
                serde_json::to_value(accepted)
                    .map_err(|error| RpcError::new("invalid_response", error.to_string()))
            }
            "entity.state_get" => {
                let request: UuidRequest = parse_v2(request)?;
                self.provider()?
                    .entity_state_get(&request.uuid)
                    .map_err(RpcError::from)
            }
            "world.chunk_status" => {
                let request: ChunkLoadedRequest = parse_v2(request)?;
                let snapshot = self
                    .provider()?
                    .chunk_status(&request.world, request.x, request.z)
                    .map_err(RpcError::from)?;
                serde_json::to_value(snapshot)
                    .map_err(|error| RpcError::new("invalid_response", error.to_string()))
            }
            "world.loaded_chunks" => {
                let request: WorldSnapshotRequest = parse_v2(request)?;
                let snapshot = self
                    .provider()?
                    .loaded_chunks(&request.world, request.limit)
                    .map_err(RpcError::from)?;
                serde_json::to_value(snapshot)
                    .map_err(|error| RpcError::new("invalid_response", error.to_string()))
            }
            "world.entities_snapshot" => {
                let request: WorldSnapshotRequest = parse_v2(request)?;
                let snapshot = self
                    .provider()?
                    .entities_snapshot(&request.world, request.limit)
                    .map_err(RpcError::from)?;
                serde_json::to_value(snapshot)
                    .map_err(|error| RpcError::new("invalid_response", error.to_string()))
            }
            "world.biome_get" => {
                let request: BiomeGetRequest = parse_v2(request)?;
                let snapshot = self
                    .provider()?
                    .biome_get(&request.world, request.x, request.y, request.z)
                    .map_err(RpcError::from)?;
                serde_json::to_value(snapshot)
                    .map_err(|error| RpcError::new("invalid_response", error.to_string()))
            }
            "entity.equipment_set_async" => {
                let request: EntityEquipmentSetRequest = parse_v2(request)?;
                let accepted = self
                    .provider()?
                    .entity_equipment_set_async(&request)
                    .map_err(RpcError::from)?;
                serde_json::to_value(accepted)
                    .map_err(|error| RpcError::new("invalid_response", error.to_string()))
            }
            "entity.drop_chance_set_async" => {
                let request: EntityDropChanceSetRequest = parse_v2(request)?;
                let accepted = self
                    .provider()?
                    .entity_drop_chance_set_async(&request)
                    .map_err(RpcError::from)?;
                serde_json::to_value(accepted)
                    .map_err(|error| RpcError::new("invalid_response", error.to_string()))
            }
            "plugin_message.register" => {
                let request: PluginChannelRegisterRequest = parse_v2(request)?;
                let plugin_key = self
                    .ensure_known_plugin(&request.plugin)?
                    .to_ascii_lowercase();
                Self::validate_channel(&request.channel)?;
                let mut channels = self
                    .plugin_channels
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                match request.direction.as_str() {
                    "incoming" => {
                        channels
                            .incoming
                            .entry(plugin_key)
                            .or_default()
                            .insert(request.channel);
                    }
                    "outgoing" => {
                        channels
                            .outgoing
                            .entry(plugin_key)
                            .or_default()
                            .insert(request.channel);
                    }
                    "both" => {
                        channels
                            .incoming
                            .entry(plugin_key.clone())
                            .or_default()
                            .insert(request.channel.clone());
                        channels
                            .outgoing
                            .entry(plugin_key)
                            .or_default()
                            .insert(request.channel);
                    }
                    other => {
                        return Err(RpcError::invalid(format!(
                            "invalid plugin_message direction '{other}'"
                        )));
                    }
                }
                Ok(json!({"registered": true}))
            }
            "plugin_message.unregister" => {
                let request: PluginChannelUnregisterRequest = parse_v2(request)?;
                let plugin_key = self
                    .ensure_known_plugin(&request.plugin)?
                    .to_ascii_lowercase();
                if let Some(channel) = &request.channel {
                    Self::validate_channel(channel)?;
                }
                let mut channels = self
                    .plugin_channels
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let remove_from = |map: &mut HashMap<String, HashSet<String>>| {
                    if let Some(channel) = &request.channel {
                        if let Some(set) = map.get_mut(&plugin_key) {
                            set.remove(channel);
                        }
                    } else {
                        map.remove(&plugin_key);
                    }
                };
                match request.direction.as_str() {
                    "incoming" => remove_from(&mut channels.incoming),
                    "outgoing" => remove_from(&mut channels.outgoing),
                    "both" => {
                        remove_from(&mut channels.incoming);
                        remove_from(&mut channels.outgoing);
                    }
                    other => {
                        return Err(RpcError::invalid(format!(
                            "invalid plugin_message direction '{other}'"
                        )));
                    }
                }
                Ok(json!({"unregistered": true}))
            }
            "plugin_message.send" => {
                let request: PluginMessageSendRequest = parse_v2(request)?;
                let plugin_key = self
                    .ensure_known_plugin(&request.plugin)?
                    .to_ascii_lowercase();
                Self::validate_channel(&request.channel)?;
                {
                    let channels = self
                        .plugin_channels
                        .read()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let registered = channels
                        .outgoing
                        .get(&plugin_key)
                        .is_some_and(|set| set.contains(&request.channel));
                    if !registered {
                        return Err(RpcError::new(
                            "channel_not_registered",
                            format!(
                                "plugin '{}' has not registered outgoing channel '{}'",
                                request.plugin, request.channel
                            ),
                        ));
                    }
                }
                let data = BASE64
                    .decode(request.data_base64.as_bytes())
                    .map_err(|error| RpcError::invalid(format!("invalid data_base64: {error}")))?;
                if data.len() > MAX_PLUGIN_MESSAGE_BYTES {
                    return Err(RpcError::new(
                        "payload_too_large",
                        format!(
                            "plugin message payload {} bytes exceeds max {}",
                            data.len(),
                            MAX_PLUGIN_MESSAGE_BYTES
                        ),
                    ));
                }
                self.provider()?
                    .send_plugin_message(request.player_uuid.as_deref(), &request.channel, data)
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "player.title_send" => {
                let request: PlayerTitleSendRequest = parse_v2(request)?;
                self.provider()?
                    .send_player_title(
                        &request.player_uuid,
                        request.title,
                        request.subtitle,
                        request.fade_in,
                        request.stay,
                        request.fade_out,
                        request.clear,
                        request.reset,
                    )
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "player.action_bar_send" => {
                let request: PlayerActionBarSendRequest = parse_v2(request)?;
                self.provider()?
                    .send_player_action_bar(&request.player_uuid, &request.text)
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "player.sound_play" => {
                let request: PlayerSoundPlayRequest = parse_v2(request)?;
                self.provider()?
                    .play_player_sound(
                        &request.player_uuid,
                        &request.sound,
                        request.category,
                        request.x,
                        request.y,
                        request.z,
                        request.volume,
                        request.pitch,
                        request.seed,
                    )
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "player.particle_spawn" => {
                let request: PlayerParticleSpawnRequest = parse_v2(request)?;
                self.provider()?
                    .spawn_player_particle(
                        &request.player_uuid,
                        &request.particle,
                        request.x,
                        request.y,
                        request.z,
                        request.count,
                        request.offset_x,
                        request.offset_y,
                        request.offset_z,
                        request.extra,
                        request.force,
                    )
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "player.boss_bar_update" => {
                let request: PlayerBossBarUpdateRequest = parse_v2(request)?;
                self.provider()?
                    .update_player_boss_bar(
                        &request.player_uuid,
                        &request.bar_uuid,
                        &request.operation,
                        request.title,
                        request.progress,
                        request.color,
                        request.overlay,
                        request.flags,
                    )
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "world.border_set" => {
                let request: WorldBorderSetRequest = parse_v2(request)?;
                self.provider()?
                    .set_world_border(
                        &request.world,
                        request.x,
                        request.z,
                        request.diameter,
                        request.warning_blocks,
                        request.warning_time,
                    )
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "player.scoreboard_update" => {
                let request: PlayerScoreboardUpdateRequest = parse_v2(request)?;
                self.provider()?
                    .update_player_scoreboard(
                        &request.player_uuid,
                        &request.kind,
                        &request.name,
                        request.method,
                        &request.display,
                        request.hearts,
                        request.slot,
                        &request.owner,
                        request.score,
                        request.params,
                        request.players,
                    )
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "player.abilities_set" => {
                let request: PlayerAbilitiesSetRequest = parse_v2(request)?;
                self.provider()?
                    .set_player_abilities(
                        &request.player_uuid,
                        request.allow_flight,
                        request.flying,
                        request.invulnerable,
                        request.fly_speed,
                        request.walk_speed,
                    )
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "player.list_header_footer" => {
                let request: PlayerListHeaderFooterRequest = parse_v2(request)?;
                self.provider()?
                    .set_player_list_header_footer(
                        &request.player_uuid,
                        &request.header,
                        &request.footer,
                    )
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "player.experience_set" => {
                let request: PlayerExperienceSetRequest = parse_v2(request)?;
                self.provider()?
                    .set_player_experience(
                        &request.player_uuid,
                        request.level,
                        request.progress,
                        request.total,
                    )
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "player.teleport" => {
                let request: PlayerTeleportRequest = parse_v2(request)?;
                self.provider()?
                    .teleport_player(
                        &request.player_uuid,
                        &request.world,
                        request.x,
                        request.y,
                        request.z,
                        request.yaw,
                        request.pitch,
                    )
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "player.health_set" => {
                let request: PlayerHealthSetRequest = parse_v2(request)?;
                self.provider()?
                    .set_player_health(&request.player_uuid, request.health)
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "player.food_set" => {
                let request: PlayerFoodSetRequest = parse_v2(request)?;
                self.provider()?
                    .set_player_food(&request.player_uuid, request.food)
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "player.gamemode_set" => {
                let request: PlayerGameModeSetRequest = parse_v2(request)?;
                self.provider()?
                    .set_player_game_mode(&request.player_uuid, &request.mode)
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "player.kick" => {
                let request: PlayerKickRequest = parse_v2(request)?;
                self.provider()?
                    .kick_player(&request.player_uuid, request.reason)
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "player.message_send" => {
                let request: PlayerMessageSendRequest = parse_v2(request)?;
                self.provider()?
                    .send_player_message(&request.player_uuid, &request.message)
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "player.velocity_set" => {
                let request: PlayerVelocitySetRequest = parse_v2(request)?;
                self.provider()?
                    .set_player_velocity(&request.player_uuid, request.vx, request.vy, request.vz)
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "player.visibility_set" => {
                let request: PlayerVisibilitySetRequest = parse_v2(request)?;
                self.provider()?
                    .set_player_visibility(
                        &request.viewer_uuid,
                        &request.target_uuid,
                        request.hidden,
                    )
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "player.compass_target_set" => {
                let request: PlayerCompassTargetSetRequest = parse_v2(request)?;
                self.provider()?
                    .set_player_compass_target(
                        &request.player_uuid,
                        request.x,
                        request.y,
                        request.z,
                    )
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "player.potion_effect_update" => {
                let request: PlayerPotionEffectUpdateRequest = parse_v2(request)?;
                self.provider()?
                    .update_player_potion_effect(
                        &request.player_uuid,
                        &request.effect,
                        request.remove,
                        request.amplifier,
                        request.duration,
                        request.flags,
                    )
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "world.lightning_strike" => {
                let request: WorldLightningStrikeRequest = parse_v2(request)?;
                self.provider()?
                    .strike_lightning(&request.world, request.x, request.y, request.z)
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "world.explosion_create" => {
                let request: WorldExplosionCreateRequest = parse_v2(request)?;
                self.provider()?
                    .create_explosion(
                        &request.world,
                        request.x,
                        request.y,
                        request.z,
                        request.power,
                        request.fire,
                        request.breaks,
                    )
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "player.open_inventory" => {
                let request: PlayerOpenInventoryRequest = parse_v2(request)?;
                self.provider()?
                    .open_player_inventory(
                        &request.player_uuid,
                        &request.title,
                        request.rows,
                        request.shulker,
                        request.items,
                    )
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "player.close_inventory" => {
                let request: PlayerUuidOnlyRequest = parse_v2(request)?;
                self.provider()?
                    .close_player_inventory(&request.player_uuid)
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "player.damage" => {
                let request: PlayerDamageRequest = parse_v2(request)?;
                self.provider()?
                    .damage_player(&request.player_uuid, request.amount)
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "player.respawn" => {
                let request: PlayerUuidOnlyRequest = parse_v2(request)?;
                self.provider()?
                    .respawn_player(&request.player_uuid)
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "player.display_name_set" => {
                let request: PlayerDisplayNameSetRequest = parse_v2(request)?;
                self.provider()?
                    .set_player_display_name(&request.player_uuid, request.display_name)
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "player.cooldown_set" => {
                let request: PlayerCooldownSetRequest = parse_v2(request)?;
                self.provider()?
                    .set_player_cooldown(&request.player_uuid, &request.group, request.ticks)
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "player.transfer" => {
                let request: PlayerTransferRequest = parse_v2(request)?;
                self.provider()?
                    .transfer_player(&request.player_uuid, &request.host, request.port)
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "player.block_change_send" => {
                let request: PlayerBlockChangeSendRequest = parse_v2(request)?;
                self.provider()?
                    .send_player_block_change(
                        &request.player_uuid,
                        request.x,
                        request.y,
                        request.z,
                        &request.material,
                    )
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "world.spawn_entity" => {
                let request: WorldSpawnEntityRequest = parse_v2(request)?;
                self.provider()?
                    .spawn_world_entity(
                        &request.world,
                        &request.kind,
                        request.x,
                        request.y,
                        request.z,
                        request.vx,
                        request.vy,
                        request.vz,
                        &request.uuid,
                    )
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "world.drop_item" => {
                let request: WorldDropItemRequest = parse_v2(request)?;
                self.provider()?
                    .drop_world_item(
                        &request.world,
                        &request.item,
                        request.amount,
                        request.x,
                        request.y,
                        request.z,
                        request.natural,
                        &request.uuid,
                    )
                    .map_err(RpcError::from)?;
                Ok(json!({"sent": true}))
            }
            "host.event_mask_sync" => {
                let request: HostEventMaskSyncRequest = parse_v2(request)?;
                self.event_mask.store(request.mask, Ordering::Relaxed);
                Ok(json!({"mask": request.mask}))
            }
            _ => Err(RpcError::new(
                "unknown_op",
                format!("unknown host op '{op}'"),
            )),
        }
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ChunkLoadedRequest {
    world: String,
    x: i32,
    z: i32,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WorldSnapshotRequest {
    world: String,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BiomeGetRequest {
    world: String,
    x: i32,
    y: i32,
    z: i32,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginChannelRegisterRequest {
    plugin: String,
    channel: String,
    direction: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerTitleSendRequest {
    player_uuid: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    subtitle: Option<String>,
    #[serde(default = "default_fade_in")]
    fade_in: i32,
    #[serde(default = "default_stay")]
    stay: i32,
    #[serde(default = "default_fade_out")]
    fade_out: i32,
    #[serde(default)]
    clear: bool,
    #[serde(default)]
    reset: bool,
}

fn default_fade_in() -> i32 {
    10
}
fn default_stay() -> i32 {
    70
}
fn default_fade_out() -> i32 {
    20
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerActionBarSendRequest {
    player_uuid: String,
    text: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerSoundPlayRequest {
    player_uuid: String,
    sound: String,
    #[serde(default)]
    category: i32,
    x: f64,
    y: f64,
    z: f64,
    volume: f32,
    pitch: f32,
    #[serde(default)]
    seed: i64,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerParticleSpawnRequest {
    player_uuid: String,
    particle: String,
    x: f64,
    y: f64,
    z: f64,
    #[serde(default = "default_particle_count")]
    count: i32,
    #[serde(default)]
    offset_x: f32,
    #[serde(default)]
    offset_y: f32,
    #[serde(default)]
    offset_z: f32,
    #[serde(default)]
    extra: f32,
    #[serde(default)]
    force: bool,
}

fn default_particle_count() -> i32 {
    1
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerBossBarUpdateRequest {
    player_uuid: String,
    bar_uuid: String,
    operation: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    progress: f32,
    #[serde(default)]
    color: i32,
    #[serde(default)]
    overlay: u8,
    #[serde(default)]
    flags: u8,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WorldBorderSetRequest {
    world: String,
    x: f64,
    z: f64,
    diameter: f64,
    #[serde(default)]
    warning_blocks: i32,
    #[serde(default)]
    warning_time: i32,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerScoreboardUpdateRequest {
    player_uuid: String,
    kind: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    method: u8,
    #[serde(default)]
    display: String,
    #[serde(default)]
    hearts: bool,
    #[serde(default)]
    slot: i32,
    #[serde(default)]
    owner: String,
    #[serde(default)]
    score: i32,
    #[serde(default)]
    params: Vec<String>,
    #[serde(default)]
    players: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerAbilitiesSetRequest {
    player_uuid: String,
    #[serde(default)]
    allow_flight: bool,
    #[serde(default)]
    flying: bool,
    #[serde(default)]
    invulnerable: bool,
    #[serde(default = "default_fly_speed")]
    fly_speed: f32,
    #[serde(default = "default_walk_speed")]
    walk_speed: f32,
}

fn default_fly_speed() -> f32 {
    0.1
}

fn default_walk_speed() -> f32 {
    0.2
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerListHeaderFooterRequest {
    player_uuid: String,
    #[serde(default)]
    header: String,
    #[serde(default)]
    footer: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerExperienceSetRequest {
    player_uuid: String,
    #[serde(default)]
    level: Option<i32>,
    #[serde(default)]
    progress: Option<f32>,
    #[serde(default)]
    total: Option<i32>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerTeleportRequest {
    player_uuid: String,
    world: String,
    x: f64,
    y: f64,
    z: f64,
    #[serde(default)]
    yaw: f32,
    #[serde(default)]
    pitch: f32,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerHealthSetRequest {
    player_uuid: String,
    health: f64,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerFoodSetRequest {
    player_uuid: String,
    food: u8,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerGameModeSetRequest {
    player_uuid: String,
    mode: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerKickRequest {
    player_uuid: String,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerMessageSendRequest {
    player_uuid: String,
    message: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerVelocitySetRequest {
    player_uuid: String,
    vx: f64,
    vy: f64,
    vz: f64,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerVisibilitySetRequest {
    viewer_uuid: String,
    target_uuid: String,
    hidden: bool,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerCompassTargetSetRequest {
    player_uuid: String,
    x: i32,
    y: i32,
    z: i32,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerPotionEffectUpdateRequest {
    player_uuid: String,
    effect: String,
    #[serde(default)]
    remove: bool,
    #[serde(default)]
    amplifier: i32,
    #[serde(default)]
    duration: i32,
    #[serde(default = "default_potion_flags")]
    flags: u8,
}

fn default_potion_flags() -> u8 {
    0x02 | 0x04
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WorldLightningStrikeRequest {
    #[serde(default = "default_world_key")]
    world: String,
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WorldExplosionCreateRequest {
    #[serde(default = "default_world_key")]
    world: String,
    x: f64,
    y: f64,
    z: f64,
    power: f32,
    #[serde(default)]
    fire: bool,
    #[serde(default = "default_true")]
    breaks: bool,
}

fn default_world_key() -> String {
    "minecraft:overworld".to_owned()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerOpenInventoryRequest {
    player_uuid: String,
    #[serde(default)]
    title: String,
    rows: u8,
    #[serde(default)]
    shulker: bool,
    #[serde(default)]
    items: Vec<Option<OpenInventoryItem>>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerUuidOnlyRequest {
    player_uuid: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerDamageRequest {
    player_uuid: String,
    amount: f32,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerDisplayNameSetRequest {
    player_uuid: String,
    #[serde(default)]
    display_name: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerCooldownSetRequest {
    player_uuid: String,
    group: String,
    ticks: i32,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerTransferRequest {
    player_uuid: String,
    host: String,
    port: u16,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerBlockChangeSendRequest {
    player_uuid: String,
    x: i32,
    y: i32,
    z: i32,
    material: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WorldSpawnEntityRequest {
    #[serde(default = "default_world_key")]
    world: String,
    kind: String,
    x: f64,
    y: f64,
    z: f64,
    #[serde(default)]
    vx: f64,
    #[serde(default)]
    vy: f64,
    #[serde(default)]
    vz: f64,
    uuid: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WorldDropItemRequest {
    #[serde(default = "default_world_key")]
    world: String,
    item: String,
    #[serde(default = "default_drop_amount")]
    amount: u8,
    x: f64,
    y: f64,
    z: f64,
    #[serde(default)]
    natural: bool,
    uuid: String,
}

fn default_drop_amount() -> u8 {
    1
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct HostEventMaskSyncRequest {
    mask: u64,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginChannelUnregisterRequest {
    plugin: String,
    #[serde(default)]
    channel: Option<String>,
    direction: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginMessageSendRequest {
    plugin: String,
    #[serde(default)]
    player_uuid: Option<String>,
    channel: String,
    data_base64: String,
}

fn parse_v2<T: DeserializeOwned>(request: &Value) -> Result<T, RpcError> {
    serde_json::from_value(request.clone()).map_err(|error| RpcError::invalid(error.to_string()))
}

/// Installs the state used by native callbacks.
pub(crate) fn install(state: Arc<NativeState>) {
    *CURRENT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(state);
}

fn current() -> Option<Arc<NativeState>> {
    CURRENT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Handles one `HostBridge.call` RPC from Rust code.
pub(crate) fn call(op: &str, request: &Value) -> Result<Value, RpcError> {
    let state = current().ok_or_else(RpcError::not_running)?;
    state.handle_call(op, request)
}

/// Native method table for `dev.ferrite.host.HostBridge`.
pub(crate) fn native_methods() -> [NativeMethod; 4] {
    [
        NativeMethod::new(
            "call",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            hostbridge_call as *const () as *mut c_void,
        ),
        NativeMethod::new(
            "log",
            "(ILjava/lang/String;Ljava/lang/String;)V",
            hostbridge_log as *const () as *mut c_void,
        ),
        NativeMethod::new(
            "blockGetFast",
            "(Ljava/lang/String;III)I",
            hostbridge_block_get_fast as *const () as *mut c_void,
        ),
        NativeMethod::new(
            "blockSetFast",
            "(Ljava/lang/String;IIII)J",
            hostbridge_block_set_fast as *const () as *mut c_void,
        ),
    ]
}

/// Zero-JSON hot-path JNI call: returns block state id (`>= 0`) or `-1` on error/unloaded chunk.
extern "system" fn hostbridge_block_get_fast(
    env: *mut jni::sys::JNIEnv,
    _class: jni::sys::jclass,
    world: jni::sys::jstring,
    x: jni::sys::jint,
    y: jni::sys::jint,
    z: jni::sys::jint,
) -> jni::sys::jint {
    std::panic::catch_unwind(|| {
        // SAFETY: the JVM passes a valid environment for the duration of the native call.
        let Ok(mut env) = (unsafe { JNIEnv::from_raw(env) }) else {
            return -1;
        };
        let Ok(world) = read_jstring(&mut env, world) else {
            return -1;
        };
        let Some(state) = current() else {
            return -1;
        };
        if state.stopped.load(Ordering::SeqCst) {
            return -1;
        }
        let Ok(provider) = state.provider() else {
            return -1;
        };
        let req = BlockGetRequest { world, x, y, z };
        match provider.block_get(&req) {
            Ok(snap) => snap.state_id as jni::sys::jint,
            Err(_) => -1,
        }
    })
    .unwrap_or(-1)
}

/// Zero-JSON hot-path JNI call: enqueues a block write and returns request_id (`> 0`) or `-1` on error.
extern "system" fn hostbridge_block_set_fast(
    env: *mut jni::sys::JNIEnv,
    _class: jni::sys::jclass,
    world: jni::sys::jstring,
    x: jni::sys::jint,
    y: jni::sys::jint,
    z: jni::sys::jint,
    state_id: jni::sys::jint,
) -> jni::sys::jlong {
    std::panic::catch_unwind(|| {
        if state_id < 0 {
            return -1;
        }
        // SAFETY: the JVM passes a valid environment for the duration of the native call.
        let Ok(mut env) = (unsafe { JNIEnv::from_raw(env) }) else {
            return -1;
        };
        let Ok(world) = read_jstring(&mut env, world) else {
            return -1;
        };
        let Some(state) = current() else {
            return -1;
        };
        if state.stopped.load(Ordering::SeqCst) {
            return -1;
        }
        let Ok(provider) = state.provider() else {
            return -1;
        };
        let req = BlockSetRequest {
            world,
            x,
            y,
            z,
            state_id: state_id as u32,
            expected_state_id: None,
        };
        match provider.block_set_async(&req) {
            Ok(accepted) => accepted.request_id as jni::sys::jlong,
            Err(_) => -1,
        }
    })
    .unwrap_or(-1)
}

fn read_jstring(env: &mut JNIEnv<'_>, value: jni::sys::jstring) -> jni::errors::Result<String> {
    if value.is_null() {
        return Ok(String::new());
    }
    // SAFETY: the JVM owns this string for the duration of the native call.
    let object = unsafe { JObject::from_raw(value) };
    let text = JString::from(object);
    Ok(env.get_string(&text)?.into())
}

fn respond(env: &mut JNIEnv<'_>, result: Result<Value, RpcError>) -> jni::sys::jstring {
    let envelope = match result {
        Ok(value) => json!({"ok": true, "result": value}),
        Err(error) => json!({
            "ok": false,
            "error": {"code": error.code, "message": error.message},
        }),
    };
    match env.new_string(envelope.to_string()) {
        Ok(text) => text.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// `static native String call(String op, String requestJson)`.
extern "system" fn hostbridge_call(
    env: *mut jni::sys::JNIEnv,
    _class: jni::sys::jclass,
    op: jni::sys::jstring,
    request: jni::sys::jstring,
) -> jni::sys::jstring {
    std::panic::catch_unwind(|| {
        // SAFETY: the JVM passes a valid environment for the duration of the call.
        let Ok(mut env) = (unsafe { JNIEnv::from_raw(env) }) else {
            return std::ptr::null_mut();
        };
        let op = match read_jstring(&mut env, op) {
            Ok(op) => op,
            Err(_) => return std::ptr::null_mut(),
        };
        let request = match read_jstring(&mut env, request) {
            Ok(request) => request,
            Err(_) => return std::ptr::null_mut(),
        };
        let request: Value = match serde_json::from_str(&request) {
            Ok(request) => request,
            Err(error) => {
                return respond(
                    &mut env,
                    Err(RpcError::new("invalid_request", error.to_string())),
                );
            }
        };
        respond(&mut env, call(&op, &request))
    })
    .unwrap_or(std::ptr::null_mut())
}

/// `static native void log(int level, String plugin, String message)`.
extern "system" fn hostbridge_log(
    env: *mut jni::sys::JNIEnv,
    _class: jni::sys::jclass,
    level: jni::sys::jint,
    plugin: jni::sys::jstring,
    message: jni::sys::jstring,
) {
    let _ = std::panic::catch_unwind(|| {
        // SAFETY: the JVM passes a valid environment for the duration of the call.
        let Ok(mut env) = (unsafe { JNIEnv::from_raw(env) }) else {
            return;
        };
        let plugin = read_jstring(&mut env, plugin).unwrap_or_default();
        let message = read_jstring(&mut env, message).unwrap_or_default();
        let plugin = if plugin.is_empty() {
            "server".to_owned()
        } else {
            plugin
        };
        match level {
            0 => tracing::trace!(target: "ferrite_plugin", plugin = %plugin, "{message}"),
            1 => tracing::debug!(target: "ferrite_plugin", plugin = %plugin, "{message}"),
            2 => tracing::info!(target: "ferrite_plugin", plugin = %plugin, "{message}"),
            3 => tracing::warn!(target: "ferrite_plugin", plugin = %plugin, "{message}"),
            4 => tracing::error!(target: "ferrite_plugin", plugin = %plugin, "{message}"),
            _ => tracing::info!(target: "ferrite_plugin", plugin = %plugin, "{message}"),
        }
    });
}

#[cfg(test)]
mod v2_tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::atomic::AtomicU64;

    use crate::state::{BlockSetAccepted, BlockSnapshot, StateError};

    struct DeterministicFakeStateProvider {
        next: AtomicU64,
    }

    impl HostStateProvider for DeterministicFakeStateProvider {
        fn block_get(&self, request: &BlockGetRequest) -> Result<BlockSnapshot, StateError> {
            Ok(BlockSnapshot {
                world: request.world.clone(),
                x: request.x,
                y: request.y,
                z: request.z,
                state_id: 1,
                material: "minecraft:stone".into(),
                properties: BTreeMap::new(),
                loaded: true,
            })
        }

        fn block_set_async(
            &self,
            _request: &BlockSetRequest,
        ) -> Result<BlockSetAccepted, StateError> {
            Ok(BlockSetAccepted {
                queued: true,
                request_id: self.next.fetch_add(1, Ordering::SeqCst) + 1,
            })
        }

        fn chunk_loaded(&self, _world: &str, _x: i32, _z: i32) -> Result<bool, StateError> {
            Ok(true)
        }

        fn supported_ops(&self) -> Vec<&'static str> {
            vec![
                "world.block_get",
                "world.block_set_async",
                "world.chunk_loaded",
            ]
        }
    }

    fn native_state() -> NativeState {
        NativeState::new(&PluginHostConfig::default(), Arc::new(Scheduler::new()), [])
    }

    #[test]
    fn v2_info_handshake_and_block_round_trip_use_the_provider() {
        let state = native_state();
        let info = state.handle_call("host.info", &json!({})).unwrap();
        assert_eq!(info["contract_version"], crate::CONTRACT_VERSION);
        assert_eq!(info["capabilities"]["state_provider"], false);
        assert_eq!(
            state
                .handle_call(
                    "world.block_get",
                    &json!({"world":"minecraft:overworld","x":0,"y":64,"z":0})
                )
                .unwrap_err()
                .code,
            "state_unavailable"
        );

        state.set_provider(Arc::new(DeterministicFakeStateProvider {
            next: AtomicU64::new(0),
        }));
        let info = state.handle_call("host.info", &json!({})).unwrap();
        assert_eq!(info["capabilities"]["state_provider"], true);
        assert!(
            info["capabilities"]["v2_ops"]
                .as_array()
                .unwrap()
                .contains(&json!("world.chunk_loaded"))
        );

        let block = state
            .handle_call(
                "world.block_get",
                &json!({"world":"minecraft:overworld","x":0,"y":64,"z":0}),
            )
            .unwrap();
        assert_eq!(block["material"], "minecraft:stone");
        assert_eq!(block["loaded"], true);

        let accepted = state
            .handle_call(
                "world.block_set_async",
                &json!({"world":"minecraft:overworld","x":0,"y":64,"z":0,"state_id":1}),
            )
            .unwrap();
        assert_eq!(accepted["queued"], true);
        assert_eq!(accepted["request_id"], 1);
        let chunk = state
            .handle_call(
                "world.chunk_loaded",
                &json!({"world":"minecraft:overworld","x":0,"z":0}),
            )
            .unwrap();
        assert_eq!(chunk["loaded"], true);
    }

    /// A provider that is installed but deliberately advertises no v2 op.
    struct SilentStateProvider;

    impl HostStateProvider for SilentStateProvider {
        fn block_get(&self, _request: &BlockGetRequest) -> Result<BlockSnapshot, StateError> {
            Err(StateError::unsupported("world.block_get"))
        }

        fn block_set_async(
            &self,
            _request: &BlockSetRequest,
        ) -> Result<BlockSetAccepted, StateError> {
            Err(StateError::unsupported("world.block_set_async"))
        }

        fn supported_ops(&self) -> Vec<&'static str> {
            Vec::new()
        }
    }

    #[test]
    fn installed_provider_is_advertised_and_shutdown_closes_every_op() {
        let state = native_state();
        state.set_provider(Arc::new(SilentStateProvider));
        let info = state.handle_call("host.info", &json!({})).unwrap();
        assert_eq!(info["capabilities"]["state_provider"], true);
        assert_eq!(info["capabilities"]["v2_ops"], json!([]));

        state.mark_stopped();
        for op in ["host.ping", "host.info", "world.block_get"] {
            assert_eq!(
                state.handle_call(op, &json!({})).unwrap_err().code,
                "shutdown",
                "{op} after shutdown"
            );
        }
    }

    #[test]
    fn v2_unknown_ops_and_invalid_requests_fail_closed() {
        let state = native_state();
        assert_eq!(
            state
                .handle_call("world.unknown", &json!({}))
                .unwrap_err()
                .code,
            "unknown_op"
        );
        assert_eq!(
            state
                .handle_call(
                    "world.block_get",
                    &json!({"world":"minecraft:overworld","x":0})
                )
                .unwrap_err()
                .code,
            "invalid_request"
        );
        assert_eq!(
            state.handle_call("host.ping", &json!({})).unwrap()["contract_version"],
            crate::CONTRACT_VERSION
        );
    }

    #[test]
    fn plugin_message_channels_enforce_ownership_size_and_shutdown_cleanup() {
        let state = NativeState::new(
            &PluginHostConfig::default(),
            Arc::new(Scheduler::new()),
            ["Foo".to_owned()],
        );
        state.set_provider(Arc::new(SilentStateProvider));

        // Unknown plugin is rejected.
        assert_eq!(
            state
                .handle_call(
                    "plugin_message.register",
                    &json!({"plugin": "Missing", "channel": "ferrite:echo", "direction": "incoming"}),
                )
                .unwrap_err()
                .code,
            "plugin_not_found"
        );

        // Register incoming + outgoing for Foo (from native_state() fixture).
        state
            .handle_call(
                "plugin_message.register",
                &json!({"plugin": "Foo", "channel": "ferrite:echo", "direction": "both"}),
            )
            .unwrap();
        assert!(state.is_incoming_channel_registered("ferrite:echo"));
        assert!(!state.is_incoming_channel_registered("ferrite:other"));

        // Sending on unregistered outgoing channel fails closed.
        assert_eq!(
            state
                .handle_call(
                    "plugin_message.send",
                    &json!({
                        "plugin": "Foo",
                        "channel": "ferrite:unregistered",
                        "data_base64": "aGk="
                    }),
                )
                .unwrap_err()
                .code,
            "channel_not_registered"
        );

        // Oversized payload (> MAX_PLUGIN_MESSAGE_BYTES) fails closed.
        let oversized = BASE64.encode(vec![0u8; MAX_PLUGIN_MESSAGE_BYTES + 1]);
        assert_eq!(
            state
                .handle_call(
                    "plugin_message.send",
                    &json!({
                        "plugin": "Foo",
                        "channel": "ferrite:echo",
                        "data_base64": oversized
                    }),
                )
                .unwrap_err()
                .code,
            "payload_too_large"
        );

        // Unregister incoming removes incoming registration.
        state
            .handle_call(
                "plugin_message.unregister",
                &json!({"plugin": "Foo", "channel": "ferrite:echo", "direction": "incoming"}),
            )
            .unwrap();
        assert!(!state.is_incoming_channel_registered("ferrite:echo"));

        // Re-register and verify shutdown_provider clears channels.
        state
            .handle_call(
                "plugin_message.register",
                &json!({"plugin": "Foo", "channel": "worldedit:cui", "direction": "both"}),
            )
            .unwrap();
        assert!(state.is_incoming_channel_registered("worldedit:cui"));
        state.shutdown_provider();
        assert!(!state.is_incoming_channel_registered("worldedit:cui"));
    }
}
