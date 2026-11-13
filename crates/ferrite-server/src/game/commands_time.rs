//! `/time`: the 26.3 world-clock tree (`commands.json`): `set|add|query|pause|resume|rate` on the
//! executor dimension's default clock, the same under `time of <clock>`, `set <timemarker>` and
//! `query timeline`.
//!
//! Clocks come from `data/minecraft/world_clock/*.json` (`overworld`, `the_end`), time markers and
//! periods from `data/minecraft/timeline/*.json`. State lives in [`GameState`]: the overworld
//! clock is `clock_time`/`clock_paused`/`clock_rate`, the End clock is `end_clock`.
//! What cannot be checked against vanilla data is listed in `docs/COMMANDS-STATUS.md`.

use std::path::{Path, PathBuf};

use ferrite_command::arguments::RegistryParser;
use ferrite_command::{
    ArgumentType, CommandContext, CommandDispatcher, CommandError, CommandSyntaxError, argument,
    literal, tree::NodeBuilder,
};
use serde_json::Value;

use super::Dimension;
use super::commands::{Action, Feedback, GameState, Source, send};

const OVERWORLD: &str = "minecraft:overworld";
const THE_END: &str = "minecraft:the_end";

/// One world clock that is not the overworld's (`GameState::clock_time` is the overworld).
#[derive(Debug, Clone, PartialEq)]
pub struct ClockState {
    pub time: i64,
    pub paused: bool,
    pub rate: f32,
    pub frac: f64,
}

impl Default for ClockState {
    fn default() -> Self {
        Self {
            time: 0,
            paused: false,
            rate: 1.0,
            frac: 0.0,
        }
    }
}

impl ClockState {
    /// Advances one game tick at the clock's rate.
    pub fn advance(&mut self) {
        if self.paused {
            return;
        }
        self.frac += f64::from(self.rate);
        let whole = self.frac.floor();
        self.time = self.time.saturating_add(whole as i64);
        self.frac -= whole;
    }
}

fn gamemasters(source: &Source) -> bool {
    source.permission >= 2
}

fn error(key: &'static str, args: Vec<String>, text: String) -> CommandError {
    CommandError::Syntax(CommandSyntaxError::new(key, args, text))
}

fn data_dir(kind: &str) -> Option<PathBuf> {
    let relative = format!("vanilla/26.3/generated/data/minecraft/{kind}");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    [PathBuf::from(&relative), root.join(&relative)]
        .into_iter()
        .find(|p| p.is_dir())
}

fn namespaced(raw: &str) -> String {
    if raw.contains(':') {
        raw.to_owned()
    } else {
        format!("minecraft:{raw}")
    }
}

/// `(id, json)` for every file of a data-pack registry directory.
fn registry(kind: &str) -> Vec<(String, Value)> {
    let Some(dir) = data_dir(kind) else {
        return Vec::new();
    };
    let mut out: Vec<(String, Value)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_stem()?.to_str()?.to_owned();
            if path.extension()? != "json" {
                return None;
            }
            let json = serde_json::from_str(&std::fs::read_to_string(&path).ok()?).ok()?;
            Some((format!("minecraft:{name}"), json))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The default clock of a dimension (`default_clock` in the dimension type); the Nether has none.
fn default_clock(dimension: Dimension) -> Option<&'static str> {
    match dimension {
        Dimension::Overworld => Some(OVERWORLD),
        Dimension::End => Some(THE_END),
        Dimension::Nether => None,
    }
}

fn dimension_name(dimension: Dimension) -> &'static str {
    match dimension {
        Dimension::Overworld => "minecraft:overworld",
        Dimension::Nether => "minecraft:the_nether",
        Dimension::End => "minecraft:the_end",
    }
}

