//! Goat special behaviour (vanilla 26.3 `Goat` and `GoatAi`):
//!
//! - **Ram / charge** (`PrepareRamNearestTarget` / `RamTarget`): scans for a player or non-goat
//!   living mob within `[4.0, 7.0]` blocks, prepares for `RAM_PREPARE_TIME = 20` ticks, then
//!   charges at `RAM_SPEED_MULT = 3.0`. On impact, deals `2.0` (adult) or `1.0` (baby) damage and
//!   strong knockback (`2.5` adult / `1.0` baby horizontal force + `0.4` upward lift).
//! - **Long jump** (`LongJumpToRandomPos`): when idle on ground and `long_jump_cooldown == 0`,
//!   picks a reachable solid landing spot within `±5` blocks horizontally and `-2..=+5` vertically
//!   and launches with a parabolic velocity toward it. Fall damage is reduced by 10 blocks in
//!   [`crate::mob::apply_fall_damage`].

use crate::EntityKey;
use crate::ai::{MobEvent, Target};
use crate::mob::{self, MobKind};
use crate::physics::BlockGetter;
use crate::rng::Rng;
use crate::store::{EntityStore, Flags};
use ferrite_math::Vec3;

/// `GoatAi.RAM_PREPARE_TIME` (20 ticks).
pub const RAM_PREPARE_TIME: u16 = 20;
/// `GoatAi.RAM_MIN_DISTANCE` and `RAM_MAX_DISTANCE` (4..=7 blocks).
pub const RAM_MIN_DISTANCE: f64 = 4.0;
pub const RAM_MAX_DISTANCE: f64 = 7.0;
/// `GoatAi.SPEED_MULTIPLIER_WHEN_RAMMING` (3.0).
pub const RAM_SPEED_MULT: f64 = 3.0;
/// `Goat.ADULT_ATTACK_DAMAGE` (2) and `BABY_ATTACK_DAMAGE` (1).
pub const ADULT_RAM_DAMAGE: f32 = 2.0;
pub const BABY_RAM_DAMAGE: f32 = 1.0;
/// `GoatAi.ADULT_RAM_KNOCKBACK_FORCE` (2.5) and `BABY_RAM_KNOCKBACK_FORCE` (1.0).
pub const ADULT_RAM_KNOCKBACK: f64 = 2.5;
pub const BABY_RAM_KNOCKBACK: f64 = 1.0;
/// Maximum ticks a charge runs before timing out if it misses.
pub const RAM_CHARGE_TIMEOUT: u16 = 40;
/// Impact radius for landing a ram hit.
pub const RAM_IMPACT_DISTANCE: f64 = 1.6;
/// Cooldown after a ram finishes (vanilla `GoatAi.TIME_BETWEEN_RAMS` = 600..6000).
pub const RAM_COOLDOWN_MIN: u16 = 600;
pub const RAM_COOLDOWN_SPAN: i32 = 5400;
/// A screaming goat rams far more often (vanilla `TIME_BETWEEN_RAMS_SCREAMER` = 100..300).
pub const SCREAMING_RAM_COOLDOWN_MIN: u16 = 100;
pub const SCREAMING_RAM_COOLDOWN_SPAN: i32 = 200;
/// `Goat.GOAT_SCREAMING_CHANCE` (2%).
pub const SCREAMING_CHANCE: f32 = 0.02;
/// `Goat.UNIHORN_CHANCE` (10% of adults spawn missing one horn).
pub const UNIHORN_CHANCE: f32 = 0.1;

/// `GoatAi.MAX_LONG_JUMP_HEIGHT` (5) and `MAX_LONG_JUMP_WIDTH` (5).
pub const LONG_JUMP_HEIGHT: i32 = 5;
pub const LONG_JUMP_WIDTH: i32 = 5;
/// Vanilla `TIME_BETWEEN_LONG_JUMPS` = 600..1200.
pub const LONG_JUMP_COOLDOWN_MIN: u16 = 600;
pub const LONG_JUMP_COOLDOWN_SPAN: i32 = 600;

