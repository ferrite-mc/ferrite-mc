//! Forwards the game loop's plugin hooks to the Java host.
//!
//! `player_join`/`player_quit` are synchronous because they come from the coordinator thread.
//! `block_break`/`player_interact` come from a region thread that owns the block; contract v1
//! does not let a region thread wait for Java. They are queued and dispatched on the next tick,
//! so a later cancellation cannot undo the action; the host logs it instead.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use ferrite_plugin_host::{CommandOrigin, PluginHost};
use serde_json::{Value, json};

use crate::game::plugin_types::{
    BlockPosition, CommandSender, Error, InteractAction, MessageContent, PlayerMove, PluginCommand,
    PluginCommandResult, PluginMessage, ServerHooks,
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

    fn chat(&self, _id: uuid::Uuid, _name: &str, message: &str) -> Result<Option<String>, Error> {
        // Contract v1 has no chat event; pass the message through unchanged.
        Ok(Some(message.to_owned()))
    }

    fn player_moved(
        &self,
        _id: uuid::Uuid,
        _name: &str,
        _movement: &PlayerMove,
    ) -> Result<bool, Error> {
        // Contract v1 has no move event; movement is allowed.
        Ok(true)
    }

    fn block_placed(
        &self,
        _id: uuid::Uuid,
        _name: &str,
        _block: BlockPosition,
        _face: u8,
        _placed: &str,
        _replaced: &str,
    ) -> Result<bool, Error> {
        // Contract v1 has no place event; the change is allowed.
        Ok(true)
    }

    fn block_broken(
        &self,
        id: uuid::Uuid,
        name: &str,
        block: BlockPosition,
        material: &str,
    ) -> Result<bool, Error> {
        self.defer(
            "block_break",
            json!({
                "player_uuid": id.to_string(),
                "player_name": name,
                "world": "minecraft:overworld",
                "x": block.x,
                "y": block.y,
                "z": block.z,
                "material": material,
            }),
        );
        // The action has already been decided; contract v1 cannot cancel it from here.
        Ok(true)
    }

    fn player_interacted(
        &self,
        id: uuid::Uuid,
        name: &str,
        action: InteractAction,
        block: BlockPosition,
        face: u8,
        material: &str,
    ) -> Result<bool, Error> {
        let action = match action {
            InteractAction::LeftClickBlock => "left_click_block",
            InteractAction::RightClickBlock => "right_click_block",
        };
        self.defer(
            "player_interact",
            json!({
                "player_uuid": id.to_string(),
                "player_name": name,
                "world": "minecraft:overworld",
                "x": block.x,
                "y": block.y,
                "z": block.z,
                "face": face,
                "material": material,
                "action": action,
            }),
        );
        Ok(true)
    }

    fn player_command(&self, _id: uuid::Uuid, _name: &str, _message: &str) -> Result<bool, Error> {
        // Commands declared by plugins are dispatched through plugin_commands and
        // run_plugin_command; there is no generic preprocess event in contract v1.
        Ok(false)
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
