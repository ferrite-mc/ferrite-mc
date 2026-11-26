//! Lock-free plugin hook profiler (`PluginProfiler`) and `/pluginprofile` command wrapper.
//!
//! Tracks call counts, total time, and max latency per hook category using relaxed `AtomicU64`
//! counters (zero locks and zero heap allocations on hot paths).

use std::fmt::Write as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use uuid::Uuid;

use crate::game::plugin_types::{
    BlockBreak, BlockInteract, BlockPosition, CommandSender, EntityTask, Error, InteractAction,
    ItemSnapshot, PlayerDeath, PlayerDeathOutcome, PlayerLocation, PlayerMove, PlayerSnapshot,
    PluginCommand, PluginCommandResult, PluginMessage, RegionTask, ServerHooks, WorldSnapshot,
};

#[derive(Default)]
struct HookMetric {
    calls: AtomicU64,
    total_ns: AtomicU64,
    max_ns: AtomicU64,
}

impl HookMetric {
    #[inline]
    fn record(&self, elapsed_ns: u64) {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.total_ns.fetch_add(elapsed_ns, Ordering::Relaxed);
        self.max_ns.fetch_max(elapsed_ns, Ordering::Relaxed);
    }

    fn snapshot(&self) -> (u64, u64, u64) {
        (
            self.calls.load(Ordering::Relaxed),
            self.total_ns.load(Ordering::Relaxed),
            self.max_ns.load(Ordering::Relaxed),
        )
    }

    fn reset(&self) {
        self.calls.store(0, Ordering::Relaxed);
        self.total_ns.store(0, Ordering::Relaxed);
        self.max_ns.store(0, Ordering::Relaxed);
    }
}

/// Lock-free metrics collector for plugin hook latency and throughput.
#[derive(Default)]
pub struct PluginProfiler {
    player_move: HookMetric,
    block_break: HookMetric,
    block_place: HookMetric,
    player_interact: HookMetric,
    chat: HookMetric,
    entity_damage: HookMetric,
    command: HookMetric,
    mob_spawn: HookMetric,
    fire_spread: HookMetric,
}

impl PluginProfiler {
    /// Formats a human-readable Timings/Spark-style summary report.
    pub fn format_report(&self) -> String {
        let rows: [(&str, &HookMetric); 9] = [
            ("player_move", &self.player_move),
            ("block_break", &self.block_break),
            ("block_place", &self.block_place),
            ("player_interact", &self.player_interact),
            ("chat", &self.chat),
            ("entity_damage", &self.entity_damage),
            ("command", &self.command),
            ("mob_spawn", &self.mob_spawn),
            ("fire_spread", &self.fire_spread),
        ];
        let mut out = String::from("Ferrite Plugin Profiler (lock-free):\n");
        for (name, metric) in rows {
            let (calls, total_ns, max_ns) = metric.snapshot();
            if calls == 0 {
                continue;
            }
            let avg_us = (total_ns as f64 / calls as f64) / 1_000.0;
            let max_us = max_ns as f64 / 1_000.0;
            let total_ms = total_ns as f64 / 1_000_000.0;
            let _ = writeln!(
                out,
                "  {name:<16} calls={calls:<8} avg={avg_us:>7.2}us  max={max_us:>7.2}us  total={total_ms:>7.2}ms"
            );
        }
        if out.lines().count() == 1 {
            out.push_str("  (no hook calls recorded yet)");
        }
        out
    }

    /// Resets all counters to zero.
    pub fn reset(&self) {
        self.player_move.reset();
        self.block_break.reset();
        self.block_place.reset();
        self.player_interact.reset();
        self.chat.reset();
        self.entity_damage.reset();
        self.command.reset();
        self.mob_spawn.reset();
        self.fire_spread.reset();
    }
}

/// Transparent `ServerHooks` decorator that records lock-free timing metrics and serves
/// `/pluginprofile [reset]`.
pub struct ProfiledHooks {
    inner: Arc<dyn ServerHooks>,
    profiler: Arc<PluginProfiler>,
}

