//! Vaults (`VaultBlock`): a trial key opens a vault once per player and the vault ejects the
//! rewards of its loot table. shortcut: the vault state (`inactive`/`active`) is not checked
//! (nothing wakes vaults up near players), no unlock/eject animation or sounds, and the
//! reward drops at once instead of one item at a time; a vault with a key already used by
//! the player is refused like vanilla.

use super::anvil::AnvilChunkSource;
use ferrite_nbt::{Compound, List, Tag};
use ferrite_world::BlockEntity;
use uuid::Uuid;

const DATA: &str = "server_data";
const REWARDED: &str = "rewarded_players";

/// The key a vault takes, and the loot table it ejects.
pub fn key_and_table(ominous: bool) -> (&'static str, &'static str) {
    if ominous {
        (
            "minecraft:ominous_trial_key",
            "minecraft:chests/trial_chambers/reward_ominous",
        )
    } else {
        (
            "minecraft:trial_key",
            "minecraft:chests/trial_chambers/reward",
        )
    }
}

fn ints(uuid: Uuid) -> Vec<i32> {
    let n = uuid.as_u128();
    vec![
        (n >> 96) as i32,
        (n >> 64) as i32,
        (n >> 32) as i32,
        n as i32,
    ]
}

/// Records `player` as rewarded by the vault at `pos`. `false` when the player already was
/// (or the chunk is not loaded), so the key is not used up.
pub fn reward_once(source: &AnvilChunkSource, [x, y, z]: [i32; 3], player: Uuid) -> bool {
    let Some(type_id) =
        ferrite_data::registries::id("minecraft:block_entity_type", "minecraft:vault")
            .and_then(|id| u32::try_from(id).ok())
    else {
        return false;
    };
    let who = ints(player);
    source
        .update_block_entity(x, y, z, |entity| {
            let entity = entity.get_or_insert_with(|| {
                let mut data = Compound::new();
                data.insert("id".into(), Tag::String("minecraft:vault".into()));
                data.insert("x".into(), Tag::Int(x));
                data.insert("y".into(), Tag::Int(y));
                data.insert("z".into(), Tag::Int(z));
                BlockEntity { type_id, data }
            });
            let server = match entity
                .data
                .entry(DATA.to_owned())
                .or_insert_with(|| Tag::Compound(Compound::new()))
            {
                Tag::Compound(c) => c,
                _ => return (false, false),
            };
            let list = server
                .entry(REWARDED.to_owned())
                .or_insert_with(|| Tag::List(List::IntArray(Vec::new())));
            match list {
                Tag::List(List::IntArray(all)) if !all.contains(&who) => {
                    all.push(who.clone());
                    (true, true)
                }
                _ => (false, false),
            }
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_player_is_rewarded_once_per_vault() {
        let source = AnvilChunkSource::new_memory();
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        assert!(reward_once(&source, [3, 70, 3], a));
        assert!(!reward_once(&source, [3, 70, 3], a));
        assert!(reward_once(&source, [3, 70, 3], b));
        assert!(reward_once(&source, [9, 70, 3], a));
    }
}
