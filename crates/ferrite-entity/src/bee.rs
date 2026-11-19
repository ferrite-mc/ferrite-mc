//! Bee special behaviour (vanilla 26.3 `Bee`):
//!
//! - **Sting** (`Bee.doHurtTarget`): an angry, unstung bee that hits a player deals its
//!   `attack_damage` (2.0), inflicts Poison (`10 s = 200 ticks` on Normal, `18 s = 360 ticks` on
//!   Hard, amplifier 0), sets `has_stung = true`, stops being angry, and plays
//!   `minecraft:entity.bee.sting`.
//! - **Post-sting death countdown** (`Bee.customServerAiStep`): after stinging, `sting_ticks`
//!   increments every tick; once `sting_ticks >= STING_DEATH_COUNTDOWN` (1200 ticks / 60 s) — or
//!   on a 1-in-500 roll after 200 ticks — the bee dies.
//! - **Not yet modelled**: beehive / bee-nest block-entity occupancy, flower pollination nectar
//!   state, and `#bee_growables` crop acceleration (no beehive block entity exists yet).

use crate::EntityKey;
use crate::ai::{MELEE_COOLDOWN, MobEvent, Target};
use crate::mob::{self, MobKind};
use crate::rng::Rng;
use crate::store::EntityStore;
use ferrite_math::Vec3;

/// `Bee.POISON_SECONDS_NORMAL` (10 s) and `POISON_SECONDS_HARD` (18 s), in ticks.
pub const POISON_TICKS_NORMAL: i32 = 10 * 20;
pub const POISON_TICKS_HARD: i32 = 18 * 20;
/// `Bee.STING_DEATH_COUNTDOWN` (1200 ticks / 60 seconds).
pub const STING_DEATH_COUNTDOWN: u16 = 1200;
/// Minimum ticks after stinging before the random early-death roll begins (10 s = 200 ticks).
pub const STING_EARLY_DEATH_START: u16 = 200;
pub const BEE_STING_SOUND: &str = "minecraft:entity.bee.sting";

/// Poison duration in ticks for a bee sting at the given difficulty.
pub const fn sting_poison_ticks(hard: bool) -> i32 {
    if hard {
        POISON_TICKS_HARD
    } else {
        POISON_TICKS_NORMAL
    }
}

/// Executes a bee's sting against `target`: emits `MobEvent::SpecialMelee` with Poison and the
/// sting sound, marks `has_stung = true`, and clears anger/target.
pub fn sting_player(
    store: &mut EntityStore,
    key: EntityKey,
    target: Target,
    start: Vec3,
    hard: bool,
    events: &mut Vec<MobEvent>,
) {
    events.push(MobEvent::SpecialMelee {
        player: target.uuid,
        damage: MobKind::Bee.attack_damage(),
        from: start,
        attacker: key,
        knockback_xz: 0.4,
        launch_y: 0.0,
        poison_ticks: sting_poison_ticks(hard),
    });
    events.push(MobEvent::Sound {
        at: start,
        sound: BEE_STING_SOUND,
        volume: 1.0,
        pitch: 1.0,
    });
    if let Some(state) = mob::state_mut(store, key) {
        state.has_stung = true;
        state.anger = 0;
        state.target = None;
        state.attack_cooldown = MELEE_COOLDOWN;
    }
}

/// Advances a stung bee's death countdown (`Bee.customServerAiStep`).
/// Returns `true` if the bee died this tick.
pub fn tick_post_sting(store: &mut EntityStore, key: EntityKey, rng: &mut Rng) -> bool {
    let mut should_die = false;
    if let Some(state) = mob::state_mut(store, key) {
        if !state.has_stung {
            return false;
        }
        state.sting_ticks = state.sting_ticks.saturating_add(1);
        if state.sting_ticks >= STING_DEATH_COUNTDOWN
            || (state.sting_ticks > STING_EARLY_DEATH_START && rng.next_int(500) == 0)
        {
            should_die = true;
        }
    }
    if should_die {
        mob::kill(store, key);
        true
    } else {
        false
    }
}
