//! `/compute`, `/waypoint` and `/ride`.
//!
//! `/compute` evaluates the context int/float providers of the extracted vanilla data
//! (`context_int_provider`, `context_float_provider`, `predicate`). `/waypoint` keeps color and
//! style per entity in [`super::commands_more::MoreState`]; [`super::waypoints`] sends them.
//! `/ride` mounts a rider on a vehicle or dismounts it through [`super::ride`], and reports the
//! vanilla `commands.ride.*` messages (`CommandRuntime::run_ride` in `game/mod.rs`).

use std::path::PathBuf;

use ferrite_command::{
    ArgumentType, ArgumentValue, CommandContext, CommandDispatcher, CommandError, NodeBuilder,
    argument, literal,
};
use serde_json::Value;

use super::commands::{Action, Feedback, Source, entity, resolved_targets, send};

fn failure(key: &'static str, args: Vec<String>) -> CommandError {
    CommandError::failed(Feedback::new(key, args).text)
}

fn namespaced(raw: &str) -> String {
    if raw.contains(':') {
        raw.to_owned()
    } else {
        format!("minecraft:{raw}")
    }
}

fn data_file(kind: &str, id: &str) -> Option<Value> {
    let id = id.strip_prefix("minecraft:").unwrap_or(id);
    if id.contains("..") {
        return None;
    }
    let relative = format!("vanilla/26.3/generated/data/minecraft/{kind}/{id}.json");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    [PathBuf::from(&relative), root.join(&relative)]
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
}

/// What a provider can look at about an entity: its type, position and dimension. The command
/// entity snapshot carries nothing else (no health, equipment or flags), so predicates that ask
/// for more never match.
#[derive(Clone, Debug, PartialEq)]
pub struct EntityFacts {
    /// Entity type id (`minecraft:pig`), `minecraft:player` for a player.
    pub kind: String,
    pub pos: [f64; 3],
    /// Dimension id (`minecraft:overworld`).
    pub dimension: String,
    /// A mob's current health (the NBT `Health` a `minecraft:nbt` predicate can match).
    pub health: Option<f32>,
}

/// What a provider can look at: the block it is evaluated for, if any, and the entity of the
/// `entity` source (vanilla's `TARGET_ENTITY`) or the executing entity of the `default` source
/// (`THIS_ENTITY`).
#[derive(Default, Clone)]
pub struct Context {
    pub block: Option<(String, Vec<(String, String)>)>,
    pub target_entity: Option<EntityFacts>,
    pub this_entity: Option<EntityFacts>,
}

fn in_range(range: &Value, value: f64) -> bool {
    match range {
        Value::Number(exact) => exact.as_f64() == Some(value),
        Value::Object(bounds) => {
            bounds
                .get("min")
                .and_then(Value::as_f64)
                .is_none_or(|min| value >= min)
                && bounds
                    .get("max")
                    .and_then(Value::as_f64)
                    .is_none_or(|max| value <= max)
        }
        _ => false,
    }
}

/// `entity_properties`: the `minecraft:entity_type` and `minecraft:location` (position and
/// dimension) entries are evaluated; any other entry fails because the data is not there.
fn entity_matches(facts: &EntityFacts, predicate: &Value) -> bool {
    let Some(entries) = predicate.as_object() else {
        return false;
    };
    entries.iter().all(|(key, wanted)| match key.as_str() {
        "minecraft:entity_type" => {
            let one = |name: &str| match name.strip_prefix('#') {
                Some(tag) => ferrite_data::tags::contains(
                    "entity_type",
                    tag.strip_prefix("minecraft:").unwrap_or(tag),
                    facts.kind.strip_prefix("minecraft:").unwrap_or(&facts.kind),
                ),
                None => namespaced(name) == facts.kind,
            };
            match wanted {
                Value::String(name) => one(name),
                Value::Array(names) => names.iter().filter_map(Value::as_str).any(one),
                _ => false,
            }
        }
        // `NbtPredicate`: the SNBT (a string, or a compound) must be contained in the entity's
        // data. Only the fields the command view has are there: `id`, `Pos` and `Health` (mobs).
        "minecraft:nbt" => {
            let expected = match wanted {
                Value::String(text) => ferrite_nbt::snbt::parse(text).ok(),
                _ => None,
            };
            expected.is_some_and(|expected| {
                super::selectors::nbt_matches(&expected, &entity_nbt(facts))
            })
        }
        "minecraft:location" => wanted.as_object().is_some_and(|location| {
            location.iter().all(|(field, value)| match field.as_str() {
                "dimension" => value.as_str().map(namespaced).as_deref() == Some(&facts.dimension),
                "position" => value.as_object().is_some_and(|axes| {
                    axes.iter().all(|(axis, range)| match axis.as_str() {
                        "x" => in_range(range, facts.pos[0]),
                        "y" => in_range(range, facts.pos[1]),
                        "z" => in_range(range, facts.pos[2]),
                        _ => false,
                    })
                }),
                _ => false,
            })
        }),
        _ => false,
    })
}

