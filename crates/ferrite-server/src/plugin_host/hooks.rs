//! Forwards the game loop's plugin hooks to the Java host.
//!
//! `player_join`/`player_quit` are synchronous because they come from the coordinator thread.
//! `block_break`/`player_interact` come from a region thread that owns the block; contract v1
//! does not let a region thread wait for Java. They are queued and dispatched on the next tick,
//! so a later cancellation cannot undo the action; the host logs it instead.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use ferrite_plugin_host::{CommandOrigin, MAX_SERVERBOUND_PLUGIN_MESSAGE_BYTES, PluginHost};
use serde_json::{Value, json};

use crate::game::plugin_types::{
    BlockBreak, BlockInteract, BlockPosition, CommandSender, Error, InteractAction, MessageContent,
    PlayerDeath, PlayerDeathOutcome, PlayerMove, PluginCommand, PluginCommandResult, PluginMessage,
    ServerHooks,
};

/// Upper bound on deferred block events, so a stalled tick loop cannot grow without limit.
const MAX_DEFERRED: usize = 4096;

struct DeferredEvent {
    name: &'static str,
    payload: Value,
}

/// `ServerHooks` backed by the embedded Java plugin host.
pub struct HostHooks {
    host: Arc<PluginHost>,
    deferred: Mutex<VecDeque<DeferredEvent>>,
}

impl HostHooks {
    /// Wraps a running host.
    pub fn new(host: Arc<PluginHost>) -> Self {
        Self {
            host,
            deferred: Mutex::new(VecDeque::new()),
        }
    }

    fn fire(&self, name: &'static str, payload: Value) {
        if let Err(error) = self.host.fire_event(name, &payload) {
            tracing::warn!(%error, event = name, "plugin event failed");
        }
    }

    fn defer(&self, name: &'static str, payload: Value) {
        let mut queue = self
            .deferred
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if queue.len() >= MAX_DEFERRED {
            queue.pop_front();
            tracing::warn!(
                event = name,
                "dropping a deferred plugin event because the queue is full"
            );
        }
        queue.push_back(DeferredEvent { name, payload });
    }
}

impl ServerHooks for HostHooks {
    fn player_joined(&self, id: uuid::Uuid, name: &str) -> Result<(), Error> {
        self.fire(
            "player_join",
            json!({"player_uuid": id.to_string(), "player_name": name}),
        );
        Ok(())
    }

    fn player_left(&self, id: uuid::Uuid, name: &str) -> Result<(), Error> {
        self.fire(
            "player_quit",
            json!({"player_uuid": id.to_string(), "player_name": name}),
        );
        Ok(())
    }

    fn chat(&self, id: uuid::Uuid, name: &str, message: &str) -> Result<Option<String>, Error> {
        if !self.host.has_event_listener("player_chat") {
            return Ok(Some(message.to_owned()));
        }
        match self.host.fire_event(
            "player_chat",
            &json!({
                "player_uuid": id.to_string(),
                "player_name": name,
                "message": message,
            }),
        ) {
            Ok(outcome) if outcome.cancelled => Ok(None),
            Ok(_) => Ok(Some(message.to_owned())),
            Err(error) => {
                tracing::warn!(%error, "plugin chat event failed; passing message through");
                Ok(Some(message.to_owned()))
            }
        }
    }

    fn player_moved(
        &self,
        id: uuid::Uuid,
        name: &str,
        movement: &PlayerMove,
    ) -> Result<bool, Error> {
        if !self.host.has_event_listener("player_move") {
            return Ok(true);
        }
        // Only cross the JVM boundary when a player crosses a block boundary so stationary/look
        // packets incur zero JNI overhead.
        let from_block = (
            movement.from[0].floor() as i32,
            movement.from[1].floor() as i32,
            movement.from[2].floor() as i32,
        );
        let to_block = (
            movement.to[0].floor() as i32,
            movement.to[1].floor() as i32,
            movement.to[2].floor() as i32,
        );
        if from_block != to_block {
            self.defer(
                "player_move",
                json!({
                    "player_uuid": id.to_string(),
                    "player_name": name,
                    "from_x": movement.from[0],
                    "from_y": movement.from[1],
                    "from_z": movement.from[2],
                    "to_x": movement.to[0],
                    "to_y": movement.to[1],
                    "to_z": movement.to[2],
                    "yaw": movement.to_yaw,
                    "pitch": movement.to_pitch,
                }),
            );
        }
        Ok(true)
    }

