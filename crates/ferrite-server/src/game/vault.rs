//! Vaults (`VaultBlockEntity`): a trial key opens a vault once per player and the vault ejects
//! the rewards of its loot table.
//!
//! A vault starts `inactive`; a non-spectator player within four blocks wakes it to `active`
//! (world event 3015, vanilla `WorldEvents.VAULT_ACTIVATES`). Using the matching trial key
//! starts `unlocking` for `UNLOCKING_DELAY_TICKS`, then `ejecting` releases the rolled loot
//! over `EJECTING_DELAY_TICKS` (world event 3017 per item), and the vault returns to `active`
//! for the next player. Walking away turns it off again (event 3016). Vanilla 26.3
//! `VaultState.ejectResultItem` sends 3017 with data 0; the item id travels on the spawned
//! item entity, not in the level event.
//!
//! The server data keeps `state`, `state_timer`, `items_to_eject`, `rewarded_players` and
//! `failed_unlock_cooldown`. The reward is claimed when unlocking starts, so one player gets
//! one reward per vault even if they leave before the items leave the vault; a rejected
//! attempt never consumes the key. Timings mirror vanilla `VaultBlockEntity.Server`:
//! `UNLOCKING_DELAY_TICKS` (14) and `INSERT_FAIL_SOUND_BUFFER_TICKS` (15). The 20-tick
//! ejection is the shutter animation: one item leaves every two ticks, and leftovers leave
//! when the shutter closes.

use super::anvil::AnvilChunkSource;
use ferrite_entity::{EntityRecord, Rng};
use ferrite_item::ItemStack;
use ferrite_math::Vec3;
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
/// Per-reward delivery id stored beside the item stack, used to reconcile a spawned item entity
/// with a ledger write that a crash may have interrupted.
const DELIVERY: &str = "ferrite_delivery";

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

fn uuid_from_ints(words: &[i32]) -> Option<Uuid> {
    if words.len() != 4 {
        return None;
    }
    let mut n: u128 = 0;
    for word in words {
        n = (n << 32) | u128::from(*word as u32);
    }
    Some(Uuid::from_u128(n))
}

/// Token for a ledger written before delivery tokens existed. It only has to be stable for the
/// same vault, index and stack, so an entity saved from that ledger can still be reconciled.
fn legacy_delivery_token([x, y, z]: [i32; 3], index: usize, stack: &ItemStack) -> Uuid {
    let mut bytes = Vec::with_capacity(24);
    for value in [x, y, z] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&(index as u64).to_le_bytes());
    bytes.extend_from_slice(&stack.item.to_le_bytes());
    bytes.extend_from_slice(&stack.count.to_le_bytes());
    let mut first = 0xcbf2_9ce4_8422_2325u64;
    let mut second = 0x9e37_79b9_7f4a_7c15u64;
    for &byte in &bytes {
        first ^= u64::from(byte);
        first = first.wrapping_mul(0x100_0000_01b3);
    }
    for &byte in bytes.iter().rev() {
        second ^= u64::from(byte);
        second = second.wrapping_mul(0x100_0000_01b3);
    }
    Uuid::from_u128((u128::from(first) << 64) | u128::from(second))
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

fn items_to_eject(server: &Compound, pos: [i32; 3]) -> Vec<PendingReward> {
    let Some(Tag::List(List::Compound(all))) = server.get(ITEMS) else {
        return Vec::new();
    };
    all.iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let stack = ItemStack::from_nbt(item).ok()?;
            let token = match item.get(DELIVERY) {
                Some(Tag::IntArray(words)) => uuid_from_ints(words),
                _ => None,
            }
            .unwrap_or_else(|| legacy_delivery_token(pos, index, &stack));
            Some(PendingReward { stack, token })
        })
        .collect()
}

