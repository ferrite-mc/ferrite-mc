//! Droppers and dispensers (vanilla `DropperBlockEntity`/`DispenserBlockEntity`).
//!
//! Both are nine-slot block entities that fire one item when a redstone signal rises. A dropper
//! moves the item into the container it faces (or drops it on the ground); a dispenser uses the
//! item's own behaviour: arrows fly, eggs and snowballs are thrown, splash potions burst, fire
//! charges light the facing block, and everything else is dropped like a dropper.

use ferrite_data::blocks;
use ferrite_entity::Rng;
use ferrite_item::ItemStack;
use ferrite_math::Vec3;
use std::collections::HashMap;

use super::anvil::AnvilChunkSource;
use super::block_container;
use super::redstone::{Redstone, RedstoneWorld};

/// Slots of one dropper or dispenser.
pub const SIZE: usize = 9;

/// Which of the two blocks this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Dropper,
    Dispenser,
}

impl Kind {
    pub fn block(self) -> &'static str {
        match self {
            Self::Dropper => "minecraft:dropper",
            Self::Dispenser => "minecraft:dispenser",
        }
    }
}

/// The block entity state of a dropper or dispenser.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DispenserData {
    pub items: Vec<ItemStack>,
}

/// What a trigger did with one item.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// A dispenser arrow, with its spawn position and velocity.
    Arrow { pos: [f64; 3], velocity: [f64; 3] },
    /// An egg or snowball, approximated by a thrown item entity.
    Throw {
        pos: [f64; 3],
        velocity: [f64; 3],
        stack: ItemStack,
    },
    /// A splash or lingering potion to burst at the facing block.
    Splash { pos: [f64; 3], stack: ItemStack },
    /// A fire charge: a small fireball leaves the dispenser face (`DispenserBlock` fire charge).
    Fire { pos: [f64; 3], velocity: [f64; 3] },
    /// A dropper (or an ordinary dispenser item) to eject.
    Drop {
        pos: [f64; 3],
        velocity: [f64; 3],
        stack: ItemStack,
    },
    /// Nothing could move (empty, or a blocked facing side).
    Nothing,
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

/// The block a dropper or dispenser faces, as an offset.
pub fn facing(source: &AnvilChunkSource, [x, y, z]: [i32; 3]) -> Option<[i32; 3]> {
    Some(match property_at(source, [x, y, z], "facing")? {
        "down" => [0, -1, 0],
        "up" => [0, 1, 0],
        "north" => [0, 0, -1],
        "south" => [0, 0, 1],
        "west" => [-1, 0, 0],
        "east" => [1, 0, 0],
        _ => return None,
    })
}

/// The dropper or dispenser at a block name.
pub fn kind(name: &str) -> Option<Kind> {
    match name {
        "minecraft:dropper" => Some(Kind::Dropper),
        "minecraft:dispenser" => Some(Kind::Dispenser),
        _ => None,
    }
}

/// Reads a dropper's or dispenser's items.
pub fn load(source: &AnvilChunkSource, [x, y, z]: [i32; 3]) -> DispenserData {
    DispenserData {
        items: block_container::load_slots(source, [x, y, z], SIZE),
    }
}

/// Writes the items back, keeping the block entity's other NBT.
pub fn store(source: &AnvilChunkSource, [x, y, z]: [i32; 3], kind: Kind, data: &DispenserData) {
    block_container::store_slots(source, [x, y, z], kind.block(), &data.items);
}

/// Vanilla `DispenserBlockEntity.getRandomSlot`: the first non-empty slot (the vanilla random
/// choice only changes which of several stacks fires).
fn first_slot(items: &[ItemStack]) -> Option<usize> {
    items.iter().position(|stack| !stack.is_empty())
}

fn plain(stack: &ItemStack) -> ferrite_protocol::packets::play::ItemStack {
    ferrite_protocol::packets::play::ItemStack {
        count: stack.count,
        item_id: i32::from(stack.item),
    }
}

fn center([x, y, z]: [i32; 3], offset: [i32; 3]) -> [f64; 3] {
    [
        f64::from(x) + 0.5 + f64::from(offset[0]) * 0.6,
        f64::from(y) + 0.5 + f64::from(offset[1]) * 0.6,
        f64::from(z) + 0.5 + f64::from(offset[2]) * 0.6,
    ]
}

