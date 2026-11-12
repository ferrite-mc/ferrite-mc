//! Blocks with an inventory: chests, trapped chests and barrels (vanilla `ChestBlock`,
//! `BarrelBlock` and their `RandomizableContainerBlockEntity` storage).
//!
//! The items live in the block entity's `Items` list in the chunk, exactly like vanilla saves
//! them, so every viewer (in any region) works on the same contents and they are written back
//! with the chunk.

use ferrite_item::ItemStack;
use ferrite_nbt::{Compound, List, Tag};
use ferrite_world::BlockEntity;

use super::anvil::AnvilChunkSource;

/// Slots of one chest or barrel.
pub const SIZE: usize = 27;

/// An inventory block as a menu sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct Opened {
    /// The block entities backing the menu, top half first; two for a double chest.
    pub parts: Vec<[i32; 3]>,
    /// The block name each part must still be for the menu to stay open.
    pub block: &'static str,
    /// Chest rows (3, or 6 for a double chest).
    pub rows: u8,
    /// Translation key of the screen title.
    pub title: &'static str,
}

fn block_at(
    source: &AnvilChunkSource,
    [x, y, z]: [i32; 3],
) -> Option<(&'static str, &'static [(&'static str, &'static str)])> {
    let state = u16::try_from(source.block(x, y, z)?).ok()?;
    ferrite_data::blocks::state(state)
}

fn prop<'a>(props: &'a [(&'static str, &'static str)], name: &str) -> Option<&'a str> {
    props
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| *value)
}

/// Whether a block name is a shulker box of any colour (vanilla `ShulkerBoxBlock`).
pub fn is_shulker_box(name: &str) -> bool {
    name.strip_prefix("minecraft:")
        .unwrap_or(name)
        .ends_with("shulker_box")
}

/// The block entity type of an inventory block: every colour of shulker box shares
/// `minecraft:shulker_box` (vanilla `BlockEntityType.SHULKER_BOX`), the rest use their block name.
fn entity_type(block: &str) -> &str {
    if is_shulker_box(block) {
        "minecraft:shulker_box"
    } else {
        block
    }
}

/// Whether `stack` may be put into the inventory block `target` by a hopper or dropper: shulker
/// boxes never nest (vanilla `ShulkerBoxBlockEntity.canPlaceItemThroughFace`).
pub fn accepts(target: &str, stack: &ItemStack) -> bool {
    !(is_shulker_box(target) && ferrite_inventory::is_shulker_box(stack))
}

/// The slot count of every block entity inventory a hopper can move items between (vanilla
/// `WorldlyContainer` implementations), or `None` for other blocks.
pub fn inventory_slots(name: &str) -> Option<usize> {
    if is_shulker_box(name) {
        return Some(ferrite_inventory::SHULKER_BOX_SLOTS);
    }
    Some(match name.strip_prefix("minecraft:").unwrap_or(name) {
        "chest" | "trapped_chest" | "barrel" => SIZE,
        "hopper" => super::hopper::SIZE,
        "dropper" | "dispenser" => super::dispenser::SIZE,
        "furnace" | "blast_furnace" | "smoker" => super::furnace::SLOTS,
        _ => return None,
    })
}

/// The items of any block entity inventory with `Items` in its NBT, sized to its slot count.
pub fn load_slots(source: &AnvilChunkSource, [x, y, z]: [i32; 3], size: usize) -> Vec<ItemStack> {
    let mut items = load_items(source.block_entity(x, y, z).as_ref());
    items.resize(size, ItemStack::empty());
    items
}

