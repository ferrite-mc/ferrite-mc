//! `/item replace|fill|override|modify` on block containers and entities.
//!
//! Writes go through [`Action::Slots`]. Players are the only entities with inventories the
//! command layer can address, so `entity` targets act on players. Components in the
//! `item[...]` argument of `with` are parsed like `/give` does and set on the stack. No item
//! modifiers ship with vanilla data: `modify` and `from ... <modifier>` run a modifier from a data
//! pack (`item_modifier/*.json`) or inline JSON, and an unknown id reports
//! `argument.id.unknown` (see `item_modifier.rs` for the supported functions).

use ferrite_command::{
    ArgumentType, ArgumentValue, CommandContext, CommandDispatcher, CommandError, NodeBuilder,
    argument,
    arguments::{Coordinates, WorldCoordinate},
    literal,
};
use ferrite_item::ItemStack;

use super::commands_world::WorldCommand;
use super::item_modifier::ItemModifier;
use super::structure_template::{Mirror, Rotation, Template};

use super::commands::{
    Action, Feedback, SlotMode, SlotSource, SlotTarget, Source, entity, item_display, parse_item,
    parse_item_components, send,
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
    CommandError::failed(Feedback::new("argument.id.unknown", vec![name.to_owned()]).text)
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
    let stack = stack_for(raw, ctx.integer("count").unwrap_or(1))?;
    send(
        ctx.source(),
        Action::Slots {
            mode,
            target: target(ctx, block)?,
            slots: ctx.string("slots").unwrap_or_default().to_owned(),
            source: SlotSource::Stacks(vec![stack]),
        },
    )
}

/// The stack `with <item>[components] <count>` describes: the count is checked against the item's
/// stack size and each component is parsed exactly as `/give` parses it.
pub(super) fn stack_for(raw: &str, count: i32) -> Result<ItemStack, CommandError> {
    let item = parse_item(raw)?;
    let components = parse_item_components(raw)?;
    let max = ferrite_item::item_defaults(item).map_or(64, |defaults| defaults.max_stack_size);
    if !(1..=max).contains(&count) {
        return Err(CommandError::failed(format!(
            "Can't give {count} of {}",
            item_display(item)
        )));
    }
    let mut stack = ItemStack::with_count(item, count);
    for (id, tag) in &components {
        stack
            .components
            .insert(*id, ferrite_item::Component::from_nbt(*id, tag));
    }
    Ok(stack)
}

/// The modifier a `<modifier>` argument names: inline JSON, or `ns:path` of a data-pack file.
fn resolve_modifier(
    ctx: &CommandContext<Source>,
    raw: &str,
) -> Result<std::sync::Arc<ItemModifier>, CommandError> {
    let raw = raw.trim();
    if raw.starts_with('{') || raw.starts_with('[') {
        return ItemModifier::parse(raw)
            .map(std::sync::Arc::new)
            .map_err(|reason| CommandError::failed(format!("Invalid item modifier: {reason}")));
    }
    let id = if raw.contains(':') {
        raw.to_owned()
    } else {
        format!("minecraft:{raw}")
    };
    let state = ctx.source().state.lock().unwrap();
    match state.functions.item_modifier(&id) {
        Some(Ok(modifier)) => Ok(modifier.clone()),
        Some(Err(reason)) => Err(CommandError::failed(format!(
            "Item modifier {id} cannot be used: {reason}"
        ))),
        None => Err(unknown_modifier(raw)),
    }
}

