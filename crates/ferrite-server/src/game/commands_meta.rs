//! Scoreboard, boss bar, tag and team commands.
//!
//! The command trees match the 26.3 `commands.json` report. State lives in [`GameState`] so
//! `/scoreboard`, `/bossbar` and `/team` survive command boundaries, and every visible change
//! is mirrored by the packet the client expects.

use std::collections::BTreeSet;

use ferrite_command::{
    ArgumentType, ArgumentValue, CommandContext, CommandDispatcher, CommandError, NodeBuilder,
    argument,
    arguments::{EntityTarget, ScoreHolder},
    literal,
};
use ferrite_protocol::{
    VarInt,
    packets::play::{
        BossBar, BossBarAction, PlayerTeam, ResetScore, SetDisplayObjective, SetObjective,
        SetScore, TeamAction,
    },
    types::PlainText,
};
use uuid::Uuid;

use super::commands::{BossBarState, Objective, Source, TeamState, resolved_targets};
use super::commands_extras::{PlayerPacket, component_text, feedback, packets};

/// Registers `/scoreboard`, `/bossbar`, `/tag` and `/team` once each.
pub fn register(tree: &mut CommandDispatcher<Source>) {
    tree.register(scoreboard_tree());
    tree.register(bossbar_tree());
    tree.register(tag_tree());
    tree.register(team_tree());
}

fn gamemasters() -> impl Fn(&Source) -> bool + Send + Sync + 'static {
    |source| source.permission >= 2
}

fn namespaced(id: &str) -> String {
    if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    }
}

fn holders(ctx: &CommandContext<Source>, name: &str) -> Result<Vec<String>, CommandError> {
    let state = ctx.source().state.lock().unwrap();
    match ctx.argument(name) {
        Some(ArgumentValue::ScoreHolder(ScoreHolder::All)) => {
            let mut all: BTreeSet<String> = state
                .players
                .iter()
                .map(|player| player.name.clone())
                .collect();
            all.extend(state.scores.keys().map(|(holder, _)| holder.clone()));
            Ok(all.into_iter().collect())
        }
        Some(ArgumentValue::ScoreHolder(ScoreHolder::Target(target))) => match target {
            EntityTarget::Name(name) => Ok(vec![name.clone()]),
            EntityTarget::Uuid(raw) => Ok(vec![raw.clone()]),
            EntityTarget::Selector { .. } => {
                let uuids =
                    super::commands::resolve_players(&state.players, target, ctx.source().uuid)?;
                Ok(uuids
                    .into_iter()
                    .filter_map(|uuid| {
                        state
                            .players
                            .iter()
                            .find(|player| player.uuid == uuid)
                            .map(|player| player.name.clone())
                    })
                    .collect())
            }
        },
        _ => Err(CommandError::failed("Missing score holder")),
    }
}

fn objective_name(ctx: &CommandContext<Source>) -> String {
    ctx.string("objective").unwrap_or_default().to_owned()
}

fn require_objective<'a>(
    state: &'a super::commands::GameState,
    name: &str,
) -> Result<&'a Objective, CommandError> {
    state
        .objectives
        .get(name)
        .ok_or_else(|| CommandError::failed(format!("Unknown objective '{name}'")))
}

fn send_score(
    ctx: &CommandContext<Source>,
    holder: &str,
    objective: &str,
    value: i32,
) -> Result<i32, CommandError> {
    packets(
        ctx,
        Vec::new(),
        PlayerPacket::SetScore(SetScore {
            entity: holder.to_owned(),
            objective: objective.to_owned(),
            value: VarInt(value),
            display_name: None,
        }),
    )
}

fn slot_id(name: &str) -> Option<i32> {
    match name {
        "list" => Some(0),
        "sidebar" => Some(1),
        "below_name" => Some(2),
        other => other
            .strip_prefix("sidebar.team.")
            .and_then(color_id)
            .map(|color| 3 + color),
    }
}

const TEAM_COLORS: [(&str, i32); 16] = [
    ("black", 0),
    ("dark_blue", 1),
    ("dark_green", 2),
    ("dark_aqua", 3),
    ("dark_red", 4),
    ("dark_purple", 5),
    ("gold", 6),
    ("gray", 7),
    ("dark_gray", 8),
    ("blue", 9),
    ("green", 10),
    ("aqua", 11),
    ("red", 12),
    ("light_purple", 13),
    ("yellow", 14),
    ("white", 15),
];

fn color_id(name: &str) -> Option<i32> {
    TEAM_COLORS
        .iter()
        .find(|(color, _)| *color == name)
        .map(|(_, id)| *id)
}

// ---------------------------------------------------------------------------------------------
// /scoreboard
// ---------------------------------------------------------------------------------------------

fn scoreboard_tree() -> NodeBuilder<Source> {
    literal::<Source>("scoreboard")
        .requires(gamemasters())
        .restricted(true)
        .then(
            literal("objectives")
                .then(
                    literal("add").then(
                        argument("objective", ArgumentType::word()).then(
                            argument(
                                "criteria",
                                ArgumentType::opaque("minecraft:objective_criteria"),
                            )
                            .executes(add_objective)
                            .then(
                                argument("displayName", ArgumentType::Component)
                                    .executes(add_objective),
                            ),
                        ),
                    ),
                )
                .then(literal("list").executes(list_objectives))
                .then(
                    literal("modify").then(
                        argument("objective", ArgumentType::opaque("minecraft:objective"))
                            .then(
                                literal("displayname").then(
                                    argument("displayName", ArgumentType::Component)
                                        .executes(set_objective_display),
                                ),
                            )
                            .then(
                                literal("rendertype")
                                    .then(
                                        literal("hearts")
                                            .executes(|ctx| set_objective_render_type(ctx, 1)),
                                    )
                                    .then(
                                        literal("integer")
                                            .executes(|ctx| set_objective_render_type(ctx, 0)),
                                    ),
                            ),
                    ),
                )
                .then(
                    literal("remove").then(
                        argument("objective", ArgumentType::opaque("minecraft:objective"))
                            .executes(remove_objective),
                    ),
                )
                .then(
                    literal("setdisplay").then(
                        argument("slot", ArgumentType::opaque("minecraft:scoreboard_slot"))
                            .executes(clear_display)
                            .then(
                                argument("objective", ArgumentType::opaque("minecraft:objective"))
                                    .executes(set_display),
                            ),
                    ),
                ),
        )
        .then(
            literal("players")
                .then(
                    literal("add").then(
                        argument("targets", ArgumentType::ScoreHolder { multiple: true }).then(
                            argument("objective", ArgumentType::opaque("minecraft:objective"))
                                .then(
                                    argument("score", ArgumentType::integer_range(0, i32::MAX))
                                        .executes(|ctx| add_score(ctx, false)),
                                ),
                        ),
                    ),
                )
                .then(
                    literal("enable").then(
                        argument("targets", ArgumentType::ScoreHolder { multiple: true }).then(
                            argument("objective", ArgumentType::opaque("minecraft:objective"))
                                .executes(enable_trigger),
                        ),
                    ),
                )
                .then(
                    literal("get").then(
                        argument("target", ArgumentType::ScoreHolder { multiple: false }).then(
                            argument("objective", ArgumentType::opaque("minecraft:objective"))
                                .executes(get_score),
                        ),
                    ),
                )
                .then(
                    literal("list").executes(list_all_scores).then(
                        argument("target", ArgumentType::ScoreHolder { multiple: false })
                            .executes(list_scores),
                    ),
                )
                .then(
                    literal("operation").then(
                        argument("targets", ArgumentType::ScoreHolder { multiple: true }).then(
                            argument(
                                "targetObjective",
                                ArgumentType::opaque("minecraft:objective"),
                            )
                            .then(
                                argument("operation", ArgumentType::opaque("minecraft:operation"))
                                    .then(
                                        argument(
                                            "source",
                                            ArgumentType::ScoreHolder { multiple: false },
                                        )
                                        .then(
                                            argument(
                                                "sourceObjective",
                                                ArgumentType::opaque("minecraft:objective"),
                                            )
                                            .executes(operate),
                                        ),
                                    ),
                            ),
                        ),
                    ),
                )
                .then(
                    literal("remove").then(
                        argument("targets", ArgumentType::ScoreHolder { multiple: true }).then(
                            argument("objective", ArgumentType::opaque("minecraft:objective"))
                                .then(
                                    argument("score", ArgumentType::integer_range(0, i32::MAX))
                                        .executes(|ctx| add_score(ctx, true)),
                                ),
                        ),
                    ),
                )
                .then(
                    literal("reset").then(
                        argument("targets", ArgumentType::ScoreHolder { multiple: true })
                            .executes(reset_all_scores)
                            .then(
                                argument("objective", ArgumentType::opaque("minecraft:objective"))
                                    .executes(reset_scores),
                            ),
                    ),
                )
                .then(
                    literal("set").then(
                        argument("targets", ArgumentType::ScoreHolder { multiple: true }).then(
                            argument("objective", ArgumentType::opaque("minecraft:objective"))
                                .then(
                                    argument("score", ArgumentType::integer()).executes(set_score),
                                ),
                        ),
                    ),
                ),
        )
}

