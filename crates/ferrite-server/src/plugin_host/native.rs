//! Zero-overhead `ServerHooks` adapter backed by [`ferrite_native_plugins::NativePluginSuite`].
//!
//! Runs completely in-process in Rust (no JVM, no JNI) and can be layered ahead of the embedded
//! JVM [`super::HostHooks`] via [`super::CompositeHooks`].

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use ferrite_native_plugins::NativePluginSuite;
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

use crate::game::plugin_types::{
    BlockBreak, BlockInteract, BlockPosition, CommandSender, Error, InteractAction, MessageContent,
    PlayerCommand, PlayerLocation, PlayerMove, PluginCommand, PluginCommandResult, PluginMessage,
    ServerHooks,
};
use crate::game::{Dimension, DimensionRegistry, WorldId};

const NATIVE_COMMANDS: &[(&str, &[&str])] = &[
    ("perm", &[]),
    ("rg", &["region"]),
    ("home", &[]),
    ("sethome", &[]),
    ("delhome", &[]),
    ("homes", &[]),
    ("warp", &[]),
    ("setwarp", &[]),
    ("delwarp", &[]),
    ("warps", &[]),
    ("spawn", &[]),
    ("setspawn", &[]),
    ("tpa", &[]),
    ("tpaccept", &[]),
    ("tpdeny", &[]),
    ("bal", &["balance"]),
    ("pay", &[]),
    ("eco", &[]),
    ("mute", &[]),
    ("unmute", &[]),
    ("back", &[]),
    ("fly", &[]),
    ("god", &[]),
    ("heal", &[]),
    ("feed", &[]),
    ("gm", &[]),
    ("tp", &[]),
    ("tphere", &[]),
    ("save-native", &[]),
    ("pos1", &[]),
    ("pos2", &[]),
    ("set", &[]),
    ("replace", &[]),
    ("walls", &[]),
    ("sphere", &[]),
    ("hsphere", &[]),
    ("cyl", &[]),
    ("hcyl", &[]),
    ("copy", &[]),
    ("rotate", &[]),
    ("paste", &[]),
    ("undo", &[]),
    ("redo", &[]),
];

/// `ServerHooks` implementation backed by [`NativePluginSuite`].
pub struct NativePluginHooks {
    suite: Arc<NativePluginSuite>,
    data_dir: Option<PathBuf>,
    command_tx: Mutex<Option<UnboundedSender<PlayerCommand>>>,
    dimensions: Mutex<Option<Arc<DimensionRegistry>>>,
    player_positions: RwLock<HashMap<Uuid, (String, [f64; 3])>>,
}

impl NativePluginHooks {
    /// Wraps an existing [`NativePluginSuite`].
    pub fn new(suite: Arc<NativePluginSuite>, data_dir: Option<PathBuf>) -> Self {
        Self {
            suite,
            data_dir,
            command_tx: Mutex::new(None),
            dimensions: Mutex::new(None),
            player_positions: RwLock::new(HashMap::new()),
        }
    }

    /// Loads native plugin state from `data_dir` (creating defaults when files are absent).
    pub fn load_or_default(data_dir: PathBuf) -> Self {
        let suite = match NativePluginSuite::load_from_dir(&data_dir) {
            Ok(loaded) => loaded,
            Err(error) => {
                tracing::warn!(%error, dir = %data_dir.display(), "loading native plugin state failed; starting fresh");
                NativePluginSuite::new()
            }
        };
        Self::new(Arc::new(suite), Some(data_dir))
    }

    /// Attaches the game-loop `PlayerCommand` channel used for real teleports.
    pub fn set_command_sender(&self, tx: UnboundedSender<PlayerCommand>) {
        *self.command_tx.lock().unwrap_or_else(|p| p.into_inner()) = Some(tx);
    }

    /// Attaches the dimension registry so native WorldEdit edits apply directly to live chunks.
    pub fn set_dimensions(&self, dimensions: Arc<DimensionRegistry>) {
        *self.dimensions.lock().unwrap_or_else(|p| p.into_inner()) = Some(dimensions);
    }

    /// Returns the underlying native plugin suite.
    pub fn suite(&self) -> &Arc<NativePluginSuite> {
        &self.suite
    }

    /// Persists native plugin data to disk when a data directory is configured.
    pub fn save(&self) {
        let Some(dir) = &self.data_dir else {
            return;
        };
        if let Err(error) = self.suite.save_to_dir(dir) {
            tracing::warn!(%error, dir = %dir.display(), "saving native plugin state failed");
        }
    }

    fn has_region_bypass(&self, id: Uuid) -> bool {
        self.suite
            .permissions
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .has_permission(id, false, "ferrite.regions.bypass")
    }

    fn player_world_and_pos(&self, id: Uuid) -> (String, [f64; 3]) {
        self.player_positions
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .get(&id)
            .cloned()
            .unwrap_or_else(|| ("minecraft:overworld".to_owned(), [0.0, 64.0, 0.0]))
    }

