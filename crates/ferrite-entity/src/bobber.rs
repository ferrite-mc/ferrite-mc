//! Fishing bobber (vanilla `FishingHook`): cast, fly, float, bite, hook an entity, reel in and
//! despawn.
//!
//! The constants and the bite timeline follow vanilla's `FishingHook` as remembered, not as
//! checked against the 26.3 jar: gravity 0.03, a 0.92 drag, a 100 to 600 tick wait that each
//! level of Lure shortens by 100 ticks, a 20 to 80 tick approach and a 20 to 40 tick nibble.
//! Rain, sky light and open-water checks are not modelled, and luck is not applied to the
//! catch. Players cannot be hooked. The rod animation is client-only and is not checked here.
//!
//! Vanilla marks the bobber as not serializable, so it is never saved; the region drops it
//! when its owner is gone.

use crate::metadata::fishing_hook as meta;
use crate::mob;
use crate::physics::{self, BlockGetter};
use crate::rng::Rng;
use crate::store::{EntityData, EntityRecord, EntityStore, Flags};
use crate::{EntityKey, allocate_id, random_uuid};
use ferrite_math::Vec3;
use ferrite_protocol::packets::play::MetaValue;
use std::sync::OnceLock;
use uuid::Uuid;

/// Ticks a bobber may rest on the ground before it is discarded.
pub const GROUND_LIFETIME: i32 = 1200;
/// Squared distance from the owner past which a bobber is discarded (`FishingHook` 32 blocks).
pub const MAX_DISTANCE_SQ: f64 = 1024.0;
const GRAVITY: f64 = 0.03;
const DRAG: f64 = 0.92;
const SIZE: f64 = 0.25;
const SPEED: f64 = 1.5;
const WAIT_MIN: i32 = 100;
const WAIT_SPAN: i32 = 500;
const LURE_TICKS: i32 = 100;
const MIN_WAIT: i32 = 20;
const APPROACH_MIN: i32 = 20;
const APPROACH_SPAN: i32 = 60;
const NIBBLE_MIN: i32 = 20;
const NIBBLE_SPAN: i32 = 20;
/// Rod damage of a plain retrieve, a fish or junk, a pulled entity and a bobber on the ground.
pub const DAMAGE_CATCH: i32 = 1;
pub const DAMAGE_ENTITY: i32 = 5;
pub const DAMAGE_GROUND: i32 = 2;

/// Where a bobber is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Flying or resting on land.
    Flying,
    /// Floating in water, waiting for a bite.
    Bobbing,
    /// Hooked into the entity with this network id.
    Hooked(i32),
}

/// The bobber fields that are not synched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BobberState {
    pub owner: Uuid,
    pub phase: Phase,
    /// Ticks spent resting on the ground.
    pub grounded: i32,
    /// Ticks left of the nibble; positive while a fish is biting.
    pub nibble: i32,
    /// Ticks until a fish notices the bait; zero means none is scheduled.
    pub time_until_lured: i32,
    /// Ticks until the approaching fish bites; zero means none is approaching.
    pub time_until_hooked: i32,
    /// The rod's Lure level.
    pub lure: i32,
}

/// Entity type id of `minecraft:fishing_bobber`.
pub fn bobber_type() -> u16 {
    static KIND: OnceLock<u16> = OnceLock::new();
    *KIND.get_or_init(|| {
        ferrite_data::entity_props::by_name("fishing_bobber")
            .expect("vanilla has a fishing_bobber entity type")
            .id
    })
}

