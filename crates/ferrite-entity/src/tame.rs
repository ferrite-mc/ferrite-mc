//! Taming (vanilla `TamableAnimal` / `Wolf`): a bone tames a wolf one time in three, and the
//! owner can then make it sit or stand. A tamed wolf that is standing follows its owner.

use crate::mob::{self, MobKind, MobState};
use crate::{EntityKey, EntityStore, Metadata, Rng};
use ferrite_math::Vec3;
use ferrite_protocol::packets::play::MetaValue;
use uuid::Uuid;

/// Synched `TamableAnimal.DATA_FLAGS_ID` (read from the 26.3 server classes).
pub const FLAGS_INDEX: u8 = 18;
/// Synched `TamableAnimal.DATA_OWNERUUID_ID`.
pub const OWNER_INDEX: u8 = 19;
const SITTING: i8 = 1;
const TAMED: i8 = 4;
/// A tamed wolf has 40 maximum health (`Wolf.setTame`).
const TAMED_HEALTH: f32 = 40.0;
/// `FollowOwnerGoal`: start following farther than this, stop once closer than `STOP_DISTANCE`.
const START_DISTANCE_SQ: f64 = 100.0;
const STOP_DISTANCE_SQ: f64 = 4.0;
/// Farther than this the wolf teleports to its owner (`FollowOwnerGoal.teleportToOwner`).
const TELEPORT_DISTANCE_SQ: f64 = 144.0;

/// Defines the tamable data values on a wolf and writes the current state into them.
pub fn define_metadata(metadata: &mut Metadata, state: &MobState) {
    metadata.define(FLAGS_INDEX, MetaValue::Byte(0));
    metadata.define(OWNER_INDEX, MetaValue::OptionalUuid(None));
    write_metadata(metadata, state);
}

fn write_metadata(metadata: &mut Metadata, state: &MobState) {
    let mut flags = 0;
    if state.sitting {
        flags |= SITTING;
    }
    if state.owner.is_some() {
        flags |= TAMED;
    }
    metadata.set(FLAGS_INDEX, MetaValue::Byte(flags));
    metadata.set(OWNER_INDEX, MetaValue::OptionalUuid(state.owner));
}

fn sync(store: &mut EntityStore, key: EntityKey) {
    if let Some(data) = store.data_mut(key)
        && let Some(state) = data.mob
    {
        write_metadata(&mut data.metadata, &state);
    }
}

/// What a right-click on a wolf did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Used {
    /// Nothing; the item is not used up.
    No,
    /// A bone was used and the wolf stayed wild.
    Consumed,
    /// A bone was used and the wolf is tame now.
    Tamed,
    /// The owner made the wolf sit or stand.
    Toggled,
}

fn is_wolf_food(item: &str) -> bool {
    matches!(
        item.strip_prefix("minecraft:").unwrap_or(item),
        "beef"
            | "cooked_beef"
            | "porkchop"
            | "cooked_porkchop"
            | "chicken"
            | "cooked_chicken"
            | "mutton"
            | "cooked_mutton"
            | "rabbit"
            | "cooked_rabbit"
            | "rotten_flesh"
    )
}

/// Vanilla `Wolf.mobInteract`, taming and sitting only.
pub fn use_on(
    store: &mut EntityStore,
    key: EntityKey,
    item: &str,
    player: Uuid,
    rng: &mut Rng,
) -> Used {
    let Some(state) = mob::state_mut(store, key) else {
        return Used::No;
    };
    if state.kind != MobKind::Wolf {
        return Used::No;
    }
    let used = match state.owner {
        None if item == "minecraft:bone" => {
            if rng.next_int(3) == 0 {
                state.owner = Some(player);
                state.sitting = true;
                state.health = state.health.max(TAMED_HEALTH);
                Used::Tamed
            } else {
                Used::Consumed
            }
        }
        Some(owner) if owner == player && !is_wolf_food(item) => {
            state.sitting = !state.sitting;
            state.following = false;
            Used::Toggled
        }
        _ => Used::No,
    };
    sync(store, key);
    used
}