    fn block_placed(
        &self,
        id: uuid::Uuid,
        name: &str,
        block: BlockPosition,
        face: u8,
        placed: &str,
        replaced: &str,
    ) -> Result<bool, Error> {
        if !self.host.has_event_listener("block_place") {
            return Ok(true);
        }
        match self.host.fire_event(
            "block_place",
            &json!({
                "player_uuid": id.to_string(),
                "player_name": name,
                "x": block.x,
                "y": block.y,
                "z": block.z,
                "face": face,
                "placed": placed,
                "replaced": replaced,
            }),
        ) {
            Ok(outcome) => Ok(!outcome.cancelled),
            Err(error) => {
                tracing::warn!(%error, "plugin block_place event failed; allowing placement");
                Ok(true)
            }
        }
    }

    fn block_broken(
        &self,
        id: uuid::Uuid,
        name: &str,
        event: BlockBreak<'_>,
    ) -> Result<bool, Error> {
        if !self.host.has_event_listener("block_break") {
            return Ok(true);
        }
        self.defer(
            "block_break",
            json!({
                "player_uuid": id.to_string(),
                "player_name": name,
                "world": event.world,
                "x": event.block.x,
                "y": event.block.y,
                "z": event.block.z,
                "material": event.material,
            }),
        );
        // Deferred off the region thread so JVM latency never stalls region ticks.
        Ok(true)
    }

    fn player_interacted(
        &self,
        id: uuid::Uuid,
        name: &str,
        action: InteractAction,
        event: BlockInteract<'_>,
    ) -> Result<bool, Error> {
        if !self.host.has_event_listener("player_interact") {
            return Ok(true);
        }
        let action = match action {
            InteractAction::LeftClickBlock => "left_click_block",
            InteractAction::RightClickBlock => "right_click_block",
        };
        self.defer(
            "player_interact",
            json!({
                "player_uuid": id.to_string(),
                "player_name": name,
                "world": event.world,
                "x": event.block.x,
                "y": event.block.y,
                "z": event.block.z,
                "face": event.face,
                "material": event.material,
                "action": action,
            }),
        );
        Ok(true)
    }

    fn player_command(&self, id: uuid::Uuid, name: &str, message: &str) -> Result<bool, Error> {
        if !self.host.has_event_listener("player_command") {
            return Ok(true);
        }
        match self.host.fire_event(
            "player_command",
            &json!({
                "player_uuid": id.to_string(),
                "player_name": name,
                "message": message,
            }),
        ) {
            Ok(outcome) => Ok(!outcome.cancelled),
            Err(error) => {
                tracing::warn!(%error, "plugin command preprocess event failed; allowing command");
                Ok(true)
            }
        }
    }

    fn entity_damaged(
        &self,
        id: uuid::Uuid,
        name: &str,
        cause: &str,
        amount: f64,
    ) -> Result<f64, Error> {
        if amount <= 0.0 || !self.host.has_event_listener("entity_damage") {
            return Ok(amount);
        }
        match self.host.fire_event(
            "entity_damage",
            &json!({
                "entity_uuid": id.to_string(),
                "entity_name": name,
                "cause": cause,
                "amount": amount,
            }),
        ) {
            Ok(outcome) if outcome.cancelled => Ok(0.0),
            Ok(_) => Ok(amount),
            Err(error) => {
                tracing::warn!(%error, "plugin entity_damage event failed; preserving damage");
                Ok(amount)
            }
        }
    }

    fn player_respawned(
        &self,
        id: uuid::Uuid,
        name: &str,
        spawn: [f64; 3],
        yaw: f32,
        pitch: f32,
    ) -> Result<[f64; 3], Error> {
        if !self.host.has_event_listener("player_respawn") {
            return Ok(spawn);
        }
        self.defer(
            "player_respawn",
            json!({
                "player_uuid": id.to_string(),
                "player_name": name,
                "x": spawn[0],
                "y": spawn[1],
                "z": spawn[2],
                "yaw": yaw,
                "pitch": pitch,
            }),
        );
        Ok(spawn)
    }

