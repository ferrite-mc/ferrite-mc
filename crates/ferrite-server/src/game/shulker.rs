//! Shulker boxes keep their contents when broken and carried (vanilla `ShulkerBoxBlock`,
//! `ShulkerBoxBlockEntity.collectImplicitComponents` / `applyImplicitComponents`).
//!
//! A shulker box block entity stores `Items` (slot byte plus the stack's own fields), an
//! optional `CustomName`, a `lock` and a `LootTable` with its `LootTableSeed`. The dropped item
//! carries the same data as the components `minecraft:container`, `custom_name`, `lock` and
//! `container_loot`. This module converts between the two, in both directions, as plain NBT
//! maps so the loot table's `copy_components` does the copying and the break path does not
//! special-case shulkers anywhere else.
//!
//! Shulker boxes never nest. A carried container that holds one is refused on placement and
//! the offending stack is handed back to the caller to drop, rather than silently deleted or
//! accepted.

use std::collections::BTreeMap;

use ferrite_item::{Component, ItemStack, component_id};
use ferrite_nbt::{Compound, List, Tag};

use super::anvil::AnvilChunkSource;
use super::block_container::{SIZE, is_shulker_box};

const CONTAINER: &str = "minecraft:container";
const CUSTOM_NAME: &str = "minecraft:custom_name";
const LOCK: &str = "minecraft:lock";
const CONTAINER_LOOT: &str = "minecraft:container_loot";

/// What converting a block entity to item components could not carry over.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Unreadable {
    /// `Items` entries with a missing or out-of-range slot, or a repeated slot. They stay out of
    /// the item and are reported, never guessed at.
    pub entries: usize,
}

/// The item of a shulker box block (every colour shares its name with its block).
fn shulker_item(block: &str) -> Option<&str> {
    is_shulker_box(block).then_some(block)
}

/// The components a shulker box block entity writes onto its item, keyed by component name.
pub fn collect_components(entity: &Compound) -> (BTreeMap<String, Tag>, Unreadable) {
    let mut components = BTreeMap::new();
    let mut unreadable = Unreadable::default();
    if let Some(Tag::List(items)) = entity.get("Items") {
        let (slots, bad) = container_from_items(items);
        unreadable.entries = bad;
        if !slots.is_empty() {
            components.insert(CONTAINER.to_owned(), Tag::List(List::Compound(slots)));
        }
    }
    if let Some(name) = entity.get("CustomName") {
        components.insert(CUSTOM_NAME.to_owned(), name.clone());
    }
    if let Some(lock) = entity.get("lock").or_else(|| entity.get("Lock")) {
        components.insert(LOCK.to_owned(), lock.clone());
    }
    if let Some(Tag::String(table)) = entity.get("LootTable") {
        let mut loot = Compound::new();
        loot.insert("loot_table".into(), Tag::String(table.clone()));
        if let Some(Tag::Long(seed)) = entity.get("LootTableSeed")
            && *seed != 0
        {
            loot.insert("seed".into(), Tag::Long(*seed));
        }
        components.insert(CONTAINER_LOOT.to_owned(), Tag::Compound(loot));
    }
    (components, unreadable)
}

/// `Items` (`{Slot, id, count, components}`) as the container component's `{slot, item}` list.
fn container_from_items(items: &List) -> (Vec<Compound>, usize) {
    let List::Compound(entries) = items else {
        return (Vec::new(), usize::from(!matches!(items, List::End)));
    };
    let mut slots: BTreeMap<i32, Compound> = BTreeMap::new();
    let mut bad = 0;
    for entry in entries {
        let slot = match entry.get("Slot") {
            Some(Tag::Byte(slot)) if (0..SIZE as i8).contains(slot) => i32::from(*slot),
            _ => {
                bad += 1;
                continue;
            }
        };
        let mut item = entry.clone();
        item.shift_remove("Slot");
        let mut out = Compound::new();
        out.insert("slot".into(), Tag::Int(slot));
        out.insert("item".into(), Tag::Compound(item));
        if slots.insert(slot, out).is_some() {
            bad += 1;
        }
    }
    (slots.into_values().collect(), bad)
}

