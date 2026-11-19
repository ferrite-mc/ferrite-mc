//! Turtle home-beach and egg cycle (vanilla 26.3 `Turtle`, `TurtleEggBlock`).
//!
//! - **Home position** (`Turtle.homePos`): a turtle remembers the beach it spawned on (or the
//!   egg block it hatched from) and returns there to lay eggs. Vanilla saves it as `home_pos`
//!   and restores the current position when the tag is absent or malformed.
//! - **Egg laying** (`Turtle$TurtleLayEggGoal`): an adult with an egg (`HAS_EGG`) walks to a sand
//!   block within `HOME_RETURN_RADIUS` of home, waits `LAY_EGG_TICKS` while `LAYING_EGG` is set,
//!   then places a `minecraft:turtle_egg` cluster with 1..=4 eggs at `hatch = 0`.
//! - **Egg block** (`TurtleEggBlock`): `eggs` 1..=4 and `hatch` 0..=2, night random-tick
//!   hatching, 1/100 step-on / 1/3 fall-on trampling, and a Silk Touch-only block drop.
//! - **Water goals** (`TurtleGoToWaterGoal`, `TurtleTravelGoal`, `TurtlePanicGoal`): a dry turtle
//!   walks to water, an adult without an egg travels randomly while swimming, and a panicking
//!   turtle runs for nearby water. The remaining approximations are listed in
//!   `docs/mob-ai-parity.md`.

use crate::EntityKey;
use crate::ai::MobEvent;
use crate::mob::{self, MobKind};
use crate::physics::BlockGetter;
use crate::rng::Rng;
use crate::store::{EntityRecord, EntityStore};
use ferrite_data::block_props::FluidKind;
use ferrite_math::Vec3;

/// `Turtle$TurtleLayEggGoal` starts only within 9 blocks of the home position.
pub const HOME_RETURN_RADIUS: f64 = 9.0;
/// `Turtle$TurtleGoHomeGoal` finishes once the turtle is within 7 blocks.
pub const HOME_ARRIVAL_RADIUS: f64 = 7.0;
/// `Turtle$TurtleLayEggGoal` waits `adjustedTickDelay(200)` before placing the cluster.
pub const LAY_EGG_TICKS: u16 = 200;
/// `MoveToBlockGoal` search range for the lay-egg goal.
pub const LAY_SEARCH_RADIUS: i32 = 16;
/// Vanilla `TurtleEggBlock.MIN_EGGS` / `MAX_EGGS`.
pub const MIN_EGGS: u8 = 1;
pub const MAX_EGGS: u8 = 4;
/// Vanilla `TurtleEggBlock.MAX_HATCH_LEVEL`.
pub const MAX_HATCH: u8 = 2;
/// `TurtleEggBlock.shouldUpdateHatchLevel` in 26.3: 1/500 in the ordinary night window.
pub const NIGHT_HATCH_CHANCE: f32 = 0.002;
/// The 26.3 `day` timeline lifts the hatch chance to 1.0 for this late-night window.
pub const LATE_NIGHT_HATCH_START: i64 = 21_062;
pub const LATE_NIGHT_HATCH_END: i64 = 21_905;
/// Vanilla step-on and fall-on destroy rolls (`destroyEgg(..., 100)` and `(..., 3)`).
pub const TRAMPLE_CHANCE: i32 = 100;
pub const FALL_TRAMPLE_CHANCE: i32 = 3;
/// Vanilla `Turtle` is 1.2 blocks wide; the move-to-block goal reaches within one block.
const LAY_REACH: f64 = 1.5;
/// `Turtle.getBreedOffspring` lays eggs instead of producing a baby; the egg timer is reset
/// after laying with `setInLoveTime(600)`.
pub const LAY_AFTER_LOVE_TICKS: u16 = 600;

pub const LAY_EGG_SOUND: &str = "minecraft:entity.turtle.lay_egg";
pub const EGG_CRACK_SOUND: &str = "minecraft:entity.turtle.egg_crack";
pub const EGG_HATCH_SOUND: &str = "minecraft:entity.turtle.egg_hatch";
pub const EGG_BREAK_SOUND: &str = "minecraft:entity.turtle.egg_break";

