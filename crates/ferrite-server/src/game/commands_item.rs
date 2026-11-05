//! `/item replace entity <players> <slot> with <item> [count]`.
//!
//! Only players' own inventories and the `with` source are supported: `block` targets, `from`,
//! `modify`, `fill` and `override` need container writes, cross-entity slot reads and loot
//! modifiers that the command layer does not have yet, so they fail with a clear message.
//! Item components in the `item[...]` argument are ignored.

use ferrite_command::{
    ArgumentType, CommandContext, CommandDispatcher, CommandError, NodeBuilder, argument, literal,
};

use super::commands::{Action, Feedback, Source, item_display, parse_item, resolved_targets, send};

fn unsupported(what: &str) -> CommandError {
    CommandError::failed(format!("Ferrite: /item {what} is not supported yet"))
}

/// The inventory index of a command slot name for a player (`container.N`, `hotbar.N`,
/// `inventory.N`, `armor.*`, `weapon.offhand`).
pub fn player_slot(name: &str) -> Result<u8, CommandError> {
    let number = |prefix: &str, max: u8| -> Option<u8> {
        name.strip_prefix(prefix)?
            .parse::<u8>()
            .ok()
            .filter(|n| *n < max)
    };
    let slot = match name {
        "armor.feet" => Some(36),
        "armor.legs" => Some(37),
        "armor.chest" => Some(38),
        "armor.head" => Some(39),
        "weapon.offhand" => Some(40),
        "weapon.mainhand" => return Err(unsupported("replace weapon.mainhand")),
        _ => number("container.", 36)
            .or_else(|| number("hotbar.", 9))
            .or_else(|| number("inventory.", 27).map(|n| 9 + n)),
    };
    slot.ok_or_else(|| {
        CommandError::failed(Feedback::new("slot.unknown", vec![name.to_owned()]).text)
    })
}

/// Every inventory index a slot source names (`hotbar.*` expands to all nine, and so on).
pub fn player_slots(name: &str) -> Result<Vec<u8>, CommandError> {
    let range = match name {
        "container.*" => Some(0..36),
        "hotbar.*" => Some(0..9),
        "inventory.*" => Some(9..36),
        "armor.*" => Some(36..40),
        "weapon.*" => return Err(unsupported("replace weapon.*")),
        _ => None,
    };
    match range {
        Some(range) => Ok(range.collect()),
        None => Ok(vec![player_slot(name)?]),
    }
}

fn replace(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let slots = player_slots(ctx.string("slots").unwrap_or_default())?;
    let raw = ctx
        .string("item")
        .ok_or_else(|| CommandError::failed("Missing item"))?;
    let item = parse_item(raw)?;
    let count = ctx.integer("count").unwrap_or(1);
    let max = ferrite_item::item_defaults(item).map_or(64, |defaults| defaults.max_stack_size);
    if !(1..=max).contains(&count) {
        return Err(CommandError::failed(format!(
            "Can't give {count} of {}",
            item_display(item)
        )));
    }
    let targets = resolved_targets(ctx, "targets")?;
    let name = ferrite_item::item_name(item).unwrap_or_default().to_owned();
    let key = if targets.len() == 1 {
        "commands.item.entity.set.success.single"
    } else {
        "commands.item.entity.set.success.multiple"
    };
    let who = if targets.len() == 1 {
        ctx.source()
            .state
            .lock()
            .unwrap()
            .players
            .iter()
            .find(|p| p.uuid == targets[0])
            .map_or_else(|| targets[0].to_string(), |p| p.name.clone())
    } else {
        targets.len().to_string()
    };
    let total = i32::try_from(targets.len()).unwrap_or(i32::MAX);
    for slot in slots {
        send(
            ctx.source(),
            Action::SetSlot {
                targets: targets.clone(),
                slot,
                item: Some((name.clone(), u8::try_from(count).unwrap_or(1))),
            },
        )?;
    }
    send(
        ctx.source(),
        Action::Feedback(Feedback::new(key, vec![who, item_display(item)])),
    )?;
    Ok(total)
}

type Run = fn(&CommandContext<Source>) -> Result<i32, CommandError>;

/// `<sourceSlots> [<modifier>]`: always the "unsupported" branch.
fn source_slots(what: &'static str) -> NodeBuilder<Source> {
    argument::<Source>("sourceSlots", ArgumentType::opaque("minecraft:slot_source"))
        .executes(move |_| Err(unsupported(what)))
        .then(
            argument::<Source>("modifier", ArgumentType::opaque("minecraft:loot_modifier"))
                .executes(move |_| Err(unsupported(what))),
        )
}

