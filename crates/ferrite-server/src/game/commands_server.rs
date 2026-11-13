//! Server-facing commands: `/locate`, `/reload`, `/datapack`, `/help`, `/schedule` and
//! `/trigger`.
//!
//! `/locate biome` asks the chunk generator (when one is configured) for the nearest match.
//! `/locate structure` and `/locate poi` are documented stubs: there is no
//! structure placement or point-of-interest storage yet, so no such feature can be found.
//! The command still validates its arguments and reports vanilla's `not_found` translation
//! key instead of guessing coordinates.

use super::commands::{Action, Feedback, Source, player_only, send};
use ferrite_command::{
    ArgumentType, CommandContext, CommandDispatcher, CommandError, NodeBuilder, argument,
    arguments::{ArgumentValue, RegistryParser},
    literal,
};
use ferrite_protocol::{VarInt, packets::play::SetScore};
use std::sync::Arc;

fn gamemasters() -> impl Fn(&Source) -> bool + Send + Sync + 'static {
    |source: &Source| source.permission >= 2
}

fn owners() -> impl Fn(&Source) -> bool + Send + Sync + 'static {
    |source: &Source| source.permission >= 4
}

fn namespaced(raw: &str) -> String {
    if raw.contains(':') {
        raw.to_owned()
    } else {
        format!("minecraft:{raw}")
    }
}

fn feedback(
    ctx: &CommandContext<Source>,
    key: &'static str,
    args: Vec<String>,
) -> Result<i32, CommandError> {
    send(ctx.source(), Action::Feedback(Feedback::new(key, args)))
}

fn not_found(
    ctx: &CommandContext<Source>,
    key: &'static str,
    args: Vec<String>,
) -> Result<i32, CommandError> {
    feedback(ctx, key, args)?;
    Ok(0)
}

// ---------------------------------------------------------------------------------------------
// /locate
// ---------------------------------------------------------------------------------------------

fn locate_tree() -> NodeBuilder<Source> {
    literal::<Source>("locate")
        .requires(gamemasters())
        .restricted(true)
        .then(
            literal("biome").then(
                argument(
                    "biome",
                    ArgumentType::Registry {
                        parser: RegistryParser::ResourceOrTag,
                        registry: "minecraft:worldgen/biome".into(),
                    },
                )
                .executes(locate_biome),
            ),
        )
        .then(
            literal("poi").then(
                argument(
                    "poi",
                    ArgumentType::Registry {
                        parser: RegistryParser::ResourceOrTag,
                        registry: "minecraft:point_of_interest_type".into(),
                    },
                )
                .executes(locate_poi),
            ),
        )
        .then(
            literal("structure").then(
                argument(
                    "structure",
                    ArgumentType::Registry {
                        parser: RegistryParser::ResourceOrTag,
                        registry: "minecraft:worldgen/structure".into(),
                    },
                )
                .executes(locate_structure),
            ),
        )
}

// ---------------------------------------------------------------------------------------------
// /fillbiome
// ---------------------------------------------------------------------------------------------

/// Largest box `/fillbiome` accepts, in blocks (vanilla `commands.fillbiome.toobig`).
const FILLBIOME_LIMIT: u128 = 32768;

fn fillbiome_tree() -> NodeBuilder<Source> {
    let biome = |name: &'static str| {
        argument::<Source>(
            name,
            ArgumentType::Registry {
                parser: RegistryParser::ResourceOrTag,
                registry: "minecraft:worldgen/biome".into(),
            },
        )
    };
    literal::<Source>("fillbiome").requires(gamemasters()).then(
        argument("from", ArgumentType::BlockPos).then(argument("to", ArgumentType::BlockPos).then(
            biome("biome").executes(|ctx| fillbiome(ctx, false)).then(
                literal("replace").then(biome("filter").executes(|ctx| fillbiome(ctx, true))),
            ),
        )),
    )
}

