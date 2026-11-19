//! Snow Golem and Iron Golem special behaviour (vanilla 26.3 `SnowGolem` and `IronGolem`).
//!
//! - **Snow Golem**: targets nearby hostile mobs (`NearestAttackableTargetGoal` for `Enemy`) within
//!   10 blocks and throws snowballs every 20 ticks (`RangedAttackGoal`, uncertainty 12.0); leaves
//!   a snow layer trail at its four foot corners when `mobGriefing` is enabled (emitted as
//!   `MobEvent::SnowTrail` for region-owned block placement).
//! - **Iron Golem**: defends against nearby hostile mobs (excluding creepers) and retaliates
//!   against players who hurt it; melee hits roll `base / 2 + nextInt(base)` (7.5..=21.5 for
//!   default `attack_damage = 15`), add `+0.4` vertical launch (`doHurtTarget`), emit entity
//!   status `4` (attack arm swing) and play `minecraft:entity.iron_golem.attack`.

use crate::EntityKey;
use crate::ai::{MELEE_COOLDOWN, MobEvent, Target};
use crate::mob::{self, MobKind};
use crate::physics::BlockGetter;
use crate::rng::Rng;
use crate::store::EntityStore;
use ferrite_math::Vec3;

/// `SnowGolem` `RangedAttackGoal(this, 1.25, 20, 10.0F)`.
pub const SNOW_GOLEM_RANGE: f64 = 10.0;
pub const SNOW_GOLEM_COOLDOWN: u16 = 20;
pub const SNOW_GOLEM_SPEED: f64 = 1.25;
/// `SnowGolem.performRangedAttack` launch speed (`1.6F`) and `rangedAttackUncertainty` (`12.0F`).
pub const SNOWBALL_SPEED: f64 = 1.6;
pub const SNOWBALL_UNCERTAINTY: f64 = 12.0;
pub const SNOW_GOLEM_SHOOT_SOUND: &str = "minecraft:entity.snow_golem.shoot";
/// `SoundEvents.SNOW_GOLEM_SHEAR` (`SnowGolem.shear`).
pub const SNOW_GOLEM_SHEAR_SOUND: &str = "minecraft:entity.snow_golem.shear";

/// `IronGolem.doHurtTarget` vertical launch added on top of knockback (`0.4000000059604645D`).
pub const IRON_GOLEM_LAUNCH_Y: f64 = 0.4;
/// Entity status byte broadcast on an Iron Golem attack (`broadcastEntityEvent(this, 4)`).
pub const IRON_GOLEM_ATTACK_STATUS: u8 = 4;
pub const IRON_GOLEM_ATTACK_SOUND: &str = "minecraft:entity.iron_golem.attack";

/// `SnowGolem.readyForShearing`: it wears a pumpkin. Snow golems have no age gate.
pub fn ready_for_shearing(store: &EntityStore, key: EntityKey) -> bool {
    mob::state(store, key).is_some_and(|s| s.kind == MobKind::SnowGolem && !s.sheared)
}

/// `SnowGolem.shear` bookkeeping: the pumpkin is removed and the synched `PUMPKIN` byte is
/// updated. `None` when `key` is not a snow golem, `Some(false)` when it has no pumpkin, and
/// `Some(true)` when it was sheared. The caller drops `shearing/snow_golem` and wears the shears.
pub fn shear(store: &mut EntityStore, key: EntityKey) -> Option<bool> {
    let state = mob::state(store, key)?;
    if state.kind != MobKind::SnowGolem {
        return None;
    }
    if state.sheared {
        return Some(false);
    }
    if let Some(state) = mob::state_mut(store, key) {
        state.sheared = true;
    }
    if let Some(data) = store.data_mut(key) {
        mob::sync_state_metadata(data);
    }
    Some(true)
}

/// Rolls Iron Golem melee damage: `base / 2.0 + nextInt(base as i32)` (`IronGolem.doHurtTarget`).
pub fn iron_golem_damage(rng: &mut Rng) -> f32 {
    let base = MobKind::IronGolem.attack_damage();
    let span = base as i32;
    if span > 0 {
        base / 2.0 + rng.next_int(span) as f32
    } else {
        base
    }
}

/// The up-to-4 deduplicated foot-corner block positions where `SnowGolem.aiStep` attempts to
/// place a snow layer (`x ± 0.25`, `y`, `z ± 0.25`).
pub fn snow_trail_positions(pos: Vec3) -> Vec<[i32; 3]> {
    let mut out = Vec::with_capacity(4);
    let y = pos.y.floor() as i32;
    for i in 0..4 {
        let ox = if i % 2 == 0 { 0.25 } else { -0.25 };
        let oz = if (i / 2) % 2 == 0 { 0.25 } else { -0.25 };
        let candidate = [(pos.x + ox).floor() as i32, y, (pos.z + oz).floor() as i32];
        if !out.contains(&candidate) {
            out.push(candidate);
        }
    }
    out
}