fn run_from(
    ctx: &CommandContext<Source>,
    mode: SlotMode,
    block: bool,
    from_block: bool,
) -> Result<i32, CommandError> {
    let modifier = ctx
        .string("modifier")
        .map(|raw| resolve_modifier(ctx, raw))
        .transpose()?;
    let slots = ctx.string("sourceSlots").unwrap_or_default().to_owned();
    let source = if from_block {
        SlotSource::FromBlock {
            pos: block_pos(ctx, "source")?,
            slots,
            modifier,
        }
    } else {
        SlotSource::FromEntity {
            target: entity(ctx, "source")?,
            slots,
            modifier,
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

fn modify_node(block: bool) -> NodeBuilder<Source> {
    argument::<Source>("slots", ArgumentType::opaque("minecraft:slot_source")).then(
        argument::<Source>("modifier", ArgumentType::opaque("minecraft:loot_modifier")).executes(
            move |ctx| {
                let modifier = resolve_modifier(ctx, ctx.string("modifier").unwrap_or_default())?;
                send(
                    ctx.source(),
                    Action::ModifySlots {
                        target: target(ctx, block)?,
                        slots: ctx.string("slots").unwrap_or_default().to_owned(),
                        modifier,
                    },
                )
            },
        ),
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
            None => modify_node(block),
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
    let pos = |run: fn(&CommandContext<Source>) -> Result<i32, CommandError>| {
        argument::<Source>("pos", ArgumentType::BlockPos).executes(run)
    };
    let resource = |name: &'static str| {
        argument::<Source>(name, ArgumentType::opaque("minecraft:resource_location"))
    };
    literal::<Source>("place")
        .requires(|s| s.permission >= 2)
        .then(
            literal::<Source>("feature").then(
                resource("feature")
                    .executes(|ctx| host_place(ctx, "feature"))
                    .then(pos(|ctx| host_place(ctx, "feature"))),
            ),
        )
        .then(
            literal::<Source>("jigsaw").then(
                resource("pool").then(
                    resource("target").then(
                        argument::<Source>("max_depth", ArgumentType::integer())
                            .executes(|ctx| host_place(ctx, "jigsaw"))
                            .then(
                                argument::<Source>("position", ArgumentType::BlockPos)
                                    .executes(|ctx| host_place(ctx, "jigsaw")),
                            ),
                    ),
                ),
            ),
        )
        .then(
            literal::<Source>("structure").then(
                resource("structure")
                    .executes(|ctx| host_place(ctx, "structure"))
                    .then(pos(|ctx| host_place(ctx, "structure"))),
            ),
        )
        .then(
            literal::<Source>("template").then(
                resource("template").then(
                    argument::<Source>("pos", ArgumentType::BlockPos)
                        .executes(template)
                        .then(
                            argument::<Source>(
                                "rotation",
                                ArgumentType::opaque("minecraft:template_rotation"),
                            )
                            .executes(template)
                            .then(
                                argument::<Source>(
                                    "mirror",
                                    ArgumentType::opaque("minecraft:template_mirror"),
                                )
                                .executes(template)
                                .then(
                                    argument::<Source>(
                                        "integrity",
                                        ArgumentType::Float { min: 0.0, max: 1.0 },
                                    )
                                    .executes(template)
                                    .then(
                                        argument::<Source>("seed", ArgumentType::integer())
                                            .executes(template)
                                            .then(literal::<Source>("strict").executes(template)),
                                    ),
                                ),
                            ),
                        ),
                ),
            ),
        )
}

/// `/place feature|structure|jigsaw`: the worldgen host runs vanilla's own placement on its
/// world and Ferrite applies the blocks that changed.
fn host_place(ctx: &CommandContext<Source>, kind: &'static str) -> Result<i32, CommandError> {
    let source = ctx.source();
    let position_name = if kind == "jigsaw" { "position" } else { "pos" };
    // Vanilla defaults to the source position when no position is given.
    let coords = match ctx.argument(position_name) {
        Some(ArgumentValue::BlockPos(coords)) => *coords,
        _ => Coordinates::World([
            WorldCoordinate::relative(0.0),
            WorldCoordinate::relative(0.0),
            WorldCoordinate::relative(0.0),
        ]),
    };
    let at = coords.resolve(source.origin, source.yaw, source.pitch);
    let base = [
        at[0].floor() as i32,
        at[1].floor() as i32,
        at[2].floor() as i32,
    ];
    let name = |key: &str| ctx.string(key).unwrap_or_default().to_owned();
    let position = format!("{};{};{}", base[0], base[1], base[2]);
    let (argument, label) = match kind {
        "feature" => (
            format!("feature;{};{position}", name("feature")),
            format!("feature {}", name("feature")),
        ),
        "structure" => (
            format!("structure;{};{position}", name("structure")),
            format!("structure {}", name("structure")),
        ),
        _ => (
            format!(
                "jigsaw;{};{};{};{position}",
                name("pool"),
                name("target"),
                ctx.integer("max_depth").unwrap_or(1)
            ),
            format!("jigsaw {}", name("pool")),
        ),
    };
    let Some(world) = source.world.as_ref() else {
        return Err(CommandError::failed(
            "Ferrite: /place needs a world with generator = vanilla",
        ));
    };
    let Some(result) = world.place(&argument) else {
        return Err(CommandError::failed(
            "Ferrite: /place needs generator = vanilla (the worldgen host)",
        ));
    };
    let placed = result.map_err(|error| {
        let key = match kind {
            "feature" => "commands.place.feature.failed",
            "jigsaw" => "commands.place.jigsaw.failed",
            _ => "commands.place.structure.failed",
        };
        let text = error.to_string();
        if text.contains("nothing changed") || text.contains("Failed") {
            CommandError::failed(Feedback::new(key, vec![]).text)
        } else {
            CommandError::failed(text)
        }
    })?;
    let blocks: Vec<([i32; 3], u16)> = placed
        .blocks
        .into_iter()
        .map(|([x, y, z], state)| ([x - base[0], y - base[1], z - base[2]], state))
        .collect();
    let entities: Vec<([i32; 3], ferrite_nbt::Compound)> = placed
        .entities
        .into_iter()
        .map(|([x, y, z], data)| ([x - base[0], y - base[1], z - base[2]], data))
        .collect();
    send(
        source,
        Action::World(WorldCommand::Template {
            pos: Coordinates::World([
                WorldCoordinate::absolute(f64::from(base[0])),
                WorldCoordinate::absolute(f64::from(base[1])),
                WorldCoordinate::absolute(f64::from(base[2])),
            ]),
            name: label,
            blocks: std::sync::Arc::new(blocks),
            entities: std::sync::Arc::new(entities),
            spawns: std::sync::Arc::default(),
        }),
    )
}

/// `/place template <id> <pos> [rotation] [mirror] [integrity] [seed] [strict]`.
///
/// `strict` sets `StructurePlaceSettings.knownShape`, which in `StructureTemplate.placeInWorld`
/// only gates the neighbour shape update at the template's edge (`updateShapeAtEdge`). Template
/// blocks are written exactly as stored here and no shape update ever runs, so a strict
/// placement matches vanilla and a non-strict one lacks the edge fix-ups (fence and wall
/// connections to neighbouring blocks, for example).
fn template(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let id = ctx.string("template").unwrap_or_default().to_owned();
    let Some(template) = Template::load(&id) else {
        return Err(CommandError::failed(
            Feedback::new("commands.place.template.invalid", vec![id]).text,
        ));
    };
    let rotation = match ctx.string("rotation") {
        Some(name) => Rotation::parse(name)
            .ok_or_else(|| CommandError::failed(format!("Unknown rotation '{name}'")))?,
        None => Rotation::None,
    };
    let mirror = match ctx.string("mirror") {
        Some(name) => Mirror::parse(name)
            .ok_or_else(|| CommandError::failed(format!("Unknown mirror '{name}'")))?,
        None => Mirror::None,
    };
    let integrity = match ctx.argument("integrity") {
        Some(ArgumentValue::Float(value)) => *value,
        _ => 1.0,
    };
    let seed = ctx.integer("seed").map_or_else(
        || {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(1, |d| d.as_nanos() as u64)
        },
        |seed| seed as u64,
    );
    let (blocks, entities) = template.place_with_entities(rotation, mirror, integrity, seed);
    let Some(ArgumentValue::BlockPos(coords)) = ctx.argument("pos") else {
        return Err(CommandError::failed("Missing position"));
    };
    send(
        ctx.source(),
        Action::World(WorldCommand::Template {
            pos: *coords,
            name: id,
            blocks: std::sync::Arc::new(blocks),
            entities: std::sync::Arc::new(entities),
            spawns: std::sync::Arc::new(template.place_entities(rotation, mirror)),
        }),
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
    fn with_applies_the_components_in_the_item_argument() {
        let stack = stack_for("minecraft:diamond_pickaxe[minecraft:damage=5]", 1).unwrap();
        assert_eq!(stack.damage(), 5);
        assert_eq!(stack.count, 1);
        assert_eq!(stack_for("minecraft:stone", 3).unwrap().count, 3);
    }

    #[test]
    fn with_rejects_unknown_components_bad_syntax_and_bad_counts() {
        assert!(stack_for("minecraft:stone[minecraft:not_a_component=1]", 1).is_err());
        assert!(stack_for("minecraft:stone[minecraft:damage=1", 1).is_err());
        assert!(stack_for("minecraft:not_an_item", 1).is_err());
        assert!(stack_for("minecraft:stone", 0).is_err());
        assert!(stack_for("minecraft:stone", 65).is_err());
        assert!(
            stack_for("minecraft:diamond_pickaxe", 2).is_err(),
            "unstackable"
        );
    }

    #[test]
    fn unknown_modifier_text() {
        assert!(unknown_modifier("x").to_string().contains("x"));
    }
}
