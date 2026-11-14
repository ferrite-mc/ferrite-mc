//! Falling blocks (vanilla `FallingBlockEntity`): a gravity block state falls as an entity and
//! is placed back where it lands.
//!
//! Physics follow `FallingBlockEntity.tick`: gravity `0.04` per tick, air drag `0.98`, and the
//! block is placed in the entity's block position once it rests on a block, provided that
//! position can be replaced (`BlockState.canBeReplaced`). A block entity, when the state has
//! one, is not preserved yet.

use crate::physics::{self, BlockGetter};
use crate::store::{EntityData, EntityRecord, EntityStore, Flags};
use crate::{EntityKey, allocate_id, random_uuid};
use ferrite_math::Vec3;
use std::sync::OnceLock;

const GRAVITY: f64 = 0.04;
const AIR_DRAG: f64 = 0.98;
/// Vanilla `FallingBlockEntity` dimensions.
pub const WIDTH: f64 = 0.98;
pub const HEIGHT: f64 = 0.98;

/// Entity type id of `minecraft:falling_block`.
pub fn falling_type() -> u16 {
    static KIND: OnceLock<u16> = OnceLock::new();
    *KIND.get_or_init(|| {
        ferrite_data::entity_props::by_name("falling_block")
            .expect("vanilla has a falling_block entity type")
            .id
    })
}

/// A falling block entity (vanilla `FallingBlockEntity.fall`).
pub fn new_falling(pos: Vec3, state: u16) -> EntityRecord {
    let mut data = EntityData::new(allocate_id(), random_uuid(), falling_type(), WIDTH, HEIGHT);
    data.block_state = Some(state);
    EntityRecord {
        pos,
        vel: Vec3::default(),
        flags: Flags::default(),
        data,
    }
}

/// Blocks a falling block lands on and edits.
pub trait FallingWorld: BlockGetter {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: u16);
}

/// Ticks every falling block in the store (vanilla `FallingBlockEntity.tick`).
pub fn tick_falling_blocks(
    store: &mut EntityStore,
    world: &mut impl FallingWorld,
    min_y: i32,
    max_y: i32,
) {
    let kind = falling_type();
    let keys = store.keys().to_vec();
    for key in keys {
        let Some(data) = store.data_mut(key) else {
            continue;
        };
        if data.kind != kind || data.block_state.is_none() {
            continue;
        }
        if store.flags(key).unwrap().has(Flags::REMOVED) {
            continue;
        }
        tick_falling(store, key, world, min_y, max_y);
    }
}

fn is_powder(state: u16) -> bool {
    ferrite_data::blocks::state(state).is_some_and(|(name, _)| name.ends_with("_concrete_powder"))
}

fn holds_water(state: u16) -> bool {
    ferrite_fluid::fluid_state(u32::from(state)).kind == ferrite_fluid::FluidKind::Water
}

fn replaceable(state: u16) -> bool {
    usize::from(state) < ferrite_data::blocks::STATE_COUNT
        && ferrite_data::block_props::get(state).replaceable
}

