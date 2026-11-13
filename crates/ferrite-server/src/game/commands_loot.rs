//! `/loot`: rolls a vanilla loot table and hands the result to players.
//!
//! Sources `loot`, `fish` and `mine` roll the extracted tables (`mine` reads the block at the
//! position). `mainhand`/`offhand` use the player's scanned held stack, including its
//! enchantments; an explicit item argument is used as written. `kill` needs the target's entity
//! type. Targets other than `give` (`insert`, `replace`, `spawn`) write through the shared slot
//! and item-entity paths.

use std::collections::BTreeMap;
use std::sync::Mutex;

use ferrite_command::{
    ArgumentType, ArgumentValue, CommandContext, CommandDispatcher, CommandError, NodeBuilder,
    argument, literal,
};
use ferrite_item::{Component, component_ids};
use ferrite_loot::LootContext;
use uuid::Uuid;

use super::commands::{
    Action, Feedback, SlotMode, SlotSource, SlotTarget, Source, item_display, parse_item,
    resolved_targets, send,
};

/// A held item's loot-relevant data, refreshed by the region inventory scan.
#[derive(Debug, Clone, Default)]
pub struct HeldTool {
    pub item: String,
    pub tool_enchantments: BTreeMap<String, i32>,
    pub fortune: i32,
    pub looting: i32,
}

impl HeldTool {
    pub fn from_stack(stack: &ferrite_item::ItemStack) -> Option<Self> {
        if stack.is_empty() {
            return None;
        }
        let item = ferrite_item::item_name(stack.item)?.to_owned();
        let mut tool_enchantments = BTreeMap::new();
        let (mut fortune, mut looting) = (0, 0);
        if let Some(Component::Enchantments(enchantments)) =
            stack.components.get(component_ids::ENCHANTMENTS)
        {
            for (name, level) in &enchantments.levels {
                tool_enchantments.insert(name.clone(), *level);
            }
            fortune = enchantments.level("minecraft:fortune").unwrap_or(0);
            looting = enchantments.level("minecraft:looting").unwrap_or(0);
        }
        Some(Self {
            item,
            tool_enchantments,
            fortune,
            looting,
        })
    }

    fn apply(&self, context: &mut LootContext) {
        context.tool = Some(self.item.clone());
        context.tool_enchantments = self.tool_enchantments.clone();
        context.fortune = self.fortune;
        context.looting = self.looting;
    }
}

/// Mainhand/offhand tools per player, published by [`Command::ScanInventory`].
pub static HELD_TOOLS: Mutex<BTreeMap<Uuid, [Option<HeldTool>; 2]>> = Mutex::new(BTreeMap::new());

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    None,
    Mainhand,
    Offhand,
    Item,
}

fn block_context(ctx: &CommandContext<Source>, tool: Tool) -> Result<LootContext, CommandError> {
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
    match tool {
        Tool::Item => {
            if let Some(tool) = ctx.string("tool") {
                context.tool = Some(tool.split(['[', '{']).next().unwrap_or(tool).to_owned());
            }
        }
        Tool::Mainhand | Tool::Offhand => {
            if let Some(uuid) = source.uuid
                && let Some(held) = HELD_TOOLS
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&uuid)
                    .and_then(|hands| hands[usize::from(tool == Tool::Offhand)].clone())
            {
                held.apply(&mut context);
            }
        }
        Tool::None => {}
    }
    Ok(context)
}

fn rolled(table: &str, context: &LootContext) -> Result<Vec<(String, i32)>, CommandError> {
    super::drops::roll_table(table, context).ok_or_else(|| {
        CommandError::failed(Feedback::new("argument.id.unknown", vec![table.to_owned()]).text)
    })
}

#[derive(Clone, Copy)]
enum From {
    Loot,
    Fish(Tool),
    Mine(Tool),
    Kill,
}

impl From {
    fn with_tool(self, tool: Tool) -> Self {
        match self {
            Self::Fish(_) => Self::Fish(tool),
            Self::Mine(_) => Self::Mine(tool),
            other => other,
        }
    }
}

