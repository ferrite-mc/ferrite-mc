//! Vaults (`VaultBlockEntity`): a trial key opens a vault once per player and the vault ejects
//! the rewards of its loot table.
//!
//! A vault starts `inactive`; a non-spectator player within four blocks wakes it to `active`
//! (world event 3015, vanilla `WorldEvents.VAULT_ACTIVATES`). Using the matching trial key
//! starts `unlocking` for `UNLOCKING_DELAY_TICKS`, then `ejecting` releases the rolled loot
//! over `EJECTING_DELAY_TICKS` (world event 3017 per item), and the vault returns to `active`
//! for the next player. Walking away turns it off again (event 3016).
//!
//! The server data keeps `state`, `state_timer`, `items_to_eject`, `rewarded_players` and
//! `failed_unlock_cooldown`. The reward is claimed when unlocking starts, so one player gets
//! one reward per vault even if they leave before the items leave the vault; a rejected
//! attempt never consumes the key. Timings mirror vanilla `VaultBlockEntity.Server`:
//! `UNLOCKING_DELAY_TICKS` (14) and `INSERT_FAIL_SOUND_BUFFER_TICKS` (15). The 20-tick
//! ejection is the shutter animation: one item leaves every two ticks, and leftovers leave
//! when the shutter closes.

use super::anvil::AnvilChunkSource;
use ferrite_item::ItemStack;
use ferrite_nbt::{Compound, List, Tag};
use ferrite_region::ChunkPos;
use ferrite_world::BlockEntity;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

const DATA: &str = "server_data";
const REWARDED: &str = "rewarded_players";
const STATE: &str = "state";
const TIMER: &str = "state_timer";
const ITEMS: &str = "items_to_eject";
const FAILED: &str = "failed_unlock_cooldown";

/// Vanilla `VaultBlockEntity.Server.UNLOCKING_DELAY_TICKS`.
pub const UNLOCKING_DELAY_TICKS: i64 = 14;
/// Length of the ejection animation before the shutter closes.
pub const EJECTING_DELAY_TICKS: i64 = 20;
/// Ticks between two ejected items.
pub const EJECT_INTERVAL_TICKS: i64 = 2;
/// Vanilla `VaultBlockEntity.Server.INSERT_FAIL_SOUND_BUFFER_TICKS`.
pub const FAILED_UNLOCK_COOLDOWN_TICKS: i64 = 15;
/// Vanilla `WorldEvents.VAULT_ACTIVATES`.
pub const VAULT_ACTIVATES: i32 = 3015;
/// Vanilla `WorldEvents.VAULT_DEACTIVATES`.
pub const VAULT_DEACTIVATES: i32 = 3016;
/// Vanilla `WorldEvents.VAULT_EJECTS_ITEM`.
pub const VAULT_EJECTS_ITEM: i32 = 3017;
/// Vanilla `VaultBlockEntity` detection box: four blocks around the vault.
pub const DETECTION_RANGE: f64 = 4.0;

/// Result of checking whether a player may unlock a vault in its current state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OpenState {
    Ready,
    AlreadyRewarded,
    Inactive,
    Busy,
}

/// What a key use did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Unlock {
    /// The key was accepted: the reward is claimed and the vault is unlocking.
    Started,
    /// A different item was used; the key is untouched.
    WrongKey,
    /// The failed-insert sound cooldown is still running.
    Cooldown,
    /// The vault is `inactive`, so it cannot be opened yet.
    Inactive,
    /// The vault is already unlocking or ejecting another reward.
    Busy,
    /// This player already received the vault's reward.
    AlreadyRewarded,
    /// There is no vault block entity at the position.
    Missing,
}

fn open_state(state: Option<&str>, rewarded: bool) -> OpenState {
    match state {
        Some("active") if rewarded => OpenState::AlreadyRewarded,
        Some("active") => OpenState::Ready,
        Some("unlocking" | "ejecting") => OpenState::Busy,
        _ => OpenState::Inactive,
    }
}

