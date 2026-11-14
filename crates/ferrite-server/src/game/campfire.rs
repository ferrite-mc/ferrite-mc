//! Campfire block entities (vanilla `CampfireBlockEntity`).
//!
//! Four food slots cook independently. The items and the per-slot cooking progress and total
//! time live in the block entity's NBT (`Items`, `CookingTimes`, `CookingTotalTimes`), so they
//! survive a chunk save and `/data merge block`. A region ticks every loaded campfire: a lit one
//! advances its food and drops the result into the world, an unlit one cools the progress down.
//! Placing food is the right-click interaction (`CampfireBlock.useItemOn`), which needs an empty
//! slot and a `campfire_cooking` recipe for the held item.

use ferrite_inventory::CookingOutput;
use ferrite_item::ItemStack;
use ferrite_nbt::{Compound, List, Tag};
use ferrite_world::BlockEntity;

use super::anvil::AnvilChunkSource;

/// Slots of a campfire: four food items.
pub const SLOTS: usize = 4;

/// The block entity type of both the campfire and the soul campfire.
pub const BLOCK: &str = "minecraft:campfire";

/// Ticks per second the progress of a removed item cools back down (vanilla `BURN_COOL_SPEED`).
const BURN_COOL_SPEED: i32 = 2;

/// The block entity state of a campfire.
#[derive(Clone, Debug, PartialEq)]
pub struct CampfireData {
    pub items: Vec<ItemStack>,
    /// Vanilla `CookingTimes`: ticks cooked so far.
    pub cooking_times: [i32; SLOTS],
    /// Vanilla `CookingTotalTimes`: ticks the recipe takes.
    pub cooking_total_times: [i32; SLOTS],
}

impl Default for CampfireData {
    fn default() -> Self {
        Self {
            items: vec![ItemStack::empty(); SLOTS],
            cooking_times: [0; SLOTS],
            cooking_total_times: [0; SLOTS],
        }
    }
}

/// Vanilla `ContainerHelper.loadAllItems` for the four campfire slots.
fn load_items(entity: Option<&BlockEntity>) -> Vec<ItemStack> {
    let mut items = vec![ItemStack::empty(); SLOTS];
    let Some(Tag::List(List::Compound(list))) = entity.and_then(|e| e.data.get("Items")) else {
        return items;
    };
    for entry in list {
        let Some(Tag::Byte(slot)) = entry.get("Slot") else {
            continue;
        };
        let Some(target) = usize::try_from(*slot).ok().and_then(|s| items.get_mut(s)) else {
            continue;
        };
        match ItemStack::from_nbt(entry) {
            Ok(stack) => *target = stack,
            Err(error) => tracing::warn!(%error, "ignoring an unreadable campfire item"),
        }
    }
    items
}

/// Vanilla `ContainerHelper.saveAllItems`: non-empty slots with their `Slot` byte.
fn save_items(items: &[ItemStack]) -> Tag {
    let list = items
        .iter()
        .enumerate()
        .filter(|(_, stack)| !stack.is_empty())
        .filter_map(|(slot, stack)| {
            let mut entry = stack.to_nbt().ok()?;
            entry.insert("Slot".into(), Tag::Byte(slot as i8));
            Some(entry)
        })
        .collect();
    Tag::List(List::Compound(list))
}

/// One of the int arrays vanilla stores (`CookingTimes` or `CookingTotalTimes`).
fn int_array(entity: Option<&BlockEntity>, key: &str) -> [i32; SLOTS] {
    let mut out = [0; SLOTS];
    if let Some(Tag::IntArray(values)) = entity.and_then(|entity| entity.data.get(key)) {
        for (slot, value) in out.iter_mut().zip(values) {
            *slot = *value;
        }
    }
    out
}

fn block_entity_type() -> Option<u32> {
    ferrite_data::registries::id("minecraft:block_entity_type", BLOCK)
        .and_then(|id| u32::try_from(id).ok())
}

/// Reads a campfire's food and timers.
pub fn load(source: &AnvilChunkSource, [x, y, z]: [i32; 3]) -> CampfireData {
    let entity = source.block_entity(x, y, z);
    let entity = entity.as_ref();
    CampfireData {
        items: load_items(entity),
        cooking_times: int_array(entity, "CookingTimes"),
        cooking_total_times: int_array(entity, "CookingTotalTimes"),
    }
}

