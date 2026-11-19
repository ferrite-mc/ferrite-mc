//! Rabbit special behaviour (vanilla 26.3 `Rabbit`):
//!
//! - **Hop movement**: when moving on the ground, a rabbit hops in discrete intervals
//!   (`JUMP_DELAY_TICKS = 10`, or `PANIC_JUMP_DELAY_TICKS = 3` when panicking/fleeing) rather
//!   than sliding smoothly, applying vertical jump strength and a forward hop impulse.
//! - **Fleeing players**: flees players within `RABBIT_FLEE_RADIUS = 8.0` blocks at
//!   `FLEE_SPEED_MOD = 2.2` unless tempted.

use crate::EntityKey;
use crate::ai::Target;
use crate::mob::{self, MobKind};
use crate::store::EntityStore;
use ferrite_math::Vec3;

/// `Rabbit.JUMP_DELAY_TICKS` (10) and `PANIC_JUMP_DELAY_TICKS` (3).
pub const JUMP_DELAY_TICKS: u16 = 10;
pub const PANIC_JUMP_DELAY_TICKS: u16 = 3;
/// `Rabbit.FLEE_SPEED_MOD` (2.2) and flee radius (`8.0` blocks).
pub const FLEE_SPEED_MOD: f64 = 2.2;
pub const RABBIT_FLEE_RADIUS: f64 = 8.0;
/// Forward horizontal burst applied on each hop launch.
pub const HOP_HORIZONTAL_IMPULSE: f64 = 0.18;

/// Returns a flee direction away from the nearest player within `RABBIT_FLEE_RADIUS`, if any.
pub fn flee_player_direction(start: Vec3, players: &[Target]) -> Option<Vec3> {
    let nearest = players
        .iter()
        .filter(|p| {
            let dx = p.pos.x - start.x;
            let dz = p.pos.z - start.z;
            dx * dx + dz * dz <= RABBIT_FLEE_RADIUS * RABBIT_FLEE_RADIUS
                && (p.pos.y - start.y).abs() <= 4.0
        })
        .min_by(|a, b| {
            let da = (a.pos.x - start.x).powi(2) + (a.pos.z - start.z).powi(2);
            let db = (b.pos.x - start.x).powi(2) + (b.pos.z - start.z).powi(2);
            da.total_cmp(&db)
        })?;
    Some(Vec3::new(
        start.x - nearest.pos.x,
        0.0,
        start.z - nearest.pos.z,
    ))
}

/// Applies rabbit hop gating to `walking` and `vel` when `was_on_ground`.
///
/// When `walking` is `Some(yaw)` and the rabbit is on the ground:
/// - If `special_cooldown == 0`: triggers a hop (`vel.y = max(jump_strength)`, adds horizontal
///   burst along `yaw`), and sets `special_cooldown` to `PANIC_JUMP_DELAY_TICKS` (when panicking
///   or fleeing) or `JUMP_DELAY_TICKS`.
/// - Otherwise: decrements `special_cooldown` and returns `None` (pausing ground movement until
///   the next hop).
pub fn apply_hop(
    store: &mut EntityStore,
    key: EntityKey,
    walking: Option<f32>,
    was_on_ground: bool,
    urgent: bool,
    vel: &mut Vec3,
) -> Option<f32> {
    let yaw = walking?;
    if !was_on_ground {
        return Some(yaw);
    }
    let delay = mob::state(store, key).map_or(0, |s| s.special_cooldown);
    if delay > 0 {
        if let Some(state) = mob::state_mut(store, key) {
            state.special_cooldown = delay - 1;
        }
        return None;
    }
    let (sin, cos) = f64::from(yaw).to_radians().sin_cos();
    let boost = if urgent {
        HOP_HORIZONTAL_IMPULSE * 1.4
    } else {
        HOP_HORIZONTAL_IMPULSE
    };
    vel.x -= sin * boost;
    vel.z += cos * boost;
    vel.y = vel.y.max(MobKind::Rabbit.jump_strength());
    if let Some(state) = mob::state_mut(store, key) {
        state.special_cooldown = if urgent {
            PANIC_JUMP_DELAY_TICKS
        } else {
            JUMP_DELAY_TICKS
        };
    }
    Some(yaw)
}