/// The clock this invocation addresses: `of <clock>` or the executor dimension's default.
fn clock_of(ctx: &CommandContext<Source>) -> Result<String, CommandError> {
    if let Some(raw) = ctx.string("clock") {
        let id = namespaced(raw);
        if registry("world_clock").iter().any(|(name, _)| *name == id) {
            return Ok(id);
        }
        return Err(CommandError::failed(format!(
            "Unknown element '{id}' of type 'minecraft:world_clock'"
        )));
    }
    let dimension = ctx.source().dimension;
    default_clock(dimension).map(str::to_owned).ok_or_else(|| {
        let name = dimension_name(dimension).to_owned();
        error(
            "commands.time.no_default_clock",
            vec![name.clone()],
            format!("There is no default clock in dimension {name}"),
        )
    })
}

struct Clock<'a> {
    time: &'a mut i64,
    paused: &'a mut bool,
    rate: &'a mut f32,
}

fn clock_mut<'a>(state: &'a mut GameState, id: &str) -> Clock<'a> {
    if id == THE_END {
        Clock {
            time: &mut state.end_clock.time,
            paused: &mut state.end_clock.paused,
            rate: &mut state.end_clock.rate,
        }
    } else {
        Clock {
            time: &mut state.clock_time,
            paused: &mut state.clock_paused,
            rate: &mut state.clock_rate,
        }
    }
}

fn notify(source: &Source, key: &'static str, args: Vec<String>) -> Result<(), CommandError> {
    send(source, Action::Feedback(Feedback::new(key, args))).map(|_| ())
}

fn set_ticks(ctx: &CommandContext<Source>, total: i64) -> Result<i32, CommandError> {
    let clock = clock_of(ctx)?;
    let source = ctx.source();
    {
        let mut state = source.state.lock().unwrap();
        let c = clock_mut(&mut state, &clock);
        if *c.time == total {
            return Err(error(
                "commands.time.set.already_at_time",
                vec![clock.clone(), total.to_string()],
                format!("Clock {clock} is already set to {total} tick(s)"),
            ));
        }
        *c.time = total;
    }
    if clock == OVERWORLD {
        send(source, Action::TimeSet(total as i32))?;
    }
    notify(
        source,
        "commands.time.set.absolute",
        vec![clock, total.to_string()],
    )?;
    Ok(total as i32)
}

fn time_set(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let ticks = ctx
        .integer("time")
        .ok_or_else(|| CommandError::failed("missing time"))?;
    set_ticks(ctx, i64::from(ticks))
}

/// The marker `id` of the timelines driven by `clock`: `(period, ticks)`.
fn time_marker(clock: &str, marker: &str) -> Option<(i64, i64)> {
    registry("timeline").into_iter().find_map(|(_, timeline)| {
        if timeline["clock"].as_str() != Some(clock) {
            return None;
        }
        let period = timeline["period_ticks"].as_i64()?;
        let entry = timeline["time_markers"].get(marker)?;
        let ticks = entry.as_i64().or_else(|| entry["ticks"].as_i64())?;
        Some((period, ticks))
    })
}

fn time_set_marker(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let clock = clock_of(ctx)?;
    let marker = namespaced(ctx.string("timemarker").unwrap_or_default());
    let Some((period, ticks)) = time_marker(&clock, &marker) else {
        return Err(error(
            "commands.time.no_time_marker_found",
            vec![marker.clone(), clock.clone()],
            format!("Time marker {marker} does not exist for clock {clock}"),
        ));
    };
    let source = ctx.source();
    let total = {
        let mut state = source.state.lock().unwrap();
        let c = clock_mut(&mut state, &clock);
        // Advance to the next occurrence of the marker (inferred; see COMMANDS-STATUS.md).
        let ahead = (ticks - c.time.rem_euclid(period)).rem_euclid(period);
        if ahead == 0 {
            return Err(error(
                "commands.time.set.already_at_time_marker",
                vec![clock.clone(), marker.clone()],
                format!("Clock {clock} is already at time marker {marker}"),
            ));
        }
        *c.time += ahead;
        *c.time
    };
    if clock == OVERWORLD {
        send(source, Action::TimeSet(total as i32))?;
    }
    notify(source, "commands.time.set.time_marker", vec![clock, marker])?;
    Ok(total as i32)
}

