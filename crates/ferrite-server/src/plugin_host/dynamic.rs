//! `ServerHooks` adapter for drop-in C-ABI dynamic native plugins (`.so` / `.dll` / `.dylib`)
//! with live `/dynplugins list | load <path> | unload <name> | reload` hot-loading commands.

use std::path::{Path, PathBuf};
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
    /// Loads every valid dynamic native library in `dir` and keeps `dir` active for runtime
    /// hot-loading via `/dynplugins reload` or `/dynplugins load <path>`.
    pub fn for_dir(dir: &Path) -> Arc<Self> {
        Arc::new(Self {
            manager: Arc::new(DynamicPluginManager::load_dir(dir)),
        })
    }

    /// Loads every valid dynamic native library in `dir`, returning `None` if none are present.
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

    fn handle_dynplugins_command(&self, sender: CommandSender, line: &str) -> Option<String> {
        let trimmed = line.trim().trim_start_matches('/');
        let mut parts = trimmed.split_whitespace();
        let cmd = parts.next()?.to_ascii_lowercase();
        if !matches!(cmd.as_str(), "dynplugins" | "dp") {
            return None;
        }
        let _ = sender;
        let args: Vec<&str> = parts.collect();
        match args.as_slice() {
            [] | ["list"] => {
                let loaded = self.manager.loaded_plugins();
                if loaded.is_empty() {
                    Some("No dynamic native plugins loaded.".to_owned())
                } else {
                    let list: Vec<String> = loaded
                        .into_iter()
                        .map(|p| format!("{} v{}", p.name, p.version))
                        .collect();
                    Some(format!("Dynamic native plugins: {}", list.join(", ")))
                }
            }
            ["reload"] => {
                let summary = self.manager.reload_dir();
                Some(format!(
                    "Dynamic reload complete: loaded={}, reloaded={}, unloaded={}, failed={}",
                    summary.loaded.len(),
                    summary.reloaded.len(),
                    summary.unloaded.len(),
                    summary.failed.len()
                ))
            }
            ["load", path] => {
                let resolved = if Path::new(path).is_absolute() {
                    PathBuf::from(path)
                } else if let Some(dir) = self.manager.watch_dir() {
                    dir.join(path)
                } else {
                    PathBuf::from(path)
                };
                match self.manager.load_library(&resolved) {
                    Ok(meta) => Some(format!(
                        "Loaded dynamic plugin {} v{} from {}",
                        meta.name,
                        meta.version,
                        meta.path.display()
                    )),
                    Err(err) => Some(format!("Failed to load dynamic plugin: {err}")),
                }
            }
            ["unload", name] => {
                if self.manager.unload_by_name(name) {
                    Some(format!("Unloaded dynamic plugin '{name}'."))
                } else {
                    Some(format!("Dynamic plugin '{name}' is not loaded."))
                }
            }
            _ => Some("Usage: /dynplugins <list|reload|load <path>|unload <name>>".to_owned()),
        }
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
        Ok(vec![PluginCommand {
            name: "dynplugins".to_owned(),
            aliases: vec!["dp".to_owned()],
            permission: Some("ferrite.native.admin".to_owned()),
            usage: Some("/dynplugins <list|reload|load <path>|unload <name>>".to_owned()),
        }])
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
        if let Some(reply) = self.handle_dynplugins_command(sender, line) {
            return Ok(PluginCommandResult {
                handled: true,
                messages: vec![PluginMessage {
                    player: sender_uuid,
                    content: MessageContent::Plain(reply),
                }],
            });
        }
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