/// The `with`/`from` branches under `<slots>`; `with` runs `with_run`.
fn slot_node(what: &'static str, with_run: Option<Run>) -> NodeBuilder<Source> {
    let fail = move |_: &CommandContext<Source>| Err(unsupported(what));
    let run = with_run.unwrap_or(|_| Err(unsupported("this form")));
    let slots = argument::<Source>("slots", ArgumentType::opaque("minecraft:slot_source"));
    if what == "modify" {
        return slots.then(
            argument::<Source>("modifier", ArgumentType::opaque("minecraft:loot_modifier"))
                .executes(fail),
        );
    }
    slots
        .then(
            literal::<Source>("from")
                .then(literal::<Source>("block").then(
                    argument::<Source>("source", ArgumentType::BlockPos).then(source_slots(what)),
                ))
                .then(literal::<Source>("entity").then(
                    argument::<Source>("source", ArgumentType::entity()).then(source_slots(what)),
                )),
        )
        .then(
            literal::<Source>("with").then(
                argument::<Source>("item", ArgumentType::opaque("minecraft:item_stack"))
                    .executes(run)
                    .then(argument::<Source>("count", ArgumentType::integer()).executes(run)),
            ),
        )
}

fn item_tree() -> NodeBuilder<Source> {
    let mut root = literal::<Source>("item").requires(|s| s.permission >= 2);
    for what in ["replace", "modify", "fill", "override"] {
        // Only `replace entity ... with` works; every other branch reports "unsupported".
        let entity_run: Option<Run> = (what == "replace").then_some(replace);
        root = root.then(
            literal::<Source>(what)
                .then(
                    literal::<Source>("block").then(
                        argument::<Source>("target", ArgumentType::BlockPos)
                            .then(slot_node(what, None)),
                    ),
                )
                .then(
                    literal::<Source>("entity").then(
                        argument::<Source>("targets", ArgumentType::entity())
                            .then(slot_node(what, entity_run)),
                    ),
                ),
        );
    }
    root
}

/// `/place`: the syntax is registered so it completes and validates, but placing needs chunk
/// state in the worldgen host (features, jigsaw, structures) or the structure template files,
/// which the extracted data does not include.
fn place_tree() -> NodeBuilder<Source> {
    let fail = |_: &CommandContext<Source>| -> Result<i32, CommandError> {
        Err(CommandError::failed("Ferrite: /place is not supported yet"))
    };
    let pos = || argument::<Source>("pos", ArgumentType::BlockPos).executes(fail);
    let resource = |name: &'static str| {
        argument::<Source>(name, ArgumentType::opaque("minecraft:resource_location"))
    };
    literal::<Source>("place")
        .requires(|s| s.permission >= 2)
        .then(literal::<Source>("feature").then(resource("feature").executes(fail).then(pos())))
        .then(
            literal::<Source>("jigsaw").then(
                resource("pool").then(
                    resource("target").then(
                        argument::<Source>("max_depth", ArgumentType::integer())
                            .executes(fail)
                            .then(
                                argument::<Source>("position", ArgumentType::BlockPos)
                                    .executes(fail),
                            ),
                    ),
                ),
            ),
        )
        .then(literal::<Source>("structure").then(resource("structure").executes(fail).then(pos())))
        .then(
            literal::<Source>("template").then(
                resource("template").executes(fail).then(
                    argument::<Source>("pos", ArgumentType::BlockPos)
                        .executes(fail)
                        .then(
                            argument::<Source>(
                                "rotation",
                                ArgumentType::opaque("minecraft:template_rotation"),
                            )
                            .executes(fail)
                            .then(
                                argument::<Source>(
                                    "mirror",
                                    ArgumentType::opaque("minecraft:template_mirror"),
                                )
                                .executes(fail),
                            ),
                        ),
                ),
            ),
        )
}

pub fn register(tree: &mut CommandDispatcher<Source>) {
    tree.register(item_tree());
    tree.register(place_tree());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_names_map_to_inventory_indexes() {
        assert_eq!(player_slot("hotbar.3").unwrap(), 3);
        assert_eq!(player_slot("container.8").unwrap(), 8);
        assert_eq!(player_slot("inventory.0").unwrap(), 9);
        assert_eq!(player_slot("inventory.26").unwrap(), 35);
        assert_eq!(player_slot("armor.head").unwrap(), 39);
        assert_eq!(player_slot("weapon.offhand").unwrap(), 40);
        assert_eq!(player_slots("hotbar.*").unwrap().len(), 9);
        assert_eq!(player_slots("armor.*").unwrap(), vec![36, 37, 38, 39]);
        assert!(player_slot("hotbar.9").is_err());
        assert!(player_slot("weapon.mainhand").is_err());
        assert!(player_slot("nope").is_err());
    }
}