fn tick_falling(
    store: &mut EntityStore,
    key: EntityKey,
    world: &mut impl FallingWorld,
    min_y: i32,
    max_y: i32,
) {
    let (state, tick_count) = {
        let data = store.data_mut(key).unwrap();
        data.tick_count += 1;
        (data.block_state.unwrap(), data.tick_count)
    };
    let before = store.pos(key).unwrap_or_default();
    // `applyGravity` then `move(MoverType.SELF, delta)`.
    let mut vel = store.vel(key).unwrap_or_default();
    vel.y -= GRAVITY;
    store.set_vel(key, vel);
    physics::move_entity(store, key, vel, world);
    let mut vel = store.vel(key).unwrap_or_default();
    // Concrete powder that reaches water counts as landed: its own cell holds water, or, when
    // it moves faster than one block a tick, the first water on the way
    // (`ClipContext.Fluid.SOURCE_ONLY`) is where it stops.
    let mut in_water = false;
    if is_powder(state) {
        let fluid_at = |p: Vec3| {
            world
                .block_state(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32)
                .is_some_and(holds_water)
        };
        let now = store.pos(key).unwrap_or_default();
        in_water = fluid_at(now);
        if !in_water && vel.x * vel.x + vel.y * vel.y + vel.z * vel.z > 1.0 {
            for step in 1..=16 {
                let t = f64::from(step) / 16.0;
                let at = Vec3::new(
                    before.x + (now.x - before.x) * t,
                    before.y + (now.y - before.y) * t,
                    before.z + (now.z - before.z) * t,
                );
                if fluid_at(at) {
                    store.set_pos(key, at);
                    in_water = true;
                    break;
                }
            }
        }
    }
    vel = Vec3::new(vel.x * AIR_DRAG, vel.y * AIR_DRAG, vel.z * AIR_DRAG);
    store.set_vel(key, vel);
    let pos = store.pos(key).unwrap_or_default();
    let (x, y, z) = (
        pos.x.floor() as i32,
        pos.y.floor() as i32,
        pos.z.floor() as i32,
    );
    let flags = store.flags(key).unwrap_or_default();
    let landed = flags.has(Flags::ON_GROUND) || flags.has(Flags::HORIZONTAL_COLLISION) || in_water;
    if landed && tick_count > 1 && world.block_state(x, y, z).is_some_and(replaceable) {
        // Vanilla: `level.setBlock(blockPosition(), blockState, 3)` then `discard()`.
        world.set_block(x, y, z, state);
        store.flags_mut(key).unwrap().set(Flags::REMOVED, true);
        return;
    }
    if y < min_y || y > max_y || tick_count > 600 {
        store.flags_mut(key).unwrap().set(Flags::REMOVED, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct Grid {
        floor: i32,
        placed: HashMap<(i32, i32, i32), u16>,
    }

    impl BlockGetter for Grid {
        fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
            if let Some(state) = self.placed.get(&(x, y, z)) {
                return Some(*state);
            }
            let name = if y < self.floor { "stone" } else { "air" };
            ferrite_data::blocks::default_state(name)
        }
    }

    impl FallingWorld for Grid {
        fn set_block(&mut self, x: i32, y: i32, z: i32, state: u16) {
            self.placed.insert((x, y, z), state);
        }
    }

    fn sand() -> u16 {
        ferrite_data::blocks::default_state("sand").unwrap()
    }

    #[test]
    fn falls_under_gravity_and_places_its_block_on_the_floor() {
        let mut store = EntityStore::default();
        let record = new_falling(Vec3::new(0.5, 70.0, 0.5), sand());
        let key = store.insert(record);
        assert_eq!(store.data(key).unwrap().block_state, Some(sand()));
        let mut world = Grid {
            floor: 64,
            placed: HashMap::new(),
        };
        for _ in 0..200 {
            tick_falling_blocks(&mut store, &mut world, -64, 320);
        }
        assert_eq!(world.placed.get(&(0, 64, 0)), Some(&sand()));
        assert!(store.flags(key).unwrap().has(Flags::REMOVED));
        assert!(store.pos(key).is_some());
    }

    #[test]
    fn concrete_powder_lands_in_water_before_reaching_the_floor() {
        let powder = ferrite_data::blocks::default_state("white_concrete_powder").unwrap();
        let water = ferrite_data::blocks::default_state("water").unwrap();
        let mut store = EntityStore::default();
        let key = store.insert(new_falling(Vec3::new(0.5, 80.0, 0.5), powder));
        // Ten blocks of water over a floor at 40: vanilla places the powder at the water's
        // surface rather than letting it sink to the bottom.
        let mut world = Grid {
            floor: 40,
            placed: (60..70).map(|y| ((0, y, 0), water)).collect(),
        };
        for _ in 0..200 {
            tick_falling_blocks(&mut store, &mut world, -64, 320);
        }
        assert!(store.flags(key).unwrap().has(Flags::REMOVED));
        let at = world
            .placed
            .iter()
            .find(|(_, state)| **state == powder)
            .map(|(pos, _)| *pos)
            .expect("the powder was placed");
        assert!(at.1 >= 60, "placed at the surface, not the floor: {at:?}");
    }

    #[test]
    fn an_entity_in_the_way_keeps_the_block_from_landing() {
        let mut store = EntityStore::default();
        store.insert(new_falling(Vec3::new(0.5, 70.0, 0.5), sand()));
        let mut world = Grid {
            floor: 64,
            placed: HashMap::from([(
                (0, 64, 0),
                ferrite_data::blocks::default_state("stone").unwrap(),
            )]),
        };
        for _ in 0..200 {
            tick_falling_blocks(&mut store, &mut world, -64, 320);
        }
        // The block at 64 is solid, so the sand lands on 65 instead of replacing it.
        assert_eq!(world.placed.get(&(0, 65, 0)), Some(&sand()));
    }
}
