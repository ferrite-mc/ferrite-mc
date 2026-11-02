//! The server's command tree and the game-loop actions its commands emit.
//!
//! Parsing, permission requirements and selectors live here (as in Brigadier); the
//! world state changes those commands describe are applied by the game loop.
use ferrite_admin::AdminLists;
use ferrite_command::{
    ArgumentType, ArgumentValue, CommandContext, CommandDispatcher, CommandError,
    CommandSyntaxError, EntityAnchor, argument,
    arguments::{Coordinates, EntityTarget, GameMode, SelectorKind, WorldCoordinate},
    literal,
};
use ferrite_entity::mob::MobKind;
use ferrite_nbt::Tag;
use ferrite_net::game::OutboundSender;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

/// The overworld clock's registry name, used by `/time` feedback.
pub const OVERWORLD_CLOCK: &str = "minecraft:overworld";
const SERVER_NAME: &str = "Server";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weather {
    Clear,
    Rain,
    Thunder,
}

impl Weather {
    fn feedback_key(self) -> &'static str {
        match self {
            Self::Clear => "commands.weather.set.clear",
            Self::Rain => "commands.weather.set.rain",
            Self::Thunder => "commands.weather.set.thunder",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerEntry {
    pub uuid: Uuid,
    pub name: String,
    pub game_mode: i32,
    /// `/spawnpoint`: block position, yaw and pitch; `None` until the player sets one.
    pub spawn: Option<([i32; 3], f32, f32)>,
    /// Active mob effects (`/effect`), effect id -> amplifier.
    pub effects: BTreeMap<i32, i32>,
    /// `/tag` names on this player.
    pub tags: BTreeSet<String>,
    /// `/team join` membership, if any.
    pub team: Option<String>,
    /// Last known position and rotation (`execute at`, `/data`, `/tp`).
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    /// The dimension the player is in, for `/execute in` and dimension-aware selectors.
    pub dimension: super::Dimension,
    /// Experience level, matched by the `level=` selector option.
    pub xp_level: i32,
}

/// A `/scoreboard objectives` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Objective {
    pub criteria: String,
    pub display_name: String,
    /// Objective render type: 0 = integer, 1 = hearts.
    pub render_type: i32,
}

/// A `/bossbar` entry.
#[derive(Debug, Clone, PartialEq)]
pub struct BossBarState {
    pub uuid: Uuid,
    pub name: String,
    pub progress: f32,
    pub value: i32,
    pub max: i32,
    pub color: i32,
    /// 0 = progress, 1 = notched_6, 2 = notched_10, 3 = notched_12, 4 = notched_20.
    pub overlay: i32,
    pub visible: bool,
    /// Players who see the bar; empty means every player.
    pub players: BTreeSet<Uuid>,
}

/// A `/team` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamState {
    pub display_name: String,
    pub color: i32,
    pub prefix: String,
    pub suffix: String,
    pub friendly_fire: bool,
    pub see_friendly_invisibles: bool,
    pub nametag_visibility: String,
    pub collision_rule: String,
    pub death_message_visibility: String,
    pub members: BTreeSet<Uuid>,
}

/// World border state shared by `/worldborder` and the packets its subcommands send.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldBorderState {
    pub center_x: f64,
    pub center_z: f64,
    pub diameter: f64,
    pub warning_distance: i32,
    /// Warning time in ticks.
    pub warning_time: i32,
    pub damage_per_block: f32,
    pub damage_buffer: f32,
}

impl Default for WorldBorderState {
    fn default() -> Self {
        Self {
            center_x: 0.0,
            center_z: 0.0,
            diameter: 59_999_968.0,
            warning_distance: 5,
            warning_time: 15,
            damage_per_block: 0.2,
            damage_buffer: 5.0,
        }
    }
}

/// Mutable world state needed by commands. Commands run on the game loop, so a short-lived
/// mutex keeps `/list`, `/gamemode <mode> <target>`, and the clocks consistent with joins,
/// leaves, and each other.
#[derive(Debug, Clone)]
pub struct GameState {
    pub default_gamemode: i32,
    pub difficulty: i32,
    /// Ticks of the overworld clock (`/time set` and `/time add`).
    pub clock_time: i64,
    /// Total world ticks (`/time query gametime`).
    pub game_time: i64,
    pub weather: Weather,
    pub weather_duration: i32,
    pub seed: i64,
    pub max_players: i32,
    pub players: Vec<PlayerEntry>,
    /// World spawn (`/setworldspawn`), its yaw and pitch.
    pub world_spawn: [i32; 3],
    pub world_spawn_yaw: f32,
    pub world_spawn_pitch: f32,
    /// World border (`/worldborder`).
    pub border: WorldBorderState,
    /// Scoreboard objectives by name.
    pub objectives: BTreeMap<String, Objective>,
    /// Scores keyed by `(holder, objective)`.
    pub scores: BTreeMap<(String, String), i32>,
    /// Display slot -> objective name.
    pub displays: BTreeMap<i32, String>,
    /// Custom boss bars by id.
    pub bossbars: BTreeMap<String, BossBarState>,
    /// Scoreboard teams by name.
    pub teams: BTreeMap<String, TeamState>,
    /// Command storage (`/data ... storage` and `execute store ... storage`).
    pub storage: BTreeMap<String, Tag>,
    /// Datapack functions loaded from the world's `datapacks` folder.
    pub functions: Arc<super::functions::FunctionStore>,
    /// Region-published entity snapshot used by entity selectors.
    pub entities: Arc<Mutex<super::entity_index::EntityIndex>>,
    /// `/schedule function`: function id -> due gametimes, oldest first.
    pub schedules: BTreeMap<String, Vec<i64>>,
    /// Trigger objectives a player has "primed" with `scoreboard players enable`.
    pub trigger_enabled: BTreeSet<(String, String)>,
}

impl GameState {
    pub fn new(default_gamemode: i32, seed: i64, max_players: i32) -> Self {
        Self {
            default_gamemode,
            difficulty: 2,
            clock_time: 0,
            game_time: 0,
            weather: Weather::Clear,
            weather_duration: 6_000,
            seed,
            max_players,
            players: Vec::new(),
            world_spawn: [0, 64, 0],
            world_spawn_yaw: 0.0,
            world_spawn_pitch: 0.0,
            border: WorldBorderState::default(),
            objectives: BTreeMap::new(),
            scores: BTreeMap::new(),
            displays: BTreeMap::new(),
            bossbars: BTreeMap::new(),
            teams: BTreeMap::new(),
            storage: BTreeMap::new(),
            functions: Arc::new(super::functions::FunctionStore::default()),
            entities: Arc::new(Mutex::new(super::entity_index::EntityIndex::default())),
            schedules: BTreeMap::new(),
            trigger_enabled: BTreeSet::new(),
        }
    }

    pub fn join(&mut self, uuid: Uuid, name: String, game_mode: i32, max_players: i32) {
        self.leave(uuid);
        self.players.push(PlayerEntry {
            uuid,
            name,
            game_mode,
            spawn: None,
            effects: BTreeMap::new(),
            tags: BTreeSet::new(),
            team: None,
            pos: [0.0, 64.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            dimension: super::Dimension::default(),
            xp_level: 0,
        });
        self.max_players = max_players;
    }

    /// Records a player's last known position for commands that resolve entity state.
    pub fn set_position(&mut self, uuid: Uuid, pos: [f64; 3], yaw: f32, pitch: f32) {
        if let Some(player) = self.players.iter_mut().find(|player| player.uuid == uuid) {
            player.pos = pos;
            player.yaw = yaw;
            player.pitch = pitch;
        }
    }

    /// Records the dimension a player is in after a transfer or respawn.
    pub fn set_dimension(&mut self, uuid: Uuid, dimension: super::Dimension) {
        if let Some(player) = self.players.iter_mut().find(|player| player.uuid == uuid) {
            player.dimension = dimension;
        }
    }

    pub fn leave(&mut self, uuid: Uuid) {
        self.players.retain(|player| player.uuid != uuid);
    }

    /// Advances the world tick. The overworld clock only moves while
    /// `advance_time` (vanilla `doDaylightCycle`) is enabled. Returns the scheduled functions
    /// that are due this tick, once per function.
    pub fn tick(&mut self, advance_time: bool) -> Vec<String> {
        self.game_time += 1;
        if advance_time {
            self.clock_time += 1;
        }
        let now = self.game_time;
        let mut due = Vec::new();
        for (name, times) in &mut self.schedules {
            let before = times.len();
            times.retain(|due| *due > now);
            if times.len() != before {
                due.push(name.clone());
            }
        }
        self.schedules.retain(|_, times| !times.is_empty());
        due
    }
}

impl Default for GameState {
    fn default() -> Self {
        Self::new(1, 0, 20)
    }
}

/// A vanilla translation key, its arguments, and the English rendering used by the
/// current plain-text chat packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feedback {
    pub key: &'static str,
    pub args: Vec<String>,
    pub text: String,
}

impl Feedback {
    pub fn new(key: &'static str, args: Vec<String>) -> Self {
        let text = match translate(key) {
            Some(template) => render(template, &args),
            None if english(key).is_empty() => {
                super::feedback::render(key, &args.iter().map(String::as_str).collect::<Vec<_>>())
            }
            None => render(english(key), &args),
        };
        Self { key, text, args }
    }
}

/// Vanilla's `reports/lang_en_us.json`, installed once at server startup so feedback
/// text is the exact 26.3 string for each key. Without it (unit tests, data-less
/// deployments) [`english`] supplies the same strings.
static LANG: OnceLock<HashMap<String, String>> = OnceLock::new();

/// Installs the English translations from `reports/lang_en_us.json`.
pub fn load_lang(reports: &std::path::Path) {
    let Ok(text) = std::fs::read_to_string(reports.join("lang_en_us.json")) else {
        return;
    };
    let Ok(table) = serde_json::from_str::<HashMap<String, String>>(&text) else {
        return;
    };
    let _ = LANG.set(table);
}

fn translate(key: &str) -> Option<&'static str> {
    LANG.get()?.get(key).map(String::as_str)
}

fn english(key: &str) -> &'static str {
    match key {
        "commands.gamemode.success.self" => "Set own game mode to %s",
        "commands.gamemode.success.other" => "Set %s's game mode to %s",
        "gameMode.changed" => "Your game mode has been updated to %s",
        "commands.defaultgamemode.success" => "The default game mode is now %s",
        "commands.difficulty.success" => "The difficulty has been set to %s",
        "commands.difficulty.query" => "The difficulty is %s",
        "commands.time.set.absolute" => "Set %s to %s tick(s)",
        "commands.time.query.absolute" => "Clock %s is at %s tick(s)",
        "commands.time.query.gametime" => "The game time is %s tick(s)",
        "commands.weather.set.clear" => "Set the weather to clear",
        "commands.weather.set.rain" => "Set the weather to rain",
        "commands.weather.set.thunder" => "Set the weather to rain and thunder",
        "commands.seed.success" => "Seed: %s",
        "commands.list.players" => "There are %s of a max of %s players online: %s",
        "commands.locate.biome.success" => "The nearest %s is at [%s, %s, %s]",
        "commands.locate.biome.not_found" => "Could not find a valid biome in the vicinity",
        "commands.locate.poi.not_found" => {
            "Could not find a valid point of interest in the vicinity"
        }
        "commands.locate.structure.not_found" => "Could not find a valid structure in the vicinity",
        "commands.reload.success" => "Reloading!",
        "commands.reload.failure" => "Failed to reload: %s",
        "commands.datapack.unknown" => "Unknown data pack '%s'",
        "commands.datapack.list.enabled.success" => "There are %s data packs enabled: %s",
        "commands.datapack.list.enabled.none" => "There are no data packs enabled",
        "commands.datapack.list.available.success" => "There are %s data packs available: %s",
        "commands.datapack.enable.success" => "Enabled data pack '%s'",
        "commands.datapack.disable.success" => "Disabled data pack '%s'",
        "commands.datapack.enable.failed" => "Data pack '%s' is already enabled",
        "commands.datapack.disable.failed" => "Data pack '%s' is already disabled",
        "commands.help.header" => "--- Showing help page %s of %s (/help <page>) ---",
        "commands.help.command" => "%s",
        "commands.help.usage" => "Usage: %s",
        "commands.failed.help" => "Unknown command, please try to use /help",
        "commands.schedule.created.function" => {
            "%s has been scheduled to run in %s tick(s) at gametime %s"
        }
        "commands.schedule.cleared.success" => "Removed %s scheduled function(s) for %s",
        "commands.schedule.cleared.failure" => "There were no schedules for %s",
        "commands.schedule.same_tick" => "Can't schedule for the current tick",
        "commands.trigger.add.success" => "Added %s to %s",
        "commands.trigger.set.success" => "Set %s to %s",
        "commands.trigger.disabled" => "Trigger %s is not enabled",
        "commands.trigger.invalid_objective" => "Objective %s not found",
        "chat.type.announcement" => "[%s] %s",
        "chat.type.emote" => "* %s %s",
        _ => "",
    }
}

fn render(template: &str, args: &[String]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut args = args.iter();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('%') => {
                chars.next();
                out.push('%');
            }
            Some('s') => {
                chars.next();
                if let Some(value) = args.next() {
                    out.push_str(value);
                }
            }
            _ => out.push('%'),
        }
    }
    out
}

/// A parsed teleport destination. Locations are already resolved against the source.
#[derive(Debug, Clone, PartialEq)]
pub enum TeleportDestination {
    /// An absolute position, with an optional explicit rotation.
    Location {
        pos: [f64; 3],
        yaw: Option<f32>,
        pitch: Option<f32>,
    },
    /// A position to look at after teleporting.
    FacingLocation { pos: [f64; 3], target: [f64; 3] },
    /// An entity to look at after teleporting.
    FacingEntity {
        pos: [f64; 3],
        entity: EntityTarget,
        anchor: EntityAnchor,
    },
    /// Another entity's position.
    Entity(EntityTarget),
}

/// An `/xp` (vanilla `/experience`) operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XpOperation {
    Add,
    Set,
    Query,
}

/// Whether an `/xp` amount counts experience points or levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XpUnit {
    Points,
    Levels,
}

