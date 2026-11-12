//! Block drops: vanilla loot tables rolled when a survival player breaks a block.

use ferrite_loot::{LootContext, LootTables};
use ferrite_protocol::packets::play::ItemStack;
use std::sync::OnceLock;

/// Loot tables from the extracted vanilla data, loaded on first use. `None` (no data on this
/// machine, or a load error) means blocks drop nothing.
fn tables() -> Option<&'static LootTables> {
    static TABLES: OnceLock<Option<LootTables>> = OnceLock::new();
    TABLES
        .get_or_init(|| {
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../vanilla/26.3/generated/data/minecraft/loot_table");
            LootTables::load(&dir)
                .inspect_err(|error| tracing::warn!(%error, "block drops disabled: no loot tables"))
                .ok()
        })
        .as_ref()
}

/// Rolls a loot table by id for `/loot`: `(namespaced item id, count)` stacks, or `None` when
/// the table does not exist (or the vanilla data is missing).
pub fn roll_table(id: &str, context: &LootContext) -> Option<Vec<(String, i32)>> {
    let id = if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    };
    let table = tables()?.get(&id)?;
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64);
    let mut rng = ferrite_entity::Rng::new(seed);
    Some(table.roll(context, &mut LootRng(&mut rng)))
}

/// Loads the loot tables off the tick threads, so the first broken block does not stall its
/// region while they parse.
pub fn preload() {
    std::thread::spawn(|| {
        tables();
    });
}

/// Adapts the entity RNG to loot evaluation.
struct LootRng<'a>(&'a mut ferrite_entity::Rng);

impl ferrite_loot::Rng for LootRng<'_> {
    fn next_int(&mut self, bound: i32) -> i32 {
        self.0.next_int(bound)
    }

    fn next_float(&mut self) -> f32 {
        self.0.next_f32()
    }
}

/// Vanilla `Tool.isCorrectForDrops`: the first rule that names the block and decides drops.
fn tool_is_correct(item_id: u16, block: &str) -> bool {
    let Some(defaults) = ferrite_item::item_defaults(item_id) else {
        return false;
    };
    let tool_id = ferrite_item::component_ids::TOOL;
    let Some(tag) = defaults.components().remove(&tool_id) else {
        return false;
    };
    let ferrite_item::Component::Tool(tool) = ferrite_item::Component::from_nbt(tool_id, &tag)
    else {
        return false;
    };
    tool.rules
        .iter()
        .filter_map(|rule| rule.correct_for_drops.map(|correct| (rule, correct)))
        .find(|(rule, _)| match rule.blocks.strip_prefix('#') {
            Some(tag) => ferrite_data::tags::contains("block", tag, block),
            None => rule.blocks.split(',').any(|b| {
                b.trim().strip_prefix("minecraft:").unwrap_or(b.trim())
                    == block.strip_prefix("minecraft:").unwrap_or(block)
            }),
        })
        .is_some_and(|(_, correct)| correct)
}

/// Rolls the loot table of `state` broken with `tool`; `None` when the block has no table or
/// the tool is not correct for it. `block_entity` is what the broken block entity writes onto
/// its item, for `copy_components`.
fn roll_block(
    state: u16,
    tool: &ferrite_item::ItemStack,
    rng: &mut ferrite_entity::Rng,
    block_entity: &std::collections::BTreeMap<String, ferrite_nbt::Tag>,
) -> Vec<ferrite_loot::LootStack> {
    if usize::from(state) >= ferrite_data::blocks::STATE_COUNT {
        return Vec::new();
    }
    let Some((name, props)) = ferrite_data::blocks::state(state) else {
        return Vec::new();
    };
    if ferrite_data::block_props::get(state).requires_correct_tool
        && (tool.count <= 0 || !tool_is_correct(tool.item, name))
    {
        return Vec::new();
    }
    let Some(table) = tables().and_then(|t| {
        t.get(&format!(
            "minecraft:blocks/{}",
            name.strip_prefix("minecraft:").unwrap_or(name)
        ))
    }) else {
        return Vec::new();
    };
    // Vanilla `LootContextParams.TOOL`: the item's enchantments feed `match_tool` predicates
    // (silk touch) and `apply_bonus` (fortune).
    let mut tool_enchantments = std::collections::BTreeMap::new();
    let mut fortune = 0;
    if let Some(ferrite_item::Component::Enchantments(enchantments)) = tool
        .components
        .get(ferrite_item::component_ids::ENCHANTMENTS)
    {
        for (name, level) in &enchantments.levels {
            tool_enchantments.insert(name.clone(), *level);
        }
        fortune = enchantments.level("minecraft:fortune").unwrap_or(0);
    }
    let context = LootContext {
        tool: (tool.count > 0)
            .then(|| ferrite_item::item_name(tool.item))
            .flatten()
            .map(str::to_owned),
        tool_enchantments,
        fortune,
        block: Some(name.to_owned()),
        block_state: props
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
        block_entity_components: block_entity.clone(),
        ..LootContext::default()
    };
    table.roll_stacks(&context, &mut LootRng(rng))
}