fn time_add(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let clock = clock_of(ctx)?;
    let delta = ctx
        .integer("time")
        .ok_or_else(|| CommandError::failed("missing time"))?;
    let source = ctx.source();
    let total = {
        let mut state = source.state.lock().unwrap();
        let c = clock_mut(&mut state, &clock);
        *c.time = c.time.saturating_add(i64::from(delta));
        *c.time
    };
    if clock == OVERWORLD {
        send(source, Action::TimeAdd(delta))?;
    }
    notify(
        source,
        "commands.time.set.absolute",
        vec![clock, total.to_string()],
    )?;
    Ok(total as i32)
}

fn time_pause(ctx: &CommandContext<Source>, pause: bool) -> Result<i32, CommandError> {
    let clock = clock_of(ctx)?;
    let source = ctx.source();
    {
        let mut state = source.state.lock().unwrap();
        let c = clock_mut(&mut state, &clock);
        if *c.paused == pause {
            let (key, text) = if pause {
                (
                    "commands.time.pause.already_paused",
                    format!("Clock {clock} is already paused"),
                )
            } else {
                (
                    "commands.time.pause.already_running",
                    format!("Clock {clock} is already running"),
                )
            };
            return Err(error(key, vec![clock], text));
        }
        *c.paused = pause;
    }
    let key = if pause {
        "commands.time.pause"
    } else {
        "commands.time.resume"
    };
    notify(source, key, vec![clock])?;
    Ok(1)
}

fn time_rate(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let clock = clock_of(ctx)?;
    let rate = ctx
        .argument("rate")
        .and_then(ferrite_command::ArgumentValue::as_f32)
        .ok_or_else(|| CommandError::failed("missing rate"))?;
    let shown = ferrite_command::error::java_float(rate);
    let source = ctx.source();
    {
        let mut state = source.state.lock().unwrap();
        let c = clock_mut(&mut state, &clock);
        if (*c.rate - rate).abs() < f32::EPSILON {
            return Err(error(
                "commands.time.rate.already_same",
                vec![clock.clone(), shown.clone()],
                format!("Clock {clock} is already advancing at {shown}x normal rate"),
            ));
        }
        *c.rate = rate;
    }
    notify(source, "commands.time.rate", vec![clock, shown])?;
    Ok(rate as i32)
}

fn time_query(ctx: &CommandContext<Source>, game_time: bool) -> Result<i32, CommandError> {
    let source = ctx.source();
    if game_time {
        let ticks = source.state.lock().unwrap().game_time;
        notify(
            source,
            "commands.time.query.gametime",
            vec![ticks.to_string()],
        )?;
        return Ok(ticks as i32);
    }
    let clock = clock_of(ctx)?;
    let ticks = {
        let mut state = source.state.lock().unwrap();
        *clock_mut(&mut state, &clock).time
    };
    notify(
        source,
        "commands.time.query.absolute",
        vec![clock, ticks.to_string()],
    )?;
    Ok(ticks as i32)
}

fn time_query_timeline(
    ctx: &CommandContext<Source>,
    repetition: bool,
) -> Result<i32, CommandError> {
    let clock = clock_of(ctx)?;
    let id = namespaced(ctx.string("timeline").unwrap_or_default());
    let timelines = registry("timeline");
    let Some((_, timeline)) = timelines.iter().find(|(name, _)| *name == id) else {
        return Err(CommandError::failed(format!(
            "Unknown element '{id}' of type 'minecraft:timeline'"
        )));
    };
    if timeline["clock"].as_str() != Some(clock.as_str()) {
        return Err(error(
            "commands.time.wrong_timeline_for_clock",
            vec![id.clone(), clock.clone()],
            format!("Timeline {id} is not valid for clock {clock}"),
        ));
    }
    let source = ctx.source();
    let total = {
        let mut state = source.state.lock().unwrap();
        *clock_mut(&mut state, &clock).time
    };
    // A timeline without `period_ticks` does not repeat: its position is the clock itself.
    let (ticks, repetitions) = match timeline["period_ticks"].as_i64() {
        Some(period) if period > 0 => (total.rem_euclid(period), total.div_euclid(period)),
        _ => (total, 0),
    };
    if repetition {
        notify(
            source,
            "commands.time.query.timeline.repetitions",
            vec![id, repetitions.to_string()],
        )?;
        Ok(repetitions as i32)
    } else {
        notify(
            source,
            "commands.time.query.timeline",
            vec![id, ticks.to_string()],
        )?;
        Ok(ticks as i32)
    }
}