impl ProfiledHooks {
    /// Wraps `inner` with lock-free hook profiling.
    pub fn wrap(inner: Arc<dyn ServerHooks>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            profiler: Arc::new(PluginProfiler::default()),
        })
    }

    /// Returns the shared lock-free profiler.
    pub fn profiler(&self) -> &Arc<PluginProfiler> {
        &self.profiler
    }

    #[inline]
    fn timed<T>(&self, metric: &HookMetric, f: impl FnOnce() -> T) -> T {
        let start = Instant::now();
        let out = f();
        let elapsed = u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX);
        metric.record(elapsed);
        out
    }
}

impl ServerHooks for ProfiledHooks {
    fn player_joined(&self, id: Uuid, name: &str) -> Result<(), Error> {
        self.inner.player_joined(id, name)
    }

    fn player_left(&self, id: Uuid, name: &str) -> Result<(), Error> {
        self.inner.player_left(id, name)
    }

    fn chat(&self, id: Uuid, name: &str, message: &str) -> Result<Option<String>, Error> {
        self.timed(&self.profiler.chat, || self.inner.chat(id, name, message))
    }

    fn player_moved(&self, id: Uuid, name: &str, movement: &PlayerMove) -> Result<bool, Error> {
        self.timed(&self.profiler.player_move, || {
            self.inner.player_moved(id, name, movement)
        })
    }

    fn player_interacted(
        &self,
        id: Uuid,
        name: &str,
        action: InteractAction,
        event: BlockInteract<'_>,
    ) -> Result<bool, Error> {
        self.timed(&self.profiler.player_interact, || {
            self.inner.player_interacted(id, name, action, event)
        })
    }

    fn block_broken(&self, id: Uuid, name: &str, event: BlockBreak<'_>) -> Result<bool, Error> {
        self.timed(&self.profiler.block_break, || {
            self.inner.block_broken(id, name, event)
        })
    }

    fn block_placed(
        &self,
        id: Uuid,
        name: &str,
        block: BlockPosition,
        face: u8,
        placed: &str,
        replaced: &str,
    ) -> Result<bool, Error> {
        self.timed(&self.profiler.block_place, || {
            self.inner
                .block_placed(id, name, block, face, placed, replaced)
        })
    }

    fn entity_changed_block(
        &self,
        id: Uuid,
        kind: &str,
        world: Uuid,
        block: BlockPosition,
        from: &str,
        to: &str,
    ) -> Result<bool, Error> {
        self.inner
            .entity_changed_block(id, kind, world, block, from, to)
    }

    fn blocks_exploded(
        &self,
        world: Uuid,
        center: BlockPosition,
        blocks: &[BlockPosition],
        yield_ratio: f32,
    ) -> Result<Option<Vec<BlockPosition>>, Error> {
        self.inner
            .blocks_exploded(world, center, blocks, yield_ratio)
    }

    fn mob_spawn_allowed(&self, world: &str, x: i32, y: i32, z: i32) -> Result<bool, Error> {
        self.timed(&self.profiler.mob_spawn, || {
            self.inner.mob_spawn_allowed(world, x, y, z)
        })
    }

    fn fire_spread_allowed(&self, world: &str, x: i32, y: i32, z: i32) -> Result<bool, Error> {
        self.timed(&self.profiler.fire_spread, || {
            self.inner.fire_spread_allowed(world, x, y, z)
        })
    }

    fn player_command(&self, id: Uuid, name: &str, message: &str) -> Result<bool, Error> {
        self.inner.player_command(id, name, message)
    }

    fn chunk_loaded(&self, world: Uuid, x: i32, z: i32, new: bool) -> Result<(), Error> {
        self.inner.chunk_loaded(world, x, z, new)
    }

    fn chunk_unloaded(&self, world: Uuid, x: i32, z: i32) -> Result<(), Error> {
        self.inner.chunk_unloaded(world, x, z)
    }

    fn player_died(
        &self,
        id: Uuid,
        name: &str,
        cause: &str,
        death: &PlayerDeath,
    ) -> Result<PlayerDeathOutcome, Error> {
        self.inner.player_died(id, name, cause, death)
    }

