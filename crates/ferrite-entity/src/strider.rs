//! Strider riding and warped-fungus steering (vanilla `Strider` + `ItemBasedSteering`, 26.3).
//!
//! A saddled adult strider is steered by a warped fungus on a stick, like a pig with a carrot
//! on a stick: the boost flag is `Strider.DATA_BOOST_TIME` (metadata index 18) and the ridden
//! speed is the strider's movement speed scaled by `Strider.getRiddenSpeed`.

use crate::mob::{self, MobKind};
use crate::{EntityKey, EntityStore, EquipmentSlot};
use ferrite_math::Vec3;
use ferrite_protocol::packets::play::MetaValue;

/// `Strider.DATA_BOOST_TIME` (index 18 in 26.3).
pub const BOOST_TIME_INDEX: u8 = crate::metadata::strider::BOOST_TIME;
/// Shortest warped-fungus boost (`ItemBasedSteering.boost`).
pub const BOOST_TICKS_MIN: i32 = 140;
/// Longest warped-fungus boost (`ItemBasedSteering.boost`).
pub const BOOST_TICKS_MAX: i32 = 980;
/// Vanilla's fixed minimum boost length, kept for callers that do not roll a duration.
pub const BOOST_TICKS: i32 = BOOST_TICKS_MIN;

/// `ItemBasedSteering.boost`: a boost lasts `140..=980` ticks, rolled per use.
pub fn roll_boost_ticks(rng: &mut crate::Rng) -> i32 {
    BOOST_TICKS_MIN + rng.next_int(BOOST_TICKS_MAX - BOOST_TICKS_MIN + 1)
}
/// `Strider.getRiddenSpeed`: the movement speed is scaled by this while ridden.
pub const RIDDEN_SPEED_FACTOR: f64 = 0.55;
/// Extra speed while the boost is active.
pub const BOOST_MULTIPLIER: f64 = 1.2;

/// Syncs strider saddle equipment from `MobState`.
pub fn sync(store: &mut EntityStore, key: EntityKey) {
    if let Some(data) = store.data_mut(key)
        && let Some(state) = data.mob
        && state.kind == MobKind::Strider
    {
        let saddle = state
            .saddled
            .then(|| ferrite_item::ItemStack::from_name("minecraft:saddle"))
            .flatten();
        data.equipment.set(EquipmentSlot::Saddle, saddle);
    }
}

/// Computes horizontal steering velocity for a saddled strider given rider `yaw` (degrees) and
/// whether the warped-fungus boost is active.
pub fn steer_velocity(saddled: bool, yaw: f32, boost_active: bool) -> Vec3 {
    if !saddled {
        return Vec3::ZERO;
    }
    let rad = f64::from(yaw).to_radians();
    let base = MobKind::Strider.movement_speed() * RIDDEN_SPEED_FACTOR;
    let speed = if boost_active {
        base * BOOST_MULTIPLIER
    } else {
        base
    };
    Vec3::new(-rad.sin() * speed, 0.0, rad.cos() * speed)
}

/// Saddles or unsaddles a strider (`SaddleItem`). Returns `true` if the state changed.
pub fn set_saddled(store: &mut EntityStore, key: EntityKey, saddled: bool) -> bool {
    let Some(state) = mob::state_mut(store, key) else {
        return false;
    };
    if state.kind != MobKind::Strider || state.saddled == saddled {
        return false;
    }
    state.saddled = saddled;
    sync(store, key);
    if saddled && let Some(data) = store.data_mut(key) {
        data.equipment.set_guaranteed_drop(EquipmentSlot::Saddle);
    }
    true
}

/// Starts a warped-fungus boost on a strider.
pub fn boost(store: &mut EntityStore, key: EntityKey) -> bool {
    let Some(data) = store.data_mut(key) else {
        return false;
    };
    if data.mob.is_none_or(|state| state.kind != MobKind::Strider) {
        return false;
    }
    data.metadata
        .set(BOOST_TIME_INDEX, MetaValue::Int(BOOST_TICKS_MIN));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rng;

    #[test]
    fn a_saddled_strider_steers_and_boosts() {
        let mut store = EntityStore::default();
        let key = store.insert(mob::new_mob(
            MobKind::Strider,
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
        assert!(boost(&mut store, key));
        assert!(matches!(
            store.data(key).unwrap().metadata.get(BOOST_TIME_INDEX),
            Some(MetaValue::Int(BOOST_TICKS_MIN))
        ));
        let mut rng = Rng::new(7);
        let mut saw_long = false;
        for _ in 0..64 {
            let ticks = roll_boost_ticks(&mut rng);
            assert!(
                (BOOST_TICKS_MIN..=BOOST_TICKS_MAX).contains(&ticks),
                "{ticks}"
            );
            saw_long |= ticks > BOOST_TICKS_MIN;
        }
        assert!(saw_long, "the boost length is rolled per use");
        let normal = steer_velocity(true, 0.0, false);
        let boosted = steer_velocity(true, 0.0, true);
        assert!(normal.z > 0.0);
        assert!(boosted.z > normal.z);
    }
}
