//! Enderman rules beyond the shared hostile goals (vanilla `EnderMan`): the stare that angers
//! it, water damage, its synched creepy / stared-at flags, and the take/leave-block goals.
//!
//! Checked against the 26.3 jar: `EndermanLookForPlayerGoal` picks the nearest player within the
//! follow range (64) who is looking at it, starts a five-tick aggro timer and only targets the
//! player when the stare lasts; a player wearing a carved pumpkin never counts
//! (`PLAYER_NOT_WEARING_DISGUISE_ITEM`); `isLookingAtMe` needs
//! `dot(view, to eye) > 1 - 0.025 / distance` and a clear line of sight to the enderman's eyes;
//! `isSensitiveToWater` makes `LivingEntity.aiStep` deal 1 drowning damage every tick the mob is
//! in water or rain, and `Enderman.hurtServer` then teleports it nine times in ten; anger lasts
//! 20 to 39 seconds. The carried block (`DATA_CARRY_STATE`, byte 16) is kept and saved; the
//! command/protocol lane adds its optional block-state wire value. `EndermanTakeBlockGoal` scans
//! a 4x3x4 box for a `#enderman_holdable` block with a clear line of sight and removes it without
//! drops, storing the block's default state; `EndermanLeaveBlockGoal` tries a 2x2x2 box and the
//! region applies the air/full-support/`canSurvive`/entity checks before placing it.

use crate::mob::{self, MobKind};
use crate::physics::{self, BlockGetter};
use crate::{EntityKey, EntityStore, Metadata, Rng};
use ferrite_math::{AABB, Vec3};
use ferrite_protocol::packets::play::MetaValue;
use uuid::Uuid;

/// Synched `EnderMan.DATA_CREEPY` (the open-mouth aggressive face) and `DATA_STARED_AT`.
pub const CREEPY_INDEX: u8 = 17;
pub const STARED_AT_INDEX: u8 = 18;
/// `isLookingAtMe` tolerance, scaled by the distance.
pub const STARE_TOLERANCE: f64 = 0.025;
/// Ticks a stare must last before the enderman turns on the player.
pub const STARE_TICKS: u8 = 5;
/// `NearestAttackableTargetGoal` interval: the stare scan runs on one tick in ten.
pub const SCAN_INTERVAL: i32 = 10;
/// The enderman's follow range, the stare scan range.
pub const STARE_RANGE: f64 = 64.0;
/// `PERSISTENT_ANGER_TIME`: 20 to 39 seconds.
pub const ANGER_MIN: u16 = 400;
pub const ANGER_SPAN: i32 = 381;
/// Damage per tick in water or rain (`DamageSources.drown`, 1.0).
pub const WATER_DAMAGE: f32 = 1.0;
/// The eye height of an enderman (`3 * 0.9`, from the entity data).
const BLOCK_STEP: f64 = 0.25;
/// `EndermanTakeBlockGoal` canUse interval: `reducedTickDelay(20)` is 10 ticks.
pub const TAKE_INTERVAL: i32 = 10;
/// `EndermanLeaveBlockGoal` canUse interval: `reducedTickDelay(2000)` is 1000 ticks.
pub const LEAVE_INTERVAL: i32 = 1000;

/// What the region knows about where a player is looking, published per owning region.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gaze {
    /// The unit view vector (`getViewVector`).
    pub look: Vec3,
    /// Whether the player wears a carved pumpkin, which hides them from endermen.
    pub pumpkin: bool,
}

/// Defines the creepy and stared-at flags on an enderman's synched data.
pub fn define_metadata(metadata: &mut Metadata) {
    metadata.define(CREEPY_INDEX, MetaValue::Boolean(false));
    metadata.define(STARED_AT_INDEX, MetaValue::Boolean(false));
}

