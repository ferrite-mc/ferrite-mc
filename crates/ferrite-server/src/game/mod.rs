mod anvil;
mod attributes;
mod slot_ops;
mod structure_template;
pub use anvil::{AnvilChunkSource, read_spawn as load_spawn};
#[cfg(test)]
mod block_tests;
mod blocks;
mod generator;
#[cfg(test)]
mod persistence_tests;
pub use generator::{ChunkGenerator, Generated};
mod vanilla_gen;
pub use vanilla_gen::{HostSettings, VanillaGenerator};
pub mod playerdata;
pub use brewing::BrewingData;
pub fn new_anvil_source(
    path: std::path::PathBuf,
    registry: ferrite_world::RuntimeRegistry,
) -> Arc<AnvilChunkSource> {
    AnvilChunkSource::new(path, registry)
}
pub fn new_anvil_source_with(
    path: std::path::PathBuf,
    registry: ferrite_world::RuntimeRegistry,
    terrain: Option<Arc<dyn ChunkGenerator>>,
) -> Arc<AnvilChunkSource> {
    AnvilChunkSource::new_with_terrain(path, registry, terrain)
}
pub fn new_memory_source(terrain: Option<Arc<dyn ChunkGenerator>>) -> Arc<AnvilChunkSource> {
    AnvilChunkSource::new_memory_with_terrain(terrain)
}
pub mod beacon;
mod block_container;
mod brewing;
mod chat;
mod chunk;
mod command_block;
mod commands;
mod dimension;
mod portal;
pub use dimension::{Dimension, DimensionRegistry};
mod commands_data;
mod commands_exec;
mod commands_extras;
mod commands_item;
mod commands_late;
mod commands_loot;
mod commands_meta;
mod commands_more;
mod commands_progress;
mod commands_server;
mod commands_world;
mod container;
mod crafting;
pub use crafting::match_for_plugin as match_crafting_for_plugin;
mod dispenser;
mod drops;
mod entity_index;
mod entity_save;
pub use explosion::{CHARGED_CREEPER_POWER, CREEPER_POWER, TNT_POWER};
mod explosion;
mod feedback;
mod functions;
pub mod interaction;
mod nbt_path;
mod profile;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn movement_validation_rejects_large_and_nonfinite_moves() {
        assert!(valid_move([0.0, 65.0, 0.0], [5.0, 65.0, 0.0]));
        assert!(!valid_move([0.0, 65.0, 0.0], [101.0, 65.0, 0.0]));
        assert!(!valid_move([0.0, 65.0, 0.0], [f64::NAN, 65.0, 0.0]));
    }

    fn routed(command: ferrite_plugin::PlayerCommand, player: Uuid) -> Vec<Outbound> {
        let (outbound, mut rx) = OutboundSender::channel(1 << 20);
        let mut mailboxes = HashMap::new();
        mailboxes.insert(player, (1, Mailbox::default(), outbound));
        route_plugin_command(command, &mailboxes, &mut HashMap::new());
        std::iter::from_fn(|| rx.try_recv()).collect()
    }

    #[test]
    fn plugin_entity_tasks_queue_on_the_owning_player() {
        let player = Uuid::from_u128(7);
        let (outbound, _rx) = OutboundSender::channel(1 << 20);
        let mailbox = Mailbox::default();
        let mut mailboxes = HashMap::new();
        mailboxes.insert(player, (1, mailbox.clone(), outbound));
        let task = |entity: String| ferrite_plugin::EntityTask {
            id: 3,
            entity,
            region_aware: true,
        };
        route_entity_task(task(player.to_string()), &mailboxes);
        route_entity_task(task(Uuid::from_u128(8).to_string()), &mailboxes);
        route_entity_task(task("not-a-uuid".into()), &mailboxes);
        let queued: Vec<_> = mailbox.lock().unwrap().drain(..).collect();
        assert!(
            matches!(queued.as_slice(), [Command::PluginEntityTask(t)] if *t == task(player.to_string())),
            "only the online player's task is queued"
        );
    }

    fn bytes(message: &Outbound) -> Vec<u8> {
        match message {
            Outbound::Packet(bytes) => bytes.to_vec(),
            Outbound::Disconnect(reason) => panic!("unexpected disconnect: {reason}"),
        }
    }

    #[test]
    fn plugin_visual_experience_reaches_only_the_target() {
        let player = Uuid::from_u128(7);
        let mailbox = Mailbox::default();
        let (outbound, _rx) = OutboundSender::channel(1 << 20);
        let mut mailboxes = HashMap::new();
        mailboxes.insert(player, (1, mailbox.clone(), outbound));
        for target in [Uuid::from_u128(8), player] {
            route_plugin_command(
                ferrite_plugin::PlayerCommand::VisualExperience {
                    player: target,
                    progress: 0.75,
                    level: 12,
                },
                &mailboxes,
                &mut HashMap::new(),
            );
        }
        let commands: Vec<_> = mailbox.lock().unwrap().drain(..).collect();
        assert!(
            matches!(commands.as_slice(), [Command::PluginVisualExperience { progress, level: Some(12) }] if *progress == 0.75)
        );
    }

    #[test]
    fn plugin_completions_reach_only_the_target() {
        let player = Uuid::from_u128(7);
        let entries = vec!["hello".into(), "".into()];
        let command = ferrite_plugin::PlayerCommand::CustomChatCompletions {
            player,
            action: 2,
            entries: entries.clone(),
        };
        assert_eq!(
            routed(command.clone(), player)
                .iter()
                .map(bytes)
                .collect::<Vec<_>>(),
            vec![bytes(&game::packet(&CustomChatCompletions {
                action: VarInt(2),
                entries
            }))]
        );
        assert!(routed(command, Uuid::from_u128(8)).is_empty());
    }

    #[test]
    fn plugin_virtual_sign_reaches_only_the_target() {
        let player = Uuid::from_u128(7);
        let position = ferrite_protocol::types::BlockPos::new(1, 64, -2).pack();
        let command = ferrite_plugin::PlayerCommand::OpenVirtualSign {
            player,
            position,
            front: false,
        };
        assert_eq!(
            routed(command.clone(), player)
                .iter()
                .map(bytes)
                .collect::<Vec<_>>(),
            vec![bytes(&game::packet(&OpenSignEditor {
                location: position,
                is_front_text: false
            }))]
        );
        assert!(routed(command, Uuid::from_u128(8)).is_empty());
    }

    #[test]
    fn plugin_block_damage_reaches_only_the_target() {
        let player = Uuid::from_u128(7);
        let position = ferrite_protocol::types::BlockPos::new(1, 64, 2).pack();
        let command = ferrite_plugin::PlayerCommand::VisualBlockDamage {
            player,
            position,
            source: 99,
            progress: 5,
        };
        let sent = routed(command.clone(), player);
        assert_eq!(
            sent.iter().map(bytes).collect::<Vec<_>>(),
            vec![bytes(&game::packet(&BlockDestruction {
                entity_id: VarInt(99),
                position,
                progress: 5
            }))]
        );
        assert!(routed(command, Uuid::from_u128(8)).is_empty());
    }

    #[test]
    fn plugin_visual_health_reaches_only_the_target() {
        let player = Uuid::from_u128(7);
        let command = ferrite_plugin::PlayerCommand::VisualHealth {
            player,
            health: 12.5,
            food: 9,
            saturation: 2.5,
        };
        let sent = routed(command.clone(), player);
        assert_eq!(
            sent.iter().map(bytes).collect::<Vec<_>>(),
            vec![bytes(&game::packet(&SetHealth {
                health: 12.5,
                food: VarInt(9),
                saturation: 2.5
            }))]
        );
        assert!(routed(command, Uuid::from_u128(8)).is_empty());
    }

    #[test]
    fn plugin_action_bar_clear_title_and_tab_list_reach_the_player() {
        let player = Uuid::from_u128(7);
        let plain = |text: &str| ferrite_protocol::types::PlainText(text.into());
        let bar = routed(
            ferrite_plugin::PlayerCommand::ActionBar {
                player,
                component_json: r#"{"text":"Low ","extra":["health"]}"#.into(),
            },
            player,
        );
        assert_eq!(
            bar.iter().map(bytes).collect::<Vec<_>>(),
            [game::packet(&SetActionBarText {
                text: plain("Low health")
            })]
            .iter()
            .map(bytes)
            .collect::<Vec<_>>()
        );
        for reset in [false, true] {
            let cleared = routed(
                ferrite_plugin::PlayerCommand::ClearTitle { player, reset },
                player,
            );
            assert_eq!(
                cleared.iter().map(bytes).collect::<Vec<_>>(),
                vec![bytes(&game::packet(&ClearTitles { reset }))]
            );
        }
        let list = routed(
            ferrite_plugin::PlayerCommand::ListHeaderFooter {
                player,
                header_json: r#"{"text":"Ferrite"}"#.into(),
                footer_json: r#"{"text":""}"#.into(),
            },
            player,
        );
        assert_eq!(
            list.iter().map(bytes).collect::<Vec<_>>(),
            vec![bytes(&game::packet(&TabList {
                header: plain("Ferrite"),
                footer: plain(""),
            }))]
        );
    }

    #[test]
    fn plugin_title_sends_times_then_texts() {
        let player = Uuid::from_u128(7);
        let sent = routed(
            ferrite_plugin::PlayerCommand::Title {
                player,
                title: Some("Hello".into()),
                subtitle: Some("World".into()),
                fade_in: 10,
                stay: 70,
                fade_out: 20,
            },
            player,
        );
        let expected = [
            game::packet(&SetTitlesAnimation {
                fade_in: 10,
                stay: 70,
                fade_out: 20,
            }),
            game::packet(&SetTitleText {
                text: ferrite_protocol::types::PlainText("Hello".into()),
            }),
            game::packet(&SetSubtitleText {
                text: ferrite_protocol::types::PlainText("World".into()),
            }),
        ];
        assert_eq!(
            sent.iter().map(bytes).collect::<Vec<_>>(),
            expected.iter().map(bytes).collect::<Vec<_>>()
        );
        // A missing title or subtitle sends only the timings.
        let sent = routed(
            ferrite_plugin::PlayerCommand::Title {
                player,
                title: None,
                subtitle: None,
                fade_in: 1,
                stay: 2,
                fade_out: 3,
            },
            player,
        );
        assert_eq!(sent.len(), 1);
    }

    #[test]
    fn plugin_kick_disconnects_with_the_reason() {
        let player = Uuid::from_u128(8);
        let kick = |component_json: Option<&str>| {
            let sent = routed(
                ferrite_plugin::PlayerCommand::Kick {
                    player,
                    component_json: component_json.map(str::to_owned),
                },
                player,
            );
            match sent.as_slice() {
                [Outbound::Disconnect(reason)] => reason.clone(),
                other => panic!("expected one disconnect, got {other:?}"),
            }
        };
        assert_eq!(kick(Some(r#"{"text":"Go ","extra":["away"]}"#)), "Go away");
        assert_eq!(kick(None), "Kicked by an operator");
    }

    #[test]
    fn plugin_sound_uses_the_registry_holder_and_category() {
        let player = Uuid::from_u128(9);
        let sound = |world: Uuid, name: &str| {
            routed(
                ferrite_plugin::PlayerCommand::PlaySound {
                    player,
                    world,
                    sound: name.into(),
                    category: ferrite_plugin::SoundCategory::Players,
                    x: 1.5,
                    y: 64.0,
                    z: -2.25,
                    volume: 0.8,
                    pitch: 1.2,
                    seed: Some(42),
                },
                player,
            )
        };
        let id = ferrite_data::registries::id(
            "minecraft:sound_event",
            "minecraft:entity.player.levelup",
        )
        .unwrap();
        let expected = game::packet(&SoundEffect {
            sound: VarInt(id + 1),
            source: VarInt(7),
            x: 12,
            y: 512,
            z: -18,
            volume: 0.8,
            pitch: 1.2,
            seed: 42,
        });
        let world = ferrite_plugin::default_world_id();
        let sent = sound(world, "minecraft:entity.player.levelup");
        assert_eq!(
            sent.iter().map(bytes).collect::<Vec<_>>(),
            vec![bytes(&expected)]
        );
        assert!(sound(world, "example:not_a_sound").is_empty());
        assert!(sound(Uuid::from_u128(99), "minecraft:entity.player.levelup").is_empty());
    }
}
fn valid_move(from: [f64; 3], to: [f64; 3]) -> bool {
    to.iter().all(|v| v.is_finite() && v.abs() <= 30_000_000.0)
        && from
            .iter()
            .zip(to)
            .map(|(a, b)| (a - b) * (a - b))
            .sum::<f64>()
            <= 10_000.0
}
mod fire;
mod fluid;
mod furnace;
mod global;
mod gravity;
mod hopper;
mod neighbors;
mod random_tick;
mod redstone;
mod region;
mod selectors;
mod survival;

use self::{
    global::{GlobalPlayer, GlobalRegion},
    region::{Command, Mailbox, Player, Region, Signal, TickContext},
};
use ferrite_admin::AdminLists;
use ferrite_net::game::{self, GameMessage, Outbound, OutboundSender, ServerboundPlay};
use ferrite_protocol::{Encode, VarInt, packets::play::*};
use ferrite_region::{ChunkPos, Regionizer, Scheduler};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;
#[derive(Debug, Clone)]
pub struct Operator {
    pub uuid: Uuid,
    pub level: u8,
}
/// What a failed plugin event (for example a JVM exception) means for the action it guards.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PluginEventPolicy {
    /// Bukkit-compatible (the default): the action proceeds as if nothing was cancelled.
    #[default]
    Allow,
    /// Treat the failure like a cancellation: undo the action and resend block/teleport state.
    Deny,
}

pub struct WorldSettings {
    pub gamemode: i32,
    /// Which dimension this world is; `Dimension::Overworld` unless configured otherwise.
    pub dimension: Dimension,
    /// Whether it is currently raining, for fire and cauldron rules.
    pub raining: bool,
    pub max_players: i32,
    pub ops: Vec<Operator>,
    /// Plugin event and command hooks; `None` when the server runs without plugins.
    pub plugins: Option<Arc<dyn ferrite_plugin::ServerHooks>>,
    /// What a failed plugin event means for the action it guards.
    pub plugin_event_errors: PluginEventPolicy,
    /// Vanilla `ops.json`/`whitelist.json`/ban lists, when the server has a data directory.
    pub admin: Option<Arc<AdminLists>>,
    /// Whether the whitelist starts enabled.
    pub whitelist: bool,
    /// Where `playerdata/<uuid>.json` lives. `None` keeps no player state, which is what test
    /// worlds use: one process-wide directory made a bot resume the gamemode and position a
    /// same-named bot from another test world had saved.
    pub playerdata: Option<std::path::PathBuf>,
    /// Player mutations plugins made through the Bukkit API, delivered to the owning region.
    pub plugin_commands: Option<mpsc::UnboundedReceiver<ferrite_plugin::PlayerCommand>>,
    /// Per-dimension chunk sources. `None` opens the standard three dimensions from the
    /// primary source's world directory (or in memory for a world without one).
    pub dimensions: Option<Arc<DimensionRegistry>>,
}
impl WorldSettings {
    fn permission(&self, uuid: Uuid) -> u8 {
        self.ops
            .iter()
            .find(|op| op.uuid == uuid)
            .map_or(0, |op| op.level)
    }
}
impl Default for WorldSettings {
    fn default() -> Self {
        Self {
            gamemode: 1,
            dimension: Dimension::Overworld,
            raining: false,
            max_players: 20,
            ops: Vec::new(),
            plugins: None,
            plugin_event_errors: PluginEventPolicy::Allow,
            admin: None,
            whitelist: false,
            playerdata: None,
            plugin_commands: None,
            dimensions: None,
        }
    }
}
pub struct World {
    pub sender: mpsc::UnboundedSender<GameMessage>,
    /// The world loop's region signal channel (tests inject entities through it).
    #[cfg_attr(not(test), allow(dead_code))]
    signals: mpsc::UnboundedSender<Signal>,
    task: tokio::task::JoinHandle<()>,
    pub tick_samples: Arc<Mutex<Vec<f64>>>,
    profiler: Arc<profile::Profiler>,
    random_tick_speed: Arc<AtomicU32>,
    source: Arc<anvil::AnvilChunkSource>,
    dimensions: Arc<DimensionRegistry>,
    stop: tokio::sync::watch::Receiver<bool>,
    game_rules: Arc<Mutex<ferrite_command::gamerule::GameRules>>,
}
impl World {
    pub fn start() -> Self {
        Self::start_with(None, [0, 65, 0])
    }

    /// Tick statistics and profiling sessions recorded by this world.
    pub fn profiler(&self) -> Arc<profile::Profiler> {
        Arc::clone(&self.profiler)
    }
    /// Chunk packet cache counters (encodes, hit rate) for this world.
    pub fn cache_stats(&self) -> anvil::CacheStats {
        self.source.cache_stats()
    }
    /// The per-dimension chunk sources of this world.
    pub fn dimensions(&self) -> &Arc<DimensionRegistry> {
        &self.dimensions
    }
    pub fn start_with(source: Option<Arc<anvil::AnvilChunkSource>>, spawn: [i32; 3]) -> Self {
        Self::start_configured(source, spawn, WorldSettings::default())
    }
    pub fn start_configured(
        source: Option<Arc<anvil::AnvilChunkSource>>,
        spawn: [i32; 3],
        settings: WorldSettings,
    ) -> Self {
        let flat = source.as_ref().is_none_or(|source| source.is_flat());
        let source = source.unwrap_or_else(anvil::AnvilChunkSource::new_memory);
        let dimensions = settings.dimensions.clone().unwrap_or_else(|| {
            DimensionRegistry::with_source(settings.dimension, Arc::clone(&source), spawn)
        });
        let (sender, rx) = mpsc::unbounded_channel();
        let (signals, signal_rx) = mpsc::unbounded_channel();
        drops::preload();
        crafting::preload();
        let (stop_tx, stop) = tokio::sync::watch::channel(false);
        let tick_samples = Arc::new(Mutex::new(Vec::new()));
        let profiler = Arc::new(profile::Profiler::new());
        let rules = source.load_game_rules().unwrap_or_default();
        let random_tick_speed = Arc::new(AtomicU32::new(
            rules.get_integer("random_tick_speed").unwrap_or(3).max(0) as u32,
        ));
        let game_rules = Arc::new(Mutex::new(rules));
        crate::plugins::rules::register(Arc::clone(&game_rules));
        let metrics = LoopMetrics {
            game_rules: Arc::clone(&game_rules),
            random_tick_speed: Arc::clone(&random_tick_speed),
            tick_samples: tick_samples.clone(),
            profiler: Arc::clone(&profiler),
            stop: stop_tx,
        };
        let task = tokio::spawn(run(
            Channels {
                events: rx,
                event_tx: sender.clone(),
                signal_tx: signals.clone(),
                signals: signal_rx,
            },
            Some(Arc::clone(&source)),
            Arc::clone(&dimensions),
            flat,
            spawn,
            metrics,
            settings,
        ));
        Self {
            sender,
            signals,
            task,
            tick_samples,
            profiler,
            random_tick_speed,
            source,
            dimensions,
            stop,
            game_rules,
        }
    }
    pub fn set_random_tick_speed(&self, speed: u32) {
        if let Err(error) = apply_game_rule(
            &self.game_rules,
            Some(&self.source),
            &self.random_tick_speed,
            "random_tick_speed",
            Some(&speed.to_string()),
        ) {
            tracing::warn!(%error, "setting random tick speed failed");
        }
    }

    /// A signal that becomes `true` once `/stop` (or shutdown) was requested.
    pub fn stop_signal(&self) -> tokio::sync::watch::Receiver<bool> {
        self.stop.clone()
    }

    pub fn stop_requested(&self) -> bool {
        *self.stop.borrow()
    }
    pub async fn console_command(&self, command: String) -> anyhow::Result<bool> {
        let (complete, done) = oneshot::channel();
        self.sender
            .send(GameMessage::Console { command, complete })
            .map_err(|_| anyhow::anyhow!("game loop stopped"))?;
        done.await
            .map_err(|_| anyhow::anyhow!("console response lost"))?
            .map_err(anyhow::Error::msg)
    }
    pub async fn shutdown(self) -> anyhow::Result<()> {
        let result: anyhow::Result<()> = if *self.stop.borrow() {
            // `/stop` already queued the shutdown event; just wait for the loop to finish.
            Ok(())
        } else {
            let (complete, done) = oneshot::channel();
            self.sender
                .send(GameMessage::Shutdown { complete })
                .map_err(|_| anyhow::anyhow!("game loop stopped"))?;
            done.await
                .map_err(|_| anyhow::anyhow!("shutdown response lost"))?
                .map_err(anyhow::Error::msg)
        };
        self.task.await?;
        result
    }
}
fn apply_game_rule(
    rules: &Mutex<ferrite_command::gamerule::GameRules>,
    source: Option<&Arc<anvil::AnvilChunkSource>>,
    speed: &AtomicU32,
    name: &str,
    value: Option<&str>,
) -> Result<String, String> {
    use ferrite_command::gamerule::RuleValue;
    let name = ferrite_command::gamerule::canonical_name(name);
    let mut rules = rules.lock().map_err(|error| error.to_string())?;
    let result = if let Some(value) = value {
        let mut updated = rules.clone();
        let result = updated.set(&name, value)?;
        if let Some(source) = source {
            source
                .save_game_rules(&updated)
                .map_err(|error| format!("failed to save game rules: {error:#}"))?;
        }
        *rules = updated;
        if name == "random_tick_speed"
            && let RuleValue::Integer(value) = result
        {
            speed.store(value.max(0) as u32, Ordering::Relaxed);
        }
        result
    } else {
        rules
            .get(&name)
            .ok_or_else(|| format!("Unknown game rule: {name}"))?
    };
    Ok(format!("{name} = {}", result.text()))
}
type TransientEntities =
    Arc<Mutex<HashMap<(Dimension, ChunkPos), Vec<ferrite_entity::EntityRecord>>>>;

/// One region scheduler per dimension: Folia's regionizer is per world, so chunks from two
/// dimensions never share ownership.
struct Worlds {
    schedulers: HashMap<Dimension, Scheduler<Region, GlobalRegion>>,
    transient_entities: Option<TransientEntities>,
    tracking: region::Tracking,
}

impl Worlds {
    fn set_period(&self, period: std::time::Duration) {
        for scheduler in self.schedulers.values() {
            scheduler.set_period(period);
        }
    }

    fn scheduler(&self, dimension: Dimension) -> &Scheduler<Region, GlobalRegion> {
        &self.schedulers[&dimension]
    }
    fn owner(&self, dimension: Dimension, chunk: ChunkPos) -> Option<ferrite_region::RegionId> {
        self.scheduler(dimension).owner(chunk)
    }
    fn add_chunk(&self, dimension: Dimension, chunk: ChunkPos) {
        self.scheduler(dimension).add_chunk(chunk);
    }
    fn remove_chunk(&self, dimension: Dimension, chunk: ChunkPos) {
        self.scheduler(dimension).remove_chunk(chunk);
    }
    fn schedule_at_chunk(
        &self,
        dimension: Dimension,
        chunk: ChunkPos,
        task: impl FnOnce(&mut Region) + Send + 'static,
    ) {
        self.scheduler(dimension).schedule_at_chunk(chunk, task);
    }
    /// Chat, tab list and scoreboards are global; the overworld scheduler owns them.
    fn schedule_global(&self, task: impl FnOnce(&mut GlobalRegion) + Send + 'static) {
        self.scheduler(Dimension::Overworld).schedule_global(task);
    }
    fn shutdown(self) -> HashMap<Dimension, ferrite_region::Shutdown<Region, GlobalRegion>> {
        self.schedulers
            .into_iter()
            .map(|(dimension, scheduler)| (dimension, scheduler.shutdown()))
            .collect()
    }
}

fn emit<P: ferrite_protocol::Packet>(tx: &OutboundSender, p: &P) {
    let _ = tx.send(game::packet(p));
}
fn plugin_command_line(name: &str, args: &[String]) -> String {
    if args.is_empty() {
        name.to_owned()
    } else {
        format!("{name} {}", args.join(" "))
    }
}
fn plugin_message_text(content: &ferrite_plugin::MessageContent) -> String {
    match content {
        ferrite_plugin::MessageContent::Plain(text) => text.clone(),
        ferrite_plugin::MessageContent::AdventureJson(json) => json_text(json),
    }
}
fn json_text(json: &str) -> String {
    fn collect(value: &serde_json::Value, out: &mut String) {
        match value {
            serde_json::Value::String(text) => out.push_str(text),
            serde_json::Value::Array(values) => {
                for value in values {
                    collect(value, out);
                }
            }
            serde_json::Value::Object(map) => {
                if let Some(value) = map.get("text") {
                    collect(value, out);
                }
                if let Some(value) = map.get("extra") {
                    collect(value, out);
                }
            }
            _ => {}
        }
    }
    // Only text that is not JSON at all is shown as-is: an empty component such as
    // Adventure's `Component.empty()` (`{"text":""}`) must stay empty.
    match serde_json::from_str(json) {
        Ok(value) => {
            let mut out = String::new();
            collect(&value, &mut out);
            out
        }
        Err(_) => json.to_owned(),
    }
}
fn chat(tx: &OutboundSender, text: &str) {
    emit(
        tx,
        &SystemChat {
            content: ferrite_protocol::types::PlainText(text.into()),
            overlay: false,
        },
    );
}
/// Per-tick measurements owned by the game loop's instrumentation.
struct LoopMetrics {
    game_rules: Arc<Mutex<ferrite_command::gamerule::GameRules>>,
    random_tick_speed: Arc<AtomicU32>,
    /// Raw region tick durations consumed by the load-test example.
    tick_samples: Arc<Mutex<Vec<f64>>>,
    profiler: Arc<profile::Profiler>,
    /// Set once `/stop` or an explicit shutdown is requested.
    stop: tokio::sync::watch::Sender<bool>,
}
/// Both ends of the world loop's event and region signal channels.
struct Channels {
    events: mpsc::UnboundedReceiver<GameMessage>,
    event_tx: mpsc::UnboundedSender<GameMessage>,
    signal_tx: mpsc::UnboundedSender<Signal>,
    signals: mpsc::UnboundedReceiver<Signal>,
}
async fn run(
    channels: Channels,
    source: Option<Arc<anvil::AnvilChunkSource>>,
    dimensions: Arc<DimensionRegistry>,
    flat: bool,
    _spawn: [i32; 3],
    metrics: LoopMetrics,
    mut settings: WorldSettings,
) {
    let Channels {
        mut events,
        event_tx,
        signal_tx,
        mut signals,
    } = channels;
    let tracking: region::Tracking = Arc::new(Mutex::new(HashMap::new()));

    let LoopMetrics {
        game_rules,
        random_tick_speed,
        tick_samples,
        profiler,
        stop: stop_tx,
    } = metrics;
    let command_entities: Arc<Mutex<entity_index::EntityIndex>> = Default::default();
    // The command tree and state exist before the tick context: command blocks run through
    // the same dispatcher as players and the world loop.
    let mut tree = commands::dispatcher();
    if let Some(plugins) = &settings.plugins {
        match plugins.plugin_commands() {
            Ok(commands) => commands::register_plugin_commands(&mut tree, &commands),
            Err(error) => tracing::warn!(%error, "plugin commands unavailable"),
        }
    }
    let command_tree = Arc::new(tree);
    let registry_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../vanilla/26.3/generated/reports");
    let parser_registry = ferrite_command::ParserRegistry::load(&registry_dir).ok();
    commands::load_lang(&registry_dir);
    commands_more::load_version(&registry_dir);
    commands_more::load_dialogs(&registry_dir.join("../data"));
    let world_path = source.as_ref().and_then(|source| source.path());
    let entity_world = world_path.map(std::path::Path::to_path_buf);
    let disabled_packs = world_path.map(functions::read_disabled).unwrap_or_default();
    let mut initial_state = commands::GameState::new(settings.gamemode, 0, settings.max_players);
    initial_state.entities = command_entities.clone();
    initial_state.functions = Arc::new(functions::FunctionStore::load_with_disabled(
        world_path,
        &disabled_packs,
    ));
    let command_state = Arc::new(Mutex::new(initial_state));
    crate::plugins::worldinfo::register({
        let state = Arc::clone(&command_state);
        Arc::new(move || {
            let state = state.lock().unwrap();
            crate::plugins::worldinfo::WorldInfo {
                clock_time: state.clock_time,
                game_time: state.game_time,
                difficulty: state.difficulty,
                sky: match state.weather {
                    commands::Weather::Clear => crate::plugins::worldinfo::Sky::Clear,
                    commands::Weather::Rain => crate::plugins::worldinfo::Sky::Rain,
                    commands::Weather::Thunder => crate::plugins::worldinfo::Sky::Thunder,
                },
                weather_duration: state.weather_duration,
                border: crate::plugins::worldinfo::Border {
                    center_x: state.border.center_x,
                    center_z: state.border.center_z,
                    diameter: state.border.diameter,
                    warning_distance: state.border.warning_distance,
                    warning_ticks: state.border.warning_time,
                    damage_amount: state.border.damage_per_block,
                    damage_buffer: state.border.damage_buffer,
                },
                seed: state.seed,
            }
        })
    });
    let command_speed = Arc::clone(&random_tick_speed);
    let mut schedulers = HashMap::new();
    for dimension in Dimension::ALL {
        let dimension_spawn = dimensions.spawn(dimension);
        let ctx = TickContext {
            signals: signal_tx.clone(),
            tracking: tracking.clone(),
            source: Some(dimensions.source(dimension).clone()),
            rules: Arc::clone(&game_rules),
            spawn: [
                dimension_spawn[0] as f64 + 0.5,
                dimension_spawn[1] as f64,
                dimension_spawn[2] as f64 + 0.5,
            ],
            dimension,
            raining: settings.raining && dimension == Dimension::Overworld,
            command_entities: command_entities.clone(),
            state: command_state.clone(),
            tree: Some(Arc::clone(&command_tree)),
        };
        let region_profiler = Arc::clone(&profiler);
        let fluid_tracking = tracking.clone();
        let fluid_source = dimensions.source(dimension).clone();
        let speed = Arc::clone(&random_tick_speed);
        let samples = tick_samples.clone();
        let tick_control = command_state.lock().unwrap().more.tick.clone();
        let tick = move |region: &mut Region| {
            let now = std::time::Instant::now();
            let slot = region_profiler.region_slot(&mut region.profile_slot);
            region
                .timings
                .get_or_insert_with(|| Arc::clone(region_profiler.timings()));
            region.tick(&ctx);
            if tick_control.simulating() {
                region.tick_fluids(&fluid_source, &fluid_tracking);
                region.tick_random(
                    &fluid_source,
                    &fluid_tracking,
                    speed.load(Ordering::Relaxed),
                );
            }
            let elapsed = now.elapsed();
            tick_control.record_mspt(elapsed.as_secs_f64() * 1000.0);
            region_profiler.record_region(slot, elapsed);
            let mut samples = samples.lock().unwrap();
            if samples.len() < 100_000 {
                samples.push(elapsed.as_secs_f64() * 1000.0);
            }
        };
        let profiler_for_global = Arc::clone(&profiler);
        let global: Box<dyn Fn(&mut GlobalRegion) + Send + Sync> =
            if dimension == Dimension::Overworld {
                Box::new(move |region| {
                    let now = std::time::Instant::now();
                    region
                        .timings
                        .get_or_insert_with(|| Arc::clone(profiler_for_global.timings()));
                    GlobalRegion::tick(region);
                    profiler_for_global.record_global(now.elapsed());
                })
            } else {
                Box::new(|_| {})
            };
        schedulers.insert(
            dimension,
            Scheduler::with_default_threads(
                Regionizer::<Region>::default(),
                GlobalRegion::default(),
                tick,
                global,
            ),
        );
    }
    let worlds = Worlds {
        schedulers,
        transient_entities: entity_world
            .is_none()
            .then(|| Arc::new(Mutex::new(HashMap::new()))),
        tracking: tracking.clone(),
    };
    let mut mailboxes: HashMap<Uuid, (u64, Mailbox, OutboundSender)> = HashMap::new();
    let mut pending: HashMap<Uuid, GameMessage> = HashMap::new();
    let mut permissions: HashMap<Uuid, u8> = HashMap::new();
    let mut names: HashMap<Uuid, String> = HashMap::new();
    let mut plugin_commands = settings.plugin_commands.take();
    let mut online: HashMap<Uuid, commands::OnlinePlayer> = HashMap::new();
    let mut admin = settings.admin.clone();
    let mut whitelist_enabled = settings.whitelist;
    let mut saving_enabled = true;
    let mut autosave = tokio::time::interval_at(
        tokio::time::Instant::now() + std::time::Duration::from_secs(300),
        std::time::Duration::from_secs(300),
    );
    autosave.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut holds: HashMap<(Dimension, ChunkPos), usize> = HashMap::new();
    let positions = Arc::new(Mutex::new(HashMap::new()));
    let mut clock = tokio::time::interval(std::time::Duration::from_millis(50));
    let mut applied_rate = 20.0_f32;
    clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut load_ran = false;
    loop {
        tokio::select! {
         event=events.recv()=>{let Some(event)=event else {break};
          if let GameMessage::Input { uuid, packet, .. } = &event && commands_more::counts_as_activity(packet) { command_state.lock().unwrap().touch(*uuid); }
          match event {
          GameMessage::Joined{session,uuid,name,properties,address,dimension_type,biome,view_distance,online_mode,secure_chat,chat_type,chat_visibility,max_players,outbound}=>{
           if let Some((_, mailbox, old)) = mailboxes.get(&uuid) {
             let _ = old.send(Outbound::Disconnect("You logged in from another location".into()));
             mailbox.lock().unwrap().push_back(Command::Leave);
             if let Some(GameMessage::Joined { outbound: displaced, .. }) = pending.insert(uuid, GameMessage::Joined { session, uuid, name, properties, address, dimension_type, biome, view_distance, online_mode, secure_chat, chat_type, chat_visibility, max_players, outbound }) { let _ = displaced.send(Outbound::Disconnect("You logged in from another location".into())); }
             continue;
           }
           // Vanilla checks bans before the whitelist (`PlayerList.canPlayerLogin`).
           if let Some(ban) = admin.as_ref().and_then(|lists| lists.bans.player_ban(uuid)) {
             // `multiplayer.disconnect.banned.reason`.
             let _ = outbound.send(Outbound::Disconnect(format!("You are banned from this server!\nReason: {}", ban.reason)));
             continue;
           }
           if let Some(ip) = address && let Some(ban) = admin.as_ref().and_then(|lists| lists.bans.ip_ban(&ip.to_string())) {
             // `multiplayer.disconnect.banned_ip.reason`.
             let _ = outbound.send(Outbound::Disconnect(format!("Your IP address is banned from this server!\nReason: {}", ban.reason)));
             continue;
           }
           if whitelist_enabled && let Some(lists) = &admin && !lists.whitelist.contains(uuid) {
             let _ = outbound.send(Outbound::Disconnect("You are not white-listed on this server!".into()));
             continue;
           }
           let permission = settings
             .permission(uuid)
             .max(admin.as_ref().and_then(|lists| lists.ops.level_of(uuid)).unwrap_or(0));
           if let Some(lists) = &admin {
               lists.usercache.cache(&name, uuid);
               if let Err(error) = lists.flush() { tracing::warn!(%error, "failed to save admin lists"); }
           }
           // Resume where the player left off; a first join (or unreadable data) uses the
           // spawn of the dimension the world is configured for.
           let saved = settings.playerdata.as_deref().and_then(|dir| playerdata::load_from(dir, &uuid));
           let start_dimension = saved.as_ref().map_or(settings.dimension, |s| s.dimension());
           let dimension_spawn = dimensions.spawn(start_dimension);
           let default_pos = [dimension_spawn[0] as f64 + 0.5, dimension_spawn[1] as f64, dimension_spawn[2] as f64 + 0.5];
           let start_pos = saved.as_ref().map_or(default_pos, |s| s.pos);
           let (start_yaw, start_pitch) = saved.as_ref().map_or((0.0, 0.0), |s| (s.yaw, s.pitch));
           let start_mode = saved.as_ref().map_or_else(|| command_state.lock().unwrap().default_gamemode, |s| s.gamemode);
           command_state.lock().unwrap().join(uuid, name.clone(), start_mode, max_players as i32);
           command_state.lock().unwrap().set_position(uuid, start_pos, start_yaw, start_pitch);
           command_state.lock().unwrap().set_dimension(uuid, start_dimension);
           let seed = command_state.lock().unwrap().seed;
           let start_chunk = ChunkPos((start_pos[0].floor() as i32).div_euclid(16), (start_pos[2].floor() as i32).div_euclid(16));
           let start_inventory = container::restore(saved.as_ref());
           let start_source = dimensions.source(start_dimension).clone();
           permissions.insert(uuid, permission);
           let entity=ferrite_entity::allocate_id();
           emit(&outbound,&PlayLogin{entity_id:entity,hardcore:false,dimensions:Dimension::ALL.iter().map(|d| d.key().to_owned()).collect(),max_players:VarInt(max_players as i32),view_distance:VarInt(view_distance as i32),simulation_distance:VarInt(view_distance as i32),reduced_debug:false,respawn_screen:true,limited_crafting:false,dimension_type:VarInt(start_dimension.dimension_type_id()),dimension:start_dimension.key().into(),seed,game_mode:VarInt(start_mode),previous_game_mode:VarInt(0),debug:false,flat,death_location:None,portal_cooldown:VarInt(0),sea_level:VarInt(start_dimension.sea_level()),online_mode,secure_chat});
           let (actions, _) = mpsc::unbounded_channel();
           if let Some(registry) = &parser_registry && let Ok(tree) = command_tree.serialize_commands(&commands::Source { permission, origin: start_pos, yaw: start_yaw, pitch: start_pitch, uuid: Some(uuid), name: Some(name.clone()), anchor: ferrite_command::EntityAnchor::Feet, dimension: start_dimension, world: source.clone(), state: command_state.clone(), actions, tree: Some(Arc::clone(&command_tree)), function_depth: 0 }, registry) {
               let mut packet = Vec::new(); VarInt(16).encode(&mut packet); packet.extend(tree.body); let _ = outbound.send(Outbound::Packet(packet.into()));
           }
           emit(&outbound, &SetHeldSlot { slot: VarInt(start_inventory.selected_slot() as i32) });
           // Vanilla sends the health bar and experience right after the join position.
           emit(&outbound, &SetHealth { health: 20.0, food: VarInt(20), saturation: 5.0 });
           emit(&outbound, &SetExperience { progress: 0.0, level: VarInt(0), total: VarInt(0) });
           emit(&outbound,&PositionSync{teleport_id:VarInt(1),x:start_pos[0],y:start_pos[1],z:start_pos[2],velocity_x:0.0,velocity_y:0.0,velocity_z:0.0,yaw:start_yaw,pitch:start_pitch,flags:0});
           emit(&outbound,&DefaultSpawn{dimension:start_dimension.key().into(),position:ferrite_protocol::types::BlockPos::new(dimension_spawn[0],dimension_spawn[1],dimension_spawn[2]).pack(),yaw:0.0,pitch:0.0});emit(&outbound,&GameEvent{event:13,value:0.0});
           let mailbox=Arc::new(Mutex::new(VecDeque::new()));mailboxes.insert(uuid,(session,mailbox.clone(),outbound.clone()));
           online.insert(uuid, commands::OnlinePlayer { name: name.clone(), entity, outbound: outbound.clone(), pos: start_pos, yaw: start_yaw, pitch: start_pitch, dimension: start_dimension });
           positions.lock().unwrap().insert(uuid, start_pos);
           tracking.lock().unwrap().insert(uuid, (start_dimension, HashSet::new(), outbound.clone()));
           let mut player=Player{uuid,name:name.clone(),permission,plugins:settings.plugins.clone(),plugin_event_errors:settings.plugin_event_errors,session,entity,broken:Vec::new(),ignited:Vec::new(),dimension:start_dimension,sleeping:None,spawn:None,explosions:Vec::new(),button_press:Vec::new(),editing_sign:None,changed_blocks:Vec::new(),primed_tnt:Vec::new(),tracking:tracking.clone(),positions:positions.clone(),source:Some(start_source),playerdata:settings.playerdata.clone(),effects:Default::default(),pos:start_pos,yaw:start_yaw,pitch:start_pitch,ground:true,chunk:start_chunk,mailbox,outbound:outbound.clone(),signals:signal_tx.clone(),biome,gamemode:start_mode,experience_level:0,experience_progress:0.0,experience_total:0,inventory:start_inventory,menus:Default::default(),breaking:None,view:view_distance as i32,pending_teleport:Some(1),next_teleport:2,loaded:HashSet::new(),queue:VecDeque::new(),awaiting_batch:false,rate:4.0,survival:Default::default(),dimension_type:start_dimension.dimension_type_id(),dimensions:dimensions.clone(),portal_ticks:0,portal_cooldown:0,portal_pending:None,seed,flat,respawn_requested:false,attacks:Vec::new(),attack_ticks:0,blocking:false,shield_disabled:0,bow_charge:0,using_bow:false,bow_released:false};player.init_inventory_menu();player.refresh_chunks();
           worlds.add_chunk(start_dimension,player.chunk);worlds.schedule_at_chunk(start_dimension,player.chunk,move |r|{r.hold_chunk(player.chunk);r.players.insert(uuid,player);});
           names.insert(uuid, name.clone());
           {
               let before = saved.is_some();
               let first = if before { settings.playerdata.as_deref().and_then(|dir| playerdata::first_played(dir, &uuid)) } else { None };
               crate::plugins::facts::set_history(uuid, before, first.unwrap_or_else(std::time::SystemTime::now));
           }
           crate::plugins::facts::record(uuid, crate::plugins::facts::PlayerFacts { entity_id: entity, joined: std::time::SystemTime::now(), address });
           if let Some(plugins) = &settings.plugins {
               let snapshot = ferrite_plugin::PlayerSnapshot {
                   id: uuid,
                   name: name.clone(),
                   operator: permission > 0,
                   health: 20.0,
                   food: 20,
                   mode: match start_mode {
                       1 => ferrite_plugin::GameMode::Creative,
                       2 => ferrite_plugin::GameMode::Adventure,
                       3 => ferrite_plugin::GameMode::Spectator,
                       _ => ferrite_plugin::GameMode::Survival,
                   },
                   location: ferrite_plugin::PlayerLocation {
                       world: ferrite_plugin::default_world_id(),
                       x: start_pos[0],
                       y: start_pos[1],
                       z: start_pos[2],
                       yaw: start_yaw,
                       pitch: start_pitch,
                   },
                   inventory: vec![None; 41],
               };
               if let Err(error) = plugins.register_player(&snapshot) { tracing::warn!(%error, "registering player with plugins failed"); }
           }
           if let Some(plugins) = &settings.plugins && let Err(error) = plugins.player_joined(uuid, &name) { tracing::warn!(%error, "player join event failed"); }
           let mode = start_mode; worlds.schedule_global(move |g|{let mut player = GlobalPlayer::new(uuid,entity,name,properties,outbound,secure_chat,chat_type,chat_visibility); player.pos = start_pos; player.game_mode = mode; player.dimension = start_dimension; g.join(player)});tracing::info!(%uuid,"player joined");
          },
          GameMessage::Input{session,uuid,packet:ServerboundPlay::Chat(chat)}=>{if mailboxes.get(&uuid).is_some_and(|m| m.0 == session) && chat.message.chars().count()<=256 {
              let mut chat = chat;
              let mut deliver = true;
              if let Some(plugins) = &settings.plugins {
                  let name = names.get(&uuid).cloned().unwrap_or_default();
                  match plugins.chat(uuid, &name, &chat.message) {
                      Ok(Some(text)) => {
                          // A plugin-edited message no longer matches the client signature.
                          if text != chat.message { chat.message = text; chat.signature = None; }
                      }
                      Ok(None) => deliver = false,
                      Err(error) => tracing::warn!(%error, "chat event failed"),
                  }
              }
              if deliver { worlds.schedule_global(move |g|g.chat(uuid,&chat)); }
          }},
          GameMessage::Input{session,uuid,packet:ServerboundPlay::ChatAck(ack)}=>{if mailboxes.get(&uuid).is_some_and(|m| m.0 == session) {worlds.schedule_global(move |g|g.chat_ack(uuid,ack.offset.0));}},
          GameMessage::Input{session,uuid,packet:ServerboundPlay::ChatSessionUpdate(update)}=>{if mailboxes.get(&uuid).is_some_and(|m| m.0 == session) {worlds.schedule_global(move |g|g.chat_session(uuid,update.chat_session));}},
          GameMessage::Input{session,uuid,packet:ServerboundPlay::ChatCommandSigned(signed)}=>{if mailboxes.get(&uuid).is_some_and(|m| m.0 == session) {
              let ServerboundChatCommandSigned { command, last_seen, .. } = signed;
              worlds.schedule_global(move |g|g.apply_chat_update(uuid,&last_seen));
              let _ = event_tx.send(GameMessage::Input{session,uuid,packet:ServerboundPlay::ChatCommand(ServerboundChatCommand{command})});
          }},
          GameMessage::Input{session,uuid,packet:ServerboundPlay::ChatCommand(command)}=>{let command = command.command; if let Some((_, mailbox, outbound)) = mailboxes.get(&uuid).filter(|m| m.0 == session) {
              let message = format!("/{}", command.trim_start_matches('/'));
              if let (Some(plugins), Some(name)) = (&settings.plugins, names.get(&uuid)) {
                  match plugins.player_command(uuid, name, &message) {
                      Ok(true) => {}
                      Ok(false) => continue,
                      Err(error) => tracing::warn!(%error, "command preprocess event failed"),
                  }
              }
              let (actions, mut received) = mpsc::unbounded_channel();
              let (origin, yaw, pitch) = online.get(&uuid).map_or(([0.0; 3], 0.0, 0.0), |player| (player.pos, player.yaw, player.pitch));
              let name = command_state.lock().unwrap().players.iter().find(|p| p.uuid == uuid).map(|p| p.name.clone());
              let command_source = commands::Source { permission: *permissions.get(&uuid).unwrap_or(&0), origin, yaw, pitch, uuid: Some(uuid), name, anchor: ferrite_command::EntityAnchor::Feet, dimension: online.get(&uuid).map_or(Dimension::Overworld, |player| player.dimension), world: source.clone(), state: command_state.clone(), actions, tree: Some(Arc::clone(&command_tree)), function_depth: 0 };
              if let Err(error) = command_tree.execute(command.trim_start_matches('/'), command_source) { emit(outbound, &SystemChat { content:ferrite_protocol::types::PlainText(error.to_string()), overlay:false }); }
              let source_name = online.get(&uuid).map_or_else(|| "Server".to_owned(), |player| player.name.clone());
              let runtime_dimension = online.get(&uuid).map_or(Dimension::Overworld, |p| p.dimension);
              let mut runtime = CommandRuntime {
                  reply: Reply { outbound: Some(outbound) },
                  source_uuid: Some(uuid),
                  source_name,
                  origin,
                  state: &command_state,
                  online: &mut online,
                  permissions: &mut permissions,
                  admin: &mut admin,
                  whitelist: &mut whitelist_enabled,
                  saving: &mut saving_enabled,
                  mailboxes: &mailboxes,
                  positions: &positions,
                  profiler: &profiler,
                  source: &source,
                  events: &event_tx,
                  stop: &stop_tx,
                  signals: &signal_tx,
                  worlds: &worlds,
                  dimension: runtime_dimension,
                  tracking: &tracking,
              };
              while let Ok(action) = received.try_recv() { match action {
                  commands::Action::GameMode { mode, targets } => { for target in targets { if let Some((_, target_mailbox, _)) = mailboxes.get(&target) { target_mailbox.lock().unwrap().push_back(Command::SetMode(mode)); } worlds.schedule_global(move |g| g.set_mode(target, mode)); } },
                  commands::Action::DefaultGameMode(mode) => tracing::info!(mode, "default game mode changed"),
                  commands::Action::Difficulty(value) => tracing::info!(value, "difficulty changed"),
                  commands::Action::TimeSet(ticks) => tracing::info!(ticks, "time set"),
                  commands::Action::TimeAdd(ticks) => tracing::info!(ticks, "time added"),
                  commands::Action::Weather { kind, duration } => tracing::info!(?kind, ?duration, "weather changed"),
                  commands::Action::Feedback(feedback) => emit(outbound, &SystemChat { content:ferrite_protocol::types::PlainText(feedback.text), overlay:false }),
                  commands::Action::PlayerFeedback { uuid: target, feedback } => { if let Some((_, _, target_outbound)) = mailboxes.get(&target) { emit(target_outbound, &SystemChat { content:ferrite_protocol::types::PlainText(feedback.text), overlay:false }); } },
                  commands::Action::Broadcast(feedback) => { let text = feedback.text; worlds.schedule_global(move |g| g.broadcast_text(&text)); },
                  commands::Action::Plugin { name, args } => {
                      if let Some(plugins) = &settings.plugins {
                          let line = plugin_command_line(&name, &args);
                          match plugins.run_plugin_command(ferrite_plugin::CommandSender::Player(uuid), &line) {
                              Ok(result) => for message in result.messages {
                                  if message.player == uuid { chat(outbound, &plugin_message_text(&message.content)); }
                              },
                              Err(error) => tracing::warn!(%error, "plugin command failed"),
                          }
                      }
                  },
                  commands::Action::GameRule(name, value) => match apply_game_rule(&game_rules, source.as_ref(), &command_speed, &name, value.as_deref()) { Ok(message) => chat(outbound, &message), Err(error) => chat(outbound, &error) },
                  commands::Action::World(command) => { mailbox.lock().unwrap().push_back(Command::World(command)); },
                  commands::Action::Clear { targets, item, max_count } => { for target in targets { if let Some((_, target_mailbox, _)) = mailboxes.get(&target) { let target_name = names.get(&target).cloned().unwrap_or_default(); target_mailbox.lock().unwrap().push_back(Command::Clear { target, target_name, item, max_count, feedback: Some(outbound.clone()) }); } } },
                  commands::Action::Give { targets, item, count, item_name, components } => { for target in targets { if let Some((_, target_mailbox, _)) = mailboxes.get(&target) { let target_name = names.get(&target).cloned().unwrap_or_default(); target_mailbox.lock().unwrap().push_back(Command::Give { target, target_name, item, count, item_name: item_name.clone(), components: components.clone(), feedback: Some(outbound.clone()) }); } } },
                  commands::Action::Experience { operation, targets, amount, unit } => { for target in targets { if let Some((_, target_mailbox, _)) = mailboxes.get(&target) { let target_name = names.get(&target).cloned().unwrap_or_default(); target_mailbox.lock().unwrap().push_back(Command::Experience { target, target_name, operation, amount, unit, feedback: Some(outbound.clone()) }); } } },
                  commands::Action::Summon { kind, pos } => {
                      let chunk = ferrite_region::ChunkPos((pos[0].floor() as i32) >> 4, (pos[2].floor() as i32) >> 4);
                      let dimension = runtime.dimension;
                      if worlds.owner(dimension, chunk).is_some() {
                          worlds.schedule_at_chunk(dimension, chunk, move |region| { region.summon(kind, pos); });
                          chat(outbound, &format!("Summoned new {}", kind.display_name()));
                      } else {
                          chat(outbound, "That position is not loaded");
                      }
                  },
                  commands::Action::Enchant { targets, enchantment, level } => { for target in targets { if let Some((_, target_mailbox, _)) = mailboxes.get(&target) { let target_name = names.get(&target).cloned().unwrap_or_default(); target_mailbox.lock().unwrap().push_back(Command::Enchant { target, target_name, enchantment: enchantment.clone(), level, feedback: Some(outbound.clone()) }); } } },
                  other => { runtime.apply(other).await; }
              }}
          }},
          GameMessage::Input{session,uuid,packet}=>{if let Some((_,m,_))=mailboxes.get(&uuid).filter(|m| m.0 == session){m.lock().unwrap().push_back(Command::Input(packet));}},
          GameMessage::Left{session,uuid}=>{if matches!(pending.get(&uuid), Some(GameMessage::Joined { session: id, .. }) if *id == session) { pending.remove(&uuid); }
            if let Some((_,m,_))=mailboxes.get(&uuid).filter(|m| m.0 == session){m.lock().unwrap().push_back(Command::Leave);}},
          GameMessage::Console{command,complete}=>{
              let (actions, mut received) = mpsc::unbounded_channel();
              let command_source = commands::Source { permission: 4, origin: [0.0; 3], yaw: 0.0, pitch: 0.0, uuid: None, name: None, anchor: ferrite_command::EntityAnchor::Feet, dimension: Dimension::Overworld, world: source.clone(), state: command_state.clone(), actions, tree: Some(Arc::clone(&command_tree)), function_depth: 0 };
              let result = command_tree.execute(command.trim().trim_start_matches('/'), command_source).map_err(|e| e.to_string());
              let mut stop = false;
              let mut runtime = CommandRuntime {
                  reply: Reply { outbound: None },
                  source_uuid: None,
                  source_name: "Server".to_owned(),
                  origin: [0.0; 3],
                  state: &command_state,
                  online: &mut online,
                  permissions: &mut permissions,
                  admin: &mut admin,
                  whitelist: &mut whitelist_enabled,
                  saving: &mut saving_enabled,
                  mailboxes: &mailboxes,
                  positions: &positions,
                  profiler: &profiler,
                  source: &source,
                  events: &event_tx,
                  stop: &stop_tx,
                  signals: &signal_tx,
                  worlds: &worlds,
                  dimension: Dimension::Overworld,
                  tracking: &tracking,
              };
              while let Ok(action) = received.try_recv() { match action {
                  commands::Action::DefaultGameMode(mode) => tracing::info!(mode, "default game mode changed"),
                  commands::Action::Difficulty(value) => tracing::info!(value, "difficulty changed"),
                  commands::Action::TimeSet(ticks) => tracing::info!(ticks, "time set"),
                  commands::Action::TimeAdd(ticks) => tracing::info!(ticks, "time added"),
                  commands::Action::Weather { kind, duration } => tracing::info!(?kind, ?duration, "weather changed"),
                  commands::Action::Feedback(feedback) => tracing::info!("{}", feedback.text),
                  commands::Action::Broadcast(feedback) => { let text = feedback.text; worlds.schedule_global(move |g| g.broadcast_text(&text)); },
                  commands::Action::GameRule(name, value) => match apply_game_rule(&game_rules, source.as_ref(), &command_speed, &name, value.as_deref()) { Ok(message) => tracing::info!("{message}"), Err(error) => tracing::warn!("{error}") },
                  commands::Action::Stop => stop = true,
                  commands::Action::Tps => for line in profiler.tps_lines_plain() { tracing::info!("{line}"); },
                  commands::Action::Mspt => for line in profiler.mspt_lines_plain() { tracing::info!("{line}"); },
                  commands::Action::ProfileStart => if let Err(error) = profiler.start_profile() { tracing::warn!(%error, "profile start failed"); } else { tracing::info!("profiling started; run profile stop to dump the slowest systems"); },
                  commands::Action::ProfileStop => match profiler.stop_profile() {
                      Ok(report) => for line in report.lines() { tracing::info!("{line}"); },
                      Err(error) => tracing::warn!(%error, "profile stop failed"),
                  },
                  commands::Action::Plugin { name, args } => {
                      if let Some(plugins) = &settings.plugins {
                          let line = plugin_command_line(&name, &args);
                          match plugins.run_plugin_command(ferrite_plugin::CommandSender::Console, &line) {
                              Ok(result) => for message in result.messages { tracing::info!("{}", plugin_message_text(&message.content)); },
                              Err(error) => tracing::warn!(%error, "plugin command failed"),
                          }
                      }
                  },
                  commands::Action::GameMode { .. } | commands::Action::PlayerFeedback { .. } => tracing::warn!("command requires a player"),
                  commands::Action::Clear { targets, item, max_count } => { for target in targets { if let Some((_, target_mailbox, _)) = mailboxes.get(&target) { let target_name = names.get(&target).cloned().unwrap_or_default(); target_mailbox.lock().unwrap().push_back(Command::Clear { target, target_name, item, max_count, feedback: None }); } } },
                  commands::Action::Give { targets, item, count, item_name, components } => { for target in targets { if let Some((_, target_mailbox, _)) = mailboxes.get(&target) { let target_name = names.get(&target).cloned().unwrap_or_default(); target_mailbox.lock().unwrap().push_back(Command::Give { target, target_name, item, count, item_name: item_name.clone(), components: components.clone(), feedback: None }); } } },
                  commands::Action::Experience { operation, targets, amount, unit } => { for target in targets { if let Some((_, target_mailbox, _)) = mailboxes.get(&target) { let target_name = names.get(&target).cloned().unwrap_or_default(); target_mailbox.lock().unwrap().push_back(Command::Experience { target, target_name, operation, amount, unit, feedback: None }); } } },
                  commands::Action::Summon { kind, pos } => {
                      let chunk = ferrite_region::ChunkPos((pos[0].floor() as i32) >> 4, (pos[2].floor() as i32) >> 4);
                      if worlds.owner(Dimension::Overworld, chunk).is_some() {
                          worlds.schedule_at_chunk(Dimension::Overworld, chunk, move |region| { region.summon(kind, pos); });
                          tracing::info!("Summoned new {} at {pos:?}", kind.display_name());
                      } else {
                          tracing::warn!("summon target chunk is not loaded");
                      }
                  },
                  commands::Action::Enchant { targets, enchantment, level } => { for target in targets { if let Some((_, target_mailbox, _)) = mailboxes.get(&target) { let target_name = names.get(&target).cloned().unwrap_or_default(); target_mailbox.lock().unwrap().push_back(Command::Enchant { target, target_name, enchantment: enchantment.clone(), level, feedback: None }); } } },
                  other => { if runtime.apply(other).await { stop = true; } }
              }}
              let _ = complete.send(result.map(|_| stop));
          },
          GameMessage::Shutdown{complete}=>{let _=stop_tx.send(true);let (ack_tx,ack_rx)=oneshot::channel();worlds.schedule_global(move |g|{g.shutdown();let _=ack_tx.send(());});let _=ack_rx.await;let mut regions: Vec<Region> = worlds.shutdown().into_values().flat_map(|stopped| stopped.regions).collect();let entity_result = match &entity_world { Some(dir) => entity_save::save_regions(dir, &mut regions).map_err(|error| format!("failed to save entities during shutdown: {error:#}")), None => Ok(()) };let result = match (entity_result, &source) { (Err(error), _) => Err(error), (Ok(()), Some(source)) => source.flush_all().await.map_err(|error| format!("failed to save world chunks during shutdown: {error:#}")), (Ok(()), None) => Ok(()) }; let _=complete.send(result);break;}
         }},
         _=autosave.tick(), if saving_enabled => {
             if let Some(source) = &source && let Err(error) = source.flush_all().await {
                 tracing::error!(%error, "autosave failed");
             }
         },
         signal=signals.recv()=>{let Some(signal)=signal else {break}; match signal {
          Signal::Hold{dimension,chunk,add}=>if add{if dimension==Dimension::Overworld{crate::plugins::chunks::set_loaded(chunk.0,chunk.1,true);}let first={let count=holds.entry((dimension,chunk)).or_default();*count += 1;*count == 1};worlds.add_chunk(dimension,chunk);let dir=entity_world.clone();let transient=worlds.transient_entities.clone();worlds.schedule_at_chunk(dimension,chunk,move |r|{r.hold_chunk(chunk);if let Some(transient)=transient {if let Some(entities)=transient.lock().unwrap().remove(&(dimension,chunk)){r.add_entities(entities);}} else if first && let Some(dir)=dir{r.add_entities(entity_save::load(&dimension.storage_root(&dir), (chunk.0, chunk.1)));}});}else{release_hold(&worlds, &mut holds, &signal_tx, dimension, chunk);},
          Signal::SaveEntities{dimension,chunk,entities}=>{if let Some(dir)=&entity_world { let dir=dimension.storage_root(dir); if let Err(error)=entity_save::save(&dir, (chunk.0, chunk.1), &entities){tracing::error!(%error, chunk=?chunk, "failed to save chunk entities");}}},
          Signal::FinishRelease{dimension,chunk,flush}=>{worlds.remove_chunk(dimension,chunk);if flush {let source=dimensions.source(dimension).clone();tokio::spawn(async move {if let Err(error)=source.flush_chunk(chunk.0,chunk.1).await{tracing::error!(%error,"failed to save unloaded chunk");}});}},
          Signal::FluidChange{dimension,x,y,z}=>{
            for (dx,dy,dz) in [(0,0,0),(1,0,0),(-1,0,0),(0,1,0),(0,-1,0),(0,0,1),(0,0,-1)] {
              let (Some(x),Some(y),Some(z))=(x.checked_add(dx),y.checked_add(dy),z.checked_add(dz)) else {continue;};
              let chunk=ChunkPos(x.div_euclid(16),z.div_euclid(16));
              if worlds.owner(dimension,chunk).is_some(){worlds.schedule_at_chunk(dimension,chunk,move |r|r.schedule_fluid((x,y,z)));}
            }
          },
          Signal::Update{uuid,pos,yaw,pitch,ground}=>{
            if let Some(player)=online.get_mut(&uuid){player.pos=pos;player.yaw=yaw;player.pitch=pitch;}
            crate::plugins::facts::set_on_ground(uuid, ground);
            command_state.lock().unwrap().set_position(uuid,pos,yaw,pitch);
            if let Some(plugins) = &settings.plugins {
              let location = ferrite_plugin::PlayerLocation { world: ferrite_plugin::default_world_id(), x: pos[0], y: pos[1], z: pos[2], yaw, pitch };
              if let Err(error) = plugins.update_player_location(uuid, &location) { tracing::warn!(%error, "updating the plugin player snapshot failed"); }
            }
            worlds.schedule_global(move |g|g.update(uuid,pos,yaw,pitch,ground));
          },
          Signal::Transfer{dimension,from,to,player}=>{worlds.add_chunk(dimension,to);let id=player.uuid;worlds.schedule_at_chunk(dimension,to,move |r|{r.hold_chunk(to);r.players.insert(id,*player);});defer_release(&worlds,&signal_tx,dimension,from,false);},
          Signal::EntityTransfer{dimension,to,entity}=>hand_off(worlds.scheduler(dimension), to, entity),
          Signal::ChangeDimension{from_dimension,from,player}=>{
            let dimension=player.dimension;let chunk=player.chunk;let id=player.uuid;
            worlds.add_chunk(dimension,chunk);worlds.schedule_at_chunk(dimension,chunk,move |r|{r.hold_chunk(chunk);r.players.insert(id,*player);});
            defer_release(&worlds,&signal_tx,from_dimension,from,false);
            if let Some(online)=online.get_mut(&id){online.dimension=dimension;}
            command_state.lock().unwrap().set_dimension(id,dimension);
            worlds.schedule_global(move |g| g.set_dimension(id, dimension));
          },
          Signal::Leave{dimension,uuid,chunk,held}=>{tracking.lock().unwrap().remove(&uuid);positions.lock().unwrap().remove(&uuid);command_state.lock().unwrap().leave(uuid);permissions.remove(&uuid);online.remove(&uuid);mailboxes.remove(&uuid);for pos in held {release_hold(&worlds, &mut holds, &signal_tx, dimension, pos);}defer_release(&worlds,&signal_tx,dimension,chunk,false);
            let name = names.remove(&uuid).unwrap_or_default();
            crate::plugins::facts::forget(uuid);
            if let Some(plugins) = &settings.plugins && let Err(error) = plugins.player_left(uuid, &name) { tracing::warn!(%error, "player quit event failed"); }
            worlds.schedule_global(move |g|g.leave(uuid));if let Some(join)=pending.remove(&uuid){let _=event_tx.send(join);}}
         }},
         command = async { match plugin_commands.as_mut() { Some(commands) => commands.recv().await, None => std::future::pending().await } }, if plugin_commands.is_some() => {
             match command {
                 // Chat attribution lives on the global region, so display names go there.
                 Some(ferrite_plugin::PlayerCommand::DisplayName { player, component_json }) => {
                     let name = component_json.as_deref().map(json_text);
                     worlds.schedule_global(move |g| g.set_display_name(player, name));
                 }
                 Some(ferrite_plugin::PlayerCommand::UpdateCommands { player }) => {
                    if let (Some(registry), Some(op)) = (&parser_registry, online.get(&player)) {
                        let (actions, _) = mpsc::unbounded_channel();
                        let permission = *permissions.get(&player).unwrap_or(&0);
                        let source_for = commands::Source { permission, origin: op.pos, yaw: op.yaw, pitch: op.pitch, uuid: Some(player), name: Some(op.name.clone()), anchor: ferrite_command::EntityAnchor::Feet, dimension: op.dimension, world: source.clone(), state: command_state.clone(), actions, tree: Some(Arc::clone(&command_tree)), function_depth: 0 };
                        if let Ok(tree) = command_tree.serialize_commands(&source_for, registry) {
                            let mut packet = Vec::new();
                            VarInt(16).encode(&mut packet);
                            packet.extend(tree.body);
                            let _ = op.outbound.send(Outbound::Packet(packet.into()));
                        }
                    }
                }
                 Some(ferrite_plugin::PlayerCommand::SpawnEntity { center, kind, block, velocity, uuid }) => {
                    let chunk = ChunkPos((center[0].floor() as i32) >> 4, (center[2].floor() as i32) >> 4);
                    if worlds.owner(Dimension::Overworld, chunk).is_some() {
                        worlds.schedule_at_chunk(Dimension::Overworld, chunk, move |r| { r.plugin_spawn(center, &kind, block.as_deref(), velocity, uuid); });
                    }
                }
                 Some(ferrite_plugin::PlayerCommand::EntityOp { uuid, op }) => {
                    if let Some(at) = crate::plugins::facts::entity_position(uuid) {
                        let chunk = ChunkPos((at[0].floor() as i32) >> 4, (at[2].floor() as i32) >> 4);
                        if worlds.owner(Dimension::Overworld, chunk).is_some() {
                            worlds.schedule_at_chunk(Dimension::Overworld, chunk, move |r| { r.plugin_entity_op(uuid, &op); });
                        }
                    }
                }
                 Some(ferrite_plugin::PlayerCommand::DropItem { center, item, amount, natural, uuid }) => {
                    let chunk = ChunkPos((center[0].floor() as i32) >> 4, (center[2].floor() as i32) >> 4);
                    if worlds.owner(Dimension::Overworld, chunk).is_some() {
                        worlds.schedule_at_chunk(Dimension::Overworld, chunk, move |r| r.plugin_drop_item(center, &item, amount, natural, uuid));
                    }
                }
                 Some(ferrite_plugin::PlayerCommand::Explosion { center, power, fire }) => {
                    let chunk = ChunkPos((center[0].floor() as i32) >> 4, (center[2].floor() as i32) >> 4);
                    if worlds.owner(Dimension::Overworld, chunk).is_some() {
                        worlds.schedule_at_chunk(Dimension::Overworld, chunk, move |r| r.plugin_explosions.push((center, power, fire)));
                    }
                }
                 Some(ferrite_plugin::PlayerCommand::Visibility { viewer, target, hidden }) => {
                    worlds.schedule_global(move |g| g.set_hidden(viewer, target, hidden));
                }
                Some(command) => route_plugin_command(command, &mailboxes, &mut permissions),
                 // The plugin host dropped its sender (no plugins loaded): stop polling.
                 None => plugin_commands = None,
             }
         },
         _tick = clock.tick() => {
             let advance_time = game_rules.lock().unwrap().get_bool("advance_time").unwrap_or(true);
             let tick_control = command_state.lock().unwrap().more.tick.clone();
             let target_rate = tick_control.rate();
             if (target_rate - applied_rate).abs() > f32::EPSILON {
                 applied_rate = target_rate;
                 let period = std::time::Duration::from_secs_f64(1.0 / f64::from(target_rate));
                 worlds.set_period(period);
                 clock = tokio::time::interval(period);
                 clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
             }
             let runs = tick_control.begin_game_tick();
             let (due, idle) = { let mut game = command_state.lock().unwrap(); let due = if runs { game.tick(advance_time) } else { Vec::new() }; (due, game.take_idle_players()) };
             for uuid in idle { if let Some((_, _, outbound)) = mailboxes.get(&uuid) { let _ = outbound.send(Outbound::Disconnect(commands::Feedback::new("multiplayer.disconnect.idling", vec![]).text)); } }
             for function in due {
                 let (complete, _unused) = oneshot::channel();
                 let _ = event_tx.send(GameMessage::Console {
                     command: format!("function {function}"),
                     complete,
                 });
             }
             if let Some(plugins) = &settings.plugins {
                 if let Err(error) = plugins.tick() {
                     tracing::warn!(%error, "plugin tick failed");
                 }
                 // Paper's region scheduler: run each due task on the thread owning its chunk.
                 for task in plugins.take_region_tasks() {
                     if task.world != ferrite_plugin::default_world_id().to_string() {
                         tracing::debug!(world = %task.world, "plugin region task for an unknown world");
                         continue;
                     }
                     let chunk = ChunkPos(task.chunk_x, task.chunk_z);
                     let plugins = Arc::clone(plugins);                      let dimension = Dimension::from_key(&task.world).unwrap_or(Dimension::Overworld);
                      worlds.schedule_at_chunk(dimension, chunk, move |_| {
                         if let Err(error) = plugins.run_region_task(task) {
                             tracing::warn!(%error, "plugin region task failed");
                         }
                     });
                 }
                 // Paper's entity scheduler: the only entities plugins hold are players, whose
                 // mailbox is drained by whichever region owns them, hand-offs included.
                 for task in plugins.take_entity_tasks() {
                     route_entity_task(task, &mailboxes);
                 }
             }
             // Vanilla runs `minecraft:load` once at startup and `minecraft:tick` every
             // tick. Both go through the console command path so their actions
             // (setblock, say, ...) are applied like any other command.
             let tags: &[&str] = if load_ran {
                 &[functions::TICK_TAG]
             } else {
                 load_ran = true;
                 &[functions::LOAD_TAG, functions::TICK_TAG]
             };
             for tag in tags {
                 let known = command_state
                     .lock()
                     .unwrap()
                     .functions
                     .tag(tag)
                     .is_some_and(|functions| !functions.is_empty());
                 if !known {
                     continue;
                 }
                 let (complete, _unused) = oneshot::channel();
                 let _ = event_tx.send(GameMessage::Console {
                     command: format!("function #{tag}"),
                     complete,
                 });
            }
         }
        }
    }
}
/// Delivers a player mutation from the embedded JVM to the region that owns the player.
///
/// Inventory and gamemode changes ride the player's mailbox so a region hand-off moves them
/// with the player; teleports and chat go through the existing dedicated paths.
fn route_plugin_command(
    command: ferrite_plugin::PlayerCommand,
    mailboxes: &HashMap<Uuid, (u64, Mailbox, OutboundSender)>,
    permissions: &mut HashMap<Uuid, u8>,
) {
    use ferrite_plugin::PlayerCommand;
    match command {
        PlayerCommand::SetInventorySlot { player, slot, item } => {
            if let Some((_, mailbox, _)) = mailboxes.get(&player) {
                mailbox
                    .lock()
                    .unwrap()
                    .push_back(Command::SetInventorySlot { slot, item });
            }
        }
        PlayerCommand::Teleport { player, location } => {
            if location.world == ferrite_plugin::default_world_id()
                && let Some((_, mailbox, _)) = mailboxes.get(&player)
            {
                mailbox.lock().unwrap().push_back(Command::Teleport {
                    pos: [location.x, location.y, location.z],
                    yaw: Some(location.yaw),
                    pitch: Some(location.pitch),
                });
            }
        }
        PlayerCommand::SetGameMode { player, mode } => {
            let mode = match mode {
                ferrite_plugin::GameMode::Survival => 0,
                ferrite_plugin::GameMode::Creative => 1,
                ferrite_plugin::GameMode::Adventure => 2,
                ferrite_plugin::GameMode::Spectator => 3,
            };
            if let Some((_, mailbox, _)) = mailboxes.get(&player) {
                mailbox.lock().unwrap().push_back(Command::SetMode(mode));
            }
        }
        PlayerCommand::Abilities {
            player,
            allow_flight,
            flying,
            invulnerable,
            fly_speed,
            walk_speed,
        } => {
            if let Some((_, mailbox, _)) = mailboxes.get(&player) {
                mailbox.lock().unwrap().push_back(Command::PluginAbilities {
                    allow_flight,
                    flying,
                    invulnerable,
                    fly_speed,
                    walk_speed,
                });
            }
        }
        PlayerCommand::GameEvent {
            player,
            event,
            value,
        } => {
            if let Some((_, mailbox, _)) = mailboxes.get(&player) {
                mailbox
                    .lock()
                    .unwrap()
                    .push_back(Command::PluginGameEvent { event, value });
            }
        }
        PlayerCommand::OpenInventory {
            player,
            title,
            rows,
            items,
        } => {
            if let Some((_, mailbox, _)) = mailboxes.get(&player) {
                mailbox
                    .lock()
                    .unwrap()
                    .push_back(Command::PluginOpenInventory { title, rows, items });
            }
        }
        PlayerCommand::Packet { player, packet } => {
            if let Some((_, mailbox, _)) = mailboxes.get(&player) {
                mailbox
                    .lock()
                    .unwrap()
                    .push_back(Command::PluginPacket(packet));
            }
        }
        PlayerCommand::Survival {
            player,
            stat,
            value,
        } => {
            if let Some((_, mailbox, _)) = mailboxes.get(&player) {
                mailbox
                    .lock()
                    .unwrap()
                    .push_back(Command::PluginSurvival { stat, value });
            }
        }
        PlayerCommand::Cooldown {
            player,
            group,
            ticks,
        } => {
            if let Some((_, mailbox, _)) = mailboxes.get(&player) {
                mailbox
                    .lock()
                    .unwrap()
                    .push_back(Command::PluginCooldown { group, ticks });
            }
        }
        PlayerCommand::EntityData {
            player,
            flags,
            silent,
            no_gravity,
            freeze_ticks,
        } => {
            if let Some((_, mailbox, _)) = mailboxes.get(&player) {
                mailbox
                    .lock()
                    .unwrap()
                    .push_back(Command::PluginEntityData {
                        flags,
                        silent,
                        no_gravity,
                        freeze_ticks,
                    });
            }
        }
        PlayerCommand::Velocity { player, velocity } => {
            if let Some((_, mailbox, _)) = mailboxes.get(&player) {
                mailbox
                    .lock()
                    .unwrap()
                    .push_back(Command::PluginVelocity { velocity });
            }
        }
        PlayerCommand::OpenStation { player, station } => {
            if let Some((_, mailbox, _)) = mailboxes.get(&player) {
                mailbox
                    .lock()
                    .unwrap()
                    .push_back(Command::PluginOpenStation { station });
            }
        }
        PlayerCommand::CloseInventory { player } => {
            if let Some((_, mailbox, _)) = mailboxes.get(&player) {
                mailbox
                    .lock()
                    .unwrap()
                    .push_back(Command::PluginCloseInventory);
            }
        }
        PlayerCommand::Damage { player, amount } => {
            if let Some((_, mailbox, _)) = mailboxes.get(&player) {
                mailbox
                    .lock()
                    .unwrap()
                    .push_back(Command::PluginDamage { amount });
            }
        }
        PlayerCommand::VisualExperienceCurrentLevel { player, progress } => {
            if let Some((_, mailbox, _)) = mailboxes.get(&player) {
                mailbox
                    .lock()
                    .unwrap()
                    .push_back(Command::PluginVisualExperience {
                        progress,
                        level: None,
                    });
            }
        }
        PlayerCommand::VisualExperience {
            player,
            progress,
            level,
        } => {
            if let Some((_, mailbox, _)) = mailboxes.get(&player) {
                mailbox
                    .lock()
                    .unwrap()
                    .push_back(Command::PluginVisualExperience {
                        progress,
                        level: Some(level),
                    });
            }
        }
        PlayerCommand::CustomChatCompletions {
            player,
            action,
            entries,
        } => {
            if let Some((_, _, outbound)) = mailboxes.get(&player) {
                emit(
                    outbound,
                    &CustomChatCompletions {
                        action: VarInt(action),
                        entries,
                    },
                );
            }
        }
        PlayerCommand::OpenVirtualSign {
            player,
            position,
            front,
        } => {
            if let Some((_, _, outbound)) = mailboxes.get(&player) {
                emit(
                    outbound,
                    &OpenSignEditor {
                        location: position,
                        is_front_text: front,
                    },
                );
            }
        }
        PlayerCommand::VisualBlockDamage {
            player,
            position,
            source,
            progress,
        } => {
            if let Some((_, _, outbound)) = mailboxes.get(&player) {
                emit(
                    outbound,
                    &BlockDestruction {
                        entity_id: VarInt(source),
                        position,
                        progress,
                    },
                );
            }
        }
        PlayerCommand::VisualHealth {
            player,
            health,
            food,
            saturation,
        } => {
            if let Some((_, _, outbound)) = mailboxes.get(&player) {
                emit(
                    outbound,
                    &SetHealth {
                        health,
                        food: VarInt(food),
                        saturation,
                    },
                );
            }
        }
        PlayerCommand::SetHealth { player, health } => {
            if let Some((_, mailbox, _)) = mailboxes.get(&player) {
                mailbox.lock().unwrap().push_back(Command::SetHealth {
                    health: health as f32,
                });
            }
        }
        PlayerCommand::SetFood { player, food } => {
            if let Some((_, mailbox, _)) = mailboxes.get(&player) {
                mailbox.lock().unwrap().push_back(Command::SetFood { food });
            }
        }
        PlayerCommand::SetExperience {
            player,
            level,
            progress,
            total,
        } => {
            if let Some((_, mailbox, _)) = mailboxes.get(&player) {
                mailbox.lock().unwrap().push_back(Command::SetExperience {
                    level,
                    progress,
                    total,
                });
            }
        }
        PlayerCommand::SetOperator { player, operator } => {
            permissions.insert(player, if operator { 4 } else { 0 });
        }
        PlayerCommand::Message { player, content } => {
            if let Some((_, _, outbound)) = mailboxes.get(&player) {
                chat(outbound, &plugin_message_text(&content));
            }
        }
        PlayerCommand::Title {
            player,
            title,
            subtitle,
            fade_in,
            stay,
            fade_out,
        } => {
            if let Some((_, _, outbound)) = mailboxes.get(&player) {
                for packet in plugin_title_packets(title, subtitle, fade_in, stay, fade_out) {
                    packet.send_to(outbound);
                }
            }
        }
        PlayerCommand::Kick {
            player,
            component_json,
        } => {
            if let Some((_, _, outbound)) = mailboxes.get(&player) {
                let _ = outbound.send(Outbound::Disconnect(plugin_kick_reason(
                    component_json.as_deref(),
                )));
            }
        }
        PlayerCommand::PlaySound {
            player,
            world,
            sound,
            category,
            x,
            y,
            z,
            volume,
            pitch,
            seed,
        } => {
            let Some((_, _, outbound)) = mailboxes.get(&player) else {
                return;
            };
            if world != ferrite_plugin::default_world_id() {
                tracing::debug!(%world, "plugin sound for a world the server does not own");
                return;
            }
            match plugin_sound_packet(&sound, category, [x, y, z], volume, pitch, seed) {
                Some(packet) => packet.send_to(outbound),
                // Resource-pack sounds need an inline `SoundEvent` holder, which the packet
                // codec does not model yet.
                None => tracing::debug!(%sound, "plugin sound is not in the sound registry"),
            }
        }
        PlayerCommand::ActionBar {
            player,
            component_json,
        } => {
            if let Some((_, _, outbound)) = mailboxes.get(&player) {
                commands_extras::PlayerPacket::ActionBar(SetActionBarText {
                    text: ferrite_protocol::types::PlainText(json_text(&component_json)),
                })
                .send_to(outbound);
            }
        }
        PlayerCommand::ClearTitle { player, reset } => {
            if let Some((_, _, outbound)) = mailboxes.get(&player) {
                commands_extras::PlayerPacket::ClearTitles(ClearTitles { reset }).send_to(outbound);
            }
        }
        PlayerCommand::ListHeaderFooter {
            player,
            header_json,
            footer_json,
        } => {
            if let Some((_, _, outbound)) = mailboxes.get(&player) {
                let _ = outbound.send(ferrite_net::game::packet(
                    &ferrite_protocol::packets::play::TabList {
                        header: ferrite_protocol::types::PlainText(json_text(&header_json)),
                        footer: ferrite_protocol::types::PlainText(json_text(&footer_json)),
                    },
                ));
            }
        }
        // The game loop sends visibility to the global region before calling this.
        PlayerCommand::Visibility { .. } => {}
        // The game loop hands explosions to the region owning the chunk before calling this.
        PlayerCommand::Explosion { .. }
        | PlayerCommand::UpdateCommands { .. }
        | PlayerCommand::EntityOp { .. }
        | PlayerCommand::DropItem { .. }
        | PlayerCommand::SpawnEntity { .. } => {}
        // The game loop sends display names to the global region before calling this.
        PlayerCommand::DisplayName { .. } => {
            tracing::debug!("plugin display name reached the player router")
        }
    }
}
/// Queues a Paper entity-scheduler task on the mailbox of the player it belongs to.
fn route_entity_task(
    task: ferrite_plugin::EntityTask,
    mailboxes: &HashMap<Uuid, (u64, Mailbox, OutboundSender)>,
) {
    let mailbox = Uuid::parse_str(&task.entity)
        .ok()
        .and_then(|id| mailboxes.get(&id));
    match mailbox {
        Some((_, mailbox, _)) => mailbox
            .lock()
            .unwrap()
            .push_back(Command::PluginEntityTask(task)),
        // Paper runs the retired callback instead; the entity is gone, so the task never runs.
        None => tracing::debug!(entity = %task.entity, "plugin entity task for an absent entity"),
    }
}
/// Paper `CraftPlayer#sendTitle`: the timings always go out, then whichever texts are set.
fn plugin_title_packets(
    title: Option<String>,
    subtitle: Option<String>,
    fade_in: i32,
    stay: i32,
    fade_out: i32,
) -> Vec<commands_extras::PlayerPacket> {
    use commands_extras::PlayerPacket;
    use ferrite_protocol::types::PlainText;
    let mut packets = vec![PlayerPacket::TitleTimes(SetTitlesAnimation {
        fade_in,
        stay,
        fade_out,
    })];
    if let Some(text) = title {
        packets.push(PlayerPacket::Title(SetTitleText {
            text: PlainText(text),
        }));
    }
    if let Some(text) = subtitle {
        packets.push(PlayerPacket::Subtitle(SetSubtitleText {
            text: PlainText(text),
        }));
    }
    packets
}
/// Paper falls back to `multiplayer.disconnect.kicked` when a plugin kicks without a reason.
fn plugin_kick_reason(component_json: Option<&str>) -> String {
    component_json.map_or_else(|| "Kicked by an operator".to_owned(), json_text)
}
/// Bukkit's `SoundCategory` declares the same constants in the same order as vanilla's
/// `SoundSource`, so the ordinal is the protocol id.
fn plugin_sound_packet(
    sound: &str,
    category: ferrite_plugin::SoundCategory,
    [x, y, z]: [f64; 3],
    volume: f32,
    pitch: f32,
    seed: Option<i64>,
) -> Option<commands_extras::PlayerPacket> {
    use ferrite_plugin::SoundCategory;
    let id = commands_extras::sound_id(sound).ok()?;
    let source = match category {
        SoundCategory::Master => 0,
        SoundCategory::Music => 1,
        SoundCategory::Records => 2,
        SoundCategory::Weather => 3,
        SoundCategory::Blocks => 4,
        SoundCategory::Hostile => 5,
        SoundCategory::Neutral => 6,
        SoundCategory::Players => 7,
        SoundCategory::Ambient => 8,
        SoundCategory::Voice => 9,
        SoundCategory::Ui => 10,
    };
    Some(commands_extras::PlayerPacket::Sound(SoundEffect {
        sound: commands_extras::sound_holder(id),
        source: VarInt(source),
        x: (x * 8.0) as i32,
        y: (y * 8.0) as i32,
        z: (z * 8.0) as i32,
        volume,
        pitch,
        // Paper draws the seed from the player's random; any fresh value will do.
        seed: seed.unwrap_or_else(|| {
            use std::hash::BuildHasher;
            std::collections::hash_map::RandomState::new().hash_one(()) as i64
        }),
    }))
}
fn defer_release(
    worlds: &Worlds,
    signals: &mpsc::UnboundedSender<Signal>,
    dimension: Dimension,
    chunk: ChunkPos,
    flush: bool,
) {
    let signals = signals.clone();
    let transient = worlds.transient_entities.clone();
    let tracking = worlds.tracking.clone();
    worlds.schedule_at_chunk(dimension, chunk, move |region| {
        region.release_chunk(chunk);
        // Entities leave the store before the chunk data is flushed, so a reload reads exactly
        // what was held when the last viewer left.
        if !region.has_chunk_hold(chunk) {
            let mut entities = region.take_chunk_entities(chunk);
            for entity in &mut entities {
                let packet = game::packet(&ferrite_protocol::packets::play::RemoveEntities {
                    ids: vec![VarInt(entity.data.id)],
                });
                for (uuid, _) in entity.data.tracker.viewers() {
                    if let Some((_, _, outbound)) = tracking.lock().unwrap().get(&uuid) {
                        let _ = outbound.send(packet.clone());
                    }
                }
                entity.data.tracker = Default::default();
            }
            if let Some(transient) = transient {
                transient
                    .lock()
                    .unwrap()
                    .insert((dimension, chunk), entities);
            } else {
                let _ = signals.send(Signal::SaveEntities {
                    dimension,
                    chunk,
                    entities,
                });
            }
        }
        let _ = signals.send(Signal::FinishRelease {
            dimension,
            chunk,
            flush,
        });
    });
}
/// Hands an entity to the region owning `to`. A chunk nobody holds parks the entity in the
/// scheduler until a region takes that chunk.
fn hand_off(
    scheduler: &Scheduler<Region, GlobalRegion>,
    to: ChunkPos,
    entity: Box<ferrite_entity::EntityRecord>,
) {
    scheduler.schedule_at_chunk(to, move |r| {
        r.entities.insert(*entity);
    });
}
/// Where command feedback goes: a player's chat, or the console log.
struct Reply<'a> {
    outbound: Option<&'a OutboundSender>,
}