/// What the turtle goal wants from the ordinary movement code this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TurtleStep {
    /// No turtle goal is active; the normal wander/look goals may run.
    Idle,
    /// Walk toward `target` with the supplied speed multiplier.
    Walking { target: Vec3, speed: f64 },
    /// Stand on the nest while `LAYING_EGG` counts down.
    Laying,
}

/// Writes the two turtle synched flags from [`crate::mob::MobState`].
pub fn sync_metadata(metadata: &mut crate::Metadata, state: &crate::mob::MobState) {
    use ferrite_protocol::packets::play::MetaValue;
    metadata.set(
        crate::metadata::turtle::HAS_EGG,
        MetaValue::Boolean(state.has_egg),
    );
    metadata.set(
        crate::metadata::turtle::LAYING_EGG,
        MetaValue::Boolean(state.laying_egg),
    );
}

/// The block position a turtle calls home.
pub fn home_center(home: [i32; 3]) -> Vec3 {
    Vec3::new(
        f64::from(home[0]) + 0.5,
        f64::from(home[1]),
        f64::from(home[2]) + 0.5,
    )
}

/// The centre of the cell above a sand block, where a laying turtle stands.
pub fn nest_center(site: [i32; 3]) -> Vec3 {
    Vec3::new(
        f64::from(site[0]) + 0.5,
        f64::from(site[1]) + 1.0,
        f64::from(site[2]) + 0.5,
    )
}