/// The biome ids a `#tag` of `data/minecraft/tags/worldgen/biome` expands to.
pub(super) fn biome_tag_members(tag: &str, depth: u8) -> Vec<String> {
    let name = tag.trim_start_matches('#');
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    if depth > 8 || name.contains("..") {
        return Vec::new();
    }
    let relative =
        std::path::Path::new("vanilla/26.3/generated/data/minecraft/tags/worldgen/biome")
            .join(format!("{name}.json"));
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let Some(text) = [relative.clone(), root.join(&relative)]
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
    else {
        return Vec::new();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let mut members = Vec::new();
    for value in json["values"].as_array().into_iter().flatten() {
        let id = value
            .as_str()
            .or_else(|| value["id"].as_str())
            .unwrap_or_default();
        if id.starts_with('#') {
            members.extend(biome_tag_members(id, depth + 1));
        } else if !id.is_empty() {
            members.push(namespaced(id));
        }
    }
    members
}

fn fillbiome(ctx: &CommandContext<Source>, filtered: bool) -> Result<i32, CommandError> {
    use super::commands_world::{Origin, resolve};
    let source = ctx.source();
    let origin = Origin {
        pos: source.origin,
        yaw: source.yaw,
        pitch: source.pitch,
    };
    let corner = |name: &str| match ctx.argument(name) {
        Some(ArgumentValue::BlockPos(coords)) => Ok(resolve(coords, &origin)),
        _ => Err(CommandError::failed(format!("missing {name}"))),
    };
    let (a, b) = (corner("from")?, corner("to")?);
    let min = [a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])];
    let max = [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])];
    let side = |i: usize| u128::from(min[i].abs_diff(max[i])) + 1;
    let volume = side(0) * side(1) * side(2);
    if volume > FILLBIOME_LIMIT {
        return Err(CommandError::failed(
            Feedback::new(
                "commands.fillbiome.toobig",
                vec![FILLBIOME_LIMIT.to_string(), volume.to_string()],
            )
            .text,
        ));
    }
    let (target, _) = resource_target(ctx, "biome")?;
    if target.starts_with('#') {
        return Err(CommandError::failed("Biome must not be a tag"));
    }
    let accept: Box<dyn Fn(&str) -> bool> = if filtered {
        let (filter, _) = resource_target(ctx, "filter")?;
        if filter.starts_with('#') {
            let members = biome_tag_members(&filter, 0);
            Box::new(move |name| members.iter().any(|m| m == name))
        } else {
            Box::new(move |name| name == filter)
        }
    } else {
        Box::new(|_| true)
    };
    let Some(world) = source.world.clone() else {
        return Err(CommandError::failed(
            Feedback::new("argument.pos.unloaded", vec![]).text,
        ));
    };
    let (count, chunks) = world
        .fill_biome(min, max, &target, &*accept)
        .map_err(|_| CommandError::failed(Feedback::new("argument.pos.unloaded", vec![]).text))?;
    if count == 0 {
        return Err(CommandError::failed(
            Feedback::new("commands.fillbiome.no_changes", vec![]).text,
        ));
    }
    send(
        source,
        Action::ResendChunks {
            dimension: source.dimension,
            chunks,
        },
    )?;
    feedback(
        ctx,
        "commands.fillbiome.success.count",
        vec![
            count.to_string(),
            min[0].to_string(),
            min[1].to_string(),
            min[2].to_string(),
            max[0].to_string(),
            max[1].to_string(),
            max[2].to_string(),
        ],
    )
}

fn located(
    ctx: &CommandContext<Source>,
    key: &'static str,
    name: String,
    (x, y, z): (i32, i32, i32),
) -> Result<i32, CommandError> {
    let origin = ctx.source().origin;
    let distance = ((x as f64 - origin[0]).powi(2) + (z as f64 - origin[2]).powi(2))
        .sqrt()
        .round();
    feedback(
        ctx,
        key,
        vec![name, format!("[{x}, {y}, {z}]"), format!("{distance}")],
    )
}

/// The `id` or `#tag` text the host matches against, and the name shown in feedback.
fn resource_target(
    ctx: &CommandContext<Source>,
    argument: &str,
) -> Result<(String, String), CommandError> {
    let (tag, id) = match ctx.argument(argument) {
        Some(ArgumentValue::Resource { tag, id }) => (*tag, id.clone()),
        _ => {
            return Err(CommandError::failed(format!(
                "missing argument '{argument}'"
            )));
        }
    };
    let id = namespaced(&id);
    Ok(if tag {
        (format!("#{id}"), format!("#{id}"))
    } else {
        (id.clone(), id)
    })
}

fn locate_biome(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let (target, name) = resource_target(ctx, "biome")?;
    let Some(world) = ctx.source().world.clone() else {
        return not_found(ctx, "commands.locate.biome.not_found", vec![name]);
    };
    let origin = ctx.source().origin;
    match world.locate_biome(&target, origin[0].floor() as i32, origin[2].floor() as i32) {
        Some(Ok(Some(position))) => located(ctx, "commands.locate.biome.success", name, position),
        Some(Ok(None)) | None | Some(Err(_)) => {
            not_found(ctx, "commands.locate.biome.not_found", vec![name])
        }
    }
}