impl Reply<'_> {
    fn key(&self, key: &str, args: &[&str]) {
        self.text(&feedback::render(key, args));
    }

    fn text(&self, text: &str) {
        match self.outbound {
            Some(outbound) => chat(outbound, text),
            None => tracing::info!("{text}"),
        }
    }
}

/// Applies command actions against the live game loop state.
struct CommandRuntime<'a> {
    reply: Reply<'a>,
    source_uuid: Option<Uuid>,
    source_name: String,
    origin: [f64; 3],
    state: &'a Arc<Mutex<commands::GameState>>,
    online: &'a mut HashMap<Uuid, commands::OnlinePlayer>,
    permissions: &'a mut HashMap<Uuid, u8>,
    admin: &'a mut Option<Arc<AdminLists>>,
    whitelist: &'a mut bool,
    saving: &'a mut bool,
    mailboxes: &'a HashMap<Uuid, (u64, Mailbox, OutboundSender)>,
    positions: &'a Arc<Mutex<HashMap<Uuid, [f64; 3]>>>,
    profiler: &'a Arc<profile::Profiler>,
    source: &'a Option<Arc<anvil::AnvilChunkSource>>,
    events: &'a mpsc::UnboundedSender<GameMessage>,
    stop: &'a tokio::sync::watch::Sender<bool>,
    signals: &'a mpsc::UnboundedSender<Signal>,
    worlds: &'a Worlds,
    /// The dimension the command source acts in.
    dimension: Dimension,
    tracking: &'a region::Tracking,
}