/// Removes the shulker box block entity at `pos` and returns the components for its item.
pub fn take_components(source: &AnvilChunkSource, [x, y, z]: [i32; 3]) -> BTreeMap<String, Tag> {
    let taken = source
        .update_block_entity(x, y, z, |entity| match entity.take() {
            Some(removed) => (true, Some(removed.data)),
            None => (false, None),
        })
        .unwrap_or_default();
    let Some(data) = taken else {
        return BTreeMap::new();
    };
    let (components, unreadable) = collect_components(&data);
    if unreadable.entries > 0 {
        tracing::warn!(
            x,
            y,
            z,
            entries = unreadable.entries,
            "a broken shulker box had unreadable item entries that were not kept"
        );
    }
    components
}

/// Builds the item for a shulker box block with the given components. Components that do not
/// parse as their type are kept as opaque NBT, never dropped.
pub fn item_with(block: &str, components: &BTreeMap<String, Tag>) -> Option<ItemStack> {
    let mut stack = ItemStack::from_name(shulker_item(block)?)?;
    for (name, tag) in components {
        if let Some(id) = component_id(name) {
            stack.components.insert(id, Component::from_nbt(id, tag));
        }
    }
    Some(stack)
}

/// Whether a shulker item carries anything worth dropping in creative mode.
pub fn has_contents(components: &BTreeMap<String, Tag>) -> bool {
    components.contains_key(CONTAINER) || components.contains_key(CONTAINER_LOOT)
}

