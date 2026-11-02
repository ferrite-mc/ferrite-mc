//! Hosts plugins in an embedded JVM.
use ferrite_jvm::jni::{
    JNIEnv,
    objects::{JClass, JObject, JString, JValue},
};
use ferrite_jvm::{Jvm, JvmConfig, NativeMethod};
pub use scheduling::{EntityTask, RegionTask};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static LOGS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
static REGION_QUEUE: OnceLock<Mutex<scheduling::Queue>> = OnceLock::new();
static READY: OnceLock<Mutex<Vec<RegionTask>>> = OnceLock::new();
static READY_ENTITIES: OnceLock<Mutex<Vec<EntityTask>>> = OnceLock::new();
static MODES: OnceLock<HashMap<String, bool>> = OnceLock::new();
static REGION_CONTEXT: OnceLock<Option<std::sync::Arc<dyn RegionContext>>> = OnceLock::new();
static SERVER_HANDLER: OnceLock<Option<ServerHandler>> = OnceLock::new();
static WORLD_HANDLER: OnceLock<Option<WorldHandler>> = OnceLock::new();
static WORLD_VIEW: OnceLock<Option<std::sync::Arc<dyn WorldView>>> = OnceLock::new();
static PLAYER_HANDLER: OnceLock<Option<PlayerHandler>> = OnceLock::new();
static COMMAND_HANDLER: OnceLock<Option<CommandHandler>> = OnceLock::new();
thread_local! { static REGION: std::cell::RefCell<Option<(String, i32, i32)>> = const { std::cell::RefCell::new(None) }; }
thread_local! { static CAPTURED_MESSAGES: std::cell::RefCell<Option<Vec<PluginMessage>>> = const { std::cell::RefCell::new(None) }; }
const BOOTSTRAP: &str = "dev/ferrite/server/Bootstrap";
/// Bukkit event classes the server forwards; keep in sync with the Java bridge.
pub const PLAYER_JOIN_EVENT: &str = "org.bukkit.event.player.PlayerJoinEvent";
pub const PLAYER_QUIT_EVENT: &str = "org.bukkit.event.player.PlayerQuitEvent";
pub const ASYNC_CHAT_EVENT: &str = "io.papermc.paper.event.player.AsyncChatEvent";
pub const ASYNC_PLAYER_CHAT_EVENT: &str = "org.bukkit.event.player.AsyncPlayerChatEvent";
pub const PLAYER_MOVE_EVENT: &str = "org.bukkit.event.player.PlayerMoveEvent";
pub const PLAYER_INTERACT_EVENT: &str = "org.bukkit.event.player.PlayerInteractEvent";
pub const BLOCK_BREAK_EVENT: &str = "org.bukkit.event.block.BlockBreakEvent";
pub const BLOCK_PLACE_EVENT: &str = "org.bukkit.event.block.BlockPlaceEvent";
pub const PLAYER_COMMAND_PREPROCESS_EVENT: &str =
    "org.bukkit.event.player.PlayerCommandPreprocessEvent";
pub const INVENTORY_CLICK_EVENT: &str = "org.bukkit.event.inventory.InventoryClickEvent";
pub const INVENTORY_CLOSE_EVENT: &str = "org.bukkit.event.inventory.InventoryCloseEvent";
pub const PLAYER_DEATH_EVENT: &str = "org.bukkit.event.entity.PlayerDeathEvent";
pub const PLAYER_RESPAWN_EVENT: &str = "org.bukkit.event.player.PlayerRespawnEvent";
pub const ENTITY_DAMAGE_EVENT: &str = "org.bukkit.event.entity.EntityDamageEvent";
pub const ENTITY_DEATH_EVENT: &str = "org.bukkit.event.entity.EntityDeathEvent";
pub const CHUNK_LOAD_EVENT: &str = "org.bukkit.event.world.ChunkLoadEvent";
pub const CHUNK_UNLOAD_EVENT: &str = "org.bukkit.event.world.ChunkUnloadEvent";
pub const WEATHER_CHANGE_EVENT: &str = "org.bukkit.event.weather.WeatherChangeEvent";
pub const WORLD_LOAD_EVENT: &str = "org.bukkit.event.world.WorldLoadEvent";
pub const PLAYER_LEVEL_CHANGE_EVENT: &str = "org.bukkit.event.player.PlayerLevelChangeEvent";
pub const PLAYER_EXP_CHANGE_EVENT: &str = "org.bukkit.event.player.PlayerExpChangeEvent";
/// Event classes with at least one registered listener, reported by the JVM.
static LISTENED_EVENTS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