/// A command's effect, applied by the game loop.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Teleport {
        targets: EntityTarget,
        destination: TeleportDestination,
        /// Dimension the destination coordinates belong to (`/execute in`).
        dimension: super::Dimension,
    },
    Message {
        targets: EntityTarget,
        text: String,
    },
    Kill {
        targets: EntityTarget,
    },
    Op {
        targets: EntityTarget,
    },
    Deop {
        targets: EntityTarget,
    },
    WhitelistAdd {
        targets: EntityTarget,
    },
    WhitelistRemove {
        targets: EntityTarget,
    },
    WhitelistList,
    WhitelistOn,
    WhitelistOff,
    WhitelistReload,
    Ban {
        targets: EntityTarget,
        reason: String,
    },
    BanIp {
        target: String,
        reason: String,
    },
    Pardon {
        targets: EntityTarget,
    },
    PardonIp {
        target: String,
    },
    BanList {
        ips: bool,
    },
    Kick {
        targets: EntityTarget,
        reason: String,
    },
    Stop,
    SaveAll {
        flush: bool,
    },
    SaveOn,
    SaveOff,
    GameRule(String, Option<String>),
    GameMode {
        mode: i32,
        targets: Vec<Uuid>,
    },
    DefaultGameMode(i32),
    Difficulty(i32),
    TimeSet(i32),
    TimeAdd(i32),
    Weather {
        kind: Weather,
        duration: Option<i32>,
    },
    /// Feedback for the command source.
    Feedback(Feedback),
    /// Feedback for one player (for example `gameMode.changed`).
    PlayerFeedback {
        uuid: Uuid,
        feedback: Feedback,
    },
    /// Chat visible to every player (`say`, `me`).
    Broadcast(Feedback),
    /// `/clear`: remove matching items from the targets' inventories.
    Clear {
        targets: Vec<Uuid>,
        item: Option<u16>,
        max_count: Option<i32>,
    },
    /// `/give`: place items in each target's inventory.
    Give {
        targets: Vec<Uuid>,
        item: u16,
        count: i32,
        item_name: String,
        /// Item components from the `item[component={...}]` suffix, as raw NBT.
        components: Vec<(u16, ferrite_nbt::Tag)>,
    },
    /// `/xp` (vanilla `/experience`): add, set or query points or levels.
    Experience {
        operation: XpOperation,
        targets: Vec<Uuid>,
        amount: i32,
        unit: XpUnit,
    },
    Tps,
    Mspt,
    /// `/summon`: create a mob at an absolute position.
    Summon {
        kind: MobKind,
        pos: [f64; 3],
    },
    ProfileStart,
    ProfileStop,
    Plugin {
        name: String,
        args: Vec<String>,
    },
    World(super::commands_world::WorldCommand),
    /// A clientbound packet for the given players, or every player when `targets` is empty.
    Packets {
        targets: Vec<Uuid>,
        packet: super::commands_extras::PlayerPacket,
    },
    /// `/effect give`: apply a mob effect to the target players.
    Effect {
        targets: Vec<Uuid>,
        effect: i32,
        amplifier: i32,
        duration: i32,
    },
    /// `/effect clear`: remove one mob effect from the target players.
    RemoveEffect {
        targets: Vec<Uuid>,
        effect: i32,
    },
    /// `/enchant`: enchant the item in each target's main hand.
    Enchant {
        targets: Vec<Uuid>,
        enchantment: String,
        level: i32,
    },
    /// `/data` on a block entity, applied by the game loop against the chunk source.
    Data(super::commands_data::BlockData),
    /// `/reload`: rebuild the datapack function store from disk.
    Reload,
    /// `/datapack list`: report enabled and disabled packs.
    DatapackList,
    /// `/datapack enable|disable`: persist the choice and reload functions.
    DatapackSet {
        pack: String,
        enable: bool,
    },
}

/// Who ran a command: permission level, position/orientation for relative and local
/// coordinates, and the channel actions are sent on.
#[derive(Clone)]
pub struct Source {
    pub permission: u8,
    pub origin: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub uuid: Option<Uuid>,
    pub name: Option<String>,
    /// `execute anchored`: `eyes` lifts the source position by the eye height.
    pub anchor: EntityAnchor,
    /// Dimension positions and selectors resolve in (`/execute in`).
    pub dimension: super::Dimension,
    /// Chunk source when the command runs inside a world (block tests, `/data block`).
    pub world: Option<Arc<super::anvil::AnvilChunkSource>>,
    pub state: Arc<Mutex<GameState>>,
    pub actions: UnboundedSender<Action>,
    /// The command tree `/function` lines run against; `None` in tests that never call it.
    pub tree: Option<Arc<CommandDispatcher<Source>>>,
    /// Current `/function` nesting depth, checked against the recursion limit.
    pub function_depth: u32,
}

/// Vanilla's `commands/entity_selectors` permission maps to gamemasters: any `@` selector
/// in a command argument requires this level, even for commands open to every player. It is
/// enforced once, by the dispatcher's selector requirement in [`dispatcher`].
pub const SELECTOR_PERMISSION: u8 = 2;

impl Source {
    fn sender_name(&self) -> String {
        self.name.clone().unwrap_or_else(|| SERVER_NAME.into())
    }
}

/// A player resolved from an entity argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPlayer {
    pub uuid: Uuid,
    pub name: String,
}

/// Online player state the command handlers need.
#[derive(Debug, Clone)]
pub struct OnlinePlayer {
    pub name: String,
    pub entity: i32,
    pub outbound: OutboundSender,
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub dimension: super::Dimension,
}

pub(crate) fn send(source: &Source, action: Action) -> Result<i32, CommandError> {
    source
        .actions
        .send(action)
        .map_err(|_| CommandError::failed("command target unavailable"))?;
    Ok(1)
}
/// Adds every command a plugin declared in its descriptor, including aliases.
///
/// Bukkit resolves the name and every alias to the same `PluginCommand`, so each literal is
/// registered on its own and runs the plugin under the label the sender actually typed.
pub fn register_plugin_commands(
    tree: &mut CommandDispatcher<Source>,
    commands: &[ferrite_plugin::PluginCommand],
) {
    for command in commands {
        tree.register(plugin_command_node(&command.name));
        for alias in &command.aliases {
            tree.register(plugin_command_node(alias));
        }
    }
}
/// One literal (a plugin command name or alias); the plugin sees it as the label.
fn plugin_command_node(literal_name: &str) -> ferrite_command::NodeBuilder<Source> {
    let bare = literal_name.to_owned();
    let with_args = literal_name.to_owned();
    literal::<Source>(literal_name)
        .executes(move |ctx| {
            send(
                ctx.source(),
                Action::Plugin {
                    name: bare.clone(),
                    args: Vec::new(),
                },
            )
        })
        .then(
            argument::<Source>("args", ArgumentType::greedy_string()).executes(move |ctx| {
                let args = ctx
                    .string("args")
                    .unwrap_or_default()
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect();
                send(
                    ctx.source(),
                    Action::Plugin {
                        name: with_args.clone(),
                        args,
                    },
                )
            }),
        )
}

/// Reads an entity argument. Selector permissions are not checked here: the dispatcher
/// rejects `@` selectors while parsing, for every command at once.
fn entity(ctx: &CommandContext<Source>, name: &str) -> Result<EntityTarget, CommandError> {
    ctx.argument(name)
        .and_then(ArgumentValue::as_entity)
        .cloned()
        .ok_or_else(|| CommandError::failed(format!("missing entity argument '{name}'")))
}

pub(crate) fn coordinates(
    ctx: &CommandContext<Source>,
    name: &str,
) -> Result<Coordinates, CommandError> {
    ctx.argument(name)
        .and_then(ArgumentValue::as_coordinates)
        .copied()
        .ok_or_else(|| CommandError::failed(format!("missing coordinate argument '{name}'")))
}

fn rotation(
    ctx: &CommandContext<Source>,
    name: &str,
) -> Result<[WorldCoordinate; 2], CommandError> {
    match ctx.argument(name) {
        Some(ArgumentValue::Rotation(rotation)) => Ok(*rotation),
        _ => Err(CommandError::failed(format!(
            "missing rotation argument '{name}'"
        ))),
    }
}

fn anchor(ctx: &CommandContext<Source>, name: &str) -> Result<EntityAnchor, CommandError> {
    ctx.argument(name)
        .and_then(ArgumentValue::as_entity_anchor)
        .ok_or_else(|| CommandError::failed(format!("missing anchor argument '{name}'")))
}

fn message_arg(ctx: &CommandContext<Source>, name: &str) -> Result<String, CommandError> {
    ctx.string(name)
        .map(str::to_owned)
        .ok_or_else(|| CommandError::failed(format!("missing text argument '{name}'")))
}

fn self_target() -> EntityTarget {
    EntityTarget::Selector {
        kind: SelectorKind::Myself,
        options: Vec::new(),
    }
}

fn target_or_self(
    ctx: &CommandContext<Source>,
    targets: bool,
) -> Result<EntityTarget, CommandError> {
    if targets {
        entity(ctx, "targets")
    } else {
        Ok(self_target())
    }
}

fn location(ctx: &CommandContext<Source>, targets: bool) -> Result<Action, CommandError> {
    let pos = coordinates(ctx, "location")?.resolve(
        ctx.source().origin,
        ctx.source().yaw,
        ctx.source().pitch,
    );
    Ok(Action::Teleport {
        targets: target_or_self(ctx, targets)?,
        dimension: ctx.source().dimension,
        destination: TeleportDestination::Location {
            pos,
            yaw: None,
            pitch: None,
        },
    })
}

fn location_rotation(ctx: &CommandContext<Source>, targets: bool) -> Result<Action, CommandError> {
    let pos = coordinates(ctx, "location")?.resolve(
        ctx.source().origin,
        ctx.source().yaw,
        ctx.source().pitch,
    );
    let (yaw, pitch) = ferrite_command::arguments::coords::resolve_rotation(
        rotation(ctx, "rotation")?,
        ctx.source().yaw,
        ctx.source().pitch,
    );
    Ok(Action::Teleport {
        targets: target_or_self(ctx, targets)?,
        dimension: ctx.source().dimension,
        destination: TeleportDestination::Location {
            pos,
            yaw: Some(yaw),
            pitch: Some(pitch),
        },
    })
}

fn location_facing_location(
    ctx: &CommandContext<Source>,
    targets: bool,
) -> Result<Action, CommandError> {
    let source = ctx.source();
    Ok(Action::Teleport {
        targets: target_or_self(ctx, targets)?,
        dimension: ctx.source().dimension,
        destination: TeleportDestination::FacingLocation {
            pos: coordinates(ctx, "location")?.resolve(source.origin, source.yaw, source.pitch),
            target: coordinates(ctx, "facingLocation")?.resolve(
                source.origin,
                source.yaw,
                source.pitch,
            ),
        },
    })
}

fn location_facing_entity(
    ctx: &CommandContext<Source>,
    targets: bool,
    anchor_argument: bool,
) -> Result<Action, CommandError> {
    let source = ctx.source();
    Ok(Action::Teleport {
        targets: target_or_self(ctx, targets)?,
        dimension: ctx.source().dimension,
        destination: TeleportDestination::FacingEntity {
            pos: coordinates(ctx, "location")?.resolve(source.origin, source.yaw, source.pitch),
            entity: entity(ctx, "facingEntity")?,
            anchor: if anchor_argument {
                anchor(ctx, "facingAnchor")?
            } else {
                EntityAnchor::Feet
            },
        },
    })
}

fn entity_destination(ctx: &CommandContext<Source>, targets: bool) -> Result<Action, CommandError> {
    Ok(Action::Teleport {
        targets: target_or_self(ctx, targets)?,
        dimension: ctx.source().dimension,
        destination: TeleportDestination::Entity(entity(ctx, "destination")?),
    })
}

fn location_branch(targets: bool) -> ferrite_command::NodeBuilder<Source> {
    let builder = argument("location", ArgumentType::Vec3)
        .executes(move |ctx| send(ctx.source(), location(ctx, targets)?));
    if !targets {
        // Vanilla's self form is `/tp <location>`: rotation and facing only exist
        // under `<targets> <location>` (see commands.json).
        return builder;
    }
    builder
        .then(
            literal("rotation").then(
                argument("rotation", ArgumentType::Rotation)
                    .executes(|ctx| send(ctx.source(), location_rotation(ctx, true)?)),
            ),
        )
        .then(
            literal("facing")
                .then(
                    literal("entity").then(
                        argument("facingEntity", ArgumentType::entity())
                            .executes(|ctx| {
                                send(ctx.source(), location_facing_entity(ctx, true, false)?)
                            })
                            .then(
                                argument("facingAnchor", ArgumentType::EntityAnchor).executes(
                                    |ctx| {
                                        send(ctx.source(), location_facing_entity(ctx, true, true)?)
                                    },
                                ),
                            ),
                    ),
                )
                .then(
                    argument("facingLocation", ArgumentType::Vec3)
                        .executes(|ctx| send(ctx.source(), location_facing_location(ctx, true)?)),
                ),
        )
}

fn destination_branch(targets: bool) -> ferrite_command::NodeBuilder<Source> {
    argument("destination", ArgumentType::entity())
        .executes(move |ctx| send(ctx.source(), entity_destination(ctx, targets)?))
}

fn teleport_builder() -> ferrite_command::NodeBuilder<Source> {
    literal::<Source>("teleport")
        .requires(|source| source.permission >= 2)
        .restricted(true)
        .then(destination_branch(false))
        .then(location_branch(false))
        .then(
            argument("targets", ArgumentType::entities())
                .then(destination_branch(true))
                .then(location_branch(true)),
        )
}

fn message_builder() -> ferrite_command::NodeBuilder<Source> {
    literal::<Source>("msg").then(argument("targets", ArgumentType::players()).then(
        argument("message", ArgumentType::Message).executes(|ctx| {
            send(
                ctx.source(),
                Action::Message {
                    targets: entity(ctx, "targets")?,
                    text: message_arg(ctx, "message")?,
                },
            )
        }),
    ))
}

fn kill_builder() -> ferrite_command::NodeBuilder<Source> {
    literal::<Source>("kill")
        .requires(|source| source.permission >= 2)
        .restricted(true)
        .executes(|ctx| {
            send(
                ctx.source(),
                Action::Kill {
                    targets: self_target(),
                },
            )
        })
        .then(
            argument("targets", ArgumentType::entities()).executes(|ctx| {
                send(
                    ctx.source(),
                    Action::Kill {
                        targets: entity(ctx, "targets")?,
                    },
                )
            }),
        )
}

fn summon_builder() -> ferrite_command::NodeBuilder<Source> {
    literal::<Source>("summon")
        .requires(|source| source.permission >= 2)
        .restricted(true)
        .then(
            argument("entity", ArgumentType::ResourceLocation)
                .executes(|ctx| summon_action(ctx, false))
                .then(argument("pos", ArgumentType::Vec3).executes(|ctx| summon_action(ctx, true))),
        )
}

