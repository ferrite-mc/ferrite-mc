//! Creeper state beyond the swell goal (vanilla `Creeper`): the charged (powered) state, its
//! doubled explosion, the hiss when the fuse starts and the synched swell / powered flags.
//!
//! Checked against the 26.3 jar: the synched data are `DATA_SWELL_DIR` (int, index 16),
//! `DATA_IS_POWERED` (index 17) and `DATA_IS_IGNITED` (index 18); `explodeCreeper` explodes with
//! `explosionRadius * (powered ? 2 : 1)` and the default radius is 3; `tick` plays
//! `creeper.primed` at volume 1, pitch 0.5 when the swell direction turns positive from a zero
//! swell; `thunderHit` makes a creeper powered. Not modelled: flint-and-steel ignition
//! (`ignited`), the head drop when a charged creeper kills, and the area effect cloud left by a
//! creeper that carries potion effects.

use crate::mob::{self, MobKind};
use crate::{EntityKey, EntityStore, Metadata};
use ferrite_protocol::packets::play::MetaValue;

pub const SWELL_DIR_INDEX: u8 = 16;
pub const POWERED_INDEX: u8 = 17;
pub const IGNITED_INDEX: u8 = 18;
/// `Creeper.explosionRadius`, doubled while powered.
pub const EXPLOSION_RADIUS: f32 = 3.0;
/// `SoundEvents.CREEPER_PRIMED`, played at volume 1 and pitch 0.5.
pub const HISS_SOUND: &str = "minecraft:entity.creeper.primed";
pub const HISS_VOLUME: f32 = 1.0;
pub const HISS_PITCH: f32 = 0.5;

/// Defines the swell direction, powered and ignited values on a creeper's synched data.
pub fn define_metadata(metadata: &mut Metadata) {
    metadata.define(SWELL_DIR_INDEX, MetaValue::Int(-1));
    metadata.define(POWERED_INDEX, MetaValue::Boolean(false));
    metadata.define(IGNITED_INDEX, MetaValue::Boolean(false));
}

/// Writes the swell direction (`1` while the fuse burns, `-1` otherwise) and powered flag from
/// the mob state.
pub fn sync(store: &mut EntityStore, key: EntityKey) {
    if let Some(data) = store.data_mut(key)
        && let Some(state) = data.mob
        && state.kind == MobKind::Creeper
    {
        data.metadata
            .set(SWELL_DIR_INDEX, MetaValue::Int(i32::from(state.swell_dir)));
        data.metadata
            .set(POWERED_INDEX, MetaValue::Boolean(state.powered));
    }
}

/// The explosion power of a creeper (`explosionRadius * (powered ? 2 : 1)`).
pub fn explosion_power(powered: bool) -> f32 {
    if powered {
        EXPLOSION_RADIUS * 2.0
    } else {
        EXPLOSION_RADIUS
    }
}

