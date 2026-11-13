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
    Action, Feedback, SlotMode, SlotSource, SlotTarget, Source, item_display, resolved_targets,
    send,
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

/// The tool a `mine`/`fish` command rolls with (vanilla passes `ItemStack.EMPTY` without one).
///
/// `mainhand`/`offhand` read the source entity's held stack and fail with
/// `commands.drop.no_held_items` when the source is not an entity that can hold items (the
/// console, a command block). An item argument is parsed with its component patch, so
/// `diamond_pickaxe[enchantments={...}]` carries its silk touch or fortune into the context.
fn tool_context(
    ctx: &CommandContext<Source>,
    tool: Tool,
    context: &mut LootContext,
) -> Result<(), CommandError> {
    let source = ctx.source();
    let held = match tool {
        Tool::None => None,
        Tool::Item => {
            let raw = ctx.string("tool").unwrap_or_default();
            let stack = super::commands_item::stack_for(raw, 1)?;
            HeldTool::from_stack(&stack)
        }
        Tool::Mainhand | Tool::Offhand => {
            let Some(uuid) = source.uuid else {
                return Err(CommandError::failed(
                    Feedback::new(
                        "commands.drop.no_held_items",
                        vec![source.name.clone().unwrap_or_default()],
                    )
                    .text,
                ));
            };
            HELD_TOOLS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&uuid)
                .and_then(|hands| hands[usize::from(tool == Tool::Offhand)].clone())
        }
    };
    if let Some(held) = held {
        held.apply(context);
    }
    Ok(())
}

fn block_pos_of(ctx: &CommandContext<Source>) -> Result<[i32; 3], CommandError> {
    let Some(ArgumentValue::BlockPos(coords)) = ctx.argument("pos") else {
        return Err(CommandError::failed("missing pos"));
    };
    let source = ctx.source();
    let origin = super::commands_world::Origin {
        pos: source.origin,
        yaw: source.yaw,
        pitch: source.pitch,
    };
    Ok(super::commands_world::resolve(coords, &origin))
}

/// Vanilla `LootCommand.dropBlockLoot`: the block state at the position and its block entity,
/// plus the tool.
fn block_context(ctx: &CommandContext<Source>, tool: Tool) -> Result<LootContext, CommandError> {
    let [x, y, z] = block_pos_of(ctx)?;
    let source = ctx.source();
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
        // A shulker box keeps its contents, name and lock on the dropped item, like breaking it.
        if name.ends_with("shulker_box")
            && let Some(entity) = source
                .world
                .as_ref()
                .and_then(|world| world.block_entity(x, y, z))
        {
            context.block_entity_components = super::shulker::collect_components(&entity.data).0;
        }
    }
    tool_context(ctx, tool, &mut context)?;
    Ok(context)
}

/// Vanilla `LootCommand.dropFishingLoot`: origin and tool only, no block.
fn fishing_context(ctx: &CommandContext<Source>, tool: Tool) -> Result<LootContext, CommandError> {
    let mut context = LootContext::default();
    tool_context(ctx, tool, &mut context)?;
    Ok(context)
}

/// Vanilla `LootCommand.dropKillLoot`: the command source is the attacker. A player source sets
/// `LAST_DAMAGE_PLAYER` (so `killed_by_player` conditions pass) and its main-hand looting level
/// feeds `enchanted_count_increase`/`apply_bonus`.
fn kill_context(source: &Source) -> LootContext {
    let mut context = LootContext::default();
    let Some(uuid) = source.uuid else {
        return context;
    };
    let is_player = source
        .state
        .lock()
        .map(|state| state.players.iter().any(|player| player.uuid == uuid))
        .unwrap_or(false);
    if !is_player {
        return context;
    }
    context.killed_by_player = true;
    if let Some(held) = HELD_TOOLS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&uuid)
        .and_then(|hands| hands[0].clone())
    {
        context.looting = held.looting;
    }
    context
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

