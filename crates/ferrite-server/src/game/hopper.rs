//! Hoppers (vanilla `HopperBlockEntity`).
//!
//! A hopper is a five-slot block entity that pulls one item per eight game ticks from the
//! container above it, pushes one item into the container it faces (or ejects it into the
//! world), and sucks item entities lying on top of it into its own slots. A hopper that a
//! redstone signal disables (`enabled=false`) does nothing while it is powered.
//!
//! The items live in the block entity's `Items` list and the transfer cooldown in
//! `TransferCooldown`, exactly like vanilla, so the contents are saved with the chunk and
//! shared with every menu that views the hopper.

use ferrite_data::blocks;
use ferrite_entity::{EntityStore, Flags, Rng};
use ferrite_item::ItemStack;
use ferrite_math::{AABB, Vec3};
use ferrite_nbt::{Compound, Tag};
use ferrite_world::BlockEntity;
use std::collections::HashMap;

use super::anvil::AnvilChunkSource;
use super::block_container;

/// Slots of one hopper.
pub const SIZE: usize = 5;
/// Vanilla `HopperBlockEntity.MOVE_ITEM_SPEED`: the transfer cooldown after a move.
pub const COOLDOWN: i32 = 8;

/// The block entity state of a hopper.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HopperData {
    pub items: Vec<ItemStack>,
    pub cooldown: i32,
}

/// Which side of a container an item enters, from the hopper's point of view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    Top,
    Side,
    Bottom,
}

/// What a push did with a hopper's item.
#[derive(Clone, Debug, PartialEq)]
pub enum Push {
    /// The item went into a container.
    Moved,
    /// The facing block is air or replaceable: the item must be dropped there.
    Ejected(ItemStack),
    /// The facing block is solid: the hopper keeps its item.
    Blocked,
}

fn name_at(source: &AnvilChunkSource, [x, y, z]: [i32; 3]) -> Option<&'static str> {
    source
        .block(x, y, z)
        .and_then(|state| u16::try_from(state).ok())
        .and_then(blocks::state)
        .map(|(name, _)| name)
}

fn property_at<'a>(source: &AnvilChunkSource, [x, y, z]: [i32; 3], key: &str) -> Option<&'a str> {
    let state = u16::try_from(source.block(x, y, z)?).ok()?;
    let (_, props) = blocks::state(state)?;
    props
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, value)| *value)
}

/// The block a hopper pushes towards (vanilla `direction`), as an offset.
pub fn facing(source: &AnvilChunkSource, [x, y, z]: [i32; 3]) -> Option<[i32; 3]> {
    Some(match property_at(source, [x, y, z], "facing")? {
        "down" => [0, -1, 0],
        "north" => [0, 0, -1],
        "south" => [0, 0, 1],
        "west" => [-1, 0, 0],
        "east" => [1, 0, 0],
        _ => return None,
    })
}

/// Vanilla `HopperBlock.ENABLED`: redstone power locks the hopper.
pub fn enabled(source: &AnvilChunkSource, pos: [i32; 3]) -> bool {
    property_at(source, pos, "enabled") != Some("false")
}

fn number(entity: Option<&BlockEntity>, key: &str) -> i32 {
    match entity.and_then(|entity| entity.data.get(key)) {
        Some(Tag::Short(value)) => i32::from(*value),
        Some(Tag::Int(value)) => *value,
        _ => 0,
    }
}

/// A hopper's items and cooldown from its block entity.
pub fn load(source: &AnvilChunkSource, [x, y, z]: [i32; 3]) -> HopperData {
    let entity = source.block_entity(x, y, z);
    HopperData {
        items: block_container::load_slots(source, [x, y, z], SIZE),
        cooldown: number(entity.as_ref(), "TransferCooldown"),
    }
}