fn summon_action(ctx: &CommandContext<Source>, with_pos: bool) -> Result<i32, CommandError> {
    let name = ctx
        .string("entity")
        .ok_or_else(|| CommandError::failed("missing entity argument 'entity'"))?;
    let kind = MobKind::from_name(name).ok_or_else(|| {
        if ferrite_data::entity_props::by_name(name).is_some() {
            CommandError::failed(format!("Unable to summon entity {name}"))
        } else {
            CommandError::failed(format!("Invalid entity {name}"))
        }
    })?;
    let pos = if with_pos {
        coordinates(ctx, "pos")?.resolve(ctx.source().origin, ctx.source().yaw, ctx.source().pitch)
    } else {
        ctx.source().origin
    };
    send(ctx.source(), Action::Summon { kind, pos })
}

fn admin_targets_command(
    name: &'static str,
    action: fn(EntityTarget) -> Action,
) -> ferrite_command::NodeBuilder<Source> {
    literal::<Source>(name)
        .requires(|source| source.permission >= 3)
        .restricted(true)
        .then(
            argument("targets", ArgumentType::GameProfile)
                .executes(move |ctx| send(ctx.source(), action(entity(ctx, "targets")?))),
        )
}

fn whitelist_builder() -> ferrite_command::NodeBuilder<Source> {
    literal::<Source>("whitelist")
        .requires(|source| source.permission >= 3)
        .restricted(true)
        .then(literal("add").then(
            argument("targets", ArgumentType::GameProfile).executes(|ctx| {
                send(
                    ctx.source(),
                    Action::WhitelistAdd {
                        targets: entity(ctx, "targets")?,
                    },
                )
            }),
        ))
        .then(
            literal("remove").then(argument("targets", ArgumentType::GameProfile).executes(
                |ctx| {
                    send(
                        ctx.source(),
                        Action::WhitelistRemove {
                            targets: entity(ctx, "targets")?,
                        },
                    )
                },
            )),
        )
        .then(literal("list").executes(|ctx| send(ctx.source(), Action::WhitelistList)))
        .then(literal("on").executes(|ctx| send(ctx.source(), Action::WhitelistOn)))
        .then(literal("off").executes(|ctx| send(ctx.source(), Action::WhitelistOff)))
        .then(literal("reload").executes(|ctx| send(ctx.source(), Action::WhitelistReload)))
}

fn ban_builder() -> ferrite_command::NodeBuilder<Source> {
    literal::<Source>("ban")
        .requires(|source| source.permission >= 3)
        .restricted(true)
        .then(
            argument("targets", ArgumentType::GameProfile)
                .executes(|ctx| {
                    send(
                        ctx.source(),
                        Action::Ban {
                            targets: entity(ctx, "targets")?,
                            reason: "Banned by an operator.".into(),
                        },
                    )
                })
                .then(argument("reason", ArgumentType::Message).executes(|ctx| {
                    send(
                        ctx.source(),
                        Action::Ban {
                            targets: entity(ctx, "targets")?,
                            reason: message_arg(ctx, "reason")?,
                        },
                    )
                })),
        )
}

fn ban_ip_builder() -> ferrite_command::NodeBuilder<Source> {
    literal::<Source>("ban-ip")
        .requires(|source| source.permission >= 3)
        .restricted(true)
        .then(
            argument("target", ArgumentType::word())
                .executes(|ctx| {
                    send(
                        ctx.source(),
                        Action::BanIp {
                            target: message_arg(ctx, "target")?,
                            reason: "Banned by an operator.".into(),
                        },
                    )
                })
                .then(argument("reason", ArgumentType::Message).executes(|ctx| {
                    send(
                        ctx.source(),
                        Action::BanIp {
                            target: message_arg(ctx, "target")?,
                            reason: message_arg(ctx, "reason")?,
                        },
                    )
                })),
        )
}

fn pardon_ip_builder() -> ferrite_command::NodeBuilder<Source> {
    literal::<Source>("pardon-ip")
        .requires(|source| source.permission >= 3)
        .restricted(true)
        .then(argument("target", ArgumentType::word()).executes(|ctx| {
            send(
                ctx.source(),
                Action::PardonIp {
                    target: message_arg(ctx, "target")?,
                },
            )
        }))
}

fn banlist_builder() -> ferrite_command::NodeBuilder<Source> {
    literal::<Source>("banlist")
        .requires(|source| source.permission >= 3)
        .restricted(true)
        .executes(|ctx| send(ctx.source(), Action::BanList { ips: false }))
        .then(literal("ips").executes(|ctx| send(ctx.source(), Action::BanList { ips: true })))
        .then(literal("players").executes(|ctx| send(ctx.source(), Action::BanList { ips: false })))
}

fn kick_builder() -> ferrite_command::NodeBuilder<Source> {
    literal::<Source>("kick")
        .requires(|source| source.permission >= 3)
        .restricted(true)
        .then(
            argument("targets", ArgumentType::players())
                .executes(|ctx| {
                    send(
                        ctx.source(),
                        Action::Kick {
                            targets: entity(ctx, "targets")?,
                            reason: "Kicked by an operator".into(),
                        },
                    )
                })
                .then(argument("reason", ArgumentType::Message).executes(|ctx| {
                    send(
                        ctx.source(),
                        Action::Kick {
                            targets: entity(ctx, "targets")?,
                            reason: message_arg(ctx, "reason")?,
                        },
                    )
                })),
        )
}

fn save_all_builder() -> ferrite_command::NodeBuilder<Source> {
    literal::<Source>("save-all")
        .requires(|source| source.permission >= 4)
        .restricted(true)
        .executes(|ctx| send(ctx.source(), Action::SaveAll { flush: false }))
        .then(literal("flush").executes(|ctx| send(ctx.source(), Action::SaveAll { flush: true })))
}

/// The full server command graph.
fn emit(source: &Source, action: Action) -> Result<(), CommandError> {
    send(source, action).map(|_| ())
}

fn syntax_error(key: &'static str, args: Vec<String>, message: impl Into<String>) -> CommandError {
    CommandError::Syntax(CommandSyntaxError::new(key, args, message.into()))
}

pub(crate) fn player_only() -> CommandError {
    syntax_error(
        "permissions.requires.player",
        Vec::new(),
        "A player is required to run this command here",
    )
}

pub(crate) fn player_not_found() -> CommandError {
    syntax_error(
        "argument.entity.notfound.player",
        Vec::new(),
        "No player was found",
    )
}

fn game_mode_id(mode: GameMode) -> i32 {
    match mode {
        GameMode::Survival => 0,
        GameMode::Creative => 1,
        GameMode::Adventure => 2,
        GameMode::Spectator => 3,
    }
}

fn game_mode_display(mode: GameMode) -> &'static str {
    match mode {
        GameMode::Survival => "Survival Mode",
        GameMode::Creative => "Creative Mode",
        GameMode::Adventure => "Adventure Mode",
        GameMode::Spectator => "Spectator Mode",
    }
}

fn difficulty_display(value: i32) -> &'static str {
    match value {
        0 => "Peaceful",
        1 => "Easy",
        2 => "Normal",
        3 => "Hard",
        _ => "Normal",
    }
}

pub(crate) fn resolve_players(
    players: &[PlayerEntry],
    target: &EntityTarget,
    source: Option<Uuid>,
) -> Result<Vec<Uuid>, CommandError> {
    let selected: Vec<&PlayerEntry> = match target {
        EntityTarget::Name(name) => players
            .iter()
            .filter(|player| player.name.eq_ignore_ascii_case(name))
            .collect(),
        EntityTarget::Uuid(id) => {
            let Ok(id) = Uuid::parse_str(id) else {
                return Err(player_not_found());
            };
            players.iter().filter(|player| player.uuid == id).collect()
        }
        EntityTarget::Selector { kind, options } => {
            let mut selected: Vec<&PlayerEntry> = match kind {
                SelectorKind::Myself => source
                    .and_then(|uuid| players.iter().find(|player| player.uuid == uuid))
                    .into_iter()
                    .collect(),
                SelectorKind::NearestPlayer | SelectorKind::NearestEntity => {
                    players.iter().take(1).collect()
                }
                SelectorKind::RandomPlayer => players.iter().take(1).collect(),
                SelectorKind::AllPlayers | SelectorKind::AllEntities => players.iter().collect(),
            };
            for option in options {
                match option.key.as_str() {
                    "name" => selected.retain(|player| player.name == option.value),
                    "limit" => {
                        if let Ok(limit) = option.value.parse::<usize>() {
                            selected.truncate(limit.max(1));
                        }
                    }
                    "gamemode" => {
                        if let Some(mode) = GameMode::from_name(&option.value) {
                            selected.retain(|player| player.game_mode == game_mode_id(mode));
                        }
                    }
                    _ => {}
                }
            }
            selected
        }
    };
    if selected.is_empty() {
        return Err(player_not_found());
    }
    Ok(selected.into_iter().map(|player| player.uuid).collect())
}

fn gamemode(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let mode = ctx
        .argument("gamemode")
        .and_then(|value| value.as_gamemode())
        .ok_or_else(|| CommandError::failed("missing gamemode"))?;
    let target = if ctx.argument("target").is_some() {
        Some(entity(ctx, "target")?)
    } else {
        None
    };
    let source = ctx.source();
    let mode_id = game_mode_id(mode);
    let mut state = source.state.lock().unwrap();
    let targets = match target {
        Some(target) => resolve_players(&state.players, &target, source.uuid)?,
        None => vec![source.uuid.ok_or_else(player_only)?],
    };
    let mut changed = Vec::new();
    let mut feedback = Vec::new();
    for uuid in targets {
        let Some(index) = state.players.iter().position(|player| player.uuid == uuid) else {
            continue;
        };
        if state.players[index].game_mode == mode_id {
            continue;
        }
        state.players[index].game_mode = mode_id;
        changed.push(uuid);
        if Some(uuid) == source.uuid {
            feedback.push(Action::Feedback(Feedback::new(
                "commands.gamemode.success.self",
                vec![game_mode_display(mode).into()],
            )));
        } else {
            feedback.push(Action::PlayerFeedback {
                uuid,
                feedback: Feedback::new("gameMode.changed", vec![game_mode_display(mode).into()]),
            });
            feedback.push(Action::Feedback(Feedback::new(
                "commands.gamemode.success.other",
                vec![
                    state.players[index].name.clone(),
                    game_mode_display(mode).into(),
                ],
            )));
        }
    }
    drop(state);
    if changed.is_empty() {
        return Ok(0);
    }
    emit(
        source,
        Action::GameMode {
            mode: mode_id,
            targets: changed.clone(),
        },
    )?;
    for action in feedback {
        emit(source, action)?;
    }
    Ok(changed.len() as i32)
}

fn default_gamemode(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let mode = ctx
        .argument("gamemode")
        .and_then(|value| value.as_gamemode())
        .ok_or_else(|| CommandError::failed("missing gamemode"))?;
    let source = ctx.source();
    source.state.lock().unwrap().default_gamemode = game_mode_id(mode);
    emit(source, Action::DefaultGameMode(game_mode_id(mode)))?;
    emit(
        source,
        Action::Feedback(Feedback::new(
            "commands.defaultgamemode.success",
            vec![game_mode_display(mode).into()],
        )),
    )?;
    Ok(1)
}

fn difficulty_query(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let source = ctx.source();
    let value = source.state.lock().unwrap().difficulty;
    emit(
        source,
        Action::Feedback(Feedback::new(
            "commands.difficulty.query",
            vec![difficulty_display(value).into()],
        )),
    )?;
    Ok(value)
}

fn set_difficulty(ctx: &CommandContext<Source>, value: i32) -> Result<i32, CommandError> {
    let source = ctx.source();
    {
        let mut state = source.state.lock().unwrap();
        if state.difficulty == value {
            let display = difficulty_display(value);
            return Err(syntax_error(
                "commands.difficulty.failure",
                vec![display.into()],
                format!("The difficulty did not change; it is already set to {display}"),
            ));
        }
        state.difficulty = value;
    }
    emit(source, Action::Difficulty(value))?;
    emit(
        source,
        Action::Feedback(Feedback::new(
            "commands.difficulty.success",
            vec![difficulty_display(value).into()],
        )),
    )?;
    Ok(0)
}

fn time_set(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let ticks = ctx
        .integer("time")
        .ok_or_else(|| CommandError::failed("missing time"))?;
    let source = ctx.source();
    {
        let mut state = source.state.lock().unwrap();
        if state.clock_time == i64::from(ticks) {
            return Err(syntax_error(
                "commands.time.set.already_at_time",
                vec![OVERWORLD_CLOCK.into(), ticks.to_string()],
                format!("Clock {OVERWORLD_CLOCK} is already set to {ticks} tick(s)"),
            ));
        }
        state.clock_time = i64::from(ticks);
    }
    emit(source, Action::TimeSet(ticks))?;
    emit(
        source,
        Action::Feedback(Feedback::new(
            "commands.time.set.absolute",
            vec![OVERWORLD_CLOCK.into(), ticks.to_string()],
        )),
    )?;
    Ok(ticks)
}

fn time_add(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let delta = ctx
        .integer("time")
        .ok_or_else(|| CommandError::failed("missing time"))?;
    let source = ctx.source();
    let total = {
        let mut state = source.state.lock().unwrap();
        state.clock_time = state.clock_time.saturating_add(i64::from(delta));
        state.clock_time
    };
    emit(source, Action::TimeAdd(delta))?;
    emit(
        source,
        Action::Feedback(Feedback::new(
            "commands.time.set.absolute",
            vec![OVERWORLD_CLOCK.into(), total.to_string()],
        )),
    )?;
    Ok(total as i32)
}

fn time_query(ctx: &CommandContext<Source>, game_time: bool) -> Result<i32, CommandError> {
    let source = ctx.source();
    let ticks = {
        let state = source.state.lock().unwrap();
        if game_time {
            state.game_time
        } else {
            state.clock_time
        }
    };
    let feedback = if game_time {
        Feedback::new("commands.time.query.gametime", vec![ticks.to_string()])
    } else {
        Feedback::new(
            "commands.time.query.absolute",
            vec![OVERWORLD_CLOCK.into(), ticks.to_string()],
        )
    };
    emit(source, Action::Feedback(feedback))?;
    Ok(ticks as i32)
}

