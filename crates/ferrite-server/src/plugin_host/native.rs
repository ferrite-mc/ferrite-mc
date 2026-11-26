//! Zero-overhead `ServerHooks` adapter backed by [`ferrite_native_plugins::NativePluginSuite`].
//!
//! Runs completely in-process in Rust (no JVM, no JNI) and can be layered ahead of the embedded
//! JVM [`super::HostHooks`] via [`super::CompositeHooks`].

use std::path::PathBuf;
use std::sync::Arc;

use ferrite_native_plugins::NativePluginSuite;

use crate::game::plugin_types::{
    BlockBreak, BlockInteract, BlockPosition, CommandSender, Error, InteractAction, MessageContent,
    PlayerMove, PluginCommand, PluginCommandResult, PluginMessage, ServerHooks,
};

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
];

/// `ServerHooks` implementation backed by [`NativePluginSuite`].
pub struct NativePluginHooks {
    suite: Arc<NativePluginSuite>,
    data_dir: Option<PathBuf>,
}

impl NativePluginHooks {
    /// Wraps an existing [`NativePluginSuite`].
    pub fn new(suite: Arc<NativePluginSuite>, data_dir: Option<PathBuf>) -> Self {
        Self { suite, data_dir }
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
        Self {
            suite: Arc::new(suite),
            data_dir: Some(data_dir),
        }
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

    fn has_region_bypass(&self, id: uuid::Uuid) -> bool {
        self.suite
            .permissions
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .has_permission(id, false, "ferrite.regions.bypass")
    }
}

impl ServerHooks for NativePluginHooks {
    fn player_joined(&self, id: uuid::Uuid, name: &str) -> Result<(), Error> {
        self.suite.register_player(id, name);
        Ok(())
    }

    fn player_left(&self, _id: uuid::Uuid, name: &str) -> Result<(), Error> {
        self.suite.unregister_player(name);
        Ok(())
    }

    fn chat(&self, id: uuid::Uuid, _name: &str, message: &str) -> Result<Option<String>, Error> {
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

    fn player_moved(
        &self,
        id: uuid::Uuid,
        _name: &str,
        movement: &PlayerMove,
    ) -> Result<bool, Error> {
        if self.has_region_bypass(id) {
            return Ok(true);
        }
        let x = movement.to[0].floor() as i32;
        let y = movement.to[1].floor() as i32;
        let z = movement.to[2].floor() as i32;
        let allowed = self
            .suite
            .regions
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .can_enter("minecraft:overworld", x, y, z, id);
        Ok(allowed)
    }

    fn player_interacted(
        &self,
        id: uuid::Uuid,
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

    fn block_broken(
        &self,
        id: uuid::Uuid,
        _name: &str,
        event: BlockBreak<'_>,
    ) -> Result<bool, Error> {
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
        id: uuid::Uuid,
        _name: &str,
        block: BlockPosition,
        _face: u8,
        _placed: &str,
        _replaced: &str,
    ) -> Result<bool, Error> {
        if self.has_region_bypass(id) {
            return Ok(true);
        }
        let allowed = self
            .suite
            .regions
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .can_place("minecraft:overworld", block.x, block.y, block.z, id);
        Ok(allowed)
    }

    fn blocks_exploded(
        &self,
        _world: uuid::Uuid,
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

    fn player_command(&self, _id: uuid::Uuid, _name: &str, _message: &str) -> Result<bool, Error> {
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
        let (sender_uuid, is_op, recipient) = match sender {
            CommandSender::Console => (None, true, uuid::Uuid::nil()),
            CommandSender::Player(uuid) => {
                let op = self
                    .suite
                    .permissions
                    .read()
                    .unwrap_or_else(|p| p.into_inner())
                    .has_permission(uuid, false, "*");
                (Some(uuid), op, uuid)
            }
        };
        let Some(outcome) = self.suite.dispatch_command(
            sender_uuid,
            is_op,
            "minecraft:overworld",
            [0.0, 64.0, 0.0],
            line,
        ) else {
            return Ok(PluginCommandResult {
                handled: false,
                messages: Vec::new(),
            });
        };
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
    fn native_hooks_enforce_regions_mutes_and_commands_without_jvm() {
        let suite = Arc::new(NativePluginSuite::new());
        let owner = uuid::Uuid::from_u128(1);
        let intruder = uuid::Uuid::from_u128(2);

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

        let res = hooks
            .run_plugin_command(CommandSender::Player(owner), "/sethome base")
            .unwrap();
        assert!(res.handled);
        assert!(!res.messages.is_empty());
    }
}