/// Writes the `Items` list of any block entity inventory, creating the block entity when
/// needed and keeping the rest of its NBT (timers, custom names) untouched.
pub fn store_slots(
    source: &AnvilChunkSource,
    [x, y, z]: [i32; 3],
    block: &str,
    items: &[ItemStack],
) {
    let block = entity_type(block);
    let Some(type_id) = ferrite_data::registries::id("minecraft:block_entity_type", block)
        .and_then(|id| u32::try_from(id).ok())
    else {
        return;
    };
    let result = source.update_block_entity(x, y, z, |entity| {
        let entity = entity.get_or_insert_with(|| {
            let mut data = Compound::new();
            data.insert("id".into(), Tag::String(block.into()));
            data.insert("x".into(), Tag::Int(x));
            data.insert("y".into(), Tag::Int(y));
            data.insert("z".into(), Tag::Int(z));
            BlockEntity { type_id, data }
        });
        let unchanged =
            entity.data.get("Items").is_some() && load_items(Some(entity))[..items.len()] == *items;
        if unchanged {
            return (false, ());
        }
        // Vanilla saves an empty `Items` list too, so an emptied container keeps its
        // block entity shape across a reload.
        entity.data.insert("Items".into(), save_items(items));
        (true, ())
    });
    if let Err(error) = result {
        tracing::warn!(x, y, z, %error, "could not store container items");
    }
}

/// One step towards a horizontal direction.
fn step(direction: &str) -> Option<[i32; 3]> {
    Some(match direction {
        "north" => [0, 0, -1],
        "south" => [0, 0, 1],
        "west" => [-1, 0, 0],
        "east" => [1, 0, 0],
        _ => return None,
    })
}

/// Vanilla `ChestBlock.getConnectedDirection`: a left half connects clockwise of its facing,
/// a right half counter-clockwise.
fn connected_direction(facing: &str, kind: &str) -> Option<&'static str> {
    let clockwise = |d: &str| match d {
        "north" => Some("east"),
        "east" => Some("south"),
        "south" => Some("west"),
        "west" => Some("north"),
        _ => None,
    };
    let counter = |d: &str| match d {
        "north" => Some("west"),
        "west" => Some("south"),
        "south" => Some("east"),
        "east" => Some("north"),
        _ => None,
    };
    match kind {
        "left" => clockwise(facing),
        "right" => counter(facing),
        _ => None,
    }
}

/// The inventory block at a position, if it is one.
///
/// A chest whose `type` is `left` or `right` joins the neighbour it points at when that is the
/// same block facing the same way with the opposite type (vanilla `ChestBlock.combine`); the
/// `right` half is vanilla's `FIRST` and holds the top three rows.
pub fn at(source: &AnvilChunkSource, pos: [i32; 3]) -> Option<Opened> {
    let (name, props) = block_at(source, pos)?;
    match name {
        _ if is_shulker_box(name) => Some(Opened {
            parts: vec![pos],
            block: name,
            rows: 3,
            title: "container.shulkerBox",
        }),
        "minecraft:barrel" => Some(Opened {
            parts: vec![pos],
            block: name,
            rows: 3,
            title: "container.barrel",
        }),
        "minecraft:chest" | "minecraft:trapped_chest" => {
            let single = Opened {
                parts: vec![pos],
                block: name,
                rows: 3,
                title: "container.chest",
            };
            let (Some(kind), Some(facing)) = (prop(props, "type"), prop(props, "facing")) else {
                return Some(single);
            };
            let Some(offset) = connected_direction(facing, kind).and_then(step) else {
                return Some(single);
            };
            let other = [pos[0] + offset[0], pos[1], pos[2] + offset[2]];
            let opposite = if kind == "left" { "right" } else { "left" };
            match block_at(source, other) {
                Some((other_name, other_props))
                    if other_name == name
                        && prop(other_props, "facing") == Some(facing)
                        && prop(other_props, "type") == Some(opposite) =>
                {
                    let parts = if kind == "right" {
                        vec![pos, other]
                    } else {
                        vec![other, pos]
                    };
                    Some(Opened {
                        parts,
                        block: name,
                        rows: 6,
                        title: "container.chestDouble",
                    })
                }
                _ => Some(single),
            }
        }
        _ => None,
    }
}

/// Whether every part is still the same inventory block.
pub fn still_there(source: &AnvilChunkSource, opened: &Opened) -> bool {
    opened
        .parts
        .iter()
        .all(|&pos| block_at(source, pos).is_some_and(|(name, _)| name == opened.block))
}