fn add_objective(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let name = objective_name(ctx);
    let criteria = namespaced(ctx.string("criteria").unwrap_or_default());
    let display = ctx
        .string("displayName")
        .map(component_text)
        .unwrap_or_else(|| name.clone());
    {
        let mut state = ctx.source().state.lock().unwrap();
        if state.objectives.contains_key(&name) {
            return Err(CommandError::failed(
                "An objective already exists by that name",
            ));
        }
        state.objectives.insert(
            name.clone(),
            Objective {
                criteria,
                display_name: display.clone(),
                render_type: 0,
            },
        );
    }
    packets(
        ctx,
        Vec::new(),
        PlayerPacket::SetObjective(SetObjective {
            name: name.clone(),
            mode: 0,
            display_name: Some(PlainText(display)),
            render_type: Some(VarInt(0)),
        }),
    )?;
    feedback(
        ctx,
        "commands.scoreboard.objectives.add.success",
        vec![name],
    )
}

fn list_objectives(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let names = {
        let state = ctx.source().state.lock().unwrap();
        state.objectives.keys().cloned().collect::<Vec<_>>()
    };
    if names.is_empty() {
        feedback(ctx, "commands.scoreboard.objectives.list.empty", Vec::new())
    } else {
        feedback(
            ctx,
            "commands.scoreboard.objectives.list.success",
            vec![names.len().to_string(), names.join(", ")],
        )
    }
}

fn remove_objective(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let name = objective_name(ctx);
    {
        let mut state = ctx.source().state.lock().unwrap();
        if state.objectives.remove(&name).is_none() {
            return Err(CommandError::failed(format!("Unknown objective '{name}'")));
        }
        state.scores.retain(|(_, objective), _| objective != &name);
        state.displays.retain(|_, objective| objective != &name);
    }
    packets(
        ctx,
        Vec::new(),
        PlayerPacket::SetObjective(SetObjective {
            name: name.clone(),
            mode: 1,
            display_name: None,
            render_type: None,
        }),
    )?;
    feedback(
        ctx,
        "commands.scoreboard.objectives.remove.success",
        vec![name],
    )
}

fn set_objective_display(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let name = objective_name(ctx);
    let display = component_text(ctx.string("displayName").unwrap_or_default());
    let existing = {
        let mut state = ctx.source().state.lock().unwrap();
        let objective = state
            .objectives
            .get_mut(&name)
            .ok_or_else(|| CommandError::failed(format!("Unknown objective '{name}'")))?;
        objective.display_name = display.clone();
        objective.render_type
    };
    packets(
        ctx,
        Vec::new(),
        PlayerPacket::SetObjective(SetObjective {
            name: name.clone(),
            mode: 2,
            display_name: Some(PlainText(display.clone())),
            render_type: Some(VarInt(existing)),
        }),
    )?;
    feedback(
        ctx,
        "commands.scoreboard.objectives.modify.displayname",
        vec![name, display],
    )
}

fn set_objective_render_type(
    ctx: &CommandContext<Source>,
    render_type: i32,
) -> Result<i32, CommandError> {
    let name = objective_name(ctx);
    let display = {
        let mut state = ctx.source().state.lock().unwrap();
        let objective = state
            .objectives
            .get_mut(&name)
            .ok_or_else(|| CommandError::failed(format!("Unknown objective '{name}'")))?;
        objective.render_type = render_type;
        objective.display_name.clone()
    };
    packets(
        ctx,
        Vec::new(),
        PlayerPacket::SetObjective(SetObjective {
            name: name.clone(),
            mode: 2,
            display_name: Some(PlainText(display)),
            render_type: Some(VarInt(render_type)),
        }),
    )?;
    feedback(
        ctx,
        "commands.scoreboard.objectives.modify.rendertype",
        vec![name],
    )
}

fn slot_name(ctx: &CommandContext<Source>) -> String {
    ctx.string("slot").unwrap_or_default().to_owned()
}

fn clear_display(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let name = slot_name(ctx);
    let slot =
        slot_id(&name).ok_or_else(|| CommandError::failed(format!("Unknown slot '{name}'")))?;
    {
        let mut state = ctx.source().state.lock().unwrap();
        if state.displays.remove(&slot).is_none() {
            return Err(CommandError::failed(
                "Nothing changed. That display slot is already empty",
            ));
        }
    }
    packets(
        ctx,
        Vec::new(),
        PlayerPacket::SetDisplayObjective(SetDisplayObjective {
            slot: VarInt(slot),
            objective: String::new(),
        }),
    )?;
    feedback(
        ctx,
        "commands.scoreboard.objectives.display.cleared",
        vec![name],
    )
}

fn set_display(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let name = slot_name(ctx);
    let slot =
        slot_id(&name).ok_or_else(|| CommandError::failed(format!("Unknown slot '{name}'")))?;
    let objective = objective_name(ctx);
    {
        let mut state = ctx.source().state.lock().unwrap();
        require_objective(&state, &objective)?;
        if state.displays.get(&slot) == Some(&objective) {
            return Err(CommandError::failed(
                "Nothing changed. That display slot is already showing that objective",
            ));
        }
        state.displays.insert(slot, objective.clone());
    }
    packets(
        ctx,
        Vec::new(),
        PlayerPacket::SetDisplayObjective(SetDisplayObjective {
            slot: VarInt(slot),
            objective: objective.clone(),
        }),
    )?;
    feedback(
        ctx,
        "commands.scoreboard.objectives.display.set",
        vec![name, objective],
    )
}

fn set_score(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let targets = holders(ctx, "targets")?;
    let objective = objective_name(ctx);
    let score = ctx.integer("score").unwrap_or(0);
    {
        let mut state = ctx.source().state.lock().unwrap();
        require_objective(&state, &objective)?;
        for target in &targets {
            state
                .scores
                .insert((target.clone(), objective.clone()), score);
        }
    }
    for target in &targets {
        send_score(ctx, target, &objective, score)?;
    }
    if targets.len() == 1 {
        feedback(
            ctx,
            "commands.scoreboard.players.set.success.single",
            vec![objective, targets[0].clone(), score.to_string()],
        )
    } else {
        feedback(
            ctx,
            "commands.scoreboard.players.set.success.multiple",
            vec![objective, targets.len().to_string(), score.to_string()],
        )
    }
}

fn add_score(ctx: &CommandContext<Source>, remove: bool) -> Result<i32, CommandError> {
    let targets = holders(ctx, "targets")?;
    let objective = objective_name(ctx);
    let amount = ctx.integer("score").unwrap_or(0);
    let mut scored = Vec::new();
    {
        let mut state = ctx.source().state.lock().unwrap();
        require_objective(&state, &objective)?;
        for target in &targets {
            let key = (target.clone(), objective.clone());
            let current = state.scores.get(&key).copied().unwrap_or(0);
            let value = if remove {
                current.saturating_sub(amount)
            } else {
                current.saturating_add(amount)
            };
            state.scores.insert(key, value);
            scored.push((target.clone(), value));
        }
    }
    for (target, value) in &scored {
        send_score(ctx, target, &objective, *value)?;
    }
    let key = if remove {
        if scored.len() == 1 {
            "commands.scoreboard.players.remove.success.single"
        } else {
            "commands.scoreboard.players.remove.success.multiple"
        }
    } else if scored.len() == 1 {
        "commands.scoreboard.players.add.success.single"
    } else {
        "commands.scoreboard.players.add.success.multiple"
    };
    if scored.len() == 1 {
        feedback(
            ctx,
            key,
            vec![
                amount.to_string(),
                objective,
                scored[0].0.clone(),
                scored[0].1.to_string(),
            ],
        )
    } else {
        feedback(
            ctx,
            key,
            vec![amount.to_string(), objective, scored.len().to_string()],
        )
    }
}

