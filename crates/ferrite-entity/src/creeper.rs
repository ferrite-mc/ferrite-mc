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
        data.metadata
            .set(IGNITED_INDEX, MetaValue::Boolean(state.ignited));
    }
}

/// Vanilla `Creeper.ignite`: ignites a creeper (flint-and-steel) so its fuse burns to
/// explosion regardless of target distance. Returns `true` if newly ignited.
pub fn ignite(store: &mut EntityStore, key: EntityKey) -> bool {
    let Some(state) = mob::state_mut(store, key) else {
        return false;
    };
    if state.kind != MobKind::Creeper || state.ignited || state.health <= 0.0 {
        return false;
    }
    state.ignited = true;
    state.swell_dir = 1;
    sync(store, key);
    true
}

/// Vanilla `Creeper.killedEntity` skull drop when a charged creeper explosion kills `victim`.
pub fn charged_skull_drop(powered: bool, victim: MobKind) -> Option<&'static str> {
    if !powered {
        return None;
    }
    match victim {
        MobKind::Skeleton => Some("minecraft:skeleton_skull"),
        MobKind::WitherSkeleton => Some("minecraft:wither_skeleton_skull"),
        MobKind::Zombie => Some("minecraft:zombie_head"),
        MobKind::Creeper => Some("minecraft:creeper_head"),
        MobKind::Piglin => Some("minecraft:piglin_head"),
        _ => None,
    }
}

/// Distance inside which a creeper flees from a cat (`AvoidEntityGoal`, 6 blocks).
pub const CAT_AVOID_DISTANCE: f64 = 6.0;

/// Returns the normalized horizontal flee direction away from the nearest cat within
/// [`CAT_AVOID_DISTANCE`], or `None` if no cat is nearby.
pub fn flee_cat_direction(
    pos: ferrite_math::Vec3,
    cats: &[ferrite_math::Vec3],
) -> Option<ferrite_math::Vec3> {
    let mut nearest_dist_sq = CAT_AVOID_DISTANCE * CAT_AVOID_DISTANCE;
    let mut away = None;
    for &cat in cats {
        let dx = pos.x - cat.x;
        let dy = pos.y - cat.y;
        let dz = pos.z - cat.z;
        let d2 = dx * dx + dy * dy + dz * dz;
        if d2 < nearest_dist_sq {
            nearest_dist_sq = d2;
            let horiz = ferrite_math::Vec3::new(dx, 0.0, dz);
            if horiz.length_sqr() > 1.0e-7 {
                away = Some(horiz.normalize());
            } else {
                away = Some(ferrite_math::Vec3::new(1.0, 0.0, 0.0));
            }
        }
    }
    away
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

    #[test]
    fn flint_and_steel_ignition_cat_avoidance_and_charged_skull_drops() {
        let (mut store, key) = setup();
        assert!(ignite(&mut store, key));
        assert!(!ignite(&mut store, key), "already ignited");
        assert_eq!(
            store.data(key).unwrap().metadata.get(IGNITED_INDEX),
            Some(&MetaValue::Boolean(true))
        );
        let far = [Target {
            uuid: Uuid::from_u128(1),
            pos: Vec3::new(40.5, 64.0, 0.5),
        }];
        let mut rng = Rng::new(7);
        let mut exploded = false;
        for _ in 0..ai::CREEPER_SWELL_TICKS {
            if ai::tick_with(&mut store, &Flat, &far, &mut rng)
                .iter()
                .any(|e| matches!(e, MobEvent::Explode { .. }))
            {
                exploded = true;
                break;
            }
        }
        assert!(exploded, "ignited creeper explodes without a nearby player");

        let away = flee_cat_direction(Vec3::new(2.0, 64.0, 0.0), &[Vec3::new(0.0, 64.0, 0.0)]);
        assert!(away.is_some_and(|v| v.x > 0.9));
        assert!(
            flee_cat_direction(Vec3::new(10.0, 64.0, 0.0), &[Vec3::new(0.0, 64.0, 0.0)]).is_none()
        );

        assert_eq!(
            charged_skull_drop(true, MobKind::Skeleton),
            Some("minecraft:skeleton_skull")
        );
        assert_eq!(charged_skull_drop(false, MobKind::Skeleton), None);
    }

    #[test]
    fn creeper_with_active_effect_emits_potion_cloud_on_explosion() {
        let (mut store, key) = setup();
        store
            .data_mut(key)
            .unwrap()
            .effects
            .add(crate::effect::MobEffectInstance::new(
                crate::effect::Effect::Speed.id(),
                200,
                0,
            ));
        assert!(ignite(&mut store, key));
        let far = [Target {
            uuid: Uuid::from_u128(1),
            pos: Vec3::new(40.5, 64.0, 0.5),
        }];
        let mut rng = Rng::new(8);
        let mut saw_cloud = false;
        for _ in 0..ai::CREEPER_SWELL_TICKS {
            let events = ai::tick_with(&mut store, &Flat, &far, &mut rng);
            if events.iter().any(|e| {
                matches!(
                    e,
                    MobEvent::CreeperPotionCloud { effects, .. }
                        if effects.iter().any(|ef| ef.effect == crate::effect::Effect::Speed.id())
                )
            }) {
                saw_cloud = true;
                break;
            }
        }
        assert!(
            saw_cloud,
            "exploding creeper emits its potion effects as a cloud"
        );
    }
}
