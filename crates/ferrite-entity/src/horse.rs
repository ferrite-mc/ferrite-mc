//! Horses (`AbstractHorse`): an untamed horse can be ridden, bucks its rider off now and then
//! and is tamed with a chance that grows with each ride (its temper); a tamed horse takes a
//! saddle and is then steered by its rider. shortcut: only the plain horse (no coat variants or
//! markings, no donkeys, mules or llamas), no armour, no inventory, no jump strength or speed
//! genetics, and no eating of treats.

use crate::mob::{self, MobKind, MobState};
use crate::{EntityKey, EntityStore, Metadata, Rng};
use ferrite_protocol::packets::play::MetaValue;
use uuid::Uuid;

/// Synched `AbstractHorse.DATA_ID_FLAGS` (read from the 26.3 server classes).
pub const FLAGS_INDEX: u8 = 17;
/// Synched `Horse.DATA_ID_TYPE_VARIANT`.
pub const VARIANT_INDEX: u8 = 18;
const TAME: i8 = 2;
/// `AbstractHorse.getMaxTemper`.
const MAX_TEMPER: i32 = 100;
/// Temper gained by every ride that did not tame (`RunAroundLikeCrazyGoal`).
const TEMPER_STEP: i32 = 5;
/// The `EquipmentSlot` index of the saddle in a `set_equipment` packet.
pub const SADDLE_SLOT: u8 = 7;

/// Defines the horse data values and writes the current state into them.
pub fn define_metadata(metadata: &mut Metadata, state: &MobState) {
    metadata.define(FLAGS_INDEX, MetaValue::Byte(0));
    metadata.define(VARIANT_INDEX, MetaValue::Int(0));
    write_metadata(metadata, state);
}

fn write_metadata(metadata: &mut Metadata, state: &MobState) {
    let flags = if state.owner.is_some() { TAME } else { 0 };
    metadata.set(FLAGS_INDEX, MetaValue::Byte(flags));
}

fn sync(store: &mut EntityStore, key: EntityKey) {
    if let Some(data) = store.data_mut(key)
        && let Some(state) = data.mob
    {
        write_metadata(&mut data.metadata, &state);
    }
}

/// Whether the horse is tame and saddled, so its rider steers it.
pub fn is_steerable(store: &EntityStore, key: EntityKey) -> bool {
    mob::state(store, key)
        .is_some_and(|s| s.kind == MobKind::Horse && s.owner.is_some() && s.saddled)
}

/// Puts a saddle on a tame, unsaddled horse. `false` when it does not apply.
pub fn saddle(store: &mut EntityStore, key: EntityKey) -> bool {
    match mob::state_mut(store, key) {
        Some(s) if s.kind == MobKind::Horse && s.owner.is_some() && !s.saddled => {
            s.saddled = true;
            true
        }
        _ => false,
    }
}

/// What an untamed horse with a rider did this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucked {
    /// The rider stays on.
    No,
    /// The horse threw the rider off and stayed wild.
    Wild,
    /// The horse accepted the rider and is tame now.
    Tamed,
}

/// `RunAroundLikeCrazyGoal`, shortened: now and then an untamed horse with a rider rolls its
/// temper against the maximum. It is tamed on success, otherwise the temper grows; either way
/// the rider falls off. shortcut: a plain 1-in-50 chance per tick instead of the goal's timing.
pub fn tick_rider(store: &mut EntityStore, key: EntityKey, rider: Uuid, rng: &mut Rng) -> Bucked {
    let Some(state) = mob::state_mut(store, key) else {
        return Bucked::No;
    };
    if state.kind != MobKind::Horse || state.owner.is_some() || rng.next_int(50) != 0 {
        return Bucked::No;
    }
    if rng.next_int(MAX_TEMPER) < state.temper {
        state.owner = Some(rider);
        sync(store, key);
        Bucked::Tamed
    } else {
        state.temper += TEMPER_STEP;
        Bucked::Wild
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mob::new_mob;
    use ferrite_math::Vec3;

    fn horse(store: &mut EntityStore) -> EntityKey {
        store.insert(new_mob(
            MobKind::Horse,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(1),
        ))
    }

    #[test]
    fn a_wild_horse_is_tamed_after_enough_rides_and_then_takes_a_saddle() {
        let mut store = EntityStore::default();
        let key = horse(&mut store);
        let rider = Uuid::from_u128(5);
        assert!(!saddle(&mut store, key), "a wild horse takes no saddle");
        let mut rng = Rng::new(7);
        let mut tamed = false;
        for _ in 0..200_000 {
            if tick_rider(&mut store, key, rider, &mut rng) == Bucked::Tamed {
                tamed = true;
                break;
            }
        }
        assert!(tamed);
        assert_eq!(mob::state(&store, key).unwrap().owner, Some(rider));
        assert_eq!(
            store.data(key).unwrap().metadata.get(FLAGS_INDEX),
            Some(&MetaValue::Byte(TAME))
        );
        assert!(!is_steerable(&store, key));
        assert!(saddle(&mut store, key));
        assert!(!saddle(&mut store, key));
        assert!(is_steerable(&store, key));
    }
}
