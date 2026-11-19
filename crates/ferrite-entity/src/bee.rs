//! Bee special behaviour (vanilla 26.3 `Bee`):
//!
//! - **Sting** (`Bee.doHurtTarget`): an angry, unstung bee that hits a player deals its
//!   `attack_damage` (2.0), inflicts Poison (`10 s = 200 ticks` on Normal, `18 s = 360 ticks` on
//!   Hard, amplifier 0), sets `has_stung = true`, stops being angry, and plays
//!   `minecraft:entity.bee.sting`.
//! - **Post-sting death countdown** (`Bee.customServerAiStep`): after stinging, `sting_ticks`
//!   increments every tick; once `sting_ticks >= STING_DEATH_COUNTDOWN` (1200 ticks / 60 s) — or
//!   on a 1-in-500 roll after 200 ticks — the bee dies.
//! - **Synched flags** (`Bee.DATA_FLAGS_ID`): bit `2` is the roll animation, bit `4` is
//!   `has_stung`, bit `8` is `has_nectar`.
//!
//! Hive occupancy, home/flower positions, pollination and `#bee_growables` crop growth are
//! modelled by `ferrite-server`'s `game::beehive`; this module owns the parts that travel with
//! the entity itself, including the flag byte and the NBT field names vanilla uses.

use crate::EntityKey;
use crate::ai::{MELEE_COOLDOWN, MobEvent, Target};
use crate::mob::{self, MobKind, MobState};
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

/// `Bee.FLAG_ROLL`.
pub const FLAG_ROLL: i8 = 2;
/// `Bee.FLAG_HAS_STUNG`.
pub const FLAG_HAS_STUNG: i8 = 4;
/// `Bee.FLAG_HAS_NECTAR`.
pub const FLAG_HAS_NECTAR: i8 = 8;

/// `Bee.MAX_CROPS_GROWABLE`: a bee grows at most ten crops between pollinations.
pub const MAX_CROPS_GROWN: u8 = 10;
/// `Bee.TICKS_WITHOUT_NECTAR_BEFORE_GOING_HOME` (3600 ticks / 3 minutes).
pub const TICKS_WITHOUT_NECTAR_BEFORE_GOING_HOME: u32 = 3600;
/// `Bee.TICKS_BEFORE_GOING_TO_KNOWN_FLOWER` (600 ticks / 30 seconds).
pub const TICKS_BEFORE_GOING_TO_KNOWN_FLOWER: u32 = 600;
/// `Bee.HIVE_SEARCH_DISTANCE` (20 blocks).
pub const HIVE_SEARCH_DISTANCE: f64 = 20.0;
/// `Bee.HIVE_CLOSE_ENOUGH_DISTANCE` (2 blocks).
pub const HIVE_CLOSE_ENOUGH_DISTANCE: f64 = 2.0;
/// `Bee.TOO_FAR_DISTANCE` (48 blocks): beyond this a bee forgets its hive.
pub const TOO_FAR_DISTANCE: f64 = 48.0;
/// `Bee.MIN_TICKS_BEFORE_REENTERING_HIVE`: a disturbed bee stays away for `400` ticks.
pub const MIN_TICKS_BEFORE_REENTERING_HIVE: u16 = 400;

/// Poison duration in ticks for a bee sting at the given difficulty.
pub const fn sting_poison_ticks(hard: bool) -> i32 {
    if hard {
        POISON_TICKS_HARD
    } else {
        POISON_TICKS_NORMAL
    }
}

/// The synched `Bee.DATA_FLAGS_ID` byte for `state`.
pub fn metadata_flags(state: &MobState) -> i8 {
    let mut flags = 0;
    if state.has_stung {
        flags |= FLAG_HAS_STUNG;
    }
    if state.has_nectar {
        flags |= FLAG_HAS_NECTAR;
    }
    flags
}

/// Writes the synched flag byte from the authoritative [`MobState`].
pub fn sync_flags(store: &mut EntityStore, key: EntityKey) {
    let Some(state) = mob::state(store, key) else {
        return;
    };
    let flags = metadata_flags(state);
    let Some(data) = store.data_mut(key) else {
        return;
    };
    data.metadata.set(
        crate::metadata::bee::FLAGS,
        ferrite_protocol::packets::play::MetaValue::Byte(flags),
    );
}

/// Sets whether a bee carries nectar and keeps the synched flag byte current.
pub fn set_has_nectar(store: &mut EntityStore, key: EntityKey, has_nectar: bool) {
    if let Some(state) = mob::state_mut(store, key) {
        state.has_nectar = has_nectar;
        if has_nectar {
            state.crops_grown = 0;
            state.ticks_since_pollination = 0;
        }
    }
    sync_flags(store, key);
}

/// `Bee.dropOffNectar`: the nectar is delivered to a hive and the crop budget resets.
pub fn drop_off_nectar(store: &mut EntityStore, key: EntityKey) {
    if let Some(state) = mob::state_mut(store, key) {
        state.has_nectar = false;
        state.crops_grown = 0;
        state.ticks_since_pollination = 0;
        state.ticks_without_nectar = 0;
    }
    sync_flags(store, key);
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
    sync_flags(store, key);
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
