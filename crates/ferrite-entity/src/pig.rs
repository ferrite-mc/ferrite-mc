//! Pig riding & saddle steering (`Pig` + `ItemBasedSteering`, vanilla 26.3).
//!
//! In 26.3, `AgeableMob` owns metadata indices `16` (`DATA_BABY_ID`) and `17` (`AGE_LOCKED`),
//! `Pig` defines `DATA_BOOST_TIME` at index `18`, and the saddle lives in
//! [`crate::EquipmentSlot::Saddle`] rather than synched entity metadata.

use crate::mob::{self, MobKind};
use crate::{EntityKey, EntityStore, EquipmentSlot, Metadata};
use ferrite_math::Vec3;
use ferrite_protocol::packets::play::MetaValue;

/// `Pig.DATA_BOOST_TIME` (index 18 in 26.3).
pub const BOOST_TIME_INDEX: u8 = 18;
/// Base ridden speed (`Pig.getRiddenSpeed`: `movement_speed * 0.225`).
pub const RIDDEN_SPEED_FACTOR: f64 = 0.225;
/// Boost multiplier while carrot-on-a-stick boost is active.
pub const BOOST_MULTIPLIER: f64 = 1.15;

/// Defines pig boost-time metadata (saddle is tracked in [`EquipmentSlot::Saddle`]).
pub fn define_metadata(metadata: &mut Metadata) {
    metadata.define(BOOST_TIME_INDEX, MetaValue::Int(0));
}

/// Syncs pig saddle equipment from `MobState`.
pub fn sync(store: &mut EntityStore, key: EntityKey) {
    if let Some(data) = store.data_mut(key)
        && let Some(state) = data.mob
        && state.kind == MobKind::Pig
    {
        let saddle = state
            .saddled
            .then(|| ferrite_item::ItemStack::from_name("minecraft:saddle"))
            .flatten();
        data.equipment.set(EquipmentSlot::Saddle, saddle);
    }
}

/// Computes horizontal steering velocity for a saddled pig given rider `yaw` (degrees) and
/// whether carrot-on-a-stick boost is active.
pub fn steer_velocity(saddled: bool, yaw: f32, boost_active: bool) -> Vec3 {
    if !saddled {
        return Vec3::ZERO;
    }
    let rad = f64::from(yaw).to_radians();
    let base = MobKind::Pig.movement_speed() * RIDDEN_SPEED_FACTOR;
    let speed = if boost_active {
        base * BOOST_MULTIPLIER
    } else {
        base
    };
    Vec3::new(-rad.sin() * speed, 0.0, rad.cos() * speed)
}

/// Saddles or unsaddles a pig (`SaddleItem`). Returns `true` if state changed.
pub fn set_saddled(store: &mut EntityStore, key: EntityKey, saddled: bool) -> bool {
    let Some(state) = mob::state_mut(store, key) else {
        return false;
    };
    if state.kind != MobKind::Pig || state.saddled == saddled {
        return false;
    }
    state.saddled = saddled;
    sync(store, key);
    if saddled && let Some(data) = store.data_mut(key) {
        data.equipment.set_guaranteed_drop(EquipmentSlot::Saddle);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rng;

    #[test]
    fn saddled_pig_steers_and_boosts_while_unsaddled_does_not() {
        let mut store = EntityStore::default();
        let key = store.insert(mob::new_mob(
            MobKind::Pig,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(1),
        ));
        assert_eq!(steer_velocity(false, 0.0, false), Vec3::ZERO);
        assert!(set_saddled(&mut store, key, true));
        assert_eq!(
            store
                .data(key)
                .unwrap()
                .equipment
                .get(EquipmentSlot::Saddle)
                .and_then(|s| s.name()),
            Some("minecraft:saddle")
        );
        assert_eq!(
            store
                .data(key)
                .unwrap()
                .equipment
                .drop_chance(EquipmentSlot::Saddle),
            crate::equipment::GUARANTEED_DROP_CHANCE
        );
        // Saddle must not collide with `AgeableMob.DATA_BABY_ID` (index 16).
        assert_eq!(
            store
                .data(key)
                .unwrap()
                .metadata
                .get(crate::breed::BABY_INDEX),
            Some(&MetaValue::Boolean(false))
        );
        let normal = steer_velocity(true, 0.0, false);
        let boosted = steer_velocity(true, 0.0, true);
        assert!(normal.z > 0.0);
        assert!(boosted.z > normal.z);
    }
}
