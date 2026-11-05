//! `/item replace|fill|override|modify` on block containers and entities.
//!
//! Writes go through [`Action::Slots`]. Players are the only entities with inventories the
//! command layer can address, so `entity` targets act on players. Item components in the
//! `item[...]` argument are ignored, and no item modifiers ship with vanilla data, so
//! `modify` and every `<modifier>` report `argument.item_modifier.unknown`.

use ferrite_command::{
    ArgumentType, ArgumentValue, CommandContext, CommandDispatcher, CommandError, NodeBuilder,
    argument, literal,
};
use ferrite_item::ItemStack;

use super::commands::{
    Action, Feedback, SlotMode, SlotSource, SlotTarget, Source, entity, item_display, parse_item,
    send,
};

/// The block position of a `<pos>` argument (relative coordinates resolved).
pub(super) fn block_pos(
    ctx: &CommandContext<Source>,
    name: &str,
) -> Result<[i32; 3], CommandError> {
    match ctx.argument(name) {
        Some(ArgumentValue::BlockPos(coords)) | Some(ArgumentValue::Vec3(coords)) => {
            let source = ctx.source();
            let pos = coords.resolve(source.origin, source.yaw, source.pitch);
            Ok([
                pos[0].floor() as i32,
                pos[1].floor() as i32,
                pos[2].floor() as i32,
            ])
        }
        _ => Err(CommandError::failed(format!("Missing position '{name}'"))),
    }
}

fn unknown_modifier(name: &str) -> CommandError {
    CommandError::failed(
        Feedback::new("argument.item_modifier.unknown", vec![name.to_owned()]).text,
    )
}

fn target(ctx: &CommandContext<Source>, block: bool) -> Result<SlotTarget, CommandError> {
    if block {
        Ok(SlotTarget::Block(block_pos(ctx, "target")?))
    } else {
        Ok(SlotTarget::Entities(entity(ctx, "targets")?))
    }
}

fn run_with(
    ctx: &CommandContext<Source>,
    mode: SlotMode,
    block: bool,
) -> Result<i32, CommandError> {
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
    send(
        ctx.source(),
        Action::Slots {
            mode,
            target: target(ctx, block)?,
            slots: ctx.string("slots").unwrap_or_default().to_owned(),
            source: SlotSource::Stacks(vec![ItemStack::with_count(item, count)]),
        },
    )
}

fn run_from(
    ctx: &CommandContext<Source>,
    mode: SlotMode,
    block: bool,
    from_block: bool,
) -> Result<i32, CommandError> {
    if let Some(modifier) = ctx.string("modifier") {
        return Err(unknown_modifier(modifier));
    }
    let slots = ctx.string("sourceSlots").unwrap_or_default().to_owned();
    let source = if from_block {
        SlotSource::FromBlock {
            pos: block_pos(ctx, "source")?,
            slots,
        }
    } else {
        SlotSource::FromEntity {
            target: entity(ctx, "source")?,
            slots,
        }
    };
    send(
        ctx.source(),
        Action::Slots {
            mode,
            target: target(ctx, block)?,
            slots: ctx.string("slots").unwrap_or_default().to_owned(),
            source,
        },
    )
}

fn from_node(from_block: bool, mode: SlotMode, block: bool) -> NodeBuilder<Source> {
    let source = if from_block {
        argument::<Source>("source", ArgumentType::BlockPos)
    } else {
        argument::<Source>("source", ArgumentType::entity())
    };
    let tail = argument::<Source>("sourceSlots", ArgumentType::opaque("minecraft:slot_source"))
        .executes(move |ctx| run_from(ctx, mode, block, from_block))
        .then(
            argument::<Source>("modifier", ArgumentType::opaque("minecraft:loot_modifier"))
                .executes(move |ctx| run_from(ctx, mode, block, from_block)),
        );
    literal::<Source>(if from_block { "block" } else { "entity" }).then(source.then(tail))
}

fn slot_node(mode: SlotMode, block: bool) -> NodeBuilder<Source> {
    let with = move |ctx: &CommandContext<Source>| run_with(ctx, mode, block);
    argument::<Source>("slots", ArgumentType::opaque("minecraft:slot_source"))
        .then(
            literal::<Source>("from")
                .then(from_node(true, mode, block))
                .then(from_node(false, mode, block)),
        )
        .then(
            literal::<Source>("with").then(
                argument::<Source>("item", ArgumentType::opaque("minecraft:item_stack"))
                    .executes(with)
                    .then(argument::<Source>("count", ArgumentType::integer()).executes(with)),
            ),
        )
}

fn modify_node() -> NodeBuilder<Source> {
    argument::<Source>("slots", ArgumentType::opaque("minecraft:slot_source")).then(
        argument::<Source>("modifier", ArgumentType::opaque("minecraft:loot_modifier"))
            .executes(|ctx| Err(unknown_modifier(ctx.string("modifier").unwrap_or_default()))),
    )
}

fn item_tree() -> NodeBuilder<Source> {
    let mut root = literal::<Source>("item").requires(|s| s.permission >= 2);
    for (name, mode) in [
        ("replace", Some(SlotMode::Replace)),
        ("modify", None),
        ("fill", Some(SlotMode::Fill)),
        ("override", Some(SlotMode::Override)),
    ] {
        let node = |block: bool| match mode {
            Some(mode) => slot_node(mode, block),
            None => modify_node(),
        };
        root =
            root.then(
                literal::<Source>(name)
                    .then(literal::<Source>("block").then(
                        argument::<Source>("target", ArgumentType::BlockPos).then(node(true)),
                    ))
                    .then(literal::<Source>("entity").then(
                        argument::<Source>("targets", ArgumentType::entity()).then(node(false)),
                    )),
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
    fn unknown_modifier_text() {
        assert!(unknown_modifier("x").to_string().contains("x"));
    }
}