    fn player_respawned(
        &self,
        id: Uuid,
        name: &str,
        spawn: [f64; 3],
        yaw: f32,
        pitch: f32,
    ) -> Result<[f64; 3], Error> {
        self.inner.player_respawned(id, name, spawn, yaw, pitch)
    }

    fn entity_damaged(&self, id: Uuid, name: &str, cause: &str, amount: f64) -> Result<f64, Error> {
        self.timed(&self.profiler.entity_damage, || {
            self.inner.entity_damaged(id, name, cause, amount)
        })
    }

    fn entity_died(
        &self,
        id: Uuid,
        kind: &str,
        position: [f64; 3],
        dropped_exp: i32,
    ) -> Result<(), Error> {
        self.inner.entity_died(id, kind, position, dropped_exp)
    }

    fn weather_changing(&self, world: Uuid, to_weather_state: bool) -> Result<bool, Error> {
        self.inner.weather_changing(world, to_weather_state)
    }

    fn world_loaded(&self, world: Uuid) -> Result<(), Error> {
        self.inner.world_loaded(world)
    }

    fn player_level_changed(
        &self,
        id: Uuid,
        name: &str,
        old_level: i32,
        new_level: i32,
    ) -> Result<i32, Error> {
        self.inner
            .player_level_changed(id, name, old_level, new_level)
    }

    fn player_exp_changed(&self, id: Uuid, name: &str, amount: i32) -> Result<i32, Error> {
        self.inner.player_exp_changed(id, name, amount)
    }

    fn inventory_clicked(
        &self,
        id: Uuid,
        name: &str,
        slot: i32,
        click: &str,
        action: &str,
    ) -> Result<bool, Error> {
        self.inner.inventory_clicked(id, name, slot, click, action)
    }

    fn inventory_dragged(
        &self,
        id: Uuid,
        name: &str,
        old_cursor: &ItemSnapshot,
        new_cursor: &ItemSnapshot,
        items: &[(i32, ItemSnapshot)],
    ) -> Result<bool, Error> {
        self.inner
            .inventory_dragged(id, name, old_cursor, new_cursor, items)
    }

    fn inventory_closed(&self, id: Uuid, name: &str) -> Result<(), Error> {
        self.inner.inventory_closed(id, name)
    }

    fn resource_pack_status(
        &self,
        player: Uuid,
        name: &str,
        pack: Uuid,
        action: i32,
    ) -> Result<(), Error> {
        self.inner.resource_pack_status(player, name, pack, action)
    }

    fn plugin_message_received(
        &self,
        player: Uuid,
        name: &str,
        channel: &str,
        data: &[u8],
    ) -> Result<bool, Error> {
        self.inner
            .plugin_message_received(player, name, channel, data)
    }

    fn update_player_location(&self, id: Uuid, location: &PlayerLocation) -> Result<(), Error> {
        self.inner.update_player_location(id, location)
    }

    fn plugin_commands(&self) -> Result<Vec<PluginCommand>, Error> {
        let mut cmds = self.inner.plugin_commands()?;
        cmds.push(PluginCommand {
            name: "pluginprofile".to_owned(),
            aliases: vec!["pprofile".to_owned()],
            permission: Some("ferrite.profile.plugins".to_owned()),
            usage: Some("/pluginprofile [reset]".to_owned()),
        });
        Ok(cmds)
    }

    fn run_plugin_command(
        &self,
        sender: CommandSender,
        line: &str,
    ) -> Result<PluginCommandResult, Error> {
        let trimmed = line.trim().trim_start_matches('/');
        let mut parts = trimmed.split_whitespace();
        if let Some(cmd) = parts.next()
            && matches!(
                cmd.to_ascii_lowercase().as_str(),
                "pluginprofile" | "pprofile"
            )
        {
            let recipient = match sender {
                CommandSender::Console => Uuid::nil(),
                CommandSender::Player(id) => id,
            };
            let reply = if parts
                .next()
                .is_some_and(|arg| arg.eq_ignore_ascii_case("reset"))
            {
                self.profiler.reset();
                "Reset plugin profiler counters.".to_owned()
            } else {
                self.profiler.format_report()
            };
            return Ok(PluginCommandResult {
                handled: true,
                messages: vec![PluginMessage {
                    player: recipient,
                    content: crate::game::plugin_types::MessageContent::Plain(reply),
                }],
            });
        }
        self.timed(&self.profiler.command, || {
            self.inner.run_plugin_command(sender, line)
        })
    }