/// The NBT a `minecraft:nbt` entity predicate is matched against.
fn entity_nbt(facts: &EntityFacts) -> ferrite_nbt::Tag {
    use ferrite_nbt::{Compound, List, Tag};
    let mut nbt = Compound::new();
    nbt.insert("id".into(), Tag::String(facts.kind.clone()));
    nbt.insert("Pos".into(), Tag::List(List::Double(facts.pos.to_vec())));
    if let Some(health) = facts.health {
        nbt.insert("Health".into(), Tag::Float(health));
    }
    Tag::Compound(nbt)
}

fn blocks_match(blocks: &Value, name: &str) -> bool {
    match blocks {
        Value::String(one) => !one.starts_with('#') && namespaced(one) == name,
        Value::Array(many) => many.iter().any(|b| blocks_match(b, name)),
        _ => false,
    }
}

/// Evaluates the predicate `id` for `/execute if|unless predicate`. Datapack predicates win over
/// the extracted vanilla ones; `None` means no pack and no vanilla data defines it.
/// Condition types outside the supported set (`match_block`, `entity_properties`, `inverted`,
/// `all_of`, `any_of`) evaluate false, like a context that lacks the data they need.
pub(super) fn evaluate_predicate(
    store: &super::functions::FunctionStore,
    id: &str,
    ctx: &Context,
) -> Option<bool> {
    let id = namespaced(id);
    let value = store
        .predicate(&id)
        .cloned()
        .or_else(|| data_file("predicate", &id))?;
    Some(condition(&value, ctx, 0))
}

fn condition(value: &Value, ctx: &Context, depth: u8) -> bool {
    if depth > 16 {
        return false;
    }
    if let Some(id) = value.as_str() {
        return data_file("predicate", id).is_some_and(|v| condition(&v, ctx, depth + 1));
    }
    if let Some(list) = value.as_array() {
        return list.iter().all(|t| condition(t, ctx, depth + 1));
    }
    match value["type"].as_str() {
        Some("minecraft:match_block") => {
            let Some((name, props)) = &ctx.block else {
                return false;
            };
            blocks_match(&value["blocks"], name)
                && value["state"].as_object().is_none_or(|wanted| {
                    wanted.iter().all(|(k, v)| {
                        props
                            .iter()
                            .any(|(pk, pv)| pk == k && Some(pv.as_str()) == v.as_str())
                    })
                })
        }
        Some("minecraft:entity_properties") => {
            let facts = match value["entity"].as_str() {
                Some("this") => ctx.this_entity.as_ref(),
                Some("target_entity") => ctx.target_entity.as_ref(),
                _ => None,
            };
            facts.is_some_and(|facts| entity_matches(facts, &value["predicate"]))
        }
        Some("minecraft:inverted") => !condition(&value["term"], ctx, depth + 1),
        Some("minecraft:all_of") => value["terms"]
            .as_array()
            .is_some_and(|terms| terms.iter().all(|t| condition(t, ctx, depth + 1))),
        Some("minecraft:any_of") => value["terms"]
            .as_array()
            .is_some_and(|terms| terms.iter().any(|t| condition(t, ctx, depth + 1))),
        _ => false,
    }
}

