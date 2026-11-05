//! Commands that complete vanilla's list: `/version`, `/random`, `/setidletimeout`,
//! `/teammsg` (`/tm`), `/transfer`, `/stopwatch`, `/publish` and `/unpublish`.
//!
//! Permission levels and syntax follow the generated `commands.json` of the user's own server
//! jar. Feedback strings come from the translations extracted from that jar
//! (`reports/lang_en_us.json`), so no Mojang text is kept here.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use ferrite_command::{
    ArgumentType, CommandContext, CommandDispatcher, CommandError, NodeBuilder, argument, literal,
};
use ferrite_protocol::{VarInt, packets::play::Transfer};

use super::commands::{
    Action, Feedback, RotateTo, Source, anchor, entity, player_only, resolved_targets, rotation,
    send,
};
use super::commands_extras::PlayerPacket;

/// State owned by these commands, kept inside [`super::commands::GameState`].
#[derive(Debug, Clone, Default)]
pub struct MoreState {
    /// `/random` sequences: id -> generator state.
    pub sequences: BTreeMap<String, u64>,
    /// `/setidletimeout`: minutes before an idle player is kicked; 0 disables it.
    pub idle_timeout_minutes: i32,
    /// `/stopwatch`: id -> game time it was (re)started at.
    pub stopwatches: BTreeMap<String, i64>,
    /// Game time of each player's last action, for `/setidletimeout`.
    pub last_active: BTreeMap<uuid::Uuid, i64>,
    /// `/tick`: shared with the game loop and the region ticks.
    pub tick: std::sync::Arc<TickControl>,
    /// `/forceload`: (dimension key, chunk x, chunk z) of every force-loaded chunk.
    pub forced: std::collections::BTreeSet<(String, i32, i32)>,
    /// `/debug` and `/perf` runs in progress: wall clock and game time at the start.
    pub profiling: BTreeMap<&'static str, (std::time::Instant, i64)>,
    /// `/attribute`: (entity, attribute path) -> base override and modifiers.
    pub attributes: BTreeMap<(uuid::Uuid, String), super::attributes::Stored>,
    /// `/posteffect`: post-processing effects each player has, in the order they were added.
    pub post_effects: BTreeMap<uuid::Uuid, Vec<String>>,
    /// `/advancement`: `advancement#criterion` keys each player has completed.
    pub advancements: BTreeMap<uuid::Uuid, std::collections::BTreeSet<String>>,
    /// `/recipe`: recipe ids each player has unlocked.
    pub recipes: BTreeMap<uuid::Uuid, std::collections::BTreeSet<String>>,
    /// `/waypoint modify`: color and style per entity.
    pub waypoints: BTreeMap<uuid::Uuid, WaypointStyle>,
}

/// What `/waypoint modify` changed for one entity.
#[derive(Debug, Clone, Default)]
pub struct WaypointStyle {
    pub color: Option<String>,
    pub style: Option<String>,
}

/// What `/tick` changes: the target rate, the frozen flag and the pending steps. The game loop
/// applies the rate to the schedulers and consumes steps; region ticks read `simulating`.
#[derive(Debug)]
pub struct TickControl {
    rate_milli: std::sync::atomic::AtomicU32,
    frozen: std::sync::atomic::AtomicBool,
    steps: std::sync::atomic::AtomicU32,
    stepping: std::sync::atomic::AtomicBool,
    mspt_micro: std::sync::atomic::AtomicU64,
}

impl Default for TickControl {
    fn default() -> Self {
        use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};
        Self {
            rate_milli: AtomicU32::new(20_000),
            frozen: AtomicBool::new(false),
            steps: AtomicU32::new(0),
            stepping: AtomicBool::new(false),
            mspt_micro: AtomicU64::new(0),
        }
    }
}

impl TickControl {
    const RELAXED: std::sync::atomic::Ordering = std::sync::atomic::Ordering::Relaxed;

    /// Target ticks per second.
    pub fn rate(&self) -> f32 {
        self.rate_milli.load(Self::RELAXED) as f32 / 1000.0
    }

    pub fn set_rate(&self, rate: f32) {
        self.rate_milli
            .store((rate * 1000.0).round() as u32, Self::RELAXED);
    }

    pub fn frozen(&self) -> bool {
        self.frozen.load(Self::RELAXED)
    }

    pub fn set_frozen(&self, frozen: bool) {
        self.frozen.store(frozen, Self::RELAXED);
        if !frozen {
            self.steps.store(0, Self::RELAXED);
        }
    }

    pub fn add_steps(&self, steps: u32) {
        self.steps.fetch_add(steps, Self::RELAXED);
    }

    /// Cancels pending steps; true when any were pending.
    pub fn stop_steps(&self) -> bool {
        self.steps.swap(0, Self::RELAXED) > 0
    }

    /// Called once per game tick: whether the game advances this tick (not frozen, or a step is
    /// due). Marks the tick as a stepping tick so region ticks run it.
    pub fn begin_game_tick(&self) -> bool {
        let run = if !self.frozen() {
            true
        } else {
            self.steps
                .try_update(Self::RELAXED, Self::RELAXED, |n| n.checked_sub(1))
                .is_ok()
        };
        self.stepping.store(run, Self::RELAXED);
        run
    }

    /// Whether world simulation runs: always unless frozen, and during a step.
    pub fn simulating(&self) -> bool {
        !self.frozen() || self.stepping.load(Self::RELAXED)
    }

    /// Folds one region tick duration into the running average.
    pub fn record_mspt(&self, ms: f64) {
        let old = self.mspt_micro.load(Self::RELAXED) as f64 / 1000.0;
        let next = if old == 0.0 {
            ms
        } else {
            old * 0.95 + ms * 0.05
        };
        self.mspt_micro.store((next * 1000.0) as u64, Self::RELAXED);
    }

    pub fn mspt(&self) -> f64 {
        self.mspt_micro.load(Self::RELAXED) as f64 / 1000.0
    }
}

/// Whether a client packet counts as the player doing something. Movement is judged from the
/// position the server records, and keep-alives, acknowledgements and chunk bookkeeping never
/// count.
pub fn counts_as_activity(packet: &ferrite_net::game::ServerboundPlay) -> bool {
    use ferrite_net::game::ServerboundPlay as P;
    matches!(
        packet,
        P::PlayerAction(_)
            | P::UseItemOn(_)
            | P::PlayerCommand(_)
            | P::PlayerInput(_)
            | P::SetHeldItem(_)
            | P::CreativeSlot(_)
            | P::ContainerClick(_)
            | P::ContainerClose(_)
            | P::ContainerButtonClick(_)
            | P::RenameItem(_)
            | P::SetBeacon(_)
            | P::SelectTrade(_)
            | P::Chat(_)
            | P::ChatCommand(_)
            | P::ChatCommandSigned(_)
            | P::UseItem(_)
            | P::Attack(_)
            | P::Interact(_)
            | P::SetCommandBlock(_)
    )
}

fn gamemasters() -> impl Fn(&Source) -> bool + Send + Sync + 'static {
    |source: &Source| source.permission >= 2
}

fn admins() -> impl Fn(&Source) -> bool + Send + Sync + 'static {
    |source: &Source| source.permission >= 3
}

fn owners() -> impl Fn(&Source) -> bool + Send + Sync + 'static {
    |source: &Source| source.permission >= 4
}

fn feedback(
    ctx: &CommandContext<Source>,
    key: &'static str,
    args: Vec<String>,
) -> Result<i32, CommandError> {
    send(ctx.source(), Action::Feedback(Feedback::new(key, args)))
}

fn failure(key: &'static str, args: Vec<String>) -> CommandError {
    CommandError::failed(Feedback::new(key, args).text)
}

// ---------------------------------------------------------------------------------------------
// /version
// ---------------------------------------------------------------------------------------------

static VERSION: OnceLock<serde_json::Value> = OnceLock::new();

static DIALOGS: OnceLock<Vec<String>> = OnceLock::new();

/// Installs the dialog registry order (the sorted file names under `data/minecraft/dialog`),
/// which is the order the configuration phase syncs the registry in, so an entry's position is
/// its protocol id.
pub fn load_dialogs(data: &std::path::Path) {
    fn walk(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<String>) {
        let Ok(items) = std::fs::read_dir(dir) else {
            return;
        };
        for item in items.flatten() {
            let path = item.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else if path.extension().is_some_and(|ext| ext == "json")
                && let Ok(relative) = path.strip_prefix(root)
            {
                out.push(format!(
                    "minecraft:{}",
                    relative
                        .with_extension("")
                        .to_string_lossy()
                        .replace('\\', "/")
                ));
            }
        }
    }
    let root = data.join("minecraft/dialog");
    let mut found = Vec::new();
    walk(&root, &root, &mut found);
    found.sort();
    let _ = DIALOGS.set(found);
}

/// Installs `reports/version.json` (copied from the user's jar by `tools/fetch_vanilla.py`).
pub fn load_version(reports: &std::path::Path) {
    let Ok(text) = std::fs::read_to_string(reports.join("version.json")) else {
        return;
    };
    if let Ok(value) = serde_json::from_str(&text) {
        let _ = VERSION.set(value);
    }
}

fn version(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let info = VERSION.get().ok_or_else(|| {
        CommandError::failed("version information is not loaded (python tools/fetch_vanilla.py)")
    })?;
    let text = |key: &str| {
        info.get(key)
            .map(|value| {
                value
                    .as_str()
                    .map_or_else(|| value.to_string(), str::to_owned)
            })
            .unwrap_or_default()
    };
    let pack = |key: &str| {
        let pack = info.get("pack_version");
        let part = |name: &str| {
            pack.and_then(|p| p.get(name))
                .map(ToString::to_string)
                .unwrap_or_default()
        };
        format!(
            "{}.{}",
            part(&format!("{key}_major")),
            part(&format!("{key}_minor"))
        )
    };
    let protocol = info
        .get("protocol_version")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or_default();
    let stable = info
        .get("stable")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    feedback(ctx, "commands.version.header", vec![])?;
    feedback(ctx, "commands.version.id", vec![text("id")])?;
    feedback(ctx, "commands.version.name", vec![text("name")])?;
    feedback(ctx, "commands.version.data", vec![text("world_version")])?;
    feedback(ctx, "commands.version.series", vec![text("series_id")])?;
    feedback(
        ctx,
        "commands.version.protocol",
        vec![protocol.to_string(), format!("{protocol:#x}")],
    )?;
    feedback(ctx, "commands.version.build_time", vec![text("build_time")])?;
    feedback(
        ctx,
        "commands.version.pack.resource",
        vec![pack("resource")],
    )?;
    feedback(ctx, "commands.version.pack.data", vec![pack("data")])?;
    feedback(
        ctx,
        if stable {
            "commands.version.stable.yes"
        } else {
            "commands.version.stable.no"
        },
        vec![],
    )?;
    Ok(1)
}

fn version_tree() -> NodeBuilder<Source> {
    literal::<Source>("version")
        .requires(gamemasters())
        .restricted(true)
        .executes(version)
}

// ---------------------------------------------------------------------------------------------
// /random
// ---------------------------------------------------------------------------------------------

/// SplitMix64: a small, well-mixed generator for command sequences.
fn next_random(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn hash_text(text: &str) -> u64 {
    // FNV-1a: stable across runs, which makes a seeded sequence repeatable.
    text.bytes().fold(0xCBF2_9CE4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01B3)
    })
}