/// Writes the flags from the mob state: creepy while it has a target, stared-at once a stare
/// started (`setBeingStaredAt`, which vanilla never clears).
pub fn sync(store: &mut EntityStore, key: EntityKey) {
    if let Some(data) = store.data_mut(key)
        && let Some(state) = data.mob
        && state.kind == MobKind::Enderman
    {
        data.metadata
            .set(CREEPY_INDEX, MetaValue::Boolean(state.target.is_some()));
        data.metadata
            .set(STARED_AT_INDEX, MetaValue::Boolean(state.stared_at));
    }
}

/// Unloaded blocks as air: a sight line is not blocked by terrain that is not there, unlike
/// movement, which treats it as solid.
struct SeeThrough<'a, B>(&'a B);

impl<B: BlockGetter> BlockGetter for SeeThrough<'_, B> {
    fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
        Some(self.0.block_state(x, y, z).unwrap_or(0))
    }
}

/// Whether the line from `from` to `to` crosses no collision shape.
fn clear_line(blocks: &impl BlockGetter, from: Vec3, to: Vec3) -> bool {
    let blocks = &SeeThrough(blocks);
    let delta = to - from;
    let length = delta.length();
    let steps = (length / BLOCK_STEP).ceil().max(1.0) as u32;
    (1..steps).all(|step| {
        let at = from + delta * (f64::from(step) / f64::from(steps));
        physics::no_collision(
            blocks,
            AABB::new(at.x, at.y, at.z, at.x + 1e-3, at.y + 1e-3, at.z + 1e-3),
        )
    })
}

/// Vanilla `EnderMan.isBeingStaredBy` for a player whose feet are at `feet`.
pub fn is_stared_at_by(
    blocks: &impl BlockGetter,
    enderman_eye: Vec3,
    feet: Vec3,
    gaze: Gaze,
) -> bool {
    if gaze.pumpkin {
        return false;
    }
    let eye = crate::combat::eye_position(feet);
    let to = enderman_eye - eye;
    let distance = to.length();
    if distance < 1e-6 {
        return true;
    }
    let dot = gaze.look.normalize().dot(to * (1.0 / distance));
    dot > 1.0 - STARE_TOLERANCE / distance && clear_line(blocks, eye, enderman_eye)
}

/// A player the enderman may turn on: where they are, and what they look at.
#[derive(Debug, Clone, Copy)]
pub struct Candidate {
    pub uuid: Uuid,
    pub pos: Vec3,
    pub gaze: Gaze,
}

/// Whether a state is in vanilla's `#minecraft:enderman_holdable` block tag.
fn holdable(state: u16) -> bool {
    ferrite_data::blocks::state(state)
        .is_some_and(|(name, _)| ferrite_data::tags::contains("block", "enderman_holdable", name))
}

/// Whether the straight line from the enderman's block centre reaches `target` before any
/// collision shape. Vanilla uses `Level.clip`; the target block itself is allowed to be solid.
fn reaches_block(blocks: &impl BlockGetter, from: Vec3, target: [i32; 3]) -> bool {
    let start = Vec3::new(
        from.x.floor() + 0.5,
        from.y.floor() + 0.5,
        from.z.floor() + 0.5,
    );
    let end = Vec3::new(
        f64::from(target[0]) + 0.5,
        f64::from(target[1]) + 0.5,
        f64::from(target[2]) + 0.5,
    );
    let delta = end - start;
    let steps = (delta.length() / BLOCK_STEP).ceil().max(1.0) as u32;
    for step in 1..=steps {
        let at = start + delta * (f64::from(step) / f64::from(steps));
        let cell = [
            at.x.floor() as i32,
            at.y.floor() as i32,
            at.z.floor() as i32,
        ];
        if cell == target {
            return true;
        }
        if !physics::no_collision(
            blocks,
            AABB::new(at.x, at.y, at.z, at.x + 1e-3, at.y + 1e-3, at.z + 1e-3),
        ) {
            return false;
        }
    }
    false
}

