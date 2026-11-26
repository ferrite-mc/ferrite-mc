//! Zero-overhead Rust native plugin suite for Ferrite.
//!
//! Replaces heavyweight JVM plugins for the core server stack:
//! - [`permissions`]: LuckPerms-style weighted group inheritance, wildcards, and per-player overrides.
//! - [`regions`]: WorldGuard-style 3D AABB region protection with fast chunk-column indexing.
//! - [`essentials`]: EssentialsX-style homes, warps, spawn, TPA with TTL, economy, and chat mutes.
//! - [`worldedit`]: FAWE/WorldEdit-style cuboid selections, pure batch planners, clipboard, and undo/redo.
//! - [`NativePluginSuite`]: Thread-safe coordinator and unified [`NativePluginSuite::dispatch_command`].

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod error;
pub mod essentials;
pub mod permissions;
pub mod regions;
mod storage;
pub mod worldedit;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::Duration;

use uuid::Uuid;

pub use error::{PluginError, Result};
pub use essentials::{EssentialsEngine, LocationSpec, MuteRecord, TpaRequest};
pub use permissions::{PermissionGroup, PermissionsEngine, PlayerPermissions};
pub use regions::{Region, RegionEngine, RegionFlags};
pub use worldedit::{BlockEdit, Clipboard, CuboidSelection, WorldEditEngine};

/// Persistence filenames inside a plugin data directory.
const PERMISSIONS_FILE: &str = "permissions.json";
const REGIONS_FILE: &str = "regions.json";
const ESSENTIALS_FILE: &str = "essentials.json";

/// Pending player teleport produced by executing a native plugin command.
#[derive(Debug, Clone, PartialEq)]
pub struct TeleportAction {
    /// Player UUID to teleport (the sender for `/home`/`/warp`/`/spawn`, or requester for `/tpaccept`).
    pub player: Uuid,
    /// Destination world, coordinates, and orientation.
    pub location: LocationSpec,
}

/// Outcome returned when [`NativePluginSuite::dispatch_command`] handles a native command.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeCommandOutcome {
    /// Whether the command succeeded (`true`) or failed permission/syntax/domain validation.
    pub success: bool,
    /// Messages to deliver back to the command sender.
    pub messages: Vec<String>,
    /// Optional teleport the server should apply to a player.
    pub teleport: Option<TeleportAction>,
    /// Optional notifications `(recipient_uuid, message)` for other online players.
    pub notifications: Vec<(Uuid, String)>,
}

impl NativeCommandOutcome {
    /// Builds a successful outcome with a single message.
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            success: true,
            messages: vec![message.into()],
            teleport: None,
            notifications: Vec::new(),
        }
    }

    /// Builds a failed outcome with an error message.
    pub fn err(message: impl Into<String>) -> Self {
        Self {
            success: false,
            messages: vec![message.into()],
            teleport: None,
            notifications: Vec::new(),
        }
    }
}

/// Thread-safe unified coordinator holding all four native plugin engines.
#[derive(Debug, Clone, Default)]
pub struct NativePluginSuite {
    /// LuckPerms-style permissions engine.
    pub permissions: Arc<RwLock<PermissionsEngine>>,
    /// WorldGuard-style 3D region protection engine.
    pub regions: Arc<RwLock<RegionEngine>>,
    /// EssentialsX-style homes, warps, spawn, TPA, economy, and mute engine.
    pub essentials: Arc<RwLock<EssentialsEngine>>,
    /// FAWE/WorldEdit-style selection, batch edit planner, and undo/redo engine.
    pub worldedit: Arc<RwLock<WorldEditEngine>>,
    /// Optional lowercase player-name -> UUID lookup table for name-based command arguments.
    player_names: Arc<RwLock<HashMap<String, Uuid>>>,
}

fn read_lock<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(PoisonError::into_inner)
}

fn write_lock<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(PoisonError::into_inner)
}