fn locate_poi(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let (target, name) = resource_target(ctx, "poi")?;
    let Some(world) = ctx.source().world.clone() else {
        return not_found(ctx, "commands.locate.poi.not_found", vec![name]);
    };
    let origin = ctx.source().origin;
    // Vanilla searches 256 blocks around the command source.
    match world.locate_poi(
        &target,
        origin[0].floor() as i32,
        origin[1].floor() as i32,
        origin[2].floor() as i32,
        256,
    ) {
        Some(Ok(Some(position))) => located(ctx, "commands.locate.poi.success", name, position),
        Some(Ok(None)) | None | Some(Err(_)) => {
            not_found(ctx, "commands.locate.poi.not_found", vec![name])
        }
    }
}

fn locate_structure(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let (target, name) = resource_target(ctx, "structure")?;
    let Some(world) = ctx.source().world.clone() else {
        return not_found(ctx, "commands.locate.structure.not_found", vec![name]);
    };
    let origin = ctx.source().origin;
    // Vanilla searches 100 chunks around the command source.
    match world.locate_structure(
        &target,
        origin[0].floor() as i32,
        origin[2].floor() as i32,
        100,
    ) {
        Some(Ok(Some(position))) => {
            located(ctx, "commands.locate.structure.success", name, position)
        }
        Some(Ok(None)) | None | Some(Err(_)) => {
            not_found(ctx, "commands.locate.structure.not_found", vec![name])
        }
    }
}

// ---------------------------------------------------------------------------------------------
// /reload and /datapack
// ---------------------------------------------------------------------------------------------

fn reload_tree() -> NodeBuilder<Source> {
    literal::<Source>("reload")
        .requires(owners())
        .restricted(true)
        .executes(|ctx| send(ctx.source(), Action::Reload))
}

/// Which packs `/datapack list` reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatapackListKind {
    Both,
    Available,
    Enabled,
}

/// Where `/datapack enable` puts the pack in the load order (the last pack wins).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackPlacement {
    /// No position given: vanilla puts a pack on top, i.e. last.
    Last,
    First,
    Before(String),
    After(String),
}

fn datapack_tree() -> NodeBuilder<Source> {
    let pack = || argument("name", ArgumentType::string());
    let enable = |placement: PackPlacement| {
        move |ctx: &CommandContext<Source>| datapack_set(ctx, true, placement.clone())
    };
    literal::<Source>("datapack")
        .requires(owners())
        .restricted(true)
        .then(
            literal("list")
                .executes(|ctx| send(ctx.source(), Action::DatapackList(DatapackListKind::Both)))
                .then(literal("available").executes(|ctx| {
                    send(
                        ctx.source(),
                        Action::DatapackList(DatapackListKind::Available),
                    )
                }))
                .then(literal("enabled").executes(|ctx| {
                    send(
                        ctx.source(),
                        Action::DatapackList(DatapackListKind::Enabled),
                    )
                })),
        )
        .then(
            literal("enable").then(
                pack()
                    .executes(enable(PackPlacement::Last))
                    .then(literal("first").executes(enable(PackPlacement::First)))
                    .then(literal("last").executes(enable(PackPlacement::Last)))
                    .then(literal("before").then(
                        argument("existing", ArgumentType::string()).executes(|ctx| {
                            let other = ctx.string("existing").unwrap_or_default().to_owned();
                            datapack_set(ctx, true, PackPlacement::Before(other))
                        }),
                    ))
                    .then(literal("after").then(
                        argument("existing", ArgumentType::string()).executes(|ctx| {
                            let other = ctx.string("existing").unwrap_or_default().to_owned();
                            datapack_set(ctx, true, PackPlacement::After(other))
                        }),
                    )),
            ),
        )
        .then(
            literal("disable")
                .then(pack().executes(|ctx| datapack_set(ctx, false, PackPlacement::Last))),
        )
        .then(
            literal("create").then(argument("id", ArgumentType::string()).then(
                argument("description", ArgumentType::Component).executes(|ctx| {
                    let id = ctx.string("id").unwrap_or_default().to_owned();
                    let description = ctx.string("description").unwrap_or_default().to_owned();
                    send(ctx.source(), Action::DatapackCreate { id, description })
                }),
            )),
        )
}