pub const RAM_PREPARE_SOUND: &str = "minecraft:entity.goat.prepare_ram";
pub const RAM_IMPACT_SOUND: &str = "minecraft:entity.goat.ram_impact";
pub const LONG_JUMP_SOUND: &str = "minecraft:entity.goat.long_jump";
pub const SCREAMING_RAM_PREPARE_SOUND: &str = "minecraft:entity.goat.screaming.prepare_ram";
pub const SCREAMING_RAM_IMPACT_SOUND: &str = "minecraft:entity.goat.screaming.ram_impact";
pub const SCREAMING_LONG_JUMP_SOUND: &str = "minecraft:entity.goat.screaming.long_jump";

/// `GoatAi.initMemories`: the initial ram cooldown, which is much shorter for screamers.
pub fn ram_cooldown(screaming: bool, rng: &mut Rng) -> u16 {
    let (min, span) = if screaming {
        (SCREAMING_RAM_COOLDOWN_MIN, SCREAMING_RAM_COOLDOWN_SPAN)
    } else {
        (RAM_COOLDOWN_MIN, RAM_COOLDOWN_SPAN)
    };
    min + rng.next_int(span) as u16
}

/// `Goat.getBreedOffspring`: one parent is chosen at random; if it is a screamer the baby is
/// too, otherwise the baby still has a 2% scream roll.
pub fn offspring_screaming(parent_a: bool, parent_b: bool, rng: &mut Rng) -> bool {
    let parent = if rng.next_int(2) == 0 {
        parent_a
    } else {
        parent_b
    };
    parent || rng.next_f32() < SCREAMING_CHANCE
}

fn prepare_sound(screaming: bool) -> &'static str {
    if screaming {
        SCREAMING_RAM_PREPARE_SOUND
    } else {
        RAM_PREPARE_SOUND
    }
}

fn impact_sound(screaming: bool) -> &'static str {
    if screaming {
        SCREAMING_RAM_IMPACT_SOUND
    } else {
        RAM_IMPACT_SOUND
    }
}

fn long_jump_sound(screaming: bool) -> &'static str {
    if screaming {
        SCREAMING_LONG_JUMP_SOUND
    } else {
        LONG_JUMP_SOUND
    }
}

const PHASE_IDLE: u8 = 0;
const PHASE_PREPARE: u8 = 1;
const PHASE_CHARGE: u8 = 2;

/// Outcome of one goat special-AI tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GoatStep {
    /// No special goal active this tick; run normal passive goals.
    Idle,
    /// Preparing to ram: stand still and face `target`.
    Preparing { target: Vec3 },
    /// Charging toward `target` at `RAM_SPEED_MULT`.
    Charging { target: Vec3 },
    /// Launched a long jump this tick (velocity already applied to the store).
    LongJumped,
}