/// Evaluates a provider JSON value; `kind` is `context_int_provider` or `context_float_provider`.
pub fn evaluate(
    value: &Value,
    kind: &str,
    ctx: &Context,
    rng: &mut dyn FnMut(u64) -> u64,
    depth: u8,
) -> Option<f64> {
    if depth > 16 {
        return None;
    }
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(id) => {
            let inner = data_file(kind, id)?;
            evaluate(&inner, kind, ctx, rng, depth + 1)
        }
        Value::Object(_) => match value["type"].as_str()? {
            "minecraft:conditional" => {
                let branch = if condition(&value["condition"], ctx, 0) {
                    &value["on_true"]
                } else {
                    &value["on_false"]
                };
                evaluate(branch, kind, ctx, rng, depth + 1)
            }
            "minecraft:div" => {
                let left = evaluate(&value["left"], kind, ctx, rng, depth + 1)?;
                let right = evaluate(&value["right"], kind, ctx, rng, depth + 1)?;
                Some(if right == 0.0 { 0.0 } else { left / right })
            }
            "minecraft:number_dispatcher" => {
                for case in value["cases"].as_array().into_iter().flatten() {
                    if condition(&case["condition"], ctx, 0) {
                        return evaluate(&case["value"], kind, ctx, rng, depth + 1);
                    }
                }
                evaluate(&value["default"], kind, ctx, rng, depth + 1)
            }
            "minecraft:weighted_list" => {
                let entries = value["distribution"].as_array()?;
                let total: u64 = entries.iter().filter_map(|e| e["weight"].as_u64()).sum();
                if total == 0 {
                    return None;
                }
                let mut roll = rng(total);
                for entry in entries {
                    let weight = entry["weight"].as_u64().unwrap_or(0);
                    if roll < weight {
                        return evaluate(&entry["data"], kind, ctx, rng, depth + 1);
                    }
                    roll -= weight;
                }
                None
            }
            "minecraft:constant" => evaluate(&value["value"], kind, ctx, rng, depth + 1),
            _ => None,
        },
        _ => None,
    }
}

fn facts_of(entity: &super::selectors::SelectedEntity) -> EntityFacts {
    EntityFacts {
        kind: entity
            .kind
            .map_or("minecraft:player", |kind| {
                ferrite_data::entity_props::get(kind).key
            })
            .to_owned(),
        pos: entity.pos,
        dimension: entity.dimension.key().to_owned(),
        health: entity.health,
    }
}

/// A provider argument as the JSON the evaluator reads: a registry id (`minecraft:brewing/x`)
/// or an inline SNBT/JSON compound.
fn provider_json(raw: &str, kind: &str) -> Result<Value, CommandError> {
    let raw = raw.trim();
    if raw.starts_with('{') {
        let tag = ferrite_nbt::snbt::parse(raw)
            .map_err(|error| CommandError::failed(format!("Invalid provider: {error}")))?;
        return Ok(tag_to_json(&tag));
    }
    let id = namespaced(raw);
    data_file(kind, &id).ok_or_else(|| failure("argument.id.unknown", vec![id]))
}

fn tag_to_json(tag: &ferrite_nbt::Tag) -> Value {
    use ferrite_nbt::{List, Tag};
    match tag {
        Tag::Byte(v) => Value::from(*v),
        Tag::Short(v) => Value::from(*v),
        Tag::Int(v) => Value::from(*v),
        Tag::Long(v) => Value::from(*v),
        Tag::Float(v) => Value::from(f64::from(*v)),
        Tag::Double(v) => Value::from(*v),
        Tag::String(v) => Value::from(v.clone()),
        Tag::Compound(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), tag_to_json(value)))
                .collect(),
        ),
        Tag::List(List::Compound(items)) => Value::Array(
            items
                .iter()
                .map(|map| tag_to_json(&Tag::Compound(map.clone())))
                .collect(),
        ),
        Tag::List(List::String(items)) => {
            Value::Array(items.iter().cloned().map(Value::from).collect())
        }
        Tag::List(List::Int(items)) => {
            Value::Array(items.iter().copied().map(Value::from).collect())
        }
        _ => Value::Null,
    }
}

/// Evaluates the provider named by `raw` (`float` picks the float registry). The error is the
/// vanilla `commands.compute.failed` when the provider has no value in this context.
pub(super) fn provider_value(
    raw: &str,
    float: bool,
    context: &Context,
) -> Result<f64, CommandError> {
    let kind = if float {
        "context_float_provider"
    } else {
        "context_int_provider"
    };
    let json = provider_json(raw, kind)?;
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64)
        | 1;
    let mut roll = move |bound: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % bound
    };
    evaluate(&json, kind, context, &mut roll, 0)
        .ok_or_else(|| failure("commands.compute.failed", vec![raw.to_owned()]))
}