/// A bobber cast from `eye` along `yaw` and `pitch` (degrees) by the player `owner`, whose
/// network id goes in the spawn packet.
pub fn cast(
    owner: Uuid,
    owner_id: i32,
    eye: Vec3,
    yaw: f32,
    pitch: f32,
    lure: i32,
    rng: &mut Rng,
) -> EntityRecord {
    let rad = std::f64::consts::PI / 180.0;
    let (sin_yaw, cos_yaw) = (f64::from(yaw) * rad).sin_cos();
    let (sin_pitch, cos_pitch) = (f64::from(pitch) * rad).sin_cos();
    let aim = Vec3::new(-sin_yaw * cos_pitch, -sin_pitch, cos_yaw * cos_pitch);
    // A little spread, as the cast is never perfectly straight.
    let vel = Vec3::new(
        aim.x * SPEED + rng.range(-0.0103, 0.0103),
        aim.y * SPEED + rng.range(-0.0103, 0.0103),
        aim.z * SPEED + rng.range(-0.0103, 0.0103),
    );
    let mut data = EntityData::new(allocate_id(), random_uuid(), bobber_type(), SIZE, SIZE);
    data.metadata.define(meta::HOOKED_ENTITY, MetaValue::Int(0));
    data.metadata
        .define(meta::BITING, MetaValue::Boolean(false));
    data.spawn_data = owner_id;
    data.yaw = yaw;
    data.pitch = pitch;
    data.bobber = Some(BobberState {
        owner,
        phase: Phase::Flying,
        grounded: 0,
        nibble: 0,
        time_until_lured: 0,
        time_until_hooked: 0,
        lure: lure.clamp(0, 5),
    });
    EntityRecord {
        pos: Vec3::new(
            eye.x + aim.x * 0.3,
            eye.y - 0.1 + aim.y * 0.3,
            eye.z + aim.z * 0.3,
        ),
        vel,
        flags: Flags::default(),
        data,
    }
}

/// Key of the bobber `owner` has cast, if any.
pub fn find_for(store: &EntityStore, owner: Uuid) -> Option<EntityKey> {
    store.keys().iter().copied().find(|key| {
        store
            .data(*key)
            .and_then(|data| data.bobber)
            .is_some_and(|state| state.owner == owner)
            && store
                .flags(*key)
                .is_some_and(|flags| !flags.has(Flags::REMOVED))
    })
}

/// Flags a bobber removed.
pub fn discard(store: &mut EntityStore, key: EntityKey) {
    if let Some(flags) = store.flags_mut(key) {
        flags.set(Flags::REMOVED, true);
    }
}

/// Whether a fish is biting.
pub fn is_biting(data: &EntityData) -> bool {
    data.bobber.is_some_and(|state| state.nibble > 0)
}

fn sync(store: &mut EntityStore, key: EntityKey) {
    let Some(data) = store.data_mut(key) else {
        return;
    };
    let Some(state) = data.bobber else { return };
    let hooked = match state.phase {
        Phase::Hooked(id) => id + 1,
        _ => 0,
    };
    data.metadata
        .set(meta::HOOKED_ENTITY, MetaValue::Int(hooked));
    data.metadata
        .set(meta::BITING, MetaValue::Boolean(state.nibble > 0));
}