fn set_weather(ctx: &CommandContext<Source>, kind: Weather) -> Result<i32, CommandError> {
    let duration = ctx.integer("duration");
    let source = ctx.source();
    {
        let mut state = source.state.lock().unwrap();
        state.weather = kind;
        if let Some(duration) = duration {
            state.weather_duration = duration;
        }
    }
    emit(source, Action::Weather { kind, duration })?;
    emit(
        source,
        Action::Feedback(Feedback::new(kind.feedback_key(), Vec::new())),
    )?;
    Ok(duration.unwrap_or(0))
}

fn seed(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let source = ctx.source();
    let seed = source.state.lock().unwrap().seed;
    emit(
        source,
        Action::Feedback(Feedback::new(
            "commands.seed.success",
            vec![seed.to_string()],
        )),
    )?;
    Ok(seed as i32)
}

fn list_players(ctx: &CommandContext<Source>, uuids: bool) -> Result<i32, CommandError> {
    let source = ctx.source();
    let (count, max_players, list) = {
        let state = source.state.lock().unwrap();
        let names: Vec<String> = state
            .players
            .iter()
            .map(|player| {
                if uuids {
                    format!("{} ({})", player.name, player.uuid)
                } else {
                    player.name.clone()
                }
            })
            .collect();
        (state.players.len(), state.max_players, names.join(", "))
    };
    emit(
        source,
        Action::Feedback(Feedback::new(
            "commands.list.players",
            vec![count.to_string(), max_players.to_string(), list],
        )),
    )?;
    Ok(count as i32)
}

fn say(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let message = ctx.string("message").unwrap_or_default().to_owned();
    let sender = ctx.source().sender_name();
    emit(
        ctx.source(),
        Action::Broadcast(Feedback::new(
            "chat.type.announcement",
            vec![sender, message],
        )),
    )?;
    Ok(1)
}

fn me(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let action = ctx.string("action").unwrap_or_default().to_owned();
    let sender = ctx.source().sender_name();
    emit(
        ctx.source(),
        Action::Broadcast(Feedback::new("chat.type.emote", vec![sender, action])),
    )?;
    Ok(1)
}

/// `gamerule <rule> [value]` for one rule, using the type and bounds from the 26.3
/// generated command tree.
fn gamerule_node(
    label: impl Into<String>,
    spec: &'static ferrite_command::gamerule::RuleSpec,
) -> ferrite_command::NodeBuilder<Source> {
    use ferrite_command::gamerule::RuleKind;
    let query_name = spec.name.to_owned();
    let set_name = spec.name.to_owned();
    let kind = spec.kind;
    let value = match kind {
        RuleKind::Boolean => ArgumentType::bool(),
        RuleKind::Integer { min, max } => ArgumentType::integer_range(min, max),
    };
    literal::<Source>(label)
        .executes(move |ctx| send(ctx.source(), Action::GameRule(query_name.clone(), None)))
        .then(argument::<Source>("value", value).executes(move |ctx| {
            let value = match kind {
                RuleKind::Boolean => ctx
                    .argument("value")
                    .and_then(|value| value.as_bool())
                    .map(|value| value.to_string()),
                RuleKind::Integer { .. } => ctx.integer("value").map(|value| value.to_string()),
            }
            .ok_or_else(|| CommandError::failed("invalid game rule value"))?;
            send(
                ctx.source(),
                Action::GameRule(set_name.clone(), Some(value)),
            )
        }))
}