/// The subcommands shared by the default clock and `of <clock>`.
fn clock_commands(with_gametime: bool) -> Vec<NodeBuilder<Source>> {
    let resource = |name: &str, registry: &str| {
        argument::<Source>(
            name,
            ArgumentType::Registry {
                parser: RegistryParser::Resource,
                registry: registry.to_owned(),
            },
        )
    };
    let mut query = literal::<Source>("query")
        .then(literal("time").executes(|ctx| time_query(ctx, false)))
        .then(
            resource("timeline", "minecraft:timeline")
                .executes(|ctx| time_query_timeline(ctx, false))
                .then(literal("repetition").executes(|ctx| time_query_timeline(ctx, true))),
        );
    if with_gametime {
        query = query.then(literal("gametime").executes(|ctx| time_query(ctx, true)));
    }
    vec![
        literal::<Source>("set")
            .then(argument("time", ArgumentType::Time { min: 0 }).executes(time_set))
            .then(argument("timemarker", ArgumentType::ResourceLocation).executes(time_set_marker)),
        literal::<Source>("add")
            .then(argument("time", ArgumentType::Time { min: i32::MIN }).executes(time_add)),
        query,
        literal::<Source>("pause").executes(|ctx| time_pause(ctx, true)),
        literal::<Source>("resume").executes(|ctx| time_pause(ctx, false)),
        literal::<Source>("rate").then(
            argument(
                "rate",
                ArgumentType::Float {
                    min: 1.0e-5,
                    max: 1000.0,
                },
            )
            .executes(time_rate),
        ),
    ]
}