fn sequence_seed(
    world_seed: i64,
    id: &str,
    seed: i64,
    include_world: bool,
    include_id: bool,
) -> u64 {
    let mut value = seed as u64;
    if include_world {
        value ^= (world_seed as u64).rotate_left(17);
    }
    if include_id {
        value ^= hash_text(id);
    }
    value
}

/// An `int_range` argument: `n`, `a..b`, `a..` or `..b`, as vanilla writes it.
fn parse_range(raw: &str) -> Option<(i32, i32)> {
    match raw.split_once("..") {
        None => raw.parse().ok().map(|n| (n, n)),
        Some((low, high)) => {
            let low = if low.is_empty() {
                None
            } else {
                Some(low.parse().ok()?)
            };
            let high = if high.is_empty() {
                None
            } else {
                Some(high.parse().ok()?)
            };
            match (low, high) {
                (None, None) => None,
                (Some(low), None) => Some((low, i32::MAX)),
                (None, Some(high)) => Some((i32::MIN, high)),
                (Some(low), Some(high)) => Some((low, high)),
            }
        }
    }
}

fn range_argument(ctx: &CommandContext<Source>) -> Result<(i32, i32), CommandError> {
    let raw = match ctx.argument("range") {
        Some(ferrite_command::ArgumentValue::Raw(raw)) => raw.clone(),
        _ => return Err(CommandError::failed("missing range argument 'range'")),
    };
    let (low, high) =
        parse_range(&raw).ok_or_else(|| CommandError::failed(format!("Invalid range '{raw}'")))?;
    let size = i64::from(high) - i64::from(low) + 1;
    if size < 1 {
        return Err(failure("commands.random.error.range_too_small", vec![]));
    }
    if size > i64::from(i32::MAX) {
        return Err(failure("commands.random.error.range_too_large", vec![]));
    }
    Ok((low, high))
}

fn random_roll(ctx: &CommandContext<Source>, announce: bool) -> Result<i32, CommandError> {
    let (low, high) = range_argument(ctx)?;
    let sequence = ctx.string("sequence").map(str::to_owned);
    let mut game = ctx.source().state.lock().unwrap();
    let seed = game.seed;
    let size = (i64::from(high) - i64::from(low) + 1) as u64;
    let draw = match &sequence {
        Some(id) => {
            let state = game
                .more
                .sequences
                .entry(id.clone())
                .or_insert_with(|| sequence_seed(seed, id, 0, true, true));
            next_random(state)
        }
        None => {
            // No sequence: a fresh draw from the clock, like vanilla's unseeded random.
            let mut state = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos() as u64);
            next_random(&mut state)
        }
    };
    drop(game);
    let value = (i64::from(low) + (draw % size) as i64) as i32;
    if announce {
        let who = ctx.source().name.clone().unwrap_or_else(|| "Server".into());
        send(
            ctx.source(),
            Action::Broadcast(Feedback::new(
                "commands.random.roll",
                vec![who, value.to_string(), low.to_string(), high.to_string()],
            )),
        )?;
    } else {
        feedback(
            ctx,
            "commands.random.sample.success",
            vec![value.to_string()],
        )?;
    }
    Ok(value)
}

fn random_reset(ctx: &CommandContext<Source>, all: bool) -> Result<i32, CommandError> {
    let seed = ctx
        .argument("seed")
        .and_then(ferrite_command::ArgumentValue::as_i32)
        .unwrap_or(0);
    let include_world = ctx
        .argument("includeWorldSeed")
        .and_then(ferrite_command::ArgumentValue::as_bool)
        .unwrap_or(true);
    let include_id = ctx
        .argument("includeSequenceId")
        .and_then(ferrite_command::ArgumentValue::as_bool)
        .unwrap_or(true);
    let mut game = ctx.source().state.lock().unwrap();
    let world_seed = game.seed;
    if all {
        let ids: Vec<String> = game.more.sequences.keys().cloned().collect();
        for id in &ids {
            let value = sequence_seed(world_seed, id, i64::from(seed), include_world, include_id);
            game.more.sequences.insert(id.clone(), value);
        }
        let count = ids.len();
        drop(game);
        feedback(
            ctx,
            "commands.random.reset.all.success",
            vec![count.to_string()],
        )?;
        Ok(count as i32)
    } else {
        let id = ctx
            .string("sequence")
            .map(str::to_owned)
            .ok_or_else(|| CommandError::failed("missing sequence"))?;
        let value = sequence_seed(world_seed, &id, i64::from(seed), include_world, include_id);
        game.more.sequences.insert(id.clone(), value);
        drop(game);
        feedback(ctx, "commands.random.reset.success", vec![id])?;
        Ok(1)
    }
}

fn int_range() -> ArgumentType {
    ArgumentType::opaque("minecraft:int_range")
}

fn sequence_arg() -> NodeBuilder<Source> {
    argument("sequence", ArgumentType::ResourceLocation)
}

fn random_tree() -> NodeBuilder<Source> {
    let seeded = |all: bool| {
        argument::<Source>("seed", ArgumentType::integer())
            .executes(move |ctx| random_reset(ctx, all))
            .then(
                argument::<Source>("includeWorldSeed", ArgumentType::bool())
                    .executes(move |ctx| random_reset(ctx, all))
                    .then(
                        argument::<Source>("includeSequenceId", ArgumentType::bool())
                            .executes(move |ctx| random_reset(ctx, all)),
                    ),
            )
    };
    literal::<Source>("random")
        .then(
            literal::<Source>("value").then(
                argument::<Source>("range", int_range())
                    .executes(|ctx| random_roll(ctx, false))
                    .then(
                        sequence_arg()
                            .requires(gamemasters())
                            .restricted(true)
                            .executes(|ctx| random_roll(ctx, false)),
                    ),
            ),
        )
        .then(
            literal::<Source>("roll").then(
                argument::<Source>("range", int_range())
                    .executes(|ctx| random_roll(ctx, true))
                    .then(
                        sequence_arg()
                            .requires(gamemasters())
                            .restricted(true)
                            .executes(|ctx| random_roll(ctx, true)),
                    ),
            ),
        )
        .then(
            literal::<Source>("reset")
                .requires(gamemasters())
                .restricted(true)
                .then(
                    literal::<Source>("*")
                        .executes(|ctx| random_reset(ctx, true))
                        .then(seeded(true)),
                )
                .then(
                    sequence_arg()
                        .executes(|ctx| random_reset(ctx, false))
                        .then(seeded(false)),
                ),
        )
}

// ---------------------------------------------------------------------------------------------
// /setidletimeout
// ---------------------------------------------------------------------------------------------

fn setidletimeout_tree() -> NodeBuilder<Source> {
    literal::<Source>("setidletimeout")
        .requires(admins())
        .restricted(true)
        .then(
            argument::<Source>("minutes", ArgumentType::integer_range(0, i32::MAX)).executes(
                |ctx| {
                    let minutes = ctx.integer("minutes").unwrap_or(0);
                    ctx.source().state.lock().unwrap().more.idle_timeout_minutes = minutes;
                    if minutes == 0 {
                        feedback(ctx, "commands.setidletimeout.success.disabled", vec![])?;
                    } else {
                        feedback(
                            ctx,
                            "commands.setidletimeout.success",
                            vec![minutes.to_string()],
                        )?;
                    }
                    Ok(minutes)
                },
            ),
        )
}

// ---------------------------------------------------------------------------------------------
// /teammsg, /tm
// ---------------------------------------------------------------------------------------------

fn teammsg(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let sender = ctx.source().uuid.ok_or_else(player_only)?;
    let message = ctx
        .string("message")
        .map(str::to_owned)
        .ok_or_else(|| CommandError::failed("missing message"))?;
    let (team_name, members, sender_name) = {
        let game = ctx.source().state.lock().unwrap();
        let player = game.players.iter().find(|p| p.uuid == sender);
        let team = player.and_then(|p| p.team.clone());
        let Some(team) = team else {
            drop(game);
            return Err(failure("commands.teammsg.failed.noteam", vec![]));
        };
        let state = game.teams.get(&team);
        (
            state.map_or(team.clone(), |t| t.display_name.clone()),
            state.map(|t| t.members.clone()).unwrap_or_default(),
            player.map(|p| p.name.clone()).unwrap_or_default(),
        )
    };
    let mut delivered = 0;
    for member in members {
        let (key, args) = if member == sender {
            (
                "chat.type.team.sent",
                vec![team_name.clone(), sender_name.clone(), message.clone()],
            )
        } else {
            (
                "chat.type.team.text",
                vec![team_name.clone(), sender_name.clone(), message.clone()],
            )
        };
        send(
            ctx.source(),
            Action::PlayerFeedback {
                uuid: member,
                feedback: Feedback::new(key, args),
            },
        )?;
        delivered += 1;
    }
    Ok(delivered)
}

fn teammsg_tree() -> NodeBuilder<Source> {
    literal::<Source>("teammsg")
        .then(argument::<Source>("message", ArgumentType::Message).executes(teammsg))
}

// ---------------------------------------------------------------------------------------------
// /transfer
// ---------------------------------------------------------------------------------------------

fn transfer(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let host = ctx
        .string("hostname")
        .map(str::to_owned)
        .ok_or_else(|| CommandError::failed("missing hostname"))?;
    let port = ctx.integer("port").unwrap_or(25565);
    let targets = if ctx.argument("players").is_some() {
        resolved_targets(ctx, "players")?
    } else {
        vec![ctx.source().uuid.ok_or_else(player_only)?]
    };
    if targets.is_empty() {
        return Err(failure("commands.transfer.error.no_players", vec![]));
    }
    let names: Vec<String> = {
        let game = ctx.source().state.lock().unwrap();
        targets
            .iter()
            .map(|uuid| {
                game.players
                    .iter()
                    .find(|p| p.uuid == *uuid)
                    .map_or_else(|| uuid.to_string(), |p| p.name.clone())
            })
            .collect()
    };
    send(
        ctx.source(),
        Action::Packets {
            targets: targets.clone(),
            packet: PlayerPacket::Transfer(Transfer {
                host: host.clone(),
                port: VarInt(port),
            }),
        },
    )?;
    if targets.len() == 1 {
        feedback(
            ctx,
            "commands.transfer.success.single",
            vec![names[0].clone(), host, port.to_string()],
        )?;
    } else {
        feedback(
            ctx,
            "commands.transfer.success.multiple",
            vec![targets.len().to_string(), host, port.to_string()],
        )?;
    }
    Ok(targets.len() as i32)
}

fn transfer_tree() -> NodeBuilder<Source> {
    literal::<Source>("transfer")
        .requires(admins())
        .restricted(true)
        .then(
            argument::<Source>("hostname", ArgumentType::string())
                .executes(transfer)
                .then(
                    argument::<Source>("port", ArgumentType::integer_range(1, 65535))
                        .executes(transfer)
                        .then(
                            argument::<Source>("players", ArgumentType::players())
                                .executes(transfer),
                        ),
                ),
        )
}

// ---------------------------------------------------------------------------------------------
// /stopwatch
// ---------------------------------------------------------------------------------------------

fn stopwatch_id(ctx: &CommandContext<Source>) -> Result<String, CommandError> {
    ctx.string("id")
        .map(|id| {
            if id.contains(':') {
                id.to_owned()
            } else {
                format!("minecraft:{id}")
            }
        })
        .ok_or_else(|| CommandError::failed("missing stopwatch id"))
}