/// Writes the food and timers back, creating the block entity when one is missing.
pub fn store(source: &AnvilChunkSource, [x, y, z]: [i32; 3], data: &CampfireData) {
    let Some(type_id) = block_entity_type() else {
        return;
    };
    let result = source.update_block_entity(x, y, z, |entity| {
        let current = entity.as_ref().map(|entity| CampfireData {
            items: load_items(Some(entity)),
            cooking_times: int_array(Some(entity), "CookingTimes"),
            cooking_total_times: int_array(Some(entity), "CookingTotalTimes"),
        });
        if current.as_ref() == Some(data) {
            return (false, ());
        }
        let entity = entity.get_or_insert_with(|| {
            let mut nbt = Compound::new();
            nbt.insert("id".into(), Tag::String(BLOCK.into()));
            nbt.insert("x".into(), Tag::Int(x));
            nbt.insert("y".into(), Tag::Int(y));
            nbt.insert("z".into(), Tag::Int(z));
            BlockEntity { type_id, data: nbt }
        });
        entity.data.insert("Items".into(), save_items(&data.items));
        entity.data.insert(
            "CookingTimes".into(),
            Tag::IntArray(data.cooking_times.to_vec()),
        );
        entity.data.insert(
            "CookingTotalTimes".into(),
            Tag::IntArray(data.cooking_total_times.to_vec()),
        );
        (true, ())
    });
    if let Err(error) = result {
        tracing::warn!(x, y, z, %error, "could not store campfire state");
    }
}

/// Creates the block entity of a freshly placed campfire.
pub fn ensure(source: &AnvilChunkSource, pos: [i32; 3]) {
    store(source, pos, &CampfireData::default());
}

/// Vanilla `CampfireBlockEntity.placeFood`: the first empty slot takes one of `stack` and the
/// recipe's cooking time. `false` when all four slots are taken (the caller then consumes
/// nothing), matching vanilla's empty-slot search.
pub fn place_food(
    source: &AnvilChunkSource,
    pos: [i32; 3],
    stack: &ItemStack,
    cooking_time: i32,
) -> bool {
    let mut data = load(source, pos);
    let Some(slot) = data.items.iter().position(ItemStack::is_empty) else {
        return false;
    };
    data.items[slot] = stack.copy_with_count(1);
    data.cooking_times[slot] = 0;
    data.cooking_total_times[slot] = cooking_time;
    store(source, pos, &data);
    true
}

/// Vanilla `CampfireBlockEntity.cookTick`: advances every item on a lit campfire and returns the
/// stacks that finished cooking (vanilla drops them at the block with `Containers.dropItemStack`
/// rather than putting them in the slot).
pub fn tick(
    source: &AnvilChunkSource,
    pos: [i32; 3],
    recipe: &dyn Fn(&ItemStack) -> Option<CookingOutput>,
) -> Vec<ItemStack> {
    let mut data = load(source, pos);
    let mut drops = Vec::new();
    let mut changed = false;
    for slot in 0..SLOTS {
        if data.items[slot].is_empty() {
            continue;
        }
        changed = true;
        data.cooking_times[slot] = data.cooking_times[slot].saturating_add(1);
        if data.cooking_times[slot] < data.cooking_total_times[slot] {
            continue;
        }
        // Vanilla: a missing recipe drops the raw item again instead of deleting it.
        let item = data.items[slot].clone();
        drops.push(recipe(&item).map_or(item, |output| output.result));
        data.items[slot] = ItemStack::empty();
        data.cooking_times[slot] = 0;
        data.cooking_total_times[slot] = 0;
    }
    if changed {
        store(source, pos, &data);
    }
    drops
}

/// Vanilla `CampfireBlockEntity.cooldownTick`: an unlit campfire cools its progress, and an
/// empty slot loses what it had accumulated.
pub fn cool(source: &AnvilChunkSource, pos: [i32; 3]) {
    let mut data = load(source, pos);
    let mut changed = false;
    for slot in 0..SLOTS {
        if data.cooking_times[slot] > 0 {
            data.cooking_times[slot] = (data.cooking_times[slot] - BURN_COOL_SPEED)
                .clamp(0, data.cooking_total_times[slot].max(0));
            changed = true;
        }
    }
    if changed {
        store(source, pos, &data);
    }
}