    fn world_uuid(world: &str) -> Uuid {
        if let Some(dim) = Dimension::from_key(world) {
            return crate::game::plugin_world_id(WorldId::Vanilla(dim));
        }
        Uuid::parse_str(world).unwrap_or_else(|_| crate::game::plugin_types::default_world_id())
    }

    fn resolve_dimension(world: &str) -> Dimension {
        Dimension::from_key(world).unwrap_or(Dimension::Overworld)
    }

    fn try_worldedit_command(
        &self,
        sender_uuid: Option<Uuid>,
        is_op: bool,
        world: &str,
        pos: [f64; 3],
        line: &str,
    ) -> Option<String> {
        let trimmed = line.trim().trim_start_matches('/');
        let mut parts = trimmed.split_whitespace();
        let cmd = parts.next()?.to_ascii_lowercase();
        let args: Vec<&str> = parts.collect();
        if !matches!(
            cmd.as_str(),
            "set"
                | "replace"
                | "walls"
                | "sphere"
                | "hsphere"
                | "cyl"
                | "hcyl"
                | "copy"
                | "rotate"
                | "paste"
                | "undo"
                | "redo"
        ) {
            return None;
        }
        let Some(player) = sender_uuid else {
            return Some("WorldEdit commands require a player sender.".to_owned());
        };
        let allowed = self
            .suite
            .permissions
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .has_permission(player, is_op, "ferrite.worldedit.edit");
        if !allowed {
            return Some("You do not have permission for WorldEdit commands.".to_owned());
        }
        let Some(dimensions) = self
            .dimensions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        else {
            return Some("World chunk source is not attached.".to_owned());
        };
        let dim = Self::resolve_dimension(world);
        let source = dimensions.source(dim);

        let parse_block = |name: &str| -> Result<u32, String> {
            ferrite_data::blocks::default_state(name)
                .map(u32::from)
                .ok_or_else(|| format!("Unknown block: {name}"))
        };
        let read_block = |x: i32, y: i32, z: i32| source.block(x, y, z);

        let mut we = self
            .suite
            .worldedit
            .write()
            .unwrap_or_else(|p| p.into_inner());
        let edits = match cmd.as_str() {
            "set" => {
                let Some(block_name) = args.first() else {
                    return Some("Usage: //set <block>".to_owned());
                };
                let state = match parse_block(block_name) {
                    Ok(s) => s,
                    Err(msg) => return Some(msg),
                };
                we.plan_set(player, state, read_block)
            }
            "replace" => {
                if args.len() < 2 {
                    return Some("Usage: //replace <from_block> <to_block>".to_owned());
                }
                let from = match parse_block(args[0]) {
                    Ok(s) => s,
                    Err(msg) => return Some(msg),
                };
                let to = match parse_block(args[1]) {
                    Ok(s) => s,
                    Err(msg) => return Some(msg),
                };
                we.plan_replace(player, from, to, read_block)
            }
            "walls" => {
                let Some(block_name) = args.first() else {
                    return Some("Usage: //walls <block>".to_owned());
                };
                let state = match parse_block(block_name) {
                    Ok(s) => s,
                    Err(msg) => return Some(msg),
                };
                we.plan_walls(player, state, read_block)
            }
            "sphere" | "hsphere" => {
                if args.len() < 2 {
                    return Some(format!("Usage: //{cmd} <block> <radius>"));
                }
                let state = match parse_block(args[0]) {
                    Ok(s) => s,
                    Err(msg) => return Some(msg),
                };
                let Ok(radius) = args[1].parse::<i32>() else {
                    return Some(format!("Invalid radius: {}", args[1]));
                };
                let center = [
                    pos[0].floor() as i32,
                    pos[1].floor() as i32,
                    pos[2].floor() as i32,
                ];
                we.plan_sphere(player, center, radius, state, cmd == "hsphere", read_block)
            }
            "cyl" | "hcyl" => {
                if args.len() < 2 {
                    return Some(format!("Usage: //{cmd} <block> <radius> [height]"));
                }
                let state = match parse_block(args[0]) {
                    Ok(s) => s,
                    Err(msg) => return Some(msg),
                };
                let Ok(radius) = args[1].parse::<i32>() else {
                    return Some(format!("Invalid radius: {}", args[1]));
                };
                let height = match args.get(2) {
                    Some(h) => match h.parse::<i32>() {
                        Ok(val) => val,
                        Err(_) => return Some(format!("Invalid height: {h}")),
                    },
                    None => 1,
                };
                let center = [
                    pos[0].floor() as i32,
                    pos[1].floor() as i32,
                    pos[2].floor() as i32,
                ];
                we.plan_cyl(
                    player,
                    center,
                    radius,
                    height,
                    state,
                    cmd == "hcyl",
                    read_block,
                )
            }
            "copy" => {
                let origin = [
                    pos[0].floor() as i32,
                    pos[1].floor() as i32,
                    pos[2].floor() as i32,
                ];
                return match we.copy_clipboard_from(player, origin, read_block) {
                    Ok(count) => Some(format!("Copied {count} blocks to clipboard.")),
                    Err(err) => Some(format!("WorldEdit error: {err}")),
                };
            }
            "rotate" => {
                let degrees = match args.first() {
                    Some(deg) => match deg.parse::<i32>() {
                        Ok(v) => v,
                        Err(_) => return Some(format!("Invalid rotation degrees: {deg}")),
                    },
                    None => 90,
                };
                return match we.rotate_clipboard(player, degrees) {
                    Ok(count) => Some(format!("Rotated {count} clipboard blocks by {degrees}°.")),
                    Err(err) => Some(format!("WorldEdit error: {err}")),
                };
            }
            "paste" => {
                let origin = [
                    pos[0].floor() as i32,
                    pos[1].floor() as i32,
                    pos[2].floor() as i32,
                ];
                we.plan_paste(player, origin, read_block)
            }
            "undo" => we.undo(player),
            "redo" => we.redo(player),
            _ => unreachable!(),
        };

        match edits {
            Ok(mut plan) => {
                // Sort edits by chunk column (cx, cz) so sequential chunk writes maximize cache
                // locality and deduplicated RefreshChunk emission needs zero HashSet allocations.
                plan.sort_unstable_by_key(|(p, _)| (p[0].div_euclid(16), p[2].div_euclid(16)));
                let mut applied = 0usize;
                let tx_opt = self
                    .command_tx
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .clone();
                let mut last_chunk: Option<(i32, i32)> = None;
                for (block_pos, state) in plan {
                    if source
                        .set_block(block_pos[0], block_pos[1], block_pos[2], state)
                        .is_ok()
                    {
                        applied += 1;
                        let chunk = (block_pos[0].div_euclid(16), block_pos[2].div_euclid(16));
                        if last_chunk != Some(chunk) {
                            last_chunk = Some(chunk);
                            if let Some(tx) = &tx_opt {
                                let _ = tx.send(PlayerCommand::RefreshChunk {
                                    x: chunk.0,
                                    z: chunk.1,
                                });
                            }
                        }
                    }
                }
                Some(format!("Applied {applied} block updates ({cmd})."))
            }
            Err(err) => Some(format!("WorldEdit error: {err}")),
        }
    }

