//! Block drops: vanilla loot tables rolled when a survival player breaks a block.

use ferrite_loot::{LootContext, LootTables};
use ferrite_protocol::packets::play::ItemStack;
use std::sync::OnceLock;

/// Loot tables from the extracted vanilla data, loaded on first use. `None` (no data on this
/// machine, or a load error) means blocks drop nothing.
pub(super) fn tables() -> Option<&'static LootTables> {
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

/// What loot functions ask the server: item defaults, and blocks from a world when `/loot` has
/// one. Biomes are not available here, so `location_check` on a biome fails (vanilla would
/// look the biome up).
#[derive(Default)]
pub struct ServerLootHost {
    world: Option<std::sync::Arc<super::anvil::AnvilChunkSource>>,
}

impl std::fmt::Debug for ServerLootHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerLootHost")
            .field("world", &self.world.is_some())
            .finish()
    }
}

impl ServerLootHost {
    /// A host that can also answer `block_at` from `world`.
    pub fn with_world(
        world: Option<std::sync::Arc<super::anvil::AnvilChunkSource>>,
    ) -> std::sync::Arc<dyn ferrite_loot::LootHost> {
        std::sync::Arc::new(Self { world })
    }
}

impl ferrite_loot::LootHost for ServerLootHost {
    fn enchantability(&self, item: &str) -> Option<i32> {
        let id = ferrite_item::item_id(item)?;
        match ferrite_item::default_components(id)?
            .get(&ferrite_item::component_ids::ENCHANTABLE)?
        {
            ferrite_nbt::Tag::Int(value) => Some(*value),
            ferrite_nbt::Tag::Compound(fields) => match fields.get("value") {
                Some(ferrite_nbt::Tag::Int(value)) => Some(*value),
                _ => None,
            },
            _ => None,
        }
    }

    fn max_damage(&self, item: &str) -> Option<i32> {
        ferrite_item::item_defaults_by_name(item)?.max_damage
    }

    fn max_stack_size(&self, item: &str) -> i32 {
        ferrite_item::item_defaults_by_name(item).map_or(64, |defaults| defaults.max_stack_size)
    }

    fn block_at(
        &self,
        pos: [i32; 3],
    ) -> Option<(String, std::collections::BTreeMap<String, String>)> {
        let state = self.world.as_ref()?.block(pos[0], pos[1], pos[2])?;
        let (name, props) = ferrite_data::blocks::state(u16::try_from(state).ok()?)?;
        Some((
            name.to_owned(),
            props
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        ))
    }
}

/// The default host (no world), shared.
pub fn default_host() -> std::sync::Arc<dyn ferrite_loot::LootHost> {
    static HOST: OnceLock<std::sync::Arc<dyn ferrite_loot::LootHost>> = OnceLock::new();
    HOST.get_or_init(|| ServerLootHost::with_world(None))
        .clone()
}

/// Rolls a loot table by id for `/loot`: `(namespaced item id, count)` stacks, or `None` when
/// the table does not exist (or the vanilla data is missing).
pub fn roll_table(id: &str, context: &LootContext) -> Option<Vec<(String, i32)>> {
    Some(
        roll_table_stacks(id, context)?
            .into_iter()
            .map(|stack| (stack.item, stack.count))
            .collect(),
    )
}

/// [`roll_table`] keeping the components loot functions attached (such as a shulker box's
/// contents copied from the block entity in `context`).
pub fn roll_table_stacks(id: &str, context: &LootContext) -> Option<Vec<ferrite_loot::LootStack>> {
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
    Some(table.roll_stacks(context, &mut LootRng(&mut rng)))
}

/// Loads the loot tables off the tick threads, so the first broken block does not stall its
/// region while they parse.
pub fn preload() {
    std::thread::spawn(|| {
        tables();
    });
}

/// Adapts the entity RNG to loot evaluation.
pub(super) struct LootRng<'a>(pub(super) &'a mut ferrite_entity::Rng);

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
        host: Some(default_host()),
        ..LootContext::default()
    };
    table.roll_stacks(&context, &mut LootRng(rng))
}

/// What breaking `state` with `tool` drops (vanilla `Block.getDrops`, only when the player
/// `hasCorrectToolForDrops`), as plain id and count stacks. The game itself uses
/// [`block_drop_stacks`], which keeps components; this is the plain form the tests compare.
#[cfg(test)]
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