/// Emits `MobEvent::SnowTrail` for each foot-corner air block resting on a solid block when
/// `griefing` is enabled and the snow golem is on the ground.
pub fn emit_snow_trail(
    pos: Vec3,
    on_ground: bool,
    griefing: bool,
    blocks: &impl BlockGetter,
    events: &mut Vec<MobEvent>,
) {
    if !on_ground || !griefing {
        return;
    }
    for at in snow_trail_positions(pos) {
        let is_air = blocks.block_state(at[0], at[1], at[2]) == Some(0);
        let below_solid = blocks
            .block_state(at[0], at[1] - 1, at[2])
            .is_some_and(crate::spawn::is_solid);
        if is_air && below_solid {
            events.push(MobEvent::SnowTrail { at });
        }
    }
}

/// Finds the nearest alive hostile mob in `store` within `range` blocks horizontally and 8 blocks
/// vertically. When `exclude_creeper` is true, creepers are skipped (vanilla `IronGolem`).
pub fn nearest_hostile_mob(
    store: &EntityStore,
    self_key: EntityKey,
    pos: Vec3,
    range: f64,
    exclude_creeper: bool,
) -> Option<(EntityKey, Vec3)> {
    let max_sqr = range * range;
    store
        .keys()
        .iter()
        .copied()
        .filter(|&other| other != self_key)
        .filter_map(|other| {
            let state = mob::state(store, other)?;
            if state.is_dead() || !state.kind.is_hostile() {
                return None;
            }
            if exclude_creeper && state.kind == MobKind::Creeper {
                return None;
            }
            let p = store.pos(other)?;
            let dx = p.x - pos.x;
            let dz = p.z - pos.z;
            let dist_sqr = dx * dx + dz * dz;
            if dist_sqr > max_sqr || (p.y - pos.y).abs() > 8.0 {
                return None;
            }
            Some((other, p, dist_sqr))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(k, p, _)| (k, p))
}

/// Computes the snowball velocity vector from `from` to `target_pos` (with target eye height),
/// matching `SnowGolem.performRangedAttack` (`dy = target_eye - 1.1 - from.y + horiz * 0.2`,
/// speed 1.6, Gaussian-like spread from `SNOWBALL_UNCERTAINTY`).
pub fn snowball_velocity(from: Vec3, target_pos: Vec3, target_eye: f64, rng: &mut Rng) -> Vec3 {
    let dx = target_pos.x - from.x;
    let dz = target_pos.z - from.z;
    let horiz = (dx * dx + dz * dz).sqrt();
    let dy = (target_pos.y + target_eye - 1.1) - from.y + horiz * 0.2;
    let spread = SNOWBALL_UNCERTAINTY * 0.0075;
    let dir = Vec3::new(dx, dy, dz).normalize();
    let noisy = Vec3::new(
        dir.x + (rng.next_f64() - 0.5) * 2.0 * spread,
        dir.y + (rng.next_f64() - 0.5) * 2.0 * spread,
        dir.z + (rng.next_f64() - 0.5) * 2.0 * spread,
    );
    noisy.normalize() * SNOWBALL_SPEED
}

/// Runs one tick of a Snow Golem's ranged attack goal against the nearest hostile mob.
/// Returns `Some((walk_target, speed))` when the golem needs to move closer, or `None` when it
/// holds position to throw (or has no target). Also returns `Option<Vec3>` look target.
pub fn tick_snow_golem(
    store: &mut EntityStore,
    key: EntityKey,
    blocks: &impl BlockGetter,
    start: Vec3,
    rng: &mut Rng,
    events: &mut Vec<MobEvent>,
) -> Option<(Option<Vec3>, Vec3)> {
    let (target_key, target_pos) =
        nearest_hostile_mob(store, key, start, MobKind::SnowGolem.follow_range(), false)?;
    let target_eye =
        mob::state(store, target_key).map_or(1.0, |s| f64::from(s.kind.props().eye_height));
    let eye_height = f64::from(MobKind::SnowGolem.props().eye_height);
    let dx = target_pos.x - start.x;
    let dz = target_pos.z - start.z;
    let horiz = (dx * dx + dz * dz).sqrt();
    let has_los = crate::ai::has_line_of_sight(blocks, start, eye_height, target_pos);
    if horiz <= SNOW_GOLEM_RANGE && has_los {
        let ready = mob::state(store, key).is_some_and(|s| s.bow_timer == 0);
        if ready {
            let from = Vec3::new(start.x, start.y + eye_height, start.z);
            let velocity = snowball_velocity(from, target_pos, target_eye, rng);
            let owner = store.data(key).map(|d| d.uuid);
            events.push(MobEvent::Snowball {
                from,
                velocity,
                owner,
            });
            events.push(MobEvent::Sound {
                at: start,
                sound: SNOW_GOLEM_SHOOT_SOUND,
                volume: 1.0,
                pitch: 0.4 / (rng.next_f32() * 0.4 + 0.8),
            });
            if let Some(state) = mob::state_mut(store, key) {
                state.bow_timer = SNOW_GOLEM_COOLDOWN;
            }
        }
        Some((None, target_pos))
    } else {
        Some((Some(target_pos), target_pos))
    }
}

/// Executes an Iron Golem's heavy melee hit against a player (`SpecialMelee` with `+0.4` vertical
/// fling, rolled damage `7.5..=21.5`, entity status `4`, and attack sound).
pub fn iron_golem_hit_player(
    store: &mut EntityStore,
    key: EntityKey,
    target: Target,
    start: Vec3,
    rng: &mut Rng,
    events: &mut Vec<MobEvent>,
) {
    let damage = iron_golem_damage(rng);
    events.push(MobEvent::SpecialMelee {
        player: target.uuid,
        damage,
        from: start,
        attacker: key,
        knockback_xz: 0.4,
        launch_y: IRON_GOLEM_LAUNCH_Y,
        poison_ticks: 0,
    });
    events.push(MobEvent::Status {
        key,
        status: IRON_GOLEM_ATTACK_STATUS,
    });
    events.push(MobEvent::Sound {
        at: start,
        sound: IRON_GOLEM_ATTACK_SOUND,
        volume: 1.0,
        pitch: 1.0,
    });
    if let Some(state) = mob::state_mut(store, key) {
        state.attack_cooldown = MELEE_COOLDOWN;
    }
}

/// Executes an Iron Golem's heavy melee hit against a hostile mob in the same region.
pub fn iron_golem_hit_mob(
    store: &mut EntityStore,
    key: EntityKey,
    victim: EntityKey,
    start: Vec3,
    rng: &mut Rng,
    events: &mut Vec<MobEvent>,
) {
    let damage = iron_golem_damage(rng);
    let hurt = mob::hurt(store, victim, damage, Some(start));
    if matches!(hurt, Some(mob::Hurt::Survived { .. })) {
        // Apply vanilla `IronGolem.doHurtTarget` +0.4 vertical fling on top of knockback.
        mob::knockback_with(store, victim, start, 0.4, IRON_GOLEM_LAUNCH_Y);
    }
    events.push(MobEvent::Status {
        key,
        status: IRON_GOLEM_ATTACK_STATUS,
    });
    events.push(MobEvent::Sound {
        at: start,
        sound: IRON_GOLEM_ATTACK_SOUND,
        volume: 1.0,
        pitch: 1.0,
    });
    if let Some(state) = mob::state_mut(store, key) {
        state.attack_cooldown = MELEE_COOLDOWN;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iron_golem_damage_stays_in_vanilla_range() {
        let mut rng = Rng::new(42);
        let mut min = f32::MAX;
        let mut max = f32::MIN;
        for _ in 0..200 {
            let d = iron_golem_damage(&mut rng);
            assert!((7.5..=21.5).contains(&d), "unexpected damage {d}");
            min = min.min(d);
            max = max.max(d);
        }
        assert!(max > min, "iron golem damage should vary");
    }

    #[test]
    fn shearing_a_snow_golem_clears_the_pumpkin_metadata_once() {
        let mut store = EntityStore::default();
        let mut rng = Rng::new(7);
        let key = store.insert(crate::mob::new_mob(
            MobKind::SnowGolem,
            Vec3::new(1.5, 64.0, 1.5),
            &mut rng,
        ));
        assert!(ready_for_shearing(&store, key));
        assert_eq!(
            store
                .data(key)
                .unwrap()
                .metadata
                .get(crate::metadata::snow_golem::PUMPKIN),
            Some(&ferrite_protocol::packets::play::MetaValue::Byte(
                crate::metadata::snow_golem::FLAG_HAS_PUMPKIN
            ))
        );
        assert_eq!(shear(&mut store, key), Some(true));
        assert!(mob::state(&store, key).unwrap().sheared);
        assert!(!ready_for_shearing(&store, key));
        assert_eq!(
            store
                .data(key)
                .unwrap()
                .metadata
                .get(crate::metadata::snow_golem::PUMPKIN),
            Some(&ferrite_protocol::packets::play::MetaValue::Byte(0))
        );
        assert_eq!(shear(&mut store, key), Some(false));

        let cow = store.insert(crate::mob::new_mob(
            MobKind::Cow,
            Vec3::new(4.5, 64.0, 1.5),
            &mut rng,
        ));
        assert_eq!(shear(&mut store, cow), None);
    }

    #[test]
    fn snow_trail_positions_are_deduplicated_around_feet() {
        let centered = snow_trail_positions(Vec3::new(0.5, 64.0, 0.5));
        assert_eq!(centered, vec![[0, 64, 0]]);

        let corner = snow_trail_positions(Vec3::new(1.0, 64.0, 1.0));
        assert_eq!(corner.len(), 4);
        assert!(corner.contains(&[1, 64, 1]));
        assert!(corner.contains(&[0, 64, 1]));
        assert!(corner.contains(&[1, 64, 0]));
        assert!(corner.contains(&[0, 64, 0]));
    }
}