/// Runs one tick of a goat's ram/charge and long-jump goals.
pub fn tick_goat(
    store: &mut EntityStore,
    key: EntityKey,
    blocks: &impl BlockGetter,
    players: &[Target],
    on_ground: bool,
    rng: &mut Rng,
    events: &mut Vec<MobEvent>,
) -> GoatStep {
    let start = store.pos(key).unwrap_or_default();
    let (phase, timer, cooldown, jump_cd, ram_target, is_baby, is_screaming) = {
        let Some(state) = mob::state_mut(store, key) else {
            return GoatStep::Idle;
        };
        state.special_cooldown = state.special_cooldown.saturating_sub(1);
        state.long_jump_cooldown = state.long_jump_cooldown.saturating_sub(1);
        (
            state.ram_phase,
            state.special_timer,
            state.special_cooldown,
            state.long_jump_cooldown,
            state.ram_target,
            state.age < 0,
            state.screaming,
        )
    };

    match phase {
        PHASE_PREPARE => {
            let Some(target) = ram_target else {
                reset_ram(store, key, rng);
                return GoatStep::Idle;
            };
            if timer <= 1 {
                if let Some(state) = mob::state_mut(store, key) {
                    state.ram_phase = PHASE_CHARGE;
                    state.special_timer = RAM_CHARGE_TIMEOUT;
                }
                return GoatStep::Charging { target };
            }
            if let Some(state) = mob::state_mut(store, key) {
                state.special_timer = timer - 1;
            }
            GoatStep::Preparing { target }
        }
        PHASE_CHARGE => {
            let Some(target) = ram_target else {
                reset_ram(store, key, rng);
                return GoatStep::Idle;
            };
            // Check for impact with any player or non-goat living mob within impact reach.
            let damage = if is_baby {
                BABY_RAM_DAMAGE
            } else {
                ADULT_RAM_DAMAGE
            };
            let knockback = if is_baby {
                BABY_RAM_KNOCKBACK
            } else {
                ADULT_RAM_KNOCKBACK
            };
            if let Some(hit_player) = players.iter().find(|p| {
                let dx = p.pos.x - start.x;
                let dz = p.pos.z - start.z;
                dx * dx + dz * dz <= RAM_IMPACT_DISTANCE * RAM_IMPACT_DISTANCE
                    && (p.pos.y - start.y).abs() <= 2.0
            }) {
                events.push(MobEvent::SpecialMelee {
                    player: hit_player.uuid,
                    damage,
                    from: start,
                    attacker: key,
                    knockback_xz: knockback,
                    launch_y: 0.4,
                    poison_ticks: 0,
                });
                events.push(MobEvent::Sound {
                    at: start,
                    sound: impact_sound(is_screaming),
                    volume: 1.0,
                    pitch: 1.0,
                });
                reset_ram(store, key, rng);
                return GoatStep::Idle;
            }
            if let Some(victim) = find_ram_mob_impact(store, key, start) {
                let _ = mob::hurt(store, victim, damage, Some(start));
                mob::knockback_with(store, victim, start, knockback, 0.4);
                events.push(MobEvent::Sound {
                    at: start,
                    sound: impact_sound(is_screaming),
                    volume: 1.0,
                    pitch: 1.0,
                });
                reset_ram(store, key, rng);
                return GoatStep::Idle;
            }
            let hit_wall = store
                .flags(key)
                .is_some_and(|f| f.has(Flags::HORIZONTAL_COLLISION));
            let reached = (target.x - start.x).powi(2) + (target.z - start.z).powi(2) <= 1.0;
            if timer <= 1 || hit_wall || reached {
                if hit_wall && hits_snap_block(blocks, start, target) {
                    events.push(MobEvent::GoatSnapHorn {
                        key,
                        at: start,
                        is_screaming,
                    });
                }
                reset_ram(store, key, rng);
                return GoatStep::Idle;
            }
            if let Some(state) = mob::state_mut(store, key) {
                state.special_timer = timer - 1;
            }
            GoatStep::Charging { target }
        }
        _ => {
            // Try starting a ram when cooldown is ready and on ground.
            if on_ground
                && cooldown == 0
                && let Some(target) = pick_ram_target(store, key, blocks, players, start)
            {
                if let Some(state) = mob::state_mut(store, key) {
                    state.ram_phase = PHASE_PREPARE;
                    state.special_timer = RAM_PREPARE_TIME;
                    state.ram_target = Some(target);
                }
                events.push(MobEvent::Sound {
                    at: start,
                    sound: prepare_sound(is_screaming),
                    volume: 1.0,
                    pitch: 1.0,
                });
                return GoatStep::Preparing { target };
            }
            // Otherwise try a long jump when cooldown is ready and on ground.
            if on_ground
                && jump_cd == 0
                && let Some(vel) =
                    pick_long_jump_velocity(blocks, start, LONG_JUMP_WIDTH, LONG_JUMP_HEIGHT, rng)
            {
                store.set_vel(key, vel);
                if let Some(state) = mob::state_mut(store, key) {
                    state.long_jump_cooldown =
                        LONG_JUMP_COOLDOWN_MIN + rng.next_int(LONG_JUMP_COOLDOWN_SPAN) as u16;
                }
                events.push(MobEvent::Sound {
                    at: start,
                    sound: long_jump_sound(is_screaming),
                    volume: 1.0,
                    pitch: 1.0,
                });
                return GoatStep::LongJumped;
            }
            GoatStep::Idle
        }
    }
}