    fn player_died(
        &self,
        id: uuid::Uuid,
        name: &str,
        cause: &str,
        death: &PlayerDeath,
    ) -> Result<PlayerDeathOutcome, Error> {
        let default = PlayerDeathOutcome {
            drops: death.drops.clone(),
            dropped_exp: death.dropped_exp,
            keep_inventory: death.keep_inventory,
        };
        if !self.host.has_event_listener("player_death") {
            return Ok(default);
        }
        let _ = self.host.fire_event(
            "player_death",
            &json!({
                "player_uuid": id.to_string(),
                "player_name": name,
                "cause": cause,
                "dropped_exp": death.dropped_exp,
                "keep_inventory": death.keep_inventory,
            }),
        );
        Ok(default)
    }

    fn inventory_clicked(
        &self,
        id: uuid::Uuid,
        name: &str,
        slot: i32,
        click: &str,
        action: &str,
    ) -> Result<bool, Error> {
        if !self.host.has_event_listener("inventory_click") {
            return Ok(true);
        }
        match self.host.fire_event(
            "inventory_click",
            &json!({
                "player_uuid": id.to_string(),
                "player_name": name,
                "slot": slot,
                "click": click,
                "action": action,
            }),
        ) {
            Ok(outcome) => Ok(!outcome.cancelled),
            Err(error) => {
                tracing::warn!(%error, "plugin inventory_click event failed; allowing click");
                Ok(true)
            }
        }
    }

    fn inventory_closed(&self, id: uuid::Uuid, name: &str) -> Result<(), Error> {
        if !self.host.has_event_listener("inventory_close") {
            return Ok(());
        }
        self.defer(
            "inventory_close",
            json!({
                "player_uuid": id.to_string(),
                "player_name": name,
            }),
        );
        Ok(())
    }

    fn chunk_loaded(&self, world: uuid::Uuid, x: i32, z: i32, new: bool) -> Result<(), Error> {
        if !self.host.has_event_listener("chunk_loaded") {
            return Ok(());
        }
        self.defer(
            "chunk_loaded",
            json!({"world": world.to_string(), "x": x, "z": z, "new": new}),
        );
        Ok(())
    }

    fn chunk_unloaded(&self, world: uuid::Uuid, x: i32, z: i32) -> Result<(), Error> {
        if !self.host.has_event_listener("chunk_unloaded") {
            return Ok(());
        }
        self.defer(
            "chunk_unloaded",
            json!({"world": world.to_string(), "x": x, "z": z}),
        );
        Ok(())
    }

    fn world_loaded(&self, world: uuid::Uuid) -> Result<(), Error> {
        if !self.host.has_event_listener("world_loaded") {
            return Ok(());
        }
        self.defer("world_loaded", json!({"world": world.to_string()}));
        Ok(())
    }

    fn blocks_exploded(
        &self,
        world: uuid::Uuid,
        center: BlockPosition,
        blocks: &[BlockPosition],
        yield_rate: f32,
    ) -> Result<Option<Vec<BlockPosition>>, Error> {
        if !self.host.has_event_listener("blocks_exploded") {
            return Ok(None);
        }
        match self.host.fire_event(
            "blocks_exploded",
            &json!({
                "world": world.to_string(),
                "x": center.x,
                "y": center.y,
                "z": center.z,
                "count": blocks.len(),
                "yield": yield_rate,
            }),
        ) {
            Ok(outcome) if outcome.cancelled => Ok(Some(Vec::new())),
            Ok(_) => Ok(None),
            Err(error) => {
                tracing::warn!(%error, "plugin blocks_exploded event failed");
                Ok(None)
            }
        }
    }

    fn weather_changing(&self, world: uuid::Uuid, to_weather_state: bool) -> Result<bool, Error> {
        if !self.host.has_event_listener("weather_changing") {
            return Ok(true);
        }
        match self.host.fire_event(
            "weather_changing",
            &json!({
                "world": world.to_string(),
                "to_weather_state": to_weather_state,
            }),
        ) {
            Ok(outcome) => Ok(!outcome.cancelled),
            Err(error) => {
                tracing::warn!(%error, "plugin weather_changing event failed");
                Ok(true)
            }
        }
    }

