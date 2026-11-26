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
    ("pos1", &[]),
    ("pos2", &[]),
    ("set", &[]),
    ("replace", &[]),
    ("walls", &[]),
    ("copy", &[]),
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
            "set" | "replace" | "walls" | "copy" | "paste" | "undo" | "redo"
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
            Ok(plan) => {
                let mut applied = 0usize;
                for (block_pos, state) in plan {
                    if source
                        .set_block(block_pos[0], block_pos[1], block_pos[2], state)
                        .is_ok()
                    {
                        applied += 1;
                    }
                }
                Some(format!("Applied {applied} block updates ({cmd})."))
            }
            Err(err) => Some(format!("WorldEdit error: {err}")),
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
        Ok(Some(message.to_owned()))
    }

    fn player_moved(&self, id: Uuid, _name: &str, movement: &PlayerMove) -> Result<bool, Error> {
        let (world, _) = self.player_world_and_pos(id);
        if !self.has_region_bypass(id) {
            let x = movement.to[0].floor() as i32;
            let y = movement.to[1].floor() as i32;
            let z = movement.to[2].floor() as i32;
            let allowed = self
                .suite
                .regions
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .can_enter(&world, x, y, z, id);
            if !allowed {
                return Ok(false);
            }
        }
        self.player_positions
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id, (world, movement.to));
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
        if self.has_region_bypass(id) {
            return Ok(true);
        }
        let allowed = self
            .suite
            .regions
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .can_interact(event.world, event.block.x, event.block.y, event.block.z, id);
        Ok(allowed)
    }

    fn block_broken(&self, id: Uuid, _name: &str, event: BlockBreak<'_>) -> Result<bool, Error> {
        if self.has_region_bypass(id) {
            return Ok(true);
        }
        let allowed = self
            .suite
            .regions
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .can_break(event.world, event.block.x, event.block.y, event.block.z, id);
        Ok(allowed)
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
        if self.has_region_bypass(id) {
            return Ok(true);
        }
        let (world, _) = self.player_world_and_pos(id);
        let allowed = self
            .suite
            .regions
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .can_place(&world, block.x, block.y, block.z, id);
        Ok(allowed)
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

        let Some(outcome) = self
            .suite
            .dispatch_command(sender_uuid, is_op, &world, pos, line)
        else {
            return Ok(PluginCommandResult {
                handled: false,
                messages: Vec::new(),
            });
        };

        if let Some(tp) = outcome.teleport
            && let Some(tx) = self
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

        let undo_res = hooks
            .run_plugin_command(CommandSender::Player(builder), "//undo")
            .unwrap();
        assert!(undo_res.handled);
        assert_eq!(source.block(2, 70, 2), Some(air));
        assert_eq!(source.block(3, 70, 2), Some(air));
    }
}
