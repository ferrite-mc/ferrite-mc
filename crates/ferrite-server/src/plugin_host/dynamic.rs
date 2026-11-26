//! `ServerHooks` adapter for drop-in C-ABI dynamic native plugins (`.so` / `.dll` / `.dylib`).

use std::path::Path;
use std::sync::Arc;

use ferrite_dynamic_plugins::DynamicPluginManager;
use uuid::Uuid;

use crate::game::plugin_types::{
    BlockBreak, BlockInteract, BlockPosition, CommandSender, Error, InteractAction, MessageContent,
    PlayerMove, PluginCommand, PluginCommandResult, PluginMessage, ServerHooks,
};

/// `ServerHooks` layer backed by [`DynamicPluginManager`].
pub struct DynamicPluginHooks {
    manager: Arc<DynamicPluginManager>,
}

impl DynamicPluginHooks {
    /// Loads every valid dynamic native library in `dir`. Returns `None` if no libraries were loaded.
    pub fn load_dir_if_nonempty(dir: &Path) -> Option<Arc<Self>> {
        let manager = DynamicPluginManager::load_dir(dir);
        if manager.is_empty() {
            None
        } else {
            Some(Arc::new(Self {
                manager: Arc::new(manager),
            }))
        }
    }

    /// Shuts down all loaded dynamic plugins cleanly.
    pub fn shutdown(&self) {
        self.manager.shutdown();
    }
}

impl ServerHooks for DynamicPluginHooks {
    fn player_joined(&self, id: Uuid, name: &str) -> Result<(), Error> {
        self.manager.dispatch_player_join(id, name);
        Ok(())
    }

    fn player_left(&self, id: Uuid, name: &str) -> Result<(), Error> {
        self.manager.dispatch_player_quit(id, name);
        Ok(())
    }

    fn chat(&self, id: Uuid, _name: &str, message: &str) -> Result<Option<String>, Error> {
        if self.manager.dispatch_chat(id, message) {
            Ok(Some(message.to_owned()))
        } else {
            Ok(None)
        }
    }

    fn player_moved(&self, _id: Uuid, _name: &str, _movement: &PlayerMove) -> Result<bool, Error> {
        Ok(true)
    }

    fn player_interacted(
        &self,
        _id: Uuid,
        _name: &str,
        _action: InteractAction,
        _event: BlockInteract<'_>,
    ) -> Result<bool, Error> {
        Ok(true)
    }

    fn player_command(&self, _id: Uuid, _name: &str, _message: &str) -> Result<bool, Error> {
        Ok(true)
    }

    fn plugin_commands(&self) -> Result<Vec<PluginCommand>, Error> {
        Ok(Vec::new())
    }

    fn block_broken(&self, id: Uuid, _name: &str, event: BlockBreak<'_>) -> Result<bool, Error> {
        Ok(self
            .manager
            .dispatch_block_break(id, event.block.x, event.block.y, event.block.z))
    }

    fn block_placed(
        &self,
        id: Uuid,
        _name: &str,
        block: BlockPosition,
        _face: u8,
        _placed: &str,
        _replaced: &str,
    ) -> Result<bool, Error> {
        Ok(self
            .manager
            .dispatch_block_place(id, block.x, block.y, block.z))
    }

    fn run_plugin_command(
        &self,
        sender: CommandSender,
        line: &str,
    ) -> Result<PluginCommandResult, Error> {
        let sender_uuid = match sender {
            CommandSender::Console => Uuid::nil(),
            CommandSender::Player(id) => id,
        };
        let Some(reply) = self.manager.dispatch_command(sender_uuid, line) else {
            return Ok(PluginCommandResult {
                handled: false,
                messages: Vec::new(),
            });
        };
        Ok(PluginCommandResult {
            handled: true,
            messages: vec![PluginMessage {
                player: sender_uuid,
                content: MessageContent::Plain(reply),
            }],
        })
    }
}