pub(super) fn block_context(ctx: &CommandContext<Source>) -> Result<Context, CommandError> {
    let Some(ArgumentValue::BlockPos(coords)) = ctx.argument("computePos") else {
        return Err(CommandError::failed("missing computePos"));
    };
    let source = ctx.source();
    let origin = super::commands_world::Origin {
        pos: source.origin,
        yaw: source.yaw,
        pitch: source.pitch,
    };
    let [x, y, z] = super::commands_world::resolve(coords, &origin);
    let block = source
        .world
        .as_ref()
        .and_then(|world| world.block(x, y, z))
        .and_then(|state| u16::try_from(state).ok())
        .and_then(ferrite_data::blocks::state)
        .map(|(name, props)| {
            (
                namespaced(name),
                props
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                    .collect(),
            )
        });
    Ok(Context {
        block,
        ..Context::default()
    })
}
pub(super) fn entity_context(ctx: &CommandContext<Source>) -> Result<Context, CommandError> {
    let target = entity(ctx, "computeTarget")?;
    let source = ctx.source();
    let found = {
        let state = source.state.lock().unwrap();
        super::selectors::resolve_selection(&state, &target, source.uuid, source.origin)?
    };
    let Some(first) = found.first() else {
        return Err(CommandError::failed(
            Feedback::new("argument.entity.notfound.entity", vec![]).text,
        ));
    };
    Ok(Context {
        target_entity: Some(facts_of(first)),
        ..Context::default()
    })
}
pub(super) fn default_context(ctx: &CommandContext<Source>) -> Result<Context, CommandError> {
    // `THIS_ENTITY` is the executing entity, when it is one the command view knows.
    let source = ctx.source();
    let this_entity = source.uuid.and_then(|uuid| {
        let state = source.state.lock().unwrap();
        super::selectors::resolve_selection(
            &state,
            &ferrite_command::arguments::EntityTarget::Uuid(uuid.to_string()),
            source.uuid,
            source.origin,
        )
        .ok()
        .and_then(|found| found.first().map(facts_of))
    });
    Ok(Context {
        this_entity,
        ..Context::default()
    })
}

fn compute(
    ctx: &CommandContext<Source>,
    float: bool,
    context: Context,
) -> Result<i32, CommandError> {
    let Some(ArgumentValue::Raw(raw)) = ctx.argument("provider") else {
        return Err(CommandError::failed("missing provider"));
    };
    let value = provider_value(raw, float, &context)?;
    let scale = match ctx.argument("scale") {
        Some(ArgumentValue::Float(scale)) => f64::from(*scale),
        _ => 1.0,
    };
    let result = if float {
        (value * scale).floor() as i32
    } else {
        value as i32
    };
    let text = if float {
        format!("{value}")
    } else {
        format!("{}", value as i64)
    };
    send(
        ctx.source(),
        Action::Feedback(Feedback::new(
            "commands.compute.success",
            vec![raw.clone(), text],
        )),
    )?;
    Ok(result)
}

fn compute_tree() -> NodeBuilder<Source> {
    fn provider(float: bool) -> NodeBuilder<Source> {
        argument::<Source>(
            "provider",
            ArgumentType::opaque(if float {
                "minecraft:context_float_provider"
            } else {
                "minecraft:context_int_provider"
            }),
        )
    }
    fn branches(
        context: impl Fn(&CommandContext<Source>) -> Result<Context, CommandError>
        + Clone
        + Send
        + Sync
        + 'static,
    ) -> (NodeBuilder<Source>, NodeBuilder<Source>) {
        let float_context = context.clone();
        let float = literal::<Source>("float").then(
            provider(true)
                .executes({
                    let context = float_context.clone();
                    move |ctx| compute(ctx, true, context(ctx)?)
                })
                .then(
                    argument::<Source>("scale", ArgumentType::float())
                        .executes(move |ctx| compute(ctx, true, float_context(ctx)?)),
                ),
        );
        let integer = literal::<Source>("integer")
            .then(provider(false).executes(move |ctx| compute(ctx, false, context(ctx)?)));
        (float, integer)
    }
    let (block_float, block_int) = branches(block_context);
    let (default_float, default_int) = branches(default_context);
    let (entity_float, entity_int) = branches(entity_context);
    literal::<Source>("compute")
        .requires(|s| s.permission >= 2)
        .then(
            literal::<Source>("block").then(
                argument::<Source>("computePos", ArgumentType::BlockPos)
                    .then(block_float)
                    .then(block_int),
            ),
        )
        .then(
            literal::<Source>("default")
                .then(default_float)
                .then(default_int),
        )
        .then(
            literal::<Source>("entity").then(
                argument::<Source>("computeTarget", ArgumentType::entity())
                    .then(entity_float)
                    .then(entity_int),
            ),
        )
}