impl NativePluginSuite {
    /// Creates a fresh native plugin suite with empty default engines.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a suite wrapping pre-configured engine instances.
    pub fn with_engines(
        permissions: PermissionsEngine,
        regions: RegionEngine,
        essentials: EssentialsEngine,
        worldedit: WorldEditEngine,
    ) -> Self {
        Self {
            permissions: Arc::new(RwLock::new(permissions)),
            regions: Arc::new(RwLock::new(regions)),
            essentials: Arc::new(RwLock::new(essentials)),
            worldedit: Arc::new(RwLock::new(worldedit)),
            player_names: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Loads `permissions.json`, `regions.json`, and `essentials.json` from `dir` (defaulting any
    /// missing file to an empty engine).
    pub fn load_from_dir(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        let permissions = PermissionsEngine::load_or_default(dir.join(PERMISSIONS_FILE))?;
        let regions = RegionEngine::load_or_default(dir.join(REGIONS_FILE))?;
        let essentials = EssentialsEngine::load_or_default(dir.join(ESSENTIALS_FILE))?;
        Ok(Self::with_engines(
            permissions,
            regions,
            essentials,
            WorldEditEngine::default(),
        ))
    }

    /// Saves `permissions.json`, `regions.json`, and `essentials.json` atomically to `dir`.
    pub fn save_to_dir(&self, dir: impl AsRef<Path>) -> Result<()> {
        let dir = dir.as_ref();
        read_lock(&self.permissions).save_to_file(dir.join(PERMISSIONS_FILE))?;
        read_lock(&self.regions).save_to_file(dir.join(REGIONS_FILE))?;
        read_lock(&self.essentials).save_to_file(dir.join(ESSENTIALS_FILE))?;
        Ok(())
    }

    /// Registers a player username <-> UUID mapping so commands can resolve player names.
    pub fn register_player(&self, uuid: Uuid, name: &str) {
        let key = name.trim().to_ascii_lowercase();
        if !key.is_empty() {
            write_lock(&self.player_names).insert(key, uuid);
        }
    }

    /// Removes a player username mapping.
    pub fn unregister_player(&self, name: &str) {
        let key = name.trim().to_ascii_lowercase();
        write_lock(&self.player_names).remove(&key);
    }

    /// Resolves either a UUID string or a registered player name.
    pub fn resolve_player(&self, input: &str) -> Option<Uuid> {
        if let Ok(uuid) = Uuid::parse_str(input.trim()) {
            return Some(uuid);
        }
        read_lock(&self.player_names)
            .get(&input.trim().to_ascii_lowercase())
            .copied()
    }

    /// Evaluates `node` for an optional sender (`None` = console, which passes if `is_op`).
    fn check_permission(
        &self,
        sender_uuid: Option<Uuid>,
        is_op: bool,
        node: &str,
        default_allow_for_players: bool,
    ) -> bool {
        let Some(uuid) = sender_uuid else {
            return is_op;
        };
        let perms = read_lock(&self.permissions);
        match perms.evaluate_permission(uuid, node) {
            Some(allowed) => allowed,
            None => {
                if default_allow_for_players {
                    true
                } else {
                    is_op
                }
            }
        }
    }

    /// Dispatches a slash command (or bare command string) across the native plugin suite.
    ///
    /// Returns `None` if the command is not owned by the native plugin suite so the server can
    /// continue routing it to vanilla/Brigadier or JVM plugins.
    pub fn dispatch_command(
        &self,
        sender_uuid: Option<Uuid>,
        is_op: bool,
        world: &str,
        pos: [f64; 3],
        cmd_line: &str,
    ) -> Option<NativeCommandOutcome> {
        let trimmed = cmd_line.trim();
        let after_single_slash = trimmed.strip_prefix('/').unwrap_or(trimmed);
        let mut parts = after_single_slash.split_whitespace();
        let raw_cmd = parts.next()?;
        let cmd_lower = raw_cmd.to_ascii_lowercase();
        let args: Vec<&str> = parts.collect();

        // Support both `/pos1` and `//pos1` (which becomes `/pos1` after stripping one `/`).
        match cmd_lower.as_str() {
            "perm" => Some(self.cmd_perm(sender_uuid, is_op, &args)),
            "rg" | "region" => Some(self.cmd_region(sender_uuid, is_op, world, pos, &args)),
            "sethome" => Some(self.cmd_sethome(sender_uuid, is_op, world, pos, &args)),
            "home" => Some(self.cmd_home(sender_uuid, is_op, &args)),
            "delhome" => Some(self.cmd_delhome(sender_uuid, is_op, &args)),
            "homes" => Some(self.cmd_homes(sender_uuid, is_op)),
            "setwarp" => Some(self.cmd_setwarp(sender_uuid, is_op, world, pos, &args)),
            "warp" => Some(self.cmd_warp(sender_uuid, is_op, &args)),
            "delwarp" => Some(self.cmd_delwarp(sender_uuid, is_op, &args)),
            "warps" => Some(self.cmd_warps(sender_uuid, is_op)),
            "setspawn" => Some(self.cmd_setspawn(sender_uuid, is_op, world, pos)),
            "spawn" => Some(self.cmd_spawn(sender_uuid, is_op)),
            "tpa" => Some(self.cmd_tpa(sender_uuid, is_op, &args)),
            "tpaccept" => Some(self.cmd_tpaccept(sender_uuid, is_op, world, pos, &args)),
            "tpdeny" => Some(self.cmd_tpdeny(sender_uuid, is_op, &args)),
            "bal" | "balance" => Some(self.cmd_bal(sender_uuid, is_op, &args)),
            "pay" => Some(self.cmd_pay(sender_uuid, is_op, &args)),
            "eco" => Some(self.cmd_eco(sender_uuid, is_op, &args)),
            "mute" => Some(self.cmd_mute(sender_uuid, is_op, &args)),
            "unmute" => Some(self.cmd_unmute(sender_uuid, is_op, &args)),
            "back" => Some(self.cmd_back(sender_uuid, is_op)),
            "fly" => Some(self.cmd_fly(sender_uuid, is_op)),
            "god" => Some(self.cmd_god(sender_uuid, is_op)),
            "pos1" | "/pos1" => Some(self.cmd_pos(sender_uuid, is_op, pos, &args, true)),
            "pos2" | "/pos2" => Some(self.cmd_pos(sender_uuid, is_op, pos, &args, false)),
            _ => None,
        }
    }

    fn cmd_perm(
        &self,
        sender_uuid: Option<Uuid>,
        is_op: bool,
        args: &[&str],
    ) -> NativeCommandOutcome {
        if !self.check_permission(sender_uuid, is_op, "ferrite.perm.admin", false) {
            return NativeCommandOutcome::err("Permission denied (ferrite.perm.admin)");
        }

        match args {
            ["group", "create", group] => {
                let mut perms = write_lock(&self.permissions);
                match perms.create_group(group, 0) {
                    Ok(()) => NativeCommandOutcome::ok(format!("Created group '{group}'")),
                    Err(e) => NativeCommandOutcome::err(e.to_string()),
                }
            }
            ["group", "create", group, weight_str] => {
                let Ok(weight) = weight_str.parse::<i32>() else {
                    return NativeCommandOutcome::err(format!("Invalid weight '{weight_str}'"));
                };
                let mut perms = write_lock(&self.permissions);
                match perms.create_group(group, weight) {
                    Ok(()) => NativeCommandOutcome::ok(format!(
                        "Created group '{group}' with weight {weight}"
                    )),
                    Err(e) => NativeCommandOutcome::err(e.to_string()),
                }
            }
            ["group", "delete", group] => {
                let mut perms = write_lock(&self.permissions);
                match perms.delete_group(group) {
                    Ok(true) => NativeCommandOutcome::ok(format!("Deleted group '{group}'")),
                    Ok(false) => NativeCommandOutcome::err(format!("Group '{group}' not found")),
                    Err(e) => NativeCommandOutcome::err(e.to_string()),
                }
            }
            ["group", group, "weight", weight_str] => {
                let Ok(weight) = weight_str.parse::<i32>() else {
                    return NativeCommandOutcome::err(format!("Invalid weight '{weight_str}'"));
                };
                let mut perms = write_lock(&self.permissions);
                match perms.set_group_weight(group, weight) {
                    Ok(()) => {
                        NativeCommandOutcome::ok(format!("Set weight of '{group}' to {weight}"))
                    }
                    Err(e) => NativeCommandOutcome::err(e.to_string()),
                }
            }
            ["group", group, "parent", "add", parent] => {
                let mut perms = write_lock(&self.permissions);
                match perms.add_group_parent(group, parent) {
                    Ok(()) => NativeCommandOutcome::ok(format!(
                        "Added parent '{parent}' to group '{group}'"
                    )),
                    Err(e) => NativeCommandOutcome::err(e.to_string()),
                }
            }
            ["group", group, "parent", "remove", parent] => {
                let mut perms = write_lock(&self.permissions);
                match perms.remove_group_parent(group, parent) {
                    Ok(true) => NativeCommandOutcome::ok(format!(
                        "Removed parent '{parent}' from group '{group}'"
                    )),
                    Ok(false) => NativeCommandOutcome::err(format!(
                        "Group '{group}' does not inherit '{parent}'"
                    )),
                    Err(e) => NativeCommandOutcome::err(e.to_string()),
                }
            }
            ["group", group, "set", node] => {
                let (clean_node, allow) = parse_node_flag(node, None);
                let mut perms = write_lock(&self.permissions);
                match perms.set_group_permission(group, clean_node, allow) {
                    Ok(()) => NativeCommandOutcome::ok(format!(
                        "Set '{clean_node}' = {allow} on group '{group}'"
                    )),
                    Err(e) => NativeCommandOutcome::err(e.to_string()),
                }
            }
            ["group", group, "set", node, val] => {
                let Ok(allow) = parse_bool_arg(val) else {
                    return NativeCommandOutcome::err(format!("Invalid boolean '{val}'"));
                };
                let (clean_node, allow) = parse_node_flag(node, Some(allow));
                let mut perms = write_lock(&self.permissions);
                match perms.set_group_permission(group, clean_node, allow) {
                    Ok(()) => NativeCommandOutcome::ok(format!(
                        "Set '{clean_node}' = {allow} on group '{group}'"
                    )),
                    Err(e) => NativeCommandOutcome::err(e.to_string()),
                }
            }
            ["group", group, "unset", node] => {
                let mut perms = write_lock(&self.permissions);
                match perms.unset_group_permission(group, node) {
                    Ok(true) => {
                        NativeCommandOutcome::ok(format!("Unset '{node}' on group '{group}'"))
                    }
                    Ok(false) => NativeCommandOutcome::err(format!(
                        "Permission '{node}' was not set on '{group}'"
                    )),
                    Err(e) => NativeCommandOutcome::err(e.to_string()),
                }
            }
            ["user", player_arg, "group", "add", group] => {
                let Some(target) = self.resolve_player(player_arg) else {
                    return NativeCommandOutcome::err(format!("Unknown player '{player_arg}'"));
                };
                let mut perms = write_lock(&self.permissions);
                match perms.add_player_group(target, group) {
                    Ok(()) => {
                        NativeCommandOutcome::ok(format!("Added {target} to group '{group}'"))
                    }
                    Err(e) => NativeCommandOutcome::err(e.to_string()),
                }
            }
            ["user", player_arg, "group", "remove", group] => {
                let Some(target) = self.resolve_player(player_arg) else {
                    return NativeCommandOutcome::err(format!("Unknown player '{player_arg}'"));
                };
                let mut perms = write_lock(&self.permissions);
                match perms.remove_player_group(target, group) {
                    Ok(true) => {
                        NativeCommandOutcome::ok(format!("Removed {target} from group '{group}'"))
                    }
                    Ok(false) => NativeCommandOutcome::err(format!(
                        "Player {target} is not in group '{group}'"
                    )),
                    Err(e) => NativeCommandOutcome::err(e.to_string()),
                }
            }
            ["user", player_arg, "set", node] => {
                let Some(target) = self.resolve_player(player_arg) else {
                    return NativeCommandOutcome::err(format!("Unknown player '{player_arg}'"));
                };
                let (clean_node, allow) = parse_node_flag(node, None);
                let mut perms = write_lock(&self.permissions);
                match perms.set_player_permission(target, clean_node, allow) {
                    Ok(()) => NativeCommandOutcome::ok(format!(
                        "Set '{clean_node}' = {allow} on {target}"
                    )),
                    Err(e) => NativeCommandOutcome::err(e.to_string()),
                }
            }
            ["user", player_arg, "set", node, val] => {
                let Some(target) = self.resolve_player(player_arg) else {
                    return NativeCommandOutcome::err(format!("Unknown player '{player_arg}'"));
                };
                let Ok(allow) = parse_bool_arg(val) else {
                    return NativeCommandOutcome::err(format!("Invalid boolean '{val}'"));
                };
                let (clean_node, allow) = parse_node_flag(node, Some(allow));
                let mut perms = write_lock(&self.permissions);
                match perms.set_player_permission(target, clean_node, allow) {
                    Ok(()) => NativeCommandOutcome::ok(format!(
                        "Set '{clean_node}' = {allow} on {target}"
                    )),
                    Err(e) => NativeCommandOutcome::err(e.to_string()),
                }
            }
            ["user", player_arg, "unset", node] => {
                let Some(target) = self.resolve_player(player_arg) else {
                    return NativeCommandOutcome::err(format!("Unknown player '{player_arg}'"));
                };
                let mut perms = write_lock(&self.permissions);
                match perms.unset_player_permission(target, node) {
                    Ok(true) => NativeCommandOutcome::ok(format!("Unset '{node}' on {target}")),
                    Ok(false) => NativeCommandOutcome::err(format!(
                        "Permission '{node}' was not set on {target}"
                    )),
                    Err(e) => NativeCommandOutcome::err(e.to_string()),
                }
            }
            ["user", player_arg, "check", node] => {
                let Some(target) = self.resolve_player(player_arg) else {
                    return NativeCommandOutcome::err(format!("Unknown player '{player_arg}'"));
                };
                let perms = read_lock(&self.permissions);
                let allowed = perms.has_permission(target, false, node);
                NativeCommandOutcome::ok(format!("{target} has '{node}': {allowed}"))
            }
            _ => NativeCommandOutcome::err(
                "Usage: /perm <group|user> ... (create, delete, weight, parent, set, unset, check)",
            ),
        }
    }

    fn cmd_region(
        &self,
        sender_uuid: Option<Uuid>,
        is_op: bool,
        world: &str,
        pos: [f64; 3],
        args: &[&str],
    ) -> NativeCommandOutcome {
        if !self.check_permission(sender_uuid, is_op, "ferrite.region.admin", false) {
            return NativeCommandOutcome::err("Permission denied (ferrite.region.admin)");
        }

        match args {
            ["define", name] => {
                let Some(uuid) = sender_uuid else {
                    return NativeCommandOutcome::err(
                        "Usage: /rg define <name> <x1> <y1> <z1> <x2> <y2> <z2>",
                    );
                };
                let bounds = read_lock(&self.worldedit).bounds(uuid);
                let Some((c1, c2)) = bounds else {
                    return NativeCommandOutcome::err(
                        "Set WorldEdit /pos1 and /pos2 first, or pass 6 coordinates",
                    );
                };
                let mut rg = write_lock(&self.regions);
                match rg.define_region(world, name, c1, c2) {
                    Ok(()) => NativeCommandOutcome::ok(format!(
                        "Defined region '{name}' in '{world}' from {c1:?} to {c2:?}"
                    )),
                    Err(e) => NativeCommandOutcome::err(e.to_string()),
                }
            }
            ["define", name, x1, y1, z1, x2, y2, z2] => {
                let Ok(c1) = parse_coords(x1, y1, z1) else {
                    return NativeCommandOutcome::err("Invalid corner 1 coordinates");
                };
                let Ok(c2) = parse_coords(x2, y2, z2) else {
                    return NativeCommandOutcome::err("Invalid corner 2 coordinates");
                };
                let mut rg = write_lock(&self.regions);
                match rg.define_region(world, name, c1, c2) {
                    Ok(()) => NativeCommandOutcome::ok(format!(
                        "Defined region '{name}' in '{world}' from {c1:?} to {c2:?}"
                    )),
                    Err(e) => NativeCommandOutcome::err(e.to_string()),
                }
            }
            ["delete" | "remove", name] => {
                let mut rg = write_lock(&self.regions);
                match rg.remove_region(world, name) {
                    Ok(true) => NativeCommandOutcome::ok(format!("Removed region '{name}'")),
                    Ok(false) => {
                        NativeCommandOutcome::err(format!("Region '{name}' not found in '{world}'"))
                    }
                    Err(e) => NativeCommandOutcome::err(e.to_string()),
                }
            }
            ["priority", name, prio_str] => {
                let Ok(priority) = prio_str.parse::<i32>() else {
                    return NativeCommandOutcome::err(format!("Invalid priority '{prio_str}'"));
                };
                let mut rg = write_lock(&self.regions);
                match rg.set_priority(world, name, priority) {
                    Ok(()) => {
                        NativeCommandOutcome::ok(format!("Set priority of '{name}' to {priority}"))
                    }
                    Err(e) => NativeCommandOutcome::err(e.to_string()),
                }
            }
            ["addowner", name, player_arg] => {
                let Some(target) = self.resolve_player(player_arg) else {
                    return NativeCommandOutcome::err(format!("Unknown player '{player_arg}'"));
                };
                let mut rg = write_lock(&self.regions);
                match rg.add_owner(world, name, target) {
                    Ok(_) => {
                        NativeCommandOutcome::ok(format!("Added {target} as owner of '{name}'"))
                    }
                    Err(e) => NativeCommandOutcome::err(e.to_string()),
                }
            }
            ["removeowner", name, player_arg] => {
                let Some(target) = self.resolve_player(player_arg) else {
                    return NativeCommandOutcome::err(format!("Unknown player '{player_arg}'"));
                };
                let mut rg = write_lock(&self.regions);
                match rg.remove_owner(world, name, target) {
                    Ok(true) => NativeCommandOutcome::ok(format!(
                        "Removed {target} from owners of '{name}'"
                    )),
                    Ok(false) => {
                        NativeCommandOutcome::err(format!("{target} is not an owner of '{name}'"))
                    }
                    Err(e) => NativeCommandOutcome::err(e.to_string()),
                }
            }
            ["addmember", name, player_arg] => {
                let Some(target) = self.resolve_player(player_arg) else {
                    return NativeCommandOutcome::err(format!("Unknown player '{player_arg}'"));
                };
                let mut rg = write_lock(&self.regions);
                match rg.add_member(world, name, target) {
                    Ok(_) => {
                        NativeCommandOutcome::ok(format!("Added {target} as member of '{name}'"))
                    }
                    Err(e) => NativeCommandOutcome::err(e.to_string()),
                }
            }
            ["removemember", name, player_arg] => {
                let Some(target) = self.resolve_player(player_arg) else {
                    return NativeCommandOutcome::err(format!("Unknown player '{player_arg}'"));
                };
                let mut rg = write_lock(&self.regions);
                match rg.remove_member(world, name, target) {
                    Ok(true) => NativeCommandOutcome::ok(format!(
                        "Removed {target} from members of '{name}'"
                    )),
                    Ok(false) => {
                        NativeCommandOutcome::err(format!("{target} is not a member of '{name}'"))
                    }
                    Err(e) => NativeCommandOutcome::err(e.to_string()),
                }
            }
            ["flag", name, flag_name, value] => {
                let flag_val = match value.to_ascii_lowercase().as_str() {
                    "allow" | "true" => Some(true),
                    "deny" | "false" => Some(false),
                    "none" | "clear" | "unset" => None,
                    _ => return NativeCommandOutcome::err(format!("Invalid flag value '{value}'")),
                };
                let flag_key = flag_name.to_ascii_lowercase();
                if !matches!(
                    flag_key.as_str(),
                    "build"
                        | "block_break"
                        | "block-break"
                        | "block_place"
                        | "block-place"
                        | "interact"
                        | "pvp"
                        | "explosions"
                        | "entry"
                ) {
                    return NativeCommandOutcome::err(format!("Unknown flag '{flag_name}'"));
                }

                let mut rg = write_lock(&self.regions);
                match rg.update_flags(world, name, |flags| match flag_key.as_str() {
                    "build" => flags.build = flag_val,
                    "block_break" | "block-break" => flags.block_break = flag_val,
                    "block_place" | "block-place" => flags.block_place = flag_val,
                    "interact" => flags.interact = flag_val,
                    "pvp" => flags.pvp = flag_val,
                    "explosions" => flags.explosions = flag_val,
                    "entry" => flags.entry = flag_val,
                    _ => {}
                }) {
                    Ok(()) => NativeCommandOutcome::ok(format!(
                        "Set flag '{flag_key}' = {flag_val:?} on '{name}'"
                    )),
                    Err(e) => NativeCommandOutcome::err(e.to_string()),
                }
            }
            ["info"] => {
                let block = floor_pos(pos);
                let rg = read_lock(&self.regions);
                let hits = rg.regions_at(world, block[0], block[1], block[2]);
                if hits.is_empty() {
                    NativeCommandOutcome::ok("No regions at current position")
                } else {
                    let names: Vec<String> = hits
                        .into_iter()
                        .map(|r| format!("{} (p={})", r.name, r.priority))
                        .collect();
                    NativeCommandOutcome::ok(format!("Regions here: {}", names.join(", ")))
                }
            }
            ["info", name] => {
                let rg = read_lock(&self.regions);
                match rg.get_region(world, name) {
                    Some(r) => NativeCommandOutcome::ok(format!(
                        "Region '{}' in '{}': {:?}..={:?}, priority={}, owners={}, members={}",
                        r.name,
                        r.world,
                        r.min,
                        r.max,
                        r.priority,
                        r.owners.len(),
                        r.members.len()
                    )),
                    None => NativeCommandOutcome::err(format!("Region '{name}' not found")),
                }
            }
            ["list"] => {
                let rg = read_lock(&self.regions);
                let list = rg.list_regions(world);
                if list.is_empty() {
                    NativeCommandOutcome::ok(format!("No regions in '{world}'"))
                } else {
                    NativeCommandOutcome::ok(format!("Regions in '{world}': {}", list.join(", ")))
                }
            }
            _ => NativeCommandOutcome::err(
                "Usage: /rg <define|delete|priority|addowner|addmember|flag|info|list> ...",
            ),
        }
    }

    fn cmd_sethome(
        &self,
        sender_uuid: Option<Uuid>,
        is_op: bool,
        world: &str,
        pos: [f64; 3],
        args: &[&str],
    ) -> NativeCommandOutcome {
        let Some(player) = sender_uuid else {
            return NativeCommandOutcome::err("Only players can set homes");
        };
        if !self.check_permission(sender_uuid, is_op, "ferrite.essentials.sethome", true) {
            return NativeCommandOutcome::err("Permission denied (ferrite.essentials.sethome)");
        }
        let name = args.first().copied().unwrap_or("home");
        if args.len() > 1 {
            return NativeCommandOutcome::err("Usage: /sethome [name]");
        }
        let loc = LocationSpec::from_pos(world, pos);
        let mut ess = write_lock(&self.essentials);
        match ess.set_home(player, name, loc) {
            Ok(()) => NativeCommandOutcome::ok(format!("Home '{name}' set")),
            Err(e) => NativeCommandOutcome::err(e.to_string()),
        }
    }

    fn cmd_home(
        &self,
        sender_uuid: Option<Uuid>,
        is_op: bool,
        args: &[&str],
    ) -> NativeCommandOutcome {
        let Some(player) = sender_uuid else {
            return NativeCommandOutcome::err("Only players can teleport to a home");
        };
        if !self.check_permission(sender_uuid, is_op, "ferrite.essentials.home", true) {
            return NativeCommandOutcome::err("Permission denied (ferrite.essentials.home)");
        }
        let name = args.first().copied().unwrap_or("home");
        if args.len() > 1 {
            return NativeCommandOutcome::err("Usage: /home [name]");
        }
        let ess = read_lock(&self.essentials);
        match ess.home(player, name) {
            Some(loc) => NativeCommandOutcome {
                success: true,
                messages: vec![format!("Teleporting to home '{name}'")],
                teleport: Some(TeleportAction {
                    player,
                    location: loc.clone(),
                }),
                notifications: Vec::new(),
            },
            None => NativeCommandOutcome::err(format!("Home '{name}' not found")),
        }
    }

    fn cmd_delhome(
        &self,
        sender_uuid: Option<Uuid>,
        is_op: bool,
        args: &[&str],
    ) -> NativeCommandOutcome {
        let Some(player) = sender_uuid else {
            return NativeCommandOutcome::err("Only players can delete homes");
        };
        if !self.check_permission(sender_uuid, is_op, "ferrite.essentials.delhome", true) {
            return NativeCommandOutcome::err("Permission denied (ferrite.essentials.delhome)");
        }
        let [name] = args else {
            return NativeCommandOutcome::err("Usage: /delhome <name>");
        };
        let mut ess = write_lock(&self.essentials);
        match ess.del_home(player, name) {
            Ok(true) => NativeCommandOutcome::ok(format!("Deleted home '{name}'")),
            Ok(false) => NativeCommandOutcome::err(format!("Home '{name}' not found")),
            Err(e) => NativeCommandOutcome::err(e.to_string()),
        }
    }

    fn cmd_homes(&self, sender_uuid: Option<Uuid>, is_op: bool) -> NativeCommandOutcome {
        let Some(player) = sender_uuid else {
            return NativeCommandOutcome::err("Only players have homes");
        };
        if !self.check_permission(sender_uuid, is_op, "ferrite.essentials.home", true) {
            return NativeCommandOutcome::err("Permission denied (ferrite.essentials.home)");
        }
        let homes = read_lock(&self.essentials).list_homes(player);
        if homes.is_empty() {
            NativeCommandOutcome::ok("You have no homes set")
        } else {
            NativeCommandOutcome::ok(format!("Homes: {}", homes.join(", ")))
        }
    }

    fn cmd_setwarp(
        &self,
        sender_uuid: Option<Uuid>,
        is_op: bool,
        world: &str,
        pos: [f64; 3],
        args: &[&str],
    ) -> NativeCommandOutcome {
        if !self.check_permission(sender_uuid, is_op, "ferrite.essentials.setwarp", false) {
            return NativeCommandOutcome::err("Permission denied (ferrite.essentials.setwarp)");
        }
        let [name] = args else {
            return NativeCommandOutcome::err("Usage: /setwarp <name>");
        };
        let loc = LocationSpec::from_pos(world, pos);
        let mut ess = write_lock(&self.essentials);
        match ess.set_warp(name, loc) {
            Ok(()) => NativeCommandOutcome::ok(format!("Warp '{name}' set")),
            Err(e) => NativeCommandOutcome::err(e.to_string()),
        }
    }

    fn cmd_warp(
        &self,
        sender_uuid: Option<Uuid>,
        is_op: bool,
        args: &[&str],
    ) -> NativeCommandOutcome {
        if !self.check_permission(sender_uuid, is_op, "ferrite.essentials.warp", true) {
            return NativeCommandOutcome::err("Permission denied (ferrite.essentials.warp)");
        }
        if args.is_empty() {
            return self.cmd_warps(sender_uuid, is_op);
        }
        let Some(player) = sender_uuid else {
            return NativeCommandOutcome::err("Only players can teleport to a warp");
        };
        let [name] = args else {
            return NativeCommandOutcome::err("Usage: /warp [name]");
        };
        let ess = read_lock(&self.essentials);
        match ess.warp(name) {
            Some(loc) => NativeCommandOutcome {
                success: true,
                messages: vec![format!("Warping to '{name}'")],
                teleport: Some(TeleportAction {
                    player,
                    location: loc.clone(),
                }),
                notifications: Vec::new(),
            },
            None => NativeCommandOutcome::err(format!("Warp '{name}' not found")),
        }
    }

    fn cmd_delwarp(
        &self,
        sender_uuid: Option<Uuid>,
        is_op: bool,
        args: &[&str],
    ) -> NativeCommandOutcome {
        if !self.check_permission(sender_uuid, is_op, "ferrite.essentials.delwarp", false) {
            return NativeCommandOutcome::err("Permission denied (ferrite.essentials.delwarp)");
        }
        let [name] = args else {
            return NativeCommandOutcome::err("Usage: /delwarp <name>");
        };
        let mut ess = write_lock(&self.essentials);
        match ess.del_warp(name) {
            Ok(true) => NativeCommandOutcome::ok(format!("Deleted warp '{name}'")),
            Ok(false) => NativeCommandOutcome::err(format!("Warp '{name}' not found")),
            Err(e) => NativeCommandOutcome::err(e.to_string()),
        }
    }

    fn cmd_warps(&self, sender_uuid: Option<Uuid>, is_op: bool) -> NativeCommandOutcome {
        if !self.check_permission(sender_uuid, is_op, "ferrite.essentials.warp", true) {
            return NativeCommandOutcome::err("Permission denied (ferrite.essentials.warp)");
        }
        let warps = read_lock(&self.essentials).list_warps();
        if warps.is_empty() {
            NativeCommandOutcome::ok("No warps set")
        } else {
            NativeCommandOutcome::ok(format!("Warps: {}", warps.join(", ")))
        }
    }

    fn cmd_setspawn(
        &self,
        sender_uuid: Option<Uuid>,
        is_op: bool,
        world: &str,
        pos: [f64; 3],
    ) -> NativeCommandOutcome {
        if !self.check_permission(sender_uuid, is_op, "ferrite.essentials.setspawn", false) {
            return NativeCommandOutcome::err("Permission denied (ferrite.essentials.setspawn)");
        }
        let loc = LocationSpec::from_pos(world, pos);
        write_lock(&self.essentials).set_spawn(loc);
        NativeCommandOutcome::ok(format!("Spawn set in '{world}'"))
    }

    fn cmd_spawn(&self, sender_uuid: Option<Uuid>, is_op: bool) -> NativeCommandOutcome {
        let Some(player) = sender_uuid else {
            return NativeCommandOutcome::err("Only players can teleport to spawn");
        };
        if !self.check_permission(sender_uuid, is_op, "ferrite.essentials.spawn", true) {
            return NativeCommandOutcome::err("Permission denied (ferrite.essentials.spawn)");
        }
        let ess = read_lock(&self.essentials);
        match ess.spawn() {
            Some(loc) => NativeCommandOutcome {
                success: true,
                messages: vec!["Teleporting to spawn".to_owned()],
                teleport: Some(TeleportAction {
                    player,
                    location: loc.clone(),
                }),
                notifications: Vec::new(),
            },
            None => NativeCommandOutcome::err("Global spawn is not set"),
        }
    }

    fn cmd_back(&self, sender_uuid: Option<Uuid>, is_op: bool) -> NativeCommandOutcome {
        let Some(player) = sender_uuid else {
            return NativeCommandOutcome::err("Only players can use /back");
        };
        if !self.check_permission(sender_uuid, is_op, "ferrite.essentials.back", true) {
            return NativeCommandOutcome::err("Permission denied (ferrite.essentials.back)");
        }
        let ess = read_lock(&self.essentials);
        match ess.last_location(player) {
            Some(loc) => NativeCommandOutcome {
                success: true,
                messages: vec!["Returning to previous location".to_owned()],
                teleport: Some(TeleportAction {
                    player,
                    location: loc.clone(),
                }),
                notifications: Vec::new(),
            },
            None => NativeCommandOutcome::err("No previous location recorded"),
        }
    }

    fn cmd_fly(&self, sender_uuid: Option<Uuid>, is_op: bool) -> NativeCommandOutcome {
        let Some(player) = sender_uuid else {
            return NativeCommandOutcome::err("Only players can toggle flight");
        };
        if !self.check_permission(sender_uuid, is_op, "ferrite.essentials.fly", false) {
            return NativeCommandOutcome::err("Permission denied (ferrite.essentials.fly)");
        }
        let enabled = write_lock(&self.essentials).toggle_fly(player);
        NativeCommandOutcome::ok(if enabled {
            "Flight enabled"
        } else {
            "Flight disabled"
        })
    }

    fn cmd_god(&self, sender_uuid: Option<Uuid>, is_op: bool) -> NativeCommandOutcome {
        let Some(player) = sender_uuid else {
            return NativeCommandOutcome::err("Only players can toggle god mode");
        };
        if !self.check_permission(sender_uuid, is_op, "ferrite.essentials.god", false) {
            return NativeCommandOutcome::err("Permission denied (ferrite.essentials.god)");
        }
        let enabled = write_lock(&self.essentials).toggle_god(player);
        NativeCommandOutcome::ok(if enabled {
            "God mode enabled"
        } else {
            "God mode disabled"
        })
    }

    fn cmd_tpa(
        &self,
        sender_uuid: Option<Uuid>,
        is_op: bool,
        args: &[&str],
    ) -> NativeCommandOutcome {
        let Some(requester) = sender_uuid else {
            return NativeCommandOutcome::err("Only players can send TPA requests");
        };
        if !self.check_permission(sender_uuid, is_op, "ferrite.essentials.tpa", true) {
            return NativeCommandOutcome::err("Permission denied (ferrite.essentials.tpa)");
        }
        let [target_arg] = args else {
            return NativeCommandOutcome::err("Usage: /tpa <player>");
        };
        let Some(target) = self.resolve_player(target_arg) else {
            return NativeCommandOutcome::err(format!("Unknown player '{target_arg}'"));
        };
        let mut ess = write_lock(&self.essentials);
        match ess.request_tpa(requester, target) {
            Ok(_) => NativeCommandOutcome {
                success: true,
                messages: vec![format!("TPA request sent to {target_arg}")],
                teleport: None,
                notifications: vec![(
                    target,
                    format!("{requester} requested to teleport to you (/tpaccept or /tpdeny)"),
                )],
            },
            Err(e) => NativeCommandOutcome::err(e.to_string()),
        }
    }

    fn cmd_tpaccept(
        &self,
        sender_uuid: Option<Uuid>,
        is_op: bool,
        world: &str,
        pos: [f64; 3],
        args: &[&str],
    ) -> NativeCommandOutcome {
        let Some(target) = sender_uuid else {
            return NativeCommandOutcome::err("Only players can accept TPA requests");
        };
        if !self.check_permission(sender_uuid, is_op, "ferrite.essentials.tpaccept", true) {
            return NativeCommandOutcome::err("Permission denied (ferrite.essentials.tpaccept)");
        }
        let requester_filter = match args {
            [] => None,
            [arg] => match self.resolve_player(arg) {
                Some(id) => Some(id),
                None => return NativeCommandOutcome::err(format!("Unknown player '{arg}'")),
            },
            _ => return NativeCommandOutcome::err("Usage: /tpaccept [player]"),
        };

        let mut ess = write_lock(&self.essentials);
        match ess.accept_tpa_at(target, requester_filter, EssentialsEngine::now_unix_ms()) {
            Ok(req) => NativeCommandOutcome {
                success: true,
                messages: vec![format!("Accepted TPA request from {}", req.requester)],
                teleport: Some(TeleportAction {
                    player: req.requester,
                    location: LocationSpec::from_pos(world, pos),
                }),
                notifications: vec![(
                    req.requester,
                    "Your teleport request was accepted".to_owned(),
                )],
            },
            Err(e) => NativeCommandOutcome::err(e.to_string()),
        }
    }

    fn cmd_tpdeny(
        &self,
        sender_uuid: Option<Uuid>,
        is_op: bool,
        args: &[&str],
    ) -> NativeCommandOutcome {
        let Some(target) = sender_uuid else {
            return NativeCommandOutcome::err("Only players can deny TPA requests");
        };
        if !self.check_permission(sender_uuid, is_op, "ferrite.essentials.tpdeny", true) {
            return NativeCommandOutcome::err("Permission denied (ferrite.essentials.tpdeny)");
        }
        let requester_filter = match args {
            [] => None,
            [arg] => match self.resolve_player(arg) {
                Some(id) => Some(id),
                None => return NativeCommandOutcome::err(format!("Unknown player '{arg}'")),
            },
            _ => return NativeCommandOutcome::err("Usage: /tpdeny [player]"),
        };

        let mut ess = write_lock(&self.essentials);
        match ess.deny_tpa_at(target, requester_filter, EssentialsEngine::now_unix_ms()) {
            Ok(req) => NativeCommandOutcome {
                success: true,
                messages: vec![format!("Denied TPA request from {}", req.requester)],
                teleport: None,
                notifications: vec![(req.requester, "Your teleport request was denied".to_owned())],
            },
            Err(e) => NativeCommandOutcome::err(e.to_string()),
        }
    }

    fn cmd_bal(
        &self,
        sender_uuid: Option<Uuid>,
        is_op: bool,
        args: &[&str],
    ) -> NativeCommandOutcome {
        if !self.check_permission(sender_uuid, is_op, "ferrite.essentials.balance", true) {
            return NativeCommandOutcome::err("Permission denied (ferrite.essentials.balance)");
        }
        let target = match args {
            [] => match sender_uuid {
                Some(id) => id,
                None => return NativeCommandOutcome::err("Usage: /bal <player>"),
            },
            [arg] => match self.resolve_player(arg) {
                Some(id) => id,
                None => return NativeCommandOutcome::err(format!("Unknown player '{arg}'")),
            },
            _ => return NativeCommandOutcome::err("Usage: /bal [player]"),
        };
        let bal = read_lock(&self.essentials).balance(target);
        NativeCommandOutcome::ok(format!("Balance: ${bal:.2}"))
    }

    fn cmd_pay(
        &self,
        sender_uuid: Option<Uuid>,
        is_op: bool,
        args: &[&str],
    ) -> NativeCommandOutcome {
        let Some(from) = sender_uuid else {
            return NativeCommandOutcome::err("Only players can use /pay");
        };
        if !self.check_permission(sender_uuid, is_op, "ferrite.essentials.pay", true) {
            return NativeCommandOutcome::err("Permission denied (ferrite.essentials.pay)");
        }
        let [target_arg, amount_arg] = args else {
            return NativeCommandOutcome::err("Usage: /pay <player> <amount>");
        };
        let Some(to) = self.resolve_player(target_arg) else {
            return NativeCommandOutcome::err(format!("Unknown player '{target_arg}'"));
        };
        let Ok(amount) = amount_arg.parse::<f64>() else {
            return NativeCommandOutcome::err(format!("Invalid amount '{amount_arg}'"));
        };
        let mut ess = write_lock(&self.essentials);
        match ess.pay(from, to, amount) {
            Ok((from_bal, _)) => NativeCommandOutcome {
                success: true,
                messages: vec![format!(
                    "Paid ${amount:.2} to {target_arg} (new balance: ${from_bal:.2})"
                )],
                teleport: None,
                notifications: vec![(to, format!("You received ${amount:.2} from {from}"))],
            },
            Err(e) => NativeCommandOutcome::err(e.to_string()),
        }
    }

    fn cmd_eco(
        &self,
        sender_uuid: Option<Uuid>,
        is_op: bool,
        args: &[&str],
    ) -> NativeCommandOutcome {
        if !self.check_permission(sender_uuid, is_op, "ferrite.essentials.eco", false) {
            return NativeCommandOutcome::err("Permission denied (ferrite.essentials.eco)");
        }
        match args {
            ["give", player_arg, amount_arg] => {
                let Some(target) = self.resolve_player(player_arg) else {
                    return NativeCommandOutcome::err(format!("Unknown player '{player_arg}'"));
                };
                let Ok(amount) = amount_arg.parse::<f64>() else {
                    return NativeCommandOutcome::err(format!("Invalid amount '{amount_arg}'"));
                };
                match write_lock(&self.essentials).deposit(target, amount) {
                    Ok(bal) => NativeCommandOutcome::ok(format!(
                        "Deposited ${amount:.2} to {target} (balance: ${bal:.2})"
                    )),
                    Err(e) => NativeCommandOutcome::err(e.to_string()),
                }
            }
            ["take", player_arg, amount_arg] => {
                let Some(target) = self.resolve_player(player_arg) else {
                    return NativeCommandOutcome::err(format!("Unknown player '{player_arg}'"));
                };
                let Ok(amount) = amount_arg.parse::<f64>() else {
                    return NativeCommandOutcome::err(format!("Invalid amount '{amount_arg}'"));
                };
                match write_lock(&self.essentials).withdraw(target, amount) {
                    Ok(bal) => NativeCommandOutcome::ok(format!(
                        "Withdrew ${amount:.2} from {target} (balance: ${bal:.2})"
                    )),
                    Err(e) => NativeCommandOutcome::err(e.to_string()),
                }
            }
            _ => NativeCommandOutcome::err("Usage: /eco <give|take> <player> <amount>"),
        }
    }

    fn cmd_mute(
        &self,
        sender_uuid: Option<Uuid>,
        is_op: bool,
        args: &[&str],
    ) -> NativeCommandOutcome {
        if !self.check_permission(sender_uuid, is_op, "ferrite.essentials.mute", false) {
            return NativeCommandOutcome::err("Permission denied (ferrite.essentials.mute)");
        }
        if args.is_empty() {
            return NativeCommandOutcome::err("Usage: /mute <player> [seconds] [reason...]");
        }
        let Some(target) = self.resolve_player(args[0]) else {
            return NativeCommandOutcome::err(format!("Unknown player '{}'", args[0]));
        };
        let (duration, reason) = if args.len() >= 2 {
            if let Ok(secs) = args[1].parse::<u64>() {
                let reason = if args.len() > 2 {
                    Some(args[2..].join(" "))
                } else {
                    None
                };
                (Some(Duration::from_secs(secs)), reason)
            } else {
                (None, Some(args[1..].join(" ")))
            }
        } else {
            (None, None)
        };
        write_lock(&self.essentials).mute(target, reason, duration);
        NativeCommandOutcome::ok(format!("Muted {target}"))
    }

    fn cmd_unmute(
        &self,
        sender_uuid: Option<Uuid>,
        is_op: bool,
        args: &[&str],
    ) -> NativeCommandOutcome {
        if !self.check_permission(sender_uuid, is_op, "ferrite.essentials.mute", false) {
            return NativeCommandOutcome::err("Permission denied (ferrite.essentials.mute)");
        }
        let [player_arg] = args else {
            return NativeCommandOutcome::err("Usage: /unmute <player>");
        };
        let Some(target) = self.resolve_player(player_arg) else {
            return NativeCommandOutcome::err(format!("Unknown player '{player_arg}'"));
        };
        if write_lock(&self.essentials).unmute(target) {
            NativeCommandOutcome::ok(format!("Unmuted {target}"))
        } else {
            NativeCommandOutcome::err(format!("{target} is not muted"))
        }
    }

    fn cmd_pos(
        &self,
        sender_uuid: Option<Uuid>,
        is_op: bool,
        pos: [f64; 3],
        args: &[&str],
        is_pos1: bool,
    ) -> NativeCommandOutcome {
        let Some(player) = sender_uuid else {
            return NativeCommandOutcome::err("Only players can set WorldEdit positions");
        };
        if !self.check_permission(sender_uuid, is_op, "ferrite.worldedit.selection", false) {
            return NativeCommandOutcome::err("Permission denied (ferrite.worldedit.selection)");
        }

        let block_pos = match args {
            [] => floor_pos(pos),
            [x, y, z] => match parse_coords(x, y, z) {
                Ok(c) => c,
                Err(()) => return NativeCommandOutcome::err("Invalid block coordinates"),
            },
            _ => {
                let name = if is_pos1 { "pos1" } else { "pos2" };
                return NativeCommandOutcome::err(format!("Usage: /{name} [x y z]"));
            }
        };

        let mut we = write_lock(&self.worldedit);
        if is_pos1 {
            we.set_pos1(player, block_pos);
        } else {
            we.set_pos2(player, block_pos);
        }

        let label = if is_pos1 { "First" } else { "Second" };
        match we.volume(player) {
            Some(vol) => NativeCommandOutcome::ok(format!(
                "{label} position set to {block_pos:?} ({vol} blocks)"
            )),
            None => NativeCommandOutcome::ok(format!("{label} position set to {block_pos:?}")),
        }
    }
}

fn floor_pos(pos: [f64; 3]) -> [i32; 3] {
    [
        pos[0].floor() as i32,
        pos[1].floor() as i32,
        pos[2].floor() as i32,
    ]
}

fn parse_coords(x: &str, y: &str, z: &str) -> std::result::Result<[i32; 3], ()> {
    let px = x.parse::<i32>().map_err(|_| ())?;
    let py = y.parse::<i32>().map_err(|_| ())?;
    let pz = z.parse::<i32>().map_err(|_| ())?;
    Ok([px, py, pz])
}

fn parse_bool_arg(raw: &str) -> std::result::Result<bool, ()> {
    match raw.to_ascii_lowercase().as_str() {
        "true" | "allow" | "yes" => Ok(true),
        "false" | "deny" | "no" => Ok(false),
        _ => Err(()),
    }
}

fn parse_node_flag(node: &str, explicit: Option<bool>) -> (&str, bool) {
    if let Some(stripped) = node.strip_prefix('-') {
        (stripped, explicit.unwrap_or(false))
    } else {
        (node, explicit.unwrap_or(true))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn dispatch_end_to_end_across_all_engines() {
        let suite = NativePluginSuite::new();
        let admin = Uuid::new_v4();
        let alice = Uuid::new_v4();
        let bob = Uuid::new_v4();
        suite.register_player(alice, "Alice");
        suite.register_player(bob, "Bob");

        // Unrecognized commands fall through (return None).
        assert_eq!(
            suite.dispatch_command(Some(admin), true, "world", [0.0; 3], "/gamemode creative"),
            None
        );

        // Permissions via /perm (non-OP denied, OP allowed).
        let denied = suite
            .dispatch_command(
                Some(alice),
                false,
                "world",
                [0.0; 3],
                "/perm group create vip",
            )
            .unwrap();
        assert!(!denied.success);

        assert!(
            suite
                .dispatch_command(
                    Some(admin),
                    true,
                    "world",
                    [0.0; 3],
                    "/perm group create builder 10"
                )
                .unwrap()
                .success
        );
        assert!(
            suite
                .dispatch_command(
                    Some(admin),
                    true,
                    "world",
                    [0.0; 3],
                    "/perm group builder set ferrite.worldedit.selection true"
                )
                .unwrap()
                .success
        );
        assert!(
            suite
                .dispatch_command(
                    Some(admin),
                    true,
                    "world",
                    [0.0; 3],
                    "/perm user Alice group add builder"
                )
                .unwrap()
                .success
        );

        // WorldEdit /pos1 and /pos2 now succeed for non-OP Alice due to group grant!
        let p1 = suite
            .dispatch_command(Some(alice), false, "world", [-0.5, 64.0, 2.2], "/pos1")
            .unwrap();
        assert!(p1.success);
        let p2 = suite
            .dispatch_command(Some(alice), false, "world", [0.0; 3], "//pos2 2 66 5")
            .unwrap();
        assert!(p2.success);
        assert_eq!(
            read_lock(&suite.worldedit).bounds(alice),
            Some(([-1, 64, 2], [2, 66, 5]))
        );

        // Region define from Alice's selection by admin, plus flags and member.
        assert!(
            suite
                .dispatch_command(
                    Some(admin),
                    true,
                    "world",
                    [0.0; 3],
                    "/rg define spawn 0 60 0 10 80 10"
                )
                .unwrap()
                .success
        );
        assert!(
            suite
                .dispatch_command(
                    Some(admin),
                    true,
                    "world",
                    [0.0; 3],
                    "/rg flag spawn build deny"
                )
                .unwrap()
                .success
        );
        assert!(
            suite
                .dispatch_command(
                    Some(admin),
                    true,
                    "world",
                    [0.0; 3],
                    "/rg addmember spawn Alice"
                )
                .unwrap()
                .success
        );
        assert!(read_lock(&suite.regions).can_break("world", 5, 65, 5, alice));
        assert!(!read_lock(&suite.regions).can_break("world", 5, 65, 5, bob));

        // Essentials: sethome, home, setwarp, warp, setspawn, spawn, tpa, tpaccept, bal, pay.
        assert!(
            suite
                .dispatch_command(
                    Some(alice),
                    false,
                    "world",
                    [12.5, 70.0, -3.5],
                    "/sethome base"
                )
                .unwrap()
                .success
        );
        let home_out = suite
            .dispatch_command(Some(alice), false, "world", [0.0; 3], "/home base")
            .unwrap();
        assert!(home_out.success);
        assert_eq!(
            home_out.teleport.unwrap().location,
            LocationSpec::from_pos("world", [12.5, 70.0, -3.5])
        );

        assert!(
            suite
                .dispatch_command(
                    Some(admin),
                    true,
                    "world",
                    [1.0, 64.0, 1.0],
                    "/setwarp market"
                )
                .unwrap()
                .success
        );
        let warp_out = suite
            .dispatch_command(Some(bob), false, "world", [0.0; 3], "/warp market")
            .unwrap();
        assert!(warp_out.success);
        assert_eq!(
            warp_out.teleport.unwrap().location,
            LocationSpec::from_pos("world", [1.0, 64.0, 1.0])
        );

        assert!(
            suite
                .dispatch_command(Some(admin), true, "world", [0.0, 80.0, 0.0], "/setspawn")
                .unwrap()
                .success
        );
        let spawn_out = suite
            .dispatch_command(Some(bob), false, "world", [50.0, 64.0, 50.0], "/spawn")
            .unwrap();
        assert_eq!(
            spawn_out.teleport.unwrap().location,
            LocationSpec::from_pos("world", [0.0, 80.0, 0.0])
        );

        let tpa_out = suite
            .dispatch_command(Some(bob), false, "world", [0.0; 3], "/tpa Alice")
            .unwrap();
        assert!(tpa_out.success);
        assert_eq!(tpa_out.notifications.len(), 1);

        let tpaccept_out = suite
            .dispatch_command(Some(alice), false, "world", [42.0, 65.0, -9.0], "/tpaccept")
            .unwrap();
        assert!(tpaccept_out.success);
        assert_eq!(
            tpaccept_out.teleport.unwrap(),
            TeleportAction {
                player: bob,
                location: LocationSpec::from_pos("world", [42.0, 65.0, -9.0]),
            }
        );

        write_lock(&suite.essentials).deposit(alice, 100.0).unwrap();
        let pay_out = suite
            .dispatch_command(Some(alice), false, "world", [0.0; 3], "/pay Bob 25.5")
            .unwrap();
        assert!(pay_out.success);
        let bal_out = suite
            .dispatch_command(Some(bob), false, "world", [0.0; 3], "/bal")
            .unwrap();
        assert_eq!(bal_out.messages, vec!["Balance: $25.50".to_owned()]);

        // Directory save + reload preserves all persisted engines.
        let dir = TempDir::new().unwrap();
        suite.save_to_dir(dir.path()).unwrap();
        let reloaded = NativePluginSuite::load_from_dir(dir.path()).unwrap();
        assert!((read_lock(&reloaded.essentials).balance(bob) - 25.5).abs() < 1e-9);
        assert!(!read_lock(&reloaded.regions).can_break("world", 5, 65, 5, bob));
        assert!(read_lock(&reloaded.permissions).has_permission(
            alice,
            false,
            "ferrite.worldedit.selection"
        ));
    }

    #[test]
    fn console_commands_eco_mute_and_selection_region() {
        let suite = NativePluginSuite::new();
        let alice = Uuid::new_v4();
        suite.register_player(alice, "Alice");

        // Console (sender_uuid=None, is_op=true) can run /eco, /mute, /unmute, /perm, /rg.
        assert!(
            suite
                .dispatch_command(None, true, "world", [0.0; 3], "/eco give Alice 75")
                .unwrap()
                .success
        );
        assert!((read_lock(&suite.essentials).balance(alice) - 75.0).abs() < 1e-9);

        assert!(
            suite
                .dispatch_command(None, true, "world", [0.0; 3], "/mute Alice 60 spamming")
                .unwrap()
                .success
        );
        assert!(read_lock(&suite.essentials).is_muted(alice));
        assert!(
            suite
                .dispatch_command(None, true, "world", [0.0; 3], "/unmute Alice")
                .unwrap()
                .success
        );
        assert!(!read_lock(&suite.essentials).is_muted(alice));

        // Player-only commands fail cleanly for console.
        assert!(
            !suite
                .dispatch_command(None, true, "world", [0.0; 3], "/home")
                .unwrap()
                .success
        );

        // /rg define <name> reads the sender's WorldEdit selection.
        let op = Uuid::new_v4();
        assert!(
            suite
                .dispatch_command(Some(op), true, "world", [0.0; 3], "/pos1 1 2 3")
                .unwrap()
                .success
        );
        assert!(
            suite
                .dispatch_command(Some(op), true, "world", [0.0; 3], "/pos2 4 5 6")
                .unwrap()
                .success
        );
        assert!(
            suite
                .dispatch_command(Some(op), true, "world", [0.0; 3], "/rg define box")
                .unwrap()
                .success
        );
        let rg = read_lock(&suite.regions);
        let region = rg.get_region("world", "box").unwrap();
        assert_eq!(region.min, [1, 2, 3]);
        assert_eq!(region.max, [4, 5, 6]);
    }
}
