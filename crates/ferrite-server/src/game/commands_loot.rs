//! `/loot`: rolls a vanilla loot table and hands the result to players.
//!
//! Sources `loot`, `fish` and `mine` roll the extracted tables (`mine` reads the block at the
//! position; the tool arguments only feed enchantment-free defaults, `mainhand`/`offhand` are
//! treated as no tool). `kill` needs the target's entity type, which the selector does not carry
//! yet, so it fails. Targets other than `give` (`insert`, `replace`, `spawn`) fail with a clear
//! message: there is no item-entity spawn or container write path for commands yet.

use ferrite_command::{
    ArgumentType, ArgumentValue, CommandContext, CommandDispatcher, CommandError, NodeBuilder,
    argument, literal,
};
use ferrite_loot::LootContext;

use super::commands::{Action, Feedback, Source, item_display, parse_item, resolved_targets, send};

fn unsupported(what: &str) -> CommandError {
    CommandError::failed(format!("Ferrite: /loot {what} is not supported yet"))
}

fn block_context(ctx: &CommandContext<Source>) -> Result<LootContext, CommandError> {
    let Some(ArgumentValue::BlockPos(coords)) = ctx.argument("pos") else {
        return Err(CommandError::failed("missing pos"));
    };
    let source = ctx.source();
    let origin = super::commands_world::Origin {
        pos: source.origin,
        yaw: source.yaw,
        pitch: source.pitch,
    };
    let [x, y, z] = super::commands_world::resolve(coords, &origin);
    let mut context = LootContext::default();
    if let Some((name, props)) = source
        .world
        .as_ref()
        .and_then(|world| world.block(x, y, z))
        .and_then(|state| u16::try_from(state).ok())
        .and_then(ferrite_data::blocks::state)
    {
        context.block = Some(name.to_owned());
        context.block_state = props
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
    }
    if let Some(tool) = ctx.string("tool") {
        context.tool = Some(tool.split(['[', '{']).next().unwrap_or(tool).to_owned());
    }
    Ok(context)
}

fn rolled(table: &str, context: &LootContext) -> Result<Vec<(String, i32)>, CommandError> {
    super::drops::roll_table(table, context).ok_or_else(|| {
        CommandError::failed(
            Feedback::new("commands.loot.unknown_table", vec![table.to_owned()]).text,
        )
    })
}

#[derive(Clone, Copy)]
enum From {
    Loot,
    Fish,
    Mine,
    Kill,
}

fn stacks(ctx: &CommandContext<Source>, from: From) -> Result<Vec<(String, i32)>, CommandError> {
    match from {
        From::Loot => {
            let table = ctx.string("loot_table").unwrap_or_default().to_owned();
            rolled(&table, &LootContext::default())
        }
        From::Fish => {
            let table = ctx.string("loot_table").unwrap_or_default().to_owned();
            rolled(&table, &block_context(ctx)?)
        }
        From::Mine => {
            let context = block_context(ctx)?;
            let Some(block) = context.block.clone() else {
                return Err(CommandError::failed(
                    Feedback::new("argument.pos.unloaded", vec![]).text,
                ));
            };
            let table = format!(
                "minecraft:blocks/{}",
                block.trim_start_matches("minecraft:")
            );
            rolled(&table, &context)
        }
        From::Kill => Err(unsupported("kill")),
    }
}

fn give(ctx: &CommandContext<Source>, from: From) -> Result<i32, CommandError> {
    let stacks = stacks(ctx, from)?;
    let targets = resolved_targets(ctx, "players")?;
    let mut total = 0;
    for (name, count) in stacks.iter().filter(|(_, count)| *count > 0) {
        let item = parse_item(name)?;
        total += count;
        send(
            ctx.source(),
            Action::Give {
                targets: targets.clone(),
                item,
                count: *count,
                item_name: item_display(item),
                components: Vec::new(),
            },
        )?;
    }
    if total == 0 {
        send(
            ctx.source(),
            Action::Feedback(Feedback::new("commands.loot.empty", vec![])),
        )?;
    }
    Ok(total)
}

fn failing(what: &'static str) -> impl Fn(&CommandContext<Source>) -> Result<i32, CommandError> {
    move |_| Err(unsupported(what))
}

/// The four sources under one parent; `run` handles the executes of each.
fn sources(
    run: impl Fn(
        From,
    ) -> Box<dyn Fn(&CommandContext<Source>) -> Result<i32, CommandError> + Send + Sync>
    + Copy,
) -> Vec<NodeBuilder<Source>> {
    let table = || argument::<Source>("loot_table", ArgumentType::opaque("minecraft:loot_table"));
    let pos = || argument::<Source>("pos", ArgumentType::BlockPos);
    let mine_tail = |from: From| {
        let f = run(from);
        let main = run(from);
        let off = run(from);
        let item = run(from);
        pos()
            .executes(move |ctx| f(ctx))
            .then(literal::<Source>("mainhand").executes(move |ctx| main(ctx)))
            .then(literal::<Source>("offhand").executes(move |ctx| off(ctx)))
            .then(
                argument::<Source>("tool", ArgumentType::opaque("minecraft:item_stack"))
                    .executes(move |ctx| item(ctx)),
            )
    };
    vec![
        literal::<Source>("fish").then(table().then(mine_tail(From::Fish))),
        literal::<Source>("kill").then(
            argument::<Source>("target", ArgumentType::entity()).executes({
                let f = run(From::Kill);
                move |ctx| f(ctx)
            }),
        ),
        literal::<Source>("loot").then(table().executes({
            let f = run(From::Loot);
            move |ctx| f(ctx)
        })),
        literal::<Source>("mine").then(mine_tail(From::Mine)),
    ]
}

fn loot_tree() -> NodeBuilder<Source> {
    let mut give_node = literal::<Source>("give");
    let mut players = argument::<Source>("players", ArgumentType::players());
    for node in sources(|from| Box::new(move |ctx| give(ctx, from))) {
        players = players.then(node);
    }
    give_node = give_node.then(players);

    let mut insert = argument::<Source>("targetPos", ArgumentType::BlockPos);
    for node in sources(|_| Box::new(failing("insert"))) {
        insert = insert.then(node);
    }
    let mut spawn = argument::<Source>("targetPos", ArgumentType::Vec3);
    for node in sources(|_| Box::new(failing("spawn"))) {
        spawn = spawn.then(node);
    }
    let slot_sources = |count: bool| {
        let mut slot = argument::<Source>("slot", ArgumentType::opaque("minecraft:item_slot"));
        if count {
            let mut counted = argument::<Source>("count", ArgumentType::integer());
            for node in sources(|_| Box::new(failing("replace"))) {
                counted = counted.then(node);
            }
            slot = slot.then(counted);
        }
        for node in sources(|_| Box::new(failing("replace"))) {
            slot = slot.then(node);
        }
        slot
    };
    let replace =
        literal::<Source>("replace")
            .then(literal::<Source>("block").then(
                argument::<Source>("targetPos", ArgumentType::BlockPos).then(slot_sources(true)),
            ))
            .then(literal::<Source>("entity").then(
                argument::<Source>("entities", ArgumentType::entity()).then(slot_sources(true)),
            ));
    literal::<Source>("loot")
        .requires(|s| s.permission >= 2)
        .then(give_node)
        .then(literal::<Source>("insert").then(insert))
        .then(replace)
        .then(literal::<Source>("spawn").then(spawn))
}

pub fn register(tree: &mut CommandDispatcher<Source>) {
    tree.register(loot_tree());
}