fn flush_admin(admin: &AdminLists) {
    if let Err(error) = admin.flush() {
        tracing::error!(%error, "failed to save admin lists");
    }
}

fn valid_teleport(pos: [f64; 3]) -> bool {
    pos.iter()
        .all(|value| value.is_finite() && value.abs() <= 30_000_000.0)
}

/// The closest survival damage kind for a `minecraft:damage_type` key.
fn damage_kind(key: &str) -> survival::DamageKind {
    use survival::DamageKind as K;
    match key.trim_start_matches("minecraft:") {
        "fall" | "ender_pearl" => K::Fall,
        "drown" => K::Drown,
        "lava" => K::Lava,
        "on_fire" | "in_fire" | "campfire" | "hot_floor" => K::Fire,
        "out_of_world" => K::Void,
        "starve" => K::Starve,
        "magic" | "indirect_magic" => K::Magic,
        "wither" => K::Wither,
        "explosion" | "player_explosion" => K::Explosion,
        "mob_attack" | "mob_attack_no_aggro" => K::Mob,
        "arrow" => K::Arrow,
        "generic_kill" => K::GenericKill,
        _ => K::Generic,
    }
}

fn look_angles(from: [f64; 3], to: [f64; 3]) -> (f32, f32) {
    let dx = to[0] - from[0];
    let dy = to[1] - from[1];
    let dz = to[2] - from[2];
    let horizontal = (dx * dx + dz * dz).sqrt();
    let yaw = if dx == 0.0 && dz == 0.0 {
        0.0
    } else {
        dz.atan2(dx).to_degrees() - 90.0
    };
    (
        wrap_yaw(yaw as f32),
        (-(dy.atan2(horizontal))).to_degrees() as f32,
    )
}