/// Writes a hopper's items and cooldown back, keeping unrelated NBT.
pub fn store(source: &AnvilChunkSource, [x, y, z]: [i32; 3], data: &HopperData) {
    block_container::store_slots(source, [x, y, z], "minecraft:hopper", &data.items);
    let Some(type_id) =
        ferrite_data::registries::id("minecraft:block_entity_type", "minecraft:hopper")
            .and_then(|id| u32::try_from(id).ok())
    else {
        return;
    };
    let result = source.update_block_entity(x, y, z, |entity| {
        if entity.is_none() && data.cooldown == 0 {
            return (false, ());
        }
        let entity = entity.get_or_insert_with(|| {
            let mut nbt = Compound::new();
            nbt.insert("id".into(), Tag::String("minecraft:hopper".into()));
            nbt.insert("x".into(), Tag::Int(x));
            nbt.insert("y".into(), Tag::Int(y));
            nbt.insert("z".into(), Tag::Int(z));
            BlockEntity { type_id, data: nbt }
        });
        if number(Some(entity), "TransferCooldown") == data.cooldown {
            return (false, ());
        }
        entity
            .data
            .insert("TransferCooldown".into(), Tag::Int(data.cooldown));
        (true, ())
    });
    if let Err(error) = result {
        tracing::warn!(x, y, z, %error, "could not store hopper state");
    }
}

/// The slots a hopper may pull from, given the face of the container it reaches.
fn extract_slots(name: &str, size: usize) -> Vec<usize> {
    match name.strip_prefix("minecraft:").unwrap_or(name) {
        // Vanilla `AbstractFurnaceBlockEntity.getSlotsForFace(DOWN)`: the result slot.
        "furnace" | "blast_furnace" | "smoker" => vec![2],
        // Vanilla `BrewingStandBlockEntity.getSlotsForFace(DOWN)`: the bottle slots.
        "brewing_stand" => vec![0, 1, 2],
        _ => (0..size).collect(),
    }
}

/// The slots a hopper may insert into, given the face of the container it enters.
fn insert_slots(name: &str, face: Face, size: usize) -> Vec<usize> {
    match name.strip_prefix("minecraft:").unwrap_or(name) {
        "furnace" | "blast_furnace" | "smoker" => match face {
            Face::Top => vec![0],
            Face::Side => vec![1],
            Face::Bottom => Vec::new(),
        },
        "brewing_stand" => match face {
            Face::Top => vec![3],
            Face::Side => vec![0, 1, 2, 4],
            Face::Bottom => Vec::new(),
        },
        _ => (0..size).collect(),
    }
}

fn same_stack(a: &ItemStack, b: &ItemStack) -> bool {
    a.item == b.item && a.is_same_item_same_components(b)
}

/// Vanilla `Container.addItem`/`HopperBlockEntity.addItem`: fills matching stacks first, then
/// empty slots. Returns the part of `stack` that did not fit.
pub fn insert(items: &mut [ItemStack], mut stack: ItemStack) -> Option<ItemStack> {
    if stack.is_empty() {
        return None;
    }
    for existing in items.iter_mut() {
        if stack.is_empty() {
            break;
        }
        if existing.is_empty() || !same_stack(existing, &stack) {
            continue;
        }
        let room = existing.max_stack_size() - existing.count;
        let moved = room.min(stack.count);
        if moved > 0 {
            existing.count += moved;
            stack.count -= moved;
        }
    }
    for empty in items.iter_mut() {
        if stack.is_empty() {
            break;
        }
        if !empty.is_empty() {
            continue;
        }
        let moved = stack.count.min(stack.max_stack_size());
        let mut placed = stack.clone();
        placed.count = moved;
        *empty = placed;
        stack.count -= moved;
    }
    (!stack.is_empty()).then_some(stack)
}