fn refusal(state: OpenState) -> Unlock {
    match state {
        OpenState::AlreadyRewarded => Unlock::AlreadyRewarded,
        OpenState::Inactive => Unlock::Inactive,
        OpenState::Busy => Unlock::Busy,
        OpenState::Ready => unreachable!("a ready vault is not a refusal"),
    }
}

/// Whether an attempt consumes the held key. Only an unlock that actually started does;
/// rejected, busy, inactive and already-rewarded attempts keep the stack.
pub fn unlock_consumes_key(outcome: Unlock) -> bool {
    outcome == Unlock::Started
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

fn is_rewarded(server: &Compound, player: Uuid) -> bool {
    match server.get(REWARDED) {
        None => false,
        Some(Tag::List(List::IntArray(all))) => all.contains(&ints(player)),
        Some(_) => false,
    }
}

fn mark_rewarded(server: &mut Compound, player: Uuid) -> bool {
    let who = ints(player);
    match server
        .entry(REWARDED.to_owned())
        .or_insert_with(|| Tag::List(List::IntArray(Vec::new())))
    {
        Tag::List(List::IntArray(all)) => {
            if !all.contains(&who) {
                all.push(who);
            }
            true
        }
        _ => false,
    }
}

fn items_to_eject(server: &Compound) -> Vec<ItemStack> {
    let Some(Tag::List(List::Compound(all))) = server.get(ITEMS) else {
        return Vec::new();
    };
    all.iter()
        .filter_map(|item| ItemStack::from_nbt(item).ok())
        .collect()
}

fn item_tags(items: &[ItemStack]) -> Tag {
    Tag::List(List::Compound(
        items.iter().filter_map(|item| item.to_nbt().ok()).collect(),
    ))
}

fn tick_value(server: &Compound, key: &str) -> Option<i64> {
    match server.get(key) {
        Some(Tag::Int(value)) => Some(i64::from(*value)),
        Some(Tag::Long(value)) => Some(*value),
        _ => None,
    }
}

fn server_data(entity: &mut BlockEntity) -> &mut Compound {
    if !matches!(entity.data.get(DATA), Some(Tag::Compound(_))) {
        entity
            .data
            .insert(DATA.into(), Tag::Compound(Compound::new()));
    }
    match entity.data.get_mut(DATA) {
        Some(Tag::Compound(server)) => server,
        _ => unreachable!("just inserted server_data"),
    }
}

fn vault_type_id() -> Option<u32> {
    ferrite_data::registries::id("minecraft:block_entity_type", "minecraft:vault")
        .and_then(|id| u32::try_from(id).ok())
}

fn is_vault_block(state: u32) -> bool {
    u16::try_from(state)
        .ok()
        .and_then(ferrite_data::blocks::state)
        .is_some_and(|(name, _)| name == "minecraft:vault")
}

fn roll_rewards(table: &str) -> Vec<ItemStack> {
    let context = ferrite_loot::LootContext::default();
    super::drops::roll_table(table, &context)
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, count)| *count > 0)
        .filter_map(|(item, count)| {
            ItemStack::from_name(&item).map(|stack| stack.copy_with_count(count))
        })
        .collect()
}

