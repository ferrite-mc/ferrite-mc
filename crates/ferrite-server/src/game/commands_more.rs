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