/// What breaking `state` with `tool` drops (vanilla `Block.getDrops`, only when the player
/// `hasCorrectToolForDrops`), as plain id and count stacks.
pub fn block_drops(
    state: u16,
    tool: &ferrite_item::ItemStack,
    rng: &mut ferrite_entity::Rng,
) -> Vec<ItemStack> {
    roll_block(state, tool, rng, &Default::default())
        .into_iter()
        .filter_map(|stack| {
            let item_id = ferrite_data::registries::id("minecraft:item", &stack.item)?;
            (stack.count > 0).then_some(ItemStack {
                count: stack.count,
                item_id,
            })
        })
        .collect()
}

/// [`block_drops`] with the components a loot table copied from the broken block entity, such
/// as a shulker box's contents, name and lock.
pub fn block_drop_stacks(
    state: u16,
    tool: &ferrite_item::ItemStack,
    rng: &mut ferrite_entity::Rng,
    block_entity: &std::collections::BTreeMap<String, ferrite_nbt::Tag>,
) -> Vec<ferrite_item::ItemStack> {
    roll_block(state, tool, rng, block_entity)
        .into_iter()
        .filter_map(|rolled| {
            let item = ferrite_item::item_id(&rolled.item)?;
            if rolled.count <= 0 {
                return None;
            }
            let mut stack = ferrite_item::ItemStack::with_count(item, rolled.count);
            for (name, tag) in &rolled.components {
                if let Some(id) = ferrite_item::component_id(name) {
                    stack
                        .components
                        .insert(id, ferrite_item::Component::from_nbt(id, tag));
                }
            }
            Some(stack)
        })
        .collect()
}

/// Rolls a mob's entity loot table (vanilla `LivingEntity.dropFromLootTable`), for example
/// leather and beef for a cow.
pub fn mob_drops(
    kind: ferrite_entity::mob::MobKind,
    rng: &mut ferrite_entity::Rng,
) -> Vec<ItemStack> {
    let Some(table) = tables().and_then(|tables| tables.get(kind.loot_table())) else {
        return Vec::new();
    };
    let context = LootContext {
        killed_by_player: true,
        ..LootContext::default()
    };
    table
        .roll(&context, &mut LootRng(rng))
        .into_iter()
        .filter_map(|(item, count)| {
            let item_id = ferrite_data::registries::id("minecraft:item", &item)?;
            (count > 0).then_some(ItemStack { count, item_id })
        })
        .collect()
}