    fn try_admin_utility_command(
        &self,
        sender_uuid: Option<Uuid>,
        is_op: bool,
        world: &str,
        pos: [f64; 3],
        line: &str,
    ) -> Option<String> {
        let trimmed = line.trim().trim_start_matches('/');
        let mut parts = trimmed.split_whitespace();
        let cmd = parts.next()?.to_ascii_lowercase();
        let args: Vec<&str> = parts.collect();
        if !matches!(
            cmd.as_str(),
            "heal" | "feed" | "gm" | "tp" | "tphere" | "save-native"
        ) {
            return None;
        }
        let check_perm = |node: &str| -> bool {
            match sender_uuid {
                None => is_op,
                Some(uuid) => self
                    .suite
                    .permissions
                    .read()
                    .unwrap_or_else(|p| p.into_inner())
                    .has_permission(uuid, is_op, node),
            }
        };
        let tx = self
            .command_tx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();

        match cmd.as_str() {
            "save-native" => {
                if !check_perm("ferrite.native.save") {
                    return Some("Permission denied (ferrite.native.save)".to_owned());
                }
                self.save();
                Some("Saved native plugin state to disk.".to_owned())
            }
            "heal" => {
                if !check_perm("ferrite.essentials.heal") {
                    return Some("Permission denied (ferrite.essentials.heal)".to_owned());
                }
                let target = args
                    .first()
                    .and_then(|name| self.suite.resolve_player(name))
                    .or(sender_uuid);
                let Some(player) = target else {
                    return Some("Usage: /heal [player]".to_owned());
                };
                if let Some(tx) = tx {
                    let _ = tx.send(PlayerCommand::SetHealth {
                        player,
                        health: 20.0,
                    });
                    let _ = tx.send(PlayerCommand::SetFood { player, food: 20 });
                }
                Some("Healed player.".to_owned())
            }
            "feed" => {
                if !check_perm("ferrite.essentials.feed") {
                    return Some("Permission denied (ferrite.essentials.feed)".to_owned());
                }
                let target = args
                    .first()
                    .and_then(|name| self.suite.resolve_player(name))
                    .or(sender_uuid);
                let Some(player) = target else {
                    return Some("Usage: /feed [player]".to_owned());
                };
                if let Some(tx) = tx {
                    let _ = tx.send(PlayerCommand::SetFood { player, food: 20 });
                }
                Some("Fed player.".to_owned())
            }
            "gm" => {
                if !check_perm("ferrite.essentials.gamemode") {
                    return Some("Permission denied (ferrite.essentials.gamemode)".to_owned());
                }
                let Some(mode_arg) = args.first() else {
                    return Some("Usage: /gm <0|1|2|3|s|c|a|sp> [player]".to_owned());
                };
                let mode = match mode_arg.to_ascii_lowercase().as_str() {
                    "0" | "s" | "survival" => crate::game::plugin_types::GameMode::Survival,
                    "1" | "c" | "creative" => crate::game::plugin_types::GameMode::Creative,
                    "2" | "a" | "adventure" => crate::game::plugin_types::GameMode::Adventure,
                    "3" | "sp" | "spectator" => crate::game::plugin_types::GameMode::Spectator,
                    _ => return Some(format!("Unknown gamemode: {mode_arg}")),
                };
                let target = args
                    .get(1)
                    .and_then(|name| self.suite.resolve_player(name))
                    .or(sender_uuid);
                let Some(player) = target else {
                    return Some("Usage: /gm <mode> [player]".to_owned());
                };
                if let Some(tx) = tx {
                    let _ = tx.send(PlayerCommand::SetGameMode { player, mode });
                }
                Some(format!("Set gamemode to {mode:?}."))
            }
            "tp" => {
                if !check_perm("ferrite.essentials.tp") {
                    return Some("Permission denied (ferrite.essentials.tp)".to_owned());
                }
                let Some(player) = sender_uuid else {
                    return Some("Only players can use /tp <target>".to_owned());
                };
                let Some(target_arg) = args.first() else {
                    return Some("Usage: /tp <player>".to_owned());
                };
                let Some(target) = self.suite.resolve_player(target_arg) else {
                    return Some(format!("Unknown player '{target_arg}'"));
                };
                let (dest_world, dest_pos) = self.player_world_and_pos(target);
                self.suite
                    .essentials
                    .write()
                    .unwrap_or_else(|p| p.into_inner())
                    .set_last_location(
                        player,
                        ferrite_native_plugins::LocationSpec::from_pos(world, pos),
                    );
                if let Some(tx) = tx {
                    let _ = tx.send(PlayerCommand::Teleport {
                        player,
                        location: PlayerLocation {
                            world: Self::world_uuid(&dest_world),
                            x: dest_pos[0],
                            y: dest_pos[1],
                            z: dest_pos[2],
                            yaw: 0.0,
                            pitch: 0.0,
                        },
                    });
                }
                Some(format!("Teleported to {target_arg}."))
            }
            "tphere" => {
                if !check_perm("ferrite.essentials.tphere") {
                    return Some("Permission denied (ferrite.essentials.tphere)".to_owned());
                }
                let Some(target_arg) = args.first() else {
                    return Some("Usage: /tphere <player>".to_owned());
                };
                let Some(target) = self.suite.resolve_player(target_arg) else {
                    return Some(format!("Unknown player '{target_arg}'"));
                };
                let (prev_world, prev_pos) = self.player_world_and_pos(target);
                self.suite
                    .essentials
                    .write()
                    .unwrap_or_else(|p| p.into_inner())
                    .set_last_location(
                        target,
                        ferrite_native_plugins::LocationSpec::from_pos(prev_world, prev_pos),
                    );
                if let Some(tx) = tx {
                    let _ = tx.send(PlayerCommand::Teleport {
                        player: target,
                        location: PlayerLocation {
                            world: Self::world_uuid(world),
                            x: pos[0],
                            y: pos[1],
                            z: pos[2],
                            yaw: 0.0,
                            pitch: 0.0,
                        },
                    });
                }
                Some(format!("Teleported {target_arg} to you."))
            }
            _ => None,
        }
    }
}

