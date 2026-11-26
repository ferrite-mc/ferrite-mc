//! `ServerHooks` adapter for drop-in C-ABI dynamic native plugins (`.so` / `.dll` / `.dylib`)
//! with live `/dynplugins list | load <path> | unload <name> | reload` hot-loading commands.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use ferrite_dynamic_plugins::{DynamicHostBridge, DynamicPluginManager, ReloadSummary};
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

use crate::game::plugin_types::{
    BlockBreak, BlockInteract, BlockPosition, CommandSender, Error, InteractAction, MessageContent,
    PlayerCommand, PlayerLocation, PlayerMove, PluginCommand, PluginCommandResult, PluginMessage,
    ServerHooks,
};
use crate::game::{Dimension, DimensionRegistry, WorldId};

/// `ServerHooks` layer backed by [`DynamicPluginManager`].
pub struct DynamicPluginHooks {
    manager: Arc<DynamicPluginManager>,
    command_tx: Mutex<Option<UnboundedSender<PlayerCommand>>>,
    dimensions: Mutex<Option<Arc<DimensionRegistry>>>,
    online_players: RwLock<HashSet<Uuid>>,
    op_players: RwLock<HashSet<Uuid>>,
}

impl DynamicPluginHooks {
    /// Loads every valid dynamic native library in `dir` and keeps `dir` active for runtime
    /// hot-loading via `/dynplugins reload` or `/dynplugins load <path>`.
    pub fn for_dir(dir: &Path) -> Arc<Self> {
        let hooks = Arc::new(Self {
            manager: Arc::new(DynamicPluginManager::load_dir(dir)),
            command_tx: Mutex::new(None),
            dimensions: Mutex::new(None),
            online_players: RwLock::new(HashSet::new()),
            op_players: RwLock::new(HashSet::new()),
        });
        ferrite_dynamic_plugins::install_host_bridge(
            Arc::clone(&hooks) as Arc<dyn DynamicHostBridge>
        );
        hooks
    }

    /// Loads every valid dynamic native library in `dir`, returning `None` if none are present.
    pub fn load_dir_if_nonempty(dir: &Path) -> Option<Arc<Self>> {
        let manager = DynamicPluginManager::load_dir(dir);
        if manager.is_empty() {
            None
        } else {
            let hooks = Arc::new(Self {
                manager: Arc::new(manager),
                command_tx: Mutex::new(None),
                dimensions: Mutex::new(None),
                online_players: RwLock::new(HashSet::new()),
                op_players: RwLock::new(HashSet::new()),
            });
            ferrite_dynamic_plugins::install_host_bridge(
                Arc::clone(&hooks) as Arc<dyn DynamicHostBridge>
            );
            Some(hooks)
        }
    }

    /// Attaches the game-loop `PlayerCommand` sender used to refresh client command trees after
    /// live hot-load / hot-unload / hot-reload events and to execute C-ABI v2 host callbacks.
    pub fn set_command_sender(&self, tx: UnboundedSender<PlayerCommand>) {
        *self.command_tx.lock().unwrap_or_else(|p| p.into_inner()) = Some(tx);
    }

    /// Attaches the dimension registry so C-ABI v2 dynamic plugins can read/write live blocks.
    pub fn set_dimensions(&self, dimensions: Arc<DimensionRegistry>) {
        *self.dimensions.lock().unwrap_or_else(|p| p.into_inner()) = Some(dimensions);
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

    /// Updates whether `player` holds operator permissions for C-ABI v2 `has_permission` checks.
    pub fn set_player_op(&self, player: Uuid, is_op: bool) {
        let mut ops = self.op_players.write().unwrap_or_else(|p| p.into_inner());
        if is_op {
            ops.insert(player);
        } else {
            ops.remove(&player);
        }
    }

    /// Shuts down all loaded dynamic plugins cleanly.
    pub fn shutdown(&self) {
        self.manager.shutdown();
    }

    /// Returns names and versions of all currently loaded dynamic native plugins.
    pub fn loaded_plugin_summaries(&self) -> Vec<String> {
        self.manager
            .loaded_plugins()
            .into_iter()
            .map(|p| format!("{} v{}", p.name, p.version))
            .collect()
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
            ["info", name] => match self.manager.plugin_info(name) {
                Some(info) => Some(format!(
                    "{} v{} (ABI {}) — path: {}, hooks: [{}]",
                    info.name,
                    info.version,
                    if info.abi_v2 { "v2" } else { "v1" },
                    info.path.display(),
                    info.hooks.join(", ")
                )),
                None => Some(format!("Dynamic plugin '{name}' is not loaded.")),
            },
            _ => Some(
                "Usage: /dynplugins <list|info <name>|reload|load <path>|unload <name>>".to_owned(),
            ),
        }
    }
}