/// Vanilla `DispenserBlock.dispenseFrom`: consumes one item and reports what the region must
/// spawn or apply. `dropper` containers receive the item through [`hopper`](super::hopper)'s
/// insertion rules.
pub fn dispense(source: &AnvilChunkSource, pos: [i32; 3], kind: Kind) -> Action {
    let Some(offset) = facing(source, pos) else {
        return Action::Nothing;
    };
    let target = [pos[0] + offset[0], pos[1] + offset[1], pos[2] + offset[2]];
    let mut data = load(source, pos);
    let Some(slot) = first_slot(&data.items) else {
        return Action::Nothing;
    };
    let name = data.items[slot]
        .name()
        .map(|name| name.strip_prefix("minecraft:").unwrap_or(name).to_owned())
        .unwrap_or_default();

    // A dropper always tries the facing container first, and drops the item when that fails.
    if kind == Kind::Dropper {
        if let Some(target_name) = name_at(source, target)
            && let Some(size) = block_container::inventory_slots(target_name)
        {
            let mut target_items = block_container::load_slots(source, target, size);
            let mut one = data.items[slot].clone();
            one.count = 1;
            if block_container::accepts(target_name, &one)
                && super::hopper::insert(&mut target_items, one).is_none()
            {
                data.items[slot].count -= 1;
                if data.items[slot].count <= 0 {
                    data.items[slot] = ItemStack::empty();
                }
                block_container::store_slots(source, target, target_name, &target_items);
                store(source, pos, kind, &data);
                return Action::Nothing;
            }
        }
        return eject(&mut data, source, pos, kind, slot, offset);
    }

    let action = match name.as_str() {
        "arrow" => Action::Arrow {
            pos: center(pos, offset),
            velocity: [
                f64::from(offset[0]) * 1.1,
                f64::from(offset[1]) * 1.1,
                f64::from(offset[2]) * 1.1,
            ],
        },
        "egg" | "snowball" => Action::Throw {
            pos: center(pos, offset),
            velocity: [
                f64::from(offset[0]) * 0.6,
                f64::from(offset[1]) * 0.6 + 0.1,
                f64::from(offset[2]) * 0.6,
            ],
            stack: one(data.items[slot].clone()),
        },
        "splash_potion" | "lingering_potion" => Action::Splash {
            pos: center(pos, offset),
            stack: one(data.items[slot].clone()),
        },
        "fire_charge" => Action::Fire {
            pos: center(pos, offset),
            velocity: [
                f64::from(offset[0]) * 0.3,
                f64::from(offset[1]) * 0.3,
                f64::from(offset[2]) * 0.3,
            ],
        },
        _ => return eject(&mut data, source, pos, kind, slot, offset),
    };
    data.items[slot].count -= 1;
    if data.items[slot].count <= 0 {
        data.items[slot] = ItemStack::empty();
    }
    store(source, pos, kind, &data);
    action
}

fn one(stack: ItemStack) -> ItemStack {
    stack.copy_with_count(1)
}

/// Vanilla `DispenserBlock`/`DropperBlock` eject path: the item leaves towards the facing
/// direction (an occupied, non-container block suppresses the drop).
fn eject(
    data: &mut DispenserData,
    source: &AnvilChunkSource,
    pos: [i32; 3],
    kind: Kind,
    slot: usize,
    offset: [i32; 3],
) -> Action {
    let target = [pos[0] + offset[0], pos[1] + offset[1], pos[2] + offset[2]];
    match name_at(source, target) {
        Some("minecraft:air") => {}
        Some(_) => return Action::Nothing,
        None => return Action::Nothing,
    }
    let stack = data.items[slot].clone();
    data.items[slot].count -= 1;
    if data.items[slot].count <= 0 {
        data.items[slot] = ItemStack::empty();
    }
    store(source, pos, kind, data);
    Action::Drop {
        pos: center(pos, offset),
        velocity: [
            f64::from(offset[0]) * 0.2,
            f64::from(offset[1]) * 0.2,
            f64::from(offset[2]) * 0.2,
        ],
        stack: one(stack),
    }
}

/// One item entity for an action that ejects or throws a stack, for the region to insert.
pub fn item_entity(action: &Action, rng: &mut Rng) -> Option<ferrite_entity::EntityRecord> {
    let (pos, velocity, stack) = match action {
        Action::Drop {
            pos,
            velocity,
            stack,
        }
        | Action::Throw {
            pos,
            velocity,
            stack,
        } => (pos, velocity, stack),
        _ => return None,
    };
    Some(ferrite_entity::item::new_item(
        Vec3::new(pos[0], pos[1], pos[2]),
        Vec3::new(velocity[0], velocity[1], velocity[2]),
        plain(stack),
        rng,
    ))
}

/// The droppers and dispensers a region watches, with their last powered state (the rising
/// edge is what fires them, vanilla `DispenserBlock.neighborChanged`).
#[derive(Debug, Default)]
pub struct Dispensers {
    powered: HashMap<[i32; 3], bool>,
}

impl Dispensers {
    pub fn observe(&mut self, source: &AnvilChunkSource, pos: [i32; 3]) {
        if name_at(source, pos).and_then(kind).is_some() {
            self.powered.entry(pos).or_insert(false);
        } else if name_at(source, pos).is_some() {
            self.powered.remove(&pos);
        }
    }

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