impl ServerHooks for NativePluginHooks {
    fn player_joined(&self, id: Uuid, name: &str) -> Result<(), Error> {
        self.suite.register_player(id, name);
        Ok(())
    }

    fn player_left(&self, id: Uuid, name: &str) -> Result<(), Error> {
        self.suite.unregister_player(name);
        self.player_positions
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&id);
        Ok(())
    }

    fn chat(&self, id: Uuid, _name: &str, message: &str) -> Result<Option<String>, Error> {
        let muted = self
            .suite
            .essentials
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .is_muted(id);
        if muted {
            return Ok(None);
        }
        let (prefix, suffix) = self
            .suite
            .permissions
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .player_prefix_suffix(id);
        match (prefix, suffix) {
            (None, None) => Ok(Some(message.to_owned())),
            (p, s) => Ok(Some(format!(
                "{}{message}{}",
                p.unwrap_or_default(),
                s.unwrap_or_default()
            ))),
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
            // Same block cell: avoid region lookup and write-lock contention on stationary/look ticks.
            let already_tracked = self
                .player_positions
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .contains_key(&id);
            if already_tracked {
                return Ok(true);
            }
        }
        let (world, _) = self.player_world_and_pos(id);
        let (allowed, transition) = {
            let regions = self.suite.regions.read().unwrap_or_else(|p| p.into_inner());
            let can_enter = regions.can_enter(&world, to_block.0, to_block.1, to_block.2, id);
            let msgs = if from_block != to_block {
                regions.transition_messages(
                    &world,
                    [from_block.0, from_block.1, from_block.2],
                    [to_block.0, to_block.1, to_block.2],
                )
            } else {
                (None, None)
            };
            (can_enter, msgs)
        };
        if !allowed && !self.has_region_bypass(id) {
            return Ok(false);
        }
        self.player_positions
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id, (world, movement.to));
        let notice = transition.1.or(transition.0);
        if let Some(text) = notice
            && let Some(tx) = self
                .command_tx
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_ref()
        {
            let _ = tx.send(PlayerCommand::ActionBar {
                player: id,
                component_json: serde_json::json!({ "text": text }).to_string(),
            });
        }
        Ok(true)
    }

    fn update_player_location(&self, id: Uuid, location: &PlayerLocation) -> Result<(), Error> {
        let world = if location.world == crate::game::plugin_types::default_world_id() {
            "minecraft:overworld".to_owned()
        } else {
            location.world.to_string()
        };
        self.player_positions
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id, (world, [location.x, location.y, location.z]));
        Ok(())
    }

    fn player_interacted(
        &self,
        id: Uuid,
        _name: &str,
        _action: InteractAction,
        event: BlockInteract<'_>,
    ) -> Result<bool, Error> {
        let allowed = self
            .suite
            .regions
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .can_interact(event.world, event.block.x, event.block.y, event.block.z, id);
        Ok(allowed || self.has_region_bypass(id))
    }

    fn block_broken(&self, id: Uuid, _name: &str, event: BlockBreak<'_>) -> Result<bool, Error> {
        let allowed = self
            .suite
            .regions
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .can_break(event.world, event.block.x, event.block.y, event.block.z, id);
        Ok(allowed || self.has_region_bypass(id))
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
        let (world, _) = self.player_world_and_pos(id);
        let allowed = self
            .suite
            .regions
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .can_place(&world, block.x, block.y, block.z, id);
        Ok(allowed || self.has_region_bypass(id))
    }

    fn blocks_exploded(
        &self,
        _world: Uuid,
        _center: BlockPosition,
        blocks: &[BlockPosition],
        _yield: f32,
    ) -> Result<Option<Vec<BlockPosition>>, Error> {
        let regions = self.suite.regions.read().unwrap_or_else(|p| p.into_inner());
        let mut filtered = Vec::with_capacity(blocks.len());
        let mut changed = false;
        for pos in blocks {
            if regions.can_explode("minecraft:overworld", pos.x, pos.y, pos.z) {
                filtered.push(*pos);
            } else {
                changed = true;
            }
        }
        Ok(changed.then_some(filtered))
    }

    fn player_command(&self, _id: Uuid, _name: &str, _message: &str) -> Result<bool, Error> {
        Ok(true)
    }

    fn entity_damaged(
        &self,
        id: Uuid,
        _name: &str,
        cause: &str,
        amount: f64,
    ) -> Result<f64, Error> {
        let is_god = self
            .suite
            .essentials
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .is_god(id);
        if is_god {
            return Ok(0.0);
        }
        let (world, pos) = self.player_world_and_pos(id);
        let bx = pos[0].floor() as i32;
        let by = pos[1].floor() as i32;
        let bz = pos[2].floor() as i32;
        let regions = self.suite.regions.read().unwrap_or_else(|p| p.into_inner());
        if regions.is_invincible(&world, bx, by, bz) {
            return Ok(0.0);
        }
        let is_pvp = matches!(
            cause,
            "ENTITY_ATTACK" | "ENTITY_SWEEP_ATTACK" | "PROJECTILE" | "PVP"
        );
        if is_pvp && !regions.can_pvp(&world, bx, by, bz) {
            return Ok(0.0);
        }
        Ok(amount)
    }

    fn mob_spawn_allowed(&self, world: &str, x: i32, y: i32, z: i32) -> Result<bool, Error> {
        Ok(self
            .suite
            .regions
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .can_mob_spawn(world, x, y, z))
    }

    fn fire_spread_allowed(&self, world: &str, x: i32, y: i32, z: i32) -> Result<bool, Error> {
        Ok(self
            .suite
            .regions
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .can_fire_spread(world, x, y, z))
    }

    fn plugin_commands(&self) -> Result<Vec<PluginCommand>, Error> {
        Ok(NATIVE_COMMANDS
            .iter()
            .map(|(name, aliases)| PluginCommand {
                name: (*name).to_owned(),
                aliases: aliases.iter().map(|a| (*a).to_owned()).collect(),
                permission: None,
                usage: None,
            })
            .collect())
    }

    fn run_plugin_command(
        &self,
        sender: CommandSender,
        line: &str,
    ) -> Result<PluginCommandResult, Error> {
        let (sender_uuid, is_op, recipient, world, pos) = match sender {
            CommandSender::Console => (
                None,
                true,
                Uuid::nil(),
                "minecraft:overworld".to_owned(),
                [0.0, 64.0, 0.0],
            ),
            CommandSender::Player(uuid) => {
                let op = self
                    .suite
                    .permissions
                    .read()
                    .unwrap_or_else(|p| p.into_inner())
                    .has_permission(uuid, false, "*");
                let (w, p) = self.player_world_and_pos(uuid);
                (Some(uuid), op, uuid, w, p)
            }
        };

        if let Some(message) = self.try_worldedit_command(sender_uuid, is_op, &world, pos, line) {
            return Ok(PluginCommandResult {
                handled: true,
                messages: vec![PluginMessage {
                    player: recipient,
                    content: MessageContent::Plain(message),
                }],
            });
        }

        if let Some(message) = self.try_admin_utility_command(sender_uuid, is_op, &world, pos, line)
        {
            return Ok(PluginCommandResult {
                handled: true,
                messages: vec![PluginMessage {
                    player: recipient,
                    content: MessageContent::Plain(message),
                }],
            });
        }

        let Some(outcome) = self
            .suite
            .dispatch_command(sender_uuid, is_op, &world, pos, line)
        else {
            return Ok(PluginCommandResult {
                handled: false,
                messages: Vec::new(),
            });
        };

        if let Some(tp) = outcome.teleport {
            let (prev_world, prev_pos) = self.player_world_and_pos(tp.player);
            self.suite
                .essentials
                .write()
                .unwrap_or_else(|p| p.into_inner())
                .set_last_location(
                    tp.player,
                    ferrite_native_plugins::LocationSpec::from_pos(prev_world, prev_pos),
                );
            if let Some(tx) = self
                .command_tx
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_ref()
            {
                let _ = tx.send(PlayerCommand::Teleport {
                    player: tp.player,
                    location: PlayerLocation {
                        world: Self::world_uuid(&tp.location.world),
                        x: tp.location.x,
                        y: tp.location.y,
                        z: tp.location.z,
                        yaw: tp.location.yaw,
                        pitch: tp.location.pitch,
                    },
                });
            }
        }

        let root_cmd = line
            .trim()
            .trim_start_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if outcome.success
            && matches!(root_cmd.as_str(), "fly" | "god")
            && let Some(player) = sender_uuid
            && let Some(tx) = self
                .command_tx
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_ref()
        {
            let ess = self
                .suite
                .essentials
                .read()
                .unwrap_or_else(|p| p.into_inner());
            let allow_flight = ess.is_fly(player);
            let invulnerable = ess.is_god(player);
            let _ = tx.send(PlayerCommand::Abilities {
                player,
                allow_flight,
                flying: allow_flight,
                invulnerable,
                fly_speed: 0.1,
                walk_speed: 0.2,
            });
        }

        let mut messages: Vec<PluginMessage> = outcome
            .messages
            .into_iter()
            .map(|msg| PluginMessage {
                player: recipient,
                content: MessageContent::Plain(msg),
            })
            .collect();
        for (target, text) in outcome.notifications {
            messages.push(PluginMessage {
                player: target,
                content: MessageContent::Plain(text),
            });
        }
        Ok(PluginCommandResult {
            handled: true,
            messages,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_native_plugins::{Region, RegionFlags};

    #[test]
    fn native_hooks_enforce_regions_mutes_and_teleports_without_jvm() {
        let suite = Arc::new(NativePluginSuite::new());
        let owner = Uuid::from_u128(1);
        let intruder = Uuid::from_u128(2);

        {
            let mut regions = suite.regions.write().unwrap();
            let mut region =
                Region::new("minecraft:overworld", "spawn", [-10, 0, -10], [10, 255, 10]);
            region.priority = 10;
            region.flags = RegionFlags {
                build: Some(false),
                block_break: Some(false),
                ..RegionFlags::default()
            };
            region.owners.insert(owner);
            regions.upsert_region(region).unwrap();
        }
        suite
            .essentials
            .write()
            .unwrap()
            .mute(intruder, Some("spam".into()), None);

        let hooks = NativePluginHooks::new(Arc::clone(&suite), None);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        hooks.set_command_sender(tx);

        // Live player coordinates flow into /sethome and /home emits PlayerCommand::Teleport.
        hooks
            .player_moved(
                owner,
                "Owner",
                &PlayerMove {
                    from: [0.0, 64.0, 0.0],
                    to: [42.5, 70.0, -18.25],
                    from_yaw: 0.0,
                    from_pitch: 0.0,
                    to_yaw: 0.0,
                    to_pitch: 0.0,
                },
            )
            .unwrap();

        let brk = BlockBreak {
            world: "minecraft:overworld",
            block: BlockPosition { x: 0, y: 64, z: 0 },
            material: "minecraft:stone",
        };
        assert!(hooks.block_broken(owner, "Owner", brk).unwrap());
        assert!(!hooks.block_broken(intruder, "Intruder", brk).unwrap());
        assert_eq!(hooks.chat(intruder, "Intruder", "hi").unwrap(), None);
        assert_eq!(
            hooks.chat(owner, "Owner", "hi").unwrap().as_deref(),
            Some("hi")
        );

        let set = hooks
            .run_plugin_command(CommandSender::Player(owner), "/sethome base")
            .unwrap();
        assert!(set.handled);
        let go = hooks
            .run_plugin_command(CommandSender::Player(owner), "/home base")
            .unwrap();
        assert!(go.handled);
        match rx.try_recv().expect("teleport command queued") {
            PlayerCommand::Teleport { player, location } => {
                assert_eq!(player, owner);
                assert!((location.x - 42.5).abs() < 1e-6);
                assert!((location.y - 70.0).abs() < 1e-6);
                assert!((location.z - -18.25).abs() < 1e-6);
            }
            other => panic!("expected PlayerCommand::Teleport, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn native_worldedit_set_and_undo_modify_real_chunks() {
        let source = crate::game::AnvilChunkSource::new_memory();
        let mut loaded = false;
        for _ in 0..2_000 {
            if let Some(result) = source.get(0, 0, 0) {
                result.expect("chunk loaded");
                loaded = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(loaded, "chunk never loaded");

        let stone = u32::from(ferrite_data::blocks::default_state("minecraft:stone").unwrap());
        let air = u32::from(ferrite_data::blocks::default_state("minecraft:air").unwrap());
        source.set_block(2, 70, 2, air).unwrap();
        source.set_block(3, 70, 2, air).unwrap();

        let dims =
            DimensionRegistry::with_source(Dimension::Overworld, Arc::clone(&source), [0, 65, 0]);
        let suite = Arc::new(NativePluginSuite::new());
        let builder = Uuid::from_u128(99);
        suite
            .permissions
            .write()
            .unwrap()
            .set_player_permission(builder, "ferrite.worldedit.*", true)
            .unwrap();

        let hooks = NativePluginHooks::new(Arc::clone(&suite), None);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        hooks.set_command_sender(tx);
        hooks.set_dimensions(dims);

        hooks
            .run_plugin_command(CommandSender::Player(builder), "//pos1 2 70 2")
            .unwrap();
        hooks
            .run_plugin_command(CommandSender::Player(builder), "//pos2 3 70 2")
            .unwrap();
        let set_res = hooks
            .run_plugin_command(CommandSender::Player(builder), "//set stone")
            .unwrap();
        assert!(set_res.handled);
        assert_eq!(source.block(2, 70, 2), Some(stone));
        assert_eq!(source.block(3, 70, 2), Some(stone));
        // Deduplicated chunk refresh broadcast for both blocks in chunk (0, 0).
        assert!(matches!(
            rx.try_recv(),
            Ok(PlayerCommand::RefreshChunk { x: 0, z: 0 })
        ));
        assert!(rx.try_recv().is_err());

        let undo_res = hooks
            .run_plugin_command(CommandSender::Player(builder), "//undo")
            .unwrap();
        assert!(undo_res.handled);
        assert_eq!(source.block(2, 70, 2), Some(air));
        assert_eq!(source.block(3, 70, 2), Some(air));
        assert!(matches!(
            rx.try_recv(),
            Ok(PlayerCommand::RefreshChunk { x: 0, z: 0 })
        ));
        assert!(rx.try_recv().is_err());

        // Test //sphere and /fly, /god, /back integration.
        let sphere_res = hooks
            .run_plugin_command(CommandSender::Player(builder), "//sphere stone 1")
            .unwrap();
        assert!(sphere_res.handled);
        assert!(rx.try_recv().is_ok());

        suite
            .permissions
            .write()
            .unwrap()
            .set_player_permission(builder, "ferrite.essentials.*", true)
            .unwrap();
        let fly_res = hooks
            .run_plugin_command(CommandSender::Player(builder), "/fly")
            .unwrap();
        assert!(fly_res.handled);
        assert!(matches!(
            rx.try_recv(),
            Ok(PlayerCommand::Abilities {
                allow_flight: true,
                ..
            })
        ));

        let god_res = hooks
            .run_plugin_command(CommandSender::Player(builder), "/god")
            .unwrap();
        assert!(god_res.handled);
        assert!(matches!(
            rx.try_recv(),
            Ok(PlayerCommand::Abilities {
                invulnerable: true,
                ..
            })
        ));
        assert_eq!(
            hooks
                .entity_damaged(builder, "Builder", "FALL", 10.0)
                .unwrap(),
            0.0
        );
    }

    #[test]
    fn native_region_pvp_invincible_and_greeting_actionbar() {
        let suite = Arc::new(NativePluginSuite::new());
        let p = Uuid::from_u128(55);
        {
            let mut regions = suite.regions.write().unwrap();
            let mut safe = Region::new("minecraft:overworld", "safezone", [0, 0, 0], [10, 255, 10]);
            safe.flags = RegionFlags {
                pvp: Some(false),
                greeting: Some("Welcome to SafeZone".into()),
                farewell: Some("Leaving SafeZone".into()),
                ..RegionFlags::default()
            };
            regions.upsert_region(safe).unwrap();
        }
        let hooks = NativePluginHooks::new(Arc::clone(&suite), None);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        hooks.set_command_sender(tx);

        // Entering safezone emits greeting ActionBar.
        assert!(
            hooks
                .player_moved(
                    p,
                    "Alice",
                    &PlayerMove {
                        from: [-2.0, 64.0, 5.0],
                        to: [2.0, 64.0, 5.0],
                        from_yaw: 0.0,
                        from_pitch: 0.0,
                        to_yaw: 0.0,
                        to_pitch: 0.0,
                    },
                )
                .unwrap()
        );
        assert!(matches!(
            rx.try_recv(),
            Ok(PlayerCommand::ActionBar {
                ref component_json,
                ..
            }) if component_json.contains("Welcome to SafeZone")
        ));

        // PvP damage is blocked, fall damage still applies when invincible is unset.
        assert_eq!(
            hooks
                .entity_damaged(p, "Alice", "ENTITY_ATTACK", 6.0)
                .unwrap(),
            0.0
        );
        assert_eq!(hooks.entity_damaged(p, "Alice", "FALL", 4.0).unwrap(), 4.0);

        // Leaving safezone emits farewell ActionBar.
        assert!(
            hooks
                .player_moved(
                    p,
                    "Alice",
                    &PlayerMove {
                        from: [2.0, 64.0, 5.0],
                        to: [-2.0, 64.0, 5.0],
                        from_yaw: 0.0,
                        from_pitch: 0.0,
                        to_yaw: 0.0,
                        to_pitch: 0.0,
                    },
                )
                .unwrap()
        );
        assert!(matches!(
            rx.try_recv(),
            Ok(PlayerCommand::ActionBar {
                ref component_json,
                ..
            }) if component_json.contains("Leaving SafeZone")
        ));
    }

    #[test]
    fn native_admin_cmds_prefix_and_mob_fire_hooks_work() {
        let suite = Arc::new(NativePluginSuite::new());
        let admin = Uuid::from_u128(10);
        let guest = Uuid::from_u128(11);
        suite.register_player(admin, "Admin");
        suite.register_player(guest, "Guest");
        {
            let mut perms = suite.permissions.write().unwrap();
            perms.create_group("vip", 50).unwrap();
            perms
                .set_group_prefix("vip", Some("[VIP] ".to_owned()))
                .unwrap();
            perms.add_player_group(admin, "vip").unwrap();
            perms.set_player_permission(admin, "*", true).unwrap();
        }
        {
            let mut regions = suite.regions.write().unwrap();
            let mut sanctuary =
                Region::new("minecraft:overworld", "sanctuary", [0, 0, 0], [8, 255, 8]);
            sanctuary.flags = RegionFlags {
                mob_spawning: Some(false),
                fire_spread: Some(false),
                ..RegionFlags::default()
            };
            regions.upsert_region(sanctuary).unwrap();
        }

        let hooks = NativePluginHooks::new(Arc::clone(&suite), None);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        hooks.set_command_sender(tx);

        // Chat prefix formatting.
        assert_eq!(
            hooks.chat(admin, "Admin", "hello").unwrap().as_deref(),
            Some("[VIP] hello")
        );
        // Mob spawning & fire spread hooks honor region flags.
        assert!(
            !hooks
                .mob_spawn_allowed("minecraft:overworld", 4, 64, 4)
                .unwrap()
        );
        assert!(
            !hooks
                .fire_spread_allowed("minecraft:overworld", 4, 64, 4)
                .unwrap()
        );
        assert!(
            hooks
                .mob_spawn_allowed("minecraft:overworld", 20, 64, 20)
                .unwrap()
        );

        // /heal, /feed, /gm, /tp, /tphere emit real PlayerCommands.
        assert!(
            hooks
                .run_plugin_command(CommandSender::Player(admin), "/heal")
                .unwrap()
                .handled
        );
        assert!(matches!(
            rx.try_recv(),
            Ok(PlayerCommand::SetHealth { health, .. }) if (health - 20.0).abs() < 1e-6
        ));
        assert!(matches!(
            rx.try_recv(),
            Ok(PlayerCommand::SetFood { food: 20, .. })
        ));

        assert!(
            hooks
                .run_plugin_command(CommandSender::Player(admin), "/gm c")
                .unwrap()
                .handled
        );
        assert!(matches!(
            rx.try_recv(),
            Ok(PlayerCommand::SetGameMode {
                mode: crate::game::plugin_types::GameMode::Creative,
                ..
            })
        ));

        assert!(
            hooks
                .run_plugin_command(CommandSender::Player(admin), "/tphere Guest")
                .unwrap()
                .handled
        );
        assert!(matches!(
            rx.try_recv(),
            Ok(PlayerCommand::Teleport { player, .. }) if player == guest
        ));
    }
}