/// Vanilla `HopperBlockEntity.addItem` restricted to a container's allowed face slots.
fn insert_into_slots(
    items: &mut [ItemStack],
    allowed: &[usize],
    mut stack: ItemStack,
) -> Option<ItemStack> {
    if stack.is_empty() {
        return None;
    }
    for &slot in allowed {
        if stack.is_empty() {
            break;
        }
        let Some(existing) = items.get_mut(slot) else {
            continue;
        };
        if existing.is_empty() || !same_stack(existing, &stack) {
            continue;
        }
        let room = existing.max_stack_size() - existing.count;
        let moved = room.min(stack.count);
        if moved > 0 {
            existing.count += moved;
            stack.count -= moved;
        }
    }
    for &slot in allowed {
        if stack.is_empty() {
            break;
        }
        let Some(empty) = items.get_mut(slot) else {
            continue;
        };
        if !empty.is_empty() {
            continue;
        }
        let moved = stack.count.min(stack.max_stack_size());
        let mut placed = stack.clone();
        placed.count = moved;
        *empty = placed;
        stack.count -= moved;
    }
    (!stack.is_empty()).then_some(stack)
}

/// Vanilla `HopperBlockEntity.tryTakeInItemFromSlot`: one item from the container above.
pub fn pull(source: &AnvilChunkSource, [x, y, z]: [i32; 3], items: &mut [ItemStack]) -> bool {
    let above = [x, y + 1, z];
    let Some(name) = name_at(source, above) else {
        return false;
    };
    let Some(size) = block_container::inventory_slots(name) else {
        return false;
    };
    let allowed = extract_slots(name, size);
    let mut source_items = block_container::load_slots(source, above, size);
    let Some(index) = allowed.iter().copied().find(|&index| {
        source_items
            .get(index)
            .is_some_and(|stack| !stack.is_empty())
    }) else {
        return false;
    };
    let mut one = source_items[index].clone();
    one.count = 1;
    if insert(items, one).is_some() {
        return false;
    }
    source_items[index].count -= 1;
    if source_items[index].count <= 0 {
        source_items[index] = ItemStack::empty();
    }
    block_container::store_slots(source, above, name, &source_items);
    true
}

/// Vanilla `HopperBlockEntity.tryMoveItems`: one item into the facing container, or out into
/// the world when the facing block is empty.
pub fn push(source: &AnvilChunkSource, [x, y, z]: [i32; 3], items: &mut [ItemStack]) -> Push {
    let Some(offset) = facing(source, [x, y, z]) else {
        return Push::Blocked;
    };
    let Some(index) = items.iter().position(|stack| !stack.is_empty()) else {
        return Push::Blocked;
    };
    let target = [x + offset[0], y + offset[1], z + offset[2]];
    if let Some(name) = name_at(source, target)
        && let Some(size) = block_container::inventory_slots(name)
    {
        let face = match offset {
            [0, -1, 0] => Face::Top,
            [0, 1, 0] => Face::Bottom,
            _ => Face::Side,
        };
        let allowed = insert_slots(name, face, size);
        let mut target_items = block_container::load_slots(source, target, size);
        let mut one = items[index].clone();
        one.count = 1;
        if block_container::accepts(name, &one)
            && insert_into_slots(&mut target_items, &allowed, one).is_none()
        {
            items[index].count -= 1;
            if items[index].count <= 0 {
                items[index] = ItemStack::empty();
            }
            block_container::store_slots(source, target, name, &target_items);
            return Push::Moved;
        }
    }
    if name_at(source, target).is_none_or(|name| name == "minecraft:air") {
        let stack = items[index].clone();
        items[index] = ItemStack::empty();
        return Push::Ejected(stack);
    }
    Push::Blocked
}

/// The comparator strength of a hopper's contents (vanilla
/// `AbstractContainerMenu.getRedstoneSignalFromContainer`): one per filled slot scaled to 15.
pub fn comparator_signal(items: &[ItemStack]) -> u8 {
    if items.is_empty() {
        return 0;
    }
    let filled = items.iter().filter(|stack| !stack.is_empty()).count();
    if filled == 0 {
        return 0;
    }
    let strength = 1 + 14 * filled / items.len();
    u8::try_from(strength).unwrap_or(15).min(15)
}

