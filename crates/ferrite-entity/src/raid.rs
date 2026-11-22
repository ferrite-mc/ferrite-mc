//! Village raids: the Bad Omen trigger and the wave composition vanilla `Raid` uses.
//!
//! The server's region owns the live raid (spawning, wave clearing, victory). This module holds
//! the deterministic parts: how many waves a Bad Omen level asks for, and which raiders a wave
//! brings. Wave composition follows the vanilla `Raid.RaiderType` groups closely enough to be
//! testable: pillagers first, vindicators from the second wave, then evokers, ravagers and the
//! witches that heal them.
//!
//! Vanilla reference values: `Raids.RAID_REMOVE_DELAY`, `Raid.getNumGroups` with the Bad Omen
//! amplifier, and `RaiderType` spawn counts. The exact per-difficulty counts are simplified to
//! fixed groups; every raider spawned this way still uses the ordinary mob AI.

use crate::mob::MobKind;

/// Bad Omen lasts this long when a player kills a raid captain (`Raids` gives 120000 ticks).
pub const BAD_OMEN_DURATION: i32 = 120_000;
/// Hero of the Village lasts 48000 ticks at Bad Omen I (`Raid` victory reward).
pub const HERO_DURATION: i32 = 48_000;
/// A player with Bad Omen this close to a villager starts a raid (`Raids.isVillage` box).
pub const VILLAGE_RANGE: f64 = 96.0;
/// Ticks between the last raider of a wave and the next wave.
pub const WAVE_BREAK_TICKS: u32 = 300;
/// A raid that cannot finish in this many ticks is abandoned.
pub const RAID_TIMEOUT_TICKS: u32 = 48_000;
/// How long surviving raiders celebrate before the raid is removed.
pub const CELEBRATION_TICKS: u32 = 600;
/// Raiders spawn this far from the village centre.
pub const SPAWN_RADIUS: f64 = 32.0;
/// Vanilla raids only happen in the Overworld.
pub const RAID_DIMENSION: &str = "overworld";

/// Number of waves for a Bad Omen amplifier (level I is 3 waves, each extra level adds one).
pub fn wave_count(bad_omen_amplifier: i32) -> u32 {
    3 + bad_omen_amplifier.clamp(0, 2) as u32
}

/// The raiders of one wave, in spawn order. The first raider of the first wave is the captain,
/// whose death gives the killer Bad Omen.
pub fn wave_raiders(wave: u32) -> Vec<MobKind> {
    let mut raiders = vec![MobKind::Pillager, MobKind::Pillager];
    if wave >= 1 {
        raiders.push(MobKind::Vindicator);
        raiders.push(MobKind::Vindicator);
    }
    if wave >= 2 {
        raiders.push(MobKind::Evoker);
        raiders.push(MobKind::Ravager);
        // Witches join to heal the raiders and are the reason `Raider.IS_CELEBRATING` is
        // visible on a witch after a victory.
        raiders.push(MobKind::Witch);
    }
    if wave >= 3 {
        raiders.push(MobKind::Ravager);
        raiders.push(MobKind::Witch);
    }
    raiders
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_omen_level_adds_waves() {
        assert_eq!(wave_count(0), 3);
        assert_eq!(wave_count(1), 4);
        assert_eq!(wave_count(2), 5);
        assert_eq!(wave_count(9), 5, "the level is capped like vanilla");
    }

    #[test]
    fn waves_grow_from_pillagers_to_evokers_and_witches() {
        let first = wave_raiders(0);
        assert_eq!(first, vec![MobKind::Pillager, MobKind::Pillager]);
        assert!(first.iter().all(|kind| kind.is_raider()));

        let second = wave_raiders(1);
        assert!(second.contains(&MobKind::Vindicator));
        assert!(!second.contains(&MobKind::Witch), "witches join later");

        let third = wave_raiders(2);
        assert!(third.contains(&MobKind::Evoker));
        assert!(third.contains(&MobKind::Ravager));
        assert!(third.contains(&MobKind::Witch));
        assert!(third.iter().all(|kind| kind.is_raider()));

        assert!(wave_raiders(3).len() > third.len());
    }

    #[test]
    fn every_raid_has_a_captain_and_a_celebrating_witch_later() {
        // The server marks `wave_raiders(0)[0]` as the captain. Later waves include a witch,
        // whose `CELEBRATING` metadata is already wired in `mob::sync_state_metadata`.
        assert_eq!(wave_raiders(0)[0], MobKind::Pillager);
        assert!(wave_raiders(2).contains(&MobKind::Witch));
        assert_eq!(RAID_DIMENSION, "overworld");
    }
}
