//! Spider behaviours beyond the chase (vanilla `Spider`): wall climbing, the leap at a target and
//! the skeleton jockey.
//!
//! Checked against the 26.3 jar: `Spider.tick` sets the synched climbing flag (`DATA_FLAGS_ID`,
//! byte index 16, bit 1) to `horizontalCollision`; a climbing entity pushed into a wall rises at
//! 0.2 per tick and otherwise falls no faster than 0.15; `LeapAtTargetGoal(0.4)` starts when the
//! target is 2 to 4 blocks away (`distanceToSqr` 4 to 16), the spider is on the ground, carries no
//! controlling passenger and `nextInt(5) == 0`, then sets the motion to
//! `normalize(horizontal delta) * 0.4 + motion * 0.2` with `y = 0.4`; `finalizeSpawn` makes one
//! spider in a hundred a skeleton jockey.
//!
//! Not modelled: `WallClimberNavigation` (the block-grid A* cannot path up a wall, so a spider
//! only climbs what it walks into), the random potion effect a spawned spider may receive, and the
//! jockey's bow and enchantments.

use crate::Rng;
use crate::{EntityKey, EntityStore, Metadata};
use ferrite_math::Vec3;
use ferrite_protocol::packets::play::MetaValue;

pub const FLAGS_INDEX: u8 = 16;
/// Upward speed of a spider pushed against a wall (`handleOnClimbable`).
pub const CLIMB_SPEED: f64 = 0.2;
/// The slowest a climbing entity falls.
pub const CLIMB_FALL_LIMIT: f64 = -0.15;
/// `LeapAtTargetGoal(0.4)`: the vertical and horizontal speed of the leap.
pub const LEAP_POWER: f64 = 0.4;
/// One spider in this many is a jockey.
pub const JOCKEY_ONE_IN: i32 = 100;

/// Defines the spider flags byte on synched data.
pub fn define_metadata(metadata: &mut Metadata) {
    metadata.define(FLAGS_INDEX, MetaValue::Byte(0));
}

/// Writes the climbing bit.
pub fn set_climbing(store: &mut EntityStore, key: EntityKey, climbing: bool) {
    if let Some(data) = store.data_mut(key) {
        data.metadata
            .set(FLAGS_INDEX, MetaValue::Byte(i8::from(climbing)));
    }
}

/// The vertical speed of a spider that is `climbing` (against a wall) and whether it is pushing
/// into it (`walking`): up at [`CLIMB_SPEED`], else no faster than the fall limit.
pub fn climb_velocity(y: f64, walking: bool) -> f64 {
    if walking {
        CLIMB_SPEED
    } else {
        y.max(CLIMB_FALL_LIMIT)
    }
}

/// `LeapAtTargetGoal.canUse` on the distance and ground, before the random roll.
pub fn can_leap(distance_sqr: f64, on_ground: bool, has_rider: bool) -> bool {
    on_ground && !has_rider && (4.0..=16.0).contains(&distance_sqr)
}

/// The one-in-five roll of `LeapAtTargetGoal.canUse`.
pub fn leap_roll(rng: &mut Rng) -> bool {
    rng.next_int(5) == 0
}

/// `LeapAtTargetGoal.start`: the new motion toward `target`.
pub fn leap_velocity(vel: Vec3, pos: Vec3, target: Vec3) -> Vec3 {
    let delta = Vec3::new(target.x - pos.x, 0.0, target.z - pos.z);
    let (x, z) = if delta.length_sqr() > 1.0E-7 {
        let aim = delta.normalize();
        (
            aim.x * LEAP_POWER + vel.x * 0.2,
            aim.z * LEAP_POWER + vel.z * 0.2,
        )
    } else {
        (vel.x, vel.z)
    };
    Vec3::new(x, LEAP_POWER, z)
}