fn stacks(ctx: &CommandContext<Source>, from: From) -> Result<Vec<(String, i32)>, CommandError> {
    match from {
        From::Loot => {
            let table = ctx.string("loot_table").unwrap_or_default().to_owned();
            rolled(&table, &LootContext::default())
        }
        From::Fish(tool) => {
            let table = ctx.string("loot_table").unwrap_or_default().to_owned();
            rolled(&table, &block_context(ctx, tool)?)
        }
        From::Mine(tool) => {
            let context = block_context(ctx, tool)?;
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
        From::Kill => {
            let target = super::commands::entity(ctx, "target")?;
            let source = ctx.source();
            let state = source.state.lock().unwrap();
            let selected =
                super::selectors::resolve_selection(&state, &target, source.uuid, source.origin)?;
            let entities = state.entities.lock().unwrap();
            let Some(entry) = selected.first().and_then(|e| entities.get(e.uuid)) else {
                return Err(CommandError::failed(
                    Feedback::new("argument.entity.notfound.entity", vec![]).text,
                ));
            };
            let table = format!(
                "minecraft:entities/{}",
                entry.type_name().trim_start_matches("minecraft:")
            );
            drop(entities);
            drop(state);
            rolled(&table, &LootContext::default())
        }
    }
}

fn item_stacks(
    ctx: &CommandContext<Source>,
    from: From,
) -> Result<Vec<ferrite_item::ItemStack>, CommandError> {
    let mut out = Vec::new();
    for (name, count) in stacks(ctx, from)?.into_iter().filter(|(_, c)| *c > 0) {
        let item = parse_item(&name)?;
        let max = ferrite_item::item_defaults(item)
            .map_or(64, |d| d.max_stack_size)
            .max(1);
        let mut left = count;
        while left > 0 {
            let take = left.min(max);
            out.push(ferrite_item::ItemStack::with_count(item, take));
            left -= take;
        }
    }
    Ok(out)
}

fn give(ctx: &CommandContext<Source>, from: From) -> Result<i32, CommandError> {
    let stacks = item_stacks(ctx, from)?;
    let targets = resolved_targets(ctx, "players")?;
    let mut total = 0;
    for stack in &stacks {
        total += stack.count;
        send(
            ctx.source(),
            Action::Give {
                targets: targets.clone(),
                item: stack.item,
                count: stack.count,
                item_name: item_display(stack.item),
                components: Vec::new(),
            },
        )?;
    }
    if total == 0 {
        send(
            ctx.source(),
            Action::Feedback(Feedback::new(
                "commands.drop.success.multiple",
                vec!["0".to_owned()],
            )),
        )?;
    }
    Ok(total)
}

fn insert_loot(ctx: &CommandContext<Source>, from: From) -> Result<i32, CommandError> {
    let stacks = item_stacks(ctx, from)?;
    send(
        ctx.source(),
        Action::Slots {
            mode: SlotMode::Insert,
            target: SlotTarget::Block(super::commands_item::block_pos(ctx, "targetPos")?),
            slots: String::new(),
            source: SlotSource::Stacks(stacks),
        },
    )
}

fn spawn_loot(ctx: &CommandContext<Source>, from: From) -> Result<i32, CommandError> {
    let stacks = item_stacks(ctx, from)?;
    let Some(ArgumentValue::Vec3(coords)) = ctx.argument("targetPos") else {
        return Err(CommandError::failed("Missing position"));
    };
    let source = ctx.source();
    let pos = coords.resolve(source.origin, source.yaw, source.pitch);
    send(source, Action::DropItems { pos, stacks })
}

/// `replace`: `<slot> [<count>]` names `count` consecutive slots starting at `slot`.
fn replace(ctx: &CommandContext<Source>, from: From, block: bool) -> Result<i32, CommandError> {
    let stacks = item_stacks(ctx, from)?;
    let slot = ctx.string("slot").unwrap_or_default();
    let count = ctx.integer("count").unwrap_or(1).max(1);
    let slots = match slot
        .rsplit_once('.')
        .and_then(|(p, n)| Some((p, n.parse::<i32>().ok()?)))
    {
        Some((prefix, first)) if count > 1 => (0..count)
            .map(|i| format!("{prefix}.{}", first + i))
            .collect::<Vec<_>>()
            .join(","),
        _ => slot.to_owned(),
    };
    let target = if block {
        SlotTarget::Block(super::commands_item::block_pos(ctx, "targetPos")?)
    } else {
        SlotTarget::Entities(super::commands::entity(ctx, "entities")?)
    };
    send(
        ctx.source(),
        Action::Slots {
            mode: SlotMode::Replace,
            target,
            slots,
            source: SlotSource::Stacks(stacks),
        },
    )
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
        let main = run(from.with_tool(Tool::Mainhand));
        let off = run(from.with_tool(Tool::Offhand));
        let item = run(from.with_tool(Tool::Item));
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
        literal::<Source>("fish").then(table().then(mine_tail(From::Fish(Tool::None)))),
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
        literal::<Source>("mine").then(mine_tail(From::Mine(Tool::None))),
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
    for node in sources(|from| Box::new(move |ctx| insert_loot(ctx, from))) {
        insert = insert.then(node);
    }
    let mut spawn = argument::<Source>("targetPos", ArgumentType::Vec3);
    for node in sources(|from| Box::new(move |ctx| spawn_loot(ctx, from))) {
        spawn = spawn.then(node);
    }
    let slot_sources = |block: bool| {
        let mut slot = argument::<Source>("slot", ArgumentType::opaque("minecraft:item_slot"));
        let mut counted = argument::<Source>("count", ArgumentType::integer());
        for node in sources(|from| Box::new(move |ctx| replace(ctx, from, block))) {
            counted = counted.then(node);
        }
        slot = slot.then(counted);
        for node in sources(|from| Box::new(move |ctx| replace(ctx, from, block))) {
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
                argument::<Source>("entities", ArgumentType::entity()).then(slot_sources(false)),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_tools_carry_the_item_and_its_loot_context() {
        assert!(HeldTool::from_stack(&ferrite_item::ItemStack::empty()).is_none());
        let stone =
            ferrite_item::ItemStack::new(ferrite_item::item_id("minecraft:stone").expect("stone"));
        let held = HeldTool::from_stack(&stone).expect("held stone");
        assert_eq!(held.item, "minecraft:stone");
        let mut context = LootContext::default();
        held.apply(&mut context);
        assert_eq!(context.tool.as_deref(), Some("minecraft:stone"));
        assert_eq!(context.fortune, 0);
    }
}