fn plain(stack: &ItemStack) -> ferrite_protocol::packets::play::ItemStack {
    ferrite_protocol::packets::play::ItemStack {
        count: stack.count,
        item_id: i32::from(stack.item),
    }
}

/// Vanilla `HopperBlockEntity.suckInItems`: moves one item from every item entity resting on
/// the hopper's top into its slots.
pub fn suck_up(
    store: &mut EntityStore,
    [x, y, z]: [i32; 3],
    items: &mut [ItemStack],
    rng: &mut Rng,
) -> bool {
    let area = AABB::new(
        f64::from(x),
        f64::from(y) + 1.0,
        f64::from(z),
        f64::from(x) + 1.0,
        f64::from(y) + 2.0,
        f64::from(z) + 1.0,
    );
    let keys = store.query_vec(area);
    let mut moved = false;
    for key in keys {
        if store
            .flags(key)
            .is_some_and(|flags| flags.has(Flags::REMOVED))
        {
            continue;
        }
        let plain_stack = match store.data(key) {
            Some(data) if data.item.is_some() => ferrite_entity::item::stack(data),
            _ => continue,
        };
        let Ok(item) = u16::try_from(plain_stack.item_id) else {
            continue;
        };
        let stack = ItemStack {
            item,
            count: 1,
            components: ferrite_item::ComponentPatch::new(),
        };
        if insert(items, stack).is_some() {
            continue;
        }
        moved = true;
        let left = {
            let data = store.data_mut(key).unwrap();
            let mut left = ferrite_entity::item::stack(data);
            left.count -= 1;
            if left.count <= 0 {
                None
            } else {
                ferrite_entity::item::set_stack(data, left);
                Some(())
            }
        };
        if left.is_none() {
            if let Some(flags) = store.flags_mut(key) {
                flags.set(Flags::REMOVED, true);
            }
            continue;
        }
        // A little jitter so the item visibly slides into the hopper.
        store.set_vel(
            key,
            Vec3::new(rng.range(-0.02, 0.02), -0.03, rng.range(-0.02, 0.02)),
        );
    }
    moved
}

/// The hoppers a region ticks, with their transfer cooldown.
#[derive(Debug, Default)]
pub struct Hoppers {
    known: HashMap<[i32; 3], i32>,
}

impl Hoppers {
    /// Registers the hopper at a position if there is one, drops it otherwise.
    pub fn observe(&mut self, source: &AnvilChunkSource, pos: [i32; 3]) {
        match name_at(source, pos) {
            Some("minecraft:hopper") => {
                self.known.entry(pos).or_default();
            }
            Some(_) => {
                self.known.remove(&pos);
            }
            // An unloaded chunk tells us nothing; keep what we had.
            None => {}
        }
    }

    /// Notices hoppers placed next to a changed block, so a hopper that was just built (or
    /// uncovered) starts ticking without a chunk scan.
    pub fn blocks_changed(&mut self, source: &AnvilChunkSource, positions: &[[i32; 3]]) {
        for &[x, y, z] in positions {
            self.observe(source, [x, y, z]);
            for offset in [
                [0, 1, 0],
                [0, -1, 0],
                [1, 0, 0],
                [-1, 0, 0],
                [0, 0, 1],
                [0, 0, -1],
            ] {
                self.observe(source, [x + offset[0], y + offset[1], z + offset[2]]);
            }
        }
    }