/// One `EndermanTakeBlockGoal.tick` attempt. The caller supplies the 1/10 canUse roll.
/// The block state is returned by position only; the region sets the carried block to its
/// default state after it removes the actual block, as vanilla does.
pub fn take_block(
    store: &EntityStore,
    key: EntityKey,
    blocks: &impl BlockGetter,
    griefing: bool,
    rng: &mut Rng,
) -> Option<[i32; 3]> {
    if !griefing {
        return None;
    }
    let state = mob::state(store, key)?;
    if state.kind != MobKind::Enderman || state.carried.is_some() {
        return None;
    }
    let pos = store.pos(key)?;
    let target = [
        (pos.x - 2.0 + rng.next_f64() * 4.0).floor() as i32,
        (pos.y + rng.next_f64() * 3.0).floor() as i32,
        (pos.z - 2.0 + rng.next_f64() * 4.0).floor() as i32,
    ];
    let block = blocks.block_state(target[0], target[1], target[2])?;
    if !holdable(block) || !reaches_block(blocks, pos, target) {
        return None;
    }
    Some(target)
}

/// One `EndermanLeaveBlockGoal.tick` attempt. The caller supplies the 1/1000 canUse roll.
/// Support (`canSurvive`) and entity occupancy are checked by the region before the block is
/// actually written; this function only chooses a candidate.
pub fn leave_block(
    store: &EntityStore,
    key: EntityKey,
    blocks: &impl BlockGetter,
    griefing: bool,
    rng: &mut Rng,
) -> Option<([i32; 3], u16)> {
    if !griefing {
        return None;
    }
    let state = mob::state(store, key)?;
    if state.kind != MobKind::Enderman {
        return None;
    }
    let carried = state.carried?;
    if ferrite_data::block_props::get(carried).is_air {
        return None;
    }
    let pos = store.pos(key)?;
    let target = [
        (pos.x - 1.0 + rng.next_f64() * 2.0).floor() as i32,
        (pos.y + rng.next_f64() * 2.0).floor() as i32,
        (pos.z - 1.0 + rng.next_f64() * 2.0).floor() as i32,
    ];
    let current = blocks.block_state(target[0], target[1], target[2])?;
    if !ferrite_data::block_props::get(current).is_air {
        return None;
    }
    let below = blocks.block_state(target[0], target[1] - 1, target[2])?;
    let (below_name, _) = ferrite_data::blocks::state(below)?;
    if below_name == "minecraft:bedrock" || !ferrite_data::block_props::get(below).full_collision {
        return None;
    }
    let area = AABB::new(
        f64::from(target[0]),
        f64::from(target[1]),
        f64::from(target[2]),
        f64::from(target[0]) + 1.0,
        f64::from(target[1]) + 1.0,
        f64::from(target[2]) + 1.0,
    );
    if store.query_vec(area).iter().any(|&other| other != key) {
        return None;
    }
    Some((target, carried))
}

/// One tick of `EndermanLookForPlayerGoal` for a calm enderman. Returns whether the enderman
/// became angry at a player this tick.
pub fn tick_stare(
    store: &mut EntityStore,
    key: EntityKey,
    blocks: &impl BlockGetter,
    players: &[Candidate],
    rng: &mut Rng,
) -> bool {
    let Some(state) = mob::state(store, key).copied() else {
        return false;
    };
    let Some(pos) = store.pos(key) else {
        return false;
    };
    let eye = Vec3::new(
        pos.x,
        pos.y + f64::from(state.kind.props().eye_height),
        pos.z,
    );
    let staring = |c: &Candidate| {
        c.pos.distance_to_sqr(pos) <= STARE_RANGE * STARE_RANGE
            && is_stared_at_by(blocks, eye, c.pos, c.gaze)
    };
    if state.stare_time > 0 {
        let still = state
            .stare_target
            .and_then(|uuid| players.iter().find(|c| c.uuid == uuid))
            .is_some_and(staring);
        let Some(state) = mob::state_mut(store, key) else {
            return false;
        };
        if !still {
            state.stare_time = 0;
            state.stare_target = None;
            return false;
        }
        state.stare_time -= 1;
        if state.stare_time == 0 {
            state.target = state.stare_target.take();
            state.anger = ANGER_MIN + rng.next_int(ANGER_SPAN) as u16;
            return true;
        }
        return false;
    }
    if rng.next_int(SCAN_INTERVAL) != 0 {
        return false;
    }
    let nearest = players
        .iter()
        .filter(|c| staring(c))
        .min_by(|a, b| {
            a.pos
                .distance_to_sqr(pos)
                .total_cmp(&b.pos.distance_to_sqr(pos))
                .then_with(|| a.uuid.cmp(&b.uuid))
        })
        .map(|c| c.uuid);
    if let Some(uuid) = nearest
        && let Some(state) = mob::state_mut(store, key)
    {
        state.stare_time = STARE_TICKS;
        state.stare_target = Some(uuid);
        state.stared_at = true;
    }
    false
}

