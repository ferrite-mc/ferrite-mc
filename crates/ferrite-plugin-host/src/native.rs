//! The Java→Rust native bridge and the v1/v2 RPC handlers.

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
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
    BlockGetRequest, BlockSetRequest, EntityEquipmentSetRequest, HostStateProvider,
    MAX_PLUGIN_CHANNEL_LEN, MAX_PLUGIN_MESSAGE_BYTES, PlayerInventorySetRequest, UuidRequest,
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
            stopped: AtomicBool::new(false),
        }
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
pub(crate) fn native_methods() -> [NativeMethod; 2] {
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
    ]
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