fn hits_snap_block(blocks: &impl BlockGetter, start: Vec3, target: Vec3) -> bool {
    let dir = Vec3::new(target.x - start.x, 0.0, target.z - start.z);
    let len = dir.length_sqr().sqrt();
    if len <= 1.0e-6 {
        return false;
    }
    let nx = (start.x + (dir.x / len) * 0.7).floor() as i32;
    let ny = start.y.floor() as i32;
    let nz = (start.z + (dir.z / len) * 0.7).floor() as i32;
    for dy in [0, 1] {
        if let Some(state) = blocks.block_state(nx, ny + dy, nz)
            && let Some((name, _)) = ferrite_data::blocks::state(state)
            && ferrite_data::tags::contains(
                "block",
                "snaps_goat_horn",
                name.strip_prefix("minecraft:").unwrap_or(name),
            )
        {
            return true;
        }
    }
    false
}

fn reset_ram(store: &mut EntityStore, key: EntityKey, rng: &mut Rng) {
    if let Some(state) = mob::state_mut(store, key) {
        state.ram_phase = PHASE_IDLE;
        state.special_timer = 0;
        state.ram_target = None;
        state.special_cooldown = ram_cooldown(state.screaming, rng);
    }
}

fn find_ram_mob_impact(store: &EntityStore, self_key: EntityKey, start: Vec3) -> Option<EntityKey> {
    let max_sqr = RAM_IMPACT_DISTANCE * RAM_IMPACT_DISTANCE;
    store.keys().iter().copied().find(|&other| {
        if other == self_key {
            return false;
        }
        let Some(state) = mob::state(store, other) else {
            return false;
        };
        if state.is_dead() || state.kind == MobKind::Goat {
            return false;
        }
        store.pos(other).is_some_and(|p| {
            let dx = p.x - start.x;
            let dz = p.z - start.z;
            dx * dx + dz * dz <= max_sqr && (p.y - start.y).abs() <= 2.0
        })
    })
}

fn pick_ram_target(
    store: &EntityStore,
    self_key: EntityKey,
    blocks: &impl BlockGetter,
    players: &[Target],
    start: Vec3,
) -> Option<Vec3> {
    let eye = f64::from(MobKind::Goat.props().eye_height);
    let min_sqr = RAM_MIN_DISTANCE * RAM_MIN_DISTANCE;
    let max_sqr = RAM_MAX_DISTANCE * RAM_MAX_DISTANCE;
    let mut best: Option<(f64, Vec3)> = None;

    for p in players {
        let dx = p.pos.x - start.x;
        let dz = p.pos.z - start.z;
        let d2 = dx * dx + dz * dz;
        if (min_sqr..=max_sqr).contains(&d2)
            && (p.pos.y - start.y).abs() <= 2.0
            && crate::ai::has_line_of_sight(blocks, start, eye, p.pos)
            && best.is_none_or(|(bd, _)| d2 < bd)
        {
            best = Some((d2, p.pos));
        }
    }
    for &other in store.keys() {
        if other == self_key {
            continue;
        }
        let Some(state) = mob::state(store, other) else {
            continue;
        };
        if state.is_dead() || state.kind == MobKind::Goat {
            continue;
        }
        let Some(pos) = store.pos(other) else {
            continue;
        };
        let dx = pos.x - start.x;
        let dz = pos.z - start.z;
        let d2 = dx * dx + dz * dz;
        if (min_sqr..=max_sqr).contains(&d2)
            && (pos.y - start.y).abs() <= 2.0
            && crate::ai::has_line_of_sight(blocks, start, eye, pos)
            && best.is_none_or(|(bd, _)| d2 < bd)
        {
            best = Some((d2, pos));
        }
    }
    best.map(|(_, pos)| pos)
}