    fn register_world(&self, world: &WorldSnapshot) -> Result<(), Error> {
        self.inner.register_world(world)
    }

    fn unregister_world(&self, id: Uuid) -> Result<bool, Error> {
        self.inner.unregister_world(id)
    }

    fn register_player(&self, player: &PlayerSnapshot) -> Result<(), Error> {
        self.inner.register_player(player)
    }

    fn inventory_slot_changed(
        &self,
        id: Uuid,
        slot: u8,
        item: Option<&ItemSnapshot>,
        data: Option<&str>,
    ) -> Result<(), Error> {
        self.inner.inventory_slot_changed(id, slot, item, data)
    }

    fn take_region_tasks(&self) -> Vec<RegionTask> {
        self.inner.take_region_tasks()
    }

    fn run_region_task(&self, task: RegionTask) -> Result<(), Error> {
        self.inner.run_region_task(task)
    }

    fn take_entity_tasks(&self) -> Vec<EntityTask> {
        self.inner.take_entity_tasks()
    }

    fn run_entity_task(&self, task: EntityTask) -> Result<(), Error> {
        self.inner.run_entity_task(task)
    }

    fn tick(&self) -> Result<(), Error> {
        self.inner.tick()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoopHooks;
    impl ServerHooks for NoopHooks {
        fn player_joined(&self, _: Uuid, _: &str) -> Result<(), Error> {
            Ok(())
        }
        fn player_left(&self, _: Uuid, _: &str) -> Result<(), Error> {
            Ok(())
        }
        fn chat(&self, _: Uuid, _: &str, msg: &str) -> Result<Option<String>, Error> {
            Ok(Some(msg.to_owned()))
        }
        fn player_moved(&self, _: Uuid, _: &str, _: &PlayerMove) -> Result<bool, Error> {
            Ok(true)
        }
        fn player_interacted(
            &self,
            _: Uuid,
            _: &str,
            _: InteractAction,
            _: BlockInteract<'_>,
        ) -> Result<bool, Error> {
            Ok(true)
        }
        fn block_broken(&self, _: Uuid, _: &str, _: BlockBreak<'_>) -> Result<bool, Error> {
            Ok(true)
        }
        fn block_placed(
            &self,
            _: Uuid,
            _: &str,
            _: BlockPosition,
            _: u8,
            _: &str,
            _: &str,
        ) -> Result<bool, Error> {
            Ok(true)
        }
        fn player_command(&self, _: Uuid, _: &str, _: &str) -> Result<bool, Error> {
            Ok(true)
        }
        fn plugin_commands(&self) -> Result<Vec<PluginCommand>, Error> {
            Ok(Vec::new())
        }
        fn run_plugin_command(
            &self,
            _: CommandSender,
            _: &str,
        ) -> Result<PluginCommandResult, Error> {
            Ok(PluginCommandResult {
                handled: false,
                messages: Vec::new(),
            })
        }
    }

    #[test]
    fn profiled_hooks_records_metrics_and_handles_pluginprofile_command() {
        let hooks = ProfiledHooks::wrap(Arc::new(NoopHooks));
        assert!(
            hooks
                .mob_spawn_allowed("minecraft:overworld", 0, 64, 0)
                .unwrap()
        );
        assert_eq!(
            hooks.chat(Uuid::nil(), "Test", "hi").unwrap().as_deref(),
            Some("hi")
        );

        let res = hooks
            .run_plugin_command(CommandSender::Console, "/pluginprofile")
            .unwrap();
        assert!(res.handled);
        let crate::game::plugin_types::MessageContent::Plain(ref text) = res.messages[0].content
        else {
            panic!("expected plain report");
        };
        assert!(text.contains("mob_spawn"));
        assert!(text.contains("chat"));

        let reset = hooks
            .run_plugin_command(CommandSender::Console, "/pluginprofile reset")
            .unwrap();
        assert!(reset.handled);
        assert!(hooks.profiler().format_report().contains("no hook calls"));
    }
}