    fn player_level_changed(
        &self,
        id: uuid::Uuid,
        name: &str,
        old_level: i32,
        new_level: i32,
    ) -> Result<i32, Error> {
        if !self.host.has_event_listener("player_level_changed") {
            return Ok(new_level);
        }
        self.defer(
            "player_level_changed",
            json!({
                "player_uuid": id.to_string(),
                "player_name": name,
                "old_level": old_level,
                "new_level": new_level,
            }),
        );
        Ok(new_level)
    }

    fn player_exp_changed(&self, id: uuid::Uuid, name: &str, amount: i32) -> Result<i32, Error> {
        if !self.host.has_event_listener("player_exp_changed") {
            return Ok(amount);
        }
        self.defer(
            "player_exp_changed",
            json!({
                "player_uuid": id.to_string(),
                "player_name": name,
                "amount": amount,
            }),
        );
        Ok(amount)
    }

    fn entity_died(
        &self,
        id: uuid::Uuid,
        kind: &str,
        position: [f64; 3],
        dropped_exp: i32,
    ) -> Result<(), Error> {
        if !self.host.has_event_listener("entity_died") {
            return Ok(());
        }
        self.defer(
            "entity_died",
            json!({
                "entity_uuid": id.to_string(),
                "kind": kind,
                "x": position[0],
                "y": position[1],
                "z": position[2],
                "dropped_exp": dropped_exp,
            }),
        );
        Ok(())
    }

    fn resource_pack_status(
        &self,
        player: uuid::Uuid,
        name: &str,
        pack: uuid::Uuid,
        action: i32,
    ) -> Result<(), Error> {
        if !self.host.has_event_listener("resource_pack_status") {
            return Ok(());
        }
        self.defer(
            "resource_pack_status",
            json!({
                "player_uuid": player.to_string(),
                "player_name": name,
                "pack_uuid": pack.to_string(),
                "action": action,
            }),
        );
        Ok(())
    }

    fn plugin_message_received(
        &self,
        id: uuid::Uuid,
        name: &str,
        channel: &str,
        data: &[u8],
    ) -> Result<bool, Error> {
        if data.len() > MAX_SERVERBOUND_PLUGIN_MESSAGE_BYTES {
            tracing::warn!(
                player = %id,
                channel,
                len = data.len(),
                max = MAX_SERVERBOUND_PLUGIN_MESSAGE_BYTES,
                "rejecting oversized inbound plugin message"
            );
            return Ok(false);
        }
        if !self.host.is_incoming_channel_registered(channel) {
            tracing::debug!(
                player = %id,
                channel,
                "rejecting inbound plugin message for unregistered channel"
            );
            return Ok(false);
        }
        let data_base64 = BASE64.encode(data);
        self.defer(
            "plugin_message",
            json!({
                "player_uuid": id.to_string(),
                "player_name": name,
                "channel": channel,
                "data_base64": data_base64,
            }),
        );
        Ok(true)
    }