/// Claims the reward for `player` and starts unlocking. Returns [`Unlock::Started`] only when
/// the vault was `active`, the player had not been rewarded, and `held` is the matching key;
/// every other outcome leaves the key and the reward list untouched.
pub fn try_unlock(
    source: &AnvilChunkSource,
    [x, y, z]: [i32; 3],
    player: Uuid,
    held: Option<&str>,
    ominous: bool,
) -> Unlock {
    let Some(type_id) = vault_type_id() else {
        return Unlock::Missing;
    };
    let Some(entity) = source
        .block_entity(x, y, z)
        .filter(|entity| entity.type_id == type_id)
    else {
        return Unlock::Missing;
    };
    let Some(server) = entity.data.get(DATA).and_then(|tag| match tag {
        Tag::Compound(server) => Some(server),
        _ => None,
    }) else {
        return Unlock::Inactive;
    };
    let state = match server.get(STATE) {
        Some(Tag::String(state)) => Some(state.as_str()),
        _ => None,
    };
    let open = open_state(state, is_rewarded(server, player));
    if open != OpenState::Ready {
        return refusal(open);
    }

    let (key, table) = key_and_table(ominous);
    if held != Some(key) {
        return source
            .update_block_entity(x, y, z, |entity| {
                let Some(entity) = entity.as_mut().filter(|entity| entity.type_id == type_id)
                else {
                    return (false, Unlock::Missing);
                };
                let server = server_data(entity);
                let state = match server.get(STATE) {
                    Some(Tag::String(state)) => Some(state.as_str()),
                    _ => None,
                };
                let open = open_state(state, is_rewarded(server, player));
                if open != OpenState::Ready {
                    return (false, refusal(open));
                }
                if tick_value(server, FAILED).unwrap_or(0) > 0 {
                    return (false, Unlock::Cooldown);
                }
                server.insert(FAILED.into(), Tag::Int(FAILED_UNLOCK_COOLDOWN_TICKS as i32));
                (true, Unlock::WrongKey)
            })
            .unwrap_or(Unlock::Missing);
    }

    let items = roll_rewards(table);
    source
        .update_block_entity(x, y, z, |entity| {
            let Some(entity) = entity.as_mut().filter(|entity| entity.type_id == type_id) else {
                return (false, Unlock::Missing);
            };
            let server = server_data(entity);
            let state = match server.get(STATE) {
                Some(Tag::String(state)) => Some(state.as_str()),
                _ => None,
            };
            let open = open_state(state, is_rewarded(server, player));
            if open != OpenState::Ready {
                return (false, refusal(open));
            }
            if !mark_rewarded(server, player) {
                return (false, Unlock::Missing);
            }
            server.insert(STATE.into(), Tag::String("unlocking".into()));
            server.insert(TIMER.into(), Tag::Int(UNLOCKING_DELAY_TICKS as i32));
            server.insert(ITEMS.into(), item_tags(&items));
            server.shift_remove(FAILED);
            (true, Unlock::Started)
        })
        .unwrap_or(Unlock::Missing)
}

/// A player the vault can detect; regions build these from their player list.
#[derive(Clone, Copy, Debug)]
pub struct VaultPlayer {
    pub pos: [f64; 3],
    pub spectator: bool,
}

impl VaultPlayer {
    fn in_range(&self, [x, y, z]: [i32; 3]) -> bool {
        !self.spectator
            && (self.pos[0] - (f64::from(x) + 0.5)).abs() <= DETECTION_RANGE
            && (self.pos[1] - (f64::from(y) + 0.5)).abs() <= DETECTION_RANGE
            && (self.pos[2] - (f64::from(z) + 0.5)).abs() <= DETECTION_RANGE
    }
}

/// A client-visible level event (`LevelEvent` packet) at a vault.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VaultEvent {
    pub pos: [i32; 3],
    pub event: i32,
    pub data: i32,
}

/// What one vault tick changed, for the region to apply.
#[derive(Debug, Default)]
pub struct VaultUpdate {
    pub pos: [i32; 3],
    /// The new block state when the visible `vault_state` changed.
    pub block_state: Option<u16>,
    pub events: Vec<VaultEvent>,
    /// Rewards leaving the vault this tick.
    pub items: Vec<ItemStack>,
}

impl VaultUpdate {
    fn changed(&self) -> bool {
        self.block_state.is_some() || !self.events.is_empty() || !self.items.is_empty()
    }
}

fn event(pos: [i32; 3], event: i32, data: i32) -> VaultEvent {
    VaultEvent { pos, event, data }
}