/// Writes the contents an item carries into the shulker box block entity at `pos`, and returns
/// the stacks that must not go in (nested shulker boxes, slots past the box) for the caller to
/// drop. An item with no shulker components leaves the block alone.
pub fn apply_item(
    source: &AnvilChunkSource,
    [x, y, z]: [i32; 3],
    item: &ItemStack,
) -> Vec<ItemStack> {
    use ferrite_item::component_ids as ids;
    let mut rejected = Vec::new();
    let mut slots = vec![ItemStack::empty(); SIZE];
    let container = item.components.get(ids::CONTAINER);
    match container {
        Some(Component::Container(stacks)) => {
            for (slot, stack) in stacks.iter().enumerate() {
                if stack.is_empty() {
                    continue;
                }
                if slot >= SIZE || ferrite_inventory::is_shulker_box(stack) {
                    rejected.push(stack.clone());
                } else {
                    slots[slot] = stack.clone();
                }
            }
        }
        Some(_) => tracing::warn!(
            x,
            y,
            z,
            "a shulker box item has a malformed container; its contents could not be placed"
        ),
        None => {}
    }
    let tag_of = |id| item.components.get(id).and_then(|c| c.to_nbt().ok());
    let (name, lock, loot) = (
        tag_of(ids::CUSTOM_NAME),
        tag_of(ids::LOCK),
        tag_of(ids::CONTAINER_LOOT),
    );
    if !matches!(container, Some(Component::Container(_)))
        && name.is_none()
        && lock.is_none()
        && loot.is_none()
    {
        return rejected;
    }
    let block = source
        .block(x, y, z)
        .and_then(|s| u16::try_from(s).ok())
        .and_then(ferrite_data::blocks::state)
        .map_or("minecraft:shulker_box", |(name, _)| name);
    super::block_container::store_slots(source, [x, y, z], block, &slots);
    let _ = source.update_block_entity(x, y, z, |entity| {
        let Some(entity) = entity.as_mut() else {
            return (false, ());
        };
        if let Some(name) = name {
            entity.data.insert("CustomName".into(), name);
        }
        if let Some(lock) = lock {
            entity.data.insert("lock".into(), lock);
        }
        if let Some(Tag::Compound(loot)) = loot {
            if let Some(table) = loot.get("loot_table") {
                entity.data.insert("LootTable".into(), table.clone());
            }
            if let Some(seed) = loot.get("seed") {
                entity.data.insert("LootTableSeed".into(), seed.clone());
            }
        }
        (true, ())
    });
    rejected
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stone(count: i32) -> ItemStack {
        ItemStack::from_name("stone")
            .unwrap()
            .copy_with_count(count)
    }

    fn entity(items: &[(i8, ItemStack)]) -> Compound {
        let mut list = Vec::new();
        for (slot, stack) in items {
            let mut entry = stack.to_nbt().unwrap();
            entry.insert("Slot".into(), Tag::Byte(*slot));
            list.push(entry);
        }
        let mut entity = Compound::new();
        entity.insert("Items".into(), Tag::List(List::Compound(list)));
        entity
    }

    #[test]
    fn items_name_lock_and_loot_become_components() {
        let mut data = entity(&[(0, stone(3)), (26, stone(1))]);
        data.insert("CustomName".into(), Tag::String("Kiste".into()));
        data.insert("lock".into(), Tag::String("key".into()));
        data.insert("LootTable".into(), Tag::String("minecraft:chests/x".into()));
        data.insert("LootTableSeed".into(), Tag::Long(99));
        let (components, unreadable) = collect_components(&data);
        assert_eq!(unreadable, Unreadable::default());
        assert_eq!(
            components.keys().map(String::as_str).collect::<Vec<_>>(),
            [CONTAINER, CONTAINER_LOOT, CUSTOM_NAME, LOCK]
        );
        let stack = item_with("minecraft:lime_shulker_box", &components).unwrap();
        assert_eq!(stack.name(), Some("minecraft:lime_shulker_box"));
        let Some(Component::Container(slots)) =
            stack.components.get(ferrite_item::component_ids::CONTAINER)
        else {
            panic!("a valid container becomes the typed component");
        };
        assert_eq!(slots.len(), 27);
        assert_eq!(slots[0], stone(3));
        assert_eq!(slots[26], stone(1));
    }

    #[test]
    fn an_empty_box_carries_no_container() {
        let (components, _) = collect_components(&entity(&[]));
        assert!(components.is_empty());
        assert!(!has_contents(&components));
    }

    #[test]
    fn unreadable_entries_are_counted_not_guessed() {
        let mut data = entity(&[(0, stone(1)), (0, stone(2)), (40, stone(1))]);
        if let Some(Tag::List(List::Compound(list))) = data.get_mut("Items") {
            list.push(Compound::new());
        }
        let (components, unreadable) = collect_components(&data);
        assert_eq!(
            unreadable.entries, 3,
            "a repeated slot, an out of range slot and a missing slot"
        );
        assert!(components.contains_key(CONTAINER));
    }

    #[test]
    fn every_colour_has_its_item() {
        for color in [
            "white",
            "orange",
            "magenta",
            "light_blue",
            "yellow",
            "lime",
            "pink",
            "gray",
            "light_gray",
            "cyan",
            "purple",
            "blue",
            "brown",
            "green",
            "red",
            "black",
        ] {
            let block = format!("minecraft:{color}_shulker_box");
            assert_eq!(
                item_with(&block, &BTreeMap::new()).unwrap().name(),
                Some(block.as_str())
            );
        }
        let plain = item_with("minecraft:shulker_box", &BTreeMap::new()).unwrap();
        assert_eq!(plain.name(), Some("minecraft:shulker_box"));
        assert!(item_with("minecraft:chest", &BTreeMap::new()).is_none());
    }

    async fn source() -> std::sync::Arc<AnvilChunkSource> {
        let source = AnvilChunkSource::new_memory();
        for _ in 0..2_000 {
            if let Some(result) = source.get(0, 0, 0) {
                result.expect("chunk load");
                return source;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("chunk never loaded");
    }

    fn place_box(source: &AnvilChunkSource, block: &str) {
        let state = ferrite_data::blocks::default_state(block).unwrap();
        source.set_block(4, 65, 4, u32::from(state)).unwrap();
    }

    fn carried(block: &str, slots: Vec<ItemStack>) -> ItemStack {
        let mut item = ItemStack::from_name(block).unwrap();
        item.components.insert(
            ferrite_item::component_ids::CONTAINER,
            Component::Container(slots),
        );
        item
    }

    fn placed_items(source: &AnvilChunkSource) -> Vec<ItemStack> {
        super::super::block_container::load_items(source.block_entity(4, 65, 4).as_ref())
    }

    #[tokio::test]
    async fn every_colour_restores_its_contents_when_placed() {
        let source = source().await;
        for block in [
            "shulker_box",
            "white_shulker_box",
            "orange_shulker_box",
            "magenta_shulker_box",
            "light_blue_shulker_box",
            "yellow_shulker_box",
            "lime_shulker_box",
            "pink_shulker_box",
            "gray_shulker_box",
            "light_gray_shulker_box",
            "cyan_shulker_box",
            "purple_shulker_box",
            "blue_shulker_box",
            "brown_shulker_box",
            "green_shulker_box",
            "red_shulker_box",
            "black_shulker_box",
        ] {
            let name = format!("minecraft:{block}");
            place_box(&source, &name);
            let mut slots = vec![ItemStack::empty(); 27];
            slots[0] = stone(64);
            slots[13] = stone(7);
            slots[26] = stone(1);
            let rejected = apply_item(&source, [4, 65, 4], &carried(block, slots.clone()));
            assert!(rejected.is_empty(), "{block}");
            let placed = placed_items(&source);
            assert_eq!(placed[0], stone(64), "{block}");
            assert_eq!(placed[13], stone(7), "{block}");
            assert_eq!(placed[26], stone(1), "{block}");
            assert_eq!(
                placed.iter().filter(|s| !s.is_empty()).count(),
                3,
                "{block}"
            );
        }
    }

    #[tokio::test]
    async fn a_full_box_and_an_empty_box_both_place() {
        let source = source().await;
        place_box(&source, "minecraft:blue_shulker_box");
        let full: Vec<_> = (0..27).map(|i| stone(i % 64 + 1)).collect();
        assert!(
            apply_item(
                &source,
                [4, 65, 4],
                &carried("blue_shulker_box", full.clone())
            )
            .is_empty()
        );
        let placed = placed_items(&source);
        assert_eq!(placed.iter().filter(|s| !s.is_empty()).count(), 27);
        assert_eq!(placed[26], full[26]);
        // An item with no shulker components leaves the block alone: no contents, no entity.
        place_box(&source, "minecraft:red_shulker_box");
        let plain = ItemStack::from_name("red_shulker_box").unwrap();
        assert!(apply_item(&source, [5, 65, 5], &plain).is_empty());
    }

    #[tokio::test]
    async fn nested_shulker_boxes_are_refused_and_handed_back() {
        let source = source().await;
        place_box(&source, "minecraft:lime_shulker_box");
        let inner = carried("pink_shulker_box", vec![stone(3)]);
        let rejected = apply_item(
            &source,
            [4, 65, 4],
            &carried("lime_shulker_box", vec![stone(5), inner.clone(), stone(2)]),
        );
        assert_eq!(
            rejected,
            [inner],
            "the inner box comes back whole, with contents"
        );
        let placed = placed_items(&source);
        assert_eq!(placed[0], stone(5));
        assert!(placed[1].is_empty(), "the nested box did not go in");
        assert_eq!(placed[2], stone(2));
    }

    #[tokio::test]
    async fn slots_past_the_box_are_handed_back_not_deleted() {
        let source = source().await;
        place_box(&source, "minecraft:cyan_shulker_box");
        let mut slots = vec![ItemStack::empty(); 30];
        slots[1] = stone(4);
        slots[28] = stone(9);
        let rejected = apply_item(&source, [4, 65, 4], &carried("cyan_shulker_box", slots));
        assert_eq!(rejected, [stone(9)]);
        assert_eq!(placed_items(&source)[1], stone(4));
    }

    #[tokio::test]
    async fn a_malformed_container_does_not_place_or_panic() {
        let source = source().await;
        place_box(&source, "minecraft:black_shulker_box");
        let mut item = ItemStack::from_name("black_shulker_box").unwrap();
        item.components.insert(
            ferrite_item::component_ids::CONTAINER,
            Component::Opaque(Tag::String("garbage".into())),
        );
        assert!(apply_item(&source, [4, 65, 4], &item).is_empty());
        assert!(placed_items(&source).iter().all(ItemStack::is_empty));
    }

    #[tokio::test]
    async fn name_and_lock_travel_with_the_item() {
        let source = source().await;
        place_box(&source, "minecraft:green_shulker_box");
        let mut item = ItemStack::from_name("green_shulker_box").unwrap();
        item.components.insert(
            ferrite_item::component_ids::CUSTOM_NAME,
            Component::CustomName(ferrite_text::Component::text("Kiste ✨")),
        );
        assert!(apply_item(&source, [4, 65, 4], &item).is_empty());
        let entity = source
            .block_entity(4, 65, 4)
            .expect("a name needs the entity");
        assert!(entity.data.get("CustomName").is_some());
    }
}
