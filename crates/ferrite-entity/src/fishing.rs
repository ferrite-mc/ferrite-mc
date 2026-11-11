//! Fishing bobbers (vanilla `FishingHook`): the hook a fishing rod casts, its flight, the water
//! bob and the bite countdown.
//!
//! The server-side state is intentionally small: the bobber is an ordinary tracked entity, and
//! [`FishingState`] records who cast it, whether it is floating and how long until a bite. A
//! player reeling in removes the entity; if it was biting, [`loot`] is rolled and the item is
//! dropped where the hook floats. The region owns the reel decision because players are not in
//! the entity store.
//!
//! Vanilla detail that is not modelled: enchantment luck and the full `gameplay/fishing` loot
//! table, fish swimming toward the hook, and the splash sound. The bobber is removed when its
//! owner leaves the region, so it never crosses a region boundary.

use crate::id::EntityKey;
use crate::physics::{self, BlockGetter};
use crate::rng::Rng;
use crate::store::{EntityData, EntityRecord, EntityStore, Flags};
use crate::{allocate_id, random_uuid};
use ferrite_math::Vec3;
use std::sync::OnceLock;
use uuid::Uuid;

/// Vanilla `FishingHook` gravity per tick.
pub const GRAVITY: f64 = 0.03;
/// Vanilla `FishingHook` water drag (the air drag is `0.92` for the first ticks).
pub const DRAG: f64 = 0.92;
/// A hook disappears after one minute without a bite (`FishingHook` lifetime).
pub const LIFETIME: u32 = 1200;
/// Vanilla wait range for a bite, in ticks (`FishingHook` 100..600).
pub const BITE_MIN: u32 = 100;
pub const BITE_MAX: u32 = 600;
/// Ticks a bite stays on the hook before the fish escapes.
pub const BITE_WINDOW: u32 = 40;

/// Per-bobber state beside the entity columns.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FishingState {
    /// The player who cast the hook.
    pub owner: Uuid,
    /// The hook is floating in water.
    pub in_water: bool,
    /// Ticks until the next bite, or the remaining bite window while `biting`.
    pub bite_timer: u32,
    /// A fish is on the hook and a reel now catches it.
    pub biting: bool,
    /// Ticks lived, capped by [`LIFETIME`].
    pub age: u32,
}

/// Entity type id of `minecraft:fishing_bobber`.
pub fn bobber_type() -> u16 {
    static KIND: OnceLock<u16> = OnceLock::new();
    *KIND.get_or_init(|| {
        ferrite_data::entity_props::by_name("fishing_bobber")
            .expect("vanilla has a fishing bobber entity type")
            .id
    })
}

/// A new bobber at `pos` flying with `vel`.
pub fn new_bobber(pos: Vec3, vel: Vec3, owner: Uuid, _rng: &mut Rng) -> EntityRecord {
    let mut data = EntityData::new(allocate_id(), random_uuid(), bobber_type(), 0.25, 0.25);
    let horizontal = (vel.x * vel.x + vel.z * vel.z).sqrt();
    data.yaw = (-vel.x).atan2(vel.z).to_degrees() as f32;
    data.pitch = (-vel.y).atan2(horizontal).to_degrees() as f32;
    data.head_yaw = data.yaw;
    data.fishing = Some(FishingState {
        owner,
        in_water: false,
        bite_timer: 0,
        biting: false,
        age: 0,
    });
    EntityRecord {
        pos,
        vel,
        flags: Flags::default(),
        data,
    }
}

/// Something the region must react to after a tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// A fish bit; the hook now yields loot when reeled.
    Bite {
        key: EntityKey,
        owner: Uuid,
        pos: Vec3,
    },
    /// The hook hit land, expired, or its owner left the region.
    Lost { key: EntityKey, owner: Uuid },
}