/// A rolled table: the stacks (split at the item's stack size) and the table they came from.
struct Rolled {
    stacks: Vec<ferrite_item::ItemStack>,
    /// The table named in the success message (vanilla `RESPONSE_WITH_LOOT_TABLE`); `loot` and
    /// `fish` use the plain message.
    table: Option<String>,
}

fn unknown_table(table: &str) -> CommandError {
    CommandError::failed(Feedback::new("argument.id.unknown", vec![table.to_owned()]).text)
}

/// Rolls `table`, keeping the components loot functions attached and splitting oversized stacks.
fn roll_stacks(table: &str, context: &LootContext) -> Option<Vec<ferrite_item::ItemStack>> {
    Some(split_stacks(super::drops::roll_table_stacks(
        table, context,
    )?))
}

/// Rolled entries as item stacks of at most the item's stack size (vanilla's loot stack
/// splitter), carrying the components loot functions set. Empty and unknown entries are dropped.
fn split_stacks(rolled: Vec<ferrite_loot::LootStack>) -> Vec<ferrite_item::ItemStack> {
    let mut out = Vec::new();
    for entry in rolled.into_iter().filter(|entry| entry.count > 0) {
        let Some(item) = ferrite_item::item_id(&entry.item) else {
            continue;
        };
        let max = ferrite_item::item_defaults(item)
            .map_or(64, |d| d.max_stack_size)
            .max(1);
        let mut left = entry.count;
        while left > 0 {
            let take = left.min(max);
            let mut stack = ferrite_item::ItemStack::with_count(item, take);
            for (name, tag) in &entry.components {
                if let Some(id) = ferrite_item::component_id(name) {
                    stack.components.insert(id, Component::from_nbt(id, tag));
                }
            }
            out.push(stack);
            left -= take;
        }
    }
    out
}

/// What an advancement's `rewards.loot` table gives a player (vanilla `AdvancementRewards.grant`):
/// the stacks the table rolls under an empty context (the reward context only has the player and
/// their position), split at each item's stack size and carrying the components loot functions
/// set. An unknown table (or missing vanilla data) is an error naming the table.
///
/// The caller hands the stacks to the player; `/loot give` does the same through
/// `Action::LootGive`.
pub fn roll_reward_table(table: &str) -> Result<Vec<ferrite_item::ItemStack>, String> {
    roll_stacks(table, &LootContext::default())
        .ok_or_else(|| Feedback::new("argument.id.unknown", vec![table.to_owned()]).text)
}

fn rolled(ctx: &CommandContext<Source>, from: From) -> Result<Rolled, CommandError> {
    match from {
        From::Loot => {
            let table = ctx.string("loot_table").unwrap_or_default().to_owned();
            let stacks = roll_stacks(&table, &LootContext::default())
                .ok_or_else(|| unknown_table(&table))?;
            Ok(Rolled {
                stacks,
                table: None,
            })
        }
        From::Fish(tool) => {
            let table = ctx.string("loot_table").unwrap_or_default().to_owned();
            let context = fishing_context(ctx, tool)?;
            let stacks = roll_stacks(&table, &context).ok_or_else(|| unknown_table(&table))?;
            Ok(Rolled {
                stacks,
                table: None,
            })
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
            let Some(stacks) = roll_stacks(&table, &context) else {
                return Err(CommandError::failed(
                    Feedback::new(
                        "commands.drop.no_loot_table.block",
                        vec![block_label(&block)],
                    )
                    .text,
                ));
            };
            Ok(Rolled {
                stacks,
                table: Some(table),
            })
        }
        From::Kill => {
            let target = super::commands::entity(ctx, "target")?;
            let source = ctx.source();
            let selected = {
                let state = source.state.lock().unwrap();
                super::selectors::resolve_selection(&state, &target, source.uuid, source.origin)?
            };
            let Some(first) = selected.first() else {
                return Err(CommandError::failed(
                    Feedback::new("argument.entity.notfound.entity", vec![]).text,
                ));
            };
            // Players and types without an `entities/<type>` table have no loot table.
            let no_table = || {
                CommandError::failed(
                    Feedback::new(
                        "commands.drop.no_loot_table.entity",
                        vec![first.name.clone()],
                    )
                    .text,
                )
            };
            let Some(kind) = first.kind else {
                return Err(no_table());
            };
            let table = format!(
                "minecraft:entities/{}",
                ferrite_data::entity_props::get(kind)
                    .key
                    .trim_start_matches("minecraft:")
            );
            let stacks = roll_stacks(&table, &kill_context(source)).ok_or_else(no_table)?;
            Ok(Rolled {
                stacks,
                table: Some(table),
            })
        }
    }
}