/// Runs one tick of the turtle goals: lay, go home, go to water, and travel.
///
/// Vanilla's `TurtleLayEggGoal` is not clock-gated; the earlier night-only restriction has been
/// removed so daytime laying matches 26.3. Water and rare home trips are transient goal state,
/// not NBT state.
pub fn tick_turtle(
    store: &mut EntityStore,
    key: EntityKey,
    blocks: &impl BlockGetter,
    start: Vec3,
    rng: &mut Rng,
    events: &mut Vec<MobEvent>,
) -> TurtleStep {
    let Some(state) = mob::state(store, key).copied() else {
        return TurtleStep::Idle;
    };
    if state.kind != MobKind::Turtle {
        return TurtleStep::Idle;
    }
    let home = state.home.unwrap_or_else(|| block_pos(start));
    let home_center = home_center(home);
    let home_dist_sq = start.distance_to_sqr(home_center);
    let wet = in_water(blocks, start);

    if state.laying_egg {
        let Some(site) = state.lay_egg_target else {
            cancel_laying(store, key);
            return TurtleStep::Idle;
        };
        if !can_lay_at(blocks, site) {
            cancel_laying(store, key);
            return TurtleStep::Idle;
        }
        if state.lay_egg_counter >= LAY_EGG_TICKS {
            let eggs = random_egg_count(rng);
            events.push(MobEvent::LayTurtleEgg {
                key,
                at: [site[0], site[1] + 1, site[2]],
                eggs,
            });
            events.push(MobEvent::Sound {
                at: start,
                sound: LAY_EGG_SOUND,
                volume: 0.3,
                pitch: 0.9 + rng.next_f32() * 0.2,
            });
            if let Some(state) = mob::state_mut(store, key) {
                state.has_egg = false;
                state.laying_egg = false;
                state.lay_egg_counter = 0;
                state.lay_egg_target = None;
                // `TurtleLayEggGoal.tick`: setInLoveTime(600) after a successful lay.
                state.love = LAY_AFTER_LOVE_TICKS;
            }
            return TurtleStep::Idle;
        }
        if let Some(state) = mob::state_mut(store, key) {
            state.lay_egg_counter = state.lay_egg_counter.saturating_add(1);
        }
        return TurtleStep::Laying;
    }

    // `TurtleGoHomeGoal`: it starts when carrying an egg, or rarely when an adult is far away.
    // It gives up only after 600 ticks spent within 16 blocks of home, or on arrival.
    if state.going_home {
        if state.age < 0 || home_dist_sq <= HOME_ARRIVAL_RADIUS * HOME_ARRIVAL_RADIUS {
            if let Some(state) = mob::state_mut(store, key) {
                state.going_home = false;
                state.home_trip_ticks = 0;
            }
        } else {
            let mut close = state.home_trip_ticks;
            if home_dist_sq <= 16.0 * 16.0 {
                close = close.saturating_add(1);
            }
            if close > 600 {
                if let Some(state) = mob::state_mut(store, key) {
                    state.going_home = false;
                    state.home_trip_ticks = 0;
                }
            } else {
                if let Some(state) = mob::state_mut(store, key) {
                    state.home_trip_ticks = close;
                }
                return TurtleStep::Walking {
                    target: home_center,
                    speed: 0.5,
                };
            }
        }
    }

    if state.has_egg {
        if home_dist_sq > HOME_ARRIVAL_RADIUS * HOME_ARRIVAL_RADIUS {
            if let Some(state) = mob::state_mut(store, key) {
                state.going_home = true;
                state.home_trip_ticks = 0;
            }
            return TurtleStep::Walking {
                target: home_center,
                speed: 0.5,
            };
        }

        // Keep a nest that is still valid; otherwise look for sand with an empty cell above.
        let site = if let Some(site) = state
            .lay_egg_target
            .filter(|site| can_lay_at(blocks, *site))
        {
            Some(site)
        } else {
            // Prefer the sand directly under the turtle: a turtle that reached its home beach is
            // usually already standing on the nest.
            let feet = block_pos(start);
            let under = [feet[0], feet[1] - 1, feet[2]];
            let found = if can_lay_at(blocks, under) {
                Some(under)
            } else {
                find_lay_site(blocks, home, LAY_SEARCH_RADIUS, rng)
            };
            if let Some(state) = mob::state_mut(store, key) {
                state.lay_egg_target = found;
            }
            found
        };
        let Some(site) = site else {
            return TurtleStep::Walking {
                target: home_center,
                speed: 0.5,
            };
        };

        // `Turtle$TurtleLayEggGoal.tick` requires `!isInWater()` and a reached target.
        if start.distance_to_sqr(nest_center(site)) <= LAY_REACH * LAY_REACH && !wet {
            if let Some(state) = mob::state_mut(store, key) {
                state.laying_egg = true;
                state.lay_egg_counter = 1;
            }
            return TurtleStep::Laying;
        }
        return TurtleStep::Walking {
            target: nest_center(site),
            speed: 0.5,
        };
    }

    // `TurtleGoHomeGoal`: the rare idle trip, checked before the water goal.
    if state.age >= 0 && home_dist_sq > 64.0 * 64.0 && rng.next_int(700) == 0 {
        if let Some(state) = mob::state_mut(store, key) {
            state.going_home = true;
            state.home_trip_ticks = 0;
        }
        return TurtleStep::Walking {
            target: home_center,
            speed: 0.5,
        };
    }

    if state.age < 0 {
        // `TurtleGoToWaterGoal.canUse`: babies always head for water when dry.
        return water_step(store, key, blocks, start, rng).unwrap_or(TurtleStep::Idle);
    }
    if !wet {
        if let Some(step) = water_step(store, key, blocks, start, rng) {
            return step;
        }
    } else if state.love == 0
        && let Some(step) = travel_step(store, key, blocks, start, rng)
    {
        return step;
    }
    TurtleStep::Idle
}

/// `TurtleGoToWaterGoal`: a dry turtle walks to a water block within 24 blocks.
fn water_step(
    store: &mut EntityStore,
    key: EntityKey,
    blocks: &impl BlockGetter,
    start: Vec3,
    rng: &mut Rng,
) -> Option<TurtleStep> {
    let state = *mob::state(store, key)?;
    if in_water(blocks, start) {
        if (state.water_target.is_some() || state.water_try_ticks != 0)
            && let Some(state) = mob::state_mut(store, key)
        {
            state.water_target = None;
            state.water_try_ticks = 0;
        }
        return None;
    }
    if state.water_try_ticks > 1200 {
        if let Some(state) = mob::state_mut(store, key) {
            state.water_target = None;
            state.water_try_ticks = 0;
        }
        return None;
    }
    let target = if let Some(target) = state
        .water_target
        .filter(|target| is_water_block(blocks, *target))
    {
        Some(target)
    } else {
        let found = find_water(blocks, start, WATER_SEARCH_RADIUS, rng);
        if let Some(state) = mob::state_mut(store, key) {
            state.water_target = found;
            state.water_try_ticks = 0;
        }
        found
    }?;
    if let Some(state) = mob::state_mut(store, key) {
        state.water_try_ticks = state.water_try_ticks.saturating_add(1);
    }
    Some(TurtleStep::Walking {
        target: block_center(target),
        speed: if state.age < 0 { 2.0 } else { 1.0 },
    })
}