/// `Spider.finalizeSpawn`: whether this spawn carries a skeleton jockey.
pub fn jockey_roll(rng: &mut Rng) -> bool {
    rng.next_int(JOCKEY_ONE_IN) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leap_needs_two_to_four_blocks_ground_and_no_rider() {
        assert!(can_leap(4.0, true, false));
        assert!(can_leap(16.0, true, false));
        assert!(!can_leap(3.9, true, false));
        assert!(!can_leap(16.1, true, false));
        assert!(!can_leap(9.0, false, false));
        assert!(!can_leap(9.0, true, true));
    }

    #[test]
    fn the_leap_points_at_the_target_and_adds_a_fifth_of_the_old_motion() {
        let v = leap_velocity(
            Vec3::new(0.1, -0.3, 0.0),
            Vec3::new(0.0, 64.0, 0.0),
            Vec3::new(3.0, 64.0, 0.0),
        );
        assert!((v.x - (0.4 + 0.02)).abs() < 1e-9);
        assert!(v.z.abs() < 1e-9);
        assert_eq!(v.y, 0.4);
    }

    #[test]
    fn about_one_spawn_in_a_hundred_is_a_jockey() {
        let mut rng = Rng::new(3);
        let jockeys = (0..100_000).filter(|_| jockey_roll(&mut rng)).count();
        assert!((800..1200).contains(&jockeys), "{jockeys}");
    }

    #[test]
    fn climbing_rises_when_pushing_and_slides_slowly_otherwise() {
        assert_eq!(climb_velocity(-1.0, true), CLIMB_SPEED);
        assert_eq!(climb_velocity(-1.0, false), CLIMB_FALL_LIMIT);
        assert_eq!(climb_velocity(-0.05, false), -0.05);
    }

    use crate::ai::{self, MobRules, Target};
    use crate::mob::{self, MobKind};
    use crate::physics::tests::TestWorld;
    use uuid::Uuid;

    fn wall() -> TestWorld {
        let mut world = TestWorld::flat(64);
        let stone = ferrite_data::blocks::default_state("stone").unwrap();
        for z in -40..40 {
            for y in 64..72 {
                world.blocks.insert((3, y, z), stone);
            }
        }
        world
    }

    fn spider_at(x: f64) -> (EntityStore, EntityKey) {
        let mut store = EntityStore::default();
        let key = store.insert(mob::new_mob(
            MobKind::Spider,
            Vec3::new(x, 64.0, 0.5),
            &mut Rng::new(1),
        ));
        (store, key)
    }

    fn player(x: f64) -> Target {
        Target {
            uuid: Uuid::from_u128(5),
            pos: Vec3::new(x, 64.0, 0.5),
        }
    }

    #[test]
    fn a_spider_chasing_a_player_over_a_wall_climbs_it() {
        let world = wall();
        let (mut store, key) = spider_at(1.5);
        let mut rng = Rng::new(9);
        let mut peak: f64 = 0.0;
        let mut flagged = false;
        for _ in 0..120 {
            ai::tick_full(
                &mut store,
                &world,
                &[player(8.5)],
                &MobRules::default(),
                &[],
                &[],
                &mut rng,
            );
            peak = peak.max(store.pos(key).unwrap().y);
            flagged |= store
                .data(key)
                .and_then(|d| d.metadata.get(FLAGS_INDEX))
                .is_some_and(|v| matches!(v, MetaValue::Byte(b) if b & 1 != 0));
        }
        assert!(peak > 66.0, "the spider climbed the wall: {peak}");
        assert!(flagged, "the climbing flag was synched");
    }

    #[test]
    fn a_spider_in_leap_range_pounces_toward_the_player() {
        let world = TestWorld::flat(64);
        let (mut store, key) = spider_at(0.5);
        let mut rng = Rng::new(9);
        let mut peak: f64 = 0.0;
        for _ in 0..60 {
            ai::tick_full(
                &mut store,
                &world,
                &[player(3.5)],
                &MobRules::default(),
                &[],
                &[],
                &mut rng,
            );
            peak = peak.max(store.pos(key).unwrap().y - 64.0);
        }
        assert!(peak > 0.5, "the spider left the ground by {peak}");
    }

    #[test]
    fn a_spider_with_a_rider_does_not_leap() {
        let world = TestWorld::flat(64);
        let (mut store, key) = spider_at(0.5);
        mob::state_mut(&mut store, key).unwrap().has_rider = true;
        let mut rng = Rng::new(9);
        for _ in 0..60 {
            ai::tick_full(
                &mut store,
                &world,
                &[player(3.5)],
                &MobRules::default(),
                &[],
                &[],
                &mut rng,
            );
            assert!(store.pos(key).unwrap().y - 64.0 < 0.3);
        }
    }
}