/// Advances one bobber one tick. Returns `false` once it was discarded.
pub fn tick(
    store: &mut EntityStore,
    key: EntityKey,
    blocks: &impl BlockGetter,
    rng: &mut Rng,
) -> bool {
    let Some(mut state) = store.data(key).and_then(|data| data.bobber) else {
        return false;
    };
    if store
        .flags(key)
        .is_none_or(|flags| flags.has(Flags::REMOVED))
    {
        return false;
    }
    if let Phase::Hooked(id) = state.phase {
        let target = store.key_of(id).filter(|k| mob::is_mob(store, *k));
        match target.and_then(|k| Some((store.pos(k)?, store.data(k)?.height))) {
            Some((pos, height)) => {
                store.set_pos(key, Vec3::new(pos.x, pos.y + height * 0.8, pos.z));
                store.set_vel(key, Vec3::ZERO);
                store_state(store, key, state);
                sync(store, key);
                return true;
            }
            None => state.phase = Phase::Flying,
        }
    }
    let Some(bbox) = store.bbox(key) else {
        return false;
    };
    let wet = physics::fluid_height(blocks, bbox, false) > 0.0;
    let mut vel = store.vel(key).unwrap_or_default();
    match (state.phase, wet) {
        (Phase::Flying, true) => {
            vel = Vec3::new(vel.x * 0.3, vel.y * 0.2, vel.z * 0.3);
            state.phase = Phase::Bobbing;
        }
        (Phase::Bobbing, false) => state.phase = Phase::Flying,
        _ => {}
    }
    match state.phase {
        Phase::Bobbing => {
            // Buoyancy: rise while submerged deeper than the bobber's height, sink above.
            let depth = physics::fluid_height(blocks, bbox, false);
            vel.y += (depth - SIZE * 0.5).clamp(-0.05, 0.05) * 0.5;
            vel = Vec3::new(vel.x * 0.9, vel.y * 0.9, vel.z * 0.9);
            catch_fish(&mut state, rng);
        }
        _ => {
            vel.y -= GRAVITY;
            vel = Vec3::new(vel.x * DRAG, vel.y * DRAG, vel.z * DRAG);
            state.nibble = 0;
            state.time_until_lured = 0;
            state.time_until_hooked = 0;
        }
    }
    store.set_vel(key, vel);
    physics::move_entity(store, key, vel, blocks);
    if state.phase == Phase::Flying {
        let grounded = store
            .flags(key)
            .is_some_and(|flags| flags.has(Flags::ON_GROUND));
        state.grounded = if grounded { state.grounded + 1 } else { 0 };
        if state.grounded >= GROUND_LIFETIME {
            discard(store, key);
            return false;
        }
        // Hook the first mob the bobber touches.
        if let Some(bbox) = store.bbox(key)
            && let Some(target) = store
                .query_vec(bbox)
                .into_iter()
                .find(|other| *other != key && mob::is_mob(store, *other))
            && let Some(id) = store.data(target).map(|data| data.id)
        {
            state.phase = Phase::Hooked(id);
        }
    }
    store_state(store, key, state);
    sync(store, key);
    true
}

fn store_state(store: &mut EntityStore, key: EntityKey, state: BobberState) {
    if let Some(data) = store.data_mut(key) {
        data.bobber = Some(state);
    }
}

/// `FishingHook.catchingFish`, without the rain and sky-light adjustments.
fn catch_fish(state: &mut BobberState, rng: &mut Rng) {
    if state.nibble > 0 {
        state.nibble -= 1;
    } else if state.time_until_hooked > 0 {
        state.time_until_hooked -= 1;
        if state.time_until_hooked == 0 {
            state.nibble = NIBBLE_MIN + rng.next_int(NIBBLE_SPAN);
        }
    } else if state.time_until_lured > 0 {
        state.time_until_lured -= 1;
        if state.time_until_lured == 0 {
            state.time_until_hooked = APPROACH_MIN + rng.next_int(APPROACH_SPAN);
        }
    } else {
        let wait = WAIT_MIN + rng.next_int(WAIT_SPAN) - state.lure * LURE_TICKS;
        state.time_until_lured = wait.max(MIN_WAIT);
    }
}

/// What reeling in a bobber did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reel {
    /// A fish was biting: roll the catch.
    Caught { damage: i32 },
    /// An entity was hooked and has been pulled toward the owner.
    Pulled { target: EntityKey, damage: i32 },
    /// Nothing came of it.
    Empty { damage: i32 },
}

impl Reel {
    /// Durability the rod loses.
    pub fn damage(self) -> i32 {
        match self {
            Self::Caught { damage } | Self::Pulled { damage, .. } | Self::Empty { damage } => {
                damage
            }
        }
    }
}