/// Rolls the fishing table (`gameplay/fishing`) for a rod. Luck of the Sea is not applied: the
/// loot context carries no luck. Empty when the loot data is missing.
pub fn fishing_catch(
    rod: &ferrite_item::ItemStack,
    rng: &mut ferrite_entity::Rng,
) -> Vec<ItemStack> {
    let Some(table) = tables().and_then(|tables| tables.get("minecraft:gameplay/fishing")) else {
        return Vec::new();
    };
    let context = LootContext {
        tool: rod.name().map(str::to_owned),
        ..LootContext::default()
    };
    table
        .roll(&context, &mut LootRng(rng))
        .into_iter()
        .filter_map(|(item, count)| {
            let item_id = ferrite_data::registries::id("minecraft:item", &item)?;
            (count > 0).then_some(ItemStack { count, item_id })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fishing_table_yields_items_for_a_rod() {
        let rod = ferrite_item::ItemStack::from_name("minecraft:fishing_rod").unwrap();
        let mut rng = ferrite_entity::Rng::new(3);
        for _ in 0..20 {
            let catch = fishing_catch(&rod, &mut rng);
            assert!(!catch.is_empty(), "every roll catches something");
            assert!(catch.iter().all(|stack| stack.count > 0));
        }
    }

    fn tool(name: &str) -> ferrite_item::ItemStack {
        ferrite_item::ItemStack::from_name(name).unwrap()
    }

    fn drop_stack(name: &str) -> ItemStack {
        ItemStack {
            count: 1,
            item_id: ferrite_data::registries::id("minecraft:item", name).unwrap(),
        }
    }

    #[test]
    fn blocks_drop_their_loot_with_the_right_tool() {
        let mut rng = ferrite_entity::Rng::new(4);
        let empty = ferrite_item::ItemStack::empty();
        let grass = ferrite_data::blocks::default_state("grass_block").unwrap();
        assert_eq!(
            block_drops(grass, &empty, &mut rng),
            [drop_stack("minecraft:dirt")]
        );
        let stone = ferrite_data::blocks::default_state("stone").unwrap();
        // Stone needs a pickaxe.
        assert!(block_drops(stone, &empty, &mut rng).is_empty());
        assert!(block_drops(stone, &tool("minecraft:wooden_shovel"), &mut rng).is_empty());
        assert_eq!(
            block_drops(stone, &tool("minecraft:wooden_pickaxe"), &mut rng),
            [drop_stack("minecraft:cobblestone")]
        );
        // Iron ore needs at least a stone pickaxe.
        let ore = ferrite_data::blocks::default_state("iron_ore").unwrap();
        assert!(block_drops(ore, &tool("minecraft:wooden_pickaxe"), &mut rng).is_empty());
        assert_eq!(
            block_drops(ore, &tool("minecraft:stone_pickaxe"), &mut rng),
            [drop_stack("minecraft:raw_iron")]
        );
        // Enchantment-free silk touch: the loot table's `match_tool` sees the tool's
        // `minecraft:silk_touch` component and keeps the block itself.
        let mut silk = tool("minecraft:stone_pickaxe");
        let mut enchantments = ferrite_item::Enchantments::new();
        enchantments.set("minecraft:silk_touch", 1);
        silk.components.insert(
            ferrite_item::component_ids::ENCHANTMENTS,
            ferrite_item::Component::Enchantments(enchantments),
        );
        assert_eq!(
            block_drops(ore, &silk, &mut rng),
            [drop_stack("minecraft:iron_ore")]
        );
        assert_eq!(
            block_drops(stone, &silk, &mut rng),
            [drop_stack("minecraft:stone")]
        );
        assert!(block_drops(0, &empty, &mut rng).is_empty());
    }

    #[test]
    fn cows_drop_leather_and_beef() {
        let leather = drop_stack("minecraft:leather");
        let beef = drop_stack("minecraft:beef");
        let mut rng = ferrite_entity::Rng::new(9);
        let mut saw_leather = false;
        for _ in 0..64 {
            let drops = mob_drops(ferrite_entity::mob::MobKind::Cow, &mut rng);
            assert!(!drops.is_empty(), "a cow always drops beef");
            assert!(
                drops
                    .iter()
                    .all(|stack| stack.item_id == beef.item_id || stack.item_id == leather.item_id),
                "unexpected cow drops: {drops:?}"
            );
            assert!(
                drops
                    .iter()
                    .any(|stack| stack.item_id == beef.item_id && stack.count >= 1)
            );
            saw_leather |= drops.iter().any(|stack| stack.item_id == leather.item_id);
        }
        assert!(saw_leather, "leather drops 2/3 of the time");
    }
}