fn wrap_yaw(yaw: f32) -> f32 {
    let mut wrapped = (yaw + 180.0) % 360.0;
    if wrapped < 0.0 {
        wrapped += 360.0;
    }
    wrapped - 180.0
}

impl CommandRuntime<'_> {
    fn profiles(
        &self,
        target: &ferrite_command::arguments::EntityTarget,
    ) -> Result<Vec<commands::ResolvedPlayer>, &'static str> {
        commands::resolve_profiles(
            target,
            self.online,
            self.admin.as_deref(),
            self.source_uuid,
            self.origin,
        )
    }

    fn targets(
        &self,
        target: &ferrite_command::arguments::EntityTarget,
    ) -> Result<Vec<commands::ResolvedPlayer>, &'static str> {
        commands::resolve_online(target, self.online, self.source_uuid, self.origin)
    }

    fn teleport_player(&mut self, uuid: Uuid, pos: [f64; 3], yaw: f32, pitch: f32) {
        if let Some(player) = self.online.get_mut(&uuid) {
            player.pos = pos;
            player.yaw = yaw;
            player.pitch = pitch;
        }
        self.positions.lock().unwrap().insert(uuid, pos);
        if let Some((_, mailbox, _)) = self.mailboxes.get(&uuid) {
            mailbox.lock().unwrap().push_back(Command::Teleport {
                pos,
                yaw: Some(yaw),
                pitch: Some(pitch),
            });
        }
    }

    fn selection(
        &self,
        target: &ferrite_command::arguments::EntityTarget,
    ) -> Result<Vec<selectors::SelectedEntity>, ferrite_command::CommandError> {
        let state = self.state.lock().unwrap();
        selectors::resolve_selection(&state, target, self.source_uuid, self.origin)
    }

    /// Applies one teleport to players (through their mailbox) and region entities (through
    /// the region that owns them, so the hand-off still moves the entity between regions).
    fn teleport_selection(
        &mut self,
        targets: &[selectors::SelectedEntity],
        pos: [f64; 3],
        yaw: f32,
        pitch: f32,
        dimension: Dimension,
    ) {
        for target in targets {
            if target.player {
                // `/execute in <dimension> run tp` moves the player across dimensions through
                // the region that owns it, which sends `Respawn` and re-adds its tracking.
                let current = self.online.get(&target.uuid).map(|player| player.dimension);
                if current.is_some_and(|current| current != dimension) {
                    if let Some((_, mailbox, _)) = self.mailboxes.get(&target.uuid) {
                        mailbox.lock().unwrap().push_back(Command::ChangeDimension {
                            dimension,
                            pos,
                            yaw,
                            pitch,
                        });
                    }
                    continue;
                }
                self.teleport_player(target.uuid, pos, yaw, pitch);
            } else {
                self.teleport_entity(target.uuid, target.pos, pos, yaw, pitch);
            }
        }
    }

    fn teleport_entity(&self, uuid: Uuid, from: [f64; 3], pos: [f64; 3], yaw: f32, pitch: f32) {
        let chunk = ChunkPos((from[0].floor() as i32) >> 4, (from[2].floor() as i32) >> 4);
        if self.worlds.owner(self.dimension, chunk).is_some() {
            self.worlds
                .schedule_at_chunk(self.dimension, chunk, move |region| {
                    region.teleport_entity(uuid, pos, yaw, pitch);
                });
        }
    }

    /// Hurts a non-player entity on the region that owns it.
    fn damage_entity(&self, uuid: Uuid, from: [f64; 3], amount: f32) {
        let from = self
            .state
            .lock()
            .unwrap()
            .entities
            .lock()
            .unwrap()
            .get(uuid)
            .map_or(from, |entity| entity.pos);
        let chunk = ChunkPos((from[0].floor() as i32) >> 4, (from[2].floor() as i32) >> 4);
        if self.worlds.owner(self.dimension, chunk).is_some() {
            self.worlds
                .schedule_at_chunk(self.dimension, chunk, move |region| {
                    region.damage_entity(uuid, amount);
                });
        }
    }

    fn kill_entity(&self, uuid: Uuid, from: [f64; 3]) {
        let from = self
            .state
            .lock()
            .unwrap()
            .entities
            .lock()
            .unwrap()
            .get(uuid)
            .map_or(from, |entity| entity.pos);
        let chunk = ChunkPos((from[0].floor() as i32) >> 4, (from[2].floor() as i32) >> 4);
        if self.worlds.owner(self.dimension, chunk).is_some() {
            self.worlds
                .schedule_at_chunk(self.dimension, chunk, move |region| {
                    region.kill_entity(uuid);
                });
        }
    }

    /// `/attribute`: applies one operation to the single target's attribute.
    fn run_attribute(
        &self,
        targets: &[selectors::SelectedEntity],
        attribute: &str,
        op: commands::AttributeOp,
    ) {
        use commands::AttributeOp;
        let [target] = targets else {
            self.reply.key("argument.entity.toomany", &[]);
            return;
        };
        let path = attribute.strip_prefix("minecraft:").unwrap_or(attribute);
        let Some(limits) = attributes::default_for(path, target.player) else {
            self.reply.key(
                "commands.attribute.failed.no_attribute",
                &[&target.name, attribute],
            );
            return;
        };
        let key = (target.uuid, path.to_owned());
        let mut state = self.state.lock().unwrap();
        let stored = state.more.attributes.entry(key.clone()).or_default();
        let number = |value: f64| format!("{value}");
        let name = target.name.as_str();
        match op {
            AttributeOp::GetValue { scale } => {
                let value = stored.value(limits, limits.default);
                self.reply.key(
                    "commands.attribute.value.get.success",
                    &[attribute, name, &number(value)],
                );
                let _ = scale;
            }
            AttributeOp::GetBase { scale } => {
                let value = stored.base.unwrap_or(limits.default);
                self.reply.key(
                    "commands.attribute.base_value.get.success",
                    &[attribute, name, &number(value * scale)],
                );
            }
            AttributeOp::SetBase(value) => {
                stored.base = Some(value.clamp(limits.min, limits.max));
                self.reply.key(
                    "commands.attribute.base_value.set.success",
                    &[attribute, name, &number(value)],
                );
            }
            AttributeOp::ResetBase => {
                stored.base = None;
                self.reply.key(
                    "commands.attribute.base_value.reset.success",
                    &[attribute, name, &number(limits.default)],
                );
            }
            AttributeOp::AddModifier {
                id,
                amount,
                operation,
            } => {
                if stored.modifiers.contains_key(&id) {
                    self.reply.key(
                        "commands.attribute.failed.modifier_already_present",
                        &[name, attribute, &id],
                    );
                } else {
                    stored.modifiers.insert(id.clone(), (amount, operation));
                    self.reply.key(
                        "commands.attribute.modifier.add.success",
                        &[&id, attribute, name],
                    );
                }
            }
            AttributeOp::RemoveModifier { id } => {
                if stored.modifiers.remove(&id).is_some() {
                    self.reply.key(
                        "commands.attribute.modifier.remove.success",
                        &[&id, attribute, name],
                    );
                } else {
                    self.reply.key(
                        "commands.attribute.failed.no_modifier",
                        &[name, attribute, &id],
                    );
                }
            }
            AttributeOp::GetModifier { id, scale } => match stored.modifiers.get(&id) {
                Some((amount, _)) => self.reply.key(
                    "commands.attribute.modifier.value.get.success",
                    &[&id, attribute, name, &number(amount * scale)],
                ),
                None => self.reply.key(
                    "commands.attribute.failed.no_modifier",
                    &[name, attribute, &id],
                ),
            },
        }
        if stored.base.is_none() && stored.modifiers.is_empty() {
            state.more.attributes.remove(&key);
        }
    }

    fn teleport_feedback(
        &self,
        targets: &[selectors::SelectedEntity],
        pos: [f64; 3],
        entity: Option<&str>,
    ) {
        if let Some(entity) = entity {
            if targets.len() == 1 {
                self.reply.key(
                    "commands.teleport.success.entity.single",
                    &[&targets[0].name, entity],
                );
            } else {
                let count = targets.len().to_string();
                self.reply.key(
                    "commands.teleport.success.entity.multiple",
                    &[&count, entity],
                );
            }
            return;
        }
        let x = ferrite_command::error::java_double(pos[0]);
        let y = ferrite_command::error::java_double(pos[1]);
        let z = ferrite_command::error::java_double(pos[2]);
        if targets.len() == 1 {
            self.reply.key(
                "commands.teleport.success.location.single",
                &[&targets[0].name, &x, &y, &z],
            );
        } else {
            let count = targets.len().to_string();
            self.reply.key(
                "commands.teleport.success.location.multiple",
                &[&count, &x, &y, &z],
            );
        }
    }

    async fn apply(&mut self, action: commands::Action) -> bool {
        use commands::{Action, TeleportDestination};
        match action {
            Action::GameMode { .. }
            | Action::DefaultGameMode(_)
            | Action::Difficulty(_)
            | Action::TimeSet(_)
            | Action::TimeAdd(_)
            | Action::Weather { .. }
            | Action::Feedback(_)
            | Action::PlayerFeedback { .. }
            | Action::Broadcast(_)
            | Action::Clear { .. }
            | Action::Give { .. }
            | Action::Experience { .. }
            | Action::Summon { .. }
            | Action::GameRule(..)
            | Action::Enchant { .. } => false,
            Action::World(command) => {
                let origin = commands_world::Origin {
                    pos: self.origin,
                    yaw: 0.0,
                    pitch: 0.0,
                };
                let chunk = commands_world::command_chunk(&command, &origin);
                if self.worlds.owner(self.dimension, chunk).is_some() {
                    if let Some(source) = self.source.clone() {
                        let tracking = Arc::clone(self.tracking);
                        self.worlds
                            .schedule_at_chunk(self.dimension, chunk, move |_region| {
                                commands_world::apply_world(&command, &origin, &*source, &tracking);
                            });
                    }
                } else if let Some(source) = self.source
                    && let Err(error) = commands_world::execute(&command, &origin, &**source)
                {
                    self.reply.text(&error);
                }
                false
            }
            Action::Data(command) => {
                match commands_data::apply_block(self.source.as_deref(), &command) {
                    Ok(feedback) => self.reply.text(&feedback.text),
                    Err(message) => self.reply.text(&message),
                }
                false
            }
            Action::Reload => {
                let world = self
                    .source
                    .as_ref()
                    .and_then(|source| source.path())
                    .map(std::path::Path::to_path_buf);
                match functions::reload(world.as_deref(), self.state) {
                    Ok(()) => self.reply.key("commands.reload.success", &[]),
                    Err(error) => self.reply.key("commands.reload.failure", &[&error]),
                }
                false
            }
            Action::DatapackList => {
                let world = self.source.as_ref().and_then(|source| source.path());
                let packs = functions::datapack_names(world);
                if packs.is_empty() {
                    self.reply.key("commands.datapack.list.enabled.none", &[]);
                    return false;
                }
                let disabled = world.map(functions::read_disabled).unwrap_or_default();
                let enabled: Vec<&str> = packs
                    .iter()
                    .filter(|pack| !disabled.contains(*pack))
                    .map(String::as_str)
                    .collect();
                if enabled.is_empty() {
                    self.reply.key("commands.datapack.list.enabled.none", &[]);
                } else {
                    self.reply.key(
                        "commands.datapack.list.enabled.success",
                        &[&enabled.len().to_string(), &enabled.join(", ")],
                    );
                }
                let available: Vec<&str> = packs
                    .iter()
                    .filter(|pack| disabled.contains(*pack))
                    .map(String::as_str)
                    .collect();
                if !available.is_empty() {
                    self.reply.key(
                        "commands.datapack.list.available.success",
                        &[&available.len().to_string(), &available.join(", ")],
                    );
                }
                false
            }
            Action::DatapackSet { pack, enable } => {
                let Some(world) = self
                    .source
                    .as_ref()
                    .and_then(|source| source.path())
                    .map(std::path::Path::to_path_buf)
                else {
                    self.reply.key("commands.datapack.unknown", &[&pack]);
                    return false;
                };
                let packs = functions::datapack_names(Some(&world));
                if !packs.contains(&pack) {
                    self.reply.key("commands.datapack.unknown", &[&pack]);
                    return false;
                }
                let mut disabled = functions::read_disabled(&world);
                let already = !disabled.contains(&pack);
                if enable == already {
                    self.reply.key(
                        if enable {
                            "commands.datapack.enable.failed"
                        } else {
                            "commands.datapack.disable.failed"
                        },
                        &[&pack],
                    );
                    return false;
                }
                if enable {
                    disabled.remove(&pack);
                } else {
                    disabled.insert(pack.clone());
                }
                if let Err(error) = functions::write_disabled(&world, &disabled) {
                    self.reply
                        .key("commands.reload.failure", &[&error.to_string()]);
                    return false;
                }
                match functions::reload(Some(&world), self.state) {
                    Ok(()) => self.reply.key(
                        if enable {
                            "commands.datapack.enable.success"
                        } else {
                            "commands.datapack.disable.success"
                        },
                        &[&pack],
                    ),
                    Err(error) => self.reply.key("commands.reload.failure", &[&error]),
                }
                false
            }
            // Plugin commands run in the dispatch loops, which own the plugin hooks.
            Action::Plugin { .. } => false,
            Action::Teleport {
                targets,
                destination,
                dimension,
            } => {
                let targets = match self.selection(&targets) {
                    Ok(targets) => targets,
                    Err(error) => {
                        self.reply.text(&error.to_string());
                        return false;
                    }
                };
                match destination {
                    TeleportDestination::Entity(destination) => {
                        let destination = match self.selection(&destination) {
                            Ok(mut entities) if entities.len() == 1 => entities.remove(0),
                            Ok(_) => {
                                self.reply.key("argument.entity.notfound.player", &[]);
                                return false;
                            }
                            Err(error) => {
                                self.reply.text(&error.to_string());
                                return false;
                            }
                        };
                        if !valid_teleport(destination.pos) {
                            self.reply.key("commands.teleport.invalidPosition", &[]);
                            return false;
                        }
                        self.teleport_selection(
                            &targets,
                            destination.pos,
                            destination.yaw,
                            destination.pitch,
                            dimension,
                        );
                        self.teleport_feedback(&targets, destination.pos, Some(&destination.name));
                    }
                    TeleportDestination::Location { pos, yaw, pitch } => {
                        if !valid_teleport(pos) {
                            self.reply.key("commands.teleport.invalidPosition", &[]);
                            return false;
                        }
                        for target in &targets {
                            self.teleport_selection(
                                std::slice::from_ref(target),
                                pos,
                                yaw.unwrap_or(target.yaw),
                                pitch.unwrap_or(target.pitch),
                                dimension,
                            );
                        }
                        self.teleport_feedback(&targets, pos, None);
                    }
                    TeleportDestination::FacingLocation { pos, target } => {
                        if !valid_teleport(pos) {
                            self.reply.key("commands.teleport.invalidPosition", &[]);
                            return false;
                        }
                        let (yaw, pitch) = look_angles([pos[0], pos[1] + 1.62, pos[2]], target);
                        self.teleport_selection(&targets, pos, yaw, pitch, dimension);
                        self.teleport_feedback(&targets, pos, None);
                    }
                    TeleportDestination::FacingEntity {
                        pos,
                        entity,
                        anchor,
                    } => {
                        if !valid_teleport(pos) {
                            self.reply.key("commands.teleport.invalidPosition", &[]);
                            return false;
                        }
                        let facing = match self.selection(&entity) {
                            Ok(mut entities) if entities.len() == 1 => entities.remove(0),
                            Ok(_) => {
                                self.reply.key("argument.entity.notfound.player", &[]);
                                return false;
                            }
                            Err(error) => {
                                self.reply.text(&error.to_string());
                                return false;
                            }
                        };
                        let point = [
                            facing.pos[0],
                            facing.pos[1] + anchor.offset(),
                            facing.pos[2],
                        ];
                        let (yaw, pitch) = look_angles([pos[0], pos[1] + 1.62, pos[2]], point);
                        self.teleport_selection(&targets, pos, yaw, pitch, dimension);
                        self.teleport_feedback(&targets, pos, None);
                    }
                }
                false
            }
            Action::Message { targets, text } => {
                let targets = match self.targets(&targets) {
                    Ok(targets) => targets,
                    Err(key) => {
                        self.reply.key(key, &[]);
                        return false;
                    }
                };
                for target in &targets {
                    if let Some(player) = self.online.get(&target.uuid) {
                        chat(
                            &player.outbound,
                            &feedback::render(
                                "commands.message.display.incoming",
                                &[&self.source_name, &text],
                            ),
                        );
                    }
                }
                let names = targets
                    .iter()
                    .map(|player| player.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                if self.source_uuid.is_some() {
                    self.reply
                        .key("commands.message.display.outgoing", &[&names, &text]);
                } else {
                    self.reply.text(&feedback::render(
                        "commands.message.display.outgoing",
                        &[&names, &text],
                    ));
                }
                false
            }
            Action::Damage {
                targets,
                amount,
                damage_type,
            } => {
                let targets = match self.selection(&targets) {
                    Ok(targets) => targets,
                    Err(error) => {
                        self.reply.text(&error.to_string());
                        return false;
                    }
                };
                let kind = damage_kind(&damage_type);
                let mut hurt = 0;
                for target in &targets {
                    if target.player {
                        if let Some((_, mailbox, _)) = self.mailboxes.get(&target.uuid) {
                            mailbox
                                .lock()
                                .unwrap()
                                .push_back(Command::Damage(kind, amount));
                            hurt += 1;
                            self.reply.key(
                                "commands.damage.success",
                                &[&format!("{amount}"), &target.name],
                            );
                        }
                    } else {
                        self.damage_entity(target.uuid, target.pos, amount);
                        hurt += 1;
                        self.reply.key(
                            "commands.damage.success",
                            &[&format!("{amount}"), &target.name],
                        );
                    }
                }
                if hurt == 0 {
                    self.reply.key("commands.damage.invulnerable", &[]);
                }
                false
            }
            Action::Spread {
                targets,
                center,
                spread,
                range,
                respect_teams,
                max_height,
            } => {
                let targets = match self.selection(&targets) {
                    Ok(targets) if !targets.is_empty() => targets,
                    Ok(_) => {
                        self.reply.key("argument.entity.notfound.entity", &[]);
                        return false;
                    }
                    Err(error) => {
                        self.reply.text(&error.to_string());
                        return false;
                    }
                };
                // Units that share one destination: each entity alone, or a whole team.
                let mut units: Vec<Vec<usize>> = Vec::new();
                {
                    let game = self.state.lock().unwrap();
                    let mut teams: std::collections::BTreeMap<String, Vec<usize>> =
                        std::collections::BTreeMap::new();
                    for (index, target) in targets.iter().enumerate() {
                        let team = respect_teams
                            .then(|| {
                                game.players
                                    .iter()
                                    .find(|player| player.uuid == target.uuid)
                                    .and_then(|player| player.team.clone())
                            })
                            .flatten();
                        match team {
                            Some(team) => teams.entry(team).or_default().push(index),
                            None => units.push(vec![index]),
                        }
                    }
                    units.extend(teams.into_values());
                }
                let seed = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0x9E37_79B9, |d| d.as_nanos() as u64)
                    | 1;
                let by_team = respect_teams && units.iter().any(|unit| unit.len() > 1);
                let center_text = [format!("{}", center.0), format!("{}", center.1)];
                let Some(points) =
                    commands_more::spread_points(units.len(), center, spread, range, seed)
                else {
                    let key = if by_team {
                        "commands.spreadplayers.failed.teams"
                    } else {
                        "commands.spreadplayers.failed.entities"
                    };
                    self.reply.key(
                        key,
                        &[
                            &units.len().to_string(),
                            &center_text[0],
                            &center_text[1],
                            &format!("{spread}"),
                        ],
                    );
                    return false;
                };
                for (unit, (x, z)) in units.iter().zip(&points) {
                    let (bx, bz) = (x.floor() as i32, z.floor() as i32);
                    let surface = self
                        .source
                        .as_ref()
                        .and_then(|world| world.spawn_height(bx, bz))
                        .unwrap_or(self.origin[1] as i32);
                    let y = max_height.map_or(surface, |limit| surface.min(limit));
                    let pos = [f64::from(bx) + 0.5, f64::from(y), f64::from(bz) + 0.5];
                    for &index in unit {
                        let target = &targets[index];
                        self.teleport_selection(
                            std::slice::from_ref(target),
                            pos,
                            target.yaw,
                            target.pitch,
                            self.dimension,
                        );
                    }
                }
                let mut total = 0.0;
                for (i, a) in points.iter().enumerate() {
                    total += points
                        .iter()
                        .enumerate()
                        .filter(|(j, _)| *j != i)
                        .map(|(_, b)| (a.0 - b.0).hypot(a.1 - b.1))
                        .fold(f64::MAX, f64::min)
                        .min(1.0e9);
                }
                let average = if points.len() > 1 {
                    total / points.len() as f64
                } else {
                    0.0
                };
                self.reply.key(
                    if by_team {
                        "commands.spreadplayers.success.teams"
                    } else {
                        "commands.spreadplayers.success.entities"
                    },
                    &[
                        &units.len().to_string(),
                        &center_text[0],
                        &center_text[1],
                        &format!("{average:.2}"),
                    ],
                );
                false
            }
            Action::ForceLoad {
                dimension,
                add,
                remove,
            } => {
                for (x, z) in add {
                    let _ = self.signals.send(Signal::Hold {
                        dimension,
                        chunk: ChunkPos(x, z),
                        add: true,
                    });
                }
                for (x, z) in remove {
                    let _ = self.signals.send(Signal::Hold {
                        dimension,
                        chunk: ChunkPos(x, z),
                        add: false,
                    });
                }
                false
            }
            Action::Rotate { targets, to } => {
                let targets = match self.selection(&targets) {
                    Ok(targets) => targets,
                    Err(error) => {
                        self.reply.text(&error.to_string());
                        return false;
                    }
                };
                let facing = match &to {
                    commands::RotateTo::FacingEntity { entity, anchor } => {
                        match self.selection(entity) {
                            Ok(mut entities) if entities.len() == 1 => {
                                let found = entities.remove(0);
                                Some([found.pos[0], found.pos[1] + anchor.offset(), found.pos[2]])
                            }
                            Ok(_) => {
                                self.reply.key("argument.entity.notfound.entity", &[]);
                                return false;
                            }
                            Err(error) => {
                                self.reply.text(&error.to_string());
                                return false;
                            }
                        }
                    }
                    commands::RotateTo::FacingLocation(point) => Some(*point),
                    commands::RotateTo::Angles(_) => None,
                };
                for target in &targets {
                    let (yaw, pitch) = match (&to, facing) {
                        (commands::RotateTo::Angles(angles), _) => {
                            ferrite_command::arguments::coords::resolve_rotation(
                                *angles,
                                target.yaw,
                                target.pitch,
                            )
                        }
                        (_, Some(point)) => {
                            look_angles([target.pos[0], target.pos[1] + 1.62, target.pos[2]], point)
                        }
                        _ => (target.yaw, target.pitch),
                    };
                    self.teleport_selection(
                        std::slice::from_ref(target),
                        target.pos,
                        yaw,
                        pitch,
                        self.dimension,
                    );
                }
                if let Some(first) = targets.first() {
                    self.reply.key("commands.rotate.success", &[&first.name]);
                }
                false
            }
            Action::Spectate { viewer, target } => {
                let mode = self
                    .state
                    .lock()
                    .unwrap()
                    .players
                    .iter()
                    .find(|p| p.uuid == viewer)
                    .map(|p| p.game_mode);
                let Some(online) = self.online.get(&viewer) else {
                    return false;
                };
                if mode != Some(3) {
                    self.reply
                        .key("commands.spectate.not_spectator", &[&online.name]);
                    return false;
                }
                let Some(target) = target else {
                    let _ = online.outbound.send(ferrite_net::game::packet(
                        &ferrite_protocol::packets::play::SetCamera {
                            entity: VarInt(online.entity),
                        },
                    ));
                    self.reply.key("commands.spectate.success.stopped", &[]);
                    return false;
                };
                let found = match self.selection(&target) {
                    Ok(mut found) if found.len() == 1 => found.remove(0),
                    Ok(_) => {
                        self.reply.key("argument.entity.toomany", &[]);
                        return false;
                    }
                    Err(error) => {
                        self.reply.text(&error.to_string());
                        return false;
                    }
                };
                if found.uuid == viewer {
                    self.reply.key("commands.spectate.self", &[]);
                    return false;
                }
                let id = match self.online.get(&found.uuid) {
                    Some(other) => other.entity,
                    None => match self
                        .state
                        .lock()
                        .unwrap()
                        .entities
                        .lock()
                        .unwrap()
                        .get(found.uuid)
                    {
                        Some(entity) => entity.network_id,
                        None => {
                            self.reply
                                .key("commands.spectate.cannot_spectate", &[&found.name]);
                            return false;
                        }
                    },
                };
                let _ = online.outbound.send(ferrite_net::game::packet(
                    &ferrite_protocol::packets::play::SetCamera { entity: VarInt(id) },
                ));
                self.reply
                    .key("commands.spectate.success.started", &[&found.name]);
                false
            }
            Action::Attribute {
                targets,
                attribute,
                op,
            } => {
                let targets = match self.selection(&targets) {
                    Ok(targets) => targets,
                    Err(error) => {
                        self.reply.text(&error.to_string());
                        return false;
                    }
                };
                self.run_attribute(&targets, &attribute, op);
                false
            }
            Action::ResendChunks { dimension, chunks } => {
                if let Some(source) = self.source.clone() {
                    let tracking = self.tracking.lock().unwrap();
                    for (x, z) in chunks {
                        if let Some(Ok(bytes)) = source.get(x, z, 0) {
                            let packet = Outbound::Packet(bytes.to_vec().into());
                            region::broadcast_tracked_chunk(&tracking, dimension, (x, z), &packet);
                        }
                    }
                }
                false
            }
            Action::Slots {
                mode,
                target,
                slots,
                source,
            } => {
                self.run_slots(mode, target, slots, source);
                false
            }
            Action::DropItems { pos, stacks } => {
                let chunk = ChunkPos((pos[0].floor() as i32) >> 4, (pos[2].floor() as i32) >> 4);
                if self.worlds.owner(self.dimension, chunk).is_none() {
                    self.reply.key("argument.pos.unloaded", &[]);
                    return false;
                }
                let total: i32 = stacks.iter().map(|s| s.count).sum();
                let first = stacks
                    .first()
                    .map(|s| commands::item_display(s.item))
                    .unwrap_or_default();
                let single = stacks.len() == 1;
                self.worlds
                    .schedule_at_chunk(self.dimension, chunk, move |region| {
                        for stack in stacks {
                            if let (Some(name), Ok(amount)) =
                                (stack.name(), u8::try_from(stack.count))
                            {
                                region.plugin_drop_item(pos, name, amount, true, Uuid::new_v4());
                            }
                        }
                    });
                if single {
                    self.reply.key(
                        "commands.drop.success.single",
                        &[&total.to_string(), &first],
                    );
                } else {
                    self.reply
                        .key("commands.drop.success.multiple", &[&total.to_string()]);
                }
                false
            }
            Action::Swing { targets, off_hand } => {
                let targets = match self.selection(&targets) {
                    Ok(targets) => targets,
                    Err(error) => {
                        self.reply.text(&error.to_string());
                        return false;
                    }
                };
                let action = if off_hand { 3 } else { 0 };
                for target in &targets {
                    if target.player {
                        if let Some((_, mailbox, _)) = self.mailboxes.get(&target.uuid) {
                            mailbox.lock().unwrap().push_back(Command::PluginPacket(
                                ferrite_plugin::PluginPacket::Animation { action },
                            ));
                        }
                    } else {
                        let uuid = target.uuid;
                        let chunk = ChunkPos(
                            (target.pos[0].floor() as i32) >> 4,
                            (target.pos[2].floor() as i32) >> 4,
                        );
                        if self.worlds.owner(self.dimension, chunk).is_some() {
                            self.worlds
                                .schedule_at_chunk(self.dimension, chunk, move |region| {
                                    region.swing_entity(uuid, action);
                                });
                        }
                    }
                }
                match targets.as_slice() {
                    [] => {}
                    [only] => self
                        .reply
                        .key("commands.swing.success.single", &[&only.name]),
                    many => {
                        let count = many.len().to_string();
                        self.reply.key("commands.swing.success.multiple", &[&count]);
                    }
                }
                false
            }
            Action::Kill { targets } => {
                let targets = match self.selection(&targets) {
                    Ok(targets) => targets,
                    Err(error) => {
                        self.reply.text(&error.to_string());
                        return false;
                    }
                };
                for target in &targets {
                    if target.player {
                        // `/kill` deals `genericKill` damage on the region owning the player.
                        if let Some((_, mailbox, _)) = self.mailboxes.get(&target.uuid) {
                            mailbox.lock().unwrap().push_back(Command::Kill);
                        }
                    } else {
                        self.kill_entity(target.uuid, target.pos);
                    }
                }
                if targets.len() == 1 {
                    self.reply
                        .key("commands.kill.success.single", &[&targets[0].name]);
                } else {
                    let count = targets.len().to_string();
                    self.reply.key("commands.kill.success.multiple", &[&count]);
                }
                false
            }
            Action::Op { targets } => {
                let Some(lists) = self.admin.clone() else {
                    self.reply.text("Administration lists are unavailable");
                    return false;
                };
                let players = match self.profiles(&targets) {
                    Ok(players) => players,
                    Err(key) => {
                        self.reply.key(key, &[]);
                        return false;
                    }
                };
                let mut changed = false;
                for player in players {
                    if lists.ops.level_of(player.uuid).is_some() {
                        self.reply.key("commands.op.failed", &[]);
                        continue;
                    }
                    lists.ops.add_op(player.uuid, &player.name, 4, false);
                    if let Some(level) = self.permissions.get_mut(&player.uuid) {
                        *level = 4;
                    }
                    self.reply.key("commands.op.success", &[&player.name]);
                    changed = true;
                }
                if changed {
                    flush_admin(&lists);
                }
                false
            }
            Action::Deop { targets } => {
                let Some(lists) = self.admin.clone() else {
                    self.reply.text("Administration lists are unavailable");
                    return false;
                };
                let players = match self.profiles(&targets) {
                    Ok(players) => players,
                    Err(key) => {
                        self.reply.key(key, &[]);
                        return false;
                    }
                };
                let mut changed = false;
                for player in players {
                    if !lists.ops.remove_op(player.uuid) {
                        self.reply.key("commands.deop.failed", &[]);
                        continue;
                    }
                    if let Some(level) = self.permissions.get_mut(&player.uuid) {
                        *level = 0;
                    }
                    self.reply.key("commands.deop.success", &[&player.name]);
                    changed = true;
                }
                if changed {
                    flush_admin(&lists);
                }
                false
            }
            Action::WhitelistAdd { targets } => {
                let Some(lists) = self.admin.clone() else {
                    self.reply.text("Administration lists are unavailable");
                    return false;
                };
                let players = match self.profiles(&targets) {
                    Ok(players) => players,
                    Err(key) => {
                        self.reply.key(key, &[]);
                        return false;
                    }
                };
                let mut changed = false;
                for player in players {
                    if lists.whitelist.contains(player.uuid) {
                        self.reply.key("commands.whitelist.add.failed", &[]);
                        continue;
                    }
                    lists.whitelist.add(player.uuid, &player.name);
                    self.reply
                        .key("commands.whitelist.add.success", &[&player.name]);
                    changed = true;
                }
                if changed {
                    flush_admin(&lists);
                }
                false
            }
            Action::WhitelistRemove { targets } => {
                let Some(lists) = self.admin.clone() else {
                    self.reply.text("Administration lists are unavailable");
                    return false;
                };
                let players = match self.profiles(&targets) {
                    Ok(players) => players,
                    Err(key) => {
                        let removed = match &targets {
                            ferrite_command::arguments::EntityTarget::Name(name)
                                if lists.whitelist.remove_name(name) =>
                            {
                                self.reply.key("commands.whitelist.remove.success", &[name]);
                                true
                            }
                            _ => false,
                        };
                        if removed {
                            flush_admin(&lists);
                        } else {
                            self.reply.key(key, &[]);
                        }
                        return false;
                    }
                };
                let mut changed = false;
                for player in players {
                    if lists.whitelist.remove(player.uuid) {
                        self.reply
                            .key("commands.whitelist.remove.success", &[&player.name]);
                        changed = true;
                    } else {
                        self.reply.key("commands.whitelist.remove.failed", &[]);
                    }
                }
                if changed {
                    flush_admin(&lists);
                }
                false
            }
            Action::WhitelistList => {
                let Some(lists) = self.admin.clone() else {
                    self.reply.text("Administration lists are unavailable");
                    return false;
                };
                let names = lists.whitelist.names();
                if names.is_empty() {
                    self.reply.key("commands.whitelist.none", &[]);
                } else {
                    let count = names.len().to_string();
                    let joined = names.join(", ");
                    self.reply
                        .key("commands.whitelist.list", &[&count, &joined]);
                }
                false
            }
            Action::WhitelistOn => {
                if *self.whitelist {
                    self.reply.key("commands.whitelist.alreadyOn", &[]);
                } else {
                    *self.whitelist = true;
                    self.reply.key("commands.whitelist.enabled", &[]);
                }
                false
            }
            Action::WhitelistOff => {
                if !*self.whitelist {
                    self.reply.key("commands.whitelist.alreadyOff", &[]);
                } else {
                    *self.whitelist = false;
                    self.reply.key("commands.whitelist.disabled", &[]);
                }
                false
            }
            Action::WhitelistReload => {
                let Some(dir) = self.admin.as_ref().map(|lists| lists.dir().to_path_buf()) else {
                    self.reply.text("Administration lists are unavailable");
                    return false;
                };
                match AdminLists::load(&dir) {
                    Ok(lists) => {
                        *self.admin = Some(Arc::new(lists));
                        self.reply.key("commands.whitelist.reloaded", &[]);
                    }
                    Err(error) => self
                        .reply
                        .text(&format!("Failed to reload the whitelist: {error}")),
                }
                false
            }
            Action::Ban { targets, reason } => {
                let Some(lists) = self.admin.clone() else {
                    self.reply.text("Administration lists are unavailable");
                    return false;
                };
                let players = match self.profiles(&targets) {
                    Ok(players) => players,
                    Err(key) => {
                        self.reply.key(key, &[]);
                        return false;
                    }
                };
                let mut changed = false;
                for player in players {
                    if lists.bans.is_banned(player.uuid) {
                        self.reply.key("commands.ban.failed", &[]);
                        continue;
                    }
                    lists
                        .bans
                        .ban(player.uuid, &player.name, &reason, &self.source_name, None);
                    if let Some(online) = self.online.get(&player.uuid) {
                        let _ = online.outbound.send(Outbound::Disconnect(reason.clone()));
                    }
                    self.reply
                        .key("commands.ban.success", &[&player.name, &reason]);
                    changed = true;
                }
                if changed {
                    flush_admin(&lists);
                }
                false
            }
            Action::BanIp { target, reason } => {
                let Some(lists) = self.admin.clone() else {
                    self.reply.text("Administration lists are unavailable");
                    return false;
                };
                let Some(ip) = commands::parse_ip(&target) else {
                    self.reply.key("commands.banip.invalid", &[]);
                    return false;
                };
                if lists.bans.is_ip_banned(&ip.to_string()) {
                    self.reply.key("commands.banip.failed", &[]);
                    return false;
                }
                lists
                    .bans
                    .ban_ip(&ip.to_string(), &reason, &self.source_name, None);
                flush_admin(&lists);
                self.reply
                    .key("commands.banip.success", &[&ip.to_string(), &reason]);
                false
            }
            Action::Pardon { targets } => {
                let Some(lists) = self.admin.clone() else {
                    self.reply.text("Administration lists are unavailable");
                    return false;
                };
                let players = match self.profiles(&targets) {
                    Ok(players) => players,
                    Err(key) => {
                        let name = match &targets {
                            ferrite_command::arguments::EntityTarget::Name(name) => Some(name),
                            _ => None,
                        };
                        let removed = name.and_then(|name| {
                            lists
                                .bans
                                .player_bans()
                                .into_iter()
                                .find(|ban| ban.name.eq_ignore_ascii_case(name))
                        });
                        if let Some(ban) = removed {
                            lists.bans.unban(ban.uuid);
                            flush_admin(&lists);
                            self.reply.key("commands.pardon.success", &[name.unwrap()]);
                        } else {
                            self.reply.key(key, &[]);
                        }
                        return false;
                    }
                };
                let mut changed = false;
                for player in players {
                    if lists.bans.unban(player.uuid) {
                        self.reply.key("commands.pardon.success", &[&player.name]);
                        changed = true;
                    } else {
                        self.reply.key("commands.pardon.failed", &[]);
                    }
                }
                if changed {
                    flush_admin(&lists);
                }
                false
            }
            Action::PardonIp { target } => {
                let Some(lists) = self.admin.clone() else {
                    self.reply.text("Administration lists are unavailable");
                    return false;
                };
                let Some(ip) = commands::parse_ip(&target) else {
                    self.reply.key("commands.pardonip.invalid", &[]);
                    return false;
                };
                if lists.bans.unban_ip(&ip.to_string()) {
                    flush_admin(&lists);
                    self.reply
                        .key("commands.pardonip.success", &[&ip.to_string()]);
                } else {
                    self.reply.key("commands.pardonip.failed", &[]);
                }
                false
            }
            Action::BanList { ips } => {
                let Some(lists) = self.admin.clone() else {
                    self.reply.text("Administration lists are unavailable");
                    return false;
                };
                if ips {
                    let bans = lists.bans.ip_bans();
                    if bans.is_empty() {
                        self.reply.key("commands.banlist.none", &[]);
                    } else {
                        let count = bans.len().to_string();
                        self.reply.key("commands.banlist.list", &[&count]);
                        for ban in bans {
                            self.reply.key(
                                "commands.banlist.entry",
                                &[&ban.ip, &ban.source, &ban.reason],
                            );
                        }
                    }
                } else {
                    let bans = lists.bans.player_bans();
                    if bans.is_empty() {
                        self.reply.key("commands.banlist.none", &[]);
                    } else {
                        let count = bans.len().to_string();
                        self.reply.key("commands.banlist.list", &[&count]);
                        for ban in bans {
                            self.reply.key(
                                "commands.banlist.entry",
                                &[&ban.name, &ban.source, &ban.reason],
                            );
                        }
                    }
                }
                false
            }
            Action::Kick { targets, reason } => {
                let players = match self.targets(&targets) {
                    Ok(players) => players,
                    Err(key) => {
                        self.reply.key(key, &[]);
                        return false;
                    }
                };
                for player in players {
                    if let Some(online) = self.online.get(&player.uuid) {
                        let _ = online.outbound.send(Outbound::Disconnect(reason.clone()));
                    }
                    self.reply
                        .key("commands.kick.success", &[&player.name, &reason]);
                }
                false
            }
            Action::Stop => {
                self.reply.key("commands.stop.stopping", &[]);
                let _ = self.stop.send(true);
                let (complete, _) = oneshot::channel();
                let _ = self.events.send(GameMessage::Shutdown { complete });
                true
            }
            Action::SaveAll { flush: _ } => {
                self.reply.key("commands.save.saving", &[]);
                let result = if let Some(source) = self.source {
                    source.flush_all().await
                } else {
                    Ok(())
                };
                match result {
                    Ok(()) => self.reply.key("commands.save.success", &[]),
                    Err(error) => {
                        tracing::error!(%error, "save-all failed");
                        self.reply.key("commands.save.failed", &[]);
                    }
                }
                false
            }
            Action::SaveOn => {
                if *self.saving {
                    self.reply.key("commands.save.alreadyOn", &[]);
                } else {
                    *self.saving = true;
                    self.reply.key("commands.save.enabled", &[]);
                }
                false
            }
            Action::SaveOff => {
                if !*self.saving {
                    self.reply.key("commands.save.alreadyOff", &[]);
                } else {
                    *self.saving = false;
                    self.reply.key("commands.save.disabled", &[]);
                }
                false
            }
            Action::Tps => {
                let lines = if self.reply.outbound.is_some() {
                    self.profiler.tps_lines()
                } else {
                    self.profiler.tps_lines_plain()
                };
                for line in lines {
                    self.reply.text(&line);
                }
                false
            }
            Action::Mspt => {
                let lines = if self.reply.outbound.is_some() {
                    self.profiler.mspt_lines()
                } else {
                    self.profiler.mspt_lines_plain()
                };
                for line in lines {
                    self.reply.text(&line);
                }
                false
            }
            Action::ProfileStart => {
                match self.profiler.start_profile() {
                    Ok(()) => self
                        .reply
                        .text("Profiling started; run /profile stop to dump the slowest systems"),
                    Err(error) => self.reply.text(&format!("Cannot start profiling: {error}")),
                }
                false
            }
            Action::ProfileStop => {
                match self.profiler.stop_profile() {
                    Ok(report) => {
                        for line in report.lines() {
                            self.reply.text(&line);
                        }
                    }
                    Err(error) => self.reply.text(&format!("Cannot stop profiling: {error}")),
                }
                false
            }
            Action::Packets { targets, packet } => {
                if targets.is_empty() {
                    for player in self.online.values() {
                        packet.send_to(&player.outbound);
                    }
                } else {
                    for target in &targets {
                        if let Some(player) = self.online.get(target) {
                            packet.send_to(&player.outbound);
                        }
                    }
                }
                false
            }
            Action::Effect {
                targets,
                effect,
                amplifier,
                duration,
            } => {
                // The client packet keeps command order; the region owns the ticking store.
                for target in targets {
                    if let Some(player) = self.online.get(&target) {
                        let _ = player
                            .outbound
                            .send(ferrite_net::game::packet(&UpdateMobEffect {
                                entity_id: VarInt(player.entity),
                                effect: VarInt(effect),
                                amplifier: VarInt(amplifier),
                                duration: VarInt(duration),
                                // ambient | show particles | show icon, like a normal potion effect.
                                flags: 0b110,
                            }));
                    }
                    if let Some((_, mailbox, _)) = self.mailboxes.get(&target) {
                        mailbox.lock().unwrap().push_back(Command::ApplyEffect {
                            effect,
                            amplifier,
                            duration,
                            notify: false,
                        });
                    }
                }
                false
            }
            Action::RemoveEffect { targets, effect } => {
                for target in targets {
                    if let Some(player) = self.online.get(&target) {
                        let _ = player
                            .outbound
                            .send(ferrite_net::game::packet(&RemoveMobEffect {
                                entity_id: VarInt(player.entity),
                                effect: VarInt(effect),
                            }));
                    }
                    if let Some((_, mailbox, _)) = self.mailboxes.get(&target) {
                        mailbox.lock().unwrap().push_back(Command::RemoveEffect {
                            effect: Some(effect),
                            notify: false,
                        });
                    }
                }
                false
            }
        }
    }
}
fn release_hold(
    worlds: &Worlds,
    holds: &mut HashMap<(Dimension, ChunkPos), usize>,
    signals: &mpsc::UnboundedSender<Signal>,
    dimension: Dimension,
    chunk: ChunkPos,
) {
    let mut flush = false;
    if let Some(count) = holds.get_mut(&(dimension, chunk)) {
        *count -= 1;
        if *count == 0 {
            holds.remove(&(dimension, chunk));
            if dimension == Dimension::Overworld {
                crate::plugins::chunks::set_loaded(chunk.0, chunk.1, false);
            }
            flush = true;
        }
    }
    defer_release(worlds, signals, dimension, chunk, flush);
}
#[cfg(test)]
mod integration;