fn get_score(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let targets = holders(ctx, "target")?;
    let target = targets
        .first()
        .ok_or_else(|| CommandError::failed("Missing score holder"))?
        .clone();
    let objective = objective_name(ctx);
    let value = {
        let state = ctx.source().state.lock().unwrap();
        require_objective(&state, &objective)?;
        state
            .scores
            .get(&(target.clone(), objective.clone()))
            .copied()
    };
    match value {
        Some(value) => feedback(
            ctx,
            "commands.scoreboard.players.get.success",
            vec![target, value.to_string(), objective],
        ),
        None => Err(CommandError::failed(format!(
            "Can't get value of {objective} for {target}; none is set"
        ))),
    }
}

fn list_all_scores(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let names = {
        let state = ctx.source().state.lock().unwrap();
        let mut names = BTreeSet::new();
        names.extend(state.scores.keys().map(|(holder, _)| holder.clone()));
        names.extend(state.players.iter().map(|player| player.name.clone()));
        names.into_iter().collect::<Vec<_>>()
    };
    if names.is_empty() {
        feedback(ctx, "commands.scoreboard.players.list.empty", Vec::new())
    } else {
        feedback(
            ctx,
            "commands.scoreboard.players.list.success",
            vec![names.len().to_string(), names.join(", ")],
        )
    }
}

fn list_scores(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let targets = holders(ctx, "target")?;
    let target = targets
        .first()
        .ok_or_else(|| CommandError::failed("Missing score holder"))?
        .clone();
    let scores = {
        let state = ctx.source().state.lock().unwrap();
        state
            .scores
            .iter()
            .filter(|((holder, _), _)| holder == &target)
            .map(|((_, objective), value)| (objective.clone(), *value))
            .collect::<Vec<_>>()
    };
    if scores.is_empty() {
        return feedback(
            ctx,
            "commands.scoreboard.players.list.entity.empty",
            vec![target],
        );
    }
    feedback(
        ctx,
        "commands.scoreboard.players.list.entity.success",
        vec![target.clone(), scores.len().to_string()],
    )?;
    for (objective, value) in scores {
        feedback(
            ctx,
            "commands.scoreboard.players.list.entity.entry",
            vec![objective, value.to_string()],
        )?;
    }
    Ok(1)
}

fn reset_all_scores(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let targets = holders(ctx, "targets")?;
    {
        let mut state = ctx.source().state.lock().unwrap();
        for target in &targets {
            state.scores.retain(|(holder, _), _| holder != target);
        }
    }
    for target in &targets {
        packets(
            ctx,
            Vec::new(),
            PlayerPacket::ResetScore(ResetScore {
                entity: target.clone(),
                objective: None,
            }),
        )?;
    }
    if targets.len() == 1 {
        feedback(
            ctx,
            "commands.scoreboard.players.reset.all.single",
            vec![targets[0].clone()],
        )
    } else {
        feedback(
            ctx,
            "commands.scoreboard.players.reset.all.multiple",
            vec![targets.len().to_string()],
        )
    }
}

fn reset_scores(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let targets = holders(ctx, "targets")?;
    let objective = objective_name(ctx);
    {
        let mut state = ctx.source().state.lock().unwrap();
        require_objective(&state, &objective)?;
        for target in &targets {
            state.scores.remove(&(target.clone(), objective.clone()));
        }
    }
    for target in &targets {
        packets(
            ctx,
            Vec::new(),
            PlayerPacket::ResetScore(ResetScore {
                entity: target.clone(),
                objective: Some(objective.clone()),
            }),
        )?;
    }
    if targets.len() == 1 {
        feedback(
            ctx,
            "commands.scoreboard.players.reset.specific.single",
            vec![objective, targets[0].clone()],
        )
    } else {
        feedback(
            ctx,
            "commands.scoreboard.players.reset.specific.multiple",
            vec![objective, targets.len().to_string()],
        )
    }
}

fn operate(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let targets = holders(ctx, "targets")?;
    let source = holders(ctx, "source")?
        .first()
        .ok_or_else(|| CommandError::failed("Missing source score holder"))?
        .clone();
    let target_objective = ctx.string("targetObjective").unwrap_or_default().to_owned();
    let source_objective = ctx.string("sourceObjective").unwrap_or_default().to_owned();
    let operation = ctx.string("operation").unwrap_or_default().to_owned();
    let (_, results) = {
        let mut state = ctx.source().state.lock().unwrap();
        require_objective(&state, &target_objective)?;
        require_objective(&state, &source_objective)?;
        let source_value = state
            .scores
            .get(&(source.clone(), source_objective.clone()))
            .copied()
            .unwrap_or(0);
        let mut results = Vec::new();
        for target in &targets {
            let key = (target.clone(), target_objective.clone());
            let current = state.scores.get(&key).copied().unwrap_or(0);
            let value = match operation.as_str() {
                "=" => source_value,
                "+=" => current.saturating_add(source_value),
                "-=" => current.saturating_sub(source_value),
                "*=" => current.saturating_mul(source_value),
                "/=" => {
                    if source_value == 0 {
                        return Err(CommandError::failed("Cannot divide by zero"));
                    }
                    current / source_value
                }
                "%=" => {
                    if source_value == 0 {
                        return Err(CommandError::failed("Cannot divide by zero"));
                    }
                    current % source_value
                }
                "<" => current.min(source_value),
                ">" => current.max(source_value),
                "><" => source_value,
                other => {
                    return Err(CommandError::failed(format!("Unknown operation '{other}'")));
                }
            };
            state.scores.insert(key, value);
            results.push((target.clone(), value));
        }
        if operation == "><" {
            state
                .scores
                .insert((source.clone(), source_objective.clone()), results[0].1);
        }
        (source_value, results)
    };
    for (target, value) in &results {
        send_score(ctx, target, &target_objective, *value)?;
    }
    if results.len() == 1 {
        feedback(
            ctx,
            "commands.scoreboard.players.operation.success.single",
            vec![
                target_objective,
                results[0].0.clone(),
                results[0].1.to_string(),
            ],
        )
    } else {
        feedback(
            ctx,
            "commands.scoreboard.players.operation.success.multiple",
            vec![target_objective, results.len().to_string()],
        )
    }
}

fn enable_trigger(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let targets = holders(ctx, "targets")?;
    let objective = objective_name(ctx);
    {
        let mut state = ctx.source().state.lock().unwrap();
        let entry = require_objective(&state, &objective)?;
        if !entry.criteria.ends_with("trigger") {
            return Err(CommandError::failed(
                "Enable only works on trigger-objectives",
            ));
        }
        // `/trigger` only accepts scores a player primed for that objective.
        for target in &targets {
            state
                .trigger_enabled
                .insert((objective.clone(), target.clone()));
        }
    }
    let objective = objective_name(ctx);
    if targets.len() == 1 {
        feedback(
            ctx,
            "commands.scoreboard.players.enable.success.single",
            vec![objective, targets[0].clone()],
        )
    } else {
        feedback(
            ctx,
            "commands.scoreboard.players.enable.success.multiple",
            vec![objective, targets.len().to_string()],
        )
    }
}

// ---------------------------------------------------------------------------------------------
// /bossbar
// ---------------------------------------------------------------------------------------------

fn bossbar_id(ctx: &CommandContext<Source>) -> String {
    namespaced(ctx.string("id").unwrap_or_default())
}