fn stopwatch_tree() -> NodeBuilder<Source> {
    let id = || argument::<Source>("id", ArgumentType::ResourceLocation);
    literal::<Source>("stopwatch")
        .requires(gamemasters())
        .restricted(true)
        .then(literal::<Source>("create").then(id().executes(|ctx| {
            let name = stopwatch_id(ctx)?;
            let mut game = ctx.source().state.lock().unwrap();
            if game.more.stopwatches.contains_key(&name) {
                drop(game);
                return Err(failure("commands.stopwatch.already_exists", vec![name]));
            }
            let now = game.game_time;
            game.more.stopwatches.insert(name.clone(), now);
            drop(game);
            feedback(ctx, "commands.stopwatch.create.success", vec![name])
        })))
        .then(literal::<Source>("restart").then(id().executes(|ctx| {
            let name = stopwatch_id(ctx)?;
            let mut game = ctx.source().state.lock().unwrap();
            let now = game.game_time;
            match game.more.stopwatches.get_mut(&name) {
                Some(start) => *start = now,
                None => {
                    drop(game);
                    return Err(failure("commands.stopwatch.does_not_exist", vec![name]));
                }
            }
            drop(game);
            feedback(ctx, "commands.stopwatch.restart.success", vec![name])
        })))
        .then(literal::<Source>("remove").then(id().executes(|ctx| {
            let name = stopwatch_id(ctx)?;
            let mut game = ctx.source().state.lock().unwrap();
            if game.more.stopwatches.remove(&name).is_none() {
                drop(game);
                return Err(failure("commands.stopwatch.does_not_exist", vec![name]));
            }
            drop(game);
            feedback(ctx, "commands.stopwatch.remove.success", vec![name])
        })))
        .then(
            literal::<Source>("query").then(id().executes(|ctx| stopwatch_query(ctx, 1.0)).then(
                argument::<Source>("scale", ArgumentType::double()).executes(|ctx| {
                    let scale = ctx
                        .argument("scale")
                        .and_then(ferrite_command::ArgumentValue::as_f64)
                        .unwrap_or(1.0);
                    stopwatch_query(ctx, scale)
                }),
            )),
        )
}

fn stopwatch_query(ctx: &CommandContext<Source>, scale: f64) -> Result<i32, CommandError> {
    let name = stopwatch_id(ctx)?;
    let game = ctx.source().state.lock().unwrap();
    let Some(start) = game.more.stopwatches.get(&name).copied() else {
        drop(game);
        return Err(failure("commands.stopwatch.does_not_exist", vec![name]));
    };
    let ticks = (game.game_time - start).max(0);
    drop(game);
    // The stopwatch runs in game ticks: 20 per second.
    let seconds = ticks as f64 / 20.0;
    feedback(
        ctx,
        "commands.stopwatch.query",
        vec![name, format!("{:.3}", seconds)],
    )?;
    Ok((seconds * scale) as i32)
}

// ---------------------------------------------------------------------------------------------
// /publish, /unpublish
// ---------------------------------------------------------------------------------------------

/// A dedicated server is already open to the network; there is no single-player world to
/// publish, so vanilla's own failure message is the answer.
fn publish_unavailable(_ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    Err(failure("commands.publish.failed", vec![]))
}

fn publish_tree() -> NodeBuilder<Source> {
    literal::<Source>("publish")
        .requires(owners())
        .restricted(true)
        .executes(publish_unavailable)
        .then(
            argument::<Source>("allowCommands", ArgumentType::bool())
                .executes(publish_unavailable)
                .then(
                    argument::<Source>("port", ArgumentType::integer_range(0, 65535))
                        .executes(publish_unavailable),
                ),
        )
}

fn unpublish_tree() -> NodeBuilder<Source> {
    literal::<Source>("unpublish")
        .requires(owners())
        .restricted(true)
        .executes(publish_unavailable)
}

/// Adds every command above to the server tree.
fn tick_tree() -> NodeBuilder<Source> {
    fn state(ctx: &CommandContext<Source>) -> std::sync::Arc<TickControl> {
        ctx.source().state.lock().unwrap().more.tick.clone()
    }
    fn step(ctx: &CommandContext<Source>, ticks: i32) -> Result<i32, CommandError> {
        let control = state(ctx);
        if !control.frozen() {
            return Err(failure("commands.tick.step.fail", vec![]));
        }
        control.add_steps(ticks as u32);
        feedback(ctx, "commands.tick.step.success", vec![ticks.to_string()])?;
        Ok(1)
    }
    literal::<Source>("tick")
        .requires(gamemasters())
        .restricted(true)
        .then(literal::<Source>("query").executes(|ctx| {
            let control = state(ctx);
            let target = control.rate();
            let mspt = control.mspt();
            let status = if control.frozen() {
                "commands.tick.status.frozen"
            } else if mspt > 1000.0 / f64::from(target) {
                "commands.tick.status.lagging"
            } else {
                "commands.tick.status.running"
            };
            feedback(ctx, status, vec![])?;
            feedback(
                ctx,
                "commands.tick.query.rate.running",
                vec![
                    format!("{target:.1}"),
                    format!("{mspt:.1}"),
                    format!("{:.1}", 1000.0 / f64::from(target)),
                ],
            )?;
            Ok(target as i32)
        }))
        .then(literal::<Source>("freeze").executes(|ctx| {
            state(ctx).set_frozen(true);
            feedback(ctx, "commands.tick.status.frozen", vec![])?;
            Ok(1)
        }))
        .then(literal::<Source>("unfreeze").executes(|ctx| {
            state(ctx).set_frozen(false);
            feedback(ctx, "commands.tick.status.running", vec![])?;
            Ok(1)
        }))
        .then(
            literal::<Source>("rate").then(
                argument::<Source>(
                    "rate",
                    ArgumentType::Float {
                        min: 1.0,
                        max: 10_000.0,
                    },
                )
                .executes(|ctx| {
                    let rate = ctx
                        .argument("rate")
                        .and_then(|value| value.as_f32())
                        .unwrap_or(20.0);
                    state(ctx).set_rate(rate);
                    feedback(ctx, "commands.tick.rate.success", vec![format!("{rate}")])?;
                    Ok(rate as i32)
                }),
            ),
        )
        .then(
            literal::<Source>("step")
                .executes(|ctx| step(ctx, 1))
                .then(literal::<Source>("stop").executes(|ctx| {
                    if state(ctx).stop_steps() {
                        feedback(ctx, "commands.tick.step.stop.success", vec![])?;
                        Ok(1)
                    } else {
                        Err(failure("commands.tick.step.stop.fail", vec![]))
                    }
                }))
                .then(
                    argument::<Source>("time", ArgumentType::Time { min: 1 }).executes(|ctx| {
                        let ticks = ctx.integer("time").unwrap_or(1);
                        step(ctx, ticks)
                    }),
                ),
        )
}

/// `/attribute <target> <attribute> (get | base get|set|reset | modifier add|remove|value get)`.
fn attribute_tree() -> NodeBuilder<Source> {
    use super::attributes::Operation;
    use crate::game::commands::AttributeOp;
    fn run(ctx: &CommandContext<Source>, op: AttributeOp) -> Result<i32, CommandError> {
        let Some(ferrite_command::ArgumentValue::Resource { id, .. }) = ctx.argument("attribute")
        else {
            return Err(CommandError::failed("missing attribute"));
        };
        let attribute = if id.contains(':') {
            id.clone()
        } else {
            format!("minecraft:{id}")
        };
        let path = attribute.strip_prefix("minecraft:").unwrap_or(&attribute);
        if super::attributes::range_of(path).is_none() {
            return Err(failure(
                "argument.resource.not_found",
                vec![attribute.clone(), "minecraft:attribute".into()],
            ));
        }
        send(
            ctx.source(),
            Action::Attribute {
                targets: entity(ctx, "target")?,
                attribute,
                op,
            },
        )
    }
    fn modifier_id(ctx: &CommandContext<Source>) -> Result<String, CommandError> {
        match ctx.argument("id") {
            Some(ferrite_command::ArgumentValue::ResourceLocation(id)) => Ok(if id.contains(':') {
                id.clone()
            } else {
                format!("minecraft:{id}")
            }),
            _ => Err(CommandError::failed("missing modifier id")),
        }
    }
    fn amount(ctx: &CommandContext<Source>, operation: Operation) -> Result<i32, CommandError> {
        let id = modifier_id(ctx)?;
        let value = ctx
            .argument("value")
            .and_then(ferrite_command::ArgumentValue::as_f64)
            .unwrap_or(0.0);
        run(
            ctx,
            AttributeOp::AddModifier {
                id,
                amount: value,
                operation,
            },
        )
    }
    let scaled = |name: &'static str| {
        argument::<Source>("scale", ArgumentType::double()).executes(move |ctx| {
            let scale = ctx
                .argument("scale")
                .and_then(ferrite_command::ArgumentValue::as_f64)
                .unwrap_or(1.0);
            run(
                ctx,
                if name == "value" {
                    AttributeOp::GetValue { scale }
                } else {
                    AttributeOp::GetBase { scale }
                },
            )
        })
    };
    literal::<Source>("attribute")
        .requires(gamemasters())
        .restricted(true)
        .then(
            argument::<Source>("target", ArgumentType::entity()).then(
                argument::<Source>(
                    "attribute",
                    ArgumentType::Registry {
                        parser: ferrite_command::arguments::RegistryParser::Resource,
                        registry: "minecraft:attribute".into(),
                    },
                )
                .then(
                    literal::<Source>("get")
                        .executes(|ctx| run(ctx, AttributeOp::GetValue { scale: 1.0 }))
                        .then(scaled("value")),
                )
                .then(
                    literal::<Source>("base")
                        .then(
                            literal::<Source>("get")
                                .executes(|ctx| run(ctx, AttributeOp::GetBase { scale: 1.0 }))
                                .then(scaled("base")),
                        )
                        .then(literal::<Source>("set").then(
                            argument::<Source>("value", ArgumentType::double()).executes(|ctx| {
                                let value = ctx
                                    .argument("value")
                                    .and_then(ferrite_command::ArgumentValue::as_f64)
                                    .unwrap_or(0.0);
                                run(ctx, AttributeOp::SetBase(value))
                            }),
                        ))
                        .then(
                            literal::<Source>("reset")
                                .executes(|ctx| run(ctx, AttributeOp::ResetBase)),
                        ),
                )
                .then(
                    literal::<Source>("modifier")
                        .then(
                            literal::<Source>("add").then(
                                argument::<Source>("id", ArgumentType::ResourceLocation).then(
                                    argument::<Source>("value", ArgumentType::double())
                                        .then(literal::<Source>("add_multiplied_base").executes(
                                            move |ctx| amount(ctx, Operation::AddMultipliedBase),
                                        ))
                                        .then(literal::<Source>("add_multiplied_total").executes(
                                            move |ctx| amount(ctx, Operation::AddMultipliedTotal),
                                        ))
                                        .then(
                                            literal::<Source>("add_value").executes(move |ctx| {
                                                amount(ctx, Operation::AddValue)
                                            }),
                                        ),
                                ),
                            ),
                        )
                        .then(literal::<Source>("remove").then(
                            argument::<Source>("id", ArgumentType::ResourceLocation).executes(
                                move |ctx| {
                                    let id = modifier_id(ctx)?;
                                    run(ctx, AttributeOp::RemoveModifier { id })
                                },
                            ),
                        ))
                        .then(
                            literal::<Source>("value").then(
                                literal::<Source>("get").then(
                                    argument::<Source>("id", ArgumentType::ResourceLocation)
                                        .executes(move |ctx| {
                                            let id = modifier_id(ctx)?;
                                            run(ctx, AttributeOp::GetModifier { id, scale: 1.0 })
                                        })
                                        .then(
                                            argument::<Source>("scale", ArgumentType::double())
                                                .executes(move |ctx| {
                                                    let id = modifier_id(ctx)?;
                                                    let scale = ctx
                                                        .argument("scale")
                                                        .and_then(
                                                            ferrite_command::ArgumentValue::as_f64,
                                                        )
                                                        .unwrap_or(1.0);
                                                    run(ctx, AttributeOp::GetModifier { id, scale })
                                                }),
                                        ),
                                ),
                            ),
                        ),
                ),
            ),
        )
}