/// `TurtleTravelGoal`: an adult without an egg travels while swimming.
fn travel_step(
    store: &mut EntityStore,
    key: EntityKey,
    blocks: &impl BlockGetter,
    start: Vec3,
    rng: &mut Rng,
) -> Option<TurtleStep> {
    let state = *mob::state(store, key)?;
    if !in_water(blocks, start) || state.has_egg || state.going_home || state.love > 0 {
        if let Some(state) = mob::state_mut(store, key) {
            state.travel_pos = None;
        }
        return None;
    }
    let target = match state.travel_pos {
        Some(target)
            if blocks
                .block_state(target[0], target[1], target[2])
                .is_some()
                && start.distance_to_sqr(block_center(target)) > 4.0 =>
        {
            target
        }
        _ => {
            // `TurtleTravelGoal.start`: +-512 x/z, +-4 y, clamped to the surface if above sea
            // level. The 34-chunk loaded-area check is approximated by the target chunk read.
            let mut target = [
                start.x.floor() as i32 + rng.next_int(1025) - 512,
                start.y.floor() as i32 + rng.next_int(9) - 4,
                start.z.floor() as i32 + rng.next_int(1025) - 512,
            ];
            if target[1] > SEA_LEVEL - 1 {
                target[1] = start.y.floor() as i32;
            }
            blocks.block_state(target[0], target[1], target[2])?;
            if let Some(state) = mob::state_mut(store, key) {
                state.travel_pos = Some(target);
            }
            target
        }
    };
    Some(TurtleStep::Walking {
        target: block_center(target),
        speed: 1.0,
    })
}

/// `TurtleGoToWaterGoal` and `TurtleTravelGoal` search ranges.
pub const WATER_SEARCH_RADIUS: i32 = 24;
/// `TurtlePanicGoal`: the water search radius when a turtle is hurt.
pub const PANIC_WATER_RADIUS: i32 = 7;
const SEA_LEVEL: i32 = 63;

/// A random water source target for the turtle water goal.
pub fn find_water(
    blocks: &impl BlockGetter,
    start: Vec3,
    radius: i32,
    rng: &mut Rng,
) -> Option<[i32; 3]> {
    let base = block_pos(start);
    let span = radius * 2 + 1;
    for _ in 0..24 {
        let at = [
            base[0] + rng.next_int(span) - radius,
            base[1] + rng.next_int(5) - 2,
            base[2] + rng.next_int(span) - radius,
        ];
        if is_water_block(blocks, at) {
            return Some(at);
        }
    }
    None
}

/// The centre of a water block where a turtle swims.
pub fn block_center(at: [i32; 3]) -> Vec3 {
    Vec3::new(
        f64::from(at[0]) + 0.5,
        f64::from(at[1]),
        f64::from(at[2]) + 0.5,
    )
}

fn is_water_block(blocks: &impl BlockGetter, at: [i32; 3]) -> bool {
    blocks
        .block_state(at[0], at[1], at[2])
        .is_some_and(is_water)
}

fn is_water(state: u16) -> bool {
    matches!(
        ferrite_data::block_props::get(state).fluid,
        Some((FluidKind::Water | FluidKind::FlowingWater, _, _))
    )
}

fn cancel_laying(store: &mut EntityStore, key: EntityKey) {
    if let Some(state) = mob::state_mut(store, key) {
        state.laying_egg = false;
        state.lay_egg_counter = 0;
        state.lay_egg_target = None;
    }
}

/// Vanilla `TurtleLayEggGoal`: `1 + random.nextInt(4)` eggs per cluster.
pub fn random_egg_count(rng: &mut Rng) -> u8 {
    MIN_EGGS + rng.next_int(i32::from(MAX_EGGS - MIN_EGGS) + 1) as u8
}

