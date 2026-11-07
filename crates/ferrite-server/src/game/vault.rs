//! Vaults (`VaultBlock`): a trial key opens a vault once per player and the vault ejects the
//! rewards of its loot table. shortcut: the vault state (`inactive`/`active`) is not checked
//! (nothing wakes vaults up near players), no unlock/eject animation or sounds, and the
//! reward drops at once instead of one item at a time; a vault with a key already used by
//! the player is refused like vanilla.

use super::anvil::AnvilChunkSource;
use ferrite_nbt::{List, Tag};
use uuid::Uuid;

const DATA: &str = "server_data";
const REWARDED: &str = "rewarded_players";
const STATE: &str = "state";

/// Result of checking whether a player may unlock a vault in its current state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OpenState {
    Ready,
    AlreadyRewarded,
    Inactive,
    Busy,
}

fn open_state(state: &str, rewarded: bool) -> OpenState {
    match state {
        "active" if rewarded => OpenState::AlreadyRewarded,
        "active" => OpenState::Ready,
        "unlocking" | "ejecting" => OpenState::Busy,
        _ => OpenState::Inactive,
    }
}

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

/// Records `player` as rewarded by an active vault at `pos`. `false` when the vault cannot be
/// opened, the player already was rewarded, or the chunk is not loaded, so the key is preserved.
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
            let Some(entity) = entity.as_mut().filter(|entity| entity.type_id == type_id) else {
                return (false, false);
            };
            let Some(Tag::Compound(server)) = entity.data.get_mut(DATA) else {
                return (false, false);
            };
            let Some(Tag::String(state)) = server.get(STATE) else {
                return (false, false);
            };
            let rewarded = match server.get(REWARDED) {
                None => false,
                Some(Tag::List(List::IntArray(all))) => all.contains(&who),
                Some(_) => return (false, false),
            };
            if open_state(state, rewarded) != OpenState::Ready {
                return (false, false);
            }
            let list = server
                .entry(REWARDED.to_owned())
                .or_insert_with(|| Tag::List(List::IntArray(Vec::new())));
            match list {
                Tag::List(List::IntArray(all)) => {
                    all.push(who);
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
    use ferrite_nbt::Compound;
    use ferrite_world::BlockEntity;

    fn put_vault(source: &AnvilChunkSource, pos: [i32; 3], state: &str) {
        let type_id =
            ferrite_data::registries::id("minecraft:block_entity_type", "minecraft:vault")
                .and_then(|id| u32::try_from(id).ok())
                .unwrap();
        source
            .update_block_entity(pos[0], pos[1], pos[2], |entity| {
                let mut server = Compound::new();
                server.insert(STATE.into(), Tag::String(state.into()));
                server.insert(REWARDED.into(), Tag::List(List::IntArray(Vec::new())));
                let mut data = Compound::new();
                data.insert("id".into(), Tag::String("minecraft:vault".into()));
                data.insert(DATA.into(), Tag::Compound(server));
                *entity = Some(BlockEntity { type_id, data });
                (true, ())
            })
            .unwrap();
    }

    #[test]
    fn only_active_vaults_can_be_opened_and_only_once_per_player() {
        assert_eq!(open_state("active", false), OpenState::Ready);
        assert_eq!(open_state("active", true), OpenState::AlreadyRewarded);
        assert_eq!(open_state("inactive", false), OpenState::Inactive);
        assert_eq!(open_state("unlocking", false), OpenState::Busy);
        assert_eq!(open_state("ejecting", false), OpenState::Busy);
        assert_eq!(open_state("unknown", false), OpenState::Inactive);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_player_is_rewarded_once_per_vault() {
        let source = AnvilChunkSource::new_memory();
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        put_vault(&source, [3, 70, 3], "active");
        put_vault(&source, [9, 70, 3], "active");
        assert!(reward_once(&source, [3, 70, 3], a));
        assert!(!reward_once(&source, [3, 70, 3], a));
        assert!(reward_once(&source, [3, 70, 3], b));
        assert!(reward_once(&source, [9, 70, 3], a));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn inactive_vault_does_not_create_reward_state_or_consume_a_claim() {
        let source = AnvilChunkSource::new_memory();
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let pos = [3, 70, 3];
        put_vault(&source, pos, "inactive");
        assert!(!reward_once(&source, pos, Uuid::from_u128(1)));
        put_vault(&source, pos, "active");
        assert!(reward_once(&source, pos, Uuid::from_u128(1)));
    }
}