/// Advances every bobber one tick. `owners` is the set of players still in the region; a hook
/// whose owner is gone is removed instead of ticking without an owner.
pub fn tick_bobbers(
    store: &mut EntityStore,
    blocks: &impl BlockGetter,
    owners: &[Uuid],
    rng: &mut Rng,
) -> Vec<Event> {
    let keys = store.keys().to_vec();
    let mut events = Vec::new();
    for key in keys {
        let Some(mut state) = store.data(key).and_then(|data| data.fishing) else {
            continue;
        };
        if store
            .flags(key)
            .is_some_and(|flags| flags.has(Flags::REMOVED))
        {
            continue;
        }
        state.age += 1;
        if state.age > LIFETIME || !owners.contains(&state.owner) {
            remove(store, key);
            events.push(Event::Lost {
                key,
                owner: state.owner,
            });
            continue;
        }
        let pos = store.pos(key).unwrap_or_default();
        if !state.in_water {
            if is_water(blocks, pos) {
                state.in_water = true;
                state.biting = false;
                state.bite_timer = bite_delay(rng);
                store.set_vel(key, Vec3::ZERO);
                if let Some(data) = store.data_mut(key) {
                    data.fishing = Some(state);
                }
                continue;
            }
            let mut vel = store.vel(key).unwrap_or_default();
            vel.y -= GRAVITY;
            vel.x *= DRAG;
            vel.y *= DRAG;
            vel.z *= DRAG;
            store.set_vel(key, vel);
            physics::move_entity(store, key, vel, blocks);
            if store.flags(key).is_some_and(|flags| {
                flags.has(Flags::HORIZONTAL_COLLISION) || flags.has(Flags::VERTICAL_COLLISION)
            }) {
                remove(store, key);
                events.push(Event::Lost {
                    key,
                    owner: state.owner,
                });
                continue;
            }
        } else if !is_water(blocks, pos) {
            // The hook left the water (a piston, a broken block): fall again.
            state.in_water = false;
            state.biting = false;
            state.bite_timer = 0;
        } else if state.bite_timer > 0 {
            state.bite_timer -= 1;
            if state.bite_timer == 0 {
                if state.biting {
                    // The bite window closed without a reel.
                    state.biting = false;
                    state.bite_timer = bite_delay(rng);
                } else {
                    state.biting = true;
                    state.bite_timer = BITE_WINDOW;
                    events.push(Event::Bite {
                        key,
                        owner: state.owner,
                        pos,
                    });
                }
            }
        }
        // A floating hook does not drift; the client draws its bob from the entity itself.
        if state.in_water {
            store.set_vel(key, Vec3::ZERO);
        }
        if let Some(data) = store.data_mut(key) {
            data.fishing = Some(state);
        }
    }
    events
}

/// The item a reeled-in bite yields. This is the vanilla pool shape (fish, junk, treasure) but
/// not the full data-driven `gameplay/fishing` table, which needs the loot engine and the
/// player's luck.
pub fn loot(rng: &mut Rng) -> &'static str {
    match rng.next_int(100) {
        0..=4 => {
            const TREASURE: [&str; 4] = [
                "minecraft:bow",
                "minecraft:book",
                "minecraft:nautilus_shell",
                "minecraft:saddle",
            ];
            TREASURE[rng.next_int(TREASURE.len() as i32) as usize]
        }
        5..=14 => {
            const JUNK: [&str; 5] = [
                "minecraft:leather",
                "minecraft:bone",
                "minecraft:string",
                "minecraft:bowl",
                "minecraft:stick",
            ];
            JUNK[rng.next_int(JUNK.len() as i32) as usize]
        }
        _ => {
            const FISH: [&str; 4] = [
                "minecraft:cod",
                "minecraft:salmon",
                "minecraft:tropical_fish",
                "minecraft:pufferfish",
            ];
            FISH[rng.next_int(FISH.len() as i32) as usize]
        }
    }
}