    /// Ticks every known hopper once (vanilla `HopperBlockEntity.pushItems`).
    pub fn tick(&mut self, source: &AnvilChunkSource, entities: &mut EntityStore, rng: &mut Rng) {
        let mut positions: Vec<[i32; 3]> = self.known.keys().copied().collect();
        positions.sort_unstable();
        for pos in positions {
            if name_at(source, pos) != Some("minecraft:hopper") {
                self.known.remove(&pos);
                continue;
            }
            let mut data = load(source, pos);
            if data.items.len() != SIZE {
                data.items.resize(SIZE, ItemStack::empty());
            }
            let mut moved = false;
            if data.cooldown > 0 {
                data.cooldown -= 1;
            } else if enabled(source, pos) {
                moved |= pull(source, pos, &mut data.items);
                match push(source, pos, &mut data.items) {
                    Push::Moved => moved = true,
                    Push::Ejected(stack) => {
                        let offset = facing(source, pos).unwrap_or([0, 0, 0]);
                        let [dx, dy, dz] = offset.map(f64::from);
                        let center = Vec3::new(
                            f64::from(pos[0]) + 0.5 + dx * 0.6,
                            f64::from(pos[1]) + 0.5 + dy * 0.6,
                            f64::from(pos[2]) + 0.5 + dz * 0.6,
                        );
                        let velocity = Vec3::new(dx * 0.12, dy * 0.12, dz * 0.12);
                        entities.insert(ferrite_entity::item::new_item(
                            center,
                            velocity,
                            plain(&stack),
                            rng,
                        ));
                        moved = true;
                    }
                    Push::Blocked => {}
                }
                moved |= suck_up(entities, pos, &mut data.items, rng);
                if moved {
                    data.cooldown = COOLDOWN;
                }
            }
            store(source, pos, &data);
            self.known.insert(pos, data.cooldown);
        }
    }

    /// Moves hoppers along with the half of a split region that owns them.
    pub fn split(&mut self, keep: impl Fn(i32, i32) -> bool) -> Self {
        let mut out = Self::default();
        let moving: Vec<_> = self
            .known
            .keys()
            .copied()
            .filter(|pos| keep(pos[0], pos[2]))
            .collect();
        for pos in moving {
            if let Some(cooldown) = self.known.remove(&pos) {
                out.known.insert(pos, cooldown);
            }
        }
        out
    }