/// Samples up to 10 candidate landing spots within `±width` horizontally and `-1..=+height`
/// vertically (solid floor with two air blocks above) and computes a ballistic launch velocity.
/// Reused by both Goat and Frog long-jump goals.
pub fn pick_long_jump_velocity(
    blocks: &impl BlockGetter,
    start: Vec3,
    width: i32,
    height: i32,
    rng: &mut Rng,
) -> Option<Vec3> {
    let bx = start.x.floor() as i32;
    let by = start.y.floor() as i32;
    let bz = start.z.floor() as i32;
    let span_xz = width * 2 + 1;
    let span_y = height + 2;
    for _ in 0..10 {
        let ox = rng.next_int(span_xz) - width;
        let oz = rng.next_int(span_xz) - width;
        if ox * ox + oz * oz < 4 {
            continue;
        }
        let oy = rng.next_int(span_y) - 1;
        let tx = bx + ox;
        let ty = by + oy;
        let tz = bz + oz;
        let solid = |x: i32, y: i32, z: i32| {
            blocks
                .block_state(x, y, z)
                .is_some_and(crate::spawn::is_solid)
        };
        if solid(tx, ty - 1, tz) && !solid(tx, ty, tz) && !solid(tx, ty + 1, tz) {
            let target = Vec3::new(f64::from(tx) + 0.5, f64::from(ty), f64::from(tz) + 0.5);
            return Some(ballistic_jump(start, target));
        }
    }
    None
}

/// Computes a bounded parabolic jump velocity from `start` to `target` under `GRAVITY = 0.08`.
pub fn ballistic_jump(start: Vec3, target: Vec3) -> Vec3 {
    let dx = target.x - start.x;
    let dy = target.y - start.y;
    let dz = target.z - start.z;
    let horiz = (dx * dx + dz * dz).sqrt().max(0.1);
    let flight_ticks = (horiz * 3.0).clamp(10.0, 22.0);
    let vx = (dx / flight_ticks).clamp(-0.65, 0.65);
    let vz = (dz / flight_ticks).clamp(-0.65, 0.65);
    let vy = ((dy + 0.5 * crate::ai::GRAVITY * flight_ticks * flight_ticks) / flight_ticks)
        .clamp(0.45, 0.95);
    Vec3::new(vx, vy, vz)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ram_cooldowns_follow_the_screaming_ranges() {
        let mut rng = Rng::new(31);
        for _ in 0..200 {
            assert!((600..6000).contains(&ram_cooldown(false, &mut rng)));
            assert!((100..300).contains(&ram_cooldown(true, &mut rng)));
        }
    }

    #[test]
    fn screaming_offspring_inherit_or_roll_two_percent() {
        let mut rng = Rng::new(32);
        for _ in 0..100 {
            assert!(offspring_screaming(true, true, &mut rng));
        }
        let mut screamers = 0;
        for _ in 0..4_000 {
            screamers += usize::from(offspring_screaming(false, false, &mut rng));
        }
        assert!((40..140).contains(&screamers), "{screamers}");
        // One screamer parent: a 50% pick plus the 2% roll on the other parent (~51%).
        let mut half = 0;
        for _ in 0..2_000 {
            half += usize::from(offspring_screaming(true, false, &mut rng));
        }
        assert!((900..1_200).contains(&half), "{half}");
    }
}