/// `TurtleEggBlock.isValidTarget`: the target is sand and the cell above it is empty.
pub fn can_lay_at(blocks: &impl BlockGetter, site: [i32; 3]) -> bool {
    blocks
        .block_state(site[0], site[1], site[2])
        .is_some_and(is_sand)
        && blocks
            .block_state(site[0], site[1] + 1, site[2])
            .is_some_and(is_air)
}

/// Searches up to 24 random cells around `home` for a valid nest (`MoveToBlockGoal.search`).
pub fn find_lay_site(
    blocks: &impl BlockGetter,
    home: [i32; 3],
    radius: i32,
    rng: &mut Rng,
) -> Option<[i32; 3]> {
    let span = radius * 2 + 1;
    for _ in 0..24 {
        let site = [
            home[0] + rng.next_int(span) - radius,
            home[1] + rng.next_int(5) - 2,
            home[2] + rng.next_int(span) - radius,
        ];
        if can_lay_at(blocks, site) {
            return Some(site);
        }
    }
    None
}

/// Whether a block state belongs to the `#minecraft:sand` tag.
pub fn is_sand(state: u16) -> bool {
    ferrite_data::blocks::state(state)
        .is_some_and(|(name, _)| ferrite_data::tags::contains("block", "minecraft:sand", name))
}

/// Whether a block state is an empty cell (`isEmptyBlock` for air and cave/void air).
pub fn is_air(state: u16) -> bool {
    ferrite_data::blocks::state(state).is_some_and(|(name, _)| {
        matches!(
            name,
            "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
        )
    })
}

fn in_water(blocks: &impl BlockGetter, start: Vec3) -> bool {
    let at = block_pos(Vec3::new(start.x, start.y + 0.1, start.z));
    blocks
        .block_state(at[0], at[1], at[2])
        .is_some_and(is_water)
}

fn block_pos(pos: Vec3) -> [i32; 3] {
    [
        pos.x.floor() as i32,
        pos.y.floor() as i32,
        pos.z.floor() as i32,
    ]
}

/// The block name and validated `(eggs, hatch)` of a `minecraft:turtle_egg` state.
pub fn egg_props(state: u16) -> Option<(u8, u8)> {
    if usize::from(state) >= ferrite_data::blocks::STATE_COUNT {
        return None;
    }
    let (name, properties) = ferrite_data::blocks::state(state)?;
    if name != "minecraft:turtle_egg" {
        return None;
    }
    let mut eggs = None;
    let mut hatch = None;
    for &(key, value) in properties {
        match key {
            "eggs" => eggs = value.parse::<u8>().ok(),
            "hatch" => hatch = value.parse::<u8>().ok(),
            _ => {}
        }
    }
    let (eggs, hatch) = (eggs?, hatch?);
    ((MIN_EGGS..=MAX_EGGS).contains(&eggs) && hatch <= MAX_HATCH).then_some((eggs, hatch))
}

/// Builds a `minecraft:turtle_egg` state, rejecting malformed property values.
pub fn egg_state(eggs: u8, hatch: u8) -> Option<u16> {
    if !(MIN_EGGS..=MAX_EGGS).contains(&eggs) || hatch > MAX_HATCH {
        return None;
    }
    let eggs = eggs.to_string();
    let hatch = hatch.to_string();
    ferrite_data::blocks::state_id(
        "turtle_egg",
        &[("eggs", eggs.as_str()), ("hatch", hatch.as_str())],
    )
}

/// The state after one egg is trampled, or `None` when the whole cluster is removed.
pub fn without_one_egg(eggs: u8, hatch: u8) -> Option<u16> {
    if eggs <= MIN_EGGS {
        None
    } else {
        egg_state(eggs - 1, hatch)
    }
}

/// One random-tick hatch step (`TurtleEggBlock.randomTick`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HatchStep {
    /// The block did not change this tick.
    None,
    /// The hatch counter advanced and the block keeps its eggs.
    Cracked(u16),
    /// The cluster is removed and `eggs` baby turtles should spawn.
    Hatched { eggs: u8 },
}