/// Vanilla `Creeper.thunderHit`: a lightning bolt charges the creeper. Returns whether the
/// state changed (a creeper that is already charged stays so).
pub fn thunder_hit(store: &mut EntityStore, key: EntityKey) -> bool {
    let Some(state) = mob::state_mut(store, key) else {
        return false;
    };
    if state.kind != MobKind::Creeper || state.powered {
        return false;
    }
    state.powered = true;
    sync(store, key);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rng;
    use crate::ai::{self, MobEvent, Target};
    use crate::physics::BlockGetter;
    use ferrite_math::Vec3;
    use uuid::Uuid;

    struct Flat;

    impl BlockGetter for Flat {
        fn block_state(&self, _x: i32, y: i32, _z: i32) -> Option<u16> {
            ferrite_data::blocks::default_state(if y < 64 { "stone" } else { "air" })
        }
    }

    fn setup() -> (EntityStore, EntityKey) {
        let mut store = EntityStore::default();
        let key = store.insert(mob::new_mob(
            MobKind::Creeper,
            Vec3::new(2.0, 64.0, 0.5),
            &mut Rng::new(5),
        ));
        (store, key)
    }

    fn player() -> [Target; 1] {
        [Target {
            uuid: Uuid::from_u128(1),
            pos: Vec3::new(0.5, 64.0, 0.5),
        }]
    }

    /// Ticks until the creeper explodes, returning the explosion power and the tick it
    /// happened on, plus how many hiss events were seen.
    fn burn(store: &mut EntityStore) -> (f32, usize, u32) {
        let mut rng = Rng::new(6);
        let mut hisses = 0;
        for tick in 1..=200 {
            for event in ai::tick_with(store, &Flat, &player(), &mut rng) {
                match event {
                    MobEvent::Hiss { .. } => hisses += 1,
                    MobEvent::Explode { power, .. } => return (power, hisses, tick),
                    _ => {}
                }
            }
        }
        panic!("the creeper never exploded");
    }

    #[test]
    fn an_uncharged_creeper_explodes_with_power_3_after_one_hiss() {
        let (mut store, _) = setup();
        let (power, hisses, _) = burn(&mut store);
        assert_eq!(power, 3.0);
        assert_eq!(hisses, 1, "the hiss plays once, when the fuse starts");
    }

    #[test]
    fn a_charged_creeper_explodes_with_double_power() {
        let (mut store, key) = setup();
        assert!(thunder_hit(&mut store, key));
        assert!(!thunder_hit(&mut store, key), "already charged");
        let (power, _, _) = burn(&mut store);
        assert_eq!(power, 6.0);
        assert_eq!(explosion_power(true), 2.0 * explosion_power(false));
    }

    #[test]
    fn a_lightning_strike_ignores_everything_that_is_not_a_creeper() {
        let mut store = EntityStore::default();
        let cow = store.insert(mob::new_mob(
            MobKind::Cow,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(1),
        ));
        assert!(!thunder_hit(&mut store, cow));
        assert!(!mob::state(&store, cow).unwrap().powered);
    }

    #[test]
    fn the_synched_flags_follow_the_fuse_and_the_charge() {
        let (mut store, key) = setup();
        let flag =
            |store: &EntityStore, index| store.data(key).unwrap().metadata.get(index).cloned();
        assert_eq!(flag(&store, SWELL_DIR_INDEX), Some(MetaValue::Int(-1)));
        assert_eq!(flag(&store, POWERED_INDEX), Some(MetaValue::Boolean(false)));
        thunder_hit(&mut store, key);
        assert_eq!(flag(&store, POWERED_INDEX), Some(MetaValue::Boolean(true)));
        // One tick inside three blocks starts the fuse.
        ai::tick_with(&mut store, &Flat, &player(), &mut Rng::new(6));
        assert_eq!(flag(&store, SWELL_DIR_INDEX), Some(MetaValue::Int(1)));
        // The target leaves: the swell direction turns back.
        let far = [Target {
            uuid: Uuid::from_u128(1),
            pos: Vec3::new(40.5, 64.0, 0.5),
        }];
        ai::tick_with(&mut store, &Flat, &far, &mut Rng::new(6));
        assert_eq!(flag(&store, SWELL_DIR_INDEX), Some(MetaValue::Int(-1)));
    }

    #[test]
    fn the_hiss_comes_again_when_the_fuse_winds_down_and_restarts() {
        let (mut store, _) = setup();
        let mut rng = Rng::new(6);
        let count = |events: Vec<MobEvent>| {
            events
                .iter()
                .filter(|event| matches!(event, MobEvent::Hiss { .. }))
                .count()
        };
        assert_eq!(
            count(ai::tick_with(&mut store, &Flat, &player(), &mut rng)),
            1
        );
        assert_eq!(
            count(ai::tick_with(&mut store, &Flat, &player(), &mut rng)),
            0
        );
        let far = [Target {
            uuid: Uuid::from_u128(1),
            pos: Vec3::new(40.5, 64.0, 0.5),
        }];
        for _ in 0..3 {
            ai::tick_with(&mut store, &Flat, &far, &mut rng);
        }
        assert_eq!(
            count(ai::tick_with(&mut store, &Flat, &player(), &mut rng)),
            1
        );
    }
}