/// Ticks one vault: activation/deactivation near players and the unlocking/ejecting timers.
/// The server data is written back when anything changed; the caller applies `block_state`
/// and forwards `events`/`items` to clients and the entity store.
pub fn tick_vault(
    source: &AnvilChunkSource,
    [x, y, z]: [i32; 3],
    players: &[VaultPlayer],
) -> Option<VaultUpdate> {
    let block_state = u16::try_from(source.block(x, y, z)?).ok()?;
    let (name, _) = ferrite_data::blocks::state(block_state)?;
    if name != "minecraft:vault" {
        return None;
    }
    let type_id = vault_type_id()?;
    let mut entity = source
        .block_entity(x, y, z)
        .filter(|entity| entity.type_id == type_id)?;
    let mut server = match entity.data.get(DATA) {
        Some(Tag::Compound(server)) => server.clone(),
        _ => Compound::new(),
    };
    let mut state = match server.get(STATE) {
        Some(Tag::String(state)) => state.clone(),
        _ => super::blocks::property(block_state, "vault_state")
            .unwrap_or("inactive")
            .to_owned(),
    };
    let ominous = super::blocks::property(block_state, "ominous") == Some("true");
    let mut timer = tick_value(&server, TIMER).unwrap_or(0);
    let mut items = items_to_eject(&server);
    let mut dirty = false;
    let mut items_dirty = false;
    let mut update = VaultUpdate {
        pos: [x, y, z],
        ..Default::default()
    };

    // An invalid insert's sound buffer counts down whether or not a player is nearby.
    if let Some(Tag::Int(cooldown)) = server.get(FAILED) {
        let next = (*cooldown - 1).max(0);
        if next != *cooldown {
            server.insert(FAILED.into(), Tag::Int(next));
            dirty = true;
        }
    }

    let nearby = players.iter().any(|player| player.in_range([x, y, z]));
    match state.as_str() {
        "inactive" if nearby => {
            state = "active".into();
            update
                .events
                .push(event([x, y, z], VAULT_ACTIVATES, i32::from(ominous)));
            dirty = true;
        }
        "active" if !nearby => {
            state = "inactive".into();
            update
                .events
                .push(event([x, y, z], VAULT_DEACTIVATES, i32::from(ominous)));
            dirty = true;
        }
        "unlocking" => {
            timer -= 1;
            if timer <= 0 {
                state = "ejecting".into();
                timer = EJECTING_DELAY_TICKS;
            }
            dirty = true;
        }
        "ejecting" => {
            if timer % EJECT_INTERVAL_TICKS == 0 && !items.is_empty() {
                update.items.push(items.remove(0));
                update.events.push(event([x, y, z], VAULT_EJECTS_ITEM, 0));
                items_dirty = true;
            }
            timer -= 1;
            if timer <= 0 {
                for item in items.drain(..) {
                    update.items.push(item);
                    update.events.push(event([x, y, z], VAULT_EJECTS_ITEM, 0));
                }
                state = "active".into();
                timer = 0;
                items_dirty = true;
            }
            dirty = true;
        }
        "inactive" | "active" => {}
        _ => {
            state = "inactive".into();
            timer = 0;
            dirty = true;
        }
    }

    if let Some(desired) =
        super::blocks::with_props(block_state, &[("vault_state", state.as_str())])
        && desired != block_state
    {
        update.block_state = Some(desired);
    }

    if dirty {
        server.insert(STATE.into(), Tag::String(state));
        server.insert(TIMER.into(), Tag::Int(timer as i32));
        if items_dirty {
            server.insert(ITEMS.into(), item_tags(&items));
        }
        entity.data.insert(DATA.into(), Tag::Compound(server));
        if let Err(error) = source.update_block_entity(x, y, z, |current| {
            if current.is_some() {
                *current = Some(entity);
                (true, ())
            } else {
                (false, ())
            }
        }) {
            // The block-state/event update is still valid; the next tick retries the data.
            tracing::warn!(x, y, z, %error, "could not store vault state");
        }
    }
    Some(update)
}

