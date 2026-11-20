//! Frog special behaviour (vanilla 26.3 `Frog`, `FrogAi`, `ShootTongue`):
//!
//! - **Long jump** (`FrogAi`): `MAX_LONG_JUMP_HEIGHT = 2`, `MAX_LONG_JUMP_WIDTH = 4`, cooldown
//!   `100..140` ticks (`TIME_BETWEEN_LONG_JUMPS`), fall damage reduced by 5 blocks in
//!   [`crate::mob::apply_fall_damage`].
//! - **Tongue catch** (`ShootTongue`): `EATING_DISTANCE = 1.75`, `TONGUE_ANIMATION_DURATION = 10`,
//!   `CATCH_ANIMATION_DURATION = 6`. In vanilla, `Frog.canEat` matches tiny `Slime` and tiny
//!   `MagmaCube` (`cube_size <= 1`). Both kinds are in [`MobKind`], [`is_edible_cube`] checks
//!   size `<= 1`, and [`froglight_for_variant`] maps the frog's variant to its froglight drop.

use crate::EntityKey;
use crate::ai::MobEvent;
use crate::mob::{self, MobKind};
use crate::physics::BlockGetter;
use crate::rng::Rng;
use crate::store::EntityStore;
use ferrite_math::Vec3;

/// `FrogAi.MAX_LONG_JUMP_HEIGHT` (2) and `MAX_LONG_JUMP_WIDTH` (4).
pub const LONG_JUMP_HEIGHT: i32 = 2;
pub const LONG_JUMP_WIDTH: i32 = 4;
/// Vanilla `FrogAi.TIME_BETWEEN_LONG_JUMPS` = 100..140 ticks.
pub const LONG_JUMP_COOLDOWN_MIN: u16 = 100;
pub const LONG_JUMP_COOLDOWN_SPAN: i32 = 40;
pub const LONG_JUMP_SOUND: &str = "minecraft:entity.frog.long_jump";

/// `ShootTongue` constants from the 26.3 jar.
pub const EATING_DISTANCE: f64 = 1.75;
pub const TONGUE_ANIMATION_DURATION: u16 = 10;
pub const CATCH_ANIMATION_DURATION: u16 = 6;
pub const TONGUE_SOUND: &str = "minecraft:entity.frog.tongue";
pub const EAT_SOUND: &str = "minecraft:entity.frog.eat";

const PHASE_IDLE: u8 = 0;
const PHASE_TONGUE: u8 = 1;
const PHASE_CATCH: u8 = 2;

/// Whether vanilla `Frog.canEat` allows eating `kind` (when tiny, see [`is_edible_cube`]).
pub const fn is_edible_prey(kind: MobKind) -> bool {
    matches!(kind, MobKind::Slime | MobKind::MagmaCube)
}

/// Whether a mob of `kind` and `cube_size` is tiny enough for a frog to eat (`AbstractCubeMob.isTiny`).
pub const fn is_edible_cube(kind: MobKind, cube_size: u8) -> bool {
    is_edible_prey(kind) && cube_size <= 1
}

/// Vanilla froglight item dropped when a frog of `variant` (`0=cold, 1=temperate, 2=warm`) eats a magma cube.
pub const fn froglight_for_variant(variant: Option<u8>) -> &'static str {
    match variant {
        Some(0) => "minecraft:verdant_froglight",
        Some(2) => "minecraft:pearlescent_froglight",
        _ => "minecraft:ochre_froglight",
    }
}

/// Outcome of one frog tongue-catch step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TongueStep {
    /// Target out of range or dead; idle.
    Idle,
    /// Approaching target to get within `EATING_DISTANCE`.
    Approach,
    /// Tongue animation playing (`TONGUE_ANIMATION_DURATION` ticks).
    Extending,
    /// Pulling target in (`CATCH_ANIMATION_DURATION` ticks).
    Catching,
    /// Prey was caught and removed.
    Eaten,
}