    fn tick(&self) -> Result<(), Error> {
        let events: Vec<DeferredEvent> = {
            let mut queue = self
                .deferred
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            queue.drain(..).collect()
        };
        for event in events {
            match self.host.fire_event(event.name, &event.payload) {
                Ok(outcome) if outcome.cancelled => tracing::warn!(
                    event = event.name,
                    "a deferred plugin event was cancelled after the action was applied"
                ),
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(%error, event = event.name, "deferred plugin event failed");
                }
            }
        }
        self.host
            .tick()
            .map_err(|error| Error::InvalidEvent(error.to_string()))
    }

    fn plugin_commands(&self) -> Result<Vec<PluginCommand>, Error> {
        let mut commands: Vec<PluginCommand> = self
            .host
            .commands()
            .into_iter()
            .map(|command| PluginCommand {
                name: command.name,
                aliases: command.aliases,
                permission: command.permission,
                usage: command.usage,
            })
            .collect();
        commands.push(PluginCommand {
            name: "ferriteplugins".to_owned(),
            aliases: vec!["fp".to_owned()],
            permission: Some("ferrite.plugins.admin".to_owned()),
            usage: Some("/ferriteplugins <list|load <jar>|unload <name>|reload <name>>".to_owned()),
        });
        Ok(commands)
    }

    fn run_plugin_command(
        &self,
        sender: CommandSender,
        line: &str,
    ) -> Result<PluginCommandResult, Error> {
        let trimmed = line.trim().trim_start_matches('/');
        let mut parts = trimmed.split_whitespace();
        let cmd_name = parts.next().unwrap_or("").to_ascii_lowercase();
        if cmd_name == "ferriteplugins" || cmd_name == "fp" {
            let reply_player = match sender {
                CommandSender::Console => uuid::Uuid::nil(),
                CommandSender::Player(uuid) => uuid,
            };
            let sub = parts.next().unwrap_or("list").to_ascii_lowercase();
            let text = match sub.as_str() {
                "list" => {
                    let loaded = self.host.loaded();
                    if loaded.is_empty() {
                        "No JVM plugins loaded.".to_owned()
                    } else {
                        let summary: Vec<String> = loaded
                            .iter()
                            .map(|p| format!("{} v{}", p.name, p.version))
                            .collect();
                        format!("JVM plugins ({}): {}", loaded.len(), summary.join(", "))
                    }
                }
                "load" => {
                    let rest: Vec<&str> = parts.collect();
                    let raw_path = rest.join(" ");
                    if raw_path.is_empty() {
                        "Usage: /ferriteplugins load <jar-path-or-filename>".to_owned()
                    } else {
                        let candidate = std::path::PathBuf::from(&raw_path);
                        let resolved = if candidate.is_absolute() || candidate.exists() {
                            candidate
                        } else {
                            self.host.plugins_dir().join(&raw_path)
                        };
                        match self.host.load_jar_at_runtime(&resolved) {
                            Ok(info) => format!(
                                "Hot-loaded JVM plugin '{}' v{} from {}",
                                info.name,
                                info.version,
                                info.jar.display()
                            ),
                            Err(err) => format!("Failed to hot-load JVM plugin: {err}"),
                        }
                    }
                }
                "unload" => {
                    let Some(name) = parts.next() else {
                        return Ok(PluginCommandResult {
                            handled: true,
                            messages: vec![PluginMessage {
                                player: reply_player,
                                content: MessageContent::Plain(
                                    "Usage: /ferriteplugins unload <name>".to_owned(),
                                ),
                            }],
                        });
                    };
                    match self.host.unload_plugin(name) {
                        Ok(true) => format!("Hot-unloaded JVM plugin '{name}'."),
                        Ok(false) => format!("JVM plugin '{name}' is not loaded."),
                        Err(err) => format!("Failed to unload JVM plugin '{name}': {err}"),
                    }
                }
                "reload" => {
                    let Some(name) = parts.next() else {
                        return Ok(PluginCommandResult {
                            handled: true,
                            messages: vec![PluginMessage {
                                player: reply_player,
                                content: MessageContent::Plain(
                                    "Usage: /ferriteplugins reload <name>".to_owned(),
                                ),
                            }],
                        });
                    };
                    match self.host.reload_plugin(name) {
                        Ok(info) => {
                            format!("Hot-reloaded JVM plugin '{}' v{}.", info.name, info.version)
                        }
                        Err(err) => format!("Failed to reload JVM plugin '{name}': {err}"),
                    }
                }
                _ => "Usage: /ferriteplugins <list|load <jar>|unload <name>|reload <name>>"
                    .to_owned(),
            };
            return Ok(PluginCommandResult {
                handled: true,
                messages: vec![PluginMessage {
                    player: reply_player,
                    content: MessageContent::Plain(text),
                }],
            });
        }
        let origin = match sender {
            CommandSender::Console => CommandOrigin::Console,
            CommandSender::Player(uuid) => CommandOrigin::Player {
                uuid: uuid.to_string(),
                name: String::new(),
            },
        };
        let outcome = self
            .host
            .run_command(origin, line)
            .map_err(|error| Error::InvalidEvent(error.to_string()))?;
        let messages = outcome
            .messages
            .into_iter()
            .map(|message| {
                // Console messages have no player UUID. The game's console path logs every
                // message without matching the UUID; a nil UUID keeps them in the result.
                let player = message
                    .player
                    .as_deref()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(uuid::Uuid::nil());
                let content = if message.adventure {
                    MessageContent::AdventureJson(message.content)
                } else {
                    MessageContent::Plain(message.content)
                };
                PluginMessage { player, content }
            })
            .collect();
        Ok(PluginCommandResult {
            handled: outcome.handled,
            messages,
        })
    }
}