/// The vaults one region ticks, discovered from loaded block entities and block changes.
#[derive(Debug, Default)]
pub struct Vaults {
    known: HashSet<[i32; 3]>,
    scan_clock: u32,
}

impl Vaults {
    /// How often loaded block entities are scanned for vaults no player has changed.
    const SCAN_INTERVAL: u32 = 20;

    /// Registers vaults from the loaded chunks this region holds. The scan runs once a second.
    pub fn discover(&mut self, source: &AnvilChunkSource, held: &HashMap<ChunkPos, usize>) {
        if self.scan_clock.is_multiple_of(Self::SCAN_INTERVAL)
            && let Some(type_id) = vault_type_id()
        {
            for (pos, entity) in source.block_entities() {
                let [x, _, z] = pos;
                if entity.type_id == type_id
                    && held.contains_key(&ChunkPos(x.div_euclid(16), z.div_euclid(16)))
                {
                    self.known.insert(pos);
                }
            }
        }
        self.scan_clock = self.scan_clock.wrapping_add(1);
    }

    /// Registers a vault placed or uncovered next to a changed block.
    pub fn blocks_changed(
        &mut self,
        source: &AnvilChunkSource,
        positions: &[[i32; 3]],
        held: &HashMap<ChunkPos, usize>,
    ) {
        for &pos in positions {
            let [x, y, z] = pos;
            if !held.contains_key(&ChunkPos(x.div_euclid(16), z.div_euclid(16))) {
                continue;
            }
            if source.block(x, y, z).is_some_and(is_vault_block) {
                self.known.insert(pos);
            }
        }
    }

    /// Ticks every known vault in a chunk this region holds. Vaults whose block is gone are
    /// forgotten; a vault in an unloaded chunk keeps ticking once its chunk returns.
    pub fn tick(
        &mut self,
        source: &AnvilChunkSource,
        players: &[VaultPlayer],
        held: &HashMap<ChunkPos, usize>,
    ) -> Vec<VaultUpdate> {
        let mut out = Vec::new();
        let positions: Vec<_> = self.known.iter().copied().collect();
        for pos in positions {
            let [x, y, z] = pos;
            if !held.contains_key(&ChunkPos(x.div_euclid(16), z.div_euclid(16))) {
                continue;
            }
            let Some(block) = source.block(x, y, z) else {
                continue;
            };
            if !is_vault_block(block) {
                self.known.remove(&pos);
                continue;
            }
            if let Some(update) = tick_vault(source, pos, players)
                && update.changed()
            {
                out.push(update);
            }
        }
        out
    }

    /// Moves the vaults in the half of a split region that keeps them.
    pub fn split(&mut self, keep: impl Fn(i32, i32) -> bool) -> Self {
        let mut out = Self::default();
        let moving: Vec<_> = self
            .known
            .iter()
            .copied()
            .filter(|pos| keep(pos[0], pos[2]))
            .collect();
        for pos in moving {
            self.known.remove(&pos);
            out.known.insert(pos);
        }
        out
    }

