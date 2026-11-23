//! Which players hostile mobs may pick as a target. Vanilla's `NearestAttackableTargetGoal`
//! uses `TargetingConditions` with `isAttackable`, which excludes players that are creative or
//! spectator (`Player.isCreative`/`isSpectator` via `canBeSeenAsEnemy`/`isInvulnerable`).
//!
//! Mobs of one region read other regions' players only through the shared position snapshot,
//! which carries no game mode. Each owning region therefore publishes whether its players are
//! untargetable into this small table (bounded by the online player count, cleared when the
//! player leaves) and target lists are filtered through it.

use std::collections::{HashMap, HashSet};

use ferrite_entity::ai::PlayerSnapshot;
use ferrite_entity::enderman::Gaze;
use std::sync::{Mutex, MutexGuard};

use uuid::Uuid;

static UNTARGETABLE: Mutex<Option<HashSet<Uuid>>> = Mutex::new(None);
/// Where each player looks, for endermen in other regions (see [`publish_gaze`]).
static GAZES: Mutex<Option<HashMap<Uuid, Gaze>>> = Mutex::new(None);
/// Per-player health, effect flags, velocity, and golden armor state for cross-region mobs.
static SNAPSHOTS: Mutex<Option<HashMap<Uuid, PlayerSnapshot>>> = Mutex::new(None);

fn table() -> MutexGuard<'static, Option<HashSet<Uuid>>> {
    UNTARGETABLE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Creative (1) and spectator (3) players are never chosen as a mob target.
pub fn is_untargetable_mode(gamemode: i32) -> bool {
    gamemode == 1 || gamemode == 3
}

/// Batches one region's players behind a single lock.
pub fn publish_many(players: impl IntoIterator<Item = (Uuid, i32)>) {
    let mut table = table();
    let set = table.get_or_insert_with(HashSet::new);
    for (player, gamemode) in players {
        if is_untargetable_mode(gamemode) {
            set.insert(player);
        } else {
            set.remove(&player);
        }
    }
}

#[cfg(test)]
pub fn publish(player: Uuid, gamemode: i32) {
    publish_many([(player, gamemode)]);
}

/// A player left the server.
pub fn forget(player: Uuid) {
    if let Some(set) = table().as_mut() {
        set.remove(&player);
    }
    if let Some(map) = GAZES.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        map.remove(&player);
    }
    if let Some(map) = SNAPSHOTS.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        map.remove(&player);
    }
}

/// Batches one region's snapshots of health, active potion flags, velocity and golden armor
/// behind a single lock for cross-region witches and piglins.
pub fn publish_snapshots(snapshots: impl IntoIterator<Item = (Uuid, PlayerSnapshot)>) {
    let mut lock = SNAPSHOTS.lock().unwrap_or_else(|e| e.into_inner());
    let map = lock.get_or_insert_with(HashMap::new);
    for (player, snapshot) in snapshots {
        map.insert(player, snapshot);
    }
}

#[cfg(test)]
pub fn publish_snapshot(player: Uuid, snapshot: PlayerSnapshot) {
    publish_snapshots([(player, snapshot)]);
}

/// The last published snapshots of every online player.
pub fn snapshots() -> HashMap<Uuid, PlayerSnapshot> {
    SNAPSHOTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_default()
}

/// Batches the view vector and carved-pumpkin state of one region's players behind a single
/// lock. An enderman in another region reads them from here, the same way it reads
/// [`publish_many`]ed game modes.
pub fn publish_gazes(gazes: impl IntoIterator<Item = (Uuid, Gaze)>) {
    let mut lock = GAZES.lock().unwrap_or_else(|e| e.into_inner());
    let map = lock.get_or_insert_with(HashMap::new);
    for (player, gaze) in gazes {
        map.insert(player, gaze);
    }
}

#[cfg(test)]
pub fn publish_gaze(player: Uuid, gaze: Gaze) {
    publish_gazes([(player, gaze)]);
}

/// The last published gaze of every player.
pub fn gazes() -> HashMap<Uuid, Gaze> {
    GAZES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_default()
}

/// The players hostile mobs may not target right now.
pub fn untargetable() -> HashSet<Uuid> {
    table().clone().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creative_and_spectator_players_are_published_until_they_change_mode_or_leave() {
        let (survival, creative, spectator, adventure) = (
            Uuid::from_u128(0x7a01),
            Uuid::from_u128(0x7a02),
            Uuid::from_u128(0x7a03),
            Uuid::from_u128(0x7a04),
        );
        publish(survival, 0);
        publish(creative, 1);
        publish(adventure, 2);
        publish(spectator, 3);
        let set = untargetable();
        assert!(!set.contains(&survival) && !set.contains(&adventure));
        assert!(set.contains(&creative) && set.contains(&spectator));
        publish(creative, 0);
        forget(spectator);
        let set = untargetable();
        assert!(!set.contains(&creative) && !set.contains(&spectator));
        for u in [survival, creative, adventure] {
            forget(u);
        }
    }
}
