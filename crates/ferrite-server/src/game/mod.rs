mod anvil;
pub use anvil::{AnvilChunkSource, read_spawn as load_spawn};
#[cfg(test)]
mod block_tests;
mod blocks;
#[cfg(test)]
mod persistence_tests;
mod terrain;
pub use terrain::{Generated, StageTimings, TerrainKind, VanillaTerrain};
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
    terrain: Option<Arc<VanillaTerrain>>,
) -> Arc<AnvilChunkSource> {
    AnvilChunkSource::new_with_terrain(path, registry, terrain)
}
pub fn new_memory_source(terrain: Option<Arc<VanillaTerrain>>) -> Arc<AnvilChunkSource> {
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
mod commands_meta;
mod commands_server;
mod commands_world;
mod container;
mod crafting;
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
        let dimensions = settings
            .dimensions
            .clone()
            .unwrap_or_else(|| DimensionRegistry::with_overworld(Arc::clone(&source), spawn));
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
    let mut out = String::new();
    if let Ok(value) = serde_json::from_str(json) {
        collect(&value, &mut out);
    }
    if out.is_empty() { json.to_owned() } else { out }
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
        let tick = move |region: &mut Region| {
            let now = std::time::Instant::now();
            let slot = region_profiler.region_slot(&mut region.profile_slot);
            region
                .timings
                .get_or_insert_with(|| Arc::clone(region_profiler.timings()));
            region.tick(&ctx);
            region.tick_fluids(&fluid_source, &fluid_tracking);
            region.tick_random(
                &fluid_source,
                &fluid_tracking,
                speed.load(Ordering::Relaxed),
            );
            let elapsed = now.elapsed();
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
    clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut load_ran = false;
    loop {
        tokio::select! {
         event=events.recv()=>{let Some(event)=event else {break}; match event {
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
           tracking.lock().unwrap().insert(uuid, (HashSet::new(), outbound.clone()));
           let mut player=Player{uuid,name:name.clone(),permission,plugins:settings.plugins.clone(),plugin_event_errors:settings.plugin_event_errors,session,entity,broken:Vec::new(),ignited:Vec::new(),dimension:start_dimension,sleeping:None,spawn:None,explosions:Vec::new(),button_press:Vec::new(),editing_sign:None,changed_blocks:Vec::new(),primed_tnt:Vec::new(),tracking:tracking.clone(),positions:positions.clone(),source:Some(start_source),playerdata:settings.playerdata.clone(),effects:Default::default(),pos:start_pos,yaw:start_yaw,pitch:start_pitch,ground:true,chunk:start_chunk,mailbox,outbound:outbound.clone(),signals:signal_tx.clone(),biome,gamemode:start_mode,experience_level:0,experience_progress:0.0,experience_total:0,inventory:start_inventory,menus:Default::default(),breaking:None,view:view_distance as i32,pending_teleport:Some(1),next_teleport:2,loaded:HashSet::new(),queue:VecDeque::new(),awaiting_batch:false,rate:4.0,survival:Default::default(),dimension_type:start_dimension.dimension_type_id(),dimensions:dimensions.clone(),portal_ticks:0,portal_cooldown:0,portal_pending:None,seed,flat,respawn_requested:false,attacks:Vec::new(),attack_ticks:0,blocking:false,shield_disabled:0,bow_charge:0,using_bow:false,bow_released:false};player.init_inventory_menu();player.refresh_chunks();
           worlds.add_chunk(start_dimension,player.chunk);worlds.schedule_at_chunk(start_dimension,player.chunk,move |r|{r.hold_chunk(player.chunk);r.players.insert(uuid,player);});
           names.insert(uuid, name.clone());
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
           let mode = start_mode; worlds.schedule_global(move |g|{let mut player = GlobalPlayer::new(uuid,entity,name,properties,outbound,secure_chat,chat_type,chat_visibility); player.pos = start_pos; player.game_mode = mode; g.join(player)});tracing::info!(%uuid,"player joined");
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
          Signal::Hold{dimension,chunk,add}=>if add{let first={let count=holds.entry((dimension,chunk)).or_default();*count += 1;*count == 1};worlds.add_chunk(dimension,chunk);let dir=entity_world.clone();let transient=worlds.transient_entities.clone();worlds.schedule_at_chunk(dimension,chunk,move |r|{r.hold_chunk(chunk);if let Some(transient)=transient {if let Some(entities)=transient.lock().unwrap().remove(&(dimension,chunk)){r.add_entities(entities);}} else if first && let Some(dir)=dir{r.add_entities(entity_save::load(&dimension.storage_root(&dir), (chunk.0, chunk.1)));}});}else{release_hold(&worlds, &mut holds, &signal_tx, dimension, chunk);},
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
          },
          Signal::Leave{dimension,uuid,chunk,held}=>{tracking.lock().unwrap().remove(&uuid);positions.lock().unwrap().remove(&uuid);command_state.lock().unwrap().leave(uuid);permissions.remove(&uuid);online.remove(&uuid);mailboxes.remove(&uuid);for pos in held {release_hold(&worlds, &mut holds, &signal_tx, dimension, pos);}defer_release(&worlds,&signal_tx,dimension,chunk,false);
            let name = names.remove(&uuid).unwrap_or_default();
            if let Some(plugins) = &settings.plugins && let Err(error) = plugins.player_left(uuid, &name) { tracing::warn!(%error, "player quit event failed"); }
            worlds.schedule_global(move |g|g.leave(uuid));if let Some(join)=pending.remove(&uuid){let _=event_tx.send(join);}}
         }},
         command = async { match plugin_commands.as_mut() { Some(commands) => commands.recv().await, None => std::future::pending().await } }, if plugin_commands.is_some() => {
             match command {
                 Some(command) => route_plugin_command(command, &mailboxes, &mut permissions),
                 // The plugin host dropped its sender (no plugins loaded): stop polling.
                 None => plugin_commands = None,
             }
         },
         _tick = clock.tick() => {
             let advance_time = game_rules.lock().unwrap().get_bool("advance_time").unwrap_or(true);
             let due = command_state.lock().unwrap().tick(advance_time);
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
        // Health, food, titles, display names, sounds and kicks have no host path yet; report
        // them instead of pretending the plugin's call succeeded.
        other => tracing::debug!(?other, "plugin player command not routed"),
    }
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
                    if let Some((_, outbound)) = tracking.lock().unwrap().get(&uuid) {
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