/// Whether the enderman stands in water, or in rain with open sky above it
/// (`isInWaterOrRain`).
pub fn in_water_or_rain(
    store: &EntityStore,
    key: EntityKey,
    blocks: &impl BlockGetter,
    raining: bool,
) -> bool {
    let Some(area) = store.bbox(key) else {
        return false;
    };
    if physics::fluid_height(blocks, area, false) > 0.0 {
        return true;
    }
    if !raining {
        return false;
    }
    let (x, z) = (area.min.x.floor() as i32, area.min.z.floor() as i32);
    let top = area.max.y.floor() as i32;
    (top + 1..=top + 320).all(|y| {
        physics::no_collision(
            blocks,
            AABB::new(
                f64::from(x),
                f64::from(y),
                f64::from(z),
                f64::from(x) + 1.0,
                f64::from(y) + 1.0,
                f64::from(z) + 1.0,
            ),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{self, MobRules, Target};
    use std::collections::HashMap;

    /// Stone below y = 64, air above, with optional extras.
    #[derive(Default)]
    struct World {
        /// A stone slab one block thick at this height over the whole area.
        roof: Option<i32>,
        /// Water fills this level (y = 64) instead of air.
        flooded: bool,
        /// A stone wall at this x.
        wall_x: Option<i32>,
        /// Explicit overrides for tests that need a holdable block.
        blocks: HashMap<(i32, i32, i32), u16>,
    }

    impl BlockGetter for World {
        fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
            if let Some(&state) = self.blocks.get(&(x, y, z)) {
                return Some(state);
            }
            let name = if y < 64 || (self.wall_x == Some(x) && y < 70) || self.roof == Some(y) {
                "stone"
            } else if self.flooded && y == 64 {
                "water"
            } else {
                "air"
            };
            ferrite_data::blocks::default_state(name)
        }
    }

    const PLAYER: Uuid = Uuid::from_u128(0xe1);

    fn spawn(store: &mut EntityStore, at: Vec3) -> EntityKey {
        store.insert(mob::new_mob(MobKind::Enderman, at, &mut Rng::new(3)))
    }

    /// A player at `feet` looking straight at `target`'s eyes.
    fn looking_at(feet: Vec3, target: Vec3) -> Candidate {
        let eye = crate::combat::eye_position(feet);
        Candidate {
            uuid: PLAYER,
            pos: feet,
            gaze: Gaze {
                look: (target - eye).normalize(),
                pumpkin: false,
            },
        }
    }

    fn eye_of(store: &EntityStore, key: EntityKey) -> Vec3 {
        let pos = store.pos(key).unwrap();
        Vec3::new(
            pos.x,
            pos.y + f64::from(MobKind::Enderman.props().eye_height),
            pos.z,
        )
    }

    #[test]
    fn a_player_looking_at_its_eyes_stares_and_one_looking_away_does_not() {
        let world = World::default();
        let mut store = EntityStore::default();
        let key = spawn(&mut store, Vec3::new(10.5, 64.0, 0.5));
        let eye = eye_of(&store, key);
        let feet = Vec3::new(0.5, 64.0, 0.5);
        let staring = looking_at(feet, eye);
        assert!(is_stared_at_by(&world, eye, feet, staring.gaze));
        let away = Gaze {
            look: Vec3::new(-1.0, 0.0, 0.0),
            pumpkin: false,
        };
        assert!(!is_stared_at_by(&world, eye, feet, away));
        // The tolerance shrinks with distance: 1 degree off at 10 blocks stares, 8 degrees off
        // does not (1 - 0.025 / 10 is about 3.2 degrees).
        let skewed = |degrees: f64| {
            let base = (eye - crate::combat::eye_position(feet)).normalize();
            let (s, c) = degrees.to_radians().sin_cos();
            Gaze {
                look: Vec3::new(base.x * c - base.z * s, base.y, base.x * s + base.z * c),
                pumpkin: false,
            }
        };
        assert!(is_stared_at_by(&world, eye, feet, skewed(1.0)));
        assert!(!is_stared_at_by(&world, eye, feet, skewed(8.0)));
    }

    #[test]
    fn a_carved_pumpkin_and_a_wall_both_hide_the_player() {
        let mut store = EntityStore::default();
        let key = spawn(&mut store, Vec3::new(10.5, 64.0, 0.5));
        let eye = eye_of(&store, key);
        let feet = Vec3::new(0.5, 64.0, 0.5);
        let mut staring = looking_at(feet, eye);
        let open = World::default();
        let walled = World {
            wall_x: Some(5),
            ..Default::default()
        };
        assert!(is_stared_at_by(&open, eye, feet, staring.gaze));
        assert!(!is_stared_at_by(&walled, eye, feet, staring.gaze), "wall");
        staring.gaze.pumpkin = true;
        assert!(!is_stared_at_by(&open, eye, feet, staring.gaze), "pumpkin");
    }

    fn stare_until_angry(
        store: &mut EntityStore,
        key: EntityKey,
        world: &World,
        player: &Candidate,
        rng: &mut Rng,
    ) -> u32 {
        for tick in 1..=2_000 {
            if tick_stare(store, key, world, std::slice::from_ref(player), rng) {
                return tick;
            }
        }
        panic!("the enderman never turned on the player");
    }

    #[test]
    fn a_stare_that_lasts_five_ticks_angers_the_enderman_for_20_to_39_seconds() {
        let world = World::default();
        let mut store = EntityStore::default();
        let key = spawn(&mut store, Vec3::new(10.5, 64.0, 0.5));
        let player = looking_at(Vec3::new(0.5, 64.0, 0.5), eye_of(&store, key));
        let mut rng = Rng::new(21);
        stare_until_angry(&mut store, key, &world, &player, &mut rng);
        let state = mob::state(&store, key).unwrap();
        assert_eq!(state.target, Some(PLAYER));
        assert!((ANGER_MIN..=780).contains(&state.anger), "{}", state.anger);
        assert!(state.stared_at);
    }

    #[test]
    fn the_anger_comes_exactly_five_ticks_after_the_stare_starts() {
        let world = World::default();
        let mut store = EntityStore::default();
        let key = spawn(&mut store, Vec3::new(10.5, 64.0, 0.5));
        let player = looking_at(Vec3::new(0.5, 64.0, 0.5), eye_of(&store, key));
        let mut rng = Rng::new(8);
        // Wait for the scan to start the stare, then count.
        while mob::state(&store, key).unwrap().stare_time == 0 {
            assert!(!tick_stare(&mut store, key, &world, &[player], &mut rng));
        }
        for tick in 1..=STARE_TICKS {
            let angry = tick_stare(&mut store, key, &world, &[player], &mut rng);
            assert_eq!(angry, tick == STARE_TICKS, "tick {tick}");
        }
    }

    #[test]
    fn looking_away_during_the_stare_cancels_it() {
        let world = World::default();
        let mut store = EntityStore::default();
        let key = spawn(&mut store, Vec3::new(10.5, 64.0, 0.5));
        let mut player = looking_at(Vec3::new(0.5, 64.0, 0.5), eye_of(&store, key));
        let mut rng = Rng::new(8);
        while mob::state(&store, key).unwrap().stare_time == 0 {
            tick_stare(&mut store, key, &world, &[player], &mut rng);
        }
        player.gaze.look = Vec3::new(0.0, 0.0, 1.0);
        assert!(!tick_stare(&mut store, key, &world, &[player], &mut rng));
        let state = mob::state(&store, key).unwrap();
        assert_eq!((state.stare_time, state.target), (0, None));
        assert!(state.stared_at, "being stared at is never cleared");
    }

    #[test]
    fn a_player_beyond_64_blocks_is_never_noticed() {
        let world = World::default();
        let mut store = EntityStore::default();
        let key = spawn(&mut store, Vec3::new(100.5, 64.0, 0.5));
        let player = looking_at(Vec3::new(0.5, 64.0, 0.5), eye_of(&store, key));
        let mut rng = Rng::new(8);
        for _ in 0..500 {
            assert!(!tick_stare(&mut store, key, &world, &[player], &mut rng));
        }
        assert_eq!(mob::state(&store, key).unwrap().stare_time, 0);
    }

    #[test]
    fn the_ai_angers_a_calm_enderman_from_a_published_gaze_and_sets_the_creepy_flag() {
        let world = World::default();
        let mut store = EntityStore::default();
        let key = spawn(&mut store, Vec3::new(10.5, 64.0, 0.5));
        let feet = Vec3::new(0.5, 64.0, 0.5);
        let candidate = looking_at(feet, eye_of(&store, key));
        let rules = MobRules {
            gazes: HashMap::from([(PLAYER, candidate.gaze)]),
            ..Default::default()
        };
        let players = [Target {
            uuid: PLAYER,
            pos: feet,
        }];
        let mut rng = Rng::new(31);
        for _ in 0..2_000 {
            ai::tick_full(&mut store, &world, &players, &rules, &[], &[], &mut rng);
            if mob::state(&store, key).unwrap().target.is_some() {
                break;
            }
        }
        assert_eq!(mob::state(&store, key).unwrap().target, Some(PLAYER));
        let flag = |index| store.data(key).unwrap().metadata.get(index).cloned();
        assert_eq!(flag(CREEPY_INDEX), Some(MetaValue::Boolean(true)));
        assert_eq!(flag(STARED_AT_INDEX), Some(MetaValue::Boolean(true)));
    }

    #[test]
    fn a_calm_enderman_with_nobody_staring_stays_calm() {
        let world = World::default();
        let mut store = EntityStore::default();
        let key = spawn(&mut store, Vec3::new(10.5, 64.0, 0.5));
        let players = [Target {
            uuid: PLAYER,
            pos: Vec3::new(0.5, 64.0, 0.5),
        }];
        let mut rng = Rng::new(31);
        for _ in 0..1_000 {
            ai::tick_full(
                &mut store,
                &world,
                &players,
                &MobRules::default(),
                &[],
                &[],
                &mut rng,
            );
        }
        let state = mob::state(&store, key).unwrap();
        assert_eq!((state.target, state.stared_at), (None, false));
        assert_eq!(
            store.data(key).unwrap().metadata.get(CREEPY_INDEX).cloned(),
            Some(MetaValue::Boolean(false))
        );
    }

    #[test]
    fn water_and_open_sky_rain_count_but_dry_air_and_a_roof_do_not() {
        let mut store = EntityStore::default();
        let key = spawn(&mut store, Vec3::new(0.5, 64.0, 0.5));
        let dry = World::default();
        let wet = World {
            flooded: true,
            ..Default::default()
        };
        let roofed = World {
            roof: Some(70),
            ..Default::default()
        };
        assert!(!in_water_or_rain(&store, key, &dry, false));
        assert!(in_water_or_rain(&store, key, &wet, false), "water");
        assert!(
            in_water_or_rain(&store, key, &dry, true),
            "rain under open sky"
        );
        assert!(
            !in_water_or_rain(&store, key, &roofed, true),
            "rain under a roof"
        );
    }

    #[test]
    fn an_enderman_in_water_loses_health_and_a_dry_one_does_not() {
        let wet = World {
            flooded: true,
            ..Default::default()
        };
        let dry = World::default();
        let health = |world: &World| {
            let mut store = EntityStore::default();
            let key = spawn(&mut store, Vec3::new(0.5, 64.0, 0.5));
            let before = mob::state(&store, key).unwrap().health;
            let mut rng = Rng::new(2);
            for _ in 0..60 {
                ai::tick_full(
                    &mut store,
                    world,
                    &[],
                    &MobRules::default(),
                    &[],
                    &[],
                    &mut rng,
                );
            }
            before - mob::state(&store, key).map_or(0.0, |s| s.health)
        };
        assert!(health(&wet) >= 1.0, "water must hurt");
        assert_eq!(health(&dry), 0.0);
    }

    #[test]
    fn rain_hurts_an_enderman_in_the_open_only() {
        let rain = MobRules {
            raining: true,
            ..Default::default()
        };
        let hurt = |world: &World| {
            let mut store = EntityStore::default();
            let key = spawn(&mut store, Vec3::new(0.5, 64.0, 0.5));
            let mut rng = Rng::new(2);
            for _ in 0..60 {
                ai::tick_full(
                    &mut store,
                    world,
                    &[Target {
                        uuid: PLAYER,
                        pos: Vec3::new(90.5, 64.0, 0.5),
                    }],
                    &rain,
                    &[],
                    &[],
                    &mut rng,
                );
            }
            mob::state(&store, key).unwrap().health < MobKind::Enderman.max_health()
        };
        assert!(hurt(&World::default()));
        assert!(!hurt(&World {
            roof: Some(70),
            ..Default::default()
        }));
    }

    fn holdable_world() -> World {
        World {
            blocks: HashMap::from([(
                (1, 64, 0),
                ferrite_data::blocks::default_state("minecraft:grass_block").unwrap(),
            )]),
            ..Default::default()
        }
    }

    #[test]
    fn an_enderman_takes_a_holdable_block_after_a_line_of_sight_check() {
        let mut store = EntityStore::default();
        let key = spawn(&mut store, Vec3::new(0.5, 64.0, 0.5));
        let world = holdable_world();
        let mut rng = Rng::new(2);
        let mut taken = None;
        for _ in 0..5000 {
            if let Some(at) = take_block(&store, key, &world, true, &mut rng) {
                taken = Some(at);
                break;
            }
        }
        assert_eq!(taken, Some([1, 64, 0]));
        assert!(take_block(&store, key, &world, false, &mut rng).is_none());
    }

    #[test]
    fn a_carrying_enderman_places_its_block_on_a_full_support() {
        let mut store = EntityStore::default();
        let key = spawn(&mut store, Vec3::new(0.5, 64.0, 0.5));
        let grass = ferrite_data::blocks::default_state("minecraft:grass_block").unwrap();
        mob::state_mut(&mut store, key).unwrap().carried = Some(grass);
        let world = World::default();
        let mut rng = Rng::new(3);
        let mut placed = None;
        for _ in 0..5000 {
            if let Some(result) = leave_block(&store, key, &world, true, &mut rng) {
                placed = Some(result);
                break;
            }
        }
        let (at, state) = placed.expect("the enderman placed its block");
        assert_eq!(state, grass);
        assert_eq!(at[1], 64);
        assert!(leave_block(&store, key, &world, false, &mut rng).is_none());
    }
}