/// `/spectate [<target> [<player>]]`.
fn spectate_tree() -> NodeBuilder<Source> {
    fn spectate(ctx: &CommandContext<Source>, with_target: bool) -> Result<i32, CommandError> {
        let viewer = if ctx.argument("player").is_some() {
            resolved_targets(ctx, "player")?
                .first()
                .copied()
                .ok_or_else(|| failure("argument.entity.notfound.player", vec![]))?
        } else {
            ctx.source().uuid.ok_or_else(player_only)?
        };
        let target = if with_target {
            Some(entity(ctx, "target")?)
        } else {
            None
        };
        send(ctx.source(), Action::Spectate { viewer, target })
    }
    literal::<Source>("spectate")
        .requires(gamemasters())
        .restricted(true)
        .executes(|ctx| spectate(ctx, false))
        .then(
            argument::<Source>("target", ArgumentType::entity())
                .executes(|ctx| spectate(ctx, true))
                .then(
                    argument::<Source>("player", ArgumentType::player())
                        .executes(|ctx| spectate(ctx, true)),
                ),
        )
}

fn dialog_tree() -> NodeBuilder<Source> {
    fn targets(ctx: &CommandContext<Source>) -> Result<Vec<uuid::Uuid>, CommandError> {
        resolved_targets(ctx, "targets")
    }
    fn names(ctx: &CommandContext<Source>, players: &[uuid::Uuid]) -> Vec<String> {
        let game = ctx.source().state.lock().unwrap();
        players
            .iter()
            .map(|uuid| {
                game.players
                    .iter()
                    .find(|p| p.uuid == *uuid)
                    .map_or_else(|| uuid.to_string(), |p| p.name.clone())
            })
            .collect()
    }
    fn report(
        ctx: &CommandContext<Source>,
        players: &[uuid::Uuid],
        single: &'static str,
        multiple: &'static str,
    ) -> Result<i32, CommandError> {
        if let [_] = players {
            feedback(ctx, single, names(ctx, players))
        } else {
            feedback(ctx, multiple, vec![players.len().to_string()])
        }
    }
    literal::<Source>("dialog")
        .requires(gamemasters())
        .restricted(true)
        .then(literal::<Source>("clear").then(
            argument::<Source>("targets", ArgumentType::players()).executes(|ctx| {
                let players = targets(ctx)?;
                send(
                    ctx.source(),
                    Action::Packets {
                        targets: players.clone(),
                        packet: PlayerPacket::ClearDialog(
                            ferrite_protocol::packets::play::ClearDialog,
                        ),
                    },
                )?;
                report(
                    ctx,
                    &players,
                    "commands.dialog.clear.single",
                    "commands.dialog.clear.multiple",
                )
            }),
        ))
        .then(literal::<Source>("show").then(
            argument::<Source>("targets", ArgumentType::players()).then(
                argument::<Source>("dialog", ArgumentType::opaque("minecraft:dialog")).executes(
                    |ctx| {
                        let Some(ferrite_command::ArgumentValue::Raw(name)) =
                            ctx.argument("dialog")
                        else {
                            return Err(CommandError::failed("missing dialog"));
                        };
                        let id = if name.contains(':') {
                            name.clone()
                        } else {
                            format!("minecraft:{name}")
                        };
                        let Some(index) = DIALOGS
                            .get()
                            .and_then(|all| all.iter().position(|d| *d == id))
                        else {
                            return Err(failure(
                                "argument.resource.not_found",
                                vec![id, "minecraft:dialog".into()],
                            ));
                        };
                        let players = targets(ctx)?;
                        send(
                            ctx.source(),
                            Action::Packets {
                                targets: players.clone(),
                                packet: PlayerPacket::ShowDialog(
                                    ferrite_protocol::packets::play::ShowDialog {
                                        holder: VarInt(index as i32 + 1),
                                    },
                                ),
                            },
                        )?;
                        report(
                            ctx,
                            &players,
                            "commands.dialog.show.single",
                            "commands.dialog.show.multiple",
                        )
                    },
                ),
            ),
        ))
}

/// `/posteffect add|remove|clear|list`: the full effect list is resent after every change.
fn posteffect_tree() -> NodeBuilder<Source> {
    fn effect(ctx: &CommandContext<Source>) -> Result<String, CommandError> {
        match ctx.argument("posteffect") {
            Some(ferrite_command::ArgumentValue::ResourceLocation(id)) => Ok(if id.contains(':') {
                id.clone()
            } else {
                format!("minecraft:{id}")
            }),
            _ => Err(CommandError::failed("missing posteffect")),
        }
    }
    fn change(ctx: &CommandContext<Source>, add: Option<bool>) -> Result<i32, CommandError> {
        let players = resolved_targets(ctx, "targets")?;
        let wanted = if add.is_some() {
            Some(effect(ctx)?)
        } else {
            None
        };
        let mut changed = Vec::new();
        let mut names = Vec::new();
        {
            let mut game = ctx.source().state.lock().unwrap();
            for uuid in &players {
                let name = game
                    .players
                    .iter()
                    .find(|p| p.uuid == *uuid)
                    .map_or_else(|| uuid.to_string(), |p| p.name.clone());
                let list = game.more.post_effects.entry(*uuid).or_default();
                let did = match (&wanted, add) {
                    (Some(id), Some(true)) if !list.contains(id) => {
                        list.push(id.clone());
                        true
                    }
                    (Some(id), Some(false)) => {
                        let before = list.len();
                        list.retain(|e| e != id);
                        list.len() != before
                    }
                    (None, None) if !list.is_empty() => {
                        list.clear();
                        true
                    }
                    _ => false,
                };
                if did {
                    changed.push((*uuid, list.clone()));
                    names.push(name);
                }
            }
        }
        let (action, scope) = match add {
            Some(true) => ("add", "commands.posteffect.add"),
            Some(false) => ("remove", "commands.posteffect.remove"),
            None => ("clear", "commands.posteffect.clear"),
        };
        let _ = action;
        if changed.is_empty() {
            return Err(failure(
                match add {
                    Some(true) => "commands.posteffect.add.failed",
                    Some(false) => "commands.posteffect.remove.failed",
                    None => "commands.posteffect.clear.failed",
                },
                vec![],
            ));
        }
        for (uuid, effects) in &changed {
            send(
                ctx.source(),
                Action::Packets {
                    targets: vec![*uuid],
                    packet: PlayerPacket::PostEffects(
                        ferrite_protocol::packets::play::PostEffects {
                            effects: effects.clone(),
                        },
                    ),
                },
            )?;
        }
        let single = match scope {
            "commands.posteffect.add" => "commands.posteffect.add.success.single",
            "commands.posteffect.remove" => "commands.posteffect.remove.success.single",
            _ => "commands.posteffect.clear.success.single",
        };
        let multiple = match scope {
            "commands.posteffect.add" => "commands.posteffect.add.success.multiple",
            "commands.posteffect.remove" => "commands.posteffect.remove.success.multiple",
            _ => "commands.posteffect.clear.success.multiple",
        };
        let mut single_args = Vec::new();
        let mut multiple_args = Vec::new();
        if let Some(id) = &wanted {
            single_args.push(id.clone());
            multiple_args.push(id.clone());
        }
        if changed.len() == 1 {
            single_args.push(names[0].clone());
            feedback(ctx, single, single_args)
        } else {
            multiple_args.push(changed.len().to_string());
            feedback(ctx, multiple, multiple_args)
        }
    }
    literal::<Source>("posteffect")
        .requires(gamemasters())
        .restricted(true)
        .then(
            literal::<Source>("add").then(
                argument::<Source>("targets", ArgumentType::players()).then(
                    argument::<Source>("posteffect", ArgumentType::ResourceLocation)
                        .executes(|ctx| change(ctx, Some(true))),
                ),
            ),
        )
        .then(
            literal::<Source>("remove").then(
                argument::<Source>("targets", ArgumentType::players()).then(
                    argument::<Source>("posteffect", ArgumentType::ResourceLocation)
                        .executes(|ctx| change(ctx, Some(false))),
                ),
            ),
        )
        .then(
            literal::<Source>("clear").then(
                argument::<Source>("targets", ArgumentType::players())
                    .executes(|ctx| change(ctx, None)),
            ),
        )
        .then(literal::<Source>("list").then(
            argument::<Source>("target", ArgumentType::player()).executes(|ctx| {
                let players = resolved_targets(ctx, "target")?;
                let Some(uuid) = players.first() else {
                    return Err(failure("argument.entity.notfound.player", vec![]));
                };
                let game = ctx.source().state.lock().unwrap();
                let name = game
                    .players
                    .iter()
                    .find(|p| p.uuid == *uuid)
                    .map_or_else(|| uuid.to_string(), |p| p.name.clone());
                let list = game
                    .more
                    .post_effects
                    .get(uuid)
                    .cloned()
                    .unwrap_or_default();
                drop(game);
                if list.is_empty() {
                    feedback(ctx, "commands.posteffect.list.empty", vec![name])
                } else {
                    feedback(
                        ctx,
                        "commands.posteffect.list.success",
                        vec![name, list.len().to_string(), list.join(", ")],
                    )
                }
            }),
        ))
}

/// `/swing <targets> [mainhand|offhand [<animation> [<duration>]]]`. The animation and duration
/// only matter for the client's item-use pose, so they are validated and otherwise ignored.
fn swing_tree() -> NodeBuilder<Source> {
    fn swing(ctx: &CommandContext<Source>, off_hand: bool) -> Result<i32, CommandError> {
        if let Some(ferrite_command::ArgumentValue::Raw(name)) = ctx.argument("animation")
            && !["none", "whack", "stab"].contains(&name.to_ascii_lowercase().as_str())
        {
            return Err(failure(
                "argument.swing_animation.invalid",
                vec![name.clone()],
            ));
        }
        send(
            ctx.source(),
            Action::Swing {
                targets: entity(ctx, "targets")?,
                off_hand,
            },
        )
    }
    fn hand(name: &'static str, off_hand: bool) -> NodeBuilder<Source> {
        literal::<Source>(name)
            .executes(move |ctx| swing(ctx, off_hand))
            .then(
                argument::<Source>(
                    "animation",
                    ArgumentType::opaque("minecraft:swing_animation"),
                )
                .executes(move |ctx| swing(ctx, off_hand))
                .then(
                    argument::<Source>("duration", ArgumentType::Time { min: 1 })
                        .executes(move |ctx| swing(ctx, off_hand)),
                ),
            )
    }
    literal::<Source>("swing")
        .requires(gamemasters())
        .restricted(true)
        .then(
            argument::<Source>("targets", ArgumentType::entity())
                .executes(|ctx| swing(ctx, false))
                .then(hand("mainhand", false))
                .then(hand("offhand", true)),
        )
}