/// Rolls a mob's entity loot table with a player credited for the kill (vanilla
/// `LivingEntity.dropFromLootTable`), for example leather and beef for a cow.
pub fn mob_drops(
    kind: ferrite_entity::mob::MobKind,
    rng: &mut ferrite_entity::Rng,
) -> Vec<ItemStack> {
    mob_drops_by(kind, rng, Some(uuid::Uuid::nil()), false)
}

/// [`mob_drops`] for a kill by `killer` (a player, with their held weapon's looting and
/// enchantments) or no player at all, of a mob that is `burning`.
pub fn mob_drops_by(
    kind: ferrite_entity::mob::MobKind,
    rng: &mut ferrite_entity::Rng,
    killer: Option<uuid::Uuid>,
    burning: bool,
) -> Vec<ItemStack> {
    let Some(table) = tables().and_then(|tables| tables.get(kind.loot_table())) else {
        return Vec::new();
    };
    let weapon = killer.and_then(|uuid| {
        super::commands_loot::HELD_TOOLS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&uuid)
            .and_then(|hands| hands[0].clone())
    });
    let player = ferrite_loot::EntityFacts {
        entity_type: "minecraft:player".to_owned(),
        mainhand: weapon
            .as_ref()
            .map(|held| (held.item.clone(), held.tool_enchantments.clone())),
        ..Default::default()
    };
    let context = LootContext {
        killed_by_player: killer.is_some(),
        looting: weapon.as_ref().map_or(0, |held| held.looting),
        this_entity: Some(ferrite_loot::EntityFacts {
            entity_type: format!("minecraft:{}", kind.name().trim_start_matches("minecraft:")),
            on_fire: burning,
            ..Default::default()
        }),
        damage_source: Some(ferrite_loot::DamageFacts {
            damage_type: "minecraft:generic".to_owned(),
            source_entity: killer.map(|_| player.clone()),
            direct_entity: killer.map(|_| player),
        }),
        host: Some(default_host()),
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

/// Rolls the fishing table (`gameplay/fishing`) for a rod; the rod's Luck of the Sea level is
/// the context's luck (vanilla `FishingHook.retrieve`: `luck + player luck attribute`, the
/// attribute is not modelled here). Empty when the loot data is missing.
pub fn fishing_catch(
    rod: &ferrite_item::ItemStack,
    rng: &mut ferrite_entity::Rng,
) -> Vec<ItemStack> {
    let Some(table) = tables().and_then(|tables| tables.get("minecraft:gameplay/fishing")) else {
        return Vec::new();
    };
    let luck = match rod.effective_component(ferrite_item::component_ids::ENCHANTMENTS) {
        Some(ferrite_item::Component::Enchantments(enchantments)) => {
            enchantments.level("minecraft:luck_of_the_sea").unwrap_or(0) as f32
        }
        _ => 0.0,
    };
    let context = LootContext {
        tool: rod.name().map(str::to_owned),
        luck,
        host: Some(default_host()),
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

    fn item_id(name: &str) -> i32 {
        ferrite_data::registries::id("minecraft:item", name).unwrap()
    }

    #[test]
    fn a_burning_cow_drops_cooked_beef_and_a_cold_one_raw_beef() {
        if tables().is_none() {
            return;
        }
        let mut rng = ferrite_entity::Rng::new(9);
        let kind = ferrite_entity::mob::MobKind::Cow;
        let player = Some(uuid::Uuid::from_u128(1));
        let mut ids = |burning| -> std::collections::BTreeSet<i32> {
            (0..40)
                .flat_map(|_| mob_drops_by(kind, &mut rng, player, burning))
                .map(|stack| stack.item_id)
                .collect()
        };
        let cold = ids(false);
        assert!(
            cold.contains(&item_id("minecraft:beef"))
                && !cold.contains(&item_id("minecraft:cooked_beef"))
        );
        let burning = ids(true);
        assert!(
            burning.contains(&item_id("minecraft:cooked_beef"))
                && !burning.contains(&item_id("minecraft:beef"))
        );
    }

    #[test]
    fn a_mob_with_no_credited_player_skips_the_player_only_pools() {
        if tables().is_none() {
            return;
        }
        let mut rng = ferrite_entity::Rng::new(4);
        let kind = ferrite_entity::mob::MobKind::Spider;
        let rod = item_id("minecraft:spider_eye");
        let by_player = (0..60)
            .flat_map(|_| mob_drops_by(kind, &mut rng, Some(uuid::Uuid::from_u128(1)), false))
            .any(|stack| stack.item_id == rod);
        let alone = (0..60)
            .flat_map(|_| mob_drops_by(kind, &mut rng, None, false))
            .any(|stack| stack.item_id == rod);
        assert!(by_player && !alone);
    }

    #[test]
    fn luck_of_the_sea_shifts_a_catch_towards_treasure() {
        if tables().is_none() {
            return;
        }
        let plain = ferrite_item::ItemStack::from_name("minecraft:fishing_rod").unwrap();
        let mut lucky = plain.clone();
        let mut enchantments = ferrite_item::Enchantments::default();
        enchantments.set("minecraft:luck_of_the_sea".to_owned(), 3);
        lucky.components.insert(
            ferrite_item::component_ids::ENCHANTMENTS,
            ferrite_item::Component::Enchantments(enchantments),
        );
        let treasure = [
            item_id("minecraft:saddle"),
            item_id("minecraft:name_tag"),
            item_id("minecraft:nautilus_shell"),
        ];
        let count = |rod: &ferrite_item::ItemStack| {
            let mut rng = ferrite_entity::Rng::new(77);
            (0..3000)
                .flat_map(|_| fishing_catch(rod, &mut rng))
                .filter(|stack| treasure.contains(&stack.item_id))
                .count()
        };
        let (base, with_luck) = (count(&plain), count(&lucky));
        assert!(with_luck > base, "treasure {base} -> {with_luck}");
    }

    /// Every shipped table rolls under the server host, and the components its functions attach
    /// survive the conversion to item stacks (enchantments stay readable as typed components).
    #[test]
    fn every_shipped_table_rolls_with_the_server_host() {
        let Some(tables) = tables() else { return };
        assert_eq!(tables.unsupported(), Vec::<String>::new());
        let context = LootContext {
            killed_by_player: true,
            looting: 1,
            host: Some(default_host()),
            ..LootContext::default()
        };
        let mut enchanted = 0;
        for id in tables.ids().map(str::to_owned).collect::<Vec<_>>() {
            let mut rng = ferrite_entity::Rng::new(id.len() as u64);
            for stack in tables
                .get(&id)
                .unwrap()
                .roll_stacks(&context, &mut LootRng(&mut rng))
            {
                assert!(
                    ferrite_item::item_id(&stack.item).is_some(),
                    "{id}: unknown item {}",
                    stack.item
                );
                if stack.components.contains_key("minecraft:enchantments")
                    || stack
                        .components
                        .contains_key("minecraft:stored_enchantments")
                {
                    enchanted += 1;
                }
            }
        }
        assert!(enchanted > 0, "no table enchanted anything");
    }

    #[test]
    fn an_exploration_map_without_a_structure_search_stays_unchanged() {
        use ferrite_loot::LootHost;
        let destinations = ["minecraft:buried_treasure".to_owned()];
        // No world at all, and no vanilla terrain behind a world: nothing is found, so the
        // stack is left as it is, like vanilla's "no structure found".
        assert!(
            ServerLootHost::default()
                .exploration_map([0.0; 3], &destinations, "minecraft:red_x", 1, false)
                .is_none()
        );
        let world = crate::game::anvil::AnvilChunkSource::new_memory();
        let host = ServerLootHost { world: Some(world) };
        assert!(
            host.exploration_map([0.0; 3], &destinations, "minecraft:red_x", 1, false)
                .is_none()
        );
    }

    #[test]
    fn the_server_host_reads_item_defaults() {
        use ferrite_loot::LootHost;
        let host = ServerLootHost::default();
        assert_eq!(host.enchantability("minecraft:iron_pickaxe"), Some(14));
        assert_eq!(host.enchantability("minecraft:stone"), None);
        assert_eq!(host.max_damage("minecraft:diamond_sword"), Some(1561));
        assert_eq!(host.max_stack_size("minecraft:diamond_sword"), 1);
        assert_eq!(host.max_stack_size("minecraft:stone"), 64);
    }

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