// ---------------------------------------------------------------------------------------------
// /waypoint
// ---------------------------------------------------------------------------------------------

fn waypoint_target(ctx: &CommandContext<Source>) -> Result<(uuid::Uuid, String), CommandError> {
    let uuids = resolved_targets(ctx, "waypoint")?;
    let uuid = *uuids
        .first()
        .ok_or_else(|| CommandError::failed("No entity was found"))?;
    let game = ctx.source().state.lock().unwrap();
    let name = game
        .players
        .iter()
        .find(|p| p.uuid == uuid)
        .map_or_else(|| uuid.to_string(), |p| p.name.clone());
    Ok((uuid, name))
}

fn waypoint_tree() -> NodeBuilder<Source> {
    fn set(
        ctx: &CommandContext<Source>,
        update: impl FnOnce(&mut super::commands_more::WaypointStyle),
        key: &'static str,
        args: Vec<String>,
    ) -> Result<i32, CommandError> {
        let (uuid, name) = waypoint_target(ctx)?;
        {
            let mut game = ctx.source().state.lock().unwrap();
            update(game.more.waypoints.entry(uuid).or_default());
        }
        let mut all = vec![name];
        all.extend(args);
        send(ctx.source(), Action::Feedback(Feedback::new(key, all)))?;
        Ok(1)
    }
    let color_name = |ctx: &CommandContext<Source>, key: &str| match ctx.argument(key) {
        Some(ArgumentValue::Raw(text) | ArgumentValue::String(text)) => text.clone(),
        _ => String::new(),
    };
    literal::<Source>("waypoint")
        .requires(|s| s.permission >= 2)
        .then(literal::<Source>("list").executes(|ctx| {
            let names: Vec<String> = ctx
                .source()
                .state
                .lock()
                .unwrap()
                .players
                .iter()
                .map(|p| p.name.clone())
                .collect();
            let key = if names.is_empty() {
                "commands.waypoint.list.empty"
            } else {
                "commands.waypoint.list.success"
            };
            send(
                ctx.source(),
                Action::Feedback(Feedback::new(
                    key,
                    if names.is_empty() {
                        vec![]
                    } else {
                        vec![names.len().to_string(), names.join(", ")]
                    },
                )),
            )?;
            Ok(i32::try_from(names.len()).unwrap_or(i32::MAX))
        }))
        .then(
            literal::<Source>("modify").then(
                argument::<Source>("waypoint", ArgumentType::entity())
                    .then(
                        literal::<Source>("color")
                            .then(
                                argument::<Source>(
                                    "color",
                                    ArgumentType::opaque("minecraft:team_color"),
                                )
                                .executes(move |ctx| {
                                    let color = color_name(ctx, "color");
                                    let value = color.clone();
                                    set(
                                        ctx,
                                        move |s| s.color = Some(value),
                                        "commands.waypoint.modify.color",
                                        vec![color],
                                    )
                                }),
                            )
                            .then(
                                literal::<Source>("hex").then(
                                    argument::<Source>(
                                        "color",
                                        ArgumentType::opaque("minecraft:hex_color"),
                                    )
                                    .executes(move |ctx| {
                                        let color = color_name(ctx, "color");
                                        let value = color.clone();
                                        set(
                                            ctx,
                                            move |s| s.color = Some(value),
                                            "commands.waypoint.modify.color",
                                            vec![color],
                                        )
                                    }),
                                ),
                            )
                            .then(literal::<Source>("reset").executes(|ctx| {
                                set(
                                    ctx,
                                    |s| s.color = None,
                                    "commands.waypoint.modify.color.reset",
                                    vec![],
                                )
                            })),
                    )
                    .then(
                        literal::<Source>("style")
                            .then(literal::<Source>("reset").executes(|ctx| {
                                set(
                                    ctx,
                                    |s| s.style = None,
                                    "commands.waypoint.modify.style",
                                    vec![],
                                )
                            }))
                            .then(
                                literal::<Source>("set").then(
                                    argument::<Source>("style", ArgumentType::ResourceLocation)
                                        .executes(|ctx| {
                                            let Some(ArgumentValue::ResourceLocation(id)) =
                                                ctx.argument("style")
                                            else {
                                                return Err(CommandError::failed("missing style"));
                                            };
                                            let id = namespaced(id);
                                            let value = id.clone();
                                            set(
                                                ctx,
                                                move |s| s.style = Some(value),
                                                "commands.waypoint.modify.style",
                                                vec![id],
                                            )
                                        }),
                                ),
                            ),
                    ),
            ),
        )
}

