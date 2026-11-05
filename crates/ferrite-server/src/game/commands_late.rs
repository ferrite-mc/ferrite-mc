//! `/compute`, `/waypoint` and `/ride`.
//!
//! `/compute` evaluates the context int/float providers of the extracted vanilla data
//! (`context_int_provider`, `context_float_provider`, `predicate`). `/waypoint` keeps color and
//! style per entity in [`super::commands_more::MoreState`] but sends no waypoint packet yet.
//! `/ride` validates its arguments and then reports that riding is unsupported: the simulation
//! has no vehicle/passenger model.

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

/// What a provider can look at: the block it is evaluated for, if any.
#[derive(Default, Clone)]
pub struct Context {
    pub block: Option<(String, Vec<(String, String)>)>,
}

fn blocks_match(blocks: &Value, name: &str) -> bool {
    match blocks {
        Value::String(one) => !one.starts_with('#') && namespaced(one) == name,
        Value::Array(many) => many.iter().any(|b| blocks_match(b, name)),
        _ => false,
    }
}

fn condition(value: &Value, ctx: &Context, depth: u8) -> bool {
    if depth > 16 {
        return false;
    }
    if let Some(id) = value.as_str() {
        return data_file("predicate", id).is_some_and(|v| condition(&v, ctx, depth + 1));
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

fn compute(
    ctx: &CommandContext<Source>,
    float: bool,
    context: Context,
) -> Result<i32, CommandError> {
    let Some(ArgumentValue::ResourceLocation(id)) = ctx.argument("provider") else {
        return Err(CommandError::failed("missing provider"));
    };
    let id = namespaced(id);
    let kind = if float {
        "context_float_provider"
    } else {
        "context_int_provider"
    };
    let Some(json) = data_file(kind, &id) else {
        return Err(failure("argument.id.unknown", vec![id]));
    };
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
    let Some(value) = evaluate(&json, kind, &context, &mut roll, 0) else {
        return Err(failure("commands.compute.failed", vec![id]));
    };
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
        Action::Feedback(Feedback::new("commands.compute.success", vec![id, text])),
    )?;
    Ok(result)
}

fn compute_tree() -> NodeBuilder<Source> {
    fn provider(name: &'static str) -> NodeBuilder<Source> {
        let _ = name;
        argument::<Source>("provider", ArgumentType::ResourceLocation)
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
            provider("float")
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
            .then(provider("int").executes(move |ctx| compute(ctx, false, context(ctx)?)));
        (float, integer)
    }
    fn block_context(ctx: &CommandContext<Source>) -> Result<Context, CommandError> {
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
        Ok(Context { block })
    }
    let (block_float, block_int) = branches(block_context);
    let (default_float, default_int) = branches(|_| Ok(Context::default()));
    let (entity_float, entity_int) = branches(|ctx| {
        entity(ctx, "computeTarget")?;
        Ok(Context::default())
    });
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
}