/// Rolls the night hatch chance and advances the hatch counter.
pub fn hatch_tick(state: u16, rng: &mut Rng, chance: f32) -> HatchStep {
    let Some((eggs, hatch)) = egg_props(state) else {
        return HatchStep::None;
    };
    if chance <= 0.0 || rng.next_f32() >= chance {
        return HatchStep::None;
    }
    if hatch < MAX_HATCH {
        egg_state(eggs, hatch + 1).map_or(HatchStep::None, HatchStep::Cracked)
    } else {
        HatchStep::Hatched { eggs }
    }
}

/// The overworld night window used by `Level.isBrightOutside` (12,542..23,460).
pub fn is_night(day_time: i64) -> bool {
    (12_542..23_460).contains(&day_time.rem_euclid(24_000))
}

/// `TurtleEggBlock.shouldUpdateHatchLevel` with the 26.3 timeline's late-night 1.0 window.
pub fn hatch_chance(day_time: i64) -> f32 {
    let time = day_time.rem_euclid(24_000);
    if (LATE_NIGHT_HATCH_START..LATE_NIGHT_HATCH_END).contains(&time) {
        1.0
    } else if is_night(time) {
        NIGHT_HATCH_CHANCE
    } else {
        0.0
    }
}

/// `TurtleEggBlock.destroyEgg` eligibility for a mob. Vanilla exempts turtles and bats, and
/// only lets non-player living entities crush eggs when `mobGriefing` is on. A zombie that
/// lands on a nest does not crush it, but walking over it still does.
pub fn mob_can_trample(kind: MobKind, falling: bool, griefing: bool) -> bool {
    griefing && kind != MobKind::Turtle && !(falling && kind.is_zombie_like())
}