fn bossbar_tree() -> NodeBuilder<Source> {
    literal::<Source>("bossbar")
        .requires(gamemasters())
        .restricted(true)
        .then(
            literal("add").then(
                argument("id", ArgumentType::ResourceLocation)
                    .then(argument("name", ArgumentType::Component).executes(add_bossbar)),
            ),
        )
        .then(
            literal("get").then(
                argument("id", ArgumentType::ResourceLocation)
                    .then(literal("max").executes(|ctx| get_bossbar(ctx, BossBarKind::Max)))
                    .then(literal("value").executes(|ctx| get_bossbar(ctx, BossBarKind::Value)))
                    .then(literal("players").executes(|ctx| get_bossbar(ctx, BossBarKind::Players)))
                    .then(
                        literal("visible").executes(|ctx| get_bossbar(ctx, BossBarKind::Visible)),
                    ),
            ),
        )
        .then(literal("list").executes(list_bossbars))
        .then(
            literal("remove")
                .then(argument("id", ArgumentType::ResourceLocation).executes(remove_bossbar)),
        )
        .then(
            literal("set").then(
                argument("id", ArgumentType::ResourceLocation)
                    .then({
                        let mut color = literal("color");
                        for (name, id) in [
                            ("blue", 1),
                            ("green", 3),
                            ("pink", 0),
                            ("purple", 5),
                            ("red", 2),
                            ("white", 6),
                            ("yellow", 4),
                        ] {
                            color = color.then(
                                literal::<Source>(name)
                                    .executes(move |ctx| set_bossbar_color(ctx, id)),
                            );
                        }
                        color
                    })
                    .then(
                        literal("max").then(
                            argument("max", ArgumentType::integer_range(1, i32::MAX))
                                .executes(set_bossbar_max),
                        ),
                    )
                    .then(
                        literal("name").then(
                            argument("name", ArgumentType::Component).executes(set_bossbar_name),
                        ),
                    )
                    .then(literal("players").executes(clear_bossbar_players).then(
                        argument("targets", ArgumentType::players()).executes(set_bossbar_players),
                    ))
                    .then({
                        let mut style = literal("style");
                        for (name, id) in [
                            ("notched_6", 1),
                            ("notched_10", 2),
                            ("notched_12", 3),
                            ("notched_20", 4),
                            ("progress", 0),
                        ] {
                            style = style.then(
                                literal::<Source>(name)
                                    .executes(move |ctx| set_bossbar_style(ctx, id)),
                            );
                        }
                        style
                    })
                    .then(
                        literal("value").then(
                            argument("value", ArgumentType::integer_range(0, i32::MAX))
                                .executes(set_bossbar_value),
                        ),
                    )
                    .then(literal("visible").then(
                        argument("visible", ArgumentType::bool()).executes(set_bossbar_visible),
                    )),
            ),
        )
}

#[derive(Debug, Clone, Copy)]
enum BossBarKind {
    Max,
    Value,
    Players,
    Visible,
}

fn add_bossbar(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let id = bossbar_id(ctx);
    let name = component_text(ctx.string("name").unwrap_or_default());
    let uuid = Uuid::new_v4();
    {
        let mut state = ctx.source().state.lock().unwrap();
        if state.bossbars.contains_key(&id) {
            return Err(CommandError::failed(format!(
                "A bossbar already exists with the ID '{id}'"
            )));
        }
        state.bossbars.insert(
            id.clone(),
            BossBarState {
                uuid,
                name: name.clone(),
                progress: 0.0,
                value: 0,
                max: 100,
                color: 0,
                overlay: 0,
                visible: true,
                players: BTreeSet::new(),
            },
        );
    }
    packets(
        ctx,
        Vec::new(),
        PlayerPacket::BossBar(BossBar {
            uuid,
            action: BossBarAction::Add {
                name: PlainText(name),
                progress: 0.0,
                color: VarInt(0),
                overlay: 0,
                flags: 0,
            },
        }),
    )?;
    feedback(ctx, "commands.bossbar.create.success", vec![id])
}

fn remove_bossbar(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let id = bossbar_id(ctx);
    let uuid = {
        let mut state = ctx.source().state.lock().unwrap();
        state
            .bossbars
            .remove(&id)
            .ok_or_else(|| CommandError::failed(format!("No bossbar exists with the ID '{id}'")))?
            .uuid
    };
    packets(
        ctx,
        Vec::new(),
        PlayerPacket::BossBar(BossBar {
            uuid,
            action: BossBarAction::Remove,
        }),
    )?;
    feedback(ctx, "commands.bossbar.remove.success", vec![id])
}

fn list_bossbars(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let names = {
        let state = ctx.source().state.lock().unwrap();
        state.bossbars.keys().cloned().collect::<Vec<_>>()
    };
    if names.is_empty() {
        feedback(ctx, "commands.bossbar.list.bars.none", Vec::new())
    } else {
        feedback(
            ctx,
            "commands.bossbar.list.bars.some",
            vec![names.len().to_string(), names.join(", ")],
        )
    }
}

fn bossbar_state(ctx: &CommandContext<Source>, id: &str) -> Result<BossBarState, CommandError> {
    ctx.source()
        .state
        .lock()
        .unwrap()
        .bossbars
        .get(id)
        .cloned()
        .ok_or_else(|| CommandError::failed(format!("No bossbar exists with the ID '{id}'")))
}

fn online_names(ctx: &CommandContext<Source>) -> Vec<String> {
    ctx.source()
        .state
        .lock()
        .unwrap()
        .players
        .iter()
        .map(|player| player.name.clone())
        .collect()
}

fn get_bossbar(ctx: &CommandContext<Source>, kind: BossBarKind) -> Result<i32, CommandError> {
    let id = bossbar_id(ctx);
    let bar = bossbar_state(ctx, &id)?;
    match kind {
        BossBarKind::Max => feedback(
            ctx,
            "commands.bossbar.get.max",
            vec![id, bar.max.to_string()],
        ),
        BossBarKind::Value => feedback(
            ctx,
            "commands.bossbar.get.value",
            vec![id, bar.value.to_string()],
        ),
        BossBarKind::Players => {
            let names = if bar.players.is_empty() {
                online_names(ctx)
            } else {
                let state = ctx.source().state.lock().unwrap();
                bar.players
                    .iter()
                    .filter_map(|uuid| {
                        state
                            .players
                            .iter()
                            .find(|player| player.uuid == *uuid)
                            .map(|player| player.name.clone())
                    })
                    .collect()
            };
            if names.is_empty() {
                feedback(ctx, "commands.bossbar.get.players.none", vec![id])
            } else {
                feedback(
                    ctx,
                    "commands.bossbar.get.players.some",
                    vec![id, names.len().to_string(), names.join(", ")],
                )
            }
        }
        BossBarKind::Visible => feedback(
            ctx,
            if bar.visible {
                "commands.bossbar.get.visible.visible"
            } else {
                "commands.bossbar.get.visible.hidden"
            },
            vec![id],
        ),
    }
}

fn set_bossbar_color(ctx: &CommandContext<Source>, color: i32) -> Result<i32, CommandError> {
    let id = bossbar_id(ctx);
    let (uuid, overlay) = {
        let mut state = ctx.source().state.lock().unwrap();
        let bar = state
            .bossbars
            .get_mut(&id)
            .ok_or_else(|| CommandError::failed(format!("No bossbar exists with the ID '{id}'")))?;
        if bar.color == color {
            return Err(CommandError::failed(
                "Nothing changed. That's already the color of this bossbar",
            ));
        }
        bar.color = color;
        (bar.uuid, bar.overlay)
    };
    packets(
        ctx,
        Vec::new(),
        PlayerPacket::BossBar(BossBar {
            uuid,
            action: BossBarAction::UpdateStyle {
                color: VarInt(color),
                overlay: overlay as u8,
            },
        }),
    )?;
    feedback(ctx, "commands.bossbar.set.color.success", vec![id])
}

fn set_bossbar_style(ctx: &CommandContext<Source>, overlay: i32) -> Result<i32, CommandError> {
    let id = bossbar_id(ctx);
    let (uuid, color) = {
        let mut state = ctx.source().state.lock().unwrap();
        let bar = state
            .bossbars
            .get_mut(&id)
            .ok_or_else(|| CommandError::failed(format!("No bossbar exists with the ID '{id}'")))?;
        if bar.overlay == overlay {
            return Err(CommandError::failed(
                "Nothing changed. That's already the style of this bossbar",
            ));
        }
        bar.overlay = overlay;
        (bar.uuid, bar.color)
    };
    packets(
        ctx,
        Vec::new(),
        PlayerPacket::BossBar(BossBar {
            uuid,
            action: BossBarAction::UpdateStyle {
                color: VarInt(color),
                overlay: overlay as u8,
            },
        }),
    )?;
    feedback(ctx, "commands.bossbar.set.style.success", vec![id])
}