fn rotate_tree() -> NodeBuilder<Source> {
    fn rotate(ctx: &CommandContext<Source>, to: RotateTo) -> Result<i32, CommandError> {
        send(
            ctx.source(),
            Action::Rotate {
                targets: entity(ctx, "target")?,
                to,
            },
        )
    }
    let single = || argument::<Source>("target", ArgumentType::entity());
    literal::<Source>("rotate")
        .requires(gamemasters())
        .restricted(true)
        .then(
            single()
                .then(
                    literal::<Source>("facing")
                        .then(
                            literal::<Source>("entity").then(
                                argument::<Source>("facingEntity", ArgumentType::entity())
                                    .executes(|ctx| {
                                        rotate(
                                            ctx,
                                            RotateTo::FacingEntity {
                                                entity: entity(ctx, "facingEntity")?,
                                                anchor: ferrite_command::EntityAnchor::Feet,
                                            },
                                        )
                                    })
                                    .then(
                                        argument::<Source>(
                                            "facingAnchor",
                                            ArgumentType::EntityAnchor,
                                        )
                                        .executes(|ctx| {
                                            rotate(
                                                ctx,
                                                RotateTo::FacingEntity {
                                                    entity: entity(ctx, "facingEntity")?,
                                                    anchor: anchor(ctx, "facingAnchor")?,
                                                },
                                            )
                                        }),
                                    ),
                            ),
                        )
                        .then(
                            argument::<Source>("facingLocation", ArgumentType::Vec3).executes(
                                |ctx| {
                                    let source = ctx.source();
                                    let point =
                                        super::commands::coordinates(ctx, "facingLocation")?
                                            .resolve(source.origin, source.yaw, source.pitch);
                                    rotate(ctx, RotateTo::FacingLocation(point))
                                },
                            ),
                        ),
                )
                .then(
                    argument::<Source>("rotation", ArgumentType::Rotation)
                        .executes(|ctx| rotate(ctx, RotateTo::Angles(rotation(ctx, "rotation")?))),
                ),
        )
}

/// Largest area `/forceload add|remove` accepts, in chunks.
const FORCELOAD_MAX: i64 = 256;

fn chunk_label(x: i32, z: i32) -> String {
    format!("[{x}, {z}]")
}

fn column(ctx: &CommandContext<Source>, name: &str) -> Result<(i32, i32), CommandError> {
    match ctx.argument(name) {
        Some(ferrite_command::ArgumentValue::ColumnPos(pair)) => {
            let origin = ctx.source().origin;
            Ok((
                (pair[0].resolve(origin[0]).floor() as i32) >> 4,
                (pair[1].resolve(origin[2]).floor() as i32) >> 4,
            ))
        }
        _ => Err(CommandError::failed(format!("missing column {name}"))),
    }
}

fn forceload_change(
    ctx: &CommandContext<Source>,
    add: bool,
    to: bool,
) -> Result<i32, CommandError> {
    let from = column(ctx, "from")?;
    let to = if to { column(ctx, "to")? } else { from };
    let (x0, x1) = (from.0.min(to.0), from.0.max(to.0));
    let (z0, z1) = (from.1.min(to.1), from.1.max(to.1));
    let count = i64::from(x1 - x0 + 1) * i64::from(z1 - z0 + 1);
    if count > FORCELOAD_MAX {
        return Err(failure(
            "commands.forceload.toobig",
            vec![FORCELOAD_MAX.to_string(), count.to_string()],
        ));
    }
    let dimension = ctx.source().dimension;
    let key = dimension.key().to_owned();
    let mut changed = Vec::new();
    {
        let mut game = ctx.source().state.lock().unwrap();
        for x in x0..=x1 {
            for z in z0..=z1 {
                let entry = (key.clone(), x, z);
                let done = if add {
                    game.more.forced.insert(entry)
                } else {
                    game.more.forced.remove(&entry)
                };
                if done {
                    changed.push((x, z));
                }
            }
        }
    }
    if changed.is_empty() {
        return Err(failure(
            if add {
                "commands.forceload.added.failure"
            } else {
                "commands.forceload.removed.failure"
            },
            vec![],
        ));
    }
    let (add_list, remove_list) = if add {
        (changed.clone(), Vec::new())
    } else {
        (Vec::new(), changed.clone())
    };
    send(
        ctx.source(),
        Action::ForceLoad {
            dimension,
            add: add_list,
            remove: remove_list,
        },
    )?;
    if changed.len() == 1 {
        feedback(
            ctx,
            if add {
                "commands.forceload.added.single"
            } else {
                "commands.forceload.removed.single"
            },
            vec![chunk_label(changed[0].0, changed[0].1), key],
        )?;
    } else {
        feedback(
            ctx,
            if add {
                "commands.forceload.added.multiple"
            } else {
                "commands.forceload.removed.multiple"
            },
            vec![
                changed.len().to_string(),
                key,
                chunk_label(x0, z0),
                chunk_label(x1, z1),
            ],
        )?;
    }
    Ok(changed.len() as i32)
}

fn forceload_tree() -> NodeBuilder<Source> {
    let pos = |name: &'static str| argument::<Source>(name, ArgumentType::ColumnPos);
    literal::<Source>("forceload")
        .requires(gamemasters())
        .restricted(true)
        .then(
            literal::<Source>("add").then(
                pos("from")
                    .executes(|ctx| forceload_change(ctx, true, false))
                    .then(pos("to").executes(|ctx| forceload_change(ctx, true, true))),
            ),
        )
        .then(
            literal::<Source>("remove")
                .then(literal::<Source>("all").executes(|ctx| {
                    let dimension = ctx.source().dimension;
                    let key = dimension.key().to_owned();
                    let removed: Vec<(i32, i32)> = {
                        let mut game = ctx.source().state.lock().unwrap();
                        let all: Vec<_> = game
                            .more
                            .forced
                            .iter()
                            .filter(|(k, ..)| *k == key)
                            .cloned()
                            .collect();
                        for entry in &all {
                            game.more.forced.remove(entry);
                        }
                        all.into_iter().map(|(_, x, z)| (x, z)).collect()
                    };
                    let count = removed.len() as i32;
                    send(
                        ctx.source(),
                        Action::ForceLoad {
                            dimension,
                            add: Vec::new(),
                            remove: removed,
                        },
                    )?;
                    feedback(ctx, "commands.forceload.removed.all", vec![key])?;
                    Ok(count)
                }))
                .then(
                    pos("from")
                        .executes(|ctx| forceload_change(ctx, false, false))
                        .then(pos("to").executes(|ctx| forceload_change(ctx, false, true))),
                ),
        )
        .then(
            literal::<Source>("query")
                .executes(|ctx| {
                    let key = ctx.source().dimension.key().to_owned();
                    let chunks: Vec<String> = ctx
                        .source()
                        .state
                        .lock()
                        .unwrap()
                        .more
                        .forced
                        .iter()
                        .filter(|(k, ..)| *k == key)
                        .map(|(_, x, z)| chunk_label(*x, *z))
                        .collect();
                    match chunks.len() {
                        0 => Err(failure("commands.forceload.added.none", vec![key])),
                        1 => {
                            feedback(
                                ctx,
                                "commands.forceload.list.single",
                                vec![key, chunks[0].clone()],
                            )?;
                            Ok(1)
                        }
                        n => {
                            feedback(
                                ctx,
                                "commands.forceload.list.multiple",
                                vec![n.to_string(), key, chunks.join(", ")],
                            )?;
                            Ok(n as i32)
                        }
                    }
                })
                .then(pos("pos").executes(|ctx| {
                    let (x, z) = column(ctx, "pos")?;
                    let key = ctx.source().dimension.key().to_owned();
                    let forced = ctx.source().state.lock().unwrap().more.forced.contains(&(
                        key.clone(),
                        x,
                        z,
                    ));
                    if forced {
                        feedback(
                            ctx,
                            "commands.forceload.query.success",
                            vec![chunk_label(x, z), key],
                        )?;
                        Ok(1)
                    } else {
                        Err(failure(
                            "commands.forceload.query.failure",
                            vec![chunk_label(x, z), key],
                        ))
                    }
                })),
        )
}

fn damage_tree() -> NodeBuilder<Source> {
    fn damage(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
        let amount = ctx
            .argument("amount")
            .and_then(|value| value.as_f32())
            .unwrap_or(0.0);
        let damage_type = ctx
            .string("damageType")
            .unwrap_or("minecraft:generic")
            .to_owned();
        send(
            ctx.source(),
            Action::Damage {
                targets: entity(ctx, "target")?,
                amount,
                damage_type,
            },
        )?;
        Ok(amount as i32)
    }
    let damage_type = || {
        argument::<Source>(
            "damageType",
            ArgumentType::Registry {
                parser: ferrite_command::arguments::RegistryParser::Resource,
                registry: "minecraft:damage_type".into(),
            },
        )
        .executes(damage)
    };
    literal::<Source>("damage")
        .requires(gamemasters())
        .restricted(true)
        .then(
            argument::<Source>("target", ArgumentType::entity()).then(
                argument::<Source>(
                    "amount",
                    ArgumentType::Float {
                        min: 0.0,
                        max: f32::MAX,
                    },
                )
                .executes(damage)
                .then(
                    damage_type()
                        .then(literal::<Source>("at").then(
                            argument::<Source>("location", ArgumentType::Vec3).executes(damage),
                        ))
                        .then(
                            literal::<Source>("by").then(
                                argument::<Source>("entity", ArgumentType::entity())
                                    .executes(damage)
                                    .then(
                                        literal::<Source>("from").then(
                                            argument::<Source>("cause", ArgumentType::entity())
                                                .executes(damage),
                                        ),
                                    ),
                            ),
                        ),
                ),
            ),
        )
}

// ---------------------------------------------------------------------------------------------
// /debug, /perf, /jfr, /test, /fetchprofile
// ---------------------------------------------------------------------------------------------

/// Starts or stops a tick profiling run; both `/debug` and `/perf` measure game ticks.
fn profiling_run(
    ctx: &CommandContext<Source>,
    start: bool,
    perf: bool,
) -> Result<i32, CommandError> {
    let (running, already, missing, started, stopped) = if perf {
        (
            "perf",
            "commands.perf.alreadyRunning",
            "commands.perf.notRunning",
            "commands.perf.started",
            "commands.perf.stopped",
        )
    } else {
        (
            "debug",
            "commands.debug.alreadyRunning",
            "commands.debug.notRunning",
            "commands.debug.started",
            "commands.debug.stopped",
        )
    };
    let mut game = ctx.source().state.lock().unwrap();
    let now = game.game_time;
    let slot = game.more.profiling.get(running).copied();
    if start {
        if slot.is_some() {
            drop(game);
            return Err(failure(already, vec![]));
        }
        game.more
            .profiling
            .insert(running, (std::time::Instant::now(), now));
        drop(game);
        feedback(ctx, started, vec![])?;
        return Ok(1);
    }
    let Some((began, from)) = slot else {
        drop(game);
        return Err(failure(missing, vec![]));
    };
    game.more.profiling.remove(running);
    drop(game);
    let seconds = began.elapsed().as_secs_f64();
    let ticks = now - from;
    let per_second = if seconds > 0.0 {
        ticks as f64 / seconds
    } else {
        0.0
    };
    feedback(
        ctx,
        stopped,
        vec![
            format!("{seconds:.2}"),
            ticks.to_string(),
            format!("{per_second:.2}"),
        ],
    )?;
    Ok(ticks as i32)
}

