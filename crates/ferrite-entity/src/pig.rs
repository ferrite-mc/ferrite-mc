//! Pig riding & saddle steering (`Pig` + `ItemBasedSteering`, vanilla 26.3).

use crate::mob::{self, MobKind};
use crate::{EntityKey, EntityStore, Metadata};
use ferrite_math::Vec3;
use ferrite_protocol::packets::play::MetaValue;

/// `Pig.DATA_SADDLE_ID` (index 16; `AgeableMob` owns 16? No: `Pig` defines saddle at 16, boost at 17).
pub const SADDLE_INDEX: u8 = 16;
pub const BOOST_TIME_INDEX: u8 = 17;
/// Base ridden speed (`Pig.getRiddenSpeed`: `movement_speed * 0.225`).
pub const RIDDEN_SPEED_FACTOR: f64 = 0.225;
/// Boost multiplier while carrot-on-a-stick boost is active.
pub const BOOST_MULTIPLIER: f64 = 1.15;

/// Defines pig saddle & boost-time metadata.
pub fn define_metadata(metadata: &mut Metadata, saddled: bool) {
    metadata.define(SADDLE_INDEX, MetaValue::Boolean(saddled));
    metadata.define(BOOST_TIME_INDEX, MetaValue::Int(0));
}

/// Syncs pig saddle flag from `MobState`.
pub fn sync(store: &mut EntityStore, key: EntityKey) {
    if let Some(data) = store.data_mut(key)
        && let Some(state) = data.mob
        && state.kind == MobKind::Pig
    {
        data.metadata
            .set(SADDLE_INDEX, MetaValue::Boolean(state.saddled));
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
            store.data(key).unwrap().metadata.get(SADDLE_INDEX),
            Some(&MetaValue::Boolean(true))
        );
        let normal = steer_velocity(true, 0.0, false);
        let boosted = steer_velocity(true, 0.0, true);
        assert!(normal.z > 0.0);
        assert!(boosted.z > normal.z);
    }
}