fn set_bossbar_max(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let id = bossbar_id(ctx);
    let max = ctx.integer("max").unwrap_or(1);
    let (uuid, progress) = {
        let mut state = ctx.source().state.lock().unwrap();
        let bar = state
            .bossbars
            .get_mut(&id)
            .ok_or_else(|| CommandError::failed(format!("No bossbar exists with the ID '{id}'")))?;
        bar.max = max;
        bar.value = bar.value.min(max);
        bar.progress = (bar.value as f32 / max as f32).clamp(0.0, 1.0);
        (bar.uuid, bar.progress)
    };
    packets(
        ctx,
        Vec::new(),
        PlayerPacket::BossBar(BossBar {
            uuid,
            action: BossBarAction::UpdateProgress(progress),
        }),
    )?;
    feedback(
        ctx,
        "commands.bossbar.set.max.success",
        vec![id, max.to_string()],
    )
}

fn set_bossbar_value(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let id = bossbar_id(ctx);
    let value = ctx.integer("value").unwrap_or(0);
    let (uuid, progress) = {
        let mut state = ctx.source().state.lock().unwrap();
        let bar = state
            .bossbars
            .get_mut(&id)
            .ok_or_else(|| CommandError::failed(format!("No bossbar exists with the ID '{id}'")))?;
        bar.value = value;
        bar.progress = (value as f32 / bar.max as f32).clamp(0.0, 1.0);
        (bar.uuid, bar.progress)
    };
    packets(
        ctx,
        Vec::new(),
        PlayerPacket::BossBar(BossBar {
            uuid,
            action: BossBarAction::UpdateProgress(progress),
        }),
    )?;
    feedback(
        ctx,
        "commands.bossbar.set.value.success",
        vec![id, value.to_string()],
    )
}

fn set_bossbar_name(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let id = bossbar_id(ctx);
    let name = component_text(ctx.string("name").unwrap_or_default());
    let uuid = {
        let mut state = ctx.source().state.lock().unwrap();
        let bar = state
            .bossbars
            .get_mut(&id)
            .ok_or_else(|| CommandError::failed(format!("No bossbar exists with the ID '{id}'")))?;
        if bar.name == name {
            return Err(CommandError::failed(
                "Nothing changed. That's already the name of this bossbar",
            ));
        }
        bar.name = name.clone();
        bar.uuid
    };
    packets(
        ctx,
        Vec::new(),
        PlayerPacket::BossBar(BossBar {
            uuid,
            action: BossBarAction::UpdateName(PlainText(name)),
        }),
    )?;
    feedback(ctx, "commands.bossbar.set.name.success", vec![id])
}

fn clear_bossbar_players(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let id = bossbar_id(ctx);
    {
        let mut state = ctx.source().state.lock().unwrap();
        let bar = state
            .bossbars
            .get_mut(&id)
            .ok_or_else(|| CommandError::failed(format!("No bossbar exists with the ID '{id}'")))?;
        if bar.players.is_empty() {
            return Err(CommandError::failed(
                "Nothing changed. Those players are already on the bossbar with nobody to add or remove",
            ));
        }
        bar.players.clear();
    }
    feedback(ctx, "commands.bossbar.set.players.success.none", vec![id])
}

fn set_bossbar_players(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let id = bossbar_id(ctx);
    let targets = resolved_targets(ctx, "targets")?;
    let names = {
        let state = ctx.source().state.lock().unwrap();
        targets
            .iter()
            .filter_map(|uuid| {
                state
                    .players
                    .iter()
                    .find(|player| player.uuid == *uuid)
                    .map(|player| player.name.clone())
            })
            .collect::<Vec<_>>()
    };
    {
        let mut state = ctx.source().state.lock().unwrap();
        let bar = state
            .bossbars
            .get_mut(&id)
            .ok_or_else(|| CommandError::failed(format!("No bossbar exists with the ID '{id}'")))?;
        bar.players = targets.into_iter().collect();
    }
    feedback(
        ctx,
        "commands.bossbar.set.players.success.some",
        vec![id, names.len().to_string(), names.join(", ")],
    )
}

fn set_bossbar_visible(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let id = bossbar_id(ctx);
    let visible = ctx
        .argument("visible")
        .and_then(ArgumentValue::as_bool)
        .ok_or_else(|| CommandError::failed("Missing visibility"))?;
    let bar = {
        let mut state = ctx.source().state.lock().unwrap();
        let bar = state
            .bossbars
            .get_mut(&id)
            .ok_or_else(|| CommandError::failed(format!("No bossbar exists with the ID '{id}'")))?;
        if bar.visible == visible {
            return Err(CommandError::failed(if visible {
                "Nothing changed. The bossbar is already visible"
            } else {
                "Nothing changed. The bossbar is already hidden"
            }));
        }
        bar.visible = visible;
        bar.clone()
    };
    let action = if visible {
        BossBarAction::Add {
            name: PlainText(bar.name),
            progress: bar.progress,
            color: VarInt(bar.color),
            overlay: bar.overlay as u8,
            flags: 0,
        }
    } else {
        BossBarAction::Remove
    };
    packets(
        ctx,
        Vec::new(),
        PlayerPacket::BossBar(BossBar {
            uuid: bar.uuid,
            action,
        }),
    )?;
    feedback(
        ctx,
        if visible {
            "commands.bossbar.set.visible.success.visible"
        } else {
            "commands.bossbar.set.visible.success.hidden"
        },
        vec![id],
    )
}

// ---------------------------------------------------------------------------------------------
// /tag
// ---------------------------------------------------------------------------------------------

fn tag_tree() -> NodeBuilder<Source> {
    literal::<Source>("tag")
        .requires(gamemasters())
        .restricted(true)
        .then(
            argument("targets", ArgumentType::entities()).then(
                literal("add").then(argument("name", ArgumentType::word()).executes(add_tag)),
            ),
        )
        .then(
            argument("targets", ArgumentType::entities())
                .then(literal("list").executes(list_tags))
                .then(
                    literal("remove")
                        .then(argument("name", ArgumentType::word()).executes(remove_tag)),
                ),
        )
}

fn tag_targets(ctx: &CommandContext<Source>) -> Result<Vec<Uuid>, CommandError> {
    resolved_targets(ctx, "targets")
}

fn add_tag(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let targets = tag_targets(ctx)?;
    let name = ctx.string("name").unwrap_or_default().to_owned();
    let mut added = 0;
    {
        let mut state = ctx.source().state.lock().unwrap();
        for target in &targets {
            let Some(player) = state
                .players
                .iter_mut()
                .find(|player| player.uuid == *target)
            else {
                continue;
            };
            if player.tags.insert(name.clone()) {
                added += 1;
            }
        }
    }
    if added == 0 {
        return Err(CommandError::failed(
            "Target either already has the tag or has too many tags",
        ));
    }
    tag_feedback(ctx, &targets, &name, added, "add")
}

fn remove_tag(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let targets = tag_targets(ctx)?;
    let name = ctx.string("name").unwrap_or_default().to_owned();
    let mut removed = 0;
    {
        let mut state = ctx.source().state.lock().unwrap();
        for target in &targets {
            let Some(player) = state
                .players
                .iter_mut()
                .find(|player| player.uuid == *target)
            else {
                continue;
            };
            if player.tags.remove(&name) {
                removed += 1;
            }
        }
    }
    if removed == 0 {
        return Err(CommandError::failed("Target does not have this tag"));
    }
    tag_feedback(ctx, &targets, &name, removed, "remove")
}

fn tag_feedback(
    ctx: &CommandContext<Source>,
    targets: &[Uuid],
    name: &str,
    changed: usize,
    action: &str,
) -> Result<i32, CommandError> {
    let single = match action {
        "add" => "commands.tag.add.success.single",
        _ => "commands.tag.remove.success.single",
    };
    let multiple = match action {
        "add" => "commands.tag.add.success.multiple",
        _ => "commands.tag.remove.success.multiple",
    };
    if targets.len() == 1 {
        let target = target_name(ctx, targets[0]);
        feedback(ctx, single, vec![name.to_owned(), target])
    } else {
        let _ = changed;
        feedback(
            ctx,
            multiple,
            vec![name.to_owned(), targets.len().to_string()],
        )
    }
}