/// `random_tick_speed` -> `randomTickSpeed`, the pre-26.3 spelling players know.
fn camel_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper = false;
    for ch in name.chars() {
        if ch == '_' {
            upper = true;
        } else if upper {
            out.push(ch.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Resolves an item argument (`stone`, `minecraft:stone`) to its protocol id.
/// Component patches are not supported yet; the bare id is what `/give` uses.
pub fn parse_item(raw: &str) -> Result<u16, CommandError> {
    let base = raw.split(['[', '{']).next().unwrap_or(raw).trim();
    ferrite_item::item_id(base)
        .ok_or_else(|| CommandError::failed(format!("Unknown item '{base}'")))
}

/// Parses the `[component={...}]` suffix of a `/give` item argument into raw component NBT.
fn parse_item_components(raw: &str) -> Result<Vec<(u16, ferrite_nbt::Tag)>, CommandError> {
    let Some(start) = raw.find('[') else {
        return Ok(Vec::new());
    };
    let end = raw
        .rfind(']')
        .filter(|end| *end > start)
        .ok_or_else(|| CommandError::failed("Unclosed item component list"))?;
    let inner = &raw[start + 1..end];
    if inner.trim().is_empty() {
        return Ok(Vec::new());
    }
    // Vanilla spells the list `item[a=1,b=2]`; SNBT needs braces and `:` separators.
    let text = brigadier_to_snbt(inner);
    let tag = ferrite_nbt::snbt::parse(&text)
        .map_err(|error| CommandError::failed(format!("Invalid item components: {error}")))?;
    let ferrite_nbt::Tag::Compound(compound) = tag else {
        return Err(CommandError::failed("Invalid item components"));
    };
    let mut components = Vec::new();
    for (name, value) in &compound {
        let id = ferrite_item::component_id(name)
            .ok_or_else(|| CommandError::failed(format!("Unknown item component '{name}'")))?;
        components.push((id, value.clone()));
    }
    Ok(components)
}

/// Rewrites a `/give` component list (`a=1,b={c:"x"}`) as an SNBT compound.
fn brigadier_to_snbt(inner: &str) -> String {
    let mut out = String::with_capacity(inner.len() + 2);
    out.push('{');
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for c in inner.chars() {
        if quoted {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                quoted = false;
            }
            continue;
        }
        match c {
            '"' => {
                quoted = true;
                out.push(c);
            }
            '[' | '{' => {
                depth += 1;
                out.push(c);
            }
            ']' | '}' => {
                depth = depth.saturating_sub(1);
                out.push(c);
            }
            '=' if depth == 0 => out.push(':'),
            _ => out.push(c),
        }
    }
    out.push('}');
    out
}

/// Vanilla's item display name for a stack, e.g. `diamond_sword` -> "Diamond Sword".
pub fn item_display(id: u16) -> String {
    let name = ferrite_item::item_name(id).unwrap_or("minecraft:air");
    let path = name.strip_prefix("minecraft:").unwrap_or(name);
    path.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Resolves a player target against the online players in the command state.
pub(crate) fn resolved_targets(
    ctx: &CommandContext<Source>,
    name: &str,
) -> Result<Vec<Uuid>, CommandError> {
    let target = entity(ctx, name)?;
    let state = ctx.source().state.lock().unwrap();
    resolve_players(&state.players, &target, ctx.source().uuid)
}

fn clear_action(
    ctx: &CommandContext<Source>,
    item: Option<&str>,
    max_count: Option<i32>,
) -> Result<i32, CommandError> {
    let targets = if ctx.argument("targets").is_some() {
        resolved_targets(ctx, "targets")?
    } else {
        vec![ctx.source().uuid.ok_or_else(player_only)?]
    };
    let item = item.map(parse_item).transpose()?;
    send(
        ctx.source(),
        Action::Clear {
            targets,
            item,
            max_count,
        },
    )
}

fn give_action(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let raw = ctx
        .string("item")
        .ok_or_else(|| CommandError::failed("Missing item"))?;
    let item = parse_item(raw)?;
    let components = parse_item_components(raw)?;
    let count = ctx.integer("count").unwrap_or(1);
    let max = ferrite_item::item_defaults(item).map_or(64, |defaults| defaults.max_stack_size);
    if count > max {
        return Err(CommandError::failed(format!(
            "Can't give more than {max} of {}",
            item_display(item)
        )));
    }
    send(
        ctx.source(),
        Action::Give {
            targets: resolved_targets(ctx, "targets")?,
            item,
            count,
            item_name: item_display(item),
            components,
        },
    )
}

fn experience_action(
    ctx: &CommandContext<Source>,
    operation: XpOperation,
    unit: XpUnit,
) -> Result<i32, CommandError> {
    send(
        ctx.source(),
        Action::Experience {
            operation,
            targets: resolved_targets(ctx, "target")?,
            amount: ctx.integer("amount").unwrap_or(0),
            unit,
        },
    )
}

pub fn dispatcher() -> CommandDispatcher<Source> {
    let mut tree = CommandDispatcher::new();
    tree.set_selector_requirement(|source: &Source| source.permission >= SELECTOR_PERMISSION);

    // ---- teleport ----
    let teleport = tree.register(teleport_builder());
    tree.register(
        literal::<Source>("tp")
            .requires(|source| source.permission >= 2)
            .restricted(true)
            .redirect(teleport),
    );

    // ---- messaging ----
    let msg = tree.register(message_builder());
    tree.register(literal::<Source>("tell").redirect(msg));
    tree.register(literal::<Source>("w").redirect(msg));

    // ---- entity effects ----
    tree.register(kill_builder());
    tree.register(summon_builder());
    tree.register(summon_builder());

    // ---- administration ----
    tree.register(admin_targets_command("op", |targets| Action::Op {
        targets,
    }));
    tree.register(admin_targets_command("deop", |targets| Action::Deop {
        targets,
    }));
    tree.register(whitelist_builder());
    tree.register(ban_builder());
    tree.register(ban_ip_builder());
    tree.register(admin_targets_command("pardon", |targets| Action::Pardon {
        targets,
    }));
    tree.register(pardon_ip_builder());
    tree.register(banlist_builder());
    tree.register(kick_builder());

    // ---- server control ----
    tree.register(
        literal::<Source>("stop")
            .requires(|source| source.permission >= 4)
            .restricted(true)
            .executes(|ctx| send(ctx.source(), Action::Stop)),
    );
    tree.register(save_all_builder());
    tree.register(
        literal::<Source>("save-on")
            .requires(|source| source.permission >= 4)
            .restricted(true)
            .executes(|ctx| send(ctx.source(), Action::SaveOn)),
    );
    tree.register(
        literal::<Source>("save-off")
            .requires(|source| source.permission >= 4)
            .restricted(true)
            .executes(|ctx| send(ctx.source(), Action::SaveOff)),
    );

    // ---- gamerule: one literal per rule in the generated 26.3 command tree ----
    let mut gamerule = literal::<Source>("gamerule")
        .requires(|source| source.permission >= 2)
        .restricted(true);
    for spec in ferrite_command::gamerule::specs() {
        gamerule = gamerule.then(gamerule_node(spec.name, spec));
        gamerule = gamerule.then(gamerule_node(format!("minecraft:{}", spec.name), spec));
        gamerule = gamerule.then(gamerule_node(camel_case(spec.name), spec));
    }
    for (alias, canonical) in ferrite_command::gamerule::aliases() {
        if let Some(spec) = ferrite_command::gamerule::spec(canonical) {
            gamerule = gamerule.then(gamerule_node(*alias, spec));
        }
    }
    tree.register(gamerule);

    // ---- inventory and experience ----
    tree.register(
        literal::<Source>("clear")
            .requires(|source| source.permission >= 2)
            .executes(|ctx| clear_action(ctx, None, None))
            .then(
                argument("targets", ArgumentType::players())
                    .executes(|ctx| clear_action(ctx, None, None))
                    .then(
                        argument("item", ArgumentType::opaque("minecraft:item_predicate"))
                            .executes(|ctx| clear_action(ctx, ctx.string("item"), None))
                            .then(
                                argument("maxCount", ArgumentType::integer_range(0, i32::MAX))
                                    .executes(|ctx| {
                                        clear_action(
                                            ctx,
                                            ctx.string("item"),
                                            ctx.integer("maxCount"),
                                        )
                                    }),
                            ),
                    ),
            ),
    );
    tree.register(
        literal::<Source>("give")
            .requires(|source| source.permission >= 2)
            .then(
                argument("targets", ArgumentType::players()).then(
                    argument("item", ArgumentType::opaque("minecraft:item_stack"))
                        .executes(give_action)
                        .then(
                            argument("count", ArgumentType::integer_range(1, i32::MAX))
                                .executes(give_action),
                        ),
                ),
            ),
    );
    let experience = tree.register(
        literal::<Source>("experience")
            .requires(|source| source.permission >= 2)
            .then(
                literal("add").then(
                    argument("target", ArgumentType::players()).then(
                        argument("amount", ArgumentType::integer())
                            .executes(|ctx| {
                                experience_action(ctx, XpOperation::Add, XpUnit::Points)
                            })
                            .then(literal("points").executes(|ctx| {
                                experience_action(ctx, XpOperation::Add, XpUnit::Points)
                            }))
                            .then(literal("levels").executes(|ctx| {
                                experience_action(ctx, XpOperation::Add, XpUnit::Levels)
                            })),
                    ),
                ),
            )
            .then(
                literal("set").then(
                    argument("target", ArgumentType::players()).then(
                        argument("amount", ArgumentType::integer_range(0, i32::MAX))
                            .executes(|ctx| {
                                experience_action(ctx, XpOperation::Set, XpUnit::Points)
                            })
                            .then(literal("points").executes(|ctx| {
                                experience_action(ctx, XpOperation::Set, XpUnit::Points)
                            }))
                            .then(literal("levels").executes(|ctx| {
                                experience_action(ctx, XpOperation::Set, XpUnit::Levels)
                            })),
                    ),
                ),
            )
            .then(
                literal("query").then(
                    argument("target", ArgumentType::player())
                        .then(literal("points").executes(|ctx| {
                            experience_action(ctx, XpOperation::Query, XpUnit::Points)
                        }))
                        .then(literal("levels").executes(|ctx| {
                            experience_action(ctx, XpOperation::Query, XpUnit::Levels)
                        })),
                ),
            ),
    );
    // Vanilla registers the tree as `experience` and aliases it as `xp`.
    tree.register(
        literal::<Source>("xp")
            .requires(|source| source.permission >= 2)
            .redirect(experience),
    );

    // ---- gamemode (owned by the gamemode worker) ----
    tree.register(
        literal::<Source>("gamemode")
            .requires(|source| source.permission >= 2)
            .then(
                argument("gamemode", ArgumentType::Gamemode)
                    .executes(gamemode)
                    .then(argument("target", ArgumentType::players()).executes(gamemode)),
            ),
    );
    tree.register(
        literal::<Source>("defaultgamemode")
            .requires(|source| source.permission >= 2)
            .then(argument("gamemode", ArgumentType::Gamemode).executes(default_gamemode)),
    );
    tree.register(
        literal::<Source>("difficulty")
            .requires(|source| source.permission >= 2)
            .executes(difficulty_query)
            .then(literal("peaceful").executes(|ctx| set_difficulty(ctx, 0)))
            .then(literal("easy").executes(|ctx| set_difficulty(ctx, 1)))
            .then(literal("normal").executes(|ctx| set_difficulty(ctx, 2)))
            .then(literal("hard").executes(|ctx| set_difficulty(ctx, 3))),
    );
    tree.register(
        literal::<Source>("time")
            .requires(|source| source.permission >= 2)
            .then(
                literal("set")
                    .then(argument("time", ArgumentType::Time { min: 0 }).executes(time_set)),
            )
            .then(
                literal("add").then(
                    argument("time", ArgumentType::Time { min: i32::MIN }).executes(time_add),
                ),
            )
            .then(
                literal("query")
                    .then(literal("time").executes(|ctx| time_query(ctx, false)))
                    .then(literal("gametime").executes(|ctx| time_query(ctx, true))),
            ),
    );
    tree.register(
        literal::<Source>("weather")
            .requires(|source| source.permission >= 2)
            .then(
                literal("clear")
                    .executes(|ctx| set_weather(ctx, Weather::Clear))
                    .then(
                        argument("duration", ArgumentType::Time { min: 1 })
                            .executes(|ctx| set_weather(ctx, Weather::Clear)),
                    ),
            )
            .then(
                literal("rain")
                    .executes(|ctx| set_weather(ctx, Weather::Rain))
                    .then(
                        argument("duration", ArgumentType::Time { min: 1 })
                            .executes(|ctx| set_weather(ctx, Weather::Rain)),
                    ),
            )
            .then(
                literal("thunder")
                    .executes(|ctx| set_weather(ctx, Weather::Thunder))
                    .then(
                        argument("duration", ArgumentType::Time { min: 1 })
                            .executes(|ctx| set_weather(ctx, Weather::Thunder)),
                    ),
            ),
    );
    tree.register(
        literal::<Source>("seed")
            .requires(|source| source.permission >= 2)
            .executes(seed),
    );
    tree.register(
        literal::<Source>("list")
            .executes(|ctx| list_players(ctx, false))
            .then(literal("uuids").executes(|ctx| list_players(ctx, true))),
    );
    tree.register(
        literal::<Source>("say")
            .requires(|source| source.permission >= 2)
            .then(argument("message", ArgumentType::Message).executes(say)),
    );
    tree.register(
        literal::<Source>("me").then(argument("action", ArgumentType::Message).executes(me)),
    );
    tree.register(literal::<Source>("tps").executes(|ctx| send(ctx.source(), Action::Tps)));
    tree.register(literal::<Source>("mspt").executes(|ctx| send(ctx.source(), Action::Mspt)));
    tree.register(
        literal::<Source>("profile")
            .requires(|s| s.permission >= 2)
            .then(
                literal::<Source>("start").executes(|ctx| send(ctx.source(), Action::ProfileStart)),
            )
            .then(
                literal::<Source>("stop").executes(|ctx| send(ctx.source(), Action::ProfileStop)),
            ),
    );
    tree.register(
        literal::<Source>("stop")
            .requires(|source| source.permission >= 4 && source.uuid.is_none())
            .executes(|ctx| send(ctx.source(), Action::Stop)),
    );
    super::commands_world::register(&mut tree);
    super::commands_extras::register(&mut tree);
    super::commands_server::register(&mut tree);
    super::commands_meta::register(&mut tree);
    super::commands_data::register(&mut tree);
    super::functions::register(&mut tree);
    // `execute` registers last: its `run` and every modifier redirect back to the root, so
    // the rest of the tree must already be in place.
    super::commands_exec::register(&mut tree);
    tree
}

/// Resolves players from an entity argument against the online players.
pub fn resolve_online(
    target: &EntityTarget,
    online: &HashMap<Uuid, OnlinePlayer>,
    source: Option<Uuid>,
    origin: [f64; 3],
) -> Result<Vec<ResolvedPlayer>, &'static str> {
    let mut players: Vec<ResolvedPlayer> = match target {
        EntityTarget::Name(name) => online
            .iter()
            .find(|(_, player)| player.name.eq_ignore_ascii_case(name))
            .map(|(uuid, player)| ResolvedPlayer {
                uuid: *uuid,
                name: player.name.clone(),
            })
            .into_iter()
            .collect(),
        EntityTarget::Uuid(raw) => Uuid::parse_str(raw)
            .ok()
            .and_then(|uuid| {
                online.get(&uuid).map(|player| ResolvedPlayer {
                    uuid,
                    name: player.name.clone(),
                })
            })
            .into_iter()
            .collect(),
        EntityTarget::Selector { kind, .. } => match kind {
            SelectorKind::Myself => source
                .and_then(|uuid| {
                    online.get(&uuid).map(|player| ResolvedPlayer {
                        uuid,
                        name: player.name.clone(),
                    })
                })
                .into_iter()
                .collect(),
            SelectorKind::AllPlayers | SelectorKind::AllEntities => online
                .iter()
                .map(|(uuid, player)| ResolvedPlayer {
                    uuid: *uuid,
                    name: player.name.clone(),
                })
                .collect(),
            SelectorKind::RandomPlayer => {
                let mut all: Vec<_> = online
                    .iter()
                    .map(|(uuid, player)| ResolvedPlayer {
                        uuid: *uuid,
                        name: player.name.clone(),
                    })
                    .collect();
                all.sort_by(|a, b| a.name.cmp(&b.name));
                if all.is_empty() {
                    Vec::new()
                } else {
                    let index = (Uuid::new_v4().as_u128() % all.len() as u128) as usize;
                    vec![all.swap_remove(index)]
                }
            }
            SelectorKind::NearestPlayer | SelectorKind::NearestEntity => online
                .iter()
                .min_by(|(_, a), (_, b)| {
                    let distance = |player: &OnlinePlayer| {
                        player
                            .pos
                            .iter()
                            .zip(origin)
                            .map(|(a, b)| (a - b) * (a - b))
                            .sum::<f64>()
                    };
                    distance(a).total_cmp(&distance(b))
                })
                .map(|(uuid, player)| ResolvedPlayer {
                    uuid: *uuid,
                    name: player.name.clone(),
                })
                .into_iter()
                .collect(),
        },
    };
    players.sort_by(|a, b| a.name.cmp(&b.name));
    players.dedup_by_key(|player| player.uuid);

    let empty_key = if matches!(
        target,
        EntityTarget::Selector {
            kind: SelectorKind::Myself,
            ..
        }
    ) {
        "argument.entity.notfound.entity"
    } else {
        "argument.entity.notfound.player"
    };
    if players.is_empty() {
        return Err(empty_key);
    }
    apply_options(target, players, online, origin)
}

fn apply_options(
    target: &EntityTarget,
    players: Vec<ResolvedPlayer>,
    online: &HashMap<Uuid, OnlinePlayer>,
    origin: [f64; 3],
) -> Result<Vec<ResolvedPlayer>, &'static str> {
    let EntityTarget::Selector { options, .. } = target else {
        return Ok(players);
    };
    let mut players = players;
    for option in options {
        match option.key.as_str() {
            "name" => {
                let expected = option.value.trim_matches('"');
                players.retain(|player| player.name == expected);
            }
            "distance" => {
                let range = option.value.trim();
                let (min, max) = range.split_once("..").map_or_else(
                    || match range.parse::<f64>() {
                        Ok(value) => (value, value),
                        Err(_) => (0.0, f64::INFINITY),
                    },
                    |(min, max)| {
                        (
                            min.parse().unwrap_or(0.0),
                            max.parse().unwrap_or(f64::INFINITY),
                        )
                    },
                );
                let distance = |player: &ResolvedPlayer| {
                    online.get(&player.uuid).map_or(0.0, |p| {
                        p.pos
                            .iter()
                            .zip(origin)
                            .map(|(a, b)| (a - b) * (a - b))
                            .sum::<f64>()
                            .sqrt()
                    })
                };
                players.retain(|player| {
                    let d = distance(player);
                    d >= min && d <= max
                });
            }
            "limit" => {
                if let Ok(limit) = option.value.parse::<usize>() {
                    players.truncate(limit);
                }
            }
            "sort" => match option.value.as_str() {
                "nearest" => players.sort_by(|a, b| {
                    let distance = |player: &ResolvedPlayer| {
                        online.get(&player.uuid).map_or(0.0, |p| {
                            p.pos
                                .iter()
                                .zip(origin)
                                .map(|(x, y)| (x - y) * (x - y))
                                .sum::<f64>()
                        })
                    };
                    distance(a).total_cmp(&distance(b))
                }),
                "furthest" => players.sort_by(|a, b| {
                    let distance = |player: &ResolvedPlayer| {
                        online.get(&player.uuid).map_or(0.0, |p| {
                            p.pos
                                .iter()
                                .zip(origin)
                                .map(|(x, y)| (x - y) * (x - y))
                                .sum::<f64>()
                        })
                    };
                    distance(b).total_cmp(&distance(a))
                }),
                "random" => {
                    let length = players.len();
                    if length > 0 {
                        let index = (Uuid::new_v4().as_u128() % length as u128) as usize;
                        players.swap(0, index);
                    }
                }
                _ => return Err("argument.entity.options.sort.irreversible"),
            },
            _ => {}
        }
    }
    Ok(players)
}

/// Resolves game profiles for admin commands: online players first, then the profile cache.
///
/// Names that are neither online nor cached are rejected, matching a server that has never
/// seen the profile.
pub fn resolve_profiles(
    target: &EntityTarget,
    online: &HashMap<Uuid, OnlinePlayer>,
    admin: Option<&AdminLists>,
    source: Option<Uuid>,
    origin: [f64; 3],
) -> Result<Vec<ResolvedPlayer>, &'static str> {
    let online_players = resolve_online(target, online, source, origin).unwrap_or_default();
    if !online_players.is_empty() || matches!(target, EntityTarget::Selector { .. }) {
        if online_players.is_empty() {
            return Err("argument.player.unknown");
        }
        return Ok(online_players);
    }
    let (raw, uuid) = match target {
        EntityTarget::Name(raw) => (raw.as_str(), Uuid::parse_str(raw).ok()),
        EntityTarget::Uuid(raw) => (raw.as_str(), Uuid::parse_str(raw).ok()),
        EntityTarget::Selector { .. } => unreachable!(),
    };
    if let Some(uuid) = uuid {
        let name = online
            .get(&uuid)
            .map(|player| player.name.clone())
            .or_else(|| admin.and_then(|admin| admin.usercache.name_for(uuid)))
            .unwrap_or_else(|| uuid.to_string());
        return Ok(vec![ResolvedPlayer { uuid, name }]);
    }
    let cached = admin.and_then(|admin| {
        admin
            .usercache
            .uuid_for(raw)
            .map(|uuid| (uuid, admin.usercache.name_for(uuid)))
    });
    Ok(match cached {
        Some((uuid, cached_name)) => vec![ResolvedPlayer {
            uuid,
            name: cached_name.unwrap_or_else(|| raw.to_owned()),
        }],
        // Vanilla's profile cache creates an offline profile for names it has never seen.
        None => vec![ResolvedPlayer {
            uuid: offline_uuid(raw),
            name: raw.to_owned(),
        }],
    })
}