    pub fn merge(&mut self, other: Self) {
        self.known.extend(other.known);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn loaded() -> std::sync::Arc<AnvilChunkSource> {
        let source = AnvilChunkSource::new_memory();
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        source
    }

    fn state(name: &str, props: &[(&str, &str)]) -> u32 {
        u32::from(blocks::state_id(name, props).unwrap())
    }

    fn hopper_state(facing: &str, enabled: &str) -> u32 {
        state("hopper", &[("facing", facing), ("enabled", enabled)])
    }

    fn chest(source: &AnvilChunkSource, pos: [i32; 3]) {
        source
            .set_block(
                pos[0],
                pos[1],
                pos[2],
                state("chest", &[("facing", "north")]),
            )
            .unwrap();
    }

    fn fill_chest(source: &AnvilChunkSource, pos: [i32; 3], stack: ItemStack) {
        let mut items = vec![ItemStack::empty(); 27];
        items[0] = stack;
        block_container::store_slots(source, pos, "minecraft:chest", &items);
    }

    #[test]
    fn insert_fills_matching_stacks_then_empty_slots() {
        let mut items = vec![ItemStack::empty(); SIZE];
        let stone = ItemStack::from_name("stone").unwrap().copy_with_count(64);
        items[0] = stone.clone();
        // A full matching stack takes nothing, so the second stack goes in the next slot.
        assert_eq!(insert(&mut items, stone.clone()), None);
        assert_eq!(items[1].count, 64);
        // Two more stacks fill the middle slots, one more fills the last slot.
        assert_eq!(insert(&mut items, stone.copy_with_count(128)), None);
        assert_eq!(items[3].count, 64);
        assert_eq!(insert(&mut items, stone.clone()), None);
        assert_eq!(items[4].count, 64);
        // A full hopper keeps the whole stack.
        let left = insert(&mut items, stone).unwrap();
        assert_eq!(left.count, 64);
    }

    #[test]
    fn comparator_signal_scales_with_filled_slots() {
        let mut items = vec![ItemStack::empty(); SIZE];
        assert_eq!(comparator_signal(&items), 0);
        items[0] = ItemStack::from_name("stone").unwrap();
        assert_eq!(comparator_signal(&items), 3);
        for item in &mut items {
            *item = ItemStack::from_name("stone").unwrap();
        }
        assert_eq!(comparator_signal(&items), 15);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn hopper_chain_moves_one_item_per_eight_ticks() {
        let source = loaded().await;
        // Source chest at y=66, two hoppers at y=65 and y=64, destination chest at y=63.
        chest(&source, [8, 66, 8]);
        source
            .set_block(8, 65, 8, hopper_state("down", "true"))
            .unwrap();
        source
            .set_block(8, 64, 8, hopper_state("down", "true"))
            .unwrap();
        chest(&source, [8, 63, 8]);
        fill_chest(
            &source,
            [8, 66, 8],
            ItemStack::from_name("stone").unwrap().copy_with_count(64),
        );

        let mut hoppers = Hoppers::default();
        hoppers.blocks_changed(&source, &[[8, 65, 8], [8, 64, 8]]);
        let mut entities = EntityStore::default();
        let mut rng = Rng::default();
        for _ in 0..8 {
            hoppers.tick(&source, &mut entities, &mut rng);
        }
        let source_items = block_container::load_slots(&source, [8, 66, 8], 27);
        assert_eq!(source_items[0].count, 63);
        assert_eq!(
            block_container::load_slots(&source, [8, 63, 8], 27)[0].count,
            1
        );
        for _ in 0..8 {
            hoppers.tick(&source, &mut entities, &mut rng);
        }
        assert_eq!(
            block_container::load_slots(&source, [8, 63, 8], 27)[0].count,
            2
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn powered_hopper_stops_and_data_round_trips() {
        let source = loaded().await;
        chest(&source, [2, 70, 2]);
        source
            .set_block(2, 69, 2, hopper_state("down", "false"))
            .unwrap();
        fill_chest(
            &source,
            [2, 70, 2],
            ItemStack::from_name("stone").unwrap().copy_with_count(3),
        );
        let mut hoppers = Hoppers::default();
        hoppers.blocks_changed(&source, &[[2, 69, 2]]);
        let mut entities = EntityStore::default();
        let mut rng = Rng::default();
        for _ in 0..20 {
            hoppers.tick(&source, &mut entities, &mut rng);
        }
        assert_eq!(
            block_container::load_slots(&source, [2, 70, 2], 27)[0].count,
            3
        );
        assert!(load(&source, [2, 69, 2]).items.iter().all(|s| s.is_empty()));

        // Unlocking it moves items again, and the cooldown is persisted.
        source
            .set_block(2, 69, 2, hopper_state("down", "true"))
            .unwrap();
        for _ in 0..8 {
            hoppers.tick(&source, &mut entities, &mut rng);
        }
        assert_eq!(
            block_container::load_slots(&source, [2, 70, 2], 27)[0].count,
            2
        );
        assert!(load(&source, [2, 69, 2]).cooldown > 0);
        let entity = source.block_entity(2, 69, 2).unwrap();
        assert!(entity.data.get("Items").is_some());
        assert!(entity.data.get("TransferCooldown").is_some());
    }

    #[test]
    fn insert_rules_respect_container_faces() {
        assert_eq!(insert_slots("furnace", Face::Top, 3), vec![0]);
        assert_eq!(insert_slots("furnace", Face::Side, 3), vec![1]);
        assert!(insert_slots("furnace", Face::Bottom, 3).is_empty());
        assert_eq!(insert_slots("brewing_stand", Face::Top, 5), vec![3]);
        assert_eq!(extract_slots("furnace", 3), vec![2]);
        assert_eq!(extract_slots("hopper", SIZE), (0..SIZE).collect::<Vec<_>>());
    }
}