fn target_name(ctx: &CommandContext<Source>, uuid: Uuid) -> String {
    ctx.source()
        .state
        .lock()
        .unwrap()
        .players
        .iter()
        .find(|player| player.uuid == uuid)
        .map(|player| player.name.clone())
        .unwrap_or_else(|| uuid.to_string())
}

fn list_tags(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let targets = tag_targets(ctx)?;
    let tags = {
        let state = ctx.source().state.lock().unwrap();
        let mut tags = BTreeSet::new();
        for target in &targets {
            if let Some(player) = state.players.iter().find(|player| player.uuid == *target) {
                tags.extend(player.tags.iter().cloned());
            }
        }
        tags.into_iter().collect::<Vec<_>>()
    };
    if targets.len() == 1 {
        let target = target_name(ctx, targets[0]);
        if tags.is_empty() {
            feedback(ctx, "commands.tag.list.single.empty", vec![target])
        } else {
            feedback(
                ctx,
                "commands.tag.list.single.success",
                vec![target, tags.len().to_string(), tags.join(", ")],
            )
        }
    } else if tags.is_empty() {
        feedback(
            ctx,
            "commands.tag.list.multiple.empty",
            vec![targets.len().to_string()],
        )
    } else {
        feedback(
            ctx,
            "commands.tag.list.multiple.success",
            vec![
                targets.len().to_string(),
                tags.len().to_string(),
                tags.join(", "),
            ],
        )
    }
}

// ---------------------------------------------------------------------------------------------
// /team
// ---------------------------------------------------------------------------------------------

fn team_name(ctx: &CommandContext<Source>) -> String {
    ctx.string("team").unwrap_or_default().to_owned()
}

fn team_tree() -> NodeBuilder<Source> {
    literal::<Source>("team")
        .requires(gamemasters())
        .restricted(true)
        .then(
            literal("add").then(
                argument("team", ArgumentType::word())
                    .executes(add_team)
                    .then(argument("displayName", ArgumentType::Component).executes(add_team)),
            ),
        )
        .then(
            literal("empty").then(
                argument("team", ArgumentType::opaque("minecraft:team")).executes(empty_team),
            ),
        )
        .then(
            literal("join").then(
                argument("team", ArgumentType::opaque("minecraft:team")).then(
                    argument("members", ArgumentType::ScoreHolder { multiple: true })
                        .executes(join_team),
                ),
            ),
        )
        .then(literal("leave").then(
            argument("members", ArgumentType::ScoreHolder { multiple: true }).executes(leave_team),
        ))
        .then(
            literal("list")
                .executes(list_teams)
                .then(argument("team", ArgumentType::opaque("minecraft:team")).executes(list_team)),
        )
        .then(
            literal("modify").then(
                argument("team", ArgumentType::opaque("minecraft:team"))
                    .then({
                        let mut color = literal("color");
                        color = color.then(
                            literal::<Source>("reset")
                                .executes(|ctx| team_modify(ctx, TeamModify::Color(None))),
                        );
                        for (name, id) in TEAM_COLORS {
                            color = color.then(literal::<Source>(name).executes(move |ctx| {
                                team_modify(ctx, TeamModify::Color(Some(id)))
                            }));
                        }
                        color
                    })
                    .then(
                        literal("displayName").then(
                            argument("displayName", ArgumentType::Component)
                                .executes(|ctx| team_modify(ctx, TeamModify::DisplayName)),
                        ),
                    )
                    .then(
                        literal("friendlyFire").then(
                            argument("allowed", ArgumentType::bool())
                                .executes(|ctx| team_modify(ctx, TeamModify::FriendlyFire)),
                        ),
                    )
                    .then(
                        literal("prefix").then(
                            argument("prefix", ArgumentType::Component)
                                .executes(|ctx| team_modify(ctx, TeamModify::Prefix)),
                        ),
                    )
                    .then(
                        literal("suffix").then(
                            argument("suffix", ArgumentType::Component)
                                .executes(|ctx| team_modify(ctx, TeamModify::Suffix)),
                        ),
                    )
                    .then(
                        literal("seeFriendlyInvisibles").then(
                            argument("allowed", ArgumentType::bool()).executes(|ctx| {
                                team_modify(ctx, TeamModify::SeeFriendlyInvisibles)
                            }),
                        ),
                    )
                    .then(literal("nametagVisibility").then(nametag_visibility_literals()))
                    .then(literal("collisionRule").then(collision_literals()))
                    .then(literal("deathMessageVisibility").then(death_visibility_literals())),
            ),
        )
        .then(
            literal("remove").then(
                argument("team", ArgumentType::opaque("minecraft:team")).executes(remove_team),
            ),
        )
}

fn nametag_visibility_literals() -> NodeBuilder<Source> {
    literal::<Source>("always")
        .executes(|ctx| team_modify(ctx, TeamModify::NametagVisibility("always")))
        .then(
            literal::<Source>("hideForOtherTeams").executes(|ctx| {
                team_modify(ctx, TeamModify::NametagVisibility("hideForOtherTeams"))
            }),
        )
        .then(
            literal::<Source>("hideForOwnTeam")
                .executes(|ctx| team_modify(ctx, TeamModify::NametagVisibility("hideForOwnTeam"))),
        )
        .then(
            literal::<Source>("never")
                .executes(|ctx| team_modify(ctx, TeamModify::NametagVisibility("never"))),
        )
}

fn death_visibility_literals() -> NodeBuilder<Source> {
    literal::<Source>("always")
        .executes(|ctx| team_modify(ctx, TeamModify::DeathMessageVisibility("always")))
        .then(literal::<Source>("hideForOtherTeams").executes(|ctx| {
            team_modify(ctx, TeamModify::DeathMessageVisibility("hideForOtherTeams"))
        }))
        .then(
            literal::<Source>("hideForOwnTeam").executes(|ctx| {
                team_modify(ctx, TeamModify::DeathMessageVisibility("hideForOwnTeam"))
            }),
        )
        .then(
            literal::<Source>("never")
                .executes(|ctx| team_modify(ctx, TeamModify::DeathMessageVisibility("never"))),
        )
}

fn collision_literals() -> NodeBuilder<Source> {
    literal::<Source>("always")
        .executes(|ctx| team_modify(ctx, TeamModify::CollisionRule("always")))
        .then(
            literal::<Source>("never")
                .executes(|ctx| team_modify(ctx, TeamModify::CollisionRule("never"))),
        )
        .then(
            literal::<Source>("pushOtherTeams")
                .executes(|ctx| team_modify(ctx, TeamModify::CollisionRule("pushOtherTeams"))),
        )
        .then(
            literal::<Source>("pushOwnTeam")
                .executes(|ctx| team_modify(ctx, TeamModify::CollisionRule("pushOwnTeam"))),
        )
}