impl DynamicHostBridge for DynamicPluginHooks {
    fn send_message(&self, player: Uuid, message: &str) {
        if let Some(tx) = self
            .command_tx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            let _ = tx.send(PlayerCommand::Message {
                player,
                content: MessageContent::Plain(message.to_owned()),
            });
        }
    }

    fn broadcast(&self, message: &str) {
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
            let _ = tx.send(PlayerCommand::Message {
                player,
                content: MessageContent::Plain(message.to_owned()),
            });
        }
    }

    fn send_action_bar(&self, player: Uuid, message: &str) {
        if let Some(tx) = self
            .command_tx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            let json = serde_json::json!({ "text": message }).to_string();
            let _ = tx.send(PlayerCommand::ActionBar {
                player,
                component_json: json,
            });
        }
    }

    fn teleport(&self, player: Uuid, world: &str, x: f64, y: f64, z: f64, yaw: f32, pitch: f32) {
        if let Some(tx) = self
            .command_tx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            let world_id = Dimension::from_key(world)
                .map(|dim| crate::game::plugin_world_id(WorldId::Vanilla(dim)))
                .unwrap_or_else(|| {
                    Uuid::parse_str(world)
                        .unwrap_or_else(|_| crate::game::plugin_types::default_world_id())
                });
            let _ = tx.send(PlayerCommand::Teleport {
                player,
                location: PlayerLocation {
                    world: world_id,
                    x,
                    y,
                    z,
                    yaw,
                    pitch,
                },
            });
        }
    }

    fn get_block(&self, world: &str, x: i32, y: i32, z: i32) -> Option<u32> {
        let dims = self
            .dimensions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()?;
        let dim = Dimension::from_key(world).unwrap_or(Dimension::Overworld);
        dims.source(dim).block(x, y, z)
    }

    fn set_block(&self, world: &str, x: i32, y: i32, z: i32, state_id: u32) -> bool {
        let Some(dims) = self
            .dimensions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        else {
            return false;
        };
        let dim = Dimension::from_key(world).unwrap_or(Dimension::Overworld);
        let source = dims.source(dim);
        if source.block(x, y, z).is_none() {
            return false;
        }
        source.set_block(x, y, z, state_id).is_ok()
    }

    fn send_title(
        &self,
        player: Uuid,
        title: &str,
        subtitle: &str,
        fade_in: i32,
        stay: i32,
        fade_out: i32,
    ) {
        if let Some(tx) = self
            .command_tx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            let _ = tx.send(PlayerCommand::Title {
                player,
                title: (!title.is_empty())
                    .then(|| serde_json::json!({ "text": title }).to_string()),
                subtitle: (!subtitle.is_empty())
                    .then(|| serde_json::json!({ "text": subtitle }).to_string()),
                fade_in,
                stay,
                fade_out,
            });
        }
    }

    fn play_sound(
        &self,
        player: Uuid,
        sound: &str,
        x: f64,
        y: f64,
        z: f64,
        volume: f32,
        pitch: f32,
    ) {
        if let Some(tx) = self
            .command_tx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            let _ = tx.send(PlayerCommand::PlaySound {
                player,
                world: crate::game::plugin_types::default_world_id(),
                sound: sound.to_owned(),
                category: crate::game::plugin_types::SoundCategory::Master,
                x,
                y,
                z,
                volume,
                pitch,
                seed: None,
            });
        }
    }

    fn kick_player(&self, player: Uuid, reason: &str) {
        if let Some(tx) = self
            .command_tx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            let _ = tx.send(PlayerCommand::Kick {
                player,
                component_json: Some(serde_json::json!({ "text": reason }).to_string()),
            });
        }
    }

    fn set_gamemode(&self, player: Uuid, mode: u8) {
        if let Some(tx) = self
            .command_tx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            let gm = match mode {
                1 => crate::game::plugin_types::GameMode::Creative,
                2 => crate::game::plugin_types::GameMode::Adventure,
                3 => crate::game::plugin_types::GameMode::Spectator,
                _ => crate::game::plugin_types::GameMode::Survival,
            };
            let _ = tx.send(PlayerCommand::SetGameMode { player, mode: gm });
        }
    }

    fn perform_command(&self, player: Uuid, command: &str) {
        if let Some(tx) = self
            .command_tx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            let cleaned = command.trim().trim_start_matches('/').to_owned();
            if !cleaned.is_empty() {
                let _ = tx.send(PlayerCommand::Chat {
                    player,
                    message: cleaned,
                });
            }
        }
    }

    fn has_permission(&self, player: Uuid, node: &str) -> bool {
        if self
            .op_players
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .contains(&player)
        {
            return true;
        }
        let lower = node.to_ascii_lowercase();
        !(lower.starts_with("admin.")
            || lower.starts_with("op.")
            || lower.starts_with("ferrite.admin")
            || lower.contains(".admin"))
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

    fn player_died(
        &self,
        id: Uuid,
        _name: &str,
        _cause: &str,
        death: &crate::game::plugin_types::PlayerDeath,
    ) -> Result<crate::game::plugin_types::PlayerDeathOutcome, Error> {
        let keep = self.manager.dispatch_player_death(id) || death.keep_inventory;
        Ok(crate::game::plugin_types::PlayerDeathOutcome {
            drops: if keep {
                Vec::new()
            } else {
                death.drops.clone()
            },
            dropped_exp: death.dropped_exp,
            keep_inventory: keep,
        })
    }

    fn player_respawned(
        &self,
        id: Uuid,
        _name: &str,
        spawn: [f64; 3],
        _yaw: f32,
        _pitch: f32,
    ) -> Result<[f64; 3], Error> {
        self.manager.dispatch_player_respawn(id, spawn);
        Ok(spawn)
    }

    fn inventory_clicked(
        &self,
        id: Uuid,
        _name: &str,
        slot: i32,
        _click: &str,
        _action: &str,
    ) -> Result<bool, Error> {
        Ok(self.manager.dispatch_inventory_click(id, slot))
    }

    fn player_command(&self, _id: Uuid, _name: &str, _message: &str) -> Result<bool, Error> {
        Ok(true)
    }

    fn plugin_commands(&self) -> Result<Vec<PluginCommand>, Error> {
        Ok(vec![PluginCommand {
            name: "dynplugins".to_owned(),
            aliases: vec!["dp".to_owned()],
            permission: Some("ferrite.native.admin".to_owned()),
            usage: Some(
                "/dynplugins <list|info <name>|reload|load <path>|unload <name>>".to_owned(),
            ),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn build_perm_cmd_class() -> Vec<u8> {
        struct Cp(Vec<Vec<u8>>);
        impl Cp {
            fn push(&mut self, entry: Vec<u8>) -> u16 {
                self.0.push(entry);
                self.0.len() as u16
            }
            fn utf8(&mut self, s: &str) -> u16 {
                let mut e = vec![1u8];
                e.extend_from_slice(&(s.len() as u16).to_be_bytes());
                e.extend_from_slice(s.as_bytes());
                self.push(e)
            }
            fn str(&mut self, u: u16) -> u16 {
                let mut e = vec![8u8];
                e.extend_from_slice(&u.to_be_bytes());
                self.push(e)
            }
            fn cls(&mut self, u: u16) -> u16 {
                let mut e = vec![7u8];
                e.extend_from_slice(&u.to_be_bytes());
                self.push(e)
            }
            fn nat(&mut self, n: u16, d: u16) -> u16 {
                let mut e = vec![12u8];
                e.extend_from_slice(&n.to_be_bytes());
                e.extend_from_slice(&d.to_be_bytes());
                self.push(e)
            }
            fn mref(&mut self, c: u16, nat: u16) -> u16 {
                let mut e = vec![10u8];
                e.extend_from_slice(&c.to_be_bytes());
                e.extend_from_slice(&nat.to_be_bytes());
                self.push(e)
            }
        }
        let mut cp = Cp(Vec::new());

        let this_u = cp.utf8("com/example/VipPlugin");
        let super_u = cp.utf8("org/bukkit/plugin/java/JavaPlugin");
        let this_c = cp.cls(this_u);
        let super_c = cp.cls(super_u);
        let code_u = cp.utf8("Code");
        let on_cmd_u = cp.utf8("onCommand");
        let on_cmd_d = cp.utf8(
            "(Lorg/bukkit/command/CommandSender;Lorg/bukkit/command/Command;Ljava/lang/String;[Ljava/lang/String;)Z",
        );
        let perm_u = cp.utf8("admin.vip");
        let perm_s = cp.str(perm_u);
        let msg_u = cp.utf8("VIP Welcome!");
        let msg_s = cp.str(msg_u);
        let has_perm_u = cp.utf8("hasPermission");
        let has_perm_d = cp.utf8("(Ljava/lang/String;)Z");
        let send_u = cp.utf8("sendMessage");
        let send_d = cp.utf8("(Ljava/lang/String;)V");
        let hp_nat = cp.nat(has_perm_u, has_perm_d);
        let has_perm_m = cp.mref(super_c, hp_nat);
        let s_nat = cp.nat(send_u, send_d);
        let send_m = cp.mref(super_c, s_nat);

        let mut bc = Vec::new();
        // ldc "admin.vip"; invokevirtual hasPermission; ldc "VIP Welcome!"; invokevirtual sendMessage; return
        bc.extend_from_slice(&[0x12, perm_s as u8, 0xb6]);
        bc.extend_from_slice(&has_perm_m.to_be_bytes());
        bc.extend_from_slice(&[0x12, msg_s as u8, 0xb6]);
        bc.extend_from_slice(&send_m.to_be_bytes());
        bc.push(0xb1);

        let mut out = Vec::new();
        out.extend_from_slice(&0xCAFE_BABEu32.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&65u16.to_be_bytes());
        out.extend_from_slice(&((cp.0.len() + 1) as u16).to_be_bytes());
        for entry in cp.0 {
            out.extend_from_slice(&entry);
        }
        out.extend_from_slice(&0x0021u16.to_be_bytes());
        out.extend_from_slice(&this_c.to_be_bytes());
        out.extend_from_slice(&super_c.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // interfaces
        out.extend_from_slice(&0u16.to_be_bytes()); // fields
        out.extend_from_slice(&1u16.to_be_bytes()); // methods
        out.extend_from_slice(&0x0001u16.to_be_bytes());
        out.extend_from_slice(&on_cmd_u.to_be_bytes());
        out.extend_from_slice(&on_cmd_d.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes()); // 1 attr: Code
        out.extend_from_slice(&code_u.to_be_bytes());
        let attr_len = (12 + bc.len()) as u32;
        out.extend_from_slice(&attr_len.to_be_bytes());
        out.extend_from_slice(&4u16.to_be_bytes());
        out.extend_from_slice(&4u16.to_be_bytes());
        out.extend_from_slice(&(bc.len() as u32).to_be_bytes());
        out.extend_from_slice(&bc);
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out
    }

    #[test]
    fn auto_converted_jar_compiles_loads_into_dynamic_host_and_enforces_permissions() {
        let dir = std::env::temp_dir().join(format!(
            "ferrite-dyn-e2e-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let native_dir = dir.join("native");
        std::fs::create_dir_all(&native_dir).unwrap();
        let jar_path = dir.join("VipPlugin.jar");
        {
            let f = std::fs::File::create(&jar_path).unwrap();
            let mut zip = zip::ZipWriter::new(f);
            let opts = SimpleFileOptions::default();
            zip.start_file("plugin.yml", opts).unwrap();
            zip.write_all(
                b"name: VipPlugin\nversion: 1.0.0\nmain: com.example.VipPlugin\ncommands:\n  vip:\n    description: VIP command\n",
            )
            .unwrap();
            zip.start_file("com/example/VipPlugin.class", opts).unwrap();
            zip.write_all(&build_perm_cmd_class()).unwrap();
            zip.finish().unwrap();
        }

        let built = ferrite_plugin_convert::auto_build_native_plugin(&jar_path, &native_dir)
            .expect("auto_build");
        let ferrite_plugin_convert::AutoBuildOutcome::Built(lib_path) = built else {
            panic!("expected native cdylib build, got {built:?}");
        };
        assert!(lib_path.is_file());

        let hooks = DynamicPluginHooks::for_dir(&native_dir);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        hooks.set_command_sender(tx);
        let player = Uuid::from_u128(0xBEEF_1234);

        // Non-OP player is denied `admin.vip` by the auto-generated C-ABI v2 permission guard!
        let _ = hooks
            .run_plugin_command(CommandSender::Player(player), "/vip")
            .unwrap();
        let msg = rx.try_recv().expect("expected denial message via bridge");
        match msg {
            PlayerCommand::Message {
                content: MessageContent::Plain(text),
                ..
            }
            | PlayerCommand::Message {
                content: MessageContent::AdventureJson(text),
                ..
            } => assert!(
                text.contains("No permission"),
                "expected No permission, got {text}"
            ),
            other => panic!("unexpected PlayerCommand: {other:?}"),
        }

        // Promote to OP -> command succeeds and sends "VIP Welcome!" through live bridge!
        hooks.set_player_op(player, true);
        let res = hooks
            .run_plugin_command(CommandSender::Player(player), "/vip")
            .unwrap();
        assert!(res.handled);
        let msg = rx.try_recv().expect("expected VIP message via bridge");
        match msg {
            PlayerCommand::Message {
                content: MessageContent::Plain(text),
                ..
            }
            | PlayerCommand::Message {
                content: MessageContent::AdventureJson(text),
                ..
            } => assert!(
                text.contains("VIP Welcome!"),
                "expected VIP Welcome!, got {text}"
            ),
            other => panic!("unexpected PlayerCommand: {other:?}"),
        }

        hooks.shutdown();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