/// `Block %s` argument of the no-loot-table message: the block's id.
fn block_label(block: &str) -> String {
    block.to_owned()
}

/// Vanilla `RESPONSE_WITH(OUT)_LOOT_TABLE`: one stack reads "Dropped 3 Stone", anything else
/// "Dropped N items"; a table adds "from loot table <id>". N is the sum of the stack counts.
pub fn success_feedback(
    stacks: &[ferrite_item::ItemStack],
    table: Option<&str>,
) -> (&'static str, Vec<String>) {
    let total: i32 = stacks.iter().map(|stack| stack.count).sum();
    let mut args = Vec::new();
    let key = match (stacks, table) {
        ([one], with_table) => {
            args.push(one.count.to_string());
            args.push(item_display(one.item));
            if with_table.is_some() {
                "commands.drop.success.single_with_table"
            } else {
                "commands.drop.success.single"
            }
        }
        (_, with_table) => {
            args.push(total.to_string());
            if with_table.is_some() {
                "commands.drop.success.multiple_with_table"
            } else {
                "commands.drop.success.multiple"
            }
        }
    };
    if let Some(table) = table {
        args.push(table.to_owned());
    }
    (key, args)
}

fn give(ctx: &CommandContext<Source>, from: From) -> Result<i32, CommandError> {
    let Rolled { stacks, table } = rolled(ctx, from)?;
    let targets = resolved_targets(ctx, "players")?;
    let total: i32 = stacks.iter().map(|stack| stack.count).sum();
    send(
        ctx.source(),
        Action::LootGive {
            targets,
            stacks,
            table,
        },
    )?;
    Ok(total)
}

fn insert_loot(ctx: &CommandContext<Source>, from: From) -> Result<i32, CommandError> {
    let stacks = rolled(ctx, from)?.stacks;
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
    let Rolled { stacks, table } = rolled(ctx, from)?;
    let Some(ArgumentValue::Vec3(coords)) = ctx.argument("targetPos") else {
        return Err(CommandError::failed("Missing position"));
    };
    let source = ctx.source();
    let pos = coords.resolve(source.origin, source.yaw, source.pitch);
    let total: i32 = stacks.iter().map(|stack| stack.count).sum();
    send(source, Action::DropItems { pos, stacks, table })?;
    Ok(total)
}