fn debug_tree() -> NodeBuilder<Source> {
    literal::<Source>("debug")
        .requires(admins())
        .restricted(true)
        .then(literal::<Source>("start").executes(|ctx| profiling_run(ctx, true, false)))
        .then(literal::<Source>("stop").executes(|ctx| profiling_run(ctx, false, false)))
        .then(
            literal::<Source>("function").then(
                argument::<Source>("name", ArgumentType::opaque("minecraft:function"))
                    .executes(|_| Err(failure("commands.debug.function.traceFailed", vec![]))),
            ),
        )
}

fn perf_tree() -> NodeBuilder<Source> {
    literal::<Source>("perf")
        .requires(owners())
        .restricted(true)
        .then(literal::<Source>("start").executes(|ctx| profiling_run(ctx, true, true)))
        .then(literal::<Source>("stop").executes(|ctx| profiling_run(ctx, false, true)))
}

/// Ferrite is not a JVM process, so there is no flight recorder to start; the failure messages
/// are vanilla's.
fn jfr_tree() -> NodeBuilder<Source> {
    literal::<Source>("jfr")
        .requires(owners())
        .restricted(true)
        .then(
            literal::<Source>("start")
                .executes(|_| Err(failure("commands.jfr.start.failed", vec![]))),
        )
        .then(literal::<Source>("stop").executes(|_| {
            Err(failure(
                "commands.jfr.dump.failed",
                vec!["Ferrite does not run on the JVM".to_owned()],
            ))
        }))
}

type Run = fn(&CommandContext<Source>) -> Result<i32, CommandError>;