/// Spawns the baby turtles of a hatched cluster (`TurtleEggBlock.randomTick`).
///
/// Each baby starts at `(x + 0.3 + i * 0.2, y, z + 0.3)`, faces yaw 0, is a baby
/// (`BABY_START_AGE`) and calls the egg block its home.
pub fn hatch_babies(at: [i32; 3], eggs: u8, rng: &mut Rng) -> Vec<EntityRecord> {
    let count = eggs.clamp(MIN_EGGS, MAX_EGGS);
    (0..count)
        .map(|index| {
            let pos = Vec3::new(
                f64::from(at[0]) + 0.3 + f64::from(index) * 0.2,
                f64::from(at[1]),
                f64::from(at[2]) + 0.3,
            );
            let mut turtle = mob::new_mob(MobKind::Turtle, pos, rng);
            turtle.data.yaw = 0.0;
            turtle.data.head_yaw = 0.0;
            turtle.data.pitch = 0.0;
            if let Some(state) = turtle.data.mob.as_mut() {
                state.age = crate::breed::BABY_AGE;
                state.home = Some(at);
            }
            mob::sync_state_metadata(&mut turtle.data);
            turtle.data.metadata.clear_dirty();
            turtle
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mob::MobState;
    use ferrite_protocol::packets::play::MetaValue;
    use std::collections::HashMap;

    #[derive(Default)]
    struct TestWorld {
        blocks: HashMap<(i32, i32, i32), u16>,
    }

    impl TestWorld {
        fn set(&mut self, at: [i32; 3], name: &str) {
            let state = ferrite_data::blocks::default_state(name).expect(name);
            self.blocks.insert((at[0], at[1], at[2]), state);
        }

        fn sand_platform(&mut self, center: [i32; 3], radius: i32) {
            self.set([center[0], center[1] - 1, center[2]], "sand");
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    self.set([center[0] + dx, center[1] - 1, center[2] + dz], "sand");
                }
            }
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    self.set([center[0] + dx, center[1], center[2] + dz], "air");
                }
            }
        }
    }

    impl BlockGetter for TestWorld {
        fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
            Some(
                self.blocks
                    .get(&(x, y, z))
                    .copied()
                    .unwrap_or_else(|| ferrite_data::blocks::default_state("air").unwrap()),
            )
        }
    }

    fn one_turtle(home: [i32; 3]) -> (EntityStore, EntityKey) {
        let mut store = EntityStore::default();
        let key = store.insert(mob::new_mob(
            MobKind::Turtle,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(1),
        ));
        mob::state_mut(&mut store, key).unwrap().home = Some(home);
        (store, key)
    }

    #[test]
    fn egg_states_round_trip_and_reject_malformed_values() {
        for eggs in MIN_EGGS..=MAX_EGGS {
            for hatch in 0..=MAX_HATCH {
                let state = egg_state(eggs, hatch).expect("valid state");
                assert_eq!(egg_props(state), Some((eggs, hatch)));
            }
        }
        assert_eq!(egg_state(0, 0), None);
        assert_eq!(egg_state(5, 0), None);
        assert_eq!(egg_state(1, 3), None);
        let stone = ferrite_data::blocks::default_state("stone").unwrap();
        assert_eq!(egg_props(stone), None);
        assert_eq!(egg_props(u16::MAX), None);
        // The random-tick interception is only reachable when the generated block props agree.
        assert!(ferrite_data::block_props::get(egg_state(1, 0).unwrap()).randomly_ticking);
    }

    #[test]
    fn hatch_cracks_twice_and_then_returns_the_egg_count() {
        let mut rng = Rng::new(7);
        let (eggs, _) = egg_props(egg_state(4, 0).unwrap()).unwrap();
        let first = hatch_tick(egg_state(eggs, 0).unwrap(), &mut rng, 1.0);
        let HatchStep::Cracked(next) = first else {
            panic!("expected a crack");
        };
        assert_eq!(egg_props(next), Some((4, 1)));
        let second = hatch_tick(next, &mut rng, 1.0);
        let HatchStep::Cracked(next) = second else {
            panic!("expected a second crack");
        };
        assert_eq!(egg_props(next), Some((4, 2)));
        assert_eq!(
            hatch_tick(next, &mut rng, 1.0),
            HatchStep::Hatched { eggs: 4 }
        );
        assert_eq!(hatch_tick(next, &mut rng, 0.0), HatchStep::None);
    }

    #[test]
    fn hatch_chance_is_night_only_with_the_late_night_window() {
        assert_eq!(hatch_chance(6_000), 0.0);
        assert_eq!(hatch_chance(18_000), NIGHT_HATCH_CHANCE);
        assert_eq!(hatch_chance(21_500), 1.0);
        assert_eq!(hatch_chance(23_500), 0.0);
    }

    #[test]
    fn a_turtle_returns_home_and_lays_a_cluster() {
        let mut world = TestWorld::default();
        world.sand_platform([0, 64, 0], 4);
        let home = [0, 64, 0];
        let (mut store, key) = one_turtle(home);
        {
            let state = mob::state_mut(&mut store, key).unwrap();
            state.has_egg = true;
            state.home = Some(home);
        }
        let mut rng = Rng::new(5);
        let mut events = Vec::new();
        let mut step = tick_turtle(
            &mut store,
            key,
            &world,
            Vec3::new(0.5, 64.0, 0.5),
            &mut rng,
            &mut events,
        );
        assert_eq!(step, TurtleStep::Laying);
        assert!(mob::state(&store, key).unwrap().laying_egg);
        for _ in 0..=LAY_EGG_TICKS {
            step = tick_turtle(
                &mut store,
                key,
                &world,
                Vec3::new(0.5, 64.0, 0.5),
                &mut rng,
                &mut events,
            );
        }
        assert_eq!(step, TurtleStep::Idle);
        let laid = events
            .iter()
            .find_map(|event| match event {
                MobEvent::LayTurtleEgg { at, eggs, .. } => Some((*at, *eggs)),
                _ => None,
            })
            .expect("a cluster was laid");
        assert!((MIN_EGGS..=MAX_EGGS).contains(&laid.1));
        assert_eq!(laid.0[1], 64);
        let state = mob::state(&store, key).unwrap();
        assert!(!state.has_egg);
        assert!(!state.laying_egg);
        assert_eq!(state.love, LAY_AFTER_LOVE_TICKS);
    }

    #[test]
    fn a_baby_turtle_walks_to_water_with_the_baby_speed() {
        let mut world = TestWorld::default();
        world.set([3, 64, 0], "water");
        let (mut store, key) = one_turtle([0, 64, 0]);
        {
            let state = mob::state_mut(&mut store, key).unwrap();
            state.age = crate::breed::BABY_AGE;
            state.water_target = Some([3, 64, 0]);
        }
        let mut rng = Rng::new(5);
        let mut events = Vec::new();
        let step = tick_turtle(
            &mut store,
            key,
            &world,
            Vec3::new(0.5, 64.0, 0.5),
            &mut rng,
            &mut events,
        );
        assert_eq!(
            step,
            TurtleStep::Walking {
                target: block_center([3, 64, 0]),
                speed: 2.0,
            }
        );
    }

    #[test]
    fn a_turtle_with_an_egg_retraces_its_home_beach() {
        let world = TestWorld::default();
        let (mut store, key) = one_turtle([0, 64, 20]);
        {
            let state = mob::state_mut(&mut store, key).unwrap();
            state.has_egg = true;
        }
        let mut rng = Rng::new(5);
        let mut events = Vec::new();
        let step = tick_turtle(
            &mut store,
            key,
            &world,
            Vec3::new(0.5, 64.0, 0.5),
            &mut rng,
            &mut events,
        );
        assert_eq!(
            step,
            TurtleStep::Walking {
                target: home_center([0, 64, 20]),
                speed: 0.5,
            }
        );
        assert!(mob::state(&store, key).unwrap().going_home);
    }

    #[test]
    fn a_swimming_adult_travels_to_its_far_target() {
        let mut world = TestWorld::default();
        world.set([0, 64, 0], "water");
        let (mut store, key) = one_turtle([0, 64, 0]);
        mob::state_mut(&mut store, key).unwrap().travel_pos = Some([8, 64, 0]);
        let mut rng = Rng::new(5);
        let mut events = Vec::new();
        let step = tick_turtle(
            &mut store,
            key,
            &world,
            Vec3::new(0.5, 64.0, 0.5),
            &mut rng,
            &mut events,
        );
        assert_eq!(
            step,
            TurtleStep::Walking {
                target: block_center([8, 64, 0]),
                speed: 1.0,
            }
        );
    }

    #[test]
    fn hatching_spawns_babies_with_the_egg_as_home() {
        let at = [3, 64, -2];
        let mut babies = hatch_babies(at, 4, &mut Rng::new(9));
        assert_eq!(babies.len(), 4);
        for (index, baby) in babies.iter_mut().enumerate() {
            let state = baby.data.mob.as_ref().unwrap();
            assert_eq!(state.kind, MobKind::Turtle);
            assert_eq!(state.age, crate::breed::BABY_AGE);
            assert_eq!(state.home, Some(at));
            assert_eq!(
                baby.data.metadata.get(crate::breed::BABY_INDEX),
                Some(&MetaValue::Boolean(true))
            );
            assert_eq!(
                baby.data.metadata.get(crate::metadata::turtle::HAS_EGG),
                Some(&MetaValue::Boolean(false))
            );
            assert_eq!(baby.data.yaw, 0.0);
            assert!((baby.pos.x - (f64::from(at[0]) + 0.3 + index as f64 * 0.2)).abs() < 1e-9);
        }
    }

    #[test]
    fn only_non_turtle_living_mobs_crush_eggs_and_zombies_only_by_walking() {
        assert!(!mob_can_trample(MobKind::Turtle, false, true));
        assert!(!mob_can_trample(MobKind::Zombie, false, false));
        assert!(mob_can_trample(MobKind::Zombie, false, true));
        assert!(!mob_can_trample(MobKind::Zombie, true, true));
        assert!(mob_can_trample(MobKind::Cow, true, true));
    }

    #[test]
    fn one_egg_trampled_leaves_no_block_and_more_leave_the_count_short() {
        let four = egg_state(4, 1).unwrap();
        assert_eq!(without_one_egg(4, 1), egg_state(3, 1));
        assert_eq!(without_one_egg(1, 1), None);
        assert_eq!(egg_props(four), Some((4, 1)));
    }

    #[test]
    fn a_new_turtle_starts_at_home_and_is_not_declared_test_only() {
        let mut rng = Rng::new(1);
        let turtle = mob::new_mob(MobKind::Turtle, Vec3::new(2.5, 64.0, -1.5), &mut rng);
        let state: &MobState = turtle.data.mob.as_ref().unwrap();
        assert_eq!(state.home, Some([2, 64, -2]));
        assert!(!state.has_egg);
        assert!(!state.laying_egg);
    }
}