// ---------------------------------------------------------------------------------------------
// /ride
// ---------------------------------------------------------------------------------------------

fn ride_tree() -> NodeBuilder<Source> {
    literal::<Source>("ride")
        .requires(|s| s.permission >= 2)
        .then(
            argument::<Source>("target", ArgumentType::entity())
                .then(literal::<Source>("dismount").executes(|ctx| {
                    send(
                        ctx.source(),
                        Action::Ride {
                            target: super::commands::entity(ctx, "target")?,
                            vehicle: None,
                        },
                    )
                }))
                .then(literal::<Source>("mount").then(
                    argument::<Source>("vehicle", ArgumentType::entity()).executes(|ctx| {
                        send(
                            ctx.source(),
                            Action::Ride {
                                target: super::commands::entity(ctx, "target")?,
                                vehicle: Some(super::commands::entity(ctx, "vehicle")?),
                            },
                        )
                    }),
                )),
        )
}

pub fn register(tree: &mut CommandDispatcher<Source>) {
    tree.register(compute_tree());
    tree.register(waypoint_tree());
    tree.register(ride_tree());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(json: &str, kind: &str, block: Option<&str>) -> Option<f64> {
        let ctx = Context {
            block: block.map(|name| (name.to_owned(), vec![])),
            ..Context::default()
        };
        let mut rng = |_bound: u64| 0;
        evaluate(
            &serde_json::from_str(json).unwrap(),
            kind,
            &ctx,
            &mut rng,
            0,
        )
    }

    #[test]
    fn providers_evaluate_constants_div_and_conditions() {
        let kind = "context_int_provider";
        assert_eq!(run("7", kind, None), Some(7.0));
        assert_eq!(
            run(
                r#"{"type":"minecraft:div","left":100,"right":4}"#,
                kind,
                None
            ),
            Some(25.0)
        );
        let conditional = r#"{"type":"minecraft:number_dispatcher","cases":[
            {"condition":{"type":"minecraft:match_block","blocks":["minecraft:smoker"]},"value":9}],
            "default":3}"#;
        assert_eq!(run(conditional, kind, Some("minecraft:smoker")), Some(9.0));
        assert_eq!(run(conditional, kind, Some("minecraft:stone")), Some(3.0));
        assert_eq!(run(conditional, kind, None), Some(3.0));
        let weighted =
            r#"{"type":"minecraft:weighted_list","distribution":[{"data":1,"weight":1}]}"#;
        assert_eq!(run(weighted, kind, None), Some(1.0));
    }

    #[test]
    fn provider_references_read_extracted_data() {
        // Only meaningful where `vanilla/` was extracted.
        if data_file("context_int_provider", "minecraft:cooking/time_wool").is_none() {
            return;
        }
        let value = run(
            "\"minecraft:cooking/time_wool\"",
            "context_int_provider",
            Some("minecraft:stone"),
        );
        assert!(value.is_some());
    }

    fn with_target(json: &str, target: Option<EntityFacts>) -> Option<f64> {
        let ctx = Context {
            target_entity: target,
            ..Context::default()
        };
        let mut rng = |_bound: u64| 0;
        evaluate(
            &serde_json::from_str(json).unwrap(),
            "context_int_provider",
            &ctx,
            &mut rng,
            0,
        )
    }

    fn pig(pos: [f64; 3]) -> EntityFacts {
        EntityFacts {
            kind: "minecraft:pig".to_owned(),
            pos,
            dimension: "minecraft:overworld".to_owned(),
            health: Some(10.0),
        }
    }

    #[test]
    fn the_entity_context_gives_providers_type_and_position() {
        let by_type = r#"{"type":"minecraft:conditional","condition":{
            "type":"minecraft:entity_properties","entity":"target_entity",
            "predicate":{"minecraft:entity_type":"minecraft:pig"}},"on_true":5,"on_false":1}"#;
        assert_eq!(with_target(by_type, Some(pig([0.0; 3]))), Some(5.0));
        let cow = EntityFacts {
            kind: "minecraft:cow".to_owned(),
            ..pig([0.0; 3])
        };
        assert_eq!(with_target(by_type, Some(cow)), Some(1.0));
        // Without an entity (the `default` source) the condition cannot hold.
        assert_eq!(with_target(by_type, None), Some(1.0));

        let by_height = r#"{"type":"minecraft:conditional","condition":{
            "type":"minecraft:entity_properties","entity":"target_entity",
            "predicate":{"minecraft:location":{"position":{"y":{"min":64}},
            "dimension":"minecraft:overworld"}}},"on_true":8,"on_false":2}"#;
        assert_eq!(
            with_target(by_height, Some(pig([0.0, 70.0, 0.0]))),
            Some(8.0)
        );
        assert_eq!(
            with_target(by_height, Some(pig([0.0, 10.0, 0.0]))),
            Some(2.0)
        );
        // A predicate entry this server has no data for (equipment, flags, ...) never matches.
        let by_health = r#"{"type":"minecraft:conditional","condition":{
            "type":"minecraft:entity_properties","entity":"target_entity",
            "predicate":{"minecraft:health":{"min":1}}},"on_true":1,"on_false":0}"#;
        assert_eq!(with_target(by_health, Some(pig([0.0; 3]))), Some(0.0));
        // `minecraft:nbt` sees the mob's real `Health` (vanilla `NbtPredicate`, a partial match).
        let by_nbt = |nbt: &str| {
            format!(
                r#"{{"type":"minecraft:conditional","condition":{{
            "type":"minecraft:entity_properties","entity":"target_entity",
            "predicate":{{"minecraft:nbt":"{nbt}"}}}},"on_true":1,"on_false":0}}"#
            )
        };
        assert_eq!(
            with_target(&by_nbt("{Health:10f}"), Some(pig([0.0; 3]))),
            Some(1.0)
        );
        assert_eq!(
            with_target(&by_nbt("{Health:20f}"), Some(pig([0.0; 3]))),
            Some(0.0)
        );
        let unhurt = EntityFacts {
            health: None,
            ..pig([0.0; 3])
        };
        assert_eq!(
            with_target(&by_nbt("{Health:10f}"), Some(unhurt)),
            Some(0.0)
        );
        assert_eq!(
            with_target(&by_nbt("{oops"), Some(pig([0.0; 3]))),
            Some(0.0)
        );
        // `this` only exists in the `default` source, not in the `entity` one.
        let this = by_type.replace("target_entity", "this");
        assert_eq!(with_target(&this, Some(pig([0.0; 3]))), Some(1.0));
    }

    #[test]
    fn facts_name_players_and_region_entities() {
        let selected = |kind: Option<u16>| super::super::selectors::SelectedEntity {
            uuid: uuid::Uuid::nil(),
            name: String::new(),
            player: kind.is_none(),
            kind,
            dimension: super::super::Dimension::Nether,
            pos: [1.0, 2.0, 3.0],
            yaw: 0.0,
            pitch: 0.0,
            health: None,
        };
        assert_eq!(facts_of(&selected(None)).kind, "minecraft:player");
        assert_eq!(facts_of(&selected(None)).dimension, "minecraft:the_nether");
        let zombie = ferrite_data::entity_props::by_name("minecraft:zombie").unwrap();
        assert_eq!(
            facts_of(&selected(Some(zombie.id))).kind,
            "minecraft:zombie"
        );
    }
}