#[derive(Default)]
pub struct PluginConfig {
    pub bridge_jar: PathBuf,
    /// The extracted vanilla data pack, so the bridge can read recipe and tag tables.
    pub data_dir: PathBuf,
    pub java_home: Option<PathBuf>,
    pub command_handler: Option<CommandHandler>,
    pub region_context: Option<std::sync::Arc<dyn RegionContext>>,
    pub server_snapshot: ServerSnapshot,
    /// Published before plugins enable, so `onEnable` can query worlds.
    pub world: Option<WorldSnapshot>,
    pub player_handler: Option<PlayerHandler>,
    pub world_handler: Option<WorldHandler>,
    /// Read-only block and biome access for Bukkit's block queries.
    pub world_view: Option<std::sync::Arc<dyn WorldView>>,
    pub server_handler: Option<ServerHandler>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BlockPosition {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}
/// Which mouse button touched a block; Bukkit's `PlayerInteractEvent.Action`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum InteractAction {
    LeftClickBlock = 0,
    RightClickBlock = 1,
}
/// Where a player was and where a movement packet takes it; Bukkit's `PlayerMoveEvent`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerMove {
    pub from: [f64; 3],
    pub to: [f64; 3],
    pub from_yaw: f32,
    pub from_pitch: f32,
    pub to_yaw: f32,
    pub to_pitch: f32,
}
#[derive(Clone, Debug)]
pub struct BlockSnapshot {
    pub position: BlockPosition,
    pub material: String,
}
/// Only explicitly supplied blocks are readable; absent positions are unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColumnHeight {
    pub x: i32,
    pub z: i32,
    pub y: i32,
}
#[derive(Clone, Debug)]
pub struct WorldSnapshot {
    pub id: uuid::Uuid,
    pub name: String,
    pub time: i64,
    pub spawn: BlockPosition,
    pub blocks: Vec<BlockSnapshot>,
    pub heights: Vec<ColumnHeight>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorldCommand {
    SetBlock {
        world: uuid::Uuid,
        position: BlockPosition,
        material: String,
    },
}
pub type WorldHandler = std::sync::Arc<dyn Fn(WorldCommand) + Send + Sync>;
/// Read-only world access for `World#getBlockAt`, `Block#getLightFromSky`, `World#getBiome`
/// and `World#getHighestBlockYAt`.
///
/// Queries may arrive from any plugin thread, so implementations must be thread-safe and
/// return `None` rather than inventing data for an unloaded chunk or an unknown world.
pub trait WorldView: Send + Sync {
    /// The namespaced material key at a position, for example `minecraft:grass_block`.
    fn block(&self, world: uuid::Uuid, x: i32, y: i32, z: i32) -> Option<String>;
    /// Stored sky light at a position (0..=15).
    fn sky_light(&self, world: uuid::Uuid, x: i32, y: i32, z: i32) -> Option<u8>;
    /// The highest non-air block in a column.
    fn highest_block(&self, world: uuid::Uuid, x: i32, z: i32) -> Option<i32>;
    /// The biome key at a position, for example `minecraft:plains`.
    fn biome(&self, world: uuid::Uuid, x: i32, y: i32, z: i32) -> Option<String>;
}
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerLocation {
    pub world: uuid::Uuid,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemSnapshot {
    pub material: String,
    pub amount: u8,
}
#[derive(Clone, Debug)]
pub struct PlayerSnapshot {
    pub id: uuid::Uuid,
    pub name: String,
    pub operator: bool,
    pub health: f64,
    pub food: u8,
    pub mode: GameMode,
    pub location: PlayerLocation,
    pub inventory: Vec<Option<ItemSnapshot>>,
}
/// A player the server knows but that is not connected.
#[derive(Clone, Debug)]
pub struct OfflinePlayerSnapshot {
    pub id: uuid::Uuid,
    pub name: Option<String>,
    pub operator: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageContent {
    Plain(String),
    AdventureJson(String),
}
#[derive(Clone, Debug, PartialEq)]
pub enum PlayerCommand {
    Message {
        player: uuid::Uuid,
        content: MessageContent,
    },
    Teleport {
        player: uuid::Uuid,
        location: PlayerLocation,
    },
    SetInventorySlot {
        player: uuid::Uuid,
        slot: u8,
        item: Option<ItemSnapshot>,
    },
    SetOperator {
        player: uuid::Uuid,
        operator: bool,
    },
    DisplayName {
        player: uuid::Uuid,
        component_json: Option<String>,
    },
    Kick {
        player: uuid::Uuid,
        component_json: Option<String>,
    },
    Title {
        player: uuid::Uuid,
        title: Option<String>,
        subtitle: Option<String>,
        fade_in: i32,
        stay: i32,
        fade_out: i32,
    },
    PlaySound {
        player: uuid::Uuid,
        world: uuid::Uuid,
        sound: String,
        category: SoundCategory,
        x: f64,
        y: f64,
        z: f64,
        volume: f32,
        pitch: f32,
        seed: Option<i64>,
    },
    SetHealth {
        player: uuid::Uuid,
        health: f64,
    },
    SetFood {
        player: uuid::Uuid,
        food: u8,
    },
    SetGameMode {
        player: uuid::Uuid,
        mode: GameMode,
    },
    SetExperience {
        player: uuid::Uuid,
        level: Option<i32>,
        progress: Option<f32>,
        total: Option<i32>,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundCategory {
    Master,
    Music,
    Records,
    Weather,
    Blocks,
    Hostile,
    Neutral,
    Players,
    Ambient,
    Voice,
    Ui,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameMode {
    Survival,
    Creative,
    Adventure,
    Spectator,
}
pub type PlayerHandler = std::sync::Arc<dyn Fn(PlayerCommand) + Send + Sync>;
/// Stable identity for the main world published to plugins.
///
/// The Rust game addresses chunks by coordinate, so the Java side needs one id that both
/// `register_world` and every player location agree on.
pub fn default_world_id() -> uuid::Uuid {
    uuid::Uuid::from_u128(1)
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerCommand {
    BanIp {
        address: std::net::IpAddr,
        reason: Option<String>,
        expires_unix_millis: Option<i64>,
        source: Option<String>,
    },
    PardonIp {
        address: std::net::IpAddr,
    },
    SetWhitelist {
        enabled: bool,
    },
}
pub type ServerHandler = std::sync::Arc<dyn Fn(ServerCommand) + Send + Sync>;
/// Server settings made visible to plugins during startup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerSnapshot {
    pub motd: String,
    pub max_players: u16,
    pub port: u16,
    pub view_distance: u8,
    pub whitelist: bool,
    pub online_mode: bool,
    pub banned_ips: Vec<std::net::IpAddr>,
    /// Host part of the bind address, so `Server#getIp` matches the listener.
    pub bind_address: String,
}
impl Default for ServerSnapshot {
    fn default() -> Self {
        Self {
            motd: "Ferrite".into(),
            max_players: 20,
            port: 25565,
            view_distance: 10,
            whitelist: false,
            online_mode: true,
            banned_ips: Vec::new(),
            bind_address: String::new(),
        }
    }
}
/// Server-owned routing and thread ownership information for region-aware plugins.
pub trait RegionContext: Send + Sync {
    fn is_owned_by_current_region(&self, world: uuid::Uuid, chunk_x: i32, chunk_z: i32) -> bool;
    fn schedule_on_region(&self, task: RegionTask);
    fn schedule_on_entity(&self, task: EntityTask);
    fn retire_entity(&self, entity: uuid::Uuid);
}
pub type CommandHandler = std::sync::Arc<dyn Fn(CommandSender, &str) -> bool + Send + Sync>;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Jvm(#[from] ferrite_jvm::JvmError),
    #[error("plugin metadata rejected: {0}")]
    Metadata(String),
    #[error("task was not issued or has already been consumed")]
    InvalidTask,
    #[error("region context is not configured")]
    MissingRegionContext,
    #[error("invalid player snapshot")]
    InvalidPlayerSnapshot,
    #[error("invalid offline player snapshot")]
    InvalidOfflinePlayerSnapshot,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandSender {
    Console,
    Player(uuid::Uuid),
}
/// A command declared by a plugin's `plugin.yml` or `paper-plugin.yml`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginCommand {
    pub name: String,
    pub aliases: Vec<String>,
    pub permission: Option<String>,
    pub usage: Option<String>,
}
/// A message a plugin sent to one player during a callback.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginMessage {
    pub player: uuid::Uuid,
    pub content: MessageContent,
}
/// Result of dispatching a line to a plugin `CommandExecutor`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PluginCommandResult {
    pub handled: bool,
    /// Messages captured from `CommandSender.sendMessage` during the call.
    pub messages: Vec<PluginMessage>,
}
/// Server hooks the game loop forwards to plugins.
pub trait ServerHooks: Send + Sync {
    /// Fires Bukkit's `PlayerJoinEvent` for a player that just joined.
    fn player_joined(&self, id: uuid::Uuid, name: &str) -> Result<(), Error>;
    /// Fires Bukkit's `PlayerQuitEvent` before the player is removed.
    fn player_left(&self, id: uuid::Uuid, name: &str) -> Result<(), Error>;
    /// Fires Paper's `AsyncChatEvent` and Bukkit's `AsyncPlayerChatEvent`.
    ///
    /// Returns the message after listeners ran, or `None` when one cancelled chat.
    fn chat(&self, id: uuid::Uuid, name: &str, message: &str) -> Result<Option<String>, Error>;
    /// Fires Bukkit's `PlayerMoveEvent`; `false` means a listener cancelled the move.
    fn player_moved(
        &self,
        id: uuid::Uuid,
        name: &str,
        movement: &PlayerMove,
    ) -> Result<bool, Error>;
    /// Fires Bukkit's `PlayerInteractEvent` for a block click; `false` means cancelled.
    fn player_interacted(
        &self,
        id: uuid::Uuid,
        name: &str,
        action: InteractAction,
        block: BlockPosition,
        face: u8,
        material: &str,
    ) -> Result<bool, Error>;
    /// Fires Bukkit's `BlockBreakEvent` before a block is removed; `false` means cancelled.
    fn block_broken(
        &self,
        id: uuid::Uuid,
        name: &str,
        block: BlockPosition,
        material: &str,
    ) -> Result<bool, Error>;
    /// Fires Bukkit's `BlockPlaceEvent` before a block is placed; `false` means cancelled.
    fn block_placed(
        &self,
        id: uuid::Uuid,
        name: &str,
        block: BlockPosition,
        face: u8,
        placed: &str,
        replaced: &str,
    ) -> Result<bool, Error>;
    /// Fires Bukkit's `PlayerCommandPreprocessEvent`; `false` means cancelled.
    fn player_command(&self, id: uuid::Uuid, name: &str, message: &str) -> Result<bool, Error>;
    /// Fires Bukkit's `ChunkLoadEvent` after the chunk's packet was queued.
    fn chunk_loaded(&self, _world: uuid::Uuid, _x: i32, _z: i32, _new: bool) -> Result<(), Error> {
        Ok(())
    }
    /// Fires Bukkit's `ChunkUnloadEvent` before the client forgets the chunk.
    fn chunk_unloaded(&self, _world: uuid::Uuid, _x: i32, _z: i32) -> Result<(), Error> {
        Ok(())
    }
    /// Fires Bukkit's `PlayerDeathEvent` once per death.
    fn player_died(
        &self,
        _id: uuid::Uuid,
        _name: &str,
        _cause: &str,
        _dropped_exp: i32,
        _message: &str,
    ) -> Result<(), Error> {
        Ok(())
    }
    /// Fires Bukkit's `PlayerRespawnEvent`; returns the spawn a listener selected.
    fn player_respawned(
        &self,
        _id: uuid::Uuid,
        _name: &str,
        spawn: [f64; 3],
        _yaw: f32,
        _pitch: f32,
    ) -> Result<[f64; 3], Error> {
        Ok(spawn)
    }
    /// Fires Bukkit's `EntityDamageEvent`; returns the damage after listeners.
    fn entity_damaged(
        &self,
        _id: uuid::Uuid,
        _name: &str,
        _cause: &str,
        amount: f64,
    ) -> Result<f64, Error> {
        Ok(amount)
    }
    /// Fires Bukkit's `EntityDeathEvent` for a non-player entity.
    fn entity_died(
        &self,
        _id: uuid::Uuid,
        _kind: &str,
        _position: [f64; 3],
        _dropped_exp: i32,
    ) -> Result<(), Error> {
        Ok(())
    }
    /// Fires Bukkit's `WeatherChangeEvent`; `false` cancels the change.
    fn weather_changing(&self, _world: uuid::Uuid, _to_weather_state: bool) -> Result<bool, Error> {
        Ok(true)
    }
    /// Fires Bukkit's `WorldLoadEvent` for a newly published world.
    fn world_loaded(&self, _world: uuid::Uuid) -> Result<(), Error> {
        Ok(())
    }
    /// Fires Bukkit's `PlayerLevelChangeEvent`; returns the level to use.
    fn player_level_changed(
        &self,
        _id: uuid::Uuid,
        _name: &str,
        _old_level: i32,
        new_level: i32,
    ) -> Result<i32, Error> {
        Ok(new_level)
    }
    /// Fires Bukkit's `PlayerExpChangeEvent`; returns the amount to grant.
    fn player_exp_changed(&self, _id: uuid::Uuid, _name: &str, amount: i32) -> Result<i32, Error> {
        Ok(amount)
    }
    /// Fires Bukkit's `InventoryClickEvent`; `false` cancels the click.
    fn inventory_clicked(
        &self,
        _id: uuid::Uuid,
        _name: &str,
        _slot: i32,
        _click: &str,
        _action: &str,
    ) -> Result<bool, Error> {
        Ok(true)
    }
    /// Fires Bukkit's `InventoryCloseEvent` when a menu closes.
    fn inventory_closed(&self, _id: uuid::Uuid, _name: &str) -> Result<(), Error> {
        Ok(())
    }
    /// Commands declared by enabled plugins.
    fn plugin_commands(&self) -> Result<Vec<PluginCommand>, Error>;
    /// Runs plugin scheduler tasks and flushes plugin logs for one server tick.
    fn tick(&self) -> Result<(), Error> {
        Ok(())
    }
    /// Publishes a world's identity, spawn and known blocks before plugins query it.
    fn register_world(&self, _world: &WorldSnapshot) -> Result<(), Error> {
        Ok(())
    }
    /// Publishes an online player's location and state for plugin lookups.
    fn register_player(&self, _player: &PlayerSnapshot) -> Result<(), Error> {
        Ok(())
    }
    /// Runs a plugin command for `sender`, returning captured `sendMessage` output.
    fn run_plugin_command(
        &self,
        sender: CommandSender,
        line: &str,
    ) -> Result<PluginCommandResult, Error>;
    /// Region-scheduler tasks that became due this tick; the server routes them to the chunk.
    fn take_region_tasks(&self) -> Vec<RegionTask> {
        Vec::new()
    }
    /// Runs one region-scheduler task on the thread that owns its chunk.
    fn run_region_task(&self, _task: RegionTask) -> Result<(), Error> {
        Ok(())
    }
    /// Moves the host-side player snapshot with the live player.
    fn update_player_location(
        &self,
        _id: uuid::Uuid,
        _location: &PlayerLocation,
    ) -> Result<(), Error> {
        Ok(())
    }
}
pub struct PluginHost {
    jvm: Jvm,
    issued_regions: Mutex<Vec<RegionTask>>,
    issued_entities: Mutex<Vec<EntityTask>>,
    region_context: Option<std::sync::Arc<dyn RegionContext>>,
}
extern "system" fn log(mut env: JNIEnv, _: JClass, message: JString) {
    if let Ok(s) = env.get_string(&message) {
        LOGS.get_or_init(Default::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(s.into());
    }
}
fn listened_events() -> &'static Mutex<HashSet<String>> {
    LISTENED_EVENTS.get_or_init(Default::default)
}
/// Records that a plugin registered a listener for one Bukkit event class.
extern "system" fn event_registered(mut env: JNIEnv, _: JClass, event: JString) {
    if let Ok(event) = env.get_string(&event) {
        let event: String = event.into();
        listened_events()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(event);
    }
}
/// Forgets every listener after the JVM disabled all plugins.
extern "system" fn events_cleared(_: JNIEnv, _: JClass) {
    listened_events()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
}
extern "system" fn server_whitelist(_: JNIEnv, _: JClass, enabled: u8) -> u8 {
    let Some(handler) = SERVER_HANDLER.get().and_then(Option::as_ref) else {
        return 0;
    };
    u8::from(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handler(ServerCommand::SetWhitelist {
                enabled: enabled != 0,
            })
        }))
        .is_ok(),
    )
}
extern "system" fn server_ban(
    mut env: JNIEnv,
    _: JClass,
    address: JString,
    reason: JString,
    expires: i64,
    source: JString,
    ban: u8,
) -> u8 {
    let Ok(address) = env.get_string(&address) else {
        return 0;
    };
    let address: String = address.into();
    let Ok(address) = address.parse() else {
        return 0;
    };
    let reason = if reason.is_null() {
        None
    } else {
        match env.get_string(&reason) {
            Ok(value) => Some(value.into()),
            Err(_) => return 0,
        }
    };
    let source = if source.is_null() {
        None
    } else {
        match env.get_string(&source) {
            Ok(value) => Some(value.into()),
            Err(_) => return 0,
        }
    };
    let command = if ban == 0 {
        ServerCommand::PardonIp { address }
    } else {
        ServerCommand::BanIp {
            address,
            reason,
            expires_unix_millis: (expires >= 0).then_some(expires),
            source,
        }
    };
    let Some(handler) = SERVER_HANDLER.get().and_then(Option::as_ref) else {
        return 0;
    };
    u8::from(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(command))).is_ok())
}
extern "system" fn world_set_block(
    mut env: JNIEnv,
    _: JClass,
    world: JString,
    x: i32,
    y: i32,
    z: i32,
    material: JString,
) -> u8 {
    let Ok(world) = env.get_string(&world) else {
        return 0;
    };
    let world: String = world.into();
    let Ok(world) = uuid::Uuid::parse_str(&world) else {
        return 0;
    };
    let Ok(material) = env.get_string(&material) else {
        return 0;
    };
    let material: String = material.into();
    let Some(handler) = WORLD_HANDLER.get().and_then(Option::as_ref) else {
        return 0;
    };
    u8::from(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handler(WorldCommand::SetBlock {
                world,
                position: BlockPosition { x, y, z },
                material,
            })
        }))
        .is_ok(),
    )
}
fn world_uuid(env: &mut JNIEnv, world: &JString) -> Option<uuid::Uuid> {
    let value: String = env.get_string(world).ok()?.into();
    uuid::Uuid::parse_str(&value).ok()
}
fn view_string<'local>(env: &mut JNIEnv<'local>, value: Option<String>) -> JString<'local> {
    match value.and_then(|value| env.new_string(value).ok()) {
        Some(value) => value,
        None => JString::from(JObject::null()),
    }
}
extern "system" fn block_type<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    world: JString<'local>,
    x: i32,
    y: i32,
    z: i32,
) -> JString<'local> {
    let value = world_uuid(&mut env, &world).and_then(|world| {
        WORLD_VIEW
            .get()
            .and_then(Option::as_ref)
            .and_then(|view| view.block(world, x, y, z))
    });
    view_string(&mut env, value)
}
extern "system" fn sky_light(
    mut env: JNIEnv,
    _: JClass,
    world: JString,
    x: i32,
    y: i32,
    z: i32,
) -> i32 {
    world_uuid(&mut env, &world)
        .and_then(|world| {
            WORLD_VIEW
                .get()
                .and_then(Option::as_ref)
                .and_then(|view| view.sky_light(world, x, y, z))
        })
        .map_or(-1, i32::from)
}
extern "system" fn highest_block_y(
    mut env: JNIEnv,
    _: JClass,
    world: JString,
    x: i32,
    z: i32,
) -> i32 {
    world_uuid(&mut env, &world)
        .and_then(|world| {
            WORLD_VIEW
                .get()
                .and_then(Option::as_ref)
                .and_then(|view| view.highest_block(world, x, z))
        })
        .unwrap_or(i32::MIN)
}
extern "system" fn biome_at<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    world: JString<'local>,
    x: i32,
    y: i32,
    z: i32,
) -> JString<'local> {
    let value = world_uuid(&mut env, &world).and_then(|world| {
        WORLD_VIEW
            .get()
            .and_then(Option::as_ref)
            .and_then(|view| view.biome(world, x, y, z))
    });
    view_string(&mut env, value)
}
extern "system" fn player_message(
    mut env: JNIEnv,
    _: JClass,
    player: JString,
    message: JString,
    component: u8,
) {
    let Ok(player) = env.get_string(&player) else {
        return;
    };
    let player: String = player.into();
    let Ok(player) = uuid::Uuid::parse_str(&player) else {
        return;
    };
    let Ok(message) = env.get_string(&message) else {
        return;
    };
    let message: String = message.into();
    let content = if component == 0 {
        MessageContent::Plain(message)
    } else {
        MessageContent::AdventureJson(message)
    };
    let captured = CAPTURED_MESSAGES.with(|captured| {
        let mut captured = captured.borrow_mut();
        let Some(messages) = captured.as_mut() else {
            return false;
        };
        messages.push(PluginMessage {
            player,
            content: content.clone(),
        });
        true
    });
    if captured {
        return;
    }
    if let Some(handler) = PLAYER_HANDLER.get().and_then(Option::as_ref) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handler(PlayerCommand::Message { player, content })
        }));
    }
}
extern "system" fn player_sound(
    mut env: JNIEnv,
    _: JClass,
    player: JString,
    world: JString,
    sound: JString,
    category: i32,
    x: f64,
    y: f64,
    z: f64,
    volume: f32,
    pitch: f32,
    has_seed: u8,
    seed: i64,
) -> u8 {
    if !x.is_finite()
        || !y.is_finite()
        || !z.is_finite()
        || !volume.is_finite()
        || !pitch.is_finite()
    {
        return 0;
    }
    let Ok(player) = env.get_string(&player) else {
        return 0;
    };
    let player: String = player.into();
    let Ok(player) = uuid::Uuid::parse_str(&player) else {
        return 0;
    };
    let Ok(world) = env.get_string(&world) else {
        return 0;
    };
    let world: String = world.into();
    let Ok(world) = uuid::Uuid::parse_str(&world) else {
        return 0;
    };
    let Ok(sound) = env.get_string(&sound) else {
        return 0;
    };
    let sound: String = sound.into();
    let category = match category {
        0 => SoundCategory::Master,
        1 => SoundCategory::Music,
        2 => SoundCategory::Records,
        3 => SoundCategory::Weather,
        4 => SoundCategory::Blocks,
        5 => SoundCategory::Hostile,
        6 => SoundCategory::Neutral,
        7 => SoundCategory::Players,
        8 => SoundCategory::Ambient,
        9 => SoundCategory::Voice,
        10 => SoundCategory::Ui,
        _ => return 0,
    };
    let Some(handler) = PLAYER_HANDLER.get().and_then(Option::as_ref) else {
        return 0;
    };
    u8::from(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handler(PlayerCommand::PlaySound {
                player,
                world,
                sound,
                category,
                x,
                y,
                z,
                volume,
                pitch,
                seed: (has_seed != 0).then_some(seed),
            })
        }))
        .is_ok(),
    )
}
extern "system" fn player_visual(
    mut env: JNIEnv,
    _: JClass,
    player: JString,
    kind: i32,
    first: JString,
    second: JString,
    fade_in: i32,
    stay: i32,
    fade_out: i32,
) -> u8 {
    let Ok(player) = env.get_string(&player) else {
        return 0;
    };
    let player: String = player.into();
    let Ok(player) = uuid::Uuid::parse_str(&player) else {
        return 0;
    };
    let first = if first.is_null() {
        None
    } else {
        match env.get_string(&first) {
            Ok(value) => Some(value.into()),
            Err(_) => return 0,
        }
    };
    let second = if second.is_null() {
        None
    } else {
        match env.get_string(&second) {
            Ok(value) => Some(value.into()),
            Err(_) => return 0,
        }
    };
    let command = match kind {
        0 => PlayerCommand::DisplayName {
            player,
            component_json: first,
        },
        1 => PlayerCommand::Kick {
            player,
            component_json: first,
        },
        2 => PlayerCommand::Title {
            player,
            title: first,
            subtitle: second,
            fade_in,
            stay,
            fade_out,
        },
        _ => return 0,
    };
    let Some(handler) = PLAYER_HANDLER.get().and_then(Option::as_ref) else {
        return 0;
    };
    u8::from(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(command))).is_ok())
}
extern "system" fn player_operator(
    mut env: JNIEnv,
    _: JClass,
    player: JString,
    operator: u8,
) -> u8 {
    let Ok(player) = env.get_string(&player) else {
        return 0;
    };
    let player: String = player.into();
    let Ok(player) = uuid::Uuid::parse_str(&player) else {
        return 0;
    };
    let Some(handler) = PLAYER_HANDLER.get().and_then(Option::as_ref) else {
        return 0;
    };
    u8::from(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handler(PlayerCommand::SetOperator {
                player,
                operator: operator != 0,
            })
        }))
        .is_ok(),
    )
}
extern "system" fn player_inventory(
    mut env: JNIEnv,
    _: JClass,
    player: JString,
    slot: i32,
    material: JString,
    amount: i32,
) -> u8 {
    if !(0..41).contains(&slot) || !(0..=255).contains(&amount) {
        return 0;
    }
    let Ok(player) = env.get_string(&player) else {
        return 0;
    };
    let player: String = player.into();
    let Ok(player) = uuid::Uuid::parse_str(&player) else {
        return 0;
    };
    let Ok(material) = env.get_string(&material) else {
        return 0;
    };
    let material: String = material.into();
    let item = if amount == 0 {
        None
    } else {
        Some(ItemSnapshot {
            material,
            amount: amount as u8,
        })
    };
    let Some(handler) = PLAYER_HANDLER.get().and_then(Option::as_ref) else {
        return 0;
    };
    u8::from(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handler(PlayerCommand::SetInventorySlot {
                player,
                slot: slot as u8,
                item,
            })
        }))
        .is_ok(),
    )
}
extern "system" fn player_teleport(
    mut env: JNIEnv,
    _: JClass,
    player: JString,
    world: JString,
    x: f64,
    y: f64,
    z: f64,
    yaw: f32,
    pitch: f32,
) -> u8 {
    if !x.is_finite() || !y.is_finite() || !z.is_finite() || !yaw.is_finite() || !pitch.is_finite()
    {
        return 0;
    }
    let Ok(player) = env.get_string(&player) else {
        return 0;
    };
    let player: String = player.into();
    let Ok(player) = uuid::Uuid::parse_str(&player) else {
        return 0;
    };
    let Ok(world) = env.get_string(&world) else {
        return 0;
    };
    let world: String = world.into();
    let Ok(world) = uuid::Uuid::parse_str(&world) else {
        return 0;
    };
    let Some(handler) = PLAYER_HANDLER.get().and_then(Option::as_ref) else {
        return 0;
    };
    u8::from(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handler(PlayerCommand::Teleport {
                player,
                location: PlayerLocation {
                    world,
                    x,
                    y,
                    z,
                    yaw,
                    pitch,
                },
            })
        }))
        .is_ok(),
    )
}
extern "system" fn player_state(
    mut env: JNIEnv,
    _: JClass,
    player: JString,
    kind: i32,
    value: f64,
) {
    let Ok(player) = env.get_string(&player) else {
        return;
    };
    let player: String = player.into();
    let Ok(player) = uuid::Uuid::parse_str(&player) else {
        return;
    };
    let command = match kind {
        0 if value.is_finite() && value >= 0.0 => PlayerCommand::SetHealth {
            player,
            health: value,
        },
        1 if value.fract() == 0.0 && (0.0..=20.0).contains(&value) => PlayerCommand::SetFood {
            player,
            food: value as u8,
        },
        2 => {
            let mode = match value as i32 {
                0 => GameMode::Survival,
                1 => GameMode::Creative,
                2 => GameMode::Adventure,
                3 => GameMode::Spectator,
                _ => return,
            };
            PlayerCommand::SetGameMode { player, mode }
        }
        3 if value.is_finite()
            && value >= 0.0
            && value <= f64::from(i32::MAX)
            && value.fract() == 0.0 =>
        {
            PlayerCommand::SetExperience {
                player,
                level: Some(value as i32),
                progress: None,
                total: None,
            }
        }
        4 if value.is_finite() && (0.0..=1.0).contains(&value) => PlayerCommand::SetExperience {
            player,
            level: None,
            progress: Some(value as f32),
            total: None,
        },
        5 if value.is_finite()
            && value >= 0.0
            && value <= f64::from(i32::MAX)
            && value.fract() == 0.0 =>
        {
            PlayerCommand::SetExperience {
                player,
                level: None,
                progress: None,
                total: Some(value as i32),
            }
        }
        _ => return,
    };
    if let Some(handler) = PLAYER_HANDLER.get().and_then(Option::as_ref) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(command)));
    }
}
extern "system" fn dispatch_back(mut env: JNIEnv, _: JClass, sender: JString, line: JString) -> u8 {
    let Ok(sender) = env.get_string(&sender) else {
        return 0;
    };
    let sender: String = sender.into();
    let sender = if sender == "console" {
        CommandSender::Console
    } else if let Ok(id) = uuid::Uuid::parse_str(&sender) {
        CommandSender::Player(id)
    } else {
        return 0;
    };
    let Ok(line) = env.get_string(&line) else {
        return 0;
    };
    let line: String = line.into();
    COMMAND_HANDLER
        .get()
        .and_then(Option::as_ref)
        .map_or(0, |handler| {
            u8::from(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(sender, &line)))
                    .unwrap_or(false),
            )
        })
}
extern "system" fn player_move(
    mut env: JNIEnv,
    _: JClass,
    player: JString,
    name: JString,
    from_x: f64,
    from_y: f64,
    from_z: f64,
    from_yaw: f32,
    from_pitch: f32,
    to_x: f64,
    to_y: f64,
    to_z: f64,
    to_yaw: f32,
    to_pitch: f32,
) -> u8 {
    if [from_x, from_y, from_z, to_x, to_y, to_z]
        .iter()
        .any(|v| !v.is_finite())
        || [from_yaw, from_pitch, to_yaw, to_pitch]
            .iter()
            .any(|v| !v.is_finite())
    {
        return 1;
    }
    let Ok(player) = env.get_string(&player) else {
        return 1;
    };
    let player: String = player.into();
    let Ok(player) = uuid::Uuid::parse_str(&player) else {
        return 1;
    };
    let Ok(name) = env.get_string(&name) else {
        return 1;
    };
    let name: String = name.into();
    let allowed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<bool> {
        let player = env.new_string(player.to_string()).ok()?;
        let name = env.new_string(name).ok()?;
        env.call_static_method(
            BOOTSTRAP,
            "firePlayerMove",
            "(Ljava/lang/String;Ljava/lang/String;DDDFFDDDFF)Z",
            &[
                JValue::Object(player.as_ref()),
                JValue::Object(name.as_ref()),
                JValue::Double(from_x),
                JValue::Double(from_y),
                JValue::Double(from_z),
                JValue::Float(from_yaw),
                JValue::Float(from_pitch),
                JValue::Double(to_x),
                JValue::Double(to_y),
                JValue::Double(to_z),
                JValue::Float(to_yaw),
                JValue::Float(to_pitch),
            ],
        )
        .ok()?
        .z()
        .ok()
    }));
    u8::from(matches!(allowed, Ok(Some(true))))
}
extern "system" fn player_interact(
    mut env: JNIEnv,
    _: JClass,
    player: JString,
    name: JString,
    action: i32,
    x: i32,
    y: i32,
    z: i32,
    face: i32,
    material: JString,
) -> u8 {
    let Ok(player) = env.get_string(&player) else {
        return 1;
    };
    let player: String = player.into();
    let Ok(player) = uuid::Uuid::parse_str(&player) else {
        return 1;
    };
    let Ok(name) = env.get_string(&name) else {
        return 1;
    };
    let name: String = name.into();
    let Ok(material) = env.get_string(&material) else {
        return 1;
    };
    let material: String = material.into();
    let allowed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<bool> {
        let player = env.new_string(player.to_string()).ok()?;
        let name = env.new_string(name).ok()?;
        let material = env.new_string(material).ok()?;
        env.call_static_method(
            BOOTSTRAP,
            "firePlayerInteract",
            "(Ljava/lang/String;Ljava/lang/String;IIIIILjava/lang/String;)Z",
            &[
                JValue::Object(player.as_ref()),
                JValue::Object(name.as_ref()),
                JValue::Int(action),
                JValue::Int(x),
                JValue::Int(y),
                JValue::Int(z),
                JValue::Int(face),
                JValue::Object(material.as_ref()),
            ],
        )
        .ok()?
        .z()
        .ok()
    }));
    u8::from(matches!(allowed, Ok(Some(true))))
}
extern "system" fn block_break(
    mut env: JNIEnv,
    _: JClass,
    player: JString,
    name: JString,
    x: i32,
    y: i32,
    z: i32,
    material: JString,
) -> u8 {
    let Ok(player) = env.get_string(&player) else {
        return 1;
    };
    let player: String = player.into();
    let Ok(player) = uuid::Uuid::parse_str(&player) else {
        return 1;
    };
    let Ok(name) = env.get_string(&name) else {
        return 1;
    };
    let name: String = name.into();
    let Ok(material) = env.get_string(&material) else {
        return 1;
    };
    let material: String = material.into();
    let allowed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<bool> {
        let player = env.new_string(player.to_string()).ok()?;
        let name = env.new_string(name).ok()?;
        let material = env.new_string(material).ok()?;
        env.call_static_method(
            BOOTSTRAP,
            "fireBlockBreak",
            "(Ljava/lang/String;Ljava/lang/String;IIILjava/lang/String;)Z",
            &[
                JValue::Object(player.as_ref()),
                JValue::Object(name.as_ref()),
                JValue::Int(x),
                JValue::Int(y),
                JValue::Int(z),
                JValue::Object(material.as_ref()),
            ],
        )
        .ok()?
        .z()
        .ok()
    }));
    u8::from(matches!(allowed, Ok(Some(true))))
}
extern "system" fn block_place(
    mut env: JNIEnv,
    _: JClass,
    player: JString,
    name: JString,
    x: i32,
    y: i32,
    z: i32,
    face: i32,
    placed: JString,
    replaced: JString,
) -> u8 {
    let Ok(player) = env.get_string(&player) else {
        return 1;
    };
    let player: String = player.into();
    let Ok(player) = uuid::Uuid::parse_str(&player) else {
        return 1;
    };
    let Ok(name) = env.get_string(&name) else {
        return 1;
    };
    let name: String = name.into();
    let Ok(placed) = env.get_string(&placed) else {
        return 1;
    };
    let placed: String = placed.into();
    let Ok(replaced) = env.get_string(&replaced) else {
        return 1;
    };
    let replaced: String = replaced.into();
    let allowed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<bool> {
        let player = env.new_string(player.to_string()).ok()?;
        let name = env.new_string(name).ok()?;
        let placed = env.new_string(placed).ok()?;
        let replaced = env.new_string(replaced).ok()?;
        env.call_static_method(
            BOOTSTRAP,
            "fireBlockPlace",
            "(Ljava/lang/String;Ljava/lang/String;IIIILjava/lang/String;Ljava/lang/String;)Z",
            &[
                JValue::Object(player.as_ref()),
                JValue::Object(name.as_ref()),
                JValue::Int(x),
                JValue::Int(y),
                JValue::Int(z),
                JValue::Int(face),
                JValue::Object(placed.as_ref()),
                JValue::Object(replaced.as_ref()),
            ],
        )
        .ok()?
        .z()
        .ok()
    }));
    u8::from(matches!(allowed, Ok(Some(true))))
}
extern "system" fn player_command(
    mut env: JNIEnv,
    _: JClass,
    player: JString,
    name: JString,
    message: JString,
) -> u8 {
    let Ok(player) = env.get_string(&player) else {
        return 1;
    };
    let player: String = player.into();
    let Ok(player) = uuid::Uuid::parse_str(&player) else {
        return 1;
    };
    let Ok(name) = env.get_string(&name) else {
        return 1;
    };
    let name: String = name.into();
    let Ok(message) = env.get_string(&message) else {
        return 1;
    };
    let message: String = message.into();
    let allowed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<bool> {
        let player = env.new_string(player.to_string()).ok()?;
        let name = env.new_string(name).ok()?;
        let message = env.new_string(message).ok()?;
        env.call_static_method(
            BOOTSTRAP,
            "firePlayerCommand",
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Z",
            &[
                JValue::Object(player.as_ref()),
                JValue::Object(name.as_ref()),
                JValue::Object(message.as_ref()),
            ],
        )
        .ok()?
        .z()
        .ok()
    }));
    u8::from(matches!(allowed, Ok(Some(true))))
}
extern "system" fn schedule_global(_: JNIEnv, _: JClass, id: i64, delay: i64) {
    REGION_QUEUE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .global(id, delay.max(0) as u64);
}
extern "system" fn current_tick(_: JNIEnv, _: JClass) -> i64 {
    REGION_QUEUE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .current_tick()
        .min(i64::MAX as u64) as i64
}
extern "system" fn schedule_region(
    mut env: JNIEnv,
    _: JClass,
    id: i64,
    world: JString,
    x: i32,
    z: i32,
    delay: i64,
    plugin: JString,
) {
    let Ok(world) = env.get_string(&world) else {
        return;
    };
    let world: String = world.into();
    let Ok(plugin) = env.get_string(&plugin) else {
        return;
    };
    let plugin: String = plugin.into();
    let aware = MODES
        .get()
        .and_then(|m| m.get(&plugin.to_ascii_lowercase()))
        .copied()
        .unwrap_or(false);
    REGION_QUEUE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .region(id, world, x, z, delay.max(0) as u64, aware);
}
extern "system" fn schedule_entity(
    mut env: JNIEnv,
    _: JClass,
    id: i64,
    entity: JString,
    delay: i64,
    plugin: JString,
) {
    let Ok(entity) = env.get_string(&entity) else {
        return;
    };
    let entity: String = entity.into();
    let Ok(plugin) = env.get_string(&plugin) else {
        return;
    };
    let plugin: String = plugin.into();
    let aware = MODES
        .get()
        .and_then(|m| m.get(&plugin.to_ascii_lowercase()))
        .copied()
        .unwrap_or(false);
    REGION_QUEUE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entity(id, entity, delay.max(0) as u64, aware);
}
extern "system" fn is_region_thread(
    mut env: JNIEnv,
    _: JClass,
    world: JString,
    x: i32,
    z: i32,
) -> u8 {
    let Ok(world) = env.get_string(&world) else {
        return 0;
    };
    let world: String = world.into();
    if let Some(context) = REGION_CONTEXT.get().and_then(Option::as_ref) {
        return uuid::Uuid::parse_str(&world).ok().map_or(0, |id| {
            u8::from(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    context.is_owned_by_current_region(id, x, z)
                }))
                .unwrap_or(false),
            )
        });
    }
    REGION.with(|region| {
        u8::from(
            region
                .borrow()
                .as_ref()
                .is_some_and(|current| current == &(world, x, z)),
        )
    })
}
impl PluginHost {
    pub fn start(dir: &Path, cfg: PluginConfig) -> Result<Self, Error> {
        let plan = ferrite_plugin_meta::scan_directory(dir)?;
        if !plan.failed.is_empty() {
            return Err(Error::Metadata(format!("{:?}", plan.failed)));
        }
        let modes = plan
            .loaded()
            .map(|p| {
                (
                    p.name().to_ascii_lowercase(),
                    matches!(
                        p.threading_mode(),
                        ferrite_plugin_meta::ThreadingMode::RegionAware
                    ),
                )
            })
            .collect();
        let jvm = Jvm::start(JvmConfig {
            java_home: cfg.java_home,
            classpath: vec![cfg.bridge_jar],
            ..Default::default()
        })?;
        let _ = MODES.set(modes);
        let _ = COMMAND_HANDLER.set(cfg.command_handler);
        let _ = PLAYER_HANDLER.set(cfg.player_handler);
        let _ = WORLD_HANDLER.set(cfg.world_handler);
        let _ = WORLD_VIEW.set(cfg.world_view);
        let _ = SERVER_HANDLER.set(cfg.server_handler);
        let _ = REGION_CONTEXT.set(cfg.region_context.clone());
        jvm.register_natives(
            "dev/ferrite/server/Native$Bindings",
            &[
                NativeMethod::new("log", "(Ljava/lang/String;)V", log as *mut _),
                NativeMethod::new(
                    "eventRegistered",
                    "(Ljava/lang/String;)V",
                    event_registered as *mut _,
                ),
                NativeMethod::new("eventsCleared", "()V", events_cleared as *mut _),
                NativeMethod::new(
                    "serverBan",
                    "(Ljava/lang/String;Ljava/lang/String;JLjava/lang/String;Z)Z",
                    server_ban as *mut _,
                ),
                NativeMethod::new("serverWhitelist", "(Z)Z", server_whitelist as *mut _),
                NativeMethod::new(
                    "worldSetBlock",
                    "(Ljava/lang/String;IIILjava/lang/String;)Z",
                    world_set_block as *mut _,
                ),
                NativeMethod::new(
                    "blockType",
                    "(Ljava/lang/String;III)Ljava/lang/String;",
                    block_type as *mut _,
                ),
                NativeMethod::new(
                    "skyLight",
                    "(Ljava/lang/String;III)I",
                    sky_light as *mut _,
                ),
                NativeMethod::new(
                    "highestBlockY",
                    "(Ljava/lang/String;II)I",
                    highest_block_y as *mut _,
                ),
                NativeMethod::new(
                    "biomeAt",
                    "(Ljava/lang/String;III)Ljava/lang/String;",
                    biome_at as *mut _,
                ),
                NativeMethod::new(
                    "playerSound",
                    "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;IDDDFFZJ)Z",
                    player_sound as *mut _,
                ),
                NativeMethod::new(
                    "playerVisual",
                    "(Ljava/lang/String;ILjava/lang/String;Ljava/lang/String;III)Z",
                    player_visual as *mut _,
                ),
                NativeMethod::new(
                    "playerOperator",
                    "(Ljava/lang/String;Z)Z",
                    player_operator as *mut _,
                ),
                NativeMethod::new(
                    "playerInventory",
                    "(Ljava/lang/String;ILjava/lang/String;I)Z",
                    player_inventory as *mut _,
                ),
                NativeMethod::new(
                    "playerTeleport",
                    "(Ljava/lang/String;Ljava/lang/String;DDDFF)Z",
                    player_teleport as *mut _,
                ),
                NativeMethod::new(
                    "playerState",
                    "(Ljava/lang/String;ID)V",
                    player_state as *mut _,
                ),
                NativeMethod::new(
                    "playerMessage",
                    "(Ljava/lang/String;Ljava/lang/String;Z)V",
                    player_message as *mut _,
                ),
                NativeMethod::new(
                    "dispatchCommand",
                    "(Ljava/lang/String;Ljava/lang/String;)Z",
                    dispatch_back as *mut _,
                ),
                NativeMethod::new(
                    "playerMove",
                    "(Ljava/lang/String;Ljava/lang/String;DDDFFDDDFF)Z",
                    player_move as *mut _,
                ),
                NativeMethod::new(
                    "playerInteract",
                    "(Ljava/lang/String;Ljava/lang/String;IIIIILjava/lang/String;)Z",
                    player_interact as *mut _,
                ),
                NativeMethod::new(
                    "blockBreak",
                    "(Ljava/lang/String;Ljava/lang/String;IIILjava/lang/String;)Z",
                    block_break as *mut _,
                ),
                NativeMethod::new(
                    "blockPlace",
                    "(Ljava/lang/String;Ljava/lang/String;IIIILjava/lang/String;Ljava/lang/String;)Z",
                    block_place as *mut _,
                ),
                NativeMethod::new(
                    "playerCommand",
                    "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Z",
                    player_command as *mut _,
                ),
                NativeMethod::new("scheduleGlobal", "(JJ)V", schedule_global as *mut _),
                NativeMethod::new("currentTick", "()J", current_tick as *mut _),
                NativeMethod::new(
                    "scheduleRegion",
                    "(JLjava/lang/String;IIJLjava/lang/String;)V",
                    schedule_region as *mut _,
                ),
                NativeMethod::new(
                    "scheduleEntity",
                    "(JLjava/lang/String;JLjava/lang/String;)V",
                    schedule_entity as *mut _,
                ),
                NativeMethod::new(
                    "isRegionThread",
                    "(Ljava/lang/String;II)Z",
                    is_region_thread as *mut _,
                ),
            ],
        )?;
        jvm.with_env(|env| {
            let motd = env.new_string(&cfg.server_snapshot.motd)?;
            env.call_static_method(
                BOOTSTRAP,
                "configure",
                "(Ljava/lang/String;IIIZZ)V",
                &[
                    JValue::Object(motd.as_ref()),
                    JValue::Int(i32::from(cfg.server_snapshot.max_players)),
                    JValue::Int(i32::from(cfg.server_snapshot.port)),
                    JValue::Int(i32::from(cfg.server_snapshot.view_distance)),
                    JValue::Bool(u8::from(cfg.server_snapshot.whitelist)),
                    JValue::Bool(u8::from(cfg.server_snapshot.online_mode)),
                ],
            )?;
            for address in &cfg.server_snapshot.banned_ips {
                let address = env.new_string(address.to_string())?;
                env.call_static_method(
                    BOOTSTRAP,
                    "configureIpBan",
                    "(Ljava/lang/String;)V",
                    &[JValue::Object(address.as_ref())],
                )?;
            }
            if !cfg.server_snapshot.bind_address.is_empty() {
                let address = env.new_string(&cfg.server_snapshot.bind_address)?;
                env.call_static_method(
                    BOOTSTRAP,
                    "configureAddress",
                    "(Ljava/lang/String;)V",
                    &[JValue::Object(address.as_ref())],
                )?;
            }
            if !cfg.data_dir.as_os_str().is_empty() {
                let dir = env.new_string(cfg.data_dir.to_string_lossy())?;
                env.call_static_method(
                    BOOTSTRAP,
                    "configureDataDir",
                    "(Ljava/lang/String;)V",
                    &[JValue::Object(dir.as_ref())],
                )?;
            }
            Ok(())
        })?;
        // Create the Bukkit server first, then publish the main world, and only then let
        // `onEnable` run: plugins query the world list during enable.
        jvm.call_static::<()>(BOOTSTRAP, "startServer", "()V", &[])?;
        let host = Self {
            jvm,
            issued_regions: Mutex::new(Vec::new()),
            issued_entities: Mutex::new(Vec::new()),
            region_context: cfg.region_context,
        };
        if let Some(world) = &cfg.world {
            host.register_world(world)?;
        }
        host.jvm.with_env(|env| {
            let paths: Vec<_> = plan.loaded().collect();
            let array =
                env.new_object_array(paths.len() as i32, "java/lang/String", JObject::null())?;
            for (i, entry) in paths.iter().enumerate() {
                let path = env.new_string(entry.source.to_string_lossy())?;
                env.set_object_array_element(&array, i as i32, path)?;
            }
            let modes = env.new_boolean_array(paths.len() as i32)?;
            let flags: Vec<u8> = paths
                .iter()
                .map(|p| {
                    u8::from(matches!(
                        p.threading_mode(),
                        ferrite_plugin_meta::ThreadingMode::RegionAware
                    ))
                })
                .collect();
            env.set_boolean_array_region(&modes, 0, &flags)?;
            env.call_static_method(
                BOOTSTRAP,
                "enablePlugins",
                "([Ljava/lang/String;[Z)V",
                &[
                    JValue::Object(array.as_ref()),
                    JValue::Object(modes.as_ref()),
                ],
            )?;
            Ok(())
        })?;
        Ok(host)
    }
    /// Publish known world blocks; unprovided positions remain unavailable.
    pub fn register_world(&self, world: &WorldSnapshot) -> Result<(), Error> {
        self.jvm.with_env(|env| {
            let id = env.new_string(world.id.to_string())?;
            let name = env.new_string(&world.name)?;
            env.call_static_method(
                BOOTSTRAP,
                "registerWorld",
                "(Ljava/lang/String;Ljava/lang/String;JIII)V",
                &[
                    JValue::Object(id.as_ref()),
                    JValue::Object(name.as_ref()),
                    JValue::Long(world.time),
                    JValue::Int(world.spawn.x),
                    JValue::Int(world.spawn.y),
                    JValue::Int(world.spawn.z),
                ],
            )?;
            for height in &world.heights {
                env.call_static_method(
                    BOOTSTRAP,
                    "setHeightSnapshot",
                    "(Ljava/lang/String;III)V",
                    &[
                        JValue::Object(id.as_ref()),
                        JValue::Int(height.x),
                        JValue::Int(height.z),
                        JValue::Int(height.y),
                    ],
                )?;
            }
            for block in &world.blocks {
                let material = env.new_string(&block.material)?;
                env.call_static_method(
                    BOOTSTRAP,
                    "setBlockSnapshot",
                    "(Ljava/lang/String;IIILjava/lang/String;)V",
                    &[
                        JValue::Object(id.as_ref()),
                        JValue::Int(block.position.x),
                        JValue::Int(block.position.y),
                        JValue::Int(block.position.z),
                        JValue::Object(material.as_ref()),
                    ],
                )?;
            }
            Ok(())
        })?;
        Ok(())
    }
    /// Publish an online player snapshot for plugin lookups and command dispatch.
    pub fn register_player(&self, player: &PlayerSnapshot) -> Result<(), Error> {
        if !player.health.is_finite()
            || player.health < 0.0
            || player.food > 20
            || player.inventory.len() > 41
            || player
                .inventory
                .iter()
                .flatten()
                .any(|item| item.amount == 0)
            || !player.location.x.is_finite()
            || !player.location.y.is_finite()
            || !player.location.z.is_finite()
            || !player.location.yaw.is_finite()
            || !player.location.pitch.is_finite()
        {
            return Err(Error::InvalidPlayerSnapshot);
        }
        let mode = match player.mode {
            GameMode::Survival => 0,
            GameMode::Creative => 1,
            GameMode::Adventure => 2,
            GameMode::Spectator => 3,
        };
        self.jvm.with_env(|env| {
            let id = env.new_string(player.id.to_string())?;
            let name = env.new_string(&player.name)?;
            let world = env.new_string(player.location.world.to_string())?;
            env.call_static_method(
                BOOTSTRAP,
                "registerPlayer",
                "(Ljava/lang/String;Ljava/lang/String;ZDIILjava/lang/String;DDDFF)V",
                &[
                    JValue::Object(id.as_ref()),
                    JValue::Object(name.as_ref()),
                    JValue::Bool(u8::from(player.operator)),
                    JValue::Double(player.health),
                    JValue::Int(i32::from(player.food)),
                    JValue::Int(mode),
                    JValue::Object(world.as_ref()),
                    JValue::Double(player.location.x),
                    JValue::Double(player.location.y),
                    JValue::Double(player.location.z),
                    JValue::Float(player.location.yaw),
                    JValue::Float(player.location.pitch),
                ],
            )?;
            for (slot, item) in player.inventory.iter().enumerate() {
                if let Some(item) = item {
                    let material = env.new_string(&item.material)?;
                    env.call_static_method(
                        BOOTSTRAP,
                        "setInventorySnapshot",
                        "(Ljava/lang/String;ILjava/lang/String;I)V",
                        &[
                            JValue::Object(id.as_ref()),
                            JValue::Int(slot as i32),
                            JValue::Object(material.as_ref()),
                            JValue::Int(i32::from(item.amount)),
                        ],
                    )?;
                }
            }
            Ok(())
        })?;
        Ok(())
    }
    /// Move the host-side player snapshot with the live player, so `getLocation` stays current.
    pub fn update_player_location(
        &self,
        id: uuid::Uuid,
        location: &PlayerLocation,
    ) -> Result<(), Error> {
        if !location.x.is_finite()
            || !location.y.is_finite()
            || !location.z.is_finite()
            || !location.yaw.is_finite()
            || !location.pitch.is_finite()
        {
            return Err(Error::InvalidPlayerSnapshot);
        }
        self.jvm.with_env(|env| {
            let id = env.new_string(id.to_string())?;
            let world = env.new_string(location.world.to_string())?;
            env.call_static_method(
                BOOTSTRAP,
                "updatePlayerLocation",
                "(Ljava/lang/String;Ljava/lang/String;DDDFF)V",
                &[
                    JValue::Object(id.as_ref()),
                    JValue::Object(world.as_ref()),
                    JValue::Double(location.x),
                    JValue::Double(location.y),
                    JValue::Double(location.z),
                    JValue::Float(location.yaw),
                    JValue::Float(location.pitch),
                ],
            )?;
            Ok(())
        })?;
        Ok(())
    }
    /// Publish a known offline player for name and operator lookups.
    pub fn register_offline_player(&self, player: &OfflinePlayerSnapshot) -> Result<(), Error> {
        if player.name.as_deref().is_some_and(str::is_empty) {
            return Err(Error::InvalidOfflinePlayerSnapshot);
        }
        self.jvm.with_env(|env| {
            let id = env.new_string(player.id.to_string())?;
            let name = match &player.name {
                Some(name) => env.new_string(name)?,
                None => JString::from(JObject::null()),
            };
            env.call_static_method(
                BOOTSTRAP,
                "registerOfflinePlayer",
                "(Ljava/lang/String;Ljava/lang/String;Z)V",
                &[
                    JValue::Object(id.as_ref()),
                    JValue::Object(name.as_ref()),
                    JValue::Bool(u8::from(player.operator)),
                ],
            )?;
            Ok(())
        })?;
        Ok(())
    }
    /// Return whether a plugin completed enabling and remains enabled.
    pub fn is_plugin_enabled(&self, name: &str) -> Result<bool, Error> {
        Ok(self.jvm.with_env(|env| {
            let name = env.new_string(name)?;
            env.call_static_method(
                BOOTSTRAP,
                "isPluginEnabled",
                "(Ljava/lang/String;)Z",
                &[JValue::Object(name.as_ref())],
            )?
            .z()
        })?)
    }
    pub fn logs(&self) -> Vec<String> {
        LOGS.get_or_init(Default::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
    /// Take accumulated plugin log messages for delivery by the server.
    pub fn drain_logs(&self) -> Vec<String> {
        std::mem::take(
            &mut *LOGS
                .get_or_init(Default::default)
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }
    pub fn dispatch_command(&self, sender: CommandSender, line: &str) -> Result<bool, Error> {
        let sender = match sender {
            CommandSender::Console => "console".to_owned(),
            CommandSender::Player(id) => id.to_string(),
        };
        Ok(self.jvm.with_env(|env| {
            let sender = env.new_string(sender)?;
            let line = env.new_string(line)?;
            env.call_static_method(
                BOOTSTRAP,
                "dispatchCommand",
                "(Ljava/lang/String;Ljava/lang/String;)Z",
                &[
                    JValue::Object(sender.as_ref()),
                    JValue::Object(line.as_ref()),
                ],
            )?
            .z()
        })?)
    }
    pub fn tick(&self) -> Result<(), Error> {
        self.jvm.call_static::<()>(BOOTSTRAP, "tick", "()V", &[])?;
        let (ready, globals, entities) = REGION_QUEUE
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .advance();
        READY
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend(ready);
        READY_ENTITIES
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend(entities);
        for id in globals {
            self.jvm
                .call_static::<()>(BOOTSTRAP, "runGlobalTask", "(J)V", &[JValue::Long(id)])?;
        }
        Ok(())
    }
    pub fn take_region_tasks(&self) -> Vec<RegionTask> {
        let tasks = std::mem::take(
            &mut *READY
                .get_or_init(Default::default)
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        self.issued_regions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend(tasks.iter().cloned());
        tasks
    }
    pub fn take_entity_tasks(&self) -> Vec<EntityTask> {
        let tasks = std::mem::take(
            &mut *READY_ENTITIES
                .get_or_init(Default::default)
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        self.issued_entities
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend(tasks.iter().cloned());
        tasks
    }
    /// Transfer ready work to the server's region and entity schedulers.
    pub fn route_ready_tasks(&self) -> Result<(), Error> {
        let context = self
            .region_context
            .as_ref()
            .ok_or(Error::MissingRegionContext)?;
        for task in self.take_region_tasks() {
            context.schedule_on_region(task);
        }
        for task in self.take_entity_tasks() {
            context.schedule_on_entity(task);
        }
        Ok(())
    }
    pub fn run_entity_task(&self, task: EntityTask) -> Result<(), Error> {
        consume_task(&self.issued_entities, &task)?;
        Ok(self
            .jvm
            .call_static(BOOTSTRAP, "runEntityTask", "(J)V", &[JValue::Long(task.id)])?)
    }
    pub fn run_region_task(&self, task: RegionTask) -> Result<(), Error> {
        consume_task(&self.issued_regions, &task)?;
        let previous = REGION
            .with(|region| region.replace(Some((task.world.clone(), task.chunk_x, task.chunk_z))));
        let result =
            self.jvm
                .call_static(BOOTSTRAP, "runRegionTask", "(J)V", &[JValue::Long(task.id)]);
        REGION.with(|region| *region.borrow_mut() = previous);
        Ok(result?)
    }
    /// Commands declared by enabled plugins in their descriptor.
    pub fn declared_commands(&self) -> Result<Vec<PluginCommand>, Error> {
        let encoded: String = self.jvm.with_env(|env| {
            let value = env
                .call_static_method(BOOTSTRAP, "pluginCommands", "()Ljava/lang/String;", &[])?
                .l()?;
            if value.is_null() {
                return Ok(String::new());
            }
            let value = JString::from(value);
            let value: String = env.get_string(&value)?.into();
            Ok(value)
        })?;
        Ok(encoded
            .lines()
            .filter_map(|line| {
                let mut fields = line.split('\t');
                let name = fields.next().filter(|name| !name.is_empty())?;
                let aliases = fields
                    .next()
                    .unwrap_or_default()
                    .split(',')
                    .filter(|alias| !alias.is_empty())
                    .map(str::to_owned)
                    .collect();
                let permission = fields
                    .next()
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned);
                let usage = fields
                    .next()
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned);
                Some(PluginCommand {
                    name: name.to_owned(),
                    aliases,
                    permission,
                    usage,
                })
            })
            .collect())
    }

    /// Executes a plugin command through its Bukkit `CommandExecutor`.
    ///
    /// Messages the plugin sends with `CommandSender.sendMessage` on this thread are captured
    /// and returned so the game loop can deliver them to the player.
    pub fn execute_plugin_command(
        &self,
        sender: CommandSender,
        line: &str,
    ) -> Result<PluginCommandResult, Error> {
        let line = line.trim().trim_start_matches('/');
        let mut parts = line.split_whitespace();
        let Some(label) = parts.next() else {
            return Ok(PluginCommandResult::default());
        };
        let label = label.to_owned();
        let args: Vec<String> = parts.map(str::to_owned).collect();
        let sender = match sender {
            CommandSender::Console => "console".to_owned(),
            CommandSender::Player(id) => id.to_string(),
        };
        let previous = CAPTURED_MESSAGES.with(|captured| captured.replace(Some(Vec::new())));
        let result = self.jvm.with_env(|env| {
            let sender = env.new_string(sender)?;
            let label = env.new_string(&label)?;
            let array =
                env.new_object_array(args.len() as i32, "java/lang/String", JObject::null())?;
            for (index, arg) in args.iter().enumerate() {
                let arg = env.new_string(arg)?;
                env.set_object_array_element(&array, index as i32, arg)?;
            }
            let status = env.call_static_method(
                BOOTSTRAP,
                "runPluginCommand",
                "(Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;)I",
                &[
                    JValue::Object(sender.as_ref()),
                    JValue::Object(label.as_ref()),
                    JValue::Object(array.as_ref()),
                ],
            )?;
            status.i()
        });
        let messages = CAPTURED_MESSAGES
            .with(|captured| captured.replace(previous))
            .unwrap_or_default();
        Ok(PluginCommandResult {
            handled: result? == 1,
            messages,
        })
    }

    /// Whether at least one plugin listener is registered for `event_class`.
    pub fn listens_for(&self, event_class: &str) -> bool {
        listened_events()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(event_class)
    }

    /// Fires Bukkit's `PlayerJoinEvent`; no JNI call without a registered listener.
    pub fn fire_player_join(&self, id: uuid::Uuid, name: &str) -> Result<(), Error> {
        if !self.listens_for(PLAYER_JOIN_EVENT) {
            return Ok(());
        }
        self.jvm.with_env(|env| {
            let id = env.new_string(id.to_string())?;
            let name = env.new_string(name)?;
            env.call_static_method(
                BOOTSTRAP,
                "firePlayerJoin",
                "(Ljava/lang/String;Ljava/lang/String;)V",
                &[JValue::Object(id.as_ref()), JValue::Object(name.as_ref())],
            )?;
            Ok(())
        })?;
        Ok(())
    }

    /// Fires Bukkit's `PlayerQuitEvent`; no JNI call without a registered listener.
    pub fn fire_player_quit(&self, id: uuid::Uuid) -> Result<(), Error> {
        if !self.listens_for(PLAYER_QUIT_EVENT) {
            return Ok(());
        }
        self.jvm.with_env(|env| {
            let id = env.new_string(id.to_string())?;
            env.call_static_method(
                BOOTSTRAP,
                "firePlayerQuit",
                "(Ljava/lang/String;)V",
                &[JValue::Object(id.as_ref())],
            )?;
            Ok(())
        })?;
        Ok(())
    }

    /// Fires Bukkit's `PlayerMoveEvent`; no JNI call without a registered listener.
    pub fn fire_player_move(
        &self,
        id: uuid::Uuid,
        name: &str,
        movement: &PlayerMove,
    ) -> Result<bool, Error> {
        if !self.listens_for(PLAYER_MOVE_EVENT) {
            return Ok(true);
        }
        Ok(self.jvm.with_env(|env| {
            let id = env.new_string(id.to_string())?;
            let name = env.new_string(name)?;
            let allowed = env
                .call_static_method(
                    BOOTSTRAP,
                    "firePlayerMove",
                    "(Ljava/lang/String;Ljava/lang/String;DDDFFDDDFF)Z",
                    &[
                        JValue::Object(id.as_ref()),
                        JValue::Object(name.as_ref()),
                        JValue::Double(movement.from[0]),
                        JValue::Double(movement.from[1]),
                        JValue::Double(movement.from[2]),
                        JValue::Float(movement.from_yaw),
                        JValue::Float(movement.from_pitch),
                        JValue::Double(movement.to[0]),
                        JValue::Double(movement.to[1]),
                        JValue::Double(movement.to[2]),
                        JValue::Float(movement.to_yaw),
                        JValue::Float(movement.to_pitch),
                    ],
                )?
                .z()?;
            Ok(allowed)
        })?)
    }

    /// Fires Bukkit's `PlayerInteractEvent`; no JNI call without a registered listener.
    pub fn fire_player_interact(
        &self,
        id: uuid::Uuid,
        name: &str,
        action: InteractAction,
        block: BlockPosition,
        face: u8,
        material: &str,
    ) -> Result<bool, Error> {
        if !self.listens_for(PLAYER_INTERACT_EVENT) {
            return Ok(true);
        }
        Ok(self.jvm.with_env(|env| {
            let id = env.new_string(id.to_string())?;
            let name = env.new_string(name)?;
            let material = env.new_string(material)?;
            let allowed = env
                .call_static_method(
                    BOOTSTRAP,
                    "firePlayerInteract",
                    "(Ljava/lang/String;Ljava/lang/String;IIIIILjava/lang/String;)Z",
                    &[
                        JValue::Object(id.as_ref()),
                        JValue::Object(name.as_ref()),
                        JValue::Int(action as i32),
                        JValue::Int(block.x),
                        JValue::Int(block.y),
                        JValue::Int(block.z),
                        JValue::Int(i32::from(face)),
                        JValue::Object(material.as_ref()),
                    ],
                )?
                .z()?;
            Ok(allowed)
        })?)
    }

    /// Fires Bukkit's `BlockBreakEvent`; no JNI call without a registered listener.
    pub fn fire_block_break(
        &self,
        id: uuid::Uuid,
        name: &str,
        block: BlockPosition,
        material: &str,
    ) -> Result<bool, Error> {
        if !self.listens_for(BLOCK_BREAK_EVENT) {
            return Ok(true);
        }
        Ok(self.jvm.with_env(|env| {
            let id = env.new_string(id.to_string())?;
            let name = env.new_string(name)?;
            let material = env.new_string(material)?;
            let allowed = env
                .call_static_method(
                    BOOTSTRAP,
                    "fireBlockBreak",
                    "(Ljava/lang/String;Ljava/lang/String;IIILjava/lang/String;)Z",
                    &[
                        JValue::Object(id.as_ref()),
                        JValue::Object(name.as_ref()),
                        JValue::Int(block.x),
                        JValue::Int(block.y),
                        JValue::Int(block.z),
                        JValue::Object(material.as_ref()),
                    ],
                )?
                .z()?;
            Ok(allowed)
        })?)
    }

    /// Fires Bukkit's `BlockPlaceEvent`; no JNI call without a registered listener.
    pub fn fire_block_place(
        &self,
        id: uuid::Uuid,
        name: &str,
        block: BlockPosition,
        face: u8,
        placed: &str,
        replaced: &str,
    ) -> Result<bool, Error> {
        if !self.listens_for(BLOCK_PLACE_EVENT) {
            return Ok(true);
        }
        Ok(self.jvm.with_env(|env| {
            let id = env.new_string(id.to_string())?;
            let name = env.new_string(name)?;
            let placed = env.new_string(placed)?;
            let replaced = env.new_string(replaced)?;
            let allowed = env
                .call_static_method(
                    BOOTSTRAP,
                    "fireBlockPlace",
                    "(Ljava/lang/String;Ljava/lang/String;IIIILjava/lang/String;Ljava/lang/String;)Z",
                    &[
                        JValue::Object(id.as_ref()),
                        JValue::Object(name.as_ref()),
                        JValue::Int(block.x),
                        JValue::Int(block.y),
                        JValue::Int(block.z),
                        JValue::Int(i32::from(face)),
                        JValue::Object(placed.as_ref()),
                        JValue::Object(replaced.as_ref()),
                    ],
                )?
                .z()?;
            Ok(allowed)
        })?)
    }

    /// Fires Bukkit's `PlayerCommandPreprocessEvent`; no JNI call without a registered listener.
    pub fn fire_player_command(
        &self,
        id: uuid::Uuid,
        name: &str,
        message: &str,
    ) -> Result<bool, Error> {
        if !self.listens_for(PLAYER_COMMAND_PREPROCESS_EVENT) {
            return Ok(true);
        }
        Ok(self.jvm.with_env(|env| {
            let id = env.new_string(id.to_string())?;
            let name = env.new_string(name)?;
            let message = env.new_string(message)?;
            let allowed = env
                .call_static_method(
                    BOOTSTRAP,
                    "firePlayerCommand",
                    "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Z",
                    &[
                        JValue::Object(id.as_ref()),
                        JValue::Object(name.as_ref()),
                        JValue::Object(message.as_ref()),
                    ],
                )?
                .z()?;
            Ok(allowed)
        })?)
    }

    /// Fires Paper's `AsyncChatEvent` and Bukkit's `AsyncPlayerChatEvent`.
    ///
    /// Returns the message after listeners ran, or `None` when a listener cancelled chat.
    pub fn fire_chat(
        &self,
        id: uuid::Uuid,
        name: &str,
        message: &str,
    ) -> Result<Option<String>, Error> {
        if !self.listens_for(ASYNC_CHAT_EVENT) && !self.listens_for(ASYNC_PLAYER_CHAT_EVENT) {
            return Ok(Some(message.to_owned()));
        }
        Ok(self.jvm.with_env(|env| {
            let id = env.new_string(id.to_string())?;
            let name = env.new_string(name)?;
            let message = env.new_string(message)?;
            let result = env
                .call_static_method(
                    BOOTSTRAP,
                    "fireChat",
                    "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
                    &[
                        JValue::Object(id.as_ref()),
                        JValue::Object(name.as_ref()),
                        JValue::Object(message.as_ref()),
                    ],
                )?
                .l()?;
            if result.is_null() {
                return Ok(None);
            }
            let result = JString::from(result);
            let result: String = env.get_string(&result)?.into();
            Ok(Some(result))
        })?)
    }
    fn fire_string_id(&self, method: &str, signature: &str, id: uuid::Uuid) -> Result<(), Error> {
        self.jvm.with_env(|env| {
            let id = env.new_string(id.to_string())?;
            env.call_static_method(BOOTSTRAP, method, signature, &[JValue::Object(id.as_ref())])?;
            Ok(())
        })?;
        Ok(())
    }

    /// Fires Bukkit's `ChunkLoadEvent`; no JNI call without a registered listener.
    pub fn fire_chunk_load(
        &self,
        world: uuid::Uuid,
        x: i32,
        z: i32,
        new: bool,
    ) -> Result<(), Error> {
        if !self.listens_for(CHUNK_LOAD_EVENT) {
            return Ok(());
        }
        self.jvm.with_env(|env| {
            let world = env.new_string(world.to_string())?;
            env.call_static_method(
                BOOTSTRAP,
                "fireChunkLoad",
                "(Ljava/lang/String;IIZ)V",
                &[
                    JValue::Object(world.as_ref()),
                    JValue::Int(x),
                    JValue::Int(z),
                    JValue::Bool(u8::from(new)),
                ],
            )?;
            Ok(())
        })?;
        Ok(())
    }

    /// Fires Bukkit's `ChunkUnloadEvent`; no JNI call without a registered listener.
    pub fn fire_chunk_unload(&self, world: uuid::Uuid, x: i32, z: i32) -> Result<(), Error> {
        if !self.listens_for(CHUNK_UNLOAD_EVENT) {
            return Ok(());
        }
        self.jvm.with_env(|env| {
            let world = env.new_string(world.to_string())?;
            env.call_static_method(
                BOOTSTRAP,
                "fireChunkUnload",
                "(Ljava/lang/String;II)V",
                &[
                    JValue::Object(world.as_ref()),
                    JValue::Int(x),
                    JValue::Int(z),
                ],
            )?;
            Ok(())
        })?;
        Ok(())
    }

    /// Fires Bukkit's `PlayerDeathEvent`; no JNI call without a registered listener.
    pub fn fire_player_death(
        &self,
        id: uuid::Uuid,
        name: &str,
        cause: &str,
        dropped_exp: i32,
        message: &str,
    ) -> Result<(), Error> {
        if !self.listens_for(PLAYER_DEATH_EVENT) {
            return Ok(());
        }
        self.jvm.with_env(|env| {
            let id = env.new_string(id.to_string())?;
            let name = env.new_string(name)?;
            let cause = env.new_string(cause)?;
            let message = env.new_string(message)?;
            env.call_static_method(
                BOOTSTRAP,
                "firePlayerDeath",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;ILjava/lang/String;)V",
                &[
                    JValue::Object(id.as_ref()),
                    JValue::Object(name.as_ref()),
                    JValue::Object(cause.as_ref()),
                    JValue::Int(dropped_exp),
                    JValue::Object(message.as_ref()),
                ],
            )?;
            Ok(())
        })?;
        Ok(())
    }

    /// Fires Bukkit's `PlayerRespawnEvent`; returns the location a listener selected.
    pub fn fire_player_respawn(
        &self,
        id: uuid::Uuid,
        name: &str,
        spawn: [f64; 3],
        yaw: f32,
        pitch: f32,
    ) -> Result<[f64; 3], Error> {
        if !self.listens_for(PLAYER_RESPAWN_EVENT) {
            return Ok(spawn);
        }
        let result = self.jvm.with_env(|env| {
            let id = env.new_string(id.to_string())?;
            let name = env.new_string(name)?;
            let reason = env.new_string("DEATH")?;
            let value = env
                .call_static_method(
                    BOOTSTRAP,
                    "firePlayerRespawn",
                    "(Ljava/lang/String;Ljava/lang/String;DDDFFLjava/lang/String;)Ljava/lang/String;",
                    &[
                        JValue::Object(id.as_ref()),
                        JValue::Object(name.as_ref()),
                        JValue::Double(spawn[0]),
                        JValue::Double(spawn[1]),
                        JValue::Double(spawn[2]),
                        JValue::Float(yaw),
                        JValue::Float(pitch),
                        JValue::Object(reason.as_ref()),
                    ],
                )?
                .l()?;
            if value.is_null() {
                return Ok(None);
            }
            let text: String = env.get_string(&JString::from(value))?.into();
            Ok(Some(text))
        })?;
        let Some(text) = result else {
            return Ok(spawn);
        };
        let parts: Vec<f64> = text
            .split_whitespace()
            .filter_map(|part| part.parse().ok())
            .collect();
        if parts.len() < 3 {
            return Ok(spawn);
        }
        Ok([parts[0], parts[1], parts[2]])
    }

    /// Fires Bukkit's `EntityDamageEvent`; returns the final damage.
    pub fn fire_entity_damage(
        &self,
        id: uuid::Uuid,
        name: &str,
        cause: &str,
        amount: f64,
    ) -> Result<f64, Error> {
        if !self.listens_for(ENTITY_DAMAGE_EVENT) {
            return Ok(amount);
        }
        Ok(self.jvm.with_env(|env| {
            let id = env.new_string(id.to_string())?;
            let name = env.new_string(name)?;
            let cause = env.new_string(cause)?;
            let value = env
                .call_static_method(
                    BOOTSTRAP,
                    "fireEntityDamage",
                    "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;D)D",
                    &[
                        JValue::Object(id.as_ref()),
                        JValue::Object(name.as_ref()),
                        JValue::Object(cause.as_ref()),
                        JValue::Double(amount),
                    ],
                )?
                .d()?;
            Ok(value)
        })?)
    }

    /// Fires Bukkit's `EntityDeathEvent` for a non-player entity.
    pub fn fire_entity_death(
        &self,
        id: uuid::Uuid,
        kind: &str,
        position: [f64; 3],
        dropped_exp: i32,
    ) -> Result<(), Error> {
        if !self.listens_for(ENTITY_DEATH_EVENT) {
            return Ok(());
        }
        self.jvm.with_env(|env| {
            let id = env.new_string(id.to_string())?;
            let kind = env.new_string(kind)?;
            env.call_static_method(
                BOOTSTRAP,
                "fireEntityDeath",
                "(Ljava/lang/String;Ljava/lang/String;DDDI)V",
                &[
                    JValue::Object(id.as_ref()),
                    JValue::Object(kind.as_ref()),
                    JValue::Double(position[0]),
                    JValue::Double(position[1]),
                    JValue::Double(position[2]),
                    JValue::Int(dropped_exp),
                ],
            )?;
            Ok(())
        })?;
        Ok(())
    }

    /// Fires Bukkit's `WeatherChangeEvent`; `false` cancels the change.
    pub fn fire_weather_change(
        &self,
        world: uuid::Uuid,
        to_weather_state: bool,
    ) -> Result<bool, Error> {
        if !self.listens_for(WEATHER_CHANGE_EVENT) {
            return Ok(true);
        }
        Ok(self.jvm.with_env(|env| {
            let world = env.new_string(world.to_string())?;
            let allowed = env
                .call_static_method(
                    BOOTSTRAP,
                    "fireWeatherChange",
                    "(Ljava/lang/String;Z)Z",
                    &[
                        JValue::Object(world.as_ref()),
                        JValue::Bool(u8::from(to_weather_state)),
                    ],
                )?
                .z()?;
            Ok(allowed)
        })?)
    }

    /// Fires Bukkit's `WorldLoadEvent` for a newly published world.
    pub fn fire_world_load(&self, world: uuid::Uuid) -> Result<(), Error> {
        if !self.listens_for(WORLD_LOAD_EVENT) {
            return Ok(());
        }
        self.fire_string_id("fireWorldLoad", "(Ljava/lang/String;)V", world)
    }

    /// Fires Bukkit's `PlayerLevelChangeEvent`; returns the level to use.
    pub fn fire_player_level(
        &self,
        id: uuid::Uuid,
        name: &str,
        old_level: i32,
        new_level: i32,
    ) -> Result<i32, Error> {
        if !self.listens_for(PLAYER_LEVEL_CHANGE_EVENT) {
            return Ok(new_level);
        }
        Ok(self.jvm.with_env(|env| {
            let id = env.new_string(id.to_string())?;
            let name = env.new_string(name)?;
            let value = env
                .call_static_method(
                    BOOTSTRAP,
                    "firePlayerLevelChange",
                    "(Ljava/lang/String;Ljava/lang/String;II)I",
                    &[
                        JValue::Object(id.as_ref()),
                        JValue::Object(name.as_ref()),
                        JValue::Int(old_level),
                        JValue::Int(new_level),
                    ],
                )?
                .i()?;
            Ok(value)
        })?)
    }

    /// Fires Bukkit's `PlayerExpChangeEvent`; returns the amount to grant.
    pub fn fire_player_exp(&self, id: uuid::Uuid, name: &str, amount: i32) -> Result<i32, Error> {
        if !self.listens_for(PLAYER_EXP_CHANGE_EVENT) {
            return Ok(amount);
        }
        Ok(self.jvm.with_env(|env| {
            let id = env.new_string(id.to_string())?;
            let name = env.new_string(name)?;
            let value = env
                .call_static_method(
                    BOOTSTRAP,
                    "firePlayerExpChange",
                    "(Ljava/lang/String;Ljava/lang/String;I)I",
                    &[
                        JValue::Object(id.as_ref()),
                        JValue::Object(name.as_ref()),
                        JValue::Int(amount),
                    ],
                )?
                .i()?;
            Ok(value)
        })?)
    }

    /// Fires Bukkit's `InventoryClickEvent`; `false` cancels the click.
    pub fn fire_inventory_click(
        &self,
        id: uuid::Uuid,
        name: &str,
        slot: i32,
        click: &str,
        action: &str,
    ) -> Result<bool, Error> {
        if !self.listens_for(INVENTORY_CLICK_EVENT) {
            return Ok(true);
        }
        Ok(self.jvm.with_env(|env| {
            let id = env.new_string(id.to_string())?;
            let name = env.new_string(name)?;
            let click = env.new_string(click)?;
            let action = env.new_string(action)?;
            let allowed = env
                .call_static_method(
                    BOOTSTRAP,
                    "fireInventoryClick",
                    "(Ljava/lang/String;Ljava/lang/String;ILjava/lang/String;Ljava/lang/String;)Z",
                    &[
                        JValue::Object(id.as_ref()),
                        JValue::Object(name.as_ref()),
                        JValue::Int(slot),
                        JValue::Object(click.as_ref()),
                        JValue::Object(action.as_ref()),
                    ],
                )?
                .z()?;
            Ok(allowed)
        })?)
    }

    /// Fires Bukkit's `InventoryCloseEvent`.
    pub fn fire_inventory_close(&self, id: uuid::Uuid, name: &str) -> Result<(), Error> {
        if !self.listens_for(INVENTORY_CLOSE_EVENT) {
            return Ok(());
        }
        self.jvm.with_env(|env| {
            let id = env.new_string(id.to_string())?;
            let name = env.new_string(name)?;
            env.call_static_method(
                BOOTSTRAP,
                "fireInventoryClose",
                "(Ljava/lang/String;Ljava/lang/String;)V",
                &[JValue::Object(id.as_ref()), JValue::Object(name.as_ref())],
            )?;
            Ok(())
        })?;
        Ok(())
    }

    pub fn shutdown(&self) -> Result<(), Error> {
        self.issued_regions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        self.issued_entities
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        Ok(self.jvm.call_static(BOOTSTRAP, "shutdown", "()V", &[])?)
    }
}
impl ServerHooks for PluginHost {
    fn player_joined(&self, id: uuid::Uuid, name: &str) -> Result<(), Error> {
        self.fire_player_join(id, name)
    }
    fn player_left(&self, id: uuid::Uuid, _name: &str) -> Result<(), Error> {
        self.fire_player_quit(id)
    }
    fn chat(&self, id: uuid::Uuid, name: &str, message: &str) -> Result<Option<String>, Error> {
        self.fire_chat(id, name, message)
    }
    fn player_moved(
        &self,
        id: uuid::Uuid,
        name: &str,
        movement: &PlayerMove,
    ) -> Result<bool, Error> {
        self.fire_player_move(id, name, movement)
    }
    fn player_interacted(
        &self,
        id: uuid::Uuid,
        name: &str,
        action: InteractAction,
        block: BlockPosition,
        face: u8,
        material: &str,
    ) -> Result<bool, Error> {
        self.fire_player_interact(id, name, action, block, face, material)
    }
    fn block_broken(
        &self,
        id: uuid::Uuid,
        name: &str,
        block: BlockPosition,
        material: &str,
    ) -> Result<bool, Error> {
        self.fire_block_break(id, name, block, material)
    }
    fn block_placed(
        &self,
        id: uuid::Uuid,
        name: &str,
        block: BlockPosition,
        face: u8,
        placed: &str,
        replaced: &str,
    ) -> Result<bool, Error> {
        self.fire_block_place(id, name, block, face, placed, replaced)
    }
    fn player_command(&self, id: uuid::Uuid, name: &str, message: &str) -> Result<bool, Error> {
        self.fire_player_command(id, name, message)
    }
    fn chunk_loaded(&self, world: uuid::Uuid, x: i32, z: i32, new: bool) -> Result<(), Error> {
        self.fire_chunk_load(world, x, z, new)
    }
    fn chunk_unloaded(&self, world: uuid::Uuid, x: i32, z: i32) -> Result<(), Error> {
        self.fire_chunk_unload(world, x, z)
    }
    fn player_died(
        &self,
        id: uuid::Uuid,
        name: &str,
        cause: &str,
        dropped_exp: i32,
        message: &str,
    ) -> Result<(), Error> {
        self.fire_player_death(id, name, cause, dropped_exp, message)
    }
    fn player_respawned(
        &self,
        id: uuid::Uuid,
        name: &str,
        spawn: [f64; 3],
        yaw: f32,
        pitch: f32,
    ) -> Result<[f64; 3], Error> {
        self.fire_player_respawn(id, name, spawn, yaw, pitch)
    }
    fn entity_damaged(
        &self,
        id: uuid::Uuid,
        name: &str,
        cause: &str,
        amount: f64,
    ) -> Result<f64, Error> {
        self.fire_entity_damage(id, name, cause, amount)
    }
    fn entity_died(
        &self,
        id: uuid::Uuid,
        kind: &str,
        position: [f64; 3],
        dropped_exp: i32,
    ) -> Result<(), Error> {
        self.fire_entity_death(id, kind, position, dropped_exp)
    }
    fn weather_changing(&self, world: uuid::Uuid, to_weather_state: bool) -> Result<bool, Error> {
        self.fire_weather_change(world, to_weather_state)
    }
    fn world_loaded(&self, world: uuid::Uuid) -> Result<(), Error> {
        self.fire_world_load(world)
    }
    fn player_level_changed(
        &self,
        id: uuid::Uuid,
        name: &str,
        old_level: i32,
        new_level: i32,
    ) -> Result<i32, Error> {
        self.fire_player_level(id, name, old_level, new_level)
    }
    fn player_exp_changed(&self, id: uuid::Uuid, name: &str, amount: i32) -> Result<i32, Error> {
        self.fire_player_exp(id, name, amount)
    }
    fn inventory_clicked(
        &self,
        id: uuid::Uuid,
        name: &str,
        slot: i32,
        click: &str,
        action: &str,
    ) -> Result<bool, Error> {
        self.fire_inventory_click(id, name, slot, click, action)
    }
    fn inventory_closed(&self, id: uuid::Uuid, name: &str) -> Result<(), Error> {
        self.fire_inventory_close(id, name)
    }
    fn plugin_commands(&self) -> Result<Vec<PluginCommand>, Error> {
        self.declared_commands()
    }
    fn tick(&self) -> Result<(), Error> {
        PluginHost::tick(self)
    }
    fn register_world(&self, world: &WorldSnapshot) -> Result<(), Error> {
        PluginHost::register_world(self, world)
    }
    fn register_player(&self, player: &PlayerSnapshot) -> Result<(), Error> {
        PluginHost::register_player(self, player)
    }
    fn run_plugin_command(
        &self,
        sender: CommandSender,
        line: &str,
    ) -> Result<PluginCommandResult, Error> {
        self.execute_plugin_command(sender, line)
    }
    fn take_region_tasks(&self) -> Vec<RegionTask> {
        PluginHost::take_region_tasks(self)
    }
    fn run_region_task(&self, task: RegionTask) -> Result<(), Error> {
        PluginHost::run_region_task(self, task)
    }
    fn update_player_location(
        &self,
        id: uuid::Uuid,
        location: &PlayerLocation,
    ) -> Result<(), Error> {
        PluginHost::update_player_location(self, id, location)
    }
}
fn consume_task<T: PartialEq>(issued: &Mutex<Vec<T>>, task: &T) -> Result<(), Error> {
    let mut issued = issued
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let index = issued
        .iter()
        .position(|candidate| candidate == task)
        .ok_or(Error::InvalidTask)?;
    issued.swap_remove(index);
    Ok(())
}
mod scheduling;