fn item_tags(items: &[PendingReward]) -> Tag {
    Tag::List(List::Compound(
        items
            .iter()
            .filter_map(|reward| {
                let mut item = reward.stack.to_nbt().ok()?;
                item.insert(DELIVERY.into(), Tag::IntArray(ints(reward.token)));
                Some(item)
            })
            .collect(),
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

    let items: Vec<PendingReward> = roll_rewards(table)
        .into_iter()
        .map(|stack| PendingReward {
            stack,
            token: ferrite_entity::random_uuid(),
        })
        .collect();
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

/// One rolled reward and its durable delivery token. The token is written into the block
/// entity's `items_to_eject` ledger and used as the spawned item entity's UUID, so a restart can
/// tell whether a ledger entry already became an entity.
#[derive(Clone, Debug)]
pub struct PendingReward {
    pub stack: ItemStack,
    pub token: Uuid,
}

/// What one vault tick changed, for the region to apply.
#[derive(Debug, Default)]
pub struct VaultUpdate {
    pub pos: [i32; 3],
    /// The new block state when the visible `vault_state` changed.
    pub block_state: Option<u16>,
    pub events: Vec<VaultEvent>,
    /// Rewards leaving the vault this tick.
    pub items: Vec<PendingReward>,
    /// The reward ledger to write after `items` have been spawned. [`tick_vault`] deliberately
    /// leaves this out of its block-entity write, so a crash before the spawn leaves the reward
    /// pending instead of losing it.
    pub pending_items: Option<Vec<PendingReward>>,
}

impl VaultUpdate {
    fn changed(&self) -> bool {
        self.block_state.is_some()
            || !self.events.is_empty()
            || !self.items.is_empty()
            || self.pending_items.is_some()
    }
}

/// The stack a client sees on an ejected reward item entity. Vault rewards are stored with
/// protocol item ids already, so the wire id is the stored id and no name lookup is needed.
pub fn ejected_item_stack(stack: &ItemStack) -> ferrite_protocol::packets::play::ItemStack {
    ferrite_protocol::packets::play::ItemStack {
        count: stack.count,
        item_id: i32::from(stack.item),
    }
}

/// The item entity for one reward leaving the vault (vanilla
/// `VaultBlockEntity.Server.tryEjectItem`): above the block with a small random push and yaw.
/// The entity UUID is the reward's delivery token, so a restart can recognize it.
pub fn ejected_item([x, y, z]: [i32; 3], reward: &PendingReward, rng: &mut Rng) -> EntityRecord {
    let mut record = ferrite_entity::item::new_item(
        Vec3::new(f64::from(x) + 0.5, f64::from(y) + 1.0, f64::from(z) + 0.5),
        Vec3::new(rng.range(-0.05, 0.05), 0.2, rng.range(-0.05, 0.05)),
        ejected_item_stack(&reward.stack),
        rng,
    );
    record.data.uuid = reward.token;
    record
}

fn event(pos: [i32; 3], event: i32, data: i32) -> VaultEvent {
    VaultEvent { pos, event, data }
}

/// Ticks one vault without entity-store reconciliation. Callers that do not own the ejected
/// entities use this; a region uses [`tick_vault_with_delivered`].
#[cfg(test)]
pub fn tick_vault(
    source: &AnvilChunkSource,
    [x, y, z]: [i32; 3],
    players: &[VaultPlayer],
) -> Option<VaultUpdate> {
    tick_vault_with_delivered(source, [x, y, z], players, |_| false)
}

/// Ticks one vault: activation/deactivation near players and the unlocking/ejecting timers.
/// The server data is written back when anything changed; the caller applies `block_state`
/// and forwards `events`/`items` to clients and the entity store. Ledger entries whose delivery
/// token is already in `delivered` are dropped without spawning another entity.
pub fn tick_vault_with_delivered(
    source: &AnvilChunkSource,
    [x, y, z]: [i32; 3],
    players: &[VaultPlayer],
    delivered: impl Fn(Uuid) -> bool,
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
    let mut items = items_to_eject(&server, [x, y, z]);
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
                let reward = items.remove(0);
                if delivered(reward.token) {
                    // A crash saved the item entity but not the ledger. Drop the ledger entry
                    // instead of spawning a second copy.
                    items_dirty = true;
                } else {
                    update.items.push(reward);
                    update.events.push(event([x, y, z], VAULT_EJECTS_ITEM, 0));
                    items_dirty = true;
                }
            }
            timer -= 1;
            if timer <= 0 {
                for reward in items.drain(..) {
                    if delivered(reward.token) {
                        continue;
                    }
                    update.items.push(reward);
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

    if items_dirty {
        // The caller writes this after the ejected item entities exist; see
        // [`commit_ejection`]. Writing it here would lose the reward in a crash between the
        // block-entity save and the entity spawn.
        update.pending_items = Some(items.clone());
    }
    if dirty {
        server.insert(STATE.into(), Tag::String(state));
        server.insert(TIMER.into(), Tag::Int(timer as i32));
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

/// Writes the pending reward ledger after the caller has spawned [`VaultUpdate::items`].
/// Keeping this separate from [`tick_vault`] means a crash before the spawn leaves the reward
/// pending; a crash after the spawn but before this write re-ejects an item whose entity was
/// never persisted, so the reward is still delivered once. Returns `false` when the write failed
/// and the caller must remove the entities it just spawned so the reward stays pending.
pub fn commit_ejection(
    source: &AnvilChunkSource,
    [x, y, z]: [i32; 3],
    items: &[PendingReward],
) -> bool {
    let Some(type_id) = vault_type_id() else {
        return true;
    };
    let result = source.update_block_entity(x, y, z, |entity| {
        let Some(entity) = entity.as_mut().filter(|entity| entity.type_id == type_id) else {
            return (false, ());
        };
        server_data(entity).insert(ITEMS.into(), item_tags(items));
        (true, ())
    });
    match result {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(x, y, z, %error, "could not store the vault reward ledger");
            false
        }
    }
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

    /// Ticks every known vault in a chunk this region holds, without entity reconciliation.
    #[cfg(test)]
    pub fn tick(
        &mut self,
        source: &AnvilChunkSource,
        players: &[VaultPlayer],
        held: &HashMap<ChunkPos, usize>,
    ) -> Vec<VaultUpdate> {
        self.tick_with_delivered(source, players, held, |_| false)
    }

    /// Ticks every known vault in a chunk this region holds. Vaults whose block is gone are
    /// forgotten; a vault in an unloaded chunk keeps ticking once its chunk returns. Ledger
    /// entries whose token is already in the region's entity store are dropped without spawning.
    pub fn tick_with_delivered(
        &mut self,
        source: &AnvilChunkSource,
        players: &[VaultPlayer],
        held: &HashMap<ChunkPos, usize>,
        delivered: impl Fn(Uuid) -> bool,
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
            if let Some(update) = tick_vault_with_delivered(source, pos, players, &delivered)
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
    use std::sync::{Arc, Barrier};

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

    fn stored_rewards(source: &AnvilChunkSource, [x, y, z]: [i32; 3]) -> Vec<PendingReward> {
        let entity = source.block_entity(x, y, z).expect("vault block entity");
        let Some(Tag::Compound(server)) = entity.data.get(DATA) else {
            return Vec::new();
        };
        items_to_eject(server, [x, y, z])
    }

    fn stored_items(source: &AnvilChunkSource, pos: [i32; 3]) -> Vec<ItemStack> {
        stored_rewards(source, pos)
            .into_iter()
            .map(|reward| reward.stack)
            .collect()
    }

    fn stacks(rewards: &[PendingReward]) -> Vec<ItemStack> {
        rewards.iter().map(|reward| reward.stack.clone()).collect()
    }

    /// The reward ledger as a multiset of `(item id, count)`, for restart accounting.
    fn id_counts(items: &[ItemStack]) -> Vec<(u16, i32)> {
        let mut counts: Vec<_> = items.iter().map(|item| (item.item, item.count)).collect();
        counts.sort_unstable();
        counts
    }

    fn reward_counts(rewards: &[PendingReward]) -> Vec<(u16, i32)> {
        id_counts(&stacks(rewards))
    }

    fn entity_stack(record: &EntityRecord) -> ItemStack {
        let wire = ferrite_entity::item::stack(&record.data);
        ItemStack::with_count(
            u16::try_from(wire.item_id).expect("item entity has a registry item id"),
            wire.count,
        )
    }

    fn entity_counts(records: &[EntityRecord]) -> Vec<(u16, i32)> {
        let items: Vec<_> = records.iter().map(entity_stack).collect();
        id_counts(&items)
    }

    fn timer_of(source: &AnvilChunkSource, [x, y, z]: [i32; 3]) -> i64 {
        let entity = source.block_entity(x, y, z).expect("vault block entity");
        let Some(Tag::Compound(server)) = entity.data.get(DATA) else {
            return 0;
        };
        tick_value(server, TIMER).unwrap_or(0)
    }

    fn rewarded_count(source: &AnvilChunkSource, [x, y, z]: [i32; 3]) -> usize {
        let entity = source.block_entity(x, y, z).expect("vault block entity");
        let Some(Tag::Compound(server)) = entity.data.get(DATA) else {
            return 0;
        };
        match server.get(REWARDED) {
            Some(Tag::List(List::IntArray(all))) => all.len(),
            _ => 0,
        }
    }

    fn player(pos: [f64; 3], spectator: bool) -> VaultPlayer {
        VaultPlayer { pos, spectator }
    }

    fn near() -> VaultPlayer {
        player([4.5, 64.5, 4.5], false)
    }

    fn apply_block_state(source: &AnvilChunkSource, update: &VaultUpdate) {
        if let Some(state) = update.block_state {
            let [x, y, z] = update.pos;
            source.set_block(x, y, z, u32::from(state)).unwrap();
        }
    }

    /// What a region does after ticking: apply the block state, spawn `items`, then write the
    /// reward ledger that no longer contains them.
    fn apply(source: &AnvilChunkSource, update: &VaultUpdate) {
        apply_block_state(source, update);
        if let Some(items) = &update.pending_items {
            assert!(
                commit_ejection(source, update.pos, items),
                "vault ledger write failed"
            );
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
        let ejected = stacks(&ejected);
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ejected_rewards_use_registry_item_ids_and_emit_one_event_each() {
        let source = loaded().await;
        let pos = [4, 64, 4];
        put_vault(&source, pos, "inactive", false);
        let (a, b) = (Uuid::from_u128(21), Uuid::from_u128(22));

        // Activation by proximity: one block-state change and one 3015 event.
        let activation = tick_vault(&source, pos, &[near()]).expect("vault tick");
        assert_eq!(activation.block_state, Some(vault_state("active", false)));
        assert_eq!(
            activation.events,
            vec![VaultEvent {
                pos,
                event: VAULT_ACTIVATES,
                data: 0,
            }]
        );
        apply(&source, &activation);

        // A wrong key is rejected without consuming it; the matching key starts the unlock.
        assert_eq!(
            try_unlock(&source, pos, a, Some("minecraft:stick"), false),
            Unlock::WrongKey
        );
        assert!(!unlock_consumes_key(Unlock::WrongKey));
        assert_eq!(
            try_unlock(&source, pos, a, Some("minecraft:trial_key"), false),
            Unlock::Started
        );
        assert!(unlock_consumes_key(Unlock::Started));
        // A duplicate interaction while the shutter is busy claims and consumes nothing.
        assert_eq!(
            try_unlock(&source, pos, a, Some("minecraft:trial_key"), false),
            Unlock::Busy
        );
        assert!(!unlock_consumes_key(Unlock::Busy));

        let reward = stored_items(&source, pos);
        assert!(!reward.is_empty(), "the vault rolled a reward");
        for _ in 0..UNLOCKING_DELAY_TICKS {
            let update = tick_vault(&source, pos, &[near()]).expect("vault tick");
            apply(&source, &update);
        }
        assert_eq!(state_of(&source, pos), "ejecting");

        let mut ejected = Vec::new();
        let mut ejection_events = 0;
        for _ in 0..EJECTING_DELAY_TICKS {
            let update = tick_vault(&source, pos, &[near()]).expect("vault tick");
            for event in &update.events {
                if event.event == VAULT_EJECTS_ITEM {
                    assert_eq!(event.pos, pos);
                    assert_eq!(event.data, 0);
                    ejection_events += 1;
                }
            }
            apply(&source, &update);
            ejected.extend(update.items);
        }
        assert_eq!(state_of(&source, pos), "active");
        assert!(stored_items(&source, pos).is_empty());
        assert_eq!(ejection_events, ejected.len());
        assert_eq!(reward_counts(&ejected), id_counts(&reward));

        let mut rng = Rng::new(7);
        for reward in &ejected {
            let stack = &reward.stack;
            let name = stack
                .name()
                .unwrap_or_else(|| panic!("unknown item id {}", stack.item));
            assert_ne!(stack.item, ferrite_item::AIR);
            assert_eq!(ferrite_item::item_id(name), Some(stack.item));
            assert_eq!(
                ferrite_data::registries::id("minecraft:item", name),
                Some(i32::from(stack.item))
            );
            let protocol = ejected_item_stack(stack);
            assert_eq!(protocol.item_id, i32::from(stack.item));
            assert_eq!(protocol.count, stack.count);

            let record = ejected_item(pos, reward, &mut rng);
            let spawned = ferrite_entity::item::stack(&record.data);
            assert_eq!(spawned.item_id, i32::from(stack.item));
            assert_eq!(spawned.count, stack.count);
            assert_eq!(record.data.uuid, reward.token);
            assert_eq!(record.pos.x, f64::from(pos[0]) + 0.5);
            assert_eq!(record.pos.y, f64::from(pos[1]) + 1.0);
            assert_eq!(record.pos.z, f64::from(pos[2]) + 0.5);
        }

        // A second player is independently eligible for the same, already-ejected vault.
        assert_eq!(
            try_unlock(&source, pos, a, Some("minecraft:trial_key"), false),
            Unlock::AlreadyRewarded
        );
        assert!(!unlock_consumes_key(Unlock::AlreadyRewarded));
        assert_eq!(
            try_unlock(&source, pos, b, Some("minecraft:trial_key"), false),
            Unlock::Started
        );
        assert!(unlock_consumes_key(Unlock::Started));
        assert!(rewarded(&source, pos, b));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn restart_mid_ejection_keeps_each_reward_exactly_once() {
        let Some(root) = vanilla_root() else {
            eprintln!("SKIP vault ejection reload test: vanilla/26.3/generated is absent");
            return;
        };
        let world = temp_dir("ejection-reload");
        let registry = ferrite_world::RuntimeRegistry::load(&root).unwrap();
        let source = AnvilChunkSource::new(world.clone(), registry);
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let pos = [4, 64, 4];
        let (a, b) = (Uuid::from_u128(31), Uuid::from_u128(32));
        put_vault(&source, pos, "active", true);
        assert_eq!(
            try_unlock(&source, pos, a, Some("minecraft:ominous_trial_key"), true),
            Unlock::Started
        );
        let reward = stored_items(&source, pos);
        assert!(!reward.is_empty());

        for _ in 0..UNLOCKING_DELAY_TICKS {
            let update = tick_vault(&source, pos, &[near()]).expect("vault tick");
            apply(&source, &update);
        }
        assert_eq!(state_of(&source, pos), "ejecting");

        // Let a few rewards leave the vault before the save/restart.
        let mut ejected_before = Vec::new();
        for _ in 0..6 {
            let update = tick_vault(&source, pos, &[near()]).expect("vault tick");
            apply(&source, &update);
            ejected_before.extend(update.items);
        }
        assert!(
            !ejected_before.is_empty(),
            "some rewards were ejected before the restart"
        );
        assert_eq!(
            try_unlock(&source, pos, b, Some("minecraft:ominous_trial_key"), true),
            Unlock::Busy
        );
        assert!(!rewarded(&source, pos, b));

        source.flush_chunk(0, 0).await.unwrap();
        drop(source);

        // Restart over the same directory: the shutter resumes and the ledger is intact.
        let registry = ferrite_world::RuntimeRegistry::load(&root).unwrap();
        let source = AnvilChunkSource::new(world.clone(), registry);
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(state_of(&source, pos), "ejecting");
        assert!(rewarded(&source, pos, a));
        assert!(!rewarded(&source, pos, b));
        let remaining = stored_items(&source, pos);
        let mut ledger = stacks(&ejected_before);
        ledger.extend(remaining.clone());
        assert_eq!(
            id_counts(&ledger),
            id_counts(&reward),
            "reload duplicates or loses pending rewards"
        );

        let mut ejected_after = Vec::new();
        let mut ejection_events = 0;
        for _ in 0..EJECTING_DELAY_TICKS {
            let update = tick_vault(&source, pos, &[near()]).expect("vault tick");
            ejection_events += update
                .events
                .iter()
                .filter(|event| event.event == VAULT_EJECTS_ITEM)
                .count();
            apply(&source, &update);
            ejected_after.extend(update.items);
        }
        assert_eq!(state_of(&source, pos), "active");
        assert!(stored_items(&source, pos).is_empty());
        assert_eq!(
            reward_counts(&ejected_after),
            id_counts(&remaining),
            "resuming re-ejects exactly the pending rewards"
        );
        assert_eq!(ejection_events, ejected_after.len());
        let mut total = stacks(&ejected_before);
        total.extend(stacks(&ejected_after));
        assert_eq!(
            id_counts(&total),
            id_counts(&reward),
            "every reward spawns exactly once across the restart"
        );

        // The claim survives a second restart and the next player still gets their own reward.
        assert_eq!(
            try_unlock(&source, pos, a, Some("minecraft:ominous_trial_key"), true),
            Unlock::AlreadyRewarded
        );
        assert!(!unlock_consumes_key(Unlock::AlreadyRewarded));
        source.flush_chunk(0, 0).await.unwrap();
        drop(source);

        let registry = ferrite_world::RuntimeRegistry::load(&root).unwrap();
        let source = AnvilChunkSource::new(world.clone(), registry);
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(state_of(&source, pos), "active");
        assert!(stored_items(&source, pos).is_empty());
        assert!(rewarded(&source, pos, a));
        assert!(!rewarded(&source, pos, b));
        assert_eq!(
            try_unlock(&source, pos, b, Some("minecraft:ominous_trial_key"), true),
            Unlock::Started
        );
        assert!(unlock_consumes_key(Unlock::Started));
        assert!(rewarded(&source, pos, b));

        drop(source);
        let _ = std::fs::remove_dir_all(world);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn crash_before_spawn_keeps_the_reward_pending() {
        let Some(root) = vanilla_root() else {
            eprintln!("SKIP vault crash test: vanilla/26.3/generated is absent");
            return;
        };
        let world = temp_dir("ejection-crash");
        let registry = ferrite_world::RuntimeRegistry::load(&root).unwrap();
        let source = AnvilChunkSource::new(world.clone(), registry);
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let pos = [4, 64, 4];
        let player = Uuid::from_u128(41);
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
        let reward = stored_items(&source, pos);
        assert!(!reward.is_empty());

        for _ in 0..UNLOCKING_DELAY_TICKS {
            let update = tick_vault(&source, pos, &[near()]).expect("vault tick");
            apply(&source, &update);
        }
        assert_eq!(state_of(&source, pos), "ejecting");

        // This tick popped a reward and handed the new ledger to the region. Simulate a crash
        // before the region spawned the item entity: only the block state is applied and the
        // ledger is never written.
        let crash = tick_vault(&source, pos, &[near()]).expect("vault tick");
        assert_eq!(crash.items.len(), 1, "one reward left the vault this tick");
        let pending = crash.pending_items.as_deref().expect("new reward ledger");
        assert_eq!(pending.len(), reward.len() - 1);
        apply_block_state(&source, &crash);

        source.flush_chunk(0, 0).await.unwrap();
        drop(source);

        // The reward was never spawned, so after the restart it must still be pending and the
        // resumed vault must deliver the whole reward exactly once.
        let registry = ferrite_world::RuntimeRegistry::load(&root).unwrap();
        let source = AnvilChunkSource::new(world.clone(), registry);
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(state_of(&source, pos), "ejecting");
        assert_eq!(
            id_counts(&stored_items(&source, pos)),
            id_counts(&reward),
            "the unspawned reward is lost across the restart"
        );

        let mut ejected = Vec::new();
        let mut ejection_events = 0;
        for _ in 0..EJECTING_DELAY_TICKS {
            let update = tick_vault(&source, pos, &[near()]).expect("vault tick");
            ejection_events += update
                .events
                .iter()
                .filter(|event| event.event == VAULT_EJECTS_ITEM)
                .count();
            apply(&source, &update);
            ejected.extend(update.items);
        }
        assert_eq!(state_of(&source, pos), "active");
        assert_eq!(ejection_events, ejected.len());
        assert_eq!(
            reward_counts(&ejected),
            id_counts(&reward),
            "the resumed vault re-ejects the whole reward once"
        );
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
    async fn chunk_unload_pauses_ejection_and_resumes_without_loss_or_duplication() {
        let Some(root) = vanilla_root() else {
            eprintln!("SKIP vault chunk-unload test: vanilla/26.3/generated is absent");
            return;
        };
        let world = temp_dir("chunk-unload");
        let registry = ferrite_world::RuntimeRegistry::load(&root).unwrap();
        let source = AnvilChunkSource::new(world.clone(), registry);
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let pos = [4, 64, 4];
        let player = Uuid::from_u128(51);
        put_vault(&source, pos, "active", false);
        assert_eq!(
            try_unlock(&source, pos, player, Some("minecraft:trial_key"), false),
            Unlock::Started
        );
        let reward = stored_items(&source, pos);
        assert!(!reward.is_empty());

        for _ in 0..UNLOCKING_DELAY_TICKS {
            let update = tick_vault(&source, pos, &[near()]).expect("vault tick");
            apply(&source, &update);
        }
        assert_eq!(state_of(&source, pos), "ejecting");

        let held = HashMap::from([(ChunkPos(0, 0), 1)]);
        let mut vaults = Vaults::default();
        vaults.discover(&source, &held);
        let mut rng = Rng::new(11);
        let mut before = Vec::new();
        for _ in 0..4 {
            for update in vaults.tick(&source, &[near()], &held) {
                for reward in &update.items {
                    before.push(ejected_item(pos, reward, &mut rng));
                }
                apply(&source, &update);
            }
        }
        assert!(!before.is_empty());
        assert_eq!(state_of(&source, pos), "ejecting");

        // The last holder released the chunk: entity sidecar first, then the chunk flush.
        crate::game::entity_save::save(&world, (0, 0), &before).unwrap();
        source.flush_chunk(0, 0).await.unwrap();
        drop(source);

        let registry = ferrite_world::RuntimeRegistry::load(&root).unwrap();
        let source = AnvilChunkSource::new(world.clone(), registry);
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let loaded = crate::game::entity_save::load(&world, (0, 0));
        assert_eq!(loaded.len(), before.len());
        assert_eq!(state_of(&source, pos), "ejecting");

        // Without a holder the vault neither advances nor emits anything.
        let state_before = state_of(&source, pos);
        let timer_before = timer_of(&source, pos);
        let no_hold = HashMap::new();
        let mut idle = Vaults::default();
        idle.discover(&source, &no_hold);
        assert!(idle.tick(&source, &[near()], &no_hold).is_empty());
        assert_eq!(state_of(&source, pos), state_before);
        assert_eq!(timer_of(&source, pos), timer_before);

        // Re-holding the chunk resumes from the persisted ledger.
        let mut resumed = Vec::new();
        let mut vaults = Vaults::default();
        vaults.discover(&source, &held);
        let mut rng = Rng::new(12);
        for _ in 0..EJECTING_DELAY_TICKS {
            for update in vaults.tick(&source, &[near()], &held) {
                for reward in &update.items {
                    resumed.push(ejected_item(pos, reward, &mut rng));
                }
                apply(&source, &update);
            }
        }
        assert_eq!(state_of(&source, pos), "active");
        let mut total = entity_counts(&loaded);
        total.extend(entity_counts(&resumed));
        total.sort_unstable();
        assert_eq!(
            total,
            id_counts(&reward),
            "unload/reload lost or duplicated a reward"
        );
        let mut ids: Vec<_> = loaded
            .iter()
            .chain(resumed.iter())
            .map(|record| record.data.uuid)
            .collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), loaded.len() + resumed.len());

        drop(source);
        let _ = std::fs::remove_dir_all(world);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn region_split_and_merge_keep_exactly_one_vault_ticker() {
        let source = loaded().await;
        let pos = [4, 64, 4];
        let player = Uuid::from_u128(61);
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
        let reward = stored_items(&source, pos);
        assert!(!reward.is_empty());

        for _ in 0..UNLOCKING_DELAY_TICKS {
            let update = tick_vault(&source, pos, &[near()]).expect("vault tick");
            apply(&source, &update);
        }
        assert_eq!(state_of(&source, pos), "ejecting");

        let held = HashMap::from([(ChunkPos(0, 0), 1)]);
        let mut left = Vaults::default();
        left.discover(&source, &held);
        assert_eq!(left.known.len(), 1);
        let mut rng = Rng::new(21);
        let mut delivered = Vec::new();
        let mut events = 0;

        for update in left.tick(&source, &[near()], &held) {
            events += update
                .events
                .iter()
                .filter(|event| event.event == VAULT_EJECTS_ITEM)
                .count();
            for reward in &update.items {
                delivered.push(ejected_item(pos, reward, &mut rng));
            }
            apply(&source, &update);
        }

        // The split moves the tracked vault to the receiving region.
        let mut right = left.split(|_, _| true);
        assert!(left.known.is_empty());
        assert_eq!(right.known.len(), 1);
        assert!(left.tick(&source, &[near()], &held).is_empty());
        for update in right.tick(&source, &[near()], &held) {
            events += update
                .events
                .iter()
                .filter(|event| event.event == VAULT_EJECTS_ITEM)
                .count();
            for reward in &update.items {
                delivered.push(ejected_item(pos, reward, &mut rng));
            }
            apply(&source, &update);
        }

        // Merging back leaves one tracker and does not replay rewards.
        left.merge(right);
        assert_eq!(left.known.len(), 1);
        for _ in 0..EJECTING_DELAY_TICKS {
            for update in left.tick(&source, &[near()], &held) {
                events += update
                    .events
                    .iter()
                    .filter(|event| event.event == VAULT_EJECTS_ITEM)
                    .count();
                for reward in &update.items {
                    delivered.push(ejected_item(pos, reward, &mut rng));
                }
                apply(&source, &update);
            }
        }
        assert_eq!(state_of(&source, pos), "active");
        assert!(stored_items(&source, pos).is_empty());
        assert_eq!(events, delivered.len());
        assert_eq!(
            entity_counts(&delivered),
            id_counts(&reward),
            "region handoff lost or duplicated a reward"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn restart_skips_entities_already_saved_for_their_ledger_token() {
        let Some(root) = vanilla_root() else {
            eprintln!("SKIP vault token-reconciliation test: vanilla/26.3/generated is absent");
            return;
        };
        let world = temp_dir("token-restart");
        let registry = ferrite_world::RuntimeRegistry::load(&root).unwrap();
        let source = AnvilChunkSource::new(world.clone(), registry);
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let pos = [4, 64, 4];
        let player = Uuid::from_u128(71);
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
        let reward = stored_items(&source, pos);
        assert!(!reward.is_empty());

        for _ in 0..UNLOCKING_DELAY_TICKS {
            let update = tick_vault(&source, pos, &[near()]).expect("vault tick");
            apply(&source, &update);
        }
        assert_eq!(state_of(&source, pos), "ejecting");

        // Spawn one item, apply its block-state change, but intentionally do not commit the
        // ledger. Persist the stale ledger and the entity, as the autosave/unload race can.
        let crash = tick_vault(&source, pos, &[near()]).expect("vault tick");
        assert_eq!(crash.items.len(), 1);
        let saved_reward = crash.items[0].clone();
        let mut rng = Rng::new(31);
        let saved_entity = ejected_item(pos, &saved_reward, &mut rng);
        apply_block_state(&source, &crash);
        source.flush_chunk(0, 0).await.unwrap();
        crate::game::entity_save::save(&world, (0, 0), std::slice::from_ref(&saved_entity))
            .unwrap();
        drop(source);

        let registry = ferrite_world::RuntimeRegistry::load(&root).unwrap();
        let source = AnvilChunkSource::new(world.clone(), registry);
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let loaded = crate::game::entity_save::load(&world, (0, 0));
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].data.uuid, saved_reward.token);
        let ledger = stored_rewards(&source, pos);
        assert!(
            ledger
                .iter()
                .any(|reward| reward.token == saved_reward.token),
            "the stale ledger still owns the spawned token"
        );

        let held = HashMap::from([(ChunkPos(0, 0), 1)]);
        let mut vaults = Vaults::default();
        vaults.discover(&source, &held);
        let mut rng = Rng::new(32);
        let mut resumed = Vec::new();
        let mut events = 0;
        for _ in 0..EJECTING_DELAY_TICKS {
            let updates = vaults.tick_with_delivered(&source, &[near()], &held, |token| {
                loaded.iter().any(|record| record.data.uuid == token)
            });
            for update in updates {
                assert!(
                    update
                        .items
                        .iter()
                        .all(|reward| reward.token != saved_reward.token),
                    "the saved entity was spawned again"
                );
                events += update
                    .events
                    .iter()
                    .filter(|event| event.event == VAULT_EJECTS_ITEM)
                    .count();
                for reward in &update.items {
                    resumed.push(ejected_item(pos, reward, &mut rng));
                }
                apply(&source, &update);
            }
        }
        assert_eq!(state_of(&source, pos), "active");
        assert!(stored_items(&source, pos).is_empty());
        assert_eq!(events, resumed.len());
        let mut total = entity_counts(&loaded);
        total.extend(entity_counts(&resumed));
        total.sort_unstable();
        assert_eq!(
            total,
            id_counts(&reward),
            "token reconciliation lost or duplicated a reward"
        );
        let mut ids: Vec<_> = loaded
            .iter()
            .chain(resumed.iter())
            .map(|record| record.data.uuid)
            .collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), loaded.len() + resumed.len());

        drop(source);
        let _ = std::fs::remove_dir_all(world);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_duplicate_unlocks_start_exactly_one_reward() {
        let source = loaded().await;
        let pos = [4, 64, 4];
        put_vault(&source, pos, "active", false);
        let attempts = 4;
        let barrier = Arc::new(Barrier::new(attempts));
        let handles: Vec<_> = (0..attempts)
            .map(|index| {
                let source = source.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    try_unlock(
                        &source,
                        pos,
                        Uuid::from_u128(80 + index as u128),
                        Some("minecraft:trial_key"),
                        false,
                    )
                })
            })
            .collect();
        let outcomes: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| **outcome == Unlock::Started)
                .count(),
            1,
            "one concurrent key use starts the unlock: {outcomes:?}"
        );
        assert!(
            outcomes
                .iter()
                .all(|outcome| matches!(outcome, Unlock::Started | Unlock::Busy))
        );
        assert_eq!(
            rewarded_count(&source, pos),
            1,
            "outcomes={outcomes:?} state={:?} rewards_tag={:?} ledger={:?}",
            state_of(&source, pos),
            source
                .block_entity(pos[0], pos[1], pos[2])
                .and_then(|entity| entity.data.get(REWARDED).cloned()),
            stored_rewards(&source, pos)
        );
        let winner = (0..attempts)
            .map(|index| Uuid::from_u128(80 + index as u128))
            .find(|player| rewarded(&source, pos, *player))
            .expect("one player claimed the reward");
        for index in 0..attempts {
            let player = Uuid::from_u128(80 + index as u128);
            if player != winner {
                assert!(!rewarded(&source, pos, player));
            }
        }

        let reward = stored_items(&source, pos);
        assert!(!reward.is_empty());
        let mut rng = Rng::new(41);
        let mut delivered = Vec::new();
        let mut events = 0;
        for _ in 0..(UNLOCKING_DELAY_TICKS + EJECTING_DELAY_TICKS) {
            let update = tick_vault(&source, pos, &[near()]).expect("vault tick");
            events += update
                .events
                .iter()
                .filter(|event| event.event == VAULT_EJECTS_ITEM)
                .count();
            for reward in &update.items {
                delivered.push(ejected_item(pos, reward, &mut rng));
            }
            apply(&source, &update);
        }
        assert_eq!(state_of(&source, pos), "active");
        assert_eq!(events, delivered.len());
        assert_eq!(
            entity_counts(&delivered),
            id_counts(&reward),
            "duplicate interaction ejected extra rewards"
        );
        let mut ids: Vec<_> = delivered.iter().map(|record| record.data.uuid).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), delivered.len());
    }
}