fn datapack_set(
    ctx: &CommandContext<Source>,
    enable: bool,
    placement: PackPlacement,
) -> Result<i32, CommandError> {
    let pack = ctx
        .string("name")
        .ok_or_else(|| CommandError::failed("missing data pack name"))?;
    send(
        ctx.source(),
        Action::DatapackSet {
            pack: pack.to_owned(),
            enable,
            placement,
        },
    )
}

// ---------------------------------------------------------------------------------------------
// /help
// ---------------------------------------------------------------------------------------------

fn help_tree() -> NodeBuilder<Source> {
    literal::<Source>("help")
        .executes(help)
        .then(argument("command", ArgumentType::word()).executes(help))
}

fn help(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let tree: Arc<CommandDispatcher<Source>> = ctx
        .source()
        .tree
        .clone()
        .ok_or_else(|| CommandError::failed("help needs a running command dispatcher"))?;
    if let Some(command) = ctx.string("command") {
        let Some(node) = tree.node(tree.root()).child(command) else {
            return Err(CommandError::failed(format!(
                "Unknown command, please try to use /help ({command})"
            )));
        };
        let usage = tree.first_usage(node).unwrap_or_else(|| command.to_owned());
        return feedback(ctx, "commands.help.usage", vec![format!("/{usage}")]);
    }
    let names: Vec<String> = tree
        .root_literals()
        .into_iter()
        .filter(|(_, node)| tree.node(*node).can_use(ctx.source()))
        .map(|(name, _)| format!("/{name}"))
        .collect();
    if names.is_empty() {
        return Err(CommandError::failed("No commands are available"));
    }
    let count = names.len() as i32;
    send(
        ctx.source(),
        Action::Feedback(Feedback::new(
            "commands.help.header",
            vec!["1".into(), "1".into()],
        )),
    )?;
    send(
        ctx.source(),
        Action::Feedback(Feedback::new(
            "commands.help.command",
            vec![names.join(", ")],
        )),
    )?;
    Ok(count)
}

// ---------------------------------------------------------------------------------------------
// /schedule
// ---------------------------------------------------------------------------------------------

fn schedule_tree() -> NodeBuilder<Source> {
    literal::<Source>("schedule")
        .requires(gamemasters())
        .restricted(true)
        .then(
            literal("function").then(
                argument("function", ArgumentType::opaque("minecraft:function"))
                    .suggests(
                        |ctx: &CommandContext<Source>,
                         builder: &mut ferrite_command::suggestion::SuggestionsBuilder| {
                        let state = ctx.source().state.lock().unwrap();
                        for name in state.functions.function_names() {
                            builder.suggest(name.to_owned());
                        }
                        for name in state.functions.tag_names() {
                            builder.suggest(format!("#{name}"));
                        }
                    })
                    .then(
                        argument("time", ArgumentType::Time { min: 0 })
                            .executes(|ctx| schedule_function(ctx, false))
                            .then(literal("append").executes(|ctx| schedule_function(ctx, false)))
                            .then(literal("replace").executes(|ctx| schedule_function(ctx, true))),
                    ),
            ),
        )
        .then(
            literal("clear").then(
                argument("function", ArgumentType::opaque("minecraft:function"))
                    .suggests(
                        |ctx: &CommandContext<Source>,
                         builder: &mut ferrite_command::suggestion::SuggestionsBuilder| {
                        let state = ctx.source().state.lock().unwrap();
                        for name in state.functions.function_names() {
                            builder.suggest(name.to_owned());
                        }
                    })
                    .executes(clear_schedule),
            ),
        )
}

fn schedule_function(ctx: &CommandContext<Source>, replace: bool) -> Result<i32, CommandError> {
    let raw = ctx
        .string("function")
        .ok_or_else(|| CommandError::failed("missing function argument 'function'"))?;
    let tag = raw.starts_with('#');
    let name = namespaced(raw.trim_start_matches('#'));
    let ticks = ctx
        .integer("time")
        .ok_or_else(|| CommandError::failed("missing time argument 'time'"))?;
    let source = ctx.source();
    let mut state = source.state.lock().unwrap();
    let known = if tag {
        state.functions.tag(&name).is_some()
    } else {
        state.functions.get(&name).is_some()
    };
    if !known {
        return Err(CommandError::failed(format!("Unknown function {raw}")));
    }
    if ticks <= 0 {
        drop(state);
        return feedback(ctx, "commands.schedule.same_tick", vec![name]);
    }
    let due = state.game_time.saturating_add(i64::from(ticks));
    if replace {
        state.schedules.insert(name.clone(), vec![due]);
    } else {
        state.schedules.entry(name.clone()).or_default().push(due);
    }
    drop(state);
    feedback(
        ctx,
        "commands.schedule.created.function",
        vec![name, ticks.to_string(), due.to_string()],
    )
}