/// A stack the client can be sent: components without a network form (kept as raw NBT from a
/// world save) are dropped with a warning instead of failing the packet.
fn network_safe(stack: ItemStack) -> ItemStack {
    if stack.encode_network().is_ok() {
        return stack;
    }
    let mut safe = stack.copy_with_count(stack.count);
    safe.components = ferrite_item::ComponentPatch::new();
    for (id, component) in stack.components.added() {
        let mut probe = safe.clone();
        probe.components.insert(id, component.clone());
        if probe.encode_network().is_ok() {
            safe = probe;
        } else {
            tracing::warn!(
                item = stack.name(),
                component = ferrite_item::component_name(id),
                "dropping a stored item component that cannot be sent to clients"
            );
        }
    }
    for id in stack.components.removed() {
        safe.components.remove(id);
    }
    safe
}

/// Vanilla `ContainerHelper.loadAllItems`: the `Items` list of a block entity.
fn load_items(entity: Option<&BlockEntity>) -> Vec<ItemStack> {
    let mut items = vec![ItemStack::empty(); SIZE];
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
            Ok(stack) => *target = network_safe(stack),
            Err(error) => tracing::warn!(%error, "ignoring an unreadable container item"),
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

/// The items of every part, top half first.
pub fn load(source: &AnvilChunkSource, opened: &Opened) -> Vec<ItemStack> {
    opened
        .parts
        .iter()
        .flat_map(|&[x, y, z]| load_items(source.block_entity(x, y, z).as_ref()))
        .collect()
}

/// Writes items back, creating the block entities when needed. Only changed parts mark their
/// chunk modified.
pub fn store(source: &AnvilChunkSource, opened: &Opened, items: &[ItemStack]) {
    let entity_name = entity_type(opened.block);
    let Some(type_id) = ferrite_data::registries::id("minecraft:block_entity_type", entity_name)
        .and_then(|id| u32::try_from(id).ok())
    else {
        return;
    };
    for (part, chunk) in opened.parts.iter().zip(items.chunks(SIZE)) {
        let [x, y, z] = *part;
        let result = source.update_block_entity(x, y, z, |entity| {
            if load_items(entity.as_ref()) == chunk {
                return (false, ());
            }
            let entity = entity.get_or_insert_with(|| {
                let mut data = Compound::new();
                data.insert("id".into(), Tag::String(entity_name.into()));
                data.insert("x".into(), Tag::Int(x));
                data.insert("y".into(), Tag::Int(y));
                data.insert("z".into(), Tag::Int(z));
                BlockEntity { type_id, data }
            });
            entity.data.insert("Items".into(), save_items(chunk));
            (true, ())
        });
        if let Err(error) = result {
            tracing::warn!(x, y, z, %error, "could not store container items");
        }
    }
}

/// Removes the block entity of a broken inventory block and returns its items (vanilla
/// `Containers.dropContentsOnDestroy`). Other blocks keep their block entity.
pub fn take_contents(
    source: &AnvilChunkSource,
    [x, y, z]: [i32; 3],
    old_state: u32,
) -> Vec<ItemStack> {
    let Some((name, _)) = u16::try_from(old_state)
        .ok()
        .and_then(ferrite_data::blocks::state)
    else {
        return Vec::new();
    };
    if inventory_slots(name).is_none() {
        return Vec::new();
    }
    source
        .update_block_entity(x, y, z, |entity| match entity.take() {
            Some(removed) => (true, load_items(Some(&removed))),
            None => (false, Vec::new()),
        })
        .unwrap_or_default()
        .into_iter()
        .filter(|stack| !stack.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(name: &str, props: &[(&str, &str)]) -> u32 {
        u32::from(ferrite_data::blocks::state_id(name, props).unwrap())
    }

    async fn loaded() -> std::sync::Arc<AnvilChunkSource> {
        let source = AnvilChunkSource::new_memory();
        // Generate the chunk so blocks can be set.
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        source
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn chests_pair_up_with_the_right_half_first() {
        let source = loaded().await;
        let left = state("chest", &[("type", "left"), ("facing", "north")]);
        let right = state("chest", &[("type", "right"), ("facing", "north")]);
        // Facing north, a left half connects to the east.
        source.set_block(1, 70, 1, left).unwrap();
        source.set_block(2, 70, 1, right).unwrap();
        let opened = at(&source, [1, 70, 1]).unwrap();
        assert_eq!(opened.parts, vec![[2, 70, 1], [1, 70, 1]]);
        assert_eq!((opened.rows, opened.title), (6, "container.chestDouble"));
        assert_eq!(at(&source, [2, 70, 1]).unwrap().parts, opened.parts);
        // A half without its partner is a single chest.
        source.set_block(2, 70, 1, 0).unwrap();
        assert_eq!(at(&source, [1, 70, 1]).unwrap().rows, 3);
        source.set_block(3, 70, 3, state("barrel", &[])).unwrap();
        assert_eq!(at(&source, [3, 70, 3]).unwrap().title, "container.barrel");
        assert_eq!(at(&source, [4, 70, 3]), None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn items_round_trip_through_the_block_entity_and_spill_on_break() {
        let source = loaded().await;
        let chest = state("chest", &[]);
        source.set_block(5, 70, 5, chest).unwrap();
        let opened = at(&source, [5, 70, 5]).unwrap();
        assert_eq!(load(&source, &opened), vec![ItemStack::empty(); SIZE]);
        let mut items = vec![ItemStack::empty(); SIZE];
        items[0] = ItemStack::from_name("stone").unwrap().copy_with_count(40);
        items[26] = ItemStack::from_name("iron_sword").unwrap();
        store(&source, &opened, &items);
        assert_eq!(load(&source, &opened), items);
        let entity = source.block_entity(5, 70, 5).unwrap();
        assert_eq!(
            entity.data.get("id"),
            Some(&Tag::String("minecraft:chest".into()))
        );
        let spilled = take_contents(&source, [5, 70, 5], chest);
        assert_eq!(spilled, vec![items[0].clone(), items[26].clone()]);
        assert!(source.block_entity(5, 70, 5).is_none());
        // Breaking any other block leaves block entities alone.
        assert!(take_contents(&source, [5, 70, 5], state("stone", &[])).is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shulker_boxes_open_as_one_27_slot_inventory_that_spills_on_break() {
        let source = loaded().await;
        assert!(is_shulker_box("minecraft:shulker_box"));
        assert!(is_shulker_box("minecraft:lime_shulker_box"));
        assert!(!is_shulker_box("minecraft:chest"));
        for (index, name) in ["shulker_box", "lime_shulker_box"].into_iter().enumerate() {
            let pos = [6 + index as i32, 70, 6];
            let block = state(name, &[]);
            source.set_block(pos[0], pos[1], pos[2], block).unwrap();
            let opened = at(&source, pos).unwrap();
            assert_eq!(
                (opened.parts.clone(), opened.rows, opened.title),
                (vec![pos], 3, "container.shulkerBox")
            );
            assert_eq!(inventory_slots(name), Some(27));
            let mut items = vec![ItemStack::empty(); 27];
            items[3] = ItemStack::from_name("diamond").unwrap().copy_with_count(7);
            items[26] = ItemStack::from_name("stone").unwrap().copy_with_count(64);
            store(&source, &opened, &items);
            assert_eq!(load(&source, &opened), items);
            let entity = source.block_entity(pos[0], pos[1], pos[2]).unwrap();
            assert_eq!(
                entity.data.get("id"),
                Some(&Tag::String("minecraft:shulker_box".into()))
            );
            assert!(still_there(&source, &opened));
            assert_eq!(
                take_contents(&source, pos, block),
                vec![items[3].clone(), items[26].clone()]
            );
            assert!(source.block_entity(pos[0], pos[1], pos[2]).is_none());
        }
    }

    #[test]
    fn hoppers_and_droppers_may_not_nest_shulker_boxes() {
        let boxed = ItemStack::from_name("red_shulker_box").unwrap();
        let stone = ItemStack::from_name("stone").unwrap();
        assert!(!accepts("minecraft:shulker_box", &boxed));
        assert!(!accepts("minecraft:cyan_shulker_box", &boxed));
        assert!(accepts("minecraft:shulker_box", &stone));
        assert!(accepts("minecraft:chest", &boxed));
    }
}