/// The fixture world is generated locally (`python tools/make_fixture_world.py`) and gitignored,
/// so CI has no copy: every test that reads it skips instead of failing.
#[cfg(test)]
fn fixture_version_root() -> Option<std::path::PathBuf> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3");
    let level = root.join("fixture-world/world/level.dat");
    if !level.exists() {
        eprintln!("SKIP fixture-world test: {} absent", level.display());
        return None;
    }
    Some(root)
}

#[cfg(test)]
mod anvil_tests {
    #[test]
    fn fixture_chunk_encodes_stored_lighting() {
        let Some(root) = super::fixture_version_root() else {
            return;
        };
        let registry = ferrite_world::RuntimeRegistry::load(root.join("generated")).unwrap();
        let path = root.join("fixture-world/world");
        let packet = super::anvil::read_chunk(&path, &registry, 0, 0)
            .unwrap()
            .unwrap();
        assert!(!packet.is_empty());
    }
}

#[test]
fn fixture_spawn_comes_from_level_dat() {
    let Some(root) = fixture_version_root() else {
        return;
    };
    assert_eq!(
        anvil::read_spawn(&root.join("fixture-world/world")).unwrap(),
        [0, 80, 112]
    );
}

#[tokio::test]
async fn modified_chunk_is_written_on_flush() {
    let Some(root) = fixture_version_root() else {
        return;
    };
    let fixture = root.join("fixture-world/world/dimensions/minecraft/overworld/region/r.0.0.mca");
    let temp = std::env::temp_dir().join(format!("ferrite-save-{}", uuid::Uuid::new_v4()));
    let dir = temp.join("dimensions/minecraft/overworld/region");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(fixture, dir.join("r.0.0.mca")).unwrap();
    let registry = ferrite_world::RuntimeRegistry::load(root.join("generated")).unwrap();
    let region = ferrite_world::RegionFile::open(dir.join("r.0.0.mca"), 0, 0).unwrap();
    let data = region.read_chunk(0, 7).unwrap().unwrap();
    let (_, tag) = ferrite_nbt::binary::read_named(&data).unwrap();
    let mut chunk = ferrite_world::chunk_from_nbt(&tag, -64, 384, &registry).unwrap();
    use ferrite_world::BlockStateRegistry;
    let air = registry
        .block_id("minecraft:air", &std::collections::BTreeMap::new())
        .unwrap();
    let stone = registry
        .block_id("minecraft:stone", &std::collections::BTreeMap::new())
        .unwrap();
    let value = if chunk.block(0, 68, 0) == Some(air) {
        stone
    } else {
        air
    };
    chunk.set_block(0, 68, 0, value, &registry).unwrap();
    let source = anvil::AnvilChunkSource::new(temp.clone(), registry);
    let mut light = ferrite_world::LightData {
        sky: vec![ferrite_world::LightSection::Unknown; 26],
        block: vec![ferrite_world::LightSection::Unknown; 26],
    };
    light.block[9] = ferrite_world::LightSection::Data(Box::new([0x11; 2048]));
    source.mark_modified(0, 7, chunk, &light).unwrap();
    let world = World::start_with(Some(source), [0, 68, 112]);
    world.shutdown().await.unwrap();
    let data = region.read_chunk(0, 7).unwrap().unwrap();
    let (_, tag) = ferrite_nbt::binary::read_named(&data).unwrap();
    let ferrite_nbt::Tag::Compound(ref root_tag) = tag else {
        panic!("chunk root")
    };
    let Some(ferrite_nbt::Tag::List(ferrite_nbt::List::Compound(sections))) =
        root_tag.get("sections")
    else {
        panic!("sections")
    };
    let section = sections
        .iter()
        .find(|s| s.get("Y") == Some(&ferrite_nbt::Tag::Byte(4)))
        .unwrap();
    assert_eq!(
        section.get("BlockLight"),
        Some(&ferrite_nbt::Tag::ByteArray(vec![0x11; 2048]))
    );
    let registry = ferrite_world::RuntimeRegistry::load(root.join("generated")).unwrap();
    let chunk = ferrite_world::chunk_from_nbt(&tag, -64, 384, &registry).unwrap();
    assert_eq!(chunk.block(0, 68, 0), Some(value));
    std::fs::remove_dir_all(temp).unwrap();
}
#[test]
fn operator_permissions_require_the_configured_uuid() {
    let settings = WorldSettings {
        gamemode: 1,
        max_players: 20,
        ops: vec![Operator {
            uuid: Uuid::from_u128(10),
            level: 3,
        }],
        plugins: None,
        ..Default::default()
    };
    assert_eq!(settings.permission(Uuid::from_u128(11)), 0);
    assert_eq!(settings.permission(Uuid::from_u128(10)), 3);
}