    /// Fires every dropper or dispenser whose redstone input rose this tick.
    pub fn tick(
        &mut self,
        source: &AnvilChunkSource,
        redstone: &Redstone,
        world: &impl RedstoneWorld,
    ) -> Vec<([i32; 3], Action)> {
        let mut positions: Vec<[i32; 3]> = self.powered.keys().copied().collect();
        positions.sort_unstable();
        let mut actions = Vec::new();
        for pos in positions {
            let Some(kind) = name_at(source, pos).and_then(kind) else {
                self.powered.remove(&pos);
                continue;
            };
            let powered = redstone.power_at(world, pos) > 0;
            let previous = self.powered.insert(pos, powered).unwrap_or(false);
            if powered && !previous {
                let action = dispense(source, pos, kind);
                if action != Action::Nothing {
                    actions.push((pos, action));
                }
            }
        }
        actions
    }

    pub fn split(&mut self, keep: impl Fn(i32, i32) -> bool) -> Self {
        let mut out = Self::default();
        let moving: Vec<_> = self
            .powered
            .keys()
            .copied()
            .filter(|pos| keep(pos[0], pos[2]))
            .collect();
        for pos in moving {
            if let Some(powered) = self.powered.remove(&pos) {
                out.powered.insert(pos, powered);
            }
        }
        out
    }

    pub fn merge(&mut self, other: Self) {
        self.powered.extend(other.powered);
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

    fn state(name: &str, facing: &str) -> u32 {
        u32::from(blocks::state_id(name, &[("facing", facing), ("triggered", "false")]).unwrap())
    }

    fn put(source: &AnvilChunkSource, pos: [i32; 3], kind: Kind, item: &str) {
        let mut items = vec![ItemStack::empty(); SIZE];
        items[0] = ItemStack::from_name(item).unwrap();
        store(source, pos, kind, &DispenserData { items });
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dropper_moves_an_item_into_the_container_below() {
        let source = loaded().await;
        source
            .set_block(4, 66, 4, state("dropper", "down"))
            .unwrap();
        source
            .set_block(
                4,
                65,
                4,
                u32::from(blocks::state_id("chest", &[("facing", "north")]).unwrap()),
            )
            .unwrap();
        put(&source, [4, 66, 4], Kind::Dropper, "stone");
        assert_eq!(
            dispense(&source, [4, 66, 4], Kind::Dropper),
            Action::Nothing
        );
        assert!(load(&source, [4, 66, 4]).items.iter().all(|s| s.is_empty()));
        let target = block_container::load_slots(&source, [4, 65, 4], 27);
        assert_eq!(target[0].name(), Some("minecraft:stone"));
        // Empty droppers do nothing.
        assert_eq!(
            dispense(&source, [4, 66, 4], Kind::Dropper),
            Action::Nothing
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dispenser_shoots_arrows_and_handles_fire_charges() {
        let source = loaded().await;
        source
            .set_block(7, 70, 7, state("dispenser", "east"))
            .unwrap();
        put(&source, [7, 70, 7], Kind::Dispenser, "arrow");
        let action = dispense(&source, [7, 70, 7], Kind::Dispenser);
        assert!(matches!(action, Action::Arrow { velocity, .. } if velocity[0] > 1.0));
        assert!(load(&source, [7, 70, 7]).items.iter().all(|s| s.is_empty()));

        put(&source, [7, 70, 7], Kind::Dispenser, "fire_charge");
        assert!(matches!(
            dispense(&source, [7, 70, 7], Kind::Dispenser),
            Action::Fire { velocity, .. } if velocity[0] > 0.0 && velocity[1] == 0.0
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn blocked_dispensers_keep_their_item() {
        let source = loaded().await;
        source
            .set_block(10, 70, 10, state("dispenser", "south"))
            .unwrap();
        // A solid stone block in front blocks the ejection.
        source
            .set_block(
                10,
                70,
                11,
                u32::from(blocks::state_id("stone", &[]).unwrap()),
            )
            .unwrap();
        put(&source, [10, 70, 10], Kind::Dispenser, "stone");
        assert_eq!(
            dispense(&source, [10, 70, 10], Kind::Dispenser),
            Action::Nothing
        );
        assert_eq!(load(&source, [10, 70, 10]).items[0].count, 1);
    }

    #[test]
    fn item_entities_are_produced_for_throws_and_drops() {
        let mut rng = Rng::default();
        let action = Action::Drop {
            pos: [0.5, 65.5, 0.5],
            velocity: [0.0, -0.2, 0.0],
            stack: ItemStack::from_name("stone").unwrap(),
        };
        let record = item_entity(&action, &mut rng).unwrap();
        assert_eq!(record.pos.y, 65.5);
        assert!(
            item_entity(
                &Action::Fire {
                    pos: [0.0; 3],
                    velocity: [0.0; 3]
                },
                &mut rng
            )
            .is_none()
        );
    }
}
