//! Primed TNT (vanilla `PrimedTnt`): a lit TNT block falls under gravity, keeps its fuse and
//! explodes when the fuse runs out.

use crate::physics::{self, BlockGetter};
use crate::store::{EntityData, EntityRecord, EntityStore, Flags};
use crate::{EntityKey, allocate_id, random_uuid};
use ferrite_math::Vec3;
use std::sync::OnceLock;

const GRAVITY: f64 = 0.04;
const AIR_DRAG: f64 = 0.98;
/// Vanilla `PrimedTnt` dimensions.
pub const WIDTH: f64 = 0.98;
pub const HEIGHT: f64 = 0.98;
/// Vanilla `PrimedTnt.DEFAULT_FUSE` (4 seconds).
pub const FUSE: i32 = 80;
/// Vanilla `PrimedTnt` explosion power (`Explosion(level, this, ..., 4.0F)`).
pub const POWER: f32 = 4.0;

/// Entity type id of `minecraft:tnt`.
pub fn tnt_type() -> u16 {
    static KIND: OnceLock<u16> = OnceLock::new();
    *KIND.get_or_init(|| {
        ferrite_data::entity_props::by_name("tnt")
            .expect("vanilla has a tnt entity type")
            .id
    })
}

/// A primed TNT entity at a block position (vanilla `PrimedTnt` constructor).
pub fn new_primed_tnt(pos: Vec3, fuse: i32) -> EntityRecord {
    new_primed_tnt_owned(pos, fuse, None)
}

/// A primed TNT entity with an optional igniter UUID (`PrimedTnt.owner`, for `/execute on origin`).
pub fn new_primed_tnt_owned(pos: Vec3, fuse: i32, owner: Option<uuid::Uuid>) -> EntityRecord {
    use ferrite_protocol::packets::play::MetaValue;
    let mut data = EntityData::new(allocate_id(), random_uuid(), tnt_type(), WIDTH, HEIGHT);
    data.fuse = Some(fuse);
    data.origin_owner = owner;
    let tnt_state = ferrite_data::blocks::default_state("minecraft:tnt").unwrap_or(0);
    data.metadata
        .define(crate::metadata::tnt::FUSE, MetaValue::Int(FUSE));
    data.metadata.define(
        crate::metadata::tnt::BLOCK_STATE,
        MetaValue::BlockState(tnt_state),
    );
    data.metadata
        .set(crate::metadata::tnt::FUSE, MetaValue::Int(fuse));
    data.metadata.clear_dirty();
    EntityRecord {
        pos,
        vel: Vec3::default(),
        flags: Flags::default(),
        data,
    }
}

/// Ticks every primed TNT in the store; returns the positions whose fuse ended, where the
/// caller runs the vanilla `Explosion`.
pub fn tick_tnt(store: &mut EntityStore, blocks: &impl BlockGetter) -> Vec<Vec3> {
    let kind = tnt_type();
    let keys = store.keys().to_vec();
    let mut explosions = Vec::new();
    for key in keys {
        let Some(data) = store.data_mut(key) else {
            continue;
        };
        if data.kind != kind || data.fuse.is_none() {
            continue;
        }
        if store.flags(key).unwrap().has(Flags::REMOVED) {
            continue;
        }
        if let Some(pos) = tick_primed_tnt(store, key, blocks) {
            explosions.push(pos);
        }
    }
    explosions
}

fn tick_primed_tnt(
    store: &mut EntityStore,
    key: EntityKey,
    blocks: &impl BlockGetter,
) -> Option<Vec3> {
    // Vanilla `PrimedTnt.tick`: gravity, `move(MoverType.SELF, delta)`, drag, ground friction.
    let mut vel = store.vel(key).unwrap_or_default();
    vel.y -= GRAVITY;
    store.set_vel(key, vel);
    physics::move_entity(store, key, vel, blocks);
    let mut vel = store.vel(key).unwrap_or_default();
    vel = Vec3::new(vel.x * AIR_DRAG, vel.y * AIR_DRAG, vel.z * AIR_DRAG);
    if store.flags(key).unwrap_or_default().has(Flags::ON_GROUND) {
        vel = Vec3::new(vel.x * 0.7, vel.y * -0.5, vel.z * 0.7);
    }
    store.set_vel(key, vel);
    let data = store.data_mut(key).unwrap();
    data.tick_count += 1;
    let fuse = data.fuse.unwrap_or(0) - 1;
    data.fuse = Some(fuse);
    if fuse > 0 {
        return None;
    }
    let pos = store.pos(key).unwrap_or_default();
    store.flags_mut(key).unwrap().set(Flags::REMOVED, true);
    Some(pos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct Grid {
        floor: i32,
        blocks: HashMap<(i32, i32, i32), u16>,
    }

    impl BlockGetter for Grid {
        fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
            if let Some(state) = self.blocks.get(&(x, y, z)) {
                return Some(*state);
            }
            ferrite_data::blocks::default_state(if y < self.floor { "stone" } else { "air" })
        }
    }

    #[test]
    fn fuse_ends_after_eighty_ticks_and_the_entity_is_removed() {
        let mut store = EntityStore::default();
        let key = store.insert(new_primed_tnt(Vec3::new(0.5, 80.0, 0.5), FUSE));
        let blocks = Grid {
            floor: 64,
            blocks: HashMap::new(),
        };
        let mut explosions = Vec::new();
        for tick in 0..FUSE {
            let ended = tick_tnt(&mut store, &blocks);
            if tick + 1 < FUSE {
                assert!(ended.is_empty(), "exploded early at tick {}", tick + 1);
            } else {
                explosions = ended;
            }
        }
        assert_eq!(explosions.len(), 1);
        // It fell for all 80 ticks and landed on the floor before exploding.
        assert!((explosions[0].y - 64.0).abs() < 0.01, "{}", explosions[0].y);
        assert!(store.flags(key).unwrap().has(Flags::REMOVED));
    }
}