#[tokio::test]
async fn memory_chunk_block_mutation_is_visible() {
    let source = anvil::AnvilChunkSource::new_memory();
    while source.get(0, 0, 0).is_none() {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let before = source.block(0, 64, 0).unwrap();
    assert_ne!(before, 0);
    source.set_block(0, 64, 0, 0).unwrap();
    assert_eq!(source.block(0, 64, 0), Some(0));
}

#[tokio::test]
async fn gamerules_persist_across_world_restart() {
    let dir = tempfile::tempdir().unwrap();
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3/generated");
    let source = anvil::AnvilChunkSource::new(
        dir.path().to_path_buf(),
        ferrite_world::RuntimeRegistry::load(root).unwrap(),
    );
    let world = World::start_with(Some(source.clone()), [0, 65, 0]);
    world
        .console_command("gamerule random_tick_speed 0".into())
        .await
        .unwrap();
    world
        .console_command("gamerule spawn_mobs false".into())
        .await
        .unwrap();
    world.shutdown().await.unwrap();
    assert!(dir.path().join("data/minecraft/game_rules.dat").exists());
    let reloaded = World::start_with(Some(source), [0, 65, 0]);
    assert_eq!(reloaded.random_tick_speed.load(Ordering::Relaxed), 0);
    assert_eq!(
        reloaded.game_rules.lock().unwrap().get("spawn_mobs"),
        Some(ferrite_command::gamerule::RuleValue::Boolean(false))
    );
    reloaded.shutdown().await.unwrap();
}