fn clear_schedule(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let raw = ctx
        .string("function")
        .ok_or_else(|| CommandError::failed("missing function argument 'function'"))?;
    let name = namespaced(raw.trim_start_matches('#'));
    let removed = ctx
        .source()
        .state
        .lock()
        .unwrap()
        .schedules
        .remove(&name)
        .map_or(0, |times| times.len());
    if removed == 0 {
        feedback(ctx, "commands.schedule.cleared.failure", vec![name])?;
        return Ok(0);
    }
    feedback(
        ctx,
        "commands.schedule.cleared.success",
        vec![removed.to_string(), name],
    )
}

// ---------------------------------------------------------------------------------------------
// /trigger
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum TriggerMode {
    /// `trigger <objective>`: adds one.
    Simple,
    Add,
    Set,
}

fn trigger_tree() -> NodeBuilder<Source> {
    // commands.json: `trigger <objective>` is executable and only `add|set <value>` take a
    // value; there is no bare `trigger <objective> <value>`.
    literal::<Source>("trigger").then(
        argument("objective", ArgumentType::opaque("minecraft:objective"))
            .executes(|ctx| trigger_value(ctx, TriggerMode::Simple))
            .then(
                literal("add").then(
                    argument("value", ArgumentType::integer())
                        .executes(|ctx| trigger_value(ctx, TriggerMode::Add)),
                ),
            )
            .then(
                literal("set").then(
                    argument("value", ArgumentType::integer())
                        .executes(|ctx| trigger_value(ctx, TriggerMode::Set)),
                ),
            ),
    )
}

fn trigger_value(ctx: &CommandContext<Source>, mode: TriggerMode) -> Result<i32, CommandError> {
    let source = ctx.source();
    let uuid = source.uuid.ok_or_else(player_only)?;
    let holder = source.name.clone().unwrap_or_default();
    // Scoreboard objectives are plain names, not namespaced ids: `trigger foo` is objective `foo`
    // (this used to look up `minecraft:foo` and never found an objective).
    let objective = ctx
        .string("objective")
        .ok_or_else(|| CommandError::failed("missing objective argument"))?
        .to_owned();
    let amount = if mode == TriggerMode::Simple {
        1
    } else {
        ctx.integer("value")
            .ok_or_else(|| CommandError::failed("missing value argument"))?
    };
    let mut state = source.state.lock().unwrap();
    let Some(entry) = state.objectives.get(&objective) else {
        return Err(CommandError::failed(format!(
            "Unknown scoreboard objective '{objective}'"
        )));
    };
    // `commands.trigger.failed.invalid` / `commands.trigger.failed.unprimed` in en_us.
    if !entry.criteria.ends_with("trigger") {
        return Err(CommandError::failed(
            "You can only trigger objectives that are 'trigger' type",
        ));
    }
    if !state
        .trigger_enabled
        .remove(&(objective.clone(), holder.clone()))
    {
        return Err(CommandError::failed(
            "You cannot trigger this objective yet",
        ));
    }
    let key = (holder, objective.clone());
    let current = *state.scores.get(&key).unwrap_or(&0);
    let value = if mode == TriggerMode::Set {
        amount
    } else {
        current.saturating_add(amount)
    };
    state.scores.insert(key, value);
    drop(state);
    send(
        source,
        Action::Packets {
            targets: vec![uuid],
            packet: super::commands_extras::PlayerPacket::SetScore(SetScore {
                entity: source.name.clone().unwrap_or_default(),
                objective: objective.clone(),
                value: VarInt(value),
                display_name: None,
                number_format: None,
            }),
        },
    )?;
    match mode {
        TriggerMode::Simple => feedback(ctx, "commands.trigger.simple.success", vec![objective]),
        TriggerMode::Add => feedback(
            ctx,
            "commands.trigger.add.success",
            vec![objective, amount.to_string()],
        ),
        TriggerMode::Set => feedback(
            ctx,
            "commands.trigger.set.success",
            vec![objective, value.to_string()],
        ),
    }
}

/// Adds every command family above to the server tree.
pub fn register(tree: &mut CommandDispatcher<Source>) {
    tree.register(locate_tree());
    tree.register(fillbiome_tree());
    tree.register(reload_tree());
    tree.register(datapack_tree());
    tree.register(help_tree());
    tree.register(schedule_tree());
    tree.register(trigger_tree());
}