fn bite_delay(rng: &mut Rng) -> u32 {
    BITE_MIN + rng.next_int((BITE_MAX - BITE_MIN + 1) as i32) as u32
}

fn is_water(blocks: &impl BlockGetter, pos: Vec3) -> bool {
    let x = pos.x.floor() as i32;
    let y = pos.y.floor() as i32;
    let z = pos.z.floor() as i32;
    blocks
        .block_state(x, y, z)
        .and_then(ferrite_data::blocks::state)
        .is_some_and(|(name, properties)| {
            name == "minecraft:water"
                || name == "water"
                || properties
                    .iter()
                    .any(|(key, value)| *key == "waterlogged" && *value == "true")
        })
}

fn remove(store: &mut EntityStore, key: EntityKey) {
    if let Some(flags) = store.flags_mut(key) {
        flags.set(Flags::REMOVED, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::tests::TestWorld;

    fn water_state() -> u16 {
        ferrite_data::blocks::default_state("water").expect("water state")
    }

    fn air_state() -> u16 {
        ferrite_data::blocks::default_state("air").expect("air state")
    }

    /// A pool whose water surface is y = 63, open air above.
    struct Pool;

    impl BlockGetter for Pool {
        fn block_state(&self, _x: i32, _y: i32, _z: i32) -> Option<u16> {
            Some(if _y <= 63 { water_state() } else { air_state() })
        }
    }

    #[test]
    fn a_cast_hook_falls_into_water_and_bites_within_the_vanilla_wait() {
        let mut store = EntityStore::default();
        let owner = Uuid::from_u128(0xf15);
        let key = store.insert(new_bobber(
            Vec3::new(0.5, 66.0, 0.5),
            Vec3::new(0.0, -1.0, 0.0),
            owner,
            &mut Rng::new(1),
        ));
        let mut rng = Rng::new(2);
        let mut events = Vec::new();
        for _ in 0..(LIFETIME + 1) {
            events = tick_bobbers(&mut store, &Pool, &[owner], &mut rng);
            if !events.is_empty() {
                break;
            }
        }
        assert!(
            matches!(events[0], Event::Bite { owner: who, .. } if who == owner),
            "{events:?}"
        );
        let state = store.data(key).unwrap().fishing.unwrap();
        assert!(state.in_water);
        assert!(state.biting);
    }

    #[test]
    fn a_hook_that_lands_on_the_ground_is_lost() {
        let mut store = EntityStore::default();
        let owner = Uuid::from_u128(0xf16);
        let key = store.insert(new_bobber(
            Vec3::new(0.5, 64.4, 0.5),
            Vec3::new(0.0, -1.0, 0.0),
            owner,
            &mut Rng::new(3),
        ));
        let events = tick_bobbers(&mut store, &TestWorld::flat(64), &[owner], &mut Rng::new(4));
        assert!(matches!(events[0], Event::Lost { key: lost, .. } if lost == key));
        assert!(store.flags(key).unwrap().has(Flags::REMOVED));
    }

    #[test]
    fn a_hook_whose_owner_left_is_lost_without_ticking_further() {
        let mut store = EntityStore::default();
        let owner = Uuid::from_u128(0xf17);
        let key = store.insert(new_bobber(
            Vec3::new(0.5, 80.0, 0.5),
            Vec3::ZERO,
            owner,
            &mut Rng::new(5),
        ));
        let events = tick_bobbers(&mut store, &Pool, &[], &mut Rng::new(6));
        assert_eq!(events, [Event::Lost { key, owner }]);
        assert!(store.flags(key).unwrap().has(Flags::REMOVED));
    }

    #[test]
    fn loot_rolls_are_in_the_vanilla_pools() {
        let mut rng = Rng::new(7);
        for _ in 0..200 {
            let name = loot(&mut rng);
            assert!(
                name.starts_with("minecraft:"),
                "a fish, junk or treasure item, not {name}"
            );
        }
    }
}