/// Removes the block entity of a broken campfire and returns the food it still held.
pub fn take_contents(source: &AnvilChunkSource, [x, y, z]: [i32; 3]) -> Vec<ItemStack> {
    source
        .update_block_entity(x, y, z, |entity| {
            entity.take().map_or((false, Vec::new()), |removed| {
                (true, load_items(Some(&removed)))
            })
        })
        .unwrap_or_default()
        .into_iter()
        .filter(|stack| !stack.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_inventory::CookingOutput;

    async fn loaded() -> std::sync::Arc<AnvilChunkSource> {
        let source = AnvilChunkSource::new_memory();
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        source
    }

    fn cooked_cod() -> CookingOutput {
        CookingOutput {
            result: ItemStack::from_name("minecraft:cooked_cod").unwrap(),
            cook_time: 600,
            experience: 0.35,
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn food_and_timers_round_trip_and_cooking_drops_the_result() {
        let source = loaded().await;
        let state = u32::from(
            ferrite_data::blocks::state_id(
                "campfire",
                &[
                    ("facing", "north"),
                    ("lit", "true"),
                    ("signal_fire", "false"),
                    ("waterlogged", "false"),
                ],
            )
            .expect("a vanilla campfire"),
        );
        source.set_block(6, 64, 6, state).unwrap();
        ensure(&source, [6, 64, 6]);
        let cod = ItemStack::from_name("minecraft:cod").unwrap();
        assert!(place_food(&source, [6, 64, 6], &cod, 600));
        let data = load(&source, [6, 64, 6]);
        assert_eq!(data.items[0].name(), Some("minecraft:cod"));
        assert_eq!(
            (data.cooking_times[0], data.cooking_total_times[0]),
            (0, 600)
        );
        assert_eq!(
            source
                .block_entity(6, 64, 6)
                .unwrap()
                .data
                .get("CookingTimes"),
            Some(&Tag::IntArray(vec![0, 0, 0, 0]))
        );

        let recipe = |_: &ItemStack| Some(cooked_cod());
        for _ in 0..599 {
            assert!(
                tick(&source, [6, 64, 6], &recipe).is_empty(),
                "cooking takes 600 ticks"
            );
        }
        let drops = tick(&source, [6, 64, 6], &recipe);
        assert_eq!(drops.len(), 1);
        assert_eq!(drops[0].name(), Some("minecraft:cooked_cod"));
        assert!(
            load(&source, [6, 64, 6])
                .items
                .iter()
                .all(ItemStack::is_empty)
        );
        // The slot is free again for the next piece.
        assert!(place_food(&source, [6, 64, 6], &cod, 600));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_unlit_campfire_cools_progress_and_a_broken_one_spills_its_food() {
        let source = loaded().await;
        let state = u32::from(
            ferrite_data::blocks::state_id(
                "campfire",
                &[
                    ("facing", "north"),
                    ("lit", "false"),
                    ("signal_fire", "false"),
                    ("waterlogged", "false"),
                ],
            )
            .unwrap(),
        );
        source.set_block(7, 64, 7, state).unwrap();
        ensure(&source, [7, 64, 7]);
        let cod = ItemStack::from_name("minecraft:cod").unwrap();
        assert!(place_food(&source, [7, 64, 7], &cod, 600));
        // Four ticks of cooking progress, then the fire goes out.
        for _ in 0..4 {
            tick(&source, [7, 64, 7], &|_| None);
        }
        assert_eq!(load(&source, [7, 64, 7]).cooking_times[0], 4);
        cool(&source, [7, 64, 7]);
        assert_eq!(load(&source, [7, 64, 7]).cooking_times[0], 2);
        // A campfire has four slots; the fifth piece has nowhere to go.
        for _ in 0..3 {
            assert!(place_food(&source, [7, 64, 7], &cod, 600));
        }
        assert!(!place_food(&source, [7, 64, 7], &cod, 600));
        assert_eq!(
            take_contents(&source, [7, 64, 7]),
            vec![ItemStack::from_name("minecraft:cod").unwrap(); 4]
        );
        assert!(source.block_entity(7, 64, 7).is_none());
    }
}