/// Vanilla's offline-mode UUID: a version-3 UUID of `OfflinePlayer:<name>`.
pub(crate) fn offline_uuid(name: &str) -> Uuid {
    let mut bytes = md5::compute(format!("OfflinePlayer:{name}")).0;
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

/// Whether a string is a literal IP address.
pub fn parse_ip(target: &str) -> Option<IpAddr> {
    IpAddr::from_str(target).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(permission: u8) -> Source {
        let (actions, _) = tokio::sync::mpsc::unbounded_channel();
        Source {
            permission,
            origin: [0.0, 64.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            uuid: None,
            name: None,
            state: Arc::new(Mutex::new(GameState::default())),
            actions,
            anchor: ferrite_command::EntityAnchor::Feet,
            dimension: crate::game::Dimension::Overworld,
            world: None,
            tree: None,
            function_depth: 0,
        }
    }

    fn run(input: &str, permission: u8) -> (Result<i32, CommandError>, Vec<Action>) {
        let tree = Arc::new(dispatcher());
        let (actions, mut received) = tokio::sync::mpsc::unbounded_channel();
        let mut source = source(permission);
        source.actions = actions;
        source.tree = Some(tree.clone());
        let result = tree.execute(input, source);
        (
            result,
            std::iter::from_fn(|| received.try_recv().ok()).collect(),
        )
    }

    fn only(input: &str, permission: u8) -> Action {
        let (result, actions) = run(input, permission);
        assert!(result.is_ok(), "{input}: {result:?}");
        assert_eq!(actions.len(), 1, "{input}: {actions:?}");
        actions.into_iter().next().unwrap()
    }

    fn denied(input: &str) {
        let (result, actions) = run(input, 0);
        assert!(result.is_err(), "{input} should be denied");
        assert!(actions.is_empty(), "{input} produced {actions:?}");
    }

    fn run_with_state(
        input: &str,
        permission: u8,
        state: Arc<Mutex<GameState>>,
        uuid: Option<Uuid>,
        name: Option<&str>,
    ) -> (Result<i32, CommandError>, Vec<Action>) {
        let tree = Arc::new(dispatcher());
        let (actions, mut received) = tokio::sync::mpsc::unbounded_channel();
        let mut source = source(permission);
        source.actions = actions;
        source.state = state;
        source.uuid = uuid;
        source.name = name.map(str::to_owned);
        source.tree = Some(tree.clone());
        let result = tree.execute(input, source);
        (
            result,
            std::iter::from_fn(|| received.try_recv().ok()).collect(),
        )
    }

    fn feedback_key(actions: &[Action]) -> Option<&'static str> {
        actions.iter().find_map(|action| match action {
            Action::Feedback(feedback) => Some(feedback.key),
            _ => None,
        })
    }

    fn function_world() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ferrite-command-test-{}", Uuid::new_v4()));
        let functions = dir.join("datapacks/test/data/minecraft/function");
        std::fs::create_dir_all(&functions).unwrap();
        std::fs::write(functions.join("foo.mcfunction"), "say scheduled\n").unwrap();
        dir
    }

    #[test]
    fn locate_biome_and_the_structure_stubs_report_feedback() {
        let (result, actions) = run("locate biome minecraft:plains", 2);
        assert_eq!(result.unwrap(), 0);
        assert_eq!(
            feedback_key(&actions),
            Some("commands.locate.biome.not_found")
        );

        let (result, actions) = run("locate structure minecraft:village_plains", 2);
        assert_eq!(result.unwrap(), 0);
        assert_eq!(
            feedback_key(&actions),
            Some("commands.locate.structure.not_found")
        );
        let (result, actions) = run("locate poi minecraft:home", 2);
        assert_eq!(result.unwrap(), 0);
        assert_eq!(
            feedback_key(&actions),
            Some("commands.locate.poi.not_found")
        );

        denied("locate biome minecraft:plains");
        assert!(run("locate biome", 2).0.is_err());
    }

    #[test]
    fn reload_and_datapack_commands_produce_actions() {
        assert_eq!(only("reload", 4), Action::Reload);
        denied("reload");
        assert_eq!(only("datapack list", 4), Action::DatapackList);
        assert_eq!(
            only("datapack enable pack", 4),
            Action::DatapackSet {
                pack: "pack".into(),
                enable: true,
            }
        );
        assert_eq!(
            only("datapack disable pack", 4),
            Action::DatapackSet {
                pack: "pack".into(),
                enable: false,
            }
        );
        denied("datapack list");
        assert!(run("datapack enable", 4).0.is_err());
    }

    #[test]
    fn help_lists_commands_and_reports_unknown_names() {
        let (result, actions) = run("help", 0);
        assert!(result.is_ok(), "{result:?}");
        assert!(actions.iter().any(|action| matches!(
            action,
            Action::Feedback(feedback) if feedback.key == "commands.help.command"
        )));

        let (result, actions) = run("help list", 0);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(feedback_key(&actions), Some("commands.help.usage"));

        assert!(run("help not_a_command", 0).0.is_err());
    }

    #[test]
    fn schedule_replaces_clears_and_becomes_due() {
        let dir = function_world();
        let mut game = GameState::default();
        game.functions = Arc::new(crate::game::functions::FunctionStore::load(Some(&dir)));
        let state = Arc::new(Mutex::new(game));

        let (result, actions) = run_with_state(
            "schedule function minecraft:foo 40t",
            2,
            state.clone(),
            None,
            None,
        );
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(
            feedback_key(&actions),
            Some("commands.schedule.created.function")
        );
        assert_eq!(state.lock().unwrap().schedules["minecraft:foo"], vec![40]);

        // The append form keeps both entries; replace keeps only the new one.
        assert!(
            run_with_state(
                "schedule function minecraft:foo 5t append",
                2,
                state.clone(),
                None,
                None
            )
            .0
            .is_ok()
        );
        assert_eq!(state.lock().unwrap().schedules["minecraft:foo"].len(), 2);
        assert!(
            run_with_state(
                "schedule function minecraft:foo 5t replace",
                2,
                state.clone(),
                None,
                None
            )
            .0
            .is_ok()
        );
        assert_eq!(state.lock().unwrap().schedules["minecraft:foo"], vec![5]);
        let (result, actions) =
            run_with_state("schedule clear minecraft:foo", 2, state.clone(), None, None);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(
            feedback_key(&actions),
            Some("commands.schedule.cleared.success")
        );
        let (result, actions) =
            run_with_state("schedule clear minecraft:foo", 2, state.clone(), None, None);
        assert_eq!(result.unwrap(), 0);
        assert_eq!(
            feedback_key(&actions),
            Some("commands.schedule.cleared.failure")
        );

        // A schedule becomes due once and leaves the map.
        assert!(
            run_with_state(
                "schedule function minecraft:foo 3t",
                2,
                state.clone(),
                None,
                None
            )
            .0
            .is_ok()
        );
        let mut due = Vec::new();
        for _ in 0..3 {
            due = state.lock().unwrap().tick(true);
        }
        assert_eq!(due, vec!["minecraft:foo".to_owned()]);
        assert!(state.lock().unwrap().schedules.is_empty());

        // Unknown functions and non-gamemasters are rejected.
        assert!(
            run_with_state(
                "schedule function minecraft:missing 1t",
                2,
                state.clone(),
                None,
                None
            )
            .0
            .is_err()
        );
        denied("schedule function minecraft:foo 1t");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn trigger_respects_objective_and_enable_state() {
        let mut game = GameState::default();
        let uuid = Uuid::new_v4();
        game.join(uuid, "Alice".into(), 0, 20);
        game.objectives.insert(
            "test:obj".into(),
            Objective {
                criteria: "minecraft:trigger".into(),
                display_name: "Obj".into(),
                render_type: 0,
            },
        );
        let state = Arc::new(Mutex::new(game));

        // The scoreboard enable command primes the trigger for Alice.
        let (result, _) = run_with_state(
            "scoreboard players enable Alice test:obj",
            2,
            state.clone(),
            None,
            None,
        );
        assert!(result.is_ok(), "{result:?}");
        assert!(
            state
                .lock()
                .unwrap()
                .trigger_enabled
                .contains(&("test:obj".into(), "Alice".into()))
        );

        let (result, actions) = run_with_state(
            "trigger test:obj add 5",
            0,
            state.clone(),
            Some(uuid),
            Some("Alice"),
        );
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(
            state.lock().unwrap().scores[&("Alice".into(), "test:obj".into())],
            5
        );
        assert!(
            actions
                .iter()
                .any(|action| matches!(action, Action::Packets { .. }))
        );
        assert!(
            !state
                .lock()
                .unwrap()
                .trigger_enabled
                .contains(&("test:obj".into(), "Alice".into()))
        );

        // A used (or never primed) trigger is disabled, and unknown objectives fail.
        assert!(
            run_with_state(
                "trigger test:obj add 1",
                0,
                state.clone(),
                Some(uuid),
                Some("Alice")
            )
            .0
            .is_err()
        );
        assert!(
            run_with_state(
                "trigger test:missing add 1",
                0,
                state.clone(),
                Some(uuid),
                Some("Alice")
            )
            .0
            .is_err()
        );
        assert!(
            run_with_state("trigger test:obj add 1", 0, state.clone(), None, None)
                .0
                .is_err()
        );
    }

    #[test]
    fn list_has_plain_and_uuid_forms_and_rejects_extra_arguments() {
        let (result, actions) = run("list", 0);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(feedback_key(&actions), Some("commands.list.players"));
        let (result, actions) = run("list uuids", 0);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(feedback_key(&actions), Some("commands.list.players"));
        assert!(run("list nope", 0).0.is_err());
    }

    #[test]
    fn tp_and_teleport_accept_vanilla_forms() {
        denied("tp 1 2 3");
        denied("teleport 1 2 3");

        assert_eq!(
            only("tp 10 70 20", 2),
            Action::Teleport {
                targets: self_target(),
                dimension: crate::game::Dimension::Overworld,
                destination: TeleportDestination::Location {
                    pos: [10.5, 70.0, 20.5],
                    yaw: None,
                    pitch: None,
                },
            }
        );
        assert_eq!(
            only("teleport @a ~ ~5 ~", 2),
            Action::Teleport {
                targets: EntityTarget::Selector {
                    kind: SelectorKind::AllPlayers,
                    options: Vec::new(),
                },
                destination: TeleportDestination::Location {
                    pos: [0.0, 69.0, 0.0],
                    yaw: None,
                    pitch: None,
                },
                dimension: crate::game::Dimension::Overworld,
            }
        );
        assert_eq!(
            only("tp @s ^1 ^2 ^3", 2),
            Action::Teleport {
                targets: self_target(),
                dimension: crate::game::Dimension::Overworld,
                destination: TeleportDestination::Location {
                    pos: [1.0, 66.0, 3.0],
                    yaw: None,
                    pitch: None,
                },
            }
        );
        assert_eq!(
            only("tp @s 10 70 20 facing 0 64 0", 2),
            Action::Teleport {
                targets: self_target(),
                dimension: crate::game::Dimension::Overworld,
                destination: TeleportDestination::FacingLocation {
                    pos: [10.5, 70.0, 20.5],
                    target: [0.5, 64.0, 0.5],
                },
            }
        );
        assert_eq!(
            only("tp @s 10 70 20 facing entity Alice eyes", 2),
            Action::Teleport {
                targets: self_target(),
                dimension: crate::game::Dimension::Overworld,
                destination: TeleportDestination::FacingEntity {
                    pos: [10.5, 70.0, 20.5],
                    entity: EntityTarget::Name("Alice".into()),
                    anchor: EntityAnchor::Eyes,
                },
            }
        );
        assert_eq!(
            only("tp @s 10 70 20 rotation ~90 ~-10", 2),
            Action::Teleport {
                targets: self_target(),
                dimension: crate::game::Dimension::Overworld,
                destination: TeleportDestination::Location {
                    pos: [10.5, 70.0, 20.5],
                    yaw: Some(90.0),
                    pitch: Some(-10.0),
                },
            }
        );
        assert_eq!(
            only("tp Alice", 2),
            Action::Teleport {
                targets: self_target(),
                dimension: crate::game::Dimension::Overworld,
                destination: TeleportDestination::Entity(EntityTarget::Name("Alice".into())),
            }
        );
        assert!(run("tp @s 1 2 3 facing entity @p head", 2).0.is_err());
    }

    #[test]
    fn msg_tell_and_w_message_players() {
        let action = only("msg Alice hello world", 0);
        assert_eq!(
            action,
            Action::Message {
                targets: EntityTarget::Name("Alice".into()),
                text: "hello world".into(),
            }
        );
        assert_eq!(only("tell Alice hi", 0), {
            Action::Message {
                targets: EntityTarget::Name("Alice".into()),
                text: "hi".into(),
            }
        });
        assert_eq!(only("w @a hi", 2), {
            Action::Message {
                targets: EntityTarget::Selector {
                    kind: SelectorKind::AllPlayers,
                    options: Vec::new(),
                },
                text: "hi".into(),
            }
        });
        assert!(run("msg", 0).0.is_err());
    }

    #[test]
    fn entity_selectors_require_gamemaster() {
        // Selectors are gated for every command, including /msg and its aliases, which
        // have no command-level requirement of their own (vanilla checkPermissions).
        for input in [
            "msg @a hi",
            "tell @a hi",
            "w @a hi",
            "msg @s hi",
            "msg @a[name=Alice] hi",
        ] {
            let (result, actions) = run(input, 1);
            let CommandError::Syntax(error) = result.unwrap_err() else {
                panic!("{input} should be rejected at level 1");
            };
            assert_eq!(
                error.translation_key(),
                "argument.entity.selector.not_allowed",
                "{input}"
            );
            assert!(actions.is_empty(), "{input} produced {actions:?}");
            assert!(run(input, 2).0.is_ok(), "{input} at level 2");
        }
        // Plain names are not selectors and stay available to unprivileged sources.
        assert_eq!(
            only("msg Alice hi", 0),
            Action::Message {
                targets: EntityTarget::Name("Alice".into()),
                text: "hi".into(),
            }
        );
        // Commands that already require gamemasters: unprivileged sources are rejected
        // and op sources are accepted.
        for input in [
            "kill @a",
            "kill @s",
            "tp @a 0 64 0",
            "teleport @s ~ ~ ~",
            "tp Alice",
        ] {
            assert!(run(input, 1).0.is_err(), "{input} at level 1");
            assert!(run(input, 2).0.is_ok(), "{input} at level 2");
        }
    }

    #[test]
    fn kill_requires_gamemaster() {
        denied("kill @a");
        assert_eq!(
            only("kill @a", 2),
            Action::Kill {
                targets: EntityTarget::Selector {
                    kind: SelectorKind::AllPlayers,
                    options: Vec::new(),
                },
            }
        );
        assert_eq!(
            only("kill", 2),
            Action::Kill {
                targets: self_target(),
            }
        );
    }

    #[test]
    fn kill_accepts_entity_type_tags() {
        assert_eq!(
            only("kill @e[type=#minecraft:zombies]", 2),
            Action::Kill {
                targets: EntityTarget::Selector {
                    kind: SelectorKind::AllEntities,
                    options: vec![ferrite_command::arguments::SelectorOption {
                        key: "type".into(),
                        value: "#minecraft:zombies".into(),
                    }],
                },
            }
        );
    }

    #[test]
    fn op_and_deop_require_admin() {
        denied("op Alice");
        denied("deop Alice");
        assert_eq!(
            only("op Alice", 3),
            Action::Op {
                targets: EntityTarget::Name("Alice".into()),
            }
        );
        assert_eq!(
            only("deop Alice", 3),
            Action::Deop {
                targets: EntityTarget::Name("Alice".into()),
            }
        );
        assert!(run("op", 3).0.is_err());
    }

    #[test]
    fn whitelist_requires_admin() {
        denied("whitelist add Alice");
        denied("whitelist on");
        assert_eq!(
            only("whitelist add Alice", 3),
            Action::WhitelistAdd {
                targets: EntityTarget::Name("Alice".into()),
            }
        );
        assert_eq!(
            only("whitelist remove Alice", 3),
            Action::WhitelistRemove {
                targets: EntityTarget::Name("Alice".into()),
            }
        );
        assert_eq!(only("whitelist list", 3), Action::WhitelistList);
        assert_eq!(only("whitelist on", 3), Action::WhitelistOn);
        assert_eq!(only("whitelist off", 3), Action::WhitelistOff);
        assert_eq!(only("whitelist reload", 3), Action::WhitelistReload);
        assert!(run("whitelist add", 3).0.is_err());
    }

    #[test]
    fn ban_requires_admin_and_defaults_the_reason() {
        denied("ban Alice");
        assert_eq!(
            only("ban Alice", 3),
            Action::Ban {
                targets: EntityTarget::Name("Alice".into()),
                reason: "Banned by an operator.".into(),
            }
        );
        assert_eq!(
            only("ban Alice griefing", 3),
            Action::Ban {
                targets: EntityTarget::Name("Alice".into()),
                reason: "griefing".into(),
            }
        );
        assert!(run("ban", 3).0.is_err());
    }

    #[test]
    fn ban_ip_requires_admin_and_accepts_a_reason() {
        denied("ban-ip 127.0.0.1");
        assert_eq!(
            only("ban-ip 127.0.0.1", 3),
            Action::BanIp {
                target: "127.0.0.1".into(),
                reason: "Banned by an operator.".into(),
            }
        );
        assert_eq!(
            only("ban-ip 127.0.0.1 griefing", 3),
            Action::BanIp {
                target: "127.0.0.1".into(),
                reason: "griefing".into(),
            }
        );
        assert!(run("ban-ip", 3).0.is_err());
    }

    #[test]
    fn pardon_commands_require_admin() {
        denied("pardon Alice");
        denied("pardon-ip 127.0.0.1");
        assert_eq!(
            only("pardon Alice", 3),
            Action::Pardon {
                targets: EntityTarget::Name("Alice".into()),
            }
        );
        assert_eq!(
            only("pardon-ip 127.0.0.1", 3),
            Action::PardonIp {
                target: "127.0.0.1".into(),
            }
        );
        assert!(run("pardon", 3).0.is_err());
    }

    #[test]
    fn banlist_requires_admin_and_lists_both_kinds() {
        denied("banlist");
        assert_eq!(only("banlist", 3), Action::BanList { ips: false });
        assert_eq!(only("banlist players", 3), Action::BanList { ips: false });
        assert_eq!(only("banlist ips", 3), Action::BanList { ips: true });
        assert!(run("banlist everything", 3).0.is_err());
    }

    #[test]
    fn kick_requires_admin_and_defaults_the_reason() {
        denied("kick Alice");
        assert_eq!(
            only("kick Alice", 3),
            Action::Kick {
                targets: EntityTarget::Name("Alice".into()),
                reason: "Kicked by an operator".into(),
            }
        );
        assert_eq!(
            only("kick @a nice try", 3),
            Action::Kick {
                targets: EntityTarget::Selector {
                    kind: SelectorKind::AllPlayers,
                    options: Vec::new(),
                },
                reason: "nice try".into(),
            }
        );
        assert!(run("kick", 3).0.is_err());
    }

    #[test]
    fn stop_requires_owner() {
        denied("stop");
        assert!(run("stop", 3).0.is_err());
        assert_eq!(only("stop", 4), Action::Stop);
    }

    #[test]
    fn save_all_requires_owner_and_accepts_flush() {
        denied("save-all");
        assert!(run("save-all", 3).0.is_err());
        assert_eq!(only("save-all", 4), Action::SaveAll { flush: false });
        assert_eq!(only("save-all flush", 4), Action::SaveAll { flush: true });
        assert!(run("save-all slowly", 4).0.is_err());
    }

    #[test]
    fn entity_and_profile_resolution_use_online_then_cache() {
        use ferrite_admin::AdminLists;

        let alice = Uuid::from_u128(1);
        let bob = Uuid::from_u128(2);
        let carol = Uuid::from_u128(3);
        let mut online = HashMap::new();
        for (uuid, name, pos) in [
            (alice, "Alice", [0.0, 64.0, 0.0]),
            (bob, "Bob", [100.0, 64.0, 0.0]),
            (carol, "Carol", [10.0, 64.0, 0.0]),
        ] {
            let (outbound, _) = OutboundSender::channel(1 << 20);
            online.insert(
                uuid,
                OnlinePlayer {
                    name: name.into(),
                    entity: 0,
                    outbound,
                    pos,
                    yaw: 0.0,
                    pitch: 0.0,
                    dimension: crate::game::Dimension::Overworld,
                },
            );
        }

        let name = |name: &str| EntityTarget::Name(name.into());
        assert_eq!(
            resolve_online(&name("alice"), &online, None, [0.0; 3]).unwrap(),
            [ResolvedPlayer {
                uuid: alice,
                name: "Alice".into()
            }]
        );
        assert_eq!(
            resolve_online(&name("Nobody"), &online, None, [0.0; 3]).unwrap_err(),
            "argument.entity.notfound.player"
        );
        let all = |options| EntityTarget::Selector {
            kind: SelectorKind::AllPlayers,
            options,
        };
        let names = |players: Vec<ResolvedPlayer>| {
            players
                .into_iter()
                .map(|player| player.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(resolve_online(&all(Vec::new()), &online, None, [0.0; 3]).unwrap()),
            ["Alice", "Bob", "Carol"]
        );
        assert_eq!(
            names(
                resolve_online(
                    &all(vec![ferrite_command::arguments::SelectorOption {
                        key: "limit".into(),
                        value: "1".into()
                    }]),
                    &online,
                    None,
                    [0.0; 3]
                )
                .unwrap()
            ),
            ["Alice"]
        );
        assert_eq!(
            names(
                resolve_online(
                    &all(vec![ferrite_command::arguments::SelectorOption {
                        key: "name".into(),
                        value: "Bob".into()
                    }]),
                    &online,
                    None,
                    [0.0; 3]
                )
                .unwrap()
            ),
            ["Bob"]
        );
        assert_eq!(
            names(
                resolve_online(
                    &EntityTarget::Selector {
                        kind: SelectorKind::NearestPlayer,
                        options: Vec::new()
                    },
                    &online,
                    None,
                    [100.0, 64.0, 0.0]
                )
                .unwrap()
            ),
            ["Bob"]
        );
        assert_eq!(
            resolve_online(&self_target(), &online, Some(carol), [0.0; 3]).unwrap(),
            [ResolvedPlayer {
                uuid: carol,
                name: "Carol".into()
            }]
        );

        let dir = std::env::temp_dir().join(format!("ferrite-command-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let admin = AdminLists::load(&dir).unwrap();
        let offline = Uuid::from_u128(4);
        admin.usercache.cache("Offline", offline);
        assert_eq!(
            resolve_profiles(&name("Offline"), &online, Some(&admin), None, [0.0; 3]).unwrap(),
            [ResolvedPlayer {
                uuid: offline,
                name: "Offline".into()
            }]
        );
        assert_eq!(
            resolve_profiles(&name("Ghost"), &online, Some(&admin), None, [0.0; 3]).unwrap(),
            [ResolvedPlayer {
                uuid: offline_uuid("Ghost"),
                name: "Ghost".into()
            }]
        );
        assert_eq!(
            offline_uuid("Alice"),
            Uuid::parse_str("10920508-d5d8-3eed-93d2-92f193afe7d7").unwrap()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Vanilla levels from `commands.json`: `tp`/`teleport`/`kill` = gamemasters (2),
    /// admin commands = admins (3), server control = owners (4), `/msg` = all (0).
    const VANILLA_COMMAND_LEVELS: &[(&str, u8)] = &[
        ("tp", 2),
        ("teleport", 2),
        ("msg", 0),
        ("tell", 0),
        ("w", 0),
        ("kill", 2),
        ("op", 3),
        ("deop", 3),
        ("whitelist", 3),
        ("ban", 3),
        ("ban-ip", 3),
        ("pardon", 3),
        ("pardon-ip", 3),
        ("banlist", 3),
        ("kick", 3),
        ("stop", 4),
        ("save-all", 4),
        ("save-on", 4),
        ("save-off", 4),
    ];

    fn vanilla_level(node: &serde_json::Value) -> u8 {
        node.pointer("/permissions/permission/level")
            .and_then(serde_json::Value::as_str)
            .map_or(0, |level| match level {
                "all" => 0,
                "moderators" => 1,
                "gamemasters" => 2,
                "admins" => 3,
                "owners" => 4,
                other => panic!("unknown vanilla permission level {other}"),
            })
    }

    #[test]
    fn command_permission_levels_match_vanilla() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../vanilla/26.3/generated/reports/commands.json");
        let Ok(json) = std::fs::read_to_string(&path) else {
            eprintln!("skipping: {} not generated", path.display());
            return;
        };
        let root: serde_json::Value = serde_json::from_str(&json).unwrap();
        let tree = dispatcher();
        for &(name, expected) in VANILLA_COMMAND_LEVELS {
            let node = root["children"]
                .get(name)
                .unwrap_or_else(|| panic!("/{name} missing from vanilla commands.json"));
            assert_eq!(
                vanilla_level(node),
                expected,
                "/{name} table disagrees with commands.json"
            );
            let id = tree
                .find_node(&[name])
                .unwrap_or_else(|| panic!("/{name} missing from the dispatcher"));
            for level in 0..=4 {
                assert_eq!(
                    tree.node(id).can_use(&source(level)),
                    level >= expected,
                    "/{name} at permission level {level}"
                );
            }
        }
    }

    #[test]
    fn save_on_and_off_require_owner() {
        denied("save-on");
        denied("save-off");
        assert!(run("save-on", 3).0.is_err());
        assert_eq!(only("save-on", 4), Action::SaveOn);
        assert_eq!(only("save-off", 4), Action::SaveOff);
    }

    use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

    const ALICE: u128 = 1;
    const BOB: u128 = 2;

    fn state_with(players: &[(u128, &str, i32)]) -> Arc<Mutex<GameState>> {
        let mut state = GameState::default();
        for &(uuid, name, mode) in players {
            state.join(Uuid::from_u128(uuid), name.into(), mode, 20);
        }
        Arc::new(Mutex::new(state))
    }

    fn player_source(
        permission: u8,
        player: Option<(u128, &str)>,
        state: Arc<Mutex<GameState>>,
    ) -> (Source, UnboundedReceiver<Action>) {
        let (actions, received) = unbounded_channel();
        let (uuid, name) = match player {
            Some((uuid, name)) => (Some(Uuid::from_u128(uuid)), Some(name.to_owned())),
            None => (None, None),
        };
        (
            Source {
                permission,
                origin: [0.0; 3],
                yaw: 0.0,
                pitch: 0.0,
                uuid,
                name,
                state,
                actions,
                anchor: ferrite_command::EntityAnchor::Feet,
                dimension: crate::game::Dimension::Overworld,
                world: None,
                tree: None,
                function_depth: 0,
            },
            received,
        )
    }

    fn next_feedback(received: &mut UnboundedReceiver<Action>) -> Feedback {
        loop {
            if let Action::Feedback(feedback) = received.try_recv().expect("expected an action") {
                return feedback;
            }
        }
    }

    fn error_key(error: CommandError) -> String {
        match error {
            CommandError::Syntax(error) => error.translation_key().to_owned(),
            other => panic!("expected a syntax error, got {other:?}"),
        }
    }

    #[test]
    fn gamemode_sets_self_and_reports_missing_target() {
        let state = state_with(&[(ALICE, "Alice", 1), (BOB, "Bob", 1)]);
        let tree = dispatcher();
        let (alice, mut received) = player_source(2, Some((ALICE, "Alice")), state.clone());
        assert_eq!(tree.execute("gamemode survival", alice).unwrap(), 1);
        assert!(matches!(
            received.try_recv().unwrap(),
            Action::GameMode { mode: 0, ref targets } if targets == &[Uuid::from_u128(ALICE)]
        ));
        let feedback = next_feedback(&mut received);
        assert_eq!(feedback.key, "commands.gamemode.success.self");
        assert_eq!(feedback.text, "Set own game mode to Survival Mode");
        assert_eq!(
            state
                .lock()
                .unwrap()
                .players
                .iter()
                .find(|player| player.uuid == Uuid::from_u128(ALICE))
                .unwrap()
                .game_mode,
            0
        );

        let (alice, _) = player_source(2, Some((ALICE, "Alice")), state);
        let error = tree.execute("gamemode creative Nobody", alice).unwrap_err();
        assert_eq!(error_key(error), "argument.entity.notfound.player");
    }

    #[test]
    fn defaultgamemode_updates_state_and_requires_an_argument() {
        let state = state_with(&[]);
        let tree = dispatcher();
        let (console, mut received) = player_source(4, None, state.clone());
        assert_eq!(
            tree.execute("defaultgamemode creative", console).unwrap(),
            1
        );
        assert!(matches!(
            received.try_recv().unwrap(),
            Action::DefaultGameMode(1)
        ));
        let feedback = next_feedback(&mut received);
        assert_eq!(feedback.key, "commands.defaultgamemode.success");
        assert_eq!(feedback.text, "The default game mode is now Creative Mode");
        assert_eq!(state.lock().unwrap().default_gamemode, 1);

        let (console, _) = player_source(4, None, state);
        let error = tree.execute("defaultgamemode", console).unwrap_err();
        assert_eq!(error_key(error), "command.unknown.command");
    }

    #[test]
    fn difficulty_queries_sets_and_rejects_the_same_value() {
        let state = state_with(&[]);
        let tree = dispatcher();
        let (console, mut received) = player_source(2, None, state.clone());
        assert_eq!(tree.execute("difficulty", console).unwrap(), 2);
        let feedback = next_feedback(&mut received);
        assert_eq!(feedback.key, "commands.difficulty.query");
        assert_eq!(feedback.text, "The difficulty is Normal");

        let (console, mut received) = player_source(2, None, state.clone());
        assert_eq!(tree.execute("difficulty hard", console).unwrap(), 0);
        assert!(matches!(
            received.try_recv().unwrap(),
            Action::Difficulty(3)
        ));
        assert_eq!(
            next_feedback(&mut received).key,
            "commands.difficulty.success"
        );
        assert_eq!(state.lock().unwrap().difficulty, 3);

        let (console, _) = player_source(2, None, state);
        let error = tree.execute("difficulty hard", console).unwrap_err();
        assert_eq!(error_key(error), "commands.difficulty.failure");
    }

    #[test]
    fn time_sets_adds_queries_and_rejects_negative_ticks() {
        let state = state_with(&[]);
        let tree = dispatcher();
        let (console, mut received) = player_source(2, None, state.clone());
        assert_eq!(tree.execute("time set 1000", console).unwrap(), 1000);
        assert!(matches!(
            received.try_recv().unwrap(),
            Action::TimeSet(1000)
        ));
        let feedback = next_feedback(&mut received);
        assert_eq!(feedback.key, "commands.time.set.absolute");
        assert_eq!(feedback.text, "Set minecraft:overworld to 1000 tick(s)");

        let (console, mut received) = player_source(2, None, state.clone());
        assert_eq!(tree.execute("time add 20", console).unwrap(), 1020);
        assert!(matches!(received.try_recv().unwrap(), Action::TimeAdd(20)));
        assert_eq!(
            next_feedback(&mut received).text,
            "Set minecraft:overworld to 1020 tick(s)"
        );

        let (console, mut received) = player_source(2, None, state.clone());
        assert_eq!(tree.execute("time query time", console).unwrap(), 1020);
        assert_eq!(
            next_feedback(&mut received).text,
            "Clock minecraft:overworld is at 1020 tick(s)"
        );

        let (console, _) = player_source(2, None, state);
        let error = tree.execute("time set -1", console).unwrap_err();
        assert_eq!(error_key(error), "argument.time.tick_count_too_low");
    }

    #[test]
    fn weather_sets_duration_and_rejects_zero() {
        let state = state_with(&[]);
        let tree = dispatcher();
        let (console, mut received) = player_source(2, None, state.clone());
        assert_eq!(tree.execute("weather thunder 200", console).unwrap(), 200);
        assert!(matches!(
            received.try_recv().unwrap(),
            Action::Weather {
                kind: Weather::Thunder,
                duration: Some(200)
            }
        ));
        let feedback = next_feedback(&mut received);
        assert_eq!(feedback.key, "commands.weather.set.thunder");
        assert_eq!(feedback.text, "Set the weather to rain and thunder");
        assert_eq!(state.lock().unwrap().weather, Weather::Thunder);

        let (console, _) = player_source(2, None, state);
        let error = tree.execute("weather rain 0", console).unwrap_err();
        assert_eq!(error_key(error), "argument.time.tick_count_too_low");
    }

    #[test]
    fn seed_reports_world_seed_and_requires_permission() {
        let game_state = GameState {
            seed: 12345,
            ..GameState::default()
        };
        let state = Arc::new(Mutex::new(game_state));
        let tree = dispatcher();
        let (console, mut received) = player_source(2, None, state.clone());
        assert_eq!(tree.execute("seed", console).unwrap(), 12345);
        let feedback = next_feedback(&mut received);
        assert_eq!(feedback.key, "commands.seed.success");
        assert_eq!(feedback.text, "Seed: 12345");

        let (guest, _) = player_source(0, None, state);
        assert!(tree.execute("seed", guest).is_err());
    }

    #[test]
    fn list_formats_players_and_rejects_unknown_subcommands() {
        let state = state_with(&[(ALICE, "Alice", 1), (BOB, "Bob", 0)]);
        let tree = dispatcher();
        let (guest, mut received) = player_source(0, Some((ALICE, "Alice")), state.clone());
        assert_eq!(tree.execute("list", guest).unwrap(), 2);
        let feedback = next_feedback(&mut received);
        assert_eq!(feedback.key, "commands.list.players");
        assert_eq!(
            feedback.text,
            "There are 2 of a max of 20 players online: Alice, Bob"
        );

        let (guest, mut received) = player_source(0, Some((ALICE, "Alice")), state.clone());
        tree.execute("list uuids", guest).unwrap();
        let feedback = next_feedback(&mut received);
        assert_eq!(
            feedback.text,
            format!(
                "There are 2 of a max of 20 players online: Alice ({}), Bob ({})",
                Uuid::from_u128(ALICE),
                Uuid::from_u128(BOB)
            )
        );

        let (guest, _) = player_source(0, Some((ALICE, "Alice")), state);
        assert!(tree.execute("list nonsense", guest).is_err());
    }

    #[test]
    fn say_broadcasts_with_announcement_chat_type_and_checks_permission() {
        let state = state_with(&[(ALICE, "Alice", 1)]);
        let tree = dispatcher();
        let (alice, mut received) = player_source(2, Some((ALICE, "Alice")), state.clone());
        assert_eq!(tree.execute("say hello world", alice).unwrap(), 1);
        let Action::Broadcast(feedback) = received.try_recv().unwrap() else {
            panic!("expected broadcast")
        };
        assert_eq!(feedback.key, "chat.type.announcement");
        assert_eq!(feedback.args, ["Alice", "hello world"]);
        assert_eq!(feedback.text, "[Alice] hello world");

        let (guest, _) = player_source(0, Some((ALICE, "Alice")), state);
        assert!(tree.execute("say nope", guest).is_err());
    }

    #[test]
    fn me_broadcasts_as_emote_and_requires_an_action() {
        let state = state_with(&[(ALICE, "Alice", 1)]);
        let tree = dispatcher();
        let (alice, mut received) = player_source(0, Some((ALICE, "Alice")), state.clone());
        assert_eq!(tree.execute("me waves", alice).unwrap(), 1);
        let Action::Broadcast(feedback) = received.try_recv().unwrap() else {
            panic!("expected broadcast")
        };
        assert_eq!(feedback.key, "chat.type.emote");
        assert_eq!(feedback.args, ["Alice", "waves"]);
        assert_eq!(feedback.text, "* Alice waves");

        let (alice, _) = player_source(0, Some((ALICE, "Alice")), state);
        assert!(tree.execute("me", alice).is_err());
    }

    #[test]
    fn entity_selectors_require_permission_level_two() {
        let state = state_with(&[(ALICE, "Alice", 1)]);
        let tree = dispatcher();
        for selector in ["@a", "@p", "@r", "@s"] {
            let (guest, _) = player_source(0, Some((ALICE, "Alice")), state.clone());
            let error = tree
                .execute(&format!("msg {selector} hi"), guest)
                .unwrap_err();
            assert_eq!(
                error_key(error),
                "argument.entity.selector.not_allowed",
                "{selector} must be rejected below permission 2"
            );
        } // `msg` only accepts player selectors, and every server command that takes an
        // entity-only selector already requires gamemasters, so the `@e` case lives in the
        // dispatcher's own test (`ferrite-command/tests/dispatcher.rs`).
        let (operator, _received) = player_source(2, Some((ALICE, "Alice")), state);
        let result = tree.execute("msg @a hi", operator);
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn tps_mspt_and_profile_use_the_profiler_and_require_permission() {
        let state = state_with(&[]);
        let tree = dispatcher();
        let (guest, _) = player_source(0, Some((ALICE, "Alice")), state.clone());
        assert!(tree.execute("profile start", guest).is_err());

        let (operator, mut received) = player_source(2, Some((ALICE, "Alice")), state);
        tree.execute("tps", operator.clone()).unwrap();
        assert!(matches!(received.try_recv().unwrap(), Action::Tps));
        tree.execute("mspt", operator.clone()).unwrap();
        assert!(matches!(received.try_recv().unwrap(), Action::Mspt));
        tree.execute("profile start", operator.clone()).unwrap();
        assert!(matches!(received.try_recv().unwrap(), Action::ProfileStart));
        tree.execute("profile stop", operator).unwrap();
        assert!(matches!(received.try_recv().unwrap(), Action::ProfileStop));
    }

    #[test]
    fn summon_parses_the_vanilla_entity_and_position_arguments() {
        assert_eq!(
            only("summon minecraft:cow", 2),
            Action::Summon {
                kind: MobKind::Cow,
                pos: [0.0, 64.0, 0.0],
            }
        );
        assert_eq!(
            only("summon pig 10 64.5 -3", 2),
            Action::Summon {
                kind: MobKind::Pig,
                pos: [10.5, 64.5, -2.5],
            }
        );
        assert_eq!(
            only("summon sheep ~ ~2 ~", 2),
            Action::Summon {
                kind: MobKind::Sheep,
                pos: [0.0, 66.0, 0.0],
            }
        );
        assert_eq!(
            only("summon minecraft:zombie 2.5 65 0.5", 2),
            Action::Summon {
                kind: MobKind::Zombie,
                pos: [2.5, 65.0, 0.5],
            }
        );
        // A vanilla entity this server cannot simulate yet, and an unknown id.
        assert_eq!(
            only("summon minecraft:villager", 2),
            Action::Summon {
                kind: MobKind::Villager,
                pos: [0.0, 64.0, 0.0],
            }
        );
        assert!(run("summon not_a_thing", 2).0.is_err());
        assert!(run("summon minecraft:cow", 1).0.is_err());
    }

    #[test]
    fn plugin_commands_register_aliases_and_forward_arguments() {
        let mut tree = dispatcher();
        register_plugin_commands(
            &mut tree,
            &[ferrite_plugin::PluginCommand {
                name: "testping".into(),
                aliases: vec!["tping".into()],
                permission: None,
                usage: Some("/testping".into()),
            }],
        );
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let source = Source {
            permission: 0,
            origin: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            uuid: None,
            name: None,
            anchor: EntityAnchor::Feet,
            dimension: crate::game::Dimension::Overworld,
            world: None,
            state: Arc::new(Mutex::new(GameState::default())),
            actions: tx,
            tree: None,
            function_depth: 0,
        };
        tree.execute("testping", source.clone()).unwrap();
        assert_eq!(
            rx.try_recv().unwrap(),
            Action::Plugin {
                name: "testping".into(),
                args: Vec::new()
            }
        );
        tree.execute("tping one two", source).unwrap();
        assert_eq!(
            rx.try_recv().unwrap(),
            Action::Plugin {
                name: "tping".into(),
                args: vec!["one".into(), "two".into()]
            }
        );
    }

    #[test]
    fn gamerule_commands_use_vanilla_names_types_and_bounds() {
        assert_eq!(
            only("gamerule random_tick_speed 0", 2),
            Action::GameRule("random_tick_speed".into(), Some("0".into()))
        );
        assert_eq!(
            only("gamerule minecraft:random_tick_speed 1", 2),
            Action::GameRule("random_tick_speed".into(), Some("1".into()))
        );
        assert_eq!(
            only("gamerule keep_inventory", 2),
            Action::GameRule("keep_inventory".into(), None)
        );
        // Pre-26.3 spellings map onto the renamed rules.
        assert_eq!(
            only("gamerule doDaylightCycle false", 2),
            Action::GameRule("advance_time".into(), Some("false".into()))
        );
        assert_eq!(
            only("gamerule randomTickSpeed 0", 2),
            Action::GameRule("random_tick_speed".into(), Some("0".into()))
        );
        // The generated parsers reject the wrong type and out-of-range integers.
        assert!(run("gamerule random_tick_speed -1", 2).0.is_err());
        assert!(run("gamerule keep_inventory 42", 2).0.is_err());
        assert!(run("gamerule does_not_exist", 2).0.is_err());
    }

    fn inventory_source(permission: u8) -> (Source, tokio::sync::mpsc::UnboundedReceiver<Action>) {
        let alice = Uuid::from_u128(1);
        let mut state = GameState::default();
        state.join(alice, "Alice".into(), 1, 20);
        state.join(Uuid::from_u128(2), "Bob".into(), 1, 20);
        let (actions, received) = tokio::sync::mpsc::unbounded_channel();
        (
            Source {
                permission,
                origin: [0.0, 64.0, 0.0],
                yaw: 0.0,
                pitch: 0.0,
                uuid: Some(alice),
                name: Some("Alice".into()),
                state: Arc::new(Mutex::new(state)),
                actions,
                anchor: ferrite_command::EntityAnchor::Feet,
                dimension: crate::game::Dimension::Overworld,
                world: None,
                tree: None,
                function_depth: 0,
            },
            received,
        )
    }

    #[test]
    fn clear_and_give_resolve_targets_items_and_counts() {
        let tree = dispatcher();
        let (source, mut rx) = inventory_source(2);
        let stone = ferrite_item::item_id("minecraft:stone").unwrap();
        tree.execute("clear", source.clone()).unwrap();
        assert_eq!(
            rx.try_recv().unwrap(),
            Action::Clear {
                targets: vec![Uuid::from_u128(1)],
                item: None,
                max_count: None
            }
        );
        tree.execute("clear Bob minecraft:stone 5", source.clone())
            .unwrap();
        assert_eq!(
            rx.try_recv().unwrap(),
            Action::Clear {
                targets: vec![Uuid::from_u128(2)],
                item: Some(stone),
                max_count: Some(5)
            }
        );
        tree.execute("give @s stone 3", source.clone()).unwrap();
        assert_eq!(
            rx.try_recv().unwrap(),
            Action::Give {
                targets: vec![Uuid::from_u128(1)],
                item: stone,
                count: 3,
                item_name: "Stone".into(),
                components: Vec::new(),
            }
        );
        tree.execute("give @p apple", source.clone()).unwrap();
        assert!(matches!(
            rx.try_recv().unwrap(),
            Action::Give { targets, count: 1, .. } if targets == vec![Uuid::from_u128(1)]
        ));
        assert!(
            tree.execute("give @s diamond_sword 2", source.clone())
                .unwrap_err()
                .to_string()
                .contains("Can't give more than 1 of Diamond Sword")
        );
        assert!(tree.execute("give @s stone 65", source.clone()).is_err());
        assert_eq!(
            tree.execute("give @s not_an_item", source.clone())
                .unwrap_err()
                .to_string(),
            "Unknown item 'not_an_item'"
        );
        tree.execute(
            r#"give @s potion[potion_contents={potion:"minecraft:swiftness"}]"#,
            source.clone(),
        )
        .unwrap();
        match rx.try_recv().unwrap() {
            Action::Give {
                item, components, ..
            } => {
                assert_eq!(item, ferrite_item::item_id("potion").unwrap());
                assert_eq!(components.len(), 1);
                assert_eq!(
                    components[0].0,
                    ferrite_item::component_ids::POTION_CONTENTS
                );
            }
            other => panic!("expected give, got {other:?}"),
        }
        let (guest, mut guest_rx) = inventory_source(1);
        assert!(tree.execute("clear", guest.clone()).is_err());
        assert!(tree.execute("give @s stone", guest).is_err());
        assert!(guest_rx.try_recv().is_err());
    }

    #[test]
    fn xp_resolves_operations_units_and_permissions() {
        let tree = dispatcher();
        let (source, mut rx) = inventory_source(2);
        tree.execute("xp add @s 5", source.clone()).unwrap();
        assert_eq!(
            rx.try_recv().unwrap(),
            Action::Experience {
                operation: XpOperation::Add,
                targets: vec![Uuid::from_u128(1)],
                amount: 5,
                unit: XpUnit::Points
            }
        );
        tree.execute("experience add Bob -3 levels", source.clone())
            .unwrap();
        assert_eq!(
            rx.try_recv().unwrap(),
            Action::Experience {
                operation: XpOperation::Add,
                targets: vec![Uuid::from_u128(2)],
                amount: -3,
                unit: XpUnit::Levels
            }
        );
        tree.execute("xp set @p 7 levels", source.clone()).unwrap();
        assert!(matches!(
            rx.try_recv().unwrap(),
            Action::Experience {
                operation: XpOperation::Set,
                amount: 7,
                unit: XpUnit::Levels,
                ..
            }
        ));
        tree.execute("xp query Alice points", source.clone())
            .unwrap();
        assert_eq!(
            rx.try_recv().unwrap(),
            Action::Experience {
                operation: XpOperation::Query,
                targets: vec![Uuid::from_u128(1)],
                amount: 0,
                unit: XpUnit::Points
            }
        );
        assert!(tree.execute("xp set @s -1", source.clone()).is_err());
        let (guest, _) = inventory_source(1);
        assert!(tree.execute("xp add @s 1", guest).is_err());
    }

    #[test]
    fn advance_time_only_stops_the_clock() {
        let mut state = GameState::default();
        state.tick(true);
        state.tick(false);
        assert_eq!(state.clock_time, 1);
        assert_eq!(state.game_time, 2);
    }
}