/// Runs one tick of the frog's long-jump goal when idle on the ground.
/// Returns `true` when the frog launched a long jump this tick.
pub fn tick_frog_long_jump(
    store: &mut EntityStore,
    key: EntityKey,
    blocks: &impl BlockGetter,
    start: Vec3,
    on_ground: bool,
    rng: &mut Rng,
    events: &mut Vec<MobEvent>,
) -> bool {
    let jump_cd = {
        let Some(state) = mob::state_mut(store, key) else {
            return false;
        };
        state.long_jump_cooldown = state.long_jump_cooldown.saturating_sub(1);
        state.long_jump_cooldown
    };
    if !on_ground || jump_cd > 0 {
        return false;
    }
    if let Some(vel) =
        crate::goat::pick_long_jump_velocity(blocks, start, LONG_JUMP_WIDTH, LONG_JUMP_HEIGHT, rng)
    {
        store.set_vel(key, vel);
        if let Some(state) = mob::state_mut(store, key) {
            state.long_jump_cooldown =
                LONG_JUMP_COOLDOWN_MIN + rng.next_int(LONG_JUMP_COOLDOWN_SPAN) as u16;
        }
        events.push(MobEvent::Sound {
            at: start,
            sound: LONG_JUMP_SOUND,
            volume: 1.0,
            pitch: 1.0,
        });
        return true;
    }
    false
}

/// Advances the `ShootTongue` state machine against an explicit `prey` entity key.
///
/// Used directly by unit tests (and ready to wire into `ai.rs` as soon as `Slime`/`MagmaCube`
/// are added to [`MobKind`]).
pub fn tick_tongue_against(
    store: &mut EntityStore,
    key: EntityKey,
    prey: EntityKey,
    events: &mut Vec<MobEvent>,
) -> TongueStep {
    let Some(start) = store.pos(key) else {
        return TongueStep::Idle;
    };
    let Some(prey_pos) = store.pos(prey) else {
        reset_tongue(store, key);
        return TongueStep::Idle;
    };
    if !mob::is_mob(store, prey) || mob::state(store, prey).is_none_or(|s| s.is_dead()) {
        reset_tongue(store, key);
        return TongueStep::Idle;
    }
    let dist = start.distance_to(prey_pos);
    let (phase, timer) =
        mob::state(store, key).map_or((PHASE_IDLE, 0), |s| (s.ram_phase, s.special_timer));

    match phase {
        PHASE_TONGUE => {
            if timer <= 1 {
                if let Some(state) = mob::state_mut(store, key) {
                    state.ram_phase = PHASE_CATCH;
                    state.special_timer = CATCH_ANIMATION_DURATION;
                }
                return TongueStep::Catching;
            }
            if let Some(state) = mob::state_mut(store, key) {
                state.special_timer = timer - 1;
            }
            TongueStep::Extending
        }
        PHASE_CATCH => {
            if timer <= 1 {
                mob::kill(store, prey);
                events.push(MobEvent::Sound {
                    at: start,
                    sound: EAT_SOUND,
                    volume: 1.0,
                    pitch: 1.0,
                });
                reset_tongue(store, key);
                return TongueStep::Eaten;
            }
            if let Some(state) = mob::state_mut(store, key) {
                state.special_timer = timer - 1;
            }
            TongueStep::Catching
        }
        _ => {
            if dist <= EATING_DISTANCE {
                if let Some(state) = mob::state_mut(store, key) {
                    state.ram_phase = PHASE_TONGUE;
                    state.special_timer = TONGUE_ANIMATION_DURATION;
                }
                events.push(MobEvent::Sound {
                    at: start,
                    sound: TONGUE_SOUND,
                    volume: 1.0,
                    pitch: 1.0,
                });
                TongueStep::Extending
            } else {
                TongueStep::Approach
            }
        }
    }
}

fn reset_tongue(store: &mut EntityStore, key: EntityKey) {
    if let Some(state) = mob::state_mut(store, key) {
        state.ram_phase = PHASE_IDLE;
        state.special_timer = 0;
    }
}