#[derive(Debug, Clone, Copy)]
enum TeamModify {
    Color(Option<i32>),
    DisplayName,
    FriendlyFire,
    Prefix,
    Suffix,
    SeeFriendlyInvisibles,
    NametagVisibility(&'static str),
    CollisionRule(&'static str),
    DeathMessageVisibility(&'static str),
}

fn team_packet(name: &str, team: &TeamState) -> PlayerTeam {
    PlayerTeam {
        name: name.to_owned(),
        action: TeamAction::Add {
            display_name: PlainText(team.display_name.clone()),
            flags: u8::from(team.friendly_fire) | (u8::from(team.see_friendly_invisibles) << 1),
            nametag_visibility: team.nametag_visibility.clone(),
            collision_rule: team.collision_rule.clone(),
            color: VarInt(team.color.max(0)),
            prefix: PlainText(team.prefix.clone()),
            suffix: PlainText(team.suffix.clone()),
        },
    }
}

fn add_team(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let name = team_name(ctx);
    let display = ctx
        .string("displayName")
        .map(component_text)
        .unwrap_or_else(|| name.clone());
    let team = {
        let mut state = ctx.source().state.lock().unwrap();
        if state.teams.contains_key(&name) {
            return Err(CommandError::failed("A team already exists by that name"));
        }
        let team = TeamState {
            display_name: display,
            color: 15,
            prefix: String::new(),
            suffix: String::new(),
            friendly_fire: true,
            see_friendly_invisibles: false,
            nametag_visibility: "always".into(),
            collision_rule: "always".into(),
            death_message_visibility: "always".into(),
            members: BTreeSet::new(),
        };
        state.teams.insert(name.clone(), team.clone());
        team
    };
    packets(
        ctx,
        Vec::new(),
        PlayerPacket::PlayerTeam(team_packet(&name, &team)),
    )?;
    feedback(ctx, "commands.team.add.success", vec![name])
}

fn remove_team(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let name = team_name(ctx);
    {
        let mut state = ctx.source().state.lock().unwrap();
        let team = state
            .teams
            .remove(&name)
            .ok_or_else(|| CommandError::failed(format!("Unknown team '{name}'")))?;
        for member in &team.members {
            if let Some(player) = state
                .players
                .iter_mut()
                .find(|player| player.uuid == *member)
            {
                player.team = None;
            }
        }
    }
    packets(
        ctx,
        Vec::new(),
        PlayerPacket::PlayerTeam(PlayerTeam {
            name: name.clone(),
            action: TeamAction::Remove,
        }),
    )?;
    feedback(ctx, "commands.team.remove.success", vec![name])
}

fn empty_team(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let name = team_name(ctx);
    let members = {
        let mut state = ctx.source().state.lock().unwrap();
        let team = state
            .teams
            .get_mut(&name)
            .ok_or_else(|| CommandError::failed(format!("Unknown team '{name}'")))?;
        if team.members.is_empty() {
            return Err(CommandError::failed(
                "Nothing changed. That team is already empty",
            ));
        }
        let members = std::mem::take(&mut team.members);
        for member in &members {
            if let Some(player) = state
                .players
                .iter_mut()
                .find(|player| player.uuid == *member)
            {
                player.team = None;
            }
        }
        members
    };
    let names = member_names(ctx, &members);
    packets(
        ctx,
        Vec::new(),
        PlayerPacket::PlayerTeam(PlayerTeam {
            name: name.clone(),
            action: TeamAction::Leave(names),
        }),
    )?;
    feedback(
        ctx,
        "commands.team.empty.success",
        vec![members.len().to_string(), name],
    )
}

fn member_names(ctx: &CommandContext<Source>, members: &BTreeSet<Uuid>) -> Vec<String> {
    let state = ctx.source().state.lock().unwrap();
    members
        .iter()
        .map(|uuid| {
            state
                .players
                .iter()
                .find(|player| player.uuid == *uuid)
                .map(|player| player.name.clone())
                .unwrap_or_else(|| uuid.to_string())
        })
        .collect()
}

fn member_uuids(ctx: &CommandContext<Source>) -> Result<Vec<(Uuid, String)>, CommandError> {
    let names = holders(ctx, "members")?;
    let state = ctx.source().state.lock().unwrap();
    Ok(names
        .into_iter()
        .map(|name| {
            let uuid = state
                .players
                .iter()
                .find(|player| player.name.eq_ignore_ascii_case(&name))
                .map(|player| player.uuid)
                .unwrap_or_else(|| super::commands::offline_uuid(&name));
            (uuid, name)
        })
        .collect())
}

fn join_team(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let name = team_name(ctx);
    let members = member_uuids(ctx)?;
    {
        let mut state = ctx.source().state.lock().unwrap();
        if !state.teams.contains_key(&name) {
            return Err(CommandError::failed(format!("Unknown team '{name}'")));
        }
        for (uuid, _) in &members {
            for other in state.teams.values_mut() {
                other.members.remove(uuid);
            }
        }
        if let Some(team) = state.teams.get_mut(&name) {
            for (uuid, _) in &members {
                team.members.insert(*uuid);
            }
        }
    }
    {
        let mut state = ctx.source().state.lock().unwrap();
        for (uuid, _) in &members {
            if let Some(player) = state.players.iter_mut().find(|player| player.uuid == *uuid) {
                player.team = Some(name.clone());
            }
        }
    }
    let names = members.iter().map(|(_, name)| name.clone()).collect();
    packets(
        ctx,
        Vec::new(),
        PlayerPacket::PlayerTeam(PlayerTeam {
            name: name.clone(),
            action: TeamAction::Join(names),
        }),
    )?;
    if members.len() == 1 {
        feedback(
            ctx,
            "commands.team.join.success.single",
            vec![members[0].1.clone(), name],
        )
    } else {
        feedback(
            ctx,
            "commands.team.join.success.multiple",
            vec![members.len().to_string(), name],
        )
    }
}

fn leave_team(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let members = member_uuids(ctx)?;
    let names = members
        .iter()
        .map(|(_, name)| name.clone())
        .collect::<Vec<_>>();
    {
        let mut state = ctx.source().state.lock().unwrap();
        for (uuid, _) in &members {
            for team in state.teams.values_mut() {
                team.members.remove(uuid);
            }
            if let Some(player) = state.players.iter_mut().find(|player| player.uuid == *uuid) {
                player.team = None;
            }
        }
    }
    packets(
        ctx,
        Vec::new(),
        PlayerPacket::PlayerTeam(PlayerTeam {
            name: String::new(),
            action: TeamAction::Leave(names.clone()),
        }),
    )?;
    if names.len() == 1 {
        feedback(
            ctx,
            "commands.team.leave.success.single",
            vec![names[0].clone()],
        )
    } else {
        feedback(
            ctx,
            "commands.team.leave.success.multiple",
            vec![names.len().to_string()],
        )
    }
}

fn list_teams(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let names = {
        let state = ctx.source().state.lock().unwrap();
        state.teams.keys().cloned().collect::<Vec<_>>()
    };
    if names.is_empty() {
        feedback(ctx, "commands.team.list.teams.empty", Vec::new())
    } else {
        feedback(
            ctx,
            "commands.team.list.teams.success",
            vec![names.len().to_string(), names.join(", ")],
        )
    }
}

fn list_team(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let name = team_name(ctx);
    let members = {
        let state = ctx.source().state.lock().unwrap();
        let team = state
            .teams
            .get(&name)
            .ok_or_else(|| CommandError::failed(format!("Unknown team '{name}'")))?;
        team.members.clone()
    };
    let names = member_names(ctx, &members);
    if names.is_empty() {
        feedback(ctx, "commands.team.list.members.empty", vec![name])
    } else {
        feedback(
            ctx,
            "commands.team.list.members.success",
            vec![name, names.len().to_string(), names.join(", ")],
        )
    }
}

fn team_modify(ctx: &CommandContext<Source>, option: TeamModify) -> Result<i32, CommandError> {
    let name = team_name(ctx);
    let key = match option {
        TeamModify::Color(_) => "commands.team.option.color.success",
        TeamModify::DisplayName => "commands.team.option.name.success",
        TeamModify::FriendlyFire => "commands.team.option.friendlyfire.enabled",
        TeamModify::Prefix => "commands.team.option.prefix.success",
        TeamModify::Suffix => "commands.team.option.suffix.success",
        TeamModify::SeeFriendlyInvisibles => "commands.team.option.seeFriendlyInvisibles.enabled",
        TeamModify::NametagVisibility(_) => "commands.team.option.nametagVisibility.success",
        TeamModify::CollisionRule(_) => "commands.team.option.collisionRule.success",
        TeamModify::DeathMessageVisibility(_) => {
            "commands.team.option.deathMessageVisibility.success"
        }
    };
    let args;
    let team = {
        let mut state = ctx.source().state.lock().unwrap();
        let team = state
            .teams
            .get_mut(&name)
            .ok_or_else(|| CommandError::failed(format!("Unknown team '{name}'")))?;
        match option {
            TeamModify::Color(color) => {
                if team.color == color.unwrap_or(-1) {
                    return Err(CommandError::failed(
                        "Nothing changed. That team already has that color",
                    ));
                }
                team.color = color.unwrap_or(-1);
                match color {
                    Some(color) => {
                        let color_name = TEAM_COLORS
                            .iter()
                            .find(|(_, id)| *id == color)
                            .map(|(name, _)| *name)
                            .unwrap_or("white");
                        args = vec![name.clone(), color_name.to_owned()];
                    }
                    None => args = vec![name.clone()],
                }
            }
            TeamModify::DisplayName => {
                let display = component_text(ctx.string("displayName").unwrap_or_default());
                team.display_name = display;
                args = vec![name.clone()];
            }
            TeamModify::FriendlyFire => {
                let allowed = ctx
                    .argument("allowed")
                    .and_then(ArgumentValue::as_bool)
                    .unwrap_or(true);
                team.friendly_fire = allowed;
                args = vec![name.clone()];
            }
            TeamModify::Prefix => {
                team.prefix = component_text(ctx.string("prefix").unwrap_or_default());
                args = vec![team.prefix.clone()];
            }
            TeamModify::Suffix => {
                team.suffix = component_text(ctx.string("suffix").unwrap_or_default());
                args = vec![team.suffix.clone()];
            }
            TeamModify::SeeFriendlyInvisibles => {
                let allowed = ctx
                    .argument("allowed")
                    .and_then(ArgumentValue::as_bool)
                    .unwrap_or(true);
                team.see_friendly_invisibles = allowed;
                args = vec![name.clone()];
            }
            TeamModify::NametagVisibility(value) => {
                team.nametag_visibility = value.to_owned();
                args = vec![name.clone(), value.to_owned()];
            }
            TeamModify::CollisionRule(value) => {
                team.collision_rule = value.to_owned();
                args = vec![name.clone(), value.to_owned()];
            }
            TeamModify::DeathMessageVisibility(value) => {
                team.death_message_visibility = value.to_owned();
                args = vec![name.clone(), value.to_owned()];
            }
        }
        team.clone()
    };
    let key = match option {
        TeamModify::Color(None) => "commands.team.option.color.clear.success",
        TeamModify::FriendlyFire if !team.friendly_fire => {
            "commands.team.option.friendlyfire.disabled"
        }
        TeamModify::SeeFriendlyInvisibles if !team.see_friendly_invisibles => {
            "commands.team.option.seeFriendlyInvisibles.disabled"
        }
        _ => key,
    };
    packets(
        ctx,
        Vec::new(),
        PlayerPacket::PlayerTeam(team_packet(&name, &team)),
    )?;
    feedback(ctx, key, args)
}

#[cfg(test)]
mod tests {
    use super::super::commands::{Action, GameState};
    use super::*;
    use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

    const ALICE: u128 = 1;

    fn source(permission: u8) -> (Source, UnboundedReceiver<Action>) {
        let mut state = GameState::default();
        state.join(Uuid::from_u128(ALICE), "Alice".into(), 1, 20);
        let (actions, received) = unbounded_channel();
        (
            Source {
                permission,
                origin: [0.0, 64.0, 0.0],
                yaw: 0.0,
                pitch: 0.0,
                uuid: Some(Uuid::from_u128(ALICE)),
                name: Some("Alice".into()),
                state: std::sync::Arc::new(std::sync::Mutex::new(state)),
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

    fn execute(
        tree: &CommandDispatcher<Source>,
        source: Source,
        received: &mut UnboundedReceiver<Action>,
        input: &str,
    ) -> Vec<Action> {
        tree.execute(input, source)
            .unwrap_or_else(|error| panic!("{input}: {error}"));
        std::iter::from_fn(|| received.try_recv().ok()).collect()
    }

    fn feedback_text(actions: &[Action]) -> Vec<&str> {
        actions
            .iter()
            .filter_map(|action| match action {
                Action::Feedback(feedback) => Some(feedback.text.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn scoreboard_objectives_and_players() {
        let tree = super::super::commands::dispatcher();
        let (source, mut received) = source(2);
        let actions = execute(
            &tree,
            source.clone(),
            &mut received,
            "scoreboard objectives add points dummy",
        );
        assert_eq!(
            feedback_text(&actions),
            vec!["Created new objective points"]
        );
        assert!(actions.iter().any(|action| matches!(
            action,
            Action::Packets {
                packet: PlayerPacket::SetObjective(_),
                ..
            }
        )));
        let state = source.state.clone();
        assert!(state.lock().unwrap().objectives.contains_key("points"));

        let actions = execute(
            &tree,
            source.clone(),
            &mut received,
            "scoreboard players set @s points 7",
        );
        assert_eq!(feedback_text(&actions), vec!["Set points for Alice to 7"]);
        assert!(actions.iter().any(|action| matches!(
            action,
            Action::Packets {
                packet: PlayerPacket::SetScore(_),
                ..
            }
        )));

        let actions = execute(
            &tree,
            source.clone(),
            &mut received,
            "scoreboard objectives list",
        );
        assert_eq!(
            feedback_text(&actions),
            vec!["There are 1 objective(s): points"]
        );

        let error = tree
            .execute("scoreboard objectives add points dummy", source.clone())
            .unwrap_err();
        assert!(error.to_string().contains("already exists"));
        let error = tree
            .execute("scoreboard players set @s missing 1", source)
            .unwrap_err();
        assert!(error.to_string().contains("Unknown objective"));
        let (guest, _) = self::source(1);
        assert!(tree.execute("scoreboard objectives list", guest).is_err());
    }

    #[test]
    fn bossbar_creates_updates_and_lists() {
        let tree = super::super::commands::dispatcher();
        let (source, mut received) = source(2);
        let actions = execute(
            &tree,
            source.clone(),
            &mut received,
            "bossbar add minecraft:test {\"text\":\"Boss\"}",
        );
        assert_eq!(
            feedback_text(&actions),
            vec!["Created custom bossbar minecraft:test"]
        );
        assert!(actions.iter().any(|action| matches!(
            action,
            Action::Packets {
                packet: PlayerPacket::BossBar(_),
                ..
            }
        )));
        let actions = execute(
            &tree,
            source.clone(),
            &mut received,
            "bossbar set minecraft:test value 50",
        );
        assert_eq!(
            feedback_text(&actions),
            vec!["Custom bossbar minecraft:test has changed value to 50"]
        );
        let actions = execute(&tree, source.clone(), &mut received, "bossbar list");
        assert_eq!(
            feedback_text(&actions),
            vec!["There are 1 custom bossbar(s) active: minecraft:test"]
        );
        let error = tree
            .execute("bossbar remove minecraft:missing", source)
            .unwrap_err();
        assert!(error.to_string().contains("No bossbar exists"));
    }

    #[test]
    fn tags_add_list_and_remove() {
        let tree = super::super::commands::dispatcher();
        let (source, mut received) = source(2);
        let actions = execute(&tree, source.clone(), &mut received, "tag @s add veteran");
        assert_eq!(
            feedback_text(&actions),
            vec!["Added tag 'veteran' to Alice"]
        );
        let actions = execute(&tree, source.clone(), &mut received, "tag @s list");
        assert_eq!(feedback_text(&actions), vec!["Alice has 1 tag(s): veteran"]);
        let error = tree
            .execute("tag @s add veteran", source.clone())
            .unwrap_err();
        assert!(error.to_string().contains("already has the tag"));
        let actions = execute(&tree, source, &mut received, "tag @s remove veteran");
        assert_eq!(
            feedback_text(&actions),
            vec!["Removed tag 'veteran' from Alice"]
        );
    }

    #[test]
    fn teams_add_join_and_modify() {
        let tree = super::super::commands::dispatcher();
        let (source, mut received) = source(2);
        let actions = execute(&tree, source.clone(), &mut received, "team add red");
        assert_eq!(feedback_text(&actions), vec!["Created team red"]);
        let actions = execute(&tree, source.clone(), &mut received, "team join red @s");
        assert_eq!(feedback_text(&actions), vec!["Added Alice to team red"]);
        let state = source.state.clone();
        assert_eq!(
            state
                .lock()
                .unwrap()
                .players
                .iter()
                .find(|player| player.uuid == Uuid::from_u128(ALICE))
                .unwrap()
                .team
                .as_deref(),
            Some("red")
        );
        let actions = execute(
            &tree,
            source.clone(),
            &mut received,
            "team modify red color dark_red",
        );
        assert_eq!(
            feedback_text(&actions),
            vec!["Updated the color for team red to dark_red"]
        );
        let error = tree.execute("team add red", source.clone()).unwrap_err();
        assert!(error.to_string().contains("already exists"));
        let error = tree.execute("team join missing @s", source).unwrap_err();
        assert!(error.to_string().contains("Unknown team"));
    }
}