/// `FishingHook.retrieve`: reels the bobber in toward `owner_pos` and discards it.
pub fn reel(store: &mut EntityStore, key: EntityKey, owner_pos: Vec3) -> Option<Reel> {
    let state = store.data(key).and_then(|data| data.bobber)?;
    let pos = store.pos(key)?;
    let outcome = match state.phase {
        Phase::Hooked(id) => match store.key_of(id) {
            Some(target) if mob::is_mob(store, target) => {
                // `FishingHook.pullEntity`: a tenth of the way to the owner, with a lift.
                let at = store.pos(target).unwrap_or(pos);
                let pull = Vec3::new(
                    (owner_pos.x - at.x) * 0.1,
                    (owner_pos.y - at.y) * 0.1 + ((owner_pos - at).length()).sqrt() * 0.08,
                    (owner_pos.z - at.z) * 0.1,
                );
                let vel = store.vel(target).unwrap_or_default();
                store.set_vel(target, vel + pull);
                Reel::Pulled {
                    target,
                    damage: DAMAGE_ENTITY,
                }
            }
            _ => Reel::Empty { damage: 0 },
        },
        _ if state.nibble > 0 && state.phase == Phase::Bobbing => Reel::Caught {
            damage: DAMAGE_CATCH,
        },
        _ if store
            .flags(key)
            .is_some_and(|flags| flags.has(Flags::ON_GROUND)) =>
        {
            Reel::Empty {
                damage: DAMAGE_GROUND,
            }
        }
        _ => Reel::Empty { damage: 0 },
    };
    discard(store, key);
    Some(outcome)
}