pub fn register(tree: &mut CommandDispatcher<Source>) {
    let mut of_clock = argument::<Source>(
        "clock",
        ArgumentType::Registry {
            parser: RegistryParser::Resource,
            registry: "minecraft:world_clock".to_owned(),
        },
    );
    for node in clock_commands(false) {
        of_clock = of_clock.then(node);
    }
    let mut time = literal::<Source>("time")
        .requires(gamemasters)
        .then(literal("of").then(of_clock));
    for node in clock_commands(true) {
        time = time.then(node);
    }
    tree.register(time);
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

    use super::*;
    use crate::game::commands::dispatcher;

    fn run_in(
        input: &str,
        state: &Arc<Mutex<GameState>>,
        dimension: Dimension,
    ) -> (Result<i32, CommandError>, Vec<Action>) {
        let (actions, mut received): (_, UnboundedReceiver<Action>) = unbounded_channel();
        let source = Source {
            permission: 2,
            origin: [0.0, 64.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            uuid: None,
            name: None,
            state: state.clone(),
            actions,
            anchor: ferrite_command::EntityAnchor::Feet,
            dimension,
            world: None,
            admin: None,
            tree: None,
            function_depth: 0,
        };
        let result = dispatcher().execute(input, source);
        (
            result,
            std::iter::from_fn(|| received.try_recv().ok()).collect(),
        )
    }

    fn run(input: &str, state: &Arc<Mutex<GameState>>) -> (Result<i32, CommandError>, Vec<Action>) {
        run_in(input, state, Dimension::Overworld)
    }

    fn key(actions: &[Action]) -> &str {
        actions
            .iter()
            .find_map(|a| match a {
                Action::Feedback(f) => Some(f.key),
                _ => None,
            })
            .unwrap_or("")
    }

    fn fresh() -> Arc<Mutex<GameState>> {
        Arc::new(Mutex::new(GameState::default()))
    }

    #[test]
    fn set_time_marker_moves_to_the_next_occurrence() {
        let state = fresh();
        // noon = 6000 in the `day` timeline (period 24000).
        let (result, actions) = run("time set noon", &state);
        assert_eq!(result.unwrap(), 6000, "{actions:?}");
        assert_eq!(key(&actions), "commands.time.set.time_marker");
        assert_eq!(state.lock().unwrap().clock_time, 6000);
        let (result, _) = run("time set minecraft:noon", &state);
        let text = result.unwrap_err().to_string();
        assert!(text.contains("already at time marker"), "{text}");
        // `day` (1000) already passed in this cycle: next day at 24000 + 1000.
        run("time set day", &state).0.unwrap();
        assert_eq!(state.lock().unwrap().clock_time, 25_000);
        let text = run("time set nope", &state).0.unwrap_err().to_string();
        assert!(text.contains("does not exist for clock"), "{text}");
    }

    #[test]
    fn pause_resume_and_rate_drive_the_clock() {
        let state = fresh();
        let (result, actions) = run("time pause", &state);
        assert!(result.is_ok());
        assert_eq!(key(&actions), "commands.time.pause");
        assert!(run("time pause", &state).0.is_err(), "already paused");
        state.lock().unwrap().tick(true);
        assert_eq!(state.lock().unwrap().clock_time, 0, "paused clock holds");
        let (result, actions) = run("time resume", &state);
        assert!(result.is_ok());
        assert_eq!(key(&actions), "commands.time.resume");
        assert!(run("time resume", &state).0.is_err(), "already running");
        let (result, actions) = run("time rate 2", &state);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(key(&actions), "commands.time.rate");
        assert!(run("time rate 2", &state).0.is_err(), "already 2x");
        state.lock().unwrap().tick(true);
        state.lock().unwrap().tick(true);
        assert_eq!(state.lock().unwrap().clock_time, 4);
        // 0.5x advances one tick every second game tick.
        run("time rate 0.5", &state).0.unwrap();
        for _ in 0..4 {
            state.lock().unwrap().tick(true);
        }
        assert_eq!(state.lock().unwrap().clock_time, 6);
        assert!(run("time rate 0", &state).0.is_err(), "min 1e-5");
        assert!(run("time rate 1001", &state).0.is_err(), "max 1000");
    }

    #[test]
    fn of_clock_addresses_the_end_clock_and_default_clock_follows_the_dimension() {
        let state = fresh();
        let (result, _) = run("time of minecraft:the_end set 77", &state);
        assert_eq!(result.unwrap(), 77);
        assert_eq!(state.lock().unwrap().end_clock.time, 77);
        assert_eq!(state.lock().unwrap().clock_time, 0);
        // From the End the default clock is the End's.
        run_in("time add 3", &state, Dimension::End).0.unwrap();
        assert_eq!(state.lock().unwrap().end_clock.time, 80);
        // The Nether has no default clock; `of` still works.
        let text = run_in("time set 5", &state, Dimension::Nether)
            .0
            .unwrap_err()
            .to_string();
        assert!(text.contains("no default clock in dimension"), "{text}");
        assert!(
            run_in("time of overworld pause", &state, Dimension::Nether)
                .0
                .is_ok()
        );
        assert!(run("time of minecraft:nowhere set 1", &state).0.is_err());
    }

    #[test]
    fn query_timeline_reports_position_and_repetitions() {
        let state = fresh();
        state.lock().unwrap().clock_time = 24_000 * 3 + 500;
        let (result, actions) = run("time query minecraft:day", &state);
        assert_eq!(result.unwrap(), 500);
        assert_eq!(key(&actions), "commands.time.query.timeline");
        let (result, actions) = run("time query minecraft:day repetition", &state);
        assert_eq!(result.unwrap(), 3);
        assert_eq!(key(&actions), "commands.time.query.timeline.repetitions");
        // The moon timeline has a longer period.
        assert_eq!(run("time query moon", &state).0.unwrap(), 24_000 * 3 + 500);
        let text = run("time of minecraft:the_end query minecraft:day", &state)
            .0
            .unwrap_err()
            .to_string();
        assert!(text.contains("not valid for clock"), "{text}");
        assert_eq!(run("time query time", &state).0.unwrap(), 72_500);
        let (result, actions) = run("time query gametime", &state);
        assert!(result.is_ok());
        assert_eq!(key(&actions), "commands.time.query.gametime");
    }
}
