//! Fox special behaviour (vanilla 26.3 `Fox`, `FoxPounceGoal`, `StalkPreyGoal`):
//!
//! - **Prey hunting & pounce**: hunts nearby chickens and rabbits within 12 blocks. Inside
//!   `2.0..=6.0` blocks on the ground with line of sight, crouches for `CROUCH_TICKS = 15` ticks
//!   and then pounces with vanilla `FoxPounceGoal.start()` velocity
//!   (`vel + Vec3(dir.x * 0.8, 0.9, dir.z * 0.8)`). Bites prey in melee reach for
//!   `MobKind::Fox.attack_damage()` (2.0).
//! - **Fleeing players**: flees nearby players within `FOX_FLEE_RADIUS = 8.0` blocks at
//!   `FOX_FLEE_SPEED = 1.6` when not tempted.

use crate::EntityKey;
use crate::ai::{MELEE_COOLDOWN, Target};
use crate::mob::{self, MobKind};
use crate::physics::BlockGetter;
use crate::store::EntityStore;
use ferrite_math::Vec3;

/// Prey detection range for a fox.
pub const PREY_RANGE: f64 = 12.0;
/// Distance band in which a fox crouches and pounces toward prey.
pub const POUNCE_MIN_DIST: f64 = 2.0;
pub const POUNCE_MAX_DIST: f64 = 6.0;
/// Ticks spent crouching before launching the pounce.
pub const CROUCH_TICKS: u16 = 15;
/// Cooldown after a pounce before crouching again.
pub const POUNCE_COOLDOWN: u16 = 40;
/// Melee reach for biting prey.
pub const BITE_REACH: f64 = 1.6;
/// Vanilla `FoxPounceGoal.start()` horizontal and vertical impulse.
pub const POUNCE_XZ: f64 = 0.8;
pub const POUNCE_Y: f64 = 0.9;
/// Distance at which an untrusted fox flees a player.
pub const FOX_FLEE_RADIUS: f64 = 8.0;
pub const FOX_FLEE_SPEED: f64 = 1.6;

const PHASE_IDLE: u8 = 0;
const PHASE_CROUCH: u8 = 1;

/// Outcome of one fox special-AI tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FoxStep {
    /// No special movement; fall back to normal passive goals.
    Idle,
    /// Fleeing a nearby player along `(dx, dz)` at `FOX_FLEE_SPEED`.
    Flee { away: Vec3 },
    /// Stalking/walking toward prey at `target`.
    Stalk { target: Vec3 },
    /// Crouching in place before pouncing at `target`.
    Crouching { target: Vec3 },
    /// Pounced this tick (velocity already applied to the store).
    Pounced { target: Vec3 },
}

/// Vanilla prey kinds modelled in Ferrite (`Chicken`, `Rabbit`, baby `Turtle`).
pub fn is_fox_prey(kind: MobKind, age: i32) -> bool {
    matches!(kind, MobKind::Chicken | MobKind::Rabbit) || (kind == MobKind::Turtle && age < 0)
}

/// Computes the vanilla `FoxPounceGoal.start()` velocity update.
pub fn pounce_velocity(current: Vec3, from: Vec3, target: Vec3) -> Vec3 {
    let dir = (target - from).normalize();
    Vec3::new(
        current.x + dir.x * POUNCE_XZ,
        current.y + POUNCE_Y,
        current.z + dir.z * POUNCE_XZ,
    )
}