/// `replace`: `<slot> [<count>]` names `count` consecutive slots starting at `slot`.
fn replace(ctx: &CommandContext<Source>, from: From, block: bool) -> Result<i32, CommandError> {
    let stacks = rolled(ctx, from)?.stacks;
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
    use crate::game::commands::{GameState, dispatcher};
    use std::sync::Arc;

    fn data() -> bool {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../vanilla/26.3/generated/data/minecraft/loot_table")
            .is_dir()
    }

    fn stack(name: &str, count: i32) -> ferrite_item::ItemStack {
        ferrite_item::ItemStack::with_count(ferrite_item::item_id(name).expect(name), count)
    }

    fn source(uuid: Option<Uuid>, state: Arc<Mutex<GameState>>) -> Source {
        let (actions, _) = tokio::sync::mpsc::unbounded_channel();
        Source {
            permission: 2,
            origin: [0.0, 64.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            uuid,
            name: uuid.map(|_| "Alice".to_owned()),
            anchor: ferrite_command::EntityAnchor::Feet,
            dimension: crate::game::Dimension::Overworld,
            world: None,
            admin: None,
            state,
            actions,
            tree: None,
            function_depth: 0,
        }
    }

    fn run(
        input: &str,
        uuid: Option<Uuid>,
        state: Arc<Mutex<GameState>>,
    ) -> (Result<i32, CommandError>, Vec<Action>) {
        let tree = Arc::new(dispatcher());
        let (actions, mut received) = tokio::sync::mpsc::unbounded_channel();
        let mut source = source(uuid, state);
        source.actions = actions;
        source.tree = Some(tree.clone());
        let result = tree.execute(input, source);
        (
            result,
            std::iter::from_fn(|| received.try_recv().ok()).collect(),
        )
    }

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

    #[test]
    fn success_messages_follow_vanilla_single_multiple_and_table_forms() {
        let (key, args) = success_feedback(&[stack("minecraft:stone", 3)], None);
        assert_eq!(key, "commands.drop.success.single");
        assert_eq!(args, ["3", "Stone"]);
        let (key, args) = success_feedback(
            &[stack("minecraft:stone", 3)],
            Some("minecraft:blocks/stone"),
        );
        assert_eq!(key, "commands.drop.success.single_with_table");
        assert_eq!(args, ["3", "Stone", "minecraft:blocks/stone"]);
        let two = [stack("minecraft:stone", 64), stack("minecraft:stone", 6)];
        let (key, args) = success_feedback(&two, None);
        assert_eq!(key, "commands.drop.success.multiple");
        assert_eq!(args, ["70"], "the total is the sum of the counts");
        let (key, args) = success_feedback(&[], Some("minecraft:entities/zombie"));
        assert_eq!(key, "commands.drop.success.multiple_with_table");
        assert_eq!(args, ["0", "minecraft:entities/zombie"]);
    }

    #[test]
    fn the_reward_table_hand_off_rolls_stacks_for_a_player() {
        let error = roll_reward_table("minecraft:nope/none").unwrap_err();
        assert!(error.contains("minecraft:nope/none"), "{error}");
        if !data() {
            return;
        }
        let stacks = roll_reward_table("minecraft:blocks/stone").unwrap();
        assert_eq!(stacks, vec![stack("minecraft:cobblestone", 1)]);
        // A bare id is namespaced like the command does.
        assert!(roll_reward_table("blocks/stone").is_ok());
    }

    #[test]
    fn an_item_tool_argument_keeps_its_enchantments() {
        let silk = crate::game::commands_item::stack_for(
            "minecraft:diamond_pickaxe[enchantments={\"minecraft:silk_touch\":1}]",
            1,
        )
        .unwrap();
        let held = HeldTool::from_stack(&silk).expect("held");
        assert_eq!(held.tool_enchantments.get("minecraft:silk_touch"), Some(&1));
        if !data() {
            return;
        }
        let mut with_silk = LootContext {
            block: Some("minecraft:stone".into()),
            ..LootContext::default()
        };
        held.apply(&mut with_silk);
        assert_eq!(
            roll_stacks("minecraft:blocks/stone", &with_silk).unwrap(),
            vec![stack("minecraft:stone", 1)],
            "silk touch drops the block itself"
        );
        let mut plain = LootContext::default();
        HeldTool::from_stack(&stack("minecraft:diamond_pickaxe", 1))
            .unwrap()
            .apply(&mut plain);
        assert_eq!(
            roll_stacks("minecraft:blocks/stone", &plain).unwrap(),
            vec![stack("minecraft:cobblestone", 1)]
        );
        // `mine` without a tool is an empty hand.
        assert_eq!(
            roll_stacks("minecraft:blocks/stone", &LootContext::default()).unwrap(),
            vec![stack("minecraft:cobblestone", 1)]
        );
    }

    #[test]
    fn kill_context_credits_a_player_source_with_its_looting_level() {
        let player = Uuid::from_u128(11);
        let state = Arc::new(Mutex::new(GameState::default()));
        state.lock().unwrap().join(player, "Alice".into(), 0, 20);
        let sword = ferrite_item::ItemStack::from_name("minecraft:diamond_sword").unwrap();
        let mut sword = sword;
        let looting = ferrite_item::component_id("enchantments").unwrap();
        let mut enchantments = ferrite_item::Enchantments::new();
        enchantments.set("minecraft:looting".to_owned(), 3);
        sword
            .components
            .insert(looting, Component::Enchantments(enchantments));
        HELD_TOOLS
            .lock()
            .unwrap()
            .insert(player, [HeldTool::from_stack(&sword), None]);
        let by_player = kill_context(&source(Some(player), state.clone()));
        assert!(by_player.killed_by_player);
        assert_eq!(by_player.looting, 3);
        // The console, or an entity that is not a player, kills without credit.
        let console = kill_context(&source(None, state.clone()));
        assert!(!console.killed_by_player);
        assert_eq!(console.looting, 0);
        let mob = kill_context(&source(Some(Uuid::from_u128(12)), state));
        assert!(!mob.killed_by_player);
    }

    #[test]
    fn killed_by_player_pools_only_drop_for_a_player_source() {
        if !data() {
            return;
        }
        let credited = LootContext {
            killed_by_player: true,
            ..LootContext::default()
        };
        let mut rods = 0;
        for _ in 0..400 {
            assert!(
                roll_stacks("minecraft:entities/blaze", &LootContext::default())
                    .unwrap()
                    .is_empty(),
                "no credit, no blaze rod"
            );
            rods += roll_stacks("minecraft:entities/blaze", &credited)
                .unwrap()
                .iter()
                .map(|stack| stack.count)
                .sum::<i32>();
        }
        assert!(rods > 0, "a credited kill drops blaze rods");
    }

    #[test]
    fn oversized_rolls_are_split_at_the_stack_size() {
        let entry = |item: &str, count| ferrite_loot::LootStack {
            item: item.into(),
            count,
            components: BTreeMap::new(),
        };
        let out = split_stacks(vec![
            entry("minecraft:cobblestone", 150),
            entry("minecraft:stone", 0),
            entry("minecraft:nope", 5),
            entry("minecraft:ender_pearl", 20),
        ]);
        let shape: Vec<_> = out
            .iter()
            .map(|stack| (stack.name().unwrap(), stack.count))
            .collect();
        assert_eq!(
            shape,
            [
                ("minecraft:cobblestone", 64),
                ("minecraft:cobblestone", 64),
                ("minecraft:cobblestone", 22),
                ("minecraft:ender_pearl", 16),
                ("minecraft:ender_pearl", 4),
            ]
        );
    }

    #[test]
    fn give_hands_the_stacks_to_the_player_and_returns_their_total() {
        if !data() {
            return;
        }
        let player = Uuid::from_u128(21);
        let state = Arc::new(Mutex::new(GameState::default()));
        state.lock().unwrap().join(player, "Alice".into(), 0, 20);
        let (result, actions) = run(
            "loot give @s loot minecraft:blocks/stone",
            Some(player),
            state.clone(),
        );
        assert_eq!(result.unwrap(), 1);
        assert!(matches!(
            actions.as_slice(),
            [Action::LootGive { stacks, table: None, .. }] if stacks == &vec![stack("minecraft:cobblestone", 1)]
        ));
        // Mainhand needs an entity source that can hold items.
        let error = run("loot give @s mine 0 64 0 mainhand", None, state.clone())
            .0
            .unwrap_err()
            .to_string();
        assert!(error.contains("can't hold any items"), "{error}");
        // A player with nothing selected still mines with an empty hand, then needs a loaded block.
        let error = run("loot give @s mine 0 64 0 mainhand", Some(player), state)
            .0
            .unwrap_err()
            .to_string();
        assert!(error.contains("not loaded"), "{error}");
    }
}