/// Where a tamed, standing wolf should walk to follow its owner, if it should. Far away it jumps
/// next to the owner instead.
pub fn follow_target(
    store: &mut EntityStore,
    key: EntityKey,
    owners: &[(Uuid, Vec3)],
    from: Vec3,
) -> Option<Vec3> {
    let state = mob::state(store, key).copied()?;
    let owner = state.owner?;
    if state.sitting {
        return None;
    }
    let (_, at) = owners.iter().find(|(uuid, _)| *uuid == owner)?;
    let distance = (at.x - from.x).powi(2) + (at.z - from.z).powi(2);
    let following = state.following && distance > STOP_DISTANCE_SQ || distance > START_DISTANCE_SQ;
    if let Some(state) = mob::state_mut(store, key) {
        state.following = following;
    }
    if distance >= TELEPORT_DISTANCE_SQ {
        store.set_pos(key, Vec3::new(at.x + 1.0, at.y, at.z));
        return None;
    }
    following.then_some(*at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wolf(store: &mut EntityStore, rng: &mut Rng) -> EntityKey {
        store.insert(mob::new_mob(MobKind::Wolf, Vec3::new(0.5, 64.0, 0.5), rng))
    }

    #[test]
    fn a_bone_tames_a_wolf_sooner_or_later_and_other_items_do_nothing() {
        let mut rng = Rng::new(3);
        let mut store = EntityStore::default();
        let key = wolf(&mut store, &mut rng);
        let player = Uuid::from_u128(1);
        assert_eq!(
            use_on(&mut store, key, "minecraft:stick", player, &mut rng),
            Used::No
        );
        let mut tries = 0;
        while use_on(&mut store, key, "minecraft:bone", player, &mut rng) != Used::Tamed {
            tries += 1;
            assert!(tries < 100);
        }
        let state = mob::state(&store, key).unwrap();
        assert_eq!(state.owner, Some(player));
        assert!(state.sitting);
        let meta = &store.data(key).unwrap().metadata;
        assert_eq!(
            meta.get(FLAGS_INDEX),
            Some(&MetaValue::Byte(SITTING | TAMED))
        );
        assert_eq!(
            meta.get(OWNER_INDEX),
            Some(&MetaValue::OptionalUuid(Some(player)))
        );
    }

    #[test]
    fn only_the_owner_toggles_sitting_and_food_does_not() {
        let mut rng = Rng::new(3);
        let mut store = EntityStore::default();
        let key = wolf(&mut store, &mut rng);
        let owner = Uuid::from_u128(1);
        let state = mob::state_mut(&mut store, key).unwrap();
        state.owner = Some(owner);
        state.sitting = true;
        let other = Uuid::from_u128(2);
        assert_eq!(
            use_on(&mut store, key, "minecraft:air", other, &mut rng),
            Used::No
        );
        assert_eq!(
            use_on(&mut store, key, "minecraft:beef", owner, &mut rng),
            Used::No
        );
        assert_eq!(
            use_on(&mut store, key, "minecraft:air", owner, &mut rng),
            Used::Toggled
        );
        assert!(!mob::state(&store, key).unwrap().sitting);
    }

    #[test]
    fn a_standing_tamed_wolf_follows_far_owners_and_teleports_to_very_far_ones() {
        let mut rng = Rng::new(3);
        let mut store = EntityStore::default();
        let key = wolf(&mut store, &mut rng);
        let owner = Uuid::from_u128(1);
        mob::state_mut(&mut store, key).unwrap().owner = Some(owner);
        let from = store.pos(key).unwrap();
        let near = [(owner, Vec3::new(3.0, 64.0, 0.5))];
        assert_eq!(follow_target(&mut store, key, &near, from), None);
        let far = [(owner, Vec3::new(11.0, 64.0, 0.5))];
        assert!(follow_target(&mut store, key, &far, from).is_some());
        // Once following it keeps going until it is close.
        let middle = [(owner, Vec3::new(5.0, 64.0, 0.5))];
        assert!(follow_target(&mut store, key, &middle, from).is_some());
        let very_far = [(owner, Vec3::new(50.0, 64.0, 0.5))];
        assert_eq!(follow_target(&mut store, key, &very_far, from), None);
        assert!((store.pos(key).unwrap().x - 51.0).abs() < 1e-9);
        mob::state_mut(&mut store, key).unwrap().sitting = true;
        assert_eq!(follow_target(&mut store, key, &far, from), None);
    }
}