/// Runs one tick of a fox's flee, stalk, pounce, and bite behaviour.
pub fn tick_fox(
    store: &mut EntityStore,
    key: EntityKey,
    blocks: &impl BlockGetter,
    players: &[Target],
    start: Vec3,
    on_ground: bool,
) -> FoxStep {
    let (phase, timer, cooldown) = {
        let Some(state) = mob::state_mut(store, key) else {
            return FoxStep::Idle;
        };
        state.special_cooldown = state.special_cooldown.saturating_sub(1);
        (state.ram_phase, state.special_timer, state.special_cooldown)
    };

    // Flee nearby players first (unless currently mid-crouch at prey? No, vanilla flees players).
    if let Some(nearest) = players
        .iter()
        .filter(|p| {
            let dx = p.pos.x - start.x;
            let dz = p.pos.z - start.z;
            dx * dx + dz * dz <= FOX_FLEE_RADIUS * FOX_FLEE_RADIUS
                && (p.pos.y - start.y).abs() <= 4.0
        })
        .min_by(|a, b| {
            let da = (a.pos.x - start.x).powi(2) + (a.pos.z - start.z).powi(2);
            let db = (b.pos.x - start.x).powi(2) + (b.pos.z - start.z).powi(2);
            da.total_cmp(&db)
        })
    {
        reset_crouch(store, key);
        let away = Vec3::new(start.x - nearest.pos.x, 0.0, start.z - nearest.pos.z);
        return FoxStep::Flee { away };
    }

    // Hunt nearest prey in the same region.
    let Some((prey_key, prey_pos)) = nearest_prey(store, key, start) else {
        reset_crouch(store, key);
        return FoxStep::Idle;
    };

    let dx = prey_pos.x - start.x;
    let dz = prey_pos.z - start.z;
    let horiz = (dx * dx + dz * dz).sqrt();

    if horiz <= BITE_REACH && (prey_pos.y - start.y).abs() <= 1.5 {
        reset_crouch(store, key);
        let ready = mob::state(store, key).is_some_and(|s| s.attack_cooldown == 0);
        if ready {
            mob::hurt(store, prey_key, MobKind::Fox.attack_damage(), Some(start));
            if let Some(state) = mob::state_mut(store, key) {
                state.attack_cooldown = MELEE_COOLDOWN;
            }
        }
        return FoxStep::Stalk { target: prey_pos };
    }

    let eye = f64::from(MobKind::Fox.props().eye_height);
    let in_pounce_band = (POUNCE_MIN_DIST..=POUNCE_MAX_DIST).contains(&horiz);
    let has_los = crate::ai::has_line_of_sight(blocks, start, eye, prey_pos);

    if phase == PHASE_CROUCH {
        if !on_ground || !in_pounce_band || !has_los {
            reset_crouch(store, key);
            return FoxStep::Stalk { target: prey_pos };
        }
        if timer <= 1 {
            let cur = store.vel(key).unwrap_or_default();
            store.set_vel(key, pounce_velocity(cur, start, prey_pos));
            if let Some(state) = mob::state_mut(store, key) {
                state.ram_phase = PHASE_IDLE;
                state.special_timer = 0;
                state.special_cooldown = POUNCE_COOLDOWN;
            }
            return FoxStep::Pounced { target: prey_pos };
        }
        if let Some(state) = mob::state_mut(store, key) {
            state.special_timer = timer - 1;
        }
        return FoxStep::Crouching { target: prey_pos };
    }

    if on_ground && cooldown == 0 && in_pounce_band && has_los {
        if let Some(state) = mob::state_mut(store, key) {
            state.ram_phase = PHASE_CROUCH;
            state.special_timer = CROUCH_TICKS;
        }
        return FoxStep::Crouching { target: prey_pos };
    }

    FoxStep::Stalk { target: prey_pos }
}

fn reset_crouch(store: &mut EntityStore, key: EntityKey) {
    if let Some(state) = mob::state_mut(store, key) {
        state.ram_phase = PHASE_IDLE;
        state.special_timer = 0;
    }
}

fn nearest_prey(
    store: &EntityStore,
    self_key: EntityKey,
    start: Vec3,
) -> Option<(EntityKey, Vec3)> {
    let max_sqr = PREY_RANGE * PREY_RANGE;
    store
        .keys()
        .iter()
        .copied()
        .filter(|&other| other != self_key)
        .filter_map(|other| {
            let state = mob::state(store, other)?;
            if state.is_dead() || !is_fox_prey(state.kind, state.age) {
                return None;
            }
            let pos = store.pos(other)?;
            let dx = pos.x - start.x;
            let dz = pos.z - start.z;
            let d2 = dx * dx + dz * dz;
            if d2 > max_sqr || (pos.y - start.y).abs() > 6.0 {
                return None;
            }
            Some((other, pos, d2))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(k, p, _)| (k, p))
}