    pub fn merge(&mut self, other: Self) {
        self.known.extend(other.known);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    async fn loaded() -> Arc<AnvilChunkSource> {
        let source = AnvilChunkSource::new_memory();
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        source
    }

    fn vanilla_root() -> Option<PathBuf> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3/generated");
        root.join("reports/blocks.json").exists().then_some(root)
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ferrite-vault-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn vault_state(state: &str, ominous: bool) -> u16 {
        ferrite_data::blocks::state_id(
            "vault",
            &[
                ("facing", "north"),
                ("ominous", if ominous { "true" } else { "false" }),
                ("vault_state", state),
            ],
        )
        .expect("vanilla vault state")
    }

    fn put_vault(source: &AnvilChunkSource, [x, y, z]: [i32; 3], state: &str, ominous: bool) {
        source
            .set_block(x, y, z, u32::from(vault_state(state, ominous)))
            .unwrap();
        let type_id = vault_type_id().unwrap();
        source
            .update_block_entity(x, y, z, |entity| {
                let mut server = Compound::new();
                server.insert(STATE.into(), Tag::String(state.into()));
                server.insert(REWARDED.into(), Tag::List(List::IntArray(Vec::new())));
                let mut data = Compound::new();
                data.insert("id".into(), Tag::String("minecraft:vault".into()));
                data.insert("x".into(), Tag::Int(x));
                data.insert("y".into(), Tag::Int(y));
                data.insert("z".into(), Tag::Int(z));
                data.insert(DATA.into(), Tag::Compound(server));
                *entity = Some(BlockEntity { type_id, data });
                (true, ())
            })
            .unwrap();
    }

    fn state_of(source: &AnvilChunkSource, [x, y, z]: [i32; 3]) -> String {
        let entity = source.block_entity(x, y, z).expect("vault block entity");
        let Some(Tag::Compound(server)) = entity.data.get(DATA) else {
            panic!("vault has server_data");
        };
        match server.get(STATE) {
            Some(Tag::String(state)) => state.clone(),
            other => panic!("vault state is a string: {other:?}"),
        }
    }

    fn rewarded(source: &AnvilChunkSource, [x, y, z]: [i32; 3], player: Uuid) -> bool {
        let entity = source.block_entity(x, y, z).expect("vault block entity");
        let Some(Tag::Compound(server)) = entity.data.get(DATA) else {
            return false;
        };
        is_rewarded(server, player)
    }

    fn stored_items(source: &AnvilChunkSource, [x, y, z]: [i32; 3]) -> Vec<ItemStack> {
        let entity = source.block_entity(x, y, z).expect("vault block entity");
        let Some(Tag::Compound(server)) = entity.data.get(DATA) else {
            return Vec::new();
        };
        items_to_eject(server)
    }

    fn player(pos: [f64; 3], spectator: bool) -> VaultPlayer {
        VaultPlayer { pos, spectator }
    }

    fn near() -> VaultPlayer {
        player([4.5, 64.5, 4.5], false)
    }

    fn apply(source: &AnvilChunkSource, update: &VaultUpdate) {
        if let Some(state) = update.block_state {
            let [x, y, z] = update.pos;
            source.set_block(x, y, z, u32::from(state)).unwrap();
        }
    }

    #[test]
    fn only_a_started_unlock_consumes_the_key() {
        assert!(unlock_consumes_key(Unlock::Started));
        for outcome in [
            Unlock::WrongKey,
            Unlock::Cooldown,
            Unlock::Inactive,
            Unlock::Busy,
            Unlock::AlreadyRewarded,
            Unlock::Missing,
        ] {
            assert!(!unlock_consumes_key(outcome), "{outcome:?}");
        }
    }

    #[test]
    fn only_active_vaults_can_be_opened_and_only_once_per_player() {
        assert_eq!(open_state(Some("active"), false), OpenState::Ready);
        assert_eq!(open_state(Some("active"), true), OpenState::AlreadyRewarded);
        assert_eq!(open_state(Some("inactive"), false), OpenState::Inactive);
        assert_eq!(open_state(Some("unlocking"), false), OpenState::Busy);
        assert_eq!(open_state(Some("ejecting"), false), OpenState::Busy);
        assert_eq!(open_state(Some("unknown"), false), OpenState::Inactive);
        assert_eq!(open_state(None, false), OpenState::Inactive);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_nearby_player_activates_an_inactive_vault_and_leaving_deactivates_it() {
        let source = loaded().await;
        let pos = [4, 64, 4];
        put_vault(&source, pos, "inactive", false);

        // At exactly four blocks the vault detects the player.
        let update = tick_vault(&source, pos, &[near()]).expect("vault tick");
        assert_eq!(update.block_state, Some(vault_state("active", false)));
        assert_eq!(
            update.events,
            vec![VaultEvent {
                pos,
                event: VAULT_ACTIVATES,
                data: 0,
            }]
        );
        apply(&source, &update);

        // Four and a half blocks away is out of range.
        let far = player([9.0, 64.5, 4.5], false);
        let idle = tick_vault(&source, pos, &[far]).expect("vault tick");
        assert_eq!(idle.block_state, Some(vault_state("inactive", false)));
        assert_eq!(idle.events[0].event, VAULT_DEACTIVATES);
        apply(&source, &idle);

        // Spectators never wake a vault.
        let update = tick_vault(&source, pos, &[player([4.5, 64.5, 4.5], true)]).expect("tick");
        assert_eq!(update.block_state, None);
        assert!(update.events.is_empty());

        // The same range applies vertically.
        let update = tick_vault(&source, pos, &[player([4.5, 69.0, 4.5], false)]).expect("tick");
        assert_eq!(update.block_state, None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unlocking_ejects_once_and_returns_to_active_for_the_next_player() {
        let source = loaded().await;
        let pos = [4, 64, 4];
        put_vault(&source, pos, "active", false);
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));

        assert_eq!(
            try_unlock(&source, pos, a, Some("minecraft:trial_key"), false),
            Unlock::Started
        );
        assert_eq!(state_of(&source, pos), "unlocking");
        assert!(rewarded(&source, pos, a));
        assert!(!rewarded(&source, pos, b));
        assert!(!stored_items(&source, pos).is_empty());
        // A second attempt while the shutter is busy is refused and claims nothing.
        assert_eq!(
            try_unlock(&source, pos, b, Some("minecraft:trial_key"), false),
            Unlock::Busy
        );
        assert!(!rewarded(&source, pos, b));

        for _ in 0..UNLOCKING_DELAY_TICKS {
            let update = tick_vault(&source, pos, &[near()]).expect("vault tick");
            apply(&source, &update);
        }
        assert_eq!(state_of(&source, pos), "ejecting");

        let mut ejected = Vec::new();
        for _ in 0..EJECTING_DELAY_TICKS {
            let update = tick_vault(&source, pos, &[near()]).expect("vault tick");
            apply(&source, &update);
            ejected.extend(update.items);
        }
        assert_eq!(state_of(&source, pos), "active");
        assert!(stored_items(&source, pos).is_empty());
        assert!(!ejected.is_empty(), "the vault ejected its loot");
        let names: Vec<_> = ejected.iter().filter_map(|item| item.name()).collect();
        assert!(names.iter().all(|name| name.starts_with("minecraft:")));

        // The first player has had their reward; the next player can still open it.
        assert_eq!(
            try_unlock(&source, pos, a, Some("minecraft:trial_key"), false),
            Unlock::AlreadyRewarded
        );
        assert_eq!(
            try_unlock(&source, pos, b, Some("minecraft:trial_key"), false),
            Unlock::Started
        );
        assert!(rewarded(&source, pos, b));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn inactive_busy_and_wrong_key_attempts_claim_nothing_and_consume_nothing() {
        let source = loaded().await;
        let pos = [4, 64, 4];
        let player = Uuid::from_u128(3);
        put_vault(&source, pos, "inactive", false);

        assert_eq!(
            try_unlock(&source, pos, player, Some("minecraft:trial_key"), false),
            Unlock::Inactive
        );
        assert!(!rewarded(&source, pos, player));
        assert!(stored_items(&source, pos).is_empty());

        // Wake it, then try the wrong item: no reward, and the failed-insert cooldown starts.
        let update = tick_vault(&source, pos, &[near()]).expect("vault tick");
        apply(&source, &update);
        assert_eq!(
            try_unlock(&source, pos, player, Some("minecraft:stick"), false),
            Unlock::WrongKey
        );
        assert_eq!(
            try_unlock(&source, pos, player, Some("minecraft:stick"), false),
            Unlock::Cooldown
        );
        assert!(!rewarded(&source, pos, player));
        assert!(stored_items(&source, pos).is_empty());

        // The cooldown expires after the documented buffer while the vault stays active.
        for _ in 0..FAILED_UNLOCK_COOLDOWN_TICKS {
            let update = tick_vault(&source, pos, &[near()]).expect("vault tick");
            apply(&source, &update);
        }
        assert_eq!(
            try_unlock(&source, pos, player, Some("minecraft:stick"), false),
            Unlock::WrongKey
        );

        // The matching key is still accepted, and only then is the reward claimed.
        assert_eq!(
            try_unlock(&source, pos, player, Some("minecraft:trial_key"), false),
            Unlock::Started
        );
        assert!(rewarded(&source, pos, player));
        assert!(!stored_items(&source, pos).is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn vault_state_and_claims_survive_a_world_reload() {
        let Some(root) = vanilla_root() else {
            eprintln!("SKIP vault persistence test: vanilla/26.3/generated is absent");
            return;
        };
        let world = temp_dir("reload");
        let registry = ferrite_world::RuntimeRegistry::load(&root).unwrap();
        let source = AnvilChunkSource::new(world.clone(), registry);
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let pos = [4, 64, 4];
        let player = Uuid::from_u128(7);
        put_vault(&source, pos, "active", true);
        assert_eq!(
            try_unlock(
                &source,
                pos,
                player,
                Some("minecraft:ominous_trial_key"),
                true
            ),
            Unlock::Started
        );
        let items_before = stored_items(&source, pos);
        assert!(!items_before.is_empty());
        source.flush_chunk(0, 0).await.unwrap();
        drop(source);

        // "Restart": a new source over the same directory.
        let registry = ferrite_world::RuntimeRegistry::load(&root).unwrap();
        let source = AnvilChunkSource::new(world.clone(), registry);
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(state_of(&source, pos), "unlocking");
        assert!(rewarded(&source, pos, player));
        assert_eq!(
            stored_items(&source, pos)
                .iter()
                .map(|item| item.name())
                .collect::<Vec<_>>(),
            items_before
                .iter()
                .map(|item| item.name())
                .collect::<Vec<_>>()
        );

        // The restored timer still finishes, and the claim is still once-only.
        for _ in 0..(UNLOCKING_DELAY_TICKS + EJECTING_DELAY_TICKS) {
            let update = tick_vault(&source, pos, &[near()]).expect("vault tick");
            apply(&source, &update);
        }
        assert_eq!(state_of(&source, pos), "active");
        assert_eq!(
            try_unlock(
                &source,
                pos,
                player,
                Some("minecraft:ominous_trial_key"),
                true
            ),
            Unlock::AlreadyRewarded
        );

        drop(source);
        let _ = std::fs::remove_dir_all(world);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn region_discovery_finds_loaded_vaults_and_respects_held_chunks() {
        let source = loaded().await;
        let pos = [4, 64, 4];
        put_vault(&source, pos, "inactive", false);
        let mut held = HashMap::new();
        held.insert(ChunkPos(0, 0), 1);

        let mut vaults = Vaults::default();
        vaults.discover(&source, &held);
        let updates = vaults.tick(&source, &[near()], &held);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].pos, pos);
        assert_eq!(updates[0].block_state, Some(vault_state("active", false)));

        // A region that does not hold the chunk leaves the vault alone.
        let elsewhere = HashMap::from([(ChunkPos(9, 9), 1)]);
        let updates = vaults.tick(&source, &[near()], &elsewhere);
        assert!(updates.is_empty());
    }
}
