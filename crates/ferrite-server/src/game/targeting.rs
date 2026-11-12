//! Which players hostile mobs may pick as a target. Vanilla's `NearestAttackableTargetGoal`
//! uses `TargetingConditions` with `isAttackable`, which excludes players that are creative or
//! spectator (`Player.isCreative`/`isSpectator` via `canBeSeenAsEnemy`/`isInvulnerable`).
//!
//! Mobs of one region read other regions' players only through the shared position snapshot,
//! which carries no game mode. Each owning region therefore publishes whether its players are
//! untargetable into this small table (bounded by the online player count, cleared when the
//! player leaves) and target lists are filtered through it.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, MutexGuard};

use uuid::Uuid;

static UNTARGETABLE: Mutex<Option<HashSet<Uuid>>> = Mutex::new(None);

fn table() -> MutexGuard<'static, Option<HashSet<Uuid>>> {
    UNTARGETABLE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Creative (1) and spectator (3) players are never chosen as a mob target.
pub fn is_untargetable_mode(gamemode: i32) -> bool {
    gamemode == 1 || gamemode == 3
}

/// Called by the owning region every tick for each of its players.
pub fn publish(player: Uuid, gamemode: i32) {
    let mut table = table();
    let set = table.get_or_insert_with(HashSet::new);
    if is_untargetable_mode(gamemode) {
        set.insert(player);
    } else {
        set.remove(&player);
    }
}

/// A player left the server.
pub fn forget(player: Uuid) {
    if let Some(set) = table().as_mut() {
        set.remove(&player);
    }
}

/// Drops the players mobs may not target from a position map.
pub fn retain_targetable<V>(players: &mut HashMap<Uuid, V>) {
    if let Some(set) = table().as_ref() {
        players.retain(|uuid, _| !set.contains(uuid));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creative_and_spectator_players_are_dropped_from_mob_targets() {
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
        let mut players: HashMap<Uuid, u8> = [survival, creative, spectator, adventure]
            .into_iter()
            .map(|u| (u, 0))
            .collect();
        retain_targetable(&mut players);
        let mut kept: Vec<_> = players.keys().copied().collect();
        kept.sort();
        assert_eq!(kept, vec![survival, adventure]);
        // Switching back to survival makes the player targetable again, and leaving clears it.
        publish(creative, 0);
        publish(spectator, 3);
        forget(spectator);
        let mut again: HashMap<Uuid, u8> =
            [creative, spectator].into_iter().map(|u| (u, 0)).collect();
        retain_targetable(&mut again);
        assert_eq!(again.len(), 2);
        for u in [survival, creative, adventure] {
            forget(u);
        }
    }
}