/// Velocity that carries a catch from the bobber to the owner (`FishingHook.retrieve`).
pub fn catch_velocity(from: Vec3, to: Vec3) -> Vec3 {
    let delta = to - from;
    Vec3::new(
        delta.x * 0.1,
        delta.y * 0.1 + delta.length().sqrt() * 0.08,
        delta.z * 0.1,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Open air with water filling every block below `surface`, and stone below `floor`.
    struct Pond {
        surface: i32,
        floor: i32,
    }

    impl BlockGetter for Pond {
        fn block_state(&self, _x: i32, y: i32, _z: i32) -> Option<u16> {
            let name = if y < self.floor {
                "stone"
            } else if y < self.surface {
                "water"
            } else {
                "air"
            };
            ferrite_data::blocks::default_state(name)
        }
    }

    const OWNER: Uuid = Uuid::from_u128(7);

    fn cast_up(rng: &mut Rng) -> EntityRecord {
        cast(OWNER, 3, Vec3::new(0.5, 64.0, 0.5), 0.0, 0.0, 0, rng)
    }

    #[test]
    fn a_cast_makes_a_bobber_entity_that_names_its_owner_in_the_spawn_data() {
        let record = cast_up(&mut Rng::new(1));
        assert_eq!(record.data.kind, bobber_type());
        assert_eq!(record.data.spawn_data, 3);
        assert_eq!(record.data.bobber.unwrap().owner, OWNER);
        assert!(record.vel.z > 1.0, "thrown along the look direction");
        assert!(record.data.metadata.non_default().is_empty());
    }

    #[test]
    fn a_bobber_flies_lands_in_water_and_settles_at_the_surface() {
        let pond = Pond {
            surface: 60,
            floor: 50,
        };
        let mut rng = Rng::new(2);
        let mut store = EntityStore::default();
        let mut record = cast_up(&mut rng);
        record.vel = Vec3::new(0.0, 0.0, 0.2);
        let key = store.insert(record);
        for _ in 0..200 {
            assert!(tick(&mut store, key, &pond, &mut rng));
        }
        let state = store.data(key).unwrap().bobber.unwrap();
        assert_eq!(state.phase, Phase::Bobbing);
        let y = store.pos(key).unwrap().y;
        assert!((59.0..61.0).contains(&y), "floats at the surface, y = {y}");
        assert!(store.vel(key).unwrap().y.abs() < 0.1);
    }

    fn bobbing(rng: &mut Rng, lure: i32) -> (EntityStore, EntityKey, Pond) {
        let pond = Pond {
            surface: 60,
            floor: 50,
        };
        let mut store = EntityStore::default();
        let mut record = cast(OWNER, 3, Vec3::new(0.5, 59.6, 0.5), 0.0, 0.0, lure, rng);
        record.vel = Vec3::ZERO;
        let key = store.insert(record);
        for _ in 0..5 {
            tick(&mut store, key, &pond, rng);
        }
        assert_eq!(
            store.data(key).unwrap().bobber.unwrap().phase,
            Phase::Bobbing
        );
        (store, key, pond)
    }

    #[test]
    fn a_fish_bites_within_the_vanilla_window_and_the_bite_is_synched() {
        let mut rng = Rng::new(5);
        let (mut store, key, pond) = bobbing(&mut rng, 0);
        let mut first_bite = None;
        for t in 0..800 {
            tick(&mut store, key, &pond, &mut rng);
            if is_biting(store.data(key).unwrap()) {
                first_bite = Some(t);
                break;
            }
        }
        let t = first_bite.expect("a fish bites");
        // Wait 100-600, approach 20-80, so the bite comes between 120 and 680 ticks in.
        assert!((120..=690).contains(&t), "bit at tick {t}");
        let sent = store.data(key).unwrap().metadata.non_default();
        assert!(
            sent.iter()
                .any(|e| e.index == meta::BITING && e.value == MetaValue::Boolean(true))
        );
        // The nibble ends and the flag clears again.
        for _ in 0..NIBBLE_MIN + NIBBLE_SPAN + 1 {
            tick(&mut store, key, &pond, &mut rng);
        }
        assert!(!is_biting(store.data(key).unwrap()));
    }

    #[test]
    fn lure_shortens_the_wait() {
        let first_bite = |lure| {
            let mut rng = Rng::new(9);
            let (mut store, key, pond) = bobbing(&mut rng, lure);
            for t in 0..800 {
                tick(&mut store, key, &pond, &mut rng);
                if is_biting(store.data(key).unwrap()) {
                    return t;
                }
            }
            panic!("no bite");
        };
        assert!(first_bite(3) < first_bite(0));
    }

    #[test]
    fn reeling_a_biting_bobber_catches_and_discards_it() {
        let mut rng = Rng::new(5);
        let (mut store, key, pond) = bobbing(&mut rng, 0);
        while !is_biting(store.data(key).unwrap()) {
            tick(&mut store, key, &pond, &mut rng);
        }
        let reel = reel(&mut store, key, Vec3::new(0.5, 64.0, 5.5)).unwrap();
        assert_eq!(
            reel,
            Reel::Caught {
                damage: DAMAGE_CATCH
            }
        );
        assert!(store.flags(key).unwrap().has(Flags::REMOVED));
        assert_eq!(find_for(&store, OWNER), None);
    }

    #[test]
    fn reeling_with_no_bite_catches_nothing_and_on_the_ground_costs_two() {
        let mut rng = Rng::new(5);
        let (mut store, key, _) = bobbing(&mut rng, 0);
        assert_eq!(
            reel(&mut store, key, Vec3::ZERO),
            Some(Reel::Empty { damage: 0 })
        );
        // A bobber that landed on stone.
        let stone = Pond {
            surface: 0,
            floor: 64,
        };
        let mut store = EntityStore::default();
        let mut record = cast_up(&mut rng);
        record.vel = Vec3::ZERO;
        record.pos = Vec3::new(0.5, 66.0, 0.5);
        let key = store.insert(record);
        for _ in 0..30 {
            tick(&mut store, key, &stone, &mut rng);
        }
        assert!(store.flags(key).unwrap().has(Flags::ON_GROUND));
        assert_eq!(
            reel(&mut store, key, Vec3::ZERO),
            Some(Reel::Empty {
                damage: DAMAGE_GROUND
            })
        );
    }

    #[test]
    fn a_bobber_resting_on_land_is_discarded_after_a_minute() {
        let stone = Pond {
            surface: 0,
            floor: 64,
        };
        let mut rng = Rng::new(5);
        let mut store = EntityStore::default();
        let mut record = cast_up(&mut rng);
        record.vel = Vec3::ZERO;
        record.pos = Vec3::new(0.5, 64.0, 0.5);
        let key = store.insert(record);
        let mut alive = 0;
        while tick(&mut store, key, &stone, &mut rng) {
            alive += 1;
            assert!(alive < GROUND_LIFETIME + 20, "never discarded");
        }
        assert!(alive >= GROUND_LIFETIME - 5, "discarded too early: {alive}");
        assert!(store.flags(key).unwrap().has(Flags::REMOVED));
        // A discarded bobber no longer ticks.
        assert!(!tick(&mut store, key, &stone, &mut rng));
    }

    #[test]
    fn a_bobber_hooks_a_mob_it_touches_and_reeling_pulls_the_mob_to_the_owner() {
        use crate::mob::{MobKind, new_mob};
        let air = Pond {
            surface: 0,
            floor: 0,
        };
        let mut rng = Rng::new(5);
        let mut store = EntityStore::default();
        let mut cow = new_mob(MobKind::Cow, Vec3::new(0.5, 64.0, 3.5), &mut rng);
        cow.flags.set(Flags::NO_GRAVITY, true);
        let cow_id = cow.data.id;
        let cow_key = store.insert(cow);
        let mut record = cast_up(&mut rng);
        record.vel = Vec3::new(0.0, 0.0, 0.5);
        record.pos = Vec3::new(0.5, 64.2, 1.0);
        let key = store.insert(record);
        for _ in 0..20 {
            tick(&mut store, key, &air, &mut rng);
            if matches!(
                store.data(key).unwrap().bobber.unwrap().phase,
                Phase::Hooked(_)
            ) {
                break;
            }
        }
        assert_eq!(
            store.data(key).unwrap().bobber.unwrap().phase,
            Phase::Hooked(cow_id)
        );
        // The hooked id is synched as id + 1.
        tick(&mut store, key, &air, &mut rng);
        assert_eq!(
            store.data(key).unwrap().metadata.get(meta::HOOKED_ENTITY),
            Some(&MetaValue::Int(cow_id + 1))
        );
        let before = store.vel(cow_key).unwrap();
        let owner = Vec3::new(0.5, 64.0, -6.0);
        let result = reel(&mut store, key, owner).unwrap();
        assert_eq!(
            result,
            Reel::Pulled {
                target: cow_key,
                damage: DAMAGE_ENTITY
            }
        );
        assert!(
            store.vel(cow_key).unwrap().z < before.z,
            "pulled toward the owner"
        );
    }

    #[test]
    fn a_hooked_mob_that_disappears_frees_the_bobber() {
        use crate::mob::{MobKind, new_mob};
        let air = Pond {
            surface: 0,
            floor: 0,
        };
        let mut rng = Rng::new(5);
        let mut store = EntityStore::default();
        let mut cow = new_mob(MobKind::Cow, Vec3::new(0.5, 64.0, 1.0), &mut rng);
        cow.flags.set(Flags::NO_GRAVITY, true);
        let cow_key = store.insert(cow);
        let mut record = cast_up(&mut rng);
        record.vel = Vec3::ZERO;
        record.pos = Vec3::new(0.5, 64.2, 1.0);
        let key = store.insert(record);
        tick(&mut store, key, &air, &mut rng);
        assert!(matches!(
            store.data(key).unwrap().bobber.unwrap().phase,
            Phase::Hooked(_)
        ));
        mob::kill(&mut store, cow_key);
        tick(&mut store, key, &air, &mut rng);
        assert_eq!(
            store.data(key).unwrap().bobber.unwrap().phase,
            Phase::Flying
        );
        assert_eq!(
            store.data(key).unwrap().metadata.get(meta::HOOKED_ENTITY),
            Some(&MetaValue::Int(0))
        );
    }
}
