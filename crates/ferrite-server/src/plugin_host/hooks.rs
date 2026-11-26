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
    PlayerMove, PluginCommand, PluginCommandResult, PluginMessage, ServerHooks,
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
        Ok(self
            .host
            .commands()
            .into_iter()
            .map(|command| PluginCommand {
                name: command.name,
                aliases: command.aliases,
                permission: command.permission,
                usage: command.usage,
            })
            .collect())
    }

    fn run_plugin_command(
        &self,
        sender: CommandSender,
        line: &str,
    ) -> Result<PluginCommandResult, Error> {
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
