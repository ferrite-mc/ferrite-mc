//! `ServerHooks` adapter for drop-in C-ABI dynamic native plugins (`.so` / `.dll` / `.dylib`)
//! with live `/dynplugins list | load <path> | unload <name> | reload` hot-loading commands.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use ferrite_dynamic_plugins::{DynamicPluginManager, ReloadSummary};
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

use crate::game::plugin_types::{
    BlockBreak, BlockInteract, BlockPosition, CommandSender, Error, InteractAction, MessageContent,
    PlayerCommand, PlayerMove, PluginCommand, PluginCommandResult, PluginMessage, ServerHooks,
};

/// `ServerHooks` layer backed by [`DynamicPluginManager`].
pub struct DynamicPluginHooks {
    manager: Arc<DynamicPluginManager>,
    command_tx: Mutex<Option<UnboundedSender<PlayerCommand>>>,
    online_players: RwLock<HashSet<Uuid>>,
}

impl DynamicPluginHooks {
    /// Loads every valid dynamic native library in `dir` and keeps `dir` active for runtime
    /// hot-loading via `/dynplugins reload` or `/dynplugins load <path>`.
    pub fn for_dir(dir: &Path) -> Arc<Self> {
        Arc::new(Self {
            manager: Arc::new(DynamicPluginManager::load_dir(dir)),
            command_tx: Mutex::new(None),
            online_players: RwLock::new(HashSet::new()),
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
                command_tx: Mutex::new(None),
                online_players: RwLock::new(HashSet::new()),
            }))
        }
    }

    /// Attaches the game-loop `PlayerCommand` sender used to refresh client command trees after
    /// live hot-load / hot-unload / hot-reload events.
    pub fn set_command_sender(&self, tx: UnboundedSender<PlayerCommand>) {
        *self.command_tx.lock().unwrap_or_else(|p| p.into_inner()) = Some(tx);
    }

    /// Checks the watch directory mtime table and automatically hot-reloads changed libraries.
    pub fn auto_reload_if_changed(&self) -> ReloadSummary {
        if !self.manager.has_changes_in_watch_dir() {
            return ReloadSummary::default();
        }
        let summary = self.manager.reload_dir();
        if !summary.loaded.is_empty()
            || !summary.reloaded.is_empty()
            || !summary.unloaded.is_empty()
        {
            self.broadcast_update_commands();
        }
        summary
    }

    fn broadcast_update_commands(&self) {
        let Some(tx) = self
            .command_tx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        else {
            return;
        };
        let players: Vec<Uuid> = self
            .online_players
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .copied()
            .collect();
        for player in players {
            let _ = tx.send(PlayerCommand::UpdateCommands { player });
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
                self.broadcast_update_commands();
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
                    Ok(meta) => {
                        self.broadcast_update_commands();
                        Some(format!(
                            "Loaded dynamic plugin {} v{} from {}",
                            meta.name,
                            meta.version,
                            meta.path.display()
                        ))
                    }
                    Err(err) => Some(format!("Failed to load dynamic plugin: {err}")),
                }
            }
            ["unload", name] => {
                if self.manager.unload_by_name(name) {
                    self.broadcast_update_commands();
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
        self.online_players
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id);
        self.manager.dispatch_player_join(id, name);
        Ok(())
    }

    fn player_left(&self, id: Uuid, name: &str) -> Result<(), Error> {
        self.online_players
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&id);
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

    fn player_moved(&self, id: Uuid, _name: &str, movement: &PlayerMove) -> Result<bool, Error> {
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
        if from_block == to_block {
            return Ok(true);
        }
        Ok(self
            .manager
            .dispatch_player_move(id, movement.from, movement.to))
    }

    fn player_interacted(
        &self,
        id: Uuid,
        _name: &str,
        action: InteractAction,
        event: BlockInteract<'_>,
    ) -> Result<bool, Error> {
        let action_code = match action {
            InteractAction::LeftClickBlock => 0,
            InteractAction::RightClickBlock => 1,
        };
        Ok(self.manager.dispatch_player_interact(
            id,
            action_code,
            event.block.x,
            event.block.y,
            event.block.z,
        ))
    }

    fn entity_damaged(
        &self,
        id: Uuid,
        _name: &str,
        _cause: &str,
        amount: f64,
    ) -> Result<f64, Error> {
        Ok(self.manager.dispatch_entity_damage(id, amount))
    }

    fn tick(&self) -> Result<(), Error> {
        self.manager.dispatch_tick();
        Ok(())
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