/// Adds the argument list under `node` so that every prefix of it is executable.
fn with_optional_args(
    node: NodeBuilder<Source>,
    args: Vec<(&'static str, ArgumentType)>,
    run: Run,
) -> NodeBuilder<Source> {
    let mut tail: Option<NodeBuilder<Source>> = None;
    for (name, ty) in args.into_iter().rev() {
        let mut current = argument::<Source>(name, ty).executes(run);
        if let Some(next) = tail.take() {
            current = current.then(next);
        }
        tail = Some(current);
    }
    match tail {
        Some(tail) => node.then(tail),
        None => node,
    }
}

/// The game-test framework only exists for vanilla's own test builds; with no test
/// instances loaded every `/test` form reports that there is nothing to do.
fn no_tests(_: &CommandContext<Source>) -> Result<i32, CommandError> {
    Err(failure("commands.test.no_tests", vec![]))
}

fn test_tree() -> NodeBuilder<Source> {
    let int = |name: &'static str| (name, ArgumentType::integer());
    let flag = |name: &'static str| (name, ArgumentType::Bool);
    let selector =
        || argument::<Source>("tests", ArgumentType::opaque("minecraft:resource_selector"));
    let mut root = literal::<Source>("test")
        .requires(gamemasters())
        .restricted(true);
    for name in [
        "clearthat",
        "clearthese",
        "resetclosest",
        "resetthat",
        "resetthese",
        "stop",
    ] {
        root = root.then(literal::<Source>(name).executes(no_tests));
    }
    let with_run = |node: NodeBuilder<Source>| node.executes(no_tests);
    root = root
        .then(with_optional_args(
            with_run(literal::<Source>("clearall")),
            vec![int("radius")],
            no_tests,
        ))
        .then(
            literal::<Source>("create").then(
                argument::<Source>("id", ArgumentType::ResourceLocation)
                    .executes(no_tests)
                    .then(
                        argument::<Source>("width", ArgumentType::integer())
                            .executes(no_tests)
                            .then(
                                argument::<Source>("height", ArgumentType::integer()).then(
                                    argument::<Source>("depth", ArgumentType::integer())
                                        .executes(no_tests),
                                ),
                            ),
                    ),
            ),
        )
        .then(literal::<Source>("locate").then(selector().executes(no_tests)))
        .then(with_optional_args(
            with_run(literal::<Source>("pos")),
            vec![("var", ArgumentType::string())],
            no_tests,
        ))
        .then(with_optional_args(
            with_run(literal::<Source>("runclosest")),
            vec![int("numberOfTimes"), flag("untilFailed")],
            no_tests,
        ))
        .then(with_optional_args(
            with_run(literal::<Source>("runthat")),
            vec![int("numberOfTimes"), flag("untilFailed")],
            no_tests,
        ))
        .then(with_optional_args(
            with_run(literal::<Source>("runthese")),
            vec![int("numberOfTimes"), flag("untilFailed")],
            no_tests,
        ))
        .then(literal::<Source>("verify").then(selector().executes(no_tests)))
        .then(literal::<Source>("runmultiple").then(with_optional_args(
            selector().executes(no_tests),
            vec![int("amount")],
            no_tests,
        )))
        .then(literal::<Source>("run").then(with_optional_args(
            selector().executes(no_tests),
            vec![
                int("numberOfTimes"),
                flag("untilFailed"),
                int("rotationSteps"),
                int("testsPerRow"),
            ],
            no_tests,
        )));
    root.then(with_optional_args(
        with_run(literal::<Source>("runfailed")),
        vec![
            int("numberOfTimes"),
            flag("untilFailed"),
            int("rotationSteps"),
            int("testsPerRow"),
        ],
        no_tests,
    ))
}

fn profile_text(name: &str, id: uuid::Uuid) -> String {
    format!("{{id:\"{id}\",name:\"{name}\"}}")
}

fn fetchprofile_tree() -> NodeBuilder<Source> {
    literal::<Source>("fetchprofile")
        .requires(gamemasters())
        .restricted(true)
        .then(literal::<Source>("entity").then(
            argument::<Source>("entity", ArgumentType::entity()).executes(|ctx| {
                let ids = resolved_targets(ctx, "entity")?;
                let Some(id) = ids.first().copied() else {
                    return Err(failure("argument.entity.notfound.player", vec![]));
                };
                let name = ctx
                    .source()
                    .state
                    .lock()
                    .unwrap()
                    .players
                    .iter()
                    .find(|player| player.uuid == id)
                    .map(|player| player.name.clone())
                    .unwrap_or_default();
                feedback(
                    ctx,
                    "commands.fetchprofile.entity.success",
                    vec![name.clone(), profile_text(&name, id)],
                )?;
                Ok(1)
            }),
        ))
        .then(literal::<Source>("id").then(
            argument::<Source>("id", ArgumentType::opaque("minecraft:uuid")).executes(|ctx| {
                let raw = ctx.string("id").unwrap_or_default().to_owned();
                let found = raw.parse::<uuid::Uuid>().ok().and_then(|id| {
                    ctx.source()
                        .state
                        .lock()
                        .unwrap()
                        .players
                        .iter()
                        .find(|player| player.uuid == id)
                        .map(|player| (player.name.clone(), id))
                });
                match found {
                    Some((name, id)) => {
                        feedback(
                            ctx,
                            "commands.fetchprofile.id.success",
                            vec![raw, profile_text(&name, id)],
                        )?;
                        Ok(1)
                    }
                    None => Err(failure("commands.fetchprofile.id.failure", vec![raw])),
                }
            }),
        ))
        .then(literal::<Source>("name").then(
            argument::<Source>("name", ArgumentType::string()).executes(|ctx| {
                let name = ctx.string("name").unwrap_or_default().to_owned();
                let found = ctx
                    .source()
                    .state
                    .lock()
                    .unwrap()
                    .players
                    .iter()
                    .find(|player| player.name.eq_ignore_ascii_case(&name))
                    .map(|player| (player.name.clone(), player.uuid));
                match found {
                    Some((exact, id)) => {
                        feedback(
                            ctx,
                            "commands.fetchprofile.name.success",
                            vec![exact.clone(), profile_text(&exact, id)],
                        )?;
                        Ok(1)
                    }
                    None => Err(failure("commands.fetchprofile.name.failure", vec![name])),
                }
            }),
        ))
}

// ---------------------------------------------------------------------------------------------
// /spreadplayers
// ---------------------------------------------------------------------------------------------

/// Places `count` points inside the square `center +- range` so that no two are closer than
/// `spread`. Random sampling with rejection; `None` when the area is too crowded.
pub(crate) fn spread_points(
    count: usize,
    center: (f64, f64),
    spread: f64,
    range: f64,
    mut seed: u64,
) -> Option<Vec<(f64, f64)>> {
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    let mut points: Vec<(f64, f64)> = Vec::with_capacity(count);
    let mut tries = 0usize;
    while points.len() < count {
        tries += 1;
        if tries > 5_000 + count * 2_000 {
            return None;
        }
        let candidate = (
            center.0 + (next() * 2.0 - 1.0) * range,
            center.1 + (next() * 2.0 - 1.0) * range,
        );
        if points
            .iter()
            .all(|p| (p.0 - candidate.0).hypot(p.1 - candidate.1) >= spread)
        {
            points.push(candidate);
        }
    }
    Some(points)
}

fn spreadplayers(ctx: &CommandContext<Source>, under: bool) -> Result<i32, CommandError> {
    let source = ctx.source();
    let center = match ctx.argument("center") {
        Some(ferrite_command::ArgumentValue::Vec2(pair)) => (
            pair[0].resolve(source.origin[0]),
            pair[1].resolve(source.origin[2]),
        ),
        _ => return Err(CommandError::failed("missing center")),
    };
    let float = |name: &str| {
        ctx.argument(name)
            .and_then(|value| value.as_f32())
            .map(f64::from)
            .unwrap_or(0.0)
    };
    send(
        source,
        Action::Spread {
            targets: entity(ctx, "targets")?,
            center,
            spread: float("spreadDistance"),
            range: float("maxRange"),
            respect_teams: ctx
                .argument("respectTeams")
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
            max_height: if under {
                ctx.integer("maxHeight")
            } else {
                None
            },
        },
    )
}

fn spreadplayers_tree() -> NodeBuilder<Source> {
    let targets = |under: bool| {
        argument::<Source>("respectTeams", ArgumentType::Bool).then(
            argument::<Source>("targets", ArgumentType::entities())
                .executes(move |ctx| spreadplayers(ctx, under)),
        )
    };
    literal::<Source>("spreadplayers")
        .requires(gamemasters())
        .restricted(true)
        .then(
            argument::<Source>("center", ArgumentType::Vec2).then(
                argument::<Source>(
                    "spreadDistance",
                    ArgumentType::Float {
                        min: 0.0,
                        max: f32::MAX,
                    },
                )
                .then(
                    argument::<Source>(
                        "maxRange",
                        ArgumentType::Float {
                            min: 1.0,
                            max: f32::MAX,
                        },
                    )
                    .then(targets(false))
                    .then(
                        literal::<Source>("under").then(
                            argument::<Source>("maxHeight", ArgumentType::integer())
                                .then(targets(true)),
                        ),
                    ),
                ),
            ),
        )
}

pub fn register(tree: &mut CommandDispatcher<Source>) {
    tree.register(version_tree());
    tree.register(random_tree());
    tree.register(setidletimeout_tree());
    let teammsg = tree.register(teammsg_tree());
    tree.register(literal::<Source>("tm").redirect(teammsg));
    tree.register(transfer_tree());
    tree.register(stopwatch_tree());
    tree.register(publish_tree());
    tree.register(unpublish_tree());
    tree.register(tick_tree());
    tree.register(rotate_tree());
    tree.register(swing_tree());
    tree.register(attribute_tree());
    tree.register(dialog_tree());
    tree.register(spectate_tree());
    tree.register(posteffect_tree());
    tree.register(forceload_tree());
    tree.register(damage_tree());
    tree.register(debug_tree());
    tree.register(perf_tree());
    tree.register(jfr_tree());
    tree.register(test_tree());
    tree.register(fetchprofile_tree());
    tree.register(spreadplayers_tree());
    super::commands_progress::register(tree);
    super::commands_late::register(tree);
    super::commands_loot::register(tree);
    super::commands_item::register(tree);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::commands::{GameState, dispatcher};
    use std::sync::{Arc, Mutex};
    use uuid::Uuid;

    fn run(
        input: &str,
        permission: u8,
        state: Arc<Mutex<GameState>>,
        uuid: Option<Uuid>,
    ) -> (Result<i32, CommandError>, Vec<Action>) {
        let tree = Arc::new(dispatcher());
        let (actions, mut received) = tokio::sync::mpsc::unbounded_channel();
        let source = Source {
            permission,
            origin: [0.0, 64.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            uuid,
            name: uuid.map(|_| "Alice".to_owned()),
            state,
            actions,
            anchor: ferrite_command::EntityAnchor::Feet,
            dimension: crate::game::Dimension::Overworld,
            world: None,
            tree: Some(tree.clone()),
            function_depth: 0,
        };
        let result = tree.execute(input, source);
        (
            result,
            std::iter::from_fn(|| received.try_recv().ok()).collect(),
        )
    }

    fn fresh() -> Arc<Mutex<GameState>> {
        Arc::new(Mutex::new(GameState::default()))
    }

    fn keys(actions: &[Action]) -> Vec<&'static str> {
        actions
            .iter()
            .filter_map(|action| match action {
                Action::Feedback(f) | Action::Broadcast(f) => Some(f.key),
                Action::PlayerFeedback { feedback, .. } => Some(feedback.key),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn every_command_here_is_in_vanilla_with_its_permission() {
        let tree = dispatcher();
        let roots: Vec<String> = tree
            .root_literals()
            .into_iter()
            .map(|(name, _)| name.to_owned())
            .collect();
        for name in [
            "version",
            "random",
            "setidletimeout",
            "teammsg",
            "tm",
            "transfer",
            "stopwatch",
            "publish",
            "unpublish",
            "tick",
            "rotate",
            "swing",
            "attribute",
            "dialog",
            "spectate",
            "posteffect",
            "forceload",
            "damage",
            "debug",
            "perf",
            "jfr",
            "test",
            "fetchprofile",
            "spreadplayers",
        ] {
            assert!(
                roots.iter().any(|root| root == name),
                "{name} is not registered"
            );
        }
        // /version needs gamemaster; /setidletimeout and /transfer need admin; /publish owner.
        assert!(run("version", 1, fresh(), None).0.is_err());
        assert!(run("setidletimeout 5", 2, fresh(), None).0.is_err());
        assert!(
            run("transfer example.com", 2, fresh(), Some(Uuid::nil()))
                .0
                .is_err()
        );
        assert!(run("publish", 3, fresh(), None).0.is_err());
    }

    #[test]
    fn random_value_stays_in_range_and_sequences_repeat_after_reset() {
        let state = fresh();
        for _ in 0..200 {
            let (result, _) = run("random value 3..7", 0, state.clone(), None);
            let value = result.unwrap();
            assert!((3..=7).contains(&value), "{value}");
        }
        let draw = |state: &Arc<Mutex<GameState>>| {
            run("random value 0..1000000 demo:a", 2, state.clone(), None)
                .0
                .unwrap()
        };
        let first = draw(&state);
        let second = draw(&state);
        run("random reset demo:a", 2, state.clone(), None)
            .0
            .unwrap();
        assert_eq!(draw(&state), first, "a reset sequence starts over");
        assert_ne!(first, second, "a sequence advances");
    }

    #[test]
    fn random_rejects_empty_and_huge_ranges() {
        let state = fresh();
        assert!(run("random value 5..1", 0, state.clone(), None).0.is_err());
        assert!(
            run(
                "random value -2147483648..2147483647",
                0,
                state.clone(),
                None
            )
            .0
            .is_err()
        );
        assert!(run("random value 1..2 demo:a", 0, state, None).0.is_err());
    }

    #[test]
    fn random_roll_is_announced_to_everyone() {
        let (result, actions) = run("random roll 1..6", 0, fresh(), None);
        assert!((1..=6).contains(&result.unwrap()));
        assert!(
            matches!(actions.as_slice(), [Action::Broadcast(f)] if f.key == "commands.random.roll")
        );
    }

    #[test]
    fn setidletimeout_is_stored_and_zero_disables() {
        let state = fresh();
        let (result, actions) = run("setidletimeout 10", 3, state.clone(), None);
        assert_eq!(result.unwrap(), 10);
        assert_eq!(keys(&actions), ["commands.setidletimeout.success"]);
        assert_eq!(state.lock().unwrap().more.idle_timeout_minutes, 10);
        let (_, actions) = run("setidletimeout 0", 3, state, None);
        assert_eq!(keys(&actions), ["commands.setidletimeout.success.disabled"]);
    }

    #[test]
    fn idle_players_are_reported_once_after_the_timeout_and_activity_resets_it() {
        let alice = Uuid::from_u128(1);
        let bob = Uuid::from_u128(2);
        let state = fresh();
        {
            let mut game = state.lock().unwrap();
            game.join(alice, "Alice".into(), 0, 20);
            game.join(bob, "Bob".into(), 0, 20);
        }
        // Disabled: nobody is ever idle.
        for _ in 0..5000 {
            state.lock().unwrap().tick(true);
        }
        assert!(state.lock().unwrap().take_idle_players().is_empty());

        run("setidletimeout 1", 3, state.clone(), None).0.unwrap();
        // One minute is 1200 ticks. Bob keeps acting, Alice does not.
        for tick in 0..1300 {
            let mut game = state.lock().unwrap();
            game.tick(true);
            if tick % 100 == 0 {
                game.touch(bob);
            }
        }
        let idle = state.lock().unwrap().take_idle_players();
        assert_eq!(idle, [alice]);
        assert!(
            state.lock().unwrap().take_idle_players().is_empty(),
            "reported once"
        );
    }

    #[test]
    fn moving_counts_as_activity_but_standing_still_does_not() {
        let alice = Uuid::from_u128(1);
        let state = fresh();
        state.lock().unwrap().join(alice, "Alice".into(), 0, 20);
        run("setidletimeout 1", 3, state.clone(), None).0.unwrap();
        for tick in 0..1300 {
            let mut game = state.lock().unwrap();
            game.tick(true);
            // The client keeps resending the same position.
            game.set_position(alice, [0.0, 64.0, 0.0], 0.0, 0.0);
            if tick == 1000 {
                game.set_position(alice, [1.0, 64.0, 0.0], 0.0, 0.0);
            }
        }
        assert!(
            state.lock().unwrap().take_idle_players().is_empty(),
            "a step at tick 1000 resets the clock"
        );
        for _ in 0..1300 {
            let mut game = state.lock().unwrap();
            game.tick(true);
            game.set_position(alice, [1.0, 64.0, 0.0], 0.0, 0.0);
        }
        assert_eq!(state.lock().unwrap().take_idle_players(), [alice]);
    }

    #[test]
    fn teammsg_reaches_every_member_and_needs_a_team() {
        let alice = Uuid::from_u128(1);
        let bob = Uuid::from_u128(2);
        let state = fresh();
        {
            let mut game = state.lock().unwrap();
            game.join(alice, "Alice".into(), 0, 20);
            game.join(bob, "Bob".into(), 0, 20);
        }
        assert!(
            run("teammsg hello", 0, state.clone(), Some(alice))
                .0
                .is_err()
        );
        run("team add red", 2, state.clone(), None).0.unwrap();
        run("team join red Alice", 2, state.clone(), None)
            .0
            .unwrap();
        run("team join red Bob", 2, state.clone(), None).0.unwrap();
        let (result, actions) = run("tm hello team", 0, state, Some(alice));
        assert_eq!(result.unwrap(), 2);
        let mut seen: Vec<_> = actions
            .iter()
            .filter_map(|a| match a {
                Action::PlayerFeedback { uuid, feedback } => Some((*uuid, feedback.key)),
                _ => None,
            })
            .collect();
        seen.sort();
        assert_eq!(
            seen,
            [(alice, "chat.type.team.sent"), (bob, "chat.type.team.text")]
        );
    }

    #[test]
    fn transfer_sends_the_packet_to_the_named_players() {
        let alice = Uuid::from_u128(1);
        let state = fresh();
        state.lock().unwrap().join(alice, "Alice".into(), 0, 20);
        let (result, actions) = run("transfer play.example.com 25570 Alice", 3, state, None);
        assert_eq!(result.unwrap(), 1);
        assert!(actions.iter().any(|a| matches!(
            a,
            Action::Packets { targets, packet: PlayerPacket::Transfer(t) }
                if targets == &[alice] && t.host == "play.example.com" && t.port.0 == 25570
        )));
        assert!(keys(&actions).contains(&"commands.transfer.success.single"));
    }

    #[test]
    fn transfer_without_players_needs_a_player_source() {
        let (result, _) = run("transfer play.example.com", 3, fresh(), None);
        assert!(result.is_err());
    }

    #[test]
    fn forceload_marks_queries_and_removes_chunks() {
        let state = fresh();
        let (result, actions) = run("forceload add 0 0 31 15", 2, state.clone(), None);
        assert_eq!(result.unwrap(), 2, "two chunks");
        assert!(matches!(
            &actions[0],
            Action::ForceLoad { add, remove, .. } if add == &[(0, 0), (1, 0)] && remove.is_empty()
        ));
        assert_eq!(keys(&actions[1..]), ["commands.forceload.added.multiple"]);
        assert!(run("forceload add 0 0", 2, state.clone(), None).0.is_err());
        assert!(
            run("forceload add 0 0 4000 4000", 2, state.clone(), None)
                .0
                .is_err()
        );
        assert_eq!(run("forceload query", 2, state.clone(), None).0.unwrap(), 2);
        assert!(run("forceload query 0 0", 2, state.clone(), None).0.is_ok());
        assert!(
            run("forceload query 100 100", 2, state.clone(), None)
                .0
                .is_err()
        );
        let (result, actions) = run("forceload remove 0 0", 2, state.clone(), None);
        assert_eq!(result.unwrap(), 1);
        assert!(matches!(
            &actions[0],
            Action::ForceLoad { remove, .. } if remove == &[(0, 0)]
        ));
        assert_eq!(
            run("forceload remove all", 2, state.clone(), None)
                .0
                .unwrap(),
            1
        );
        assert!(run("forceload remove all", 1, state, None).0.is_err());
    }

    #[test]
    fn damage_forms_parse_and_need_permission() {
        for input in [
            "damage @s 4",
            "damage @s 4 minecraft:fall",
            "damage @s 4 minecraft:fall at 1 2 3",
            "damage @s 4 minecraft:arrow by @p",
            "damage @s 4 minecraft:arrow by @p from @s",
        ] {
            let (result, actions) = run(input, 2, fresh(), None);
            assert!(result.is_ok(), "{input}: {result:?}");
            assert!(
                matches!(&actions[0], Action::Damage { amount, .. } if *amount == 4.0),
                "{input}"
            );
        }
        let (_, actions) = run("damage @s 2.5 fall", 2, fresh(), None);
        assert!(matches!(
            &actions[0],
            Action::Damage { damage_type, .. } if damage_type == "minecraft:fall" || damage_type == "fall"
        ));
        assert!(run("damage @s -1", 2, fresh(), None).0.is_err());
        assert!(run("damage @s 4", 1, fresh(), None).0.is_err());
    }

    #[test]
    fn debug_and_perf_runs_start_once_and_report_ticks() {
        let state = fresh();
        assert!(run("debug stop", 3, state.clone(), None).0.is_err());
        run("debug start", 3, state.clone(), None).0.unwrap();
        assert!(run("debug start", 3, state.clone(), None).0.is_err());
        for _ in 0..5 {
            state.lock().unwrap().tick(true);
        }
        let (result, actions) = run("debug stop", 3, state.clone(), None);
        assert_eq!(result.unwrap(), 5);
        assert_eq!(keys(&actions), ["commands.debug.stopped"]);
        run("perf start", 4, state.clone(), None).0.unwrap();
        assert!(run("perf start", 4, state.clone(), None).0.is_err());
        assert!(
            run("perf start", 3, state.clone(), None).0.is_err(),
            "owner only"
        );
        run("perf stop", 4, state.clone(), None).0.unwrap();
        assert!(run("jfr start", 4, state, None).0.is_err());
    }

    #[test]
    fn test_command_syntax_parses_and_reports_no_tests() {
        for input in [
            "test run mod:a",
            "test run mod:a 3 true 1 4",
            "test runfailed",
            "test runfailed 2 true",
            "test create demo:one 4 5 6",
            "test clearall 10",
            "test pos",
            "test pos var",
            "test stop",
        ] {
            let error = run(input, 2, fresh(), None).0.unwrap_err();
            assert!(
                !matches!(error, CommandError::Syntax(_)),
                "{input}: {error:?}"
            );
        }
    }

    #[test]
    fn spread_points_keep_their_distance_and_fail_when_crowded() {
        let points = spread_points(10, (0.0, 0.0), 20.0, 100.0, 7).unwrap();
        assert_eq!(points.len(), 10);
        for (i, a) in points.iter().enumerate() {
            assert!(a.0.abs() <= 100.0 && a.1.abs() <= 100.0);
            for b in &points[i + 1..] {
                assert!((a.0 - b.0).hypot(a.1 - b.1) >= 20.0);
            }
        }
        assert!(spread_points(50, (0.0, 0.0), 100.0, 5.0, 7).is_none());
    }

    #[test]
    fn spreadplayers_forms_produce_a_spread_action() {
        for (input, height) in [
            ("spreadplayers 0 0 10 50 false @e", None),
            ("spreadplayers ~ ~ 10 50 true @a", None),
            ("spreadplayers 0 0 10 50 under 80 true @a", Some(80)),
        ] {
            let (result, actions) = run(input, 2, fresh(), None);
            assert!(result.is_ok(), "{input}: {result:?}");
            assert!(
                matches!(&actions[0], Action::Spread { max_height, .. } if *max_height == height),
                "{input}: {actions:?}"
            );
        }
        assert!(
            run("spreadplayers 0 0 -1 50 false @a", 2, fresh(), None)
                .0
                .is_err()
        );
        assert!(
            run("spreadplayers 0 0 10 50 false @a", 1, fresh(), None)
                .0
                .is_err()
        );
    }

    #[test]
    fn rotate_forms_produce_rotate_actions() {
        use crate::game::commands::RotateTo;
        for (input, check) in [
            ("rotate @s 90 -10", 0),
            ("rotate @s ~10 ~", 0),
            ("rotate @s facing 1 2 3", 1),
            ("rotate @s facing entity @p", 2),
            ("rotate @s facing entity @p eyes", 2),
        ] {
            let (result, actions) = run(input, 2, fresh(), None);
            assert!(result.is_ok(), "{input}: {result:?}");
            let Some(Action::Rotate { to, .. }) = actions.first() else {
                panic!("{input}: {actions:?}");
            };
            let ok = match check {
                0 => matches!(to, RotateTo::Angles(_)),
                1 => matches!(to, RotateTo::FacingLocation(_)),
                _ => matches!(to, RotateTo::FacingEntity { .. }),
            };
            assert!(ok, "{input}: {to:?}");
        }
        assert!(run("rotate @s 1 2", 1, fresh(), None).0.is_err());
        assert!(run("rotate", 2, fresh(), None).0.is_err());
    }

    #[test]
    fn attribute_forms_parse_to_attribute_actions() {
        use crate::game::commands::AttributeOp;
        for input in [
            "attribute @s minecraft:max_health get",
            "attribute @s max_health get 2",
            "attribute @s minecraft:armor base get",
            "attribute @s armor base set 5",
            "attribute @s armor base reset",
            "attribute @s armor modifier add test:a 1.5 add_value",
            "attribute @s armor modifier add test:a 0.1 add_multiplied_total",
            "attribute @s armor modifier remove test:a",
            "attribute @s armor modifier value get test:a 10",
        ] {
            let (result, actions) = run(input, 2, fresh(), None);
            assert!(result.is_ok(), "{input}: {result:?}");
            let Some(Action::Attribute { attribute, op, .. }) = actions.first() else {
                panic!("{input}: {actions:?}");
            };
            assert!(attribute.starts_with("minecraft:"), "{attribute}");
            if input.ends_with("reset") {
                assert_eq!(*op, AttributeOp::ResetBase);
            }
        }
        assert!(
            run("attribute @s minecraft:nonsense get", 2, fresh(), None)
                .0
                .is_err()
        );
        assert!(run("attribute @s armor get", 1, fresh(), None).0.is_err());
    }

    #[test]
    fn spectate_forms_parse_and_need_a_player_source() {
        let (result, _) = run("spectate", 2, fresh(), None);
        assert!(result.is_err(), "the console has no camera");
        assert!(
            run("spectate @e @a", 2, fresh(), None).0.is_err(),
            "no such players online"
        );
        assert!(
            run("spectate @s", 1, fresh(), None).0.is_err(),
            "needs permission level 2"
        );
    }

    #[test]
    fn dialog_and_posteffect_forms_parse() {
        for input in [
            "dialog clear @a",
            "posteffect add @a minecraft:blur",
            "posteffect remove @a blur",
            "posteffect clear @a",
            "posteffect list @p",
        ] {
            let (result, _) = run(input, 2, fresh(), None);
            // Targets resolve against the (empty) test game, so only syntax errors matter.
            assert!(
                !format!("{result:?}").contains("Unknown or incomplete"),
                "{input}: {result:?}"
            );
        }
        assert!(run("posteffect add @a", 2, fresh(), None).0.is_err());
        assert!(run("dialog show @a", 2, fresh(), None).0.is_err());
        assert!(
            run("dialog clear @a", 1, fresh(), None).0.is_err(),
            "needs permission level 2"
        );
    }

    #[test]
    fn swing_picks_the_hand_and_validates_the_animation() {
        for (input, off) in [
            ("swing @s", false),
            ("swing @s mainhand", false),
            ("swing @s offhand", true),
            ("swing @s offhand stab 5", true),
            ("swing @s mainhand whack", false),
        ] {
            let (result, actions) = run(input, 2, fresh(), None);
            assert!(result.is_ok(), "{input}: {result:?}");
            assert!(
                matches!(actions.first(), Some(Action::Swing { off_hand, .. }) if *off_hand == off),
                "{input}: {actions:?}"
            );
        }
        let (bad, actions) = run("swing @s mainhand flail", 2, fresh(), None);
        assert!(bad.is_err());
        assert!(actions.is_empty());
        assert!(
            run("swing @s", 1, fresh(), None).0.is_err(),
            "needs permission level 2"
        );
    }

    #[test]
    fn tick_freezes_steps_and_changes_the_rate() {
        let state = fresh();
        let control = state.lock().unwrap().more.tick.clone();
        let (result, actions) = run("tick step", 2, state.clone(), None);
        assert!(result.is_err(), "stepping needs a frozen game");
        assert!(actions.is_empty());
        run("tick freeze", 2, state.clone(), None).0.unwrap();
        assert!(control.frozen() && !control.simulating());
        assert!(!control.begin_game_tick(), "a frozen game does not tick");
        let (_, actions) = run("tick step 2", 2, state.clone(), None);
        assert_eq!(keys(&actions), ["commands.tick.step.success"]);
        assert!(control.begin_game_tick() && control.simulating());
        assert!(control.begin_game_tick());
        assert!(!control.begin_game_tick() && !control.simulating());
        run("tick step 5", 2, state.clone(), None).0.unwrap();
        run("tick step stop", 2, state.clone(), None).0.unwrap();
        assert!(run("tick step stop", 2, state.clone(), None).0.is_err());
        run("tick unfreeze", 2, state.clone(), None).0.unwrap();
        assert!(control.begin_game_tick());
        run("tick rate 40", 2, state.clone(), None).0.unwrap();
        assert_eq!(control.rate(), 40.0);
        assert!(run("tick rate 0.5", 2, state.clone(), None).0.is_err());
        assert!(run("tick rate 40", 1, state.clone(), None).0.is_err());
        let (_, actions) = run("tick query", 2, state, None);
        assert_eq!(
            keys(&actions),
            [
                "commands.tick.status.running",
                "commands.tick.query.rate.running"
            ]
        );
    }

    #[test]
    fn stopwatch_counts_game_ticks() {
        let state = fresh();
        run("stopwatch create demo:lap", 2, state.clone(), None)
            .0
            .unwrap();
        assert!(
            run("stopwatch create demo:lap", 2, state.clone(), None)
                .0
                .is_err()
        );
        for _ in 0..40 {
            state.lock().unwrap().tick(true);
        }
        let (result, actions) = run("stopwatch query demo:lap", 2, state.clone(), None);
        assert_eq!(result.unwrap(), 2, "40 ticks is two seconds");
        assert_eq!(keys(&actions), ["commands.stopwatch.query"]);
        run("stopwatch restart demo:lap", 2, state.clone(), None)
            .0
            .unwrap();
        assert_eq!(
            run("stopwatch query demo:lap 20", 2, state.clone(), None)
                .0
                .unwrap(),
            0
        );
        run("stopwatch remove demo:lap", 2, state.clone(), None)
            .0
            .unwrap();
        assert!(run("stopwatch query demo:lap", 2, state, None).0.is_err());
    }

    #[test]
    fn publish_reports_vanillas_failure_on_a_dedicated_server() {
        assert!(run("publish", 4, fresh(), None).0.is_err());
        assert!(run("unpublish", 4, fresh(), None).0.is_err());
    }

    #[test]
    fn range_syntax_matches_vanilla() {
        assert_eq!(parse_range("5"), Some((5, 5)));
        assert_eq!(parse_range("1..6"), Some((1, 6)));
        assert_eq!(parse_range("3.."), Some((3, i32::MAX)));
        assert_eq!(parse_range("..9"), Some((i32::MIN, 9)));
        assert_eq!(parse_range(".."), None);
        assert_eq!(parse_range("x"), None);
    }
}
