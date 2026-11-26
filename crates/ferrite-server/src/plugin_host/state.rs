//! Server-owned v2 block state seam.
//!
//! Reads use the dimension's real [`AnvilChunkSource`]; they never generate a chunk. Writes are
//! validated and queued here, then applied by [`WorldStateProvider::tick`] from the game-loop
//! tick. `world.block_set_async` returning `queued: true` therefore means exactly that: the
//! write was accepted, not applied.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use crate::game::entity_index::EntityIndex;
use crate::game::{AnvilChunkSource, Dimension, DimensionRegistry, ride};
use base64::Engine as _;
use ferrite_plugin_host::{
    BiomeSnapshot, BlockGetRequest, BlockSetAccepted, BlockSetRequest, BlockSnapshot, ChunkCoord,
    ChunkStatusSnapshot, EntityDropChanceSetRequest, EntityEquipmentSetAccepted,
    EntityEquipmentSetRequest, HostStateProvider, InventorySetAccepted, InventorySlotPatch,
    LoadedChunksSnapshot, PlayerInventorySetRequest, PlayerInventorySnapshot, StateError,
    WorldEntitiesSnapshot, WorldEntitySnapshotEntry,
};
use serde_json::{Value, json};
use uuid::Uuid;

/// Maximum accepted-but-not-yet-applied block writes. Overflow is a hard `queue_full` error.
pub const MAX_PENDING_BLOCK_WRITES: usize = 4096;

/// Maximum accepted-but-not-yet-applied inventory writes. Overflow is a hard `queue_full` error.
pub const MAX_PENDING_INVENTORY_WRITES: usize = 4096;

/// Maximum chunk coordinates returned by a single `world.loaded_chunks` snapshot.
pub const MAX_LOADED_CHUNKS_SNAPSHOT: usize = 4096;

/// Maximum entities returned by a single `world.entities_snapshot` call.
pub const MAX_ENTITIES_SNAPSHOT: usize = 1024;

/// Maximum accepted-but-not-yet-applied entity equipment writes.
pub const MAX_PENDING_EQUIPMENT_WRITES: usize = 4096;

/// Default view distance (in chunks) used when computing `world.chunk_status` `in_use`.
const DEFAULT_CHUNK_VIEW_DISTANCE: i32 = 10;

/// Shared registry of live player inventories keyed by player UUID.
pub type PlayerInventoryRegistry = Arc<RwLock<HashMap<Uuid, ferrite_inventory::PlayerInventory>>>;

struct QueuedWrite {
    source: Arc<AnvilChunkSource>,
    x: i32,
    y: i32,
    z: i32,
    state_id: u32,
    expected_state_id: Option<u32>,
}

struct QueuedInventoryWrite {
    uuid: Uuid,
    held_slot: Option<u8>,
    slots: Vec<(usize, ferrite_item::ItemStack)>,
}

struct QueuedEntityEquipmentWrite {
    uuid: Uuid,
    slot: ferrite_entity::EquipmentSlot,
    item: Option<ferrite_item::ItemStack>,
}

struct QueuedEntityDropChanceWrite {
    uuid: Uuid,
    slot: ferrite_entity::EquipmentSlot,
    chance: f32,
}

/// Callback invoked after `WorldStateProvider::drain` applies a queued inventory write.
pub type InventorySyncFn = Arc<dyn Fn(Uuid, ferrite_inventory::PlayerInventory) + Send + Sync>;

/// Callback invoked when `WorldStateProvider::drain` routes a queued entity equipment write.
pub type EntityEquipmentSyncFn =
    Arc<dyn Fn(Uuid, ferrite_entity::EquipmentSlot, Option<ferrite_item::ItemStack>) + Send + Sync>;

/// Callback invoked when `WorldStateProvider::drain` routes a queued entity drop-chance write.
pub type EntityDropChanceSyncFn =
    Arc<dyn Fn(Uuid, ferrite_entity::EquipmentSlot, f32) + Send + Sync>;

/// Ownership-safe snapshot of an online player's live state.
#[derive(Debug, Clone, PartialEq)]
pub struct LivePlayerState {
    pub uuid: Uuid,
    pub name: String,
    pub world: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub vx: f64,
    pub vy: f64,
    pub vz: f64,
    pub game_mode: String,
    pub health: f32,
    pub food_level: i32,
    pub network_id: i32,
}

/// Ownership-safe snapshot of a non-player entity's state (used for direct registration/tests
/// alongside the region-published [`EntityIndex`]).
#[derive(Debug, Clone, PartialEq)]
pub struct LiveEntityState {
    pub uuid: Uuid,
    pub type_key: String,
    pub name: String,
    pub custom_name: Option<String>,
    pub world: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub vx: f64,
    pub vy: f64,
    pub vz: f64,
    pub health: Option<f32>,
    pub network_id: i32,
    pub vehicle: Option<Uuid>,
    pub passengers: Vec<Uuid>,
}

/// Adapter from the contract-v2 provider seam to Ferrite's actual dimension sources.
pub struct WorldStateProvider {
    dimensions: Arc<DimensionRegistry>,
    inventories: PlayerInventoryRegistry,
    players: RwLock<HashMap<Uuid, LivePlayerState>>,
    ops: RwLock<std::collections::HashSet<Uuid>>,
    entities: RwLock<HashMap<Uuid, LiveEntityState>>,
    entity_index: Mutex<Option<Arc<Mutex<EntityIndex>>>>,
    on_inventory_applied: Mutex<Option<InventorySyncFn>>,
    on_entity_equipment_applied: Mutex<Option<EntityEquipmentSyncFn>>,
    on_entity_drop_chance_applied: Mutex<Option<EntityDropChanceSyncFn>>,
    command_tx:
        Mutex<Option<tokio::sync::mpsc::UnboundedSender<crate::game::plugin_types::PlayerCommand>>>,
    pending: Mutex<VecDeque<QueuedWrite>>,
    pending_inventory: Mutex<VecDeque<QueuedInventoryWrite>>,
    pending_equipment: Mutex<VecDeque<QueuedEntityEquipmentWrite>>,
    pending_drop_chances: Mutex<VecDeque<QueuedEntityDropChanceWrite>>,
    next_request: AtomicU64,
}

impl WorldStateProvider {
    /// Wraps the world's real dimension registry.
    pub fn new(dimensions: Arc<DimensionRegistry>) -> Self {
        Self::with_inventories(dimensions, Arc::new(RwLock::new(HashMap::new())))
    }

    /// Wraps the world's dimension registry and live player inventory registry.
    pub fn with_inventories(
        dimensions: Arc<DimensionRegistry>,
        inventories: PlayerInventoryRegistry,
    ) -> Self {
        Self {
            dimensions,
            inventories,
            players: RwLock::new(HashMap::new()),
            ops: RwLock::new(std::collections::HashSet::new()),
            entities: RwLock::new(HashMap::new()),
            entity_index: Mutex::new(None),
            on_inventory_applied: Mutex::new(None),
            on_entity_equipment_applied: Mutex::new(None),
            on_entity_drop_chance_applied: Mutex::new(None),
            command_tx: Mutex::new(None),
            pending: Mutex::new(VecDeque::new()),
            pending_inventory: Mutex::new(VecDeque::new()),
            pending_equipment: Mutex::new(VecDeque::new()),
            pending_drop_chances: Mutex::new(VecDeque::new()),
            next_request: AtomicU64::new(0),
        }
    }

    /// Attaches the region-published entity snapshot index for `entity.state_get`.
    pub fn set_entity_index(&self, index: Arc<Mutex<EntityIndex>>) {
        *self
            .entity_index
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(index);
    }

    /// Attaches the game-loop `PlayerCommand` sender for outbound plugin packets.
    pub fn set_command_sender(
        &self,
        sender: tokio::sync::mpsc::UnboundedSender<crate::game::plugin_types::PlayerCommand>,
    ) {
        *self
            .command_tx
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(sender);
    }

    /// Updates whether an online player is a server operator.
    pub fn set_player_op(&self, uuid: Uuid, op: bool) {
        let mut ops = self
            .ops
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if op {
            ops.insert(uuid);
        } else {
            ops.remove(&uuid);
        }
    }

    /// Registers or replaces an online player's live snapshot from its components.
    #[allow(clippy::too_many_arguments)]
    pub fn register_player_snapshot(
        &self,
        uuid: Uuid,
        name: String,
        world: String,
        pos: [f64; 3],
        yaw: f32,
        pitch: f32,
        game_mode: &str,
        health: f32,
        food_level: i32,
        network_id: i32,
    ) {
        self.register_player_state(LivePlayerState {
            uuid,
            name,
            world,
            x: pos[0],
            y: pos[1],
            z: pos[2],
            yaw,
            pitch,
            vx: 0.0,
            vy: 0.0,
            vz: 0.0,
            game_mode: game_mode.to_owned(),
            health,
            food_level,
            network_id,
        });
    }

    /// Registers or replaces an online player's live snapshot.
    pub fn register_player_state(&self, state: LivePlayerState) {
        self.players
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(state.uuid, state);
    }

    /// Removes a player's live snapshot when they disconnect.
    pub fn remove_player_state(&self, uuid: Uuid) {
        self.players
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&uuid);
    }

    /// Updates a player's cached position, rotation, and optional world key.
    pub fn update_player_location(
        &self,
        uuid: Uuid,
        world: Option<&str>,
        pos: [f64; 3],
        yaw: f32,
        pitch: f32,
    ) {
        let mut players = self
            .players
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(player) = players.get_mut(&uuid) {
            if let Some(world_key) = world {
                player.world = world_key.to_owned();
            }
            player.x = pos[0];
            player.y = pos[1];
            player.z = pos[2];
            player.yaw = yaw;
            player.pitch = pitch;
        }
    }

    /// Updates a player's cached game mode.
    pub fn update_player_game_mode(&self, uuid: Uuid, game_mode: &str) {
        let mut players = self
            .players
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(player) = players.get_mut(&uuid) {
            player.game_mode = game_mode.to_owned();
        }
    }

    /// Updates a player's cached health.
    pub fn update_player_health(&self, uuid: Uuid, health: f32) {
        let mut players = self
            .players
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(player) = players.get_mut(&uuid) {
            player.health = health;
        }
    }

    /// Updates a player's cached food level.
    pub fn update_player_food(&self, uuid: Uuid, food_level: i32) {
        let mut players = self
            .players
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(player) = players.get_mut(&uuid) {
            player.food_level = food_level;
        }
    }

    /// Registers or replaces a non-player entity snapshot (for tests or direct injection).
    pub fn register_entity_state(&self, state: LiveEntityState) {
        self.entities
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(state.uuid, state);
    }

    /// Removes a directly registered non-player entity snapshot.
    pub fn remove_entity_state(&self, uuid: Uuid) {
        self.entities
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&uuid);
    }

    /// Installs a region-routing callback invoked whenever `drain` applies a queued inventory write.
    pub fn set_inventory_sync(&self, sync: InventorySyncFn) {
        *self
            .on_inventory_applied
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(sync);
    }

    /// Installs a region-routing callback invoked whenever `drain` routes a queued entity equipment write.
    pub fn set_entity_equipment_sync(&self, sync: EntityEquipmentSyncFn) {
        *self
            .on_entity_equipment_applied
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(sync);
    }

    /// Installs a region-routing callback invoked whenever `drain` routes a queued entity drop-chance write.
    pub fn set_entity_drop_chance_sync(&self, sync: EntityDropChanceSyncFn) {
        *self
            .on_entity_drop_chance_applied
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(sync);
    }

    /// Removes a player's inventory when they disconnect.
    pub fn remove_player_inventory(&self, uuid: Uuid) {
        self.inventories
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&uuid);
    }

    /// Registers or replaces a live player's inventory in this provider.
    pub fn register_player_inventory(
        &self,
        uuid: Uuid,
        inventory: ferrite_inventory::PlayerInventory,
    ) {
        self.inventories
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(uuid, inventory);
    }

    /// Returns the shared player inventory registry.
    pub fn inventories(&self) -> &PlayerInventoryRegistry {
        &self.inventories
    }

    fn source(&self, world: &str) -> Result<Arc<AnvilChunkSource>, StateError> {
        let dimension = match world {
            "minecraft:overworld" => Dimension::Overworld,
            "minecraft:the_nether" => Dimension::Nether,
            "minecraft:the_end" => Dimension::End,
            _ => return Err(StateError::unknown_world(world)),
        };
        Ok(Arc::clone(self.dimensions.source(dimension)))
    }

    fn snapshot(
        &self,
        world: &str,
        id: u16,
        x: i32,
        y: i32,
        z: i32,
    ) -> Result<BlockSnapshot, StateError> {
        let (material, properties) = ferrite_data::blocks::state(id)
            .ok_or_else(|| StateError::unknown_block_state(u32::from(id)))?;
        Ok(BlockSnapshot {
            world: world.to_owned(),
            x,
            y,
            z,
            state_id: u32::from(id),
            material: material.to_owned(),
            properties: properties
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect::<BTreeMap<_, _>>(),
            loaded: true,
        })
    }

    /// Applies the queue on the game-loop thread. Every failure is logged; it never panics the
    /// tick loop. A failed write is not silently retried.
    pub fn drain(&self) {
        let writes: Vec<QueuedWrite> = {
            let mut pending = self
                .pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            pending.drain(..).collect()
        };
        for write in writes {
            match write.source.compare_set_block(
                write.x,
                write.y,
                write.z,
                write.expected_state_id,
                write.state_id,
            ) {
                Ok(()) => tracing::debug!(
                    x = write.x,
                    y = write.y,
                    z = write.z,
                    state = write.state_id,
                    "applied a plugin block write"
                ),
                Err(error) => tracing::warn!(
                    x = write.x,
                    y = write.y,
                    z = write.z,
                    state = write.state_id,
                    %error,
                    "a queued plugin block write failed"
                ),
            }
        }
        let inv_writes: Vec<QueuedInventoryWrite> = {
            let mut pending = self
                .pending_inventory
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            pending.drain(..).collect()
        };
        if !inv_writes.is_empty() {
            let mut applied = Vec::with_capacity(inv_writes.len());
            {
                let mut inventories = self
                    .inventories
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                for write in inv_writes {
                    let Some(inventory) = inventories.get_mut(&write.uuid) else {
                        tracing::warn!(uuid = %write.uuid, "queued inventory write skipped: player offline");
                        continue;
                    };
                    if let Some(held) = write.held_slot {
                        inventory.selected = usize::from(held.min(8));
                    }
                    for (slot, stack) in write.slots {
                        inventory.set(slot, stack);
                    }
                    applied.push((write.uuid, inventory.clone()));
                }
            }
            if !applied.is_empty()
                && let Some(sync) = self
                    .on_inventory_applied
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone()
            {
                for (uuid, inv) in applied {
                    sync(uuid, inv);
                }
            }
        }
        let equip_writes: Vec<QueuedEntityEquipmentWrite> = {
            let mut pending = self
                .pending_equipment
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            pending.drain(..).collect()
        };
        if !equip_writes.is_empty()
            && let Some(sync) = self
                .on_entity_equipment_applied
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        {
            for write in equip_writes {
                sync(write.uuid, write.slot, write.item);
            }
        }
        let drop_chance_writes: Vec<QueuedEntityDropChanceWrite> = {
            let mut pending = self
                .pending_drop_chances
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            pending.drain(..).collect()
        };
        if !drop_chance_writes.is_empty()
            && let Some(sync) = self
                .on_entity_drop_chance_applied
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        {
            for write in drop_chance_writes {
                sync(write.uuid, write.slot, write.chance);
            }
        }
    }
}

fn encode_slot_patch(stack: &ferrite_item::ItemStack) -> Result<InventorySlotPatch, StateError> {
    let id = stack
        .name()
        .ok_or_else(|| StateError::invalid(format!("unregistered item id {}", stack.item)))?
        .to_string();
    let nbt_base64 = if stack.components.is_empty() {
        None
    } else {
        let compound = stack
            .to_nbt()
            .map_err(|err| StateError::invalid(format!("item nbt encode error: {err}")))?;
        let bytes = ferrite_nbt::binary::write_named("", &ferrite_nbt::Tag::Compound(compound))
            .map_err(|err| StateError::invalid(format!("item binary nbt error: {err}")))?;
        Some(base64::engine::general_purpose::STANDARD.encode(bytes))
    };
    Ok(InventorySlotPatch {
        id,
        count: stack.count,
        nbt_base64,
    })
}

fn decode_slot_patch(patch: &InventorySlotPatch) -> Result<ferrite_item::ItemStack, StateError> {
    if patch.count <= 0 || patch.count > 99 {
        return Err(StateError::invalid(format!(
            "slot count {} out of range 1..=99",
            patch.count
        )));
    }
    if let Some(encoded) = &patch.nbt_base64 {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|err| StateError::invalid(format!("invalid nbt_base64: {err}")))?;
        let (_, tag) = ferrite_nbt::binary::read_named(&bytes)
            .map_err(|err| StateError::invalid(format!("invalid binary nbt: {err}")))?;
        let ferrite_nbt::Tag::Compound(mut root) = tag else {
            return Err(StateError::invalid(
                "item nbt_base64 root tag must be a compound",
            ));
        };
        let mut stack = ferrite_item::ItemStack::from_nbt(&root).map_err(|err| {
            if err.to_string().contains("unknown") {
                StateError::unknown_item(&patch.id)
            } else {
                StateError::invalid(format!("invalid item nbt: {err}"))
            }
        })?;
        if let Some(expected_id) = stack.name()
            && expected_id != patch.id
        {
            return Err(StateError::invalid(format!(
                "slot patch id '{}' does not match nbt id '{expected_id}'",
                patch.id
            )));
        }
        let _ = &mut root;
        stack.count = patch.count;
        return Ok(stack);
    }
    let mut stack = ferrite_item::ItemStack::from_name(&patch.id)
        .ok_or_else(|| StateError::unknown_item(&patch.id))?;
    stack.count = patch.count;
    Ok(stack)
}

impl HostStateProvider for WorldStateProvider {
    fn block_get(&self, request: &BlockGetRequest) -> Result<BlockSnapshot, StateError> {
        let source = self.source(&request.world)?;
        let state = source
            .block(request.x, request.y, request.z)
            .ok_or_else(|| {
                StateError::chunk_not_loaded(request.x.div_euclid(16), request.z.div_euclid(16))
            })?;
        let id = u16::try_from(state).map_err(|_| StateError::unknown_block_state(state))?;
        self.snapshot(&request.world, id, request.x, request.y, request.z)
    }

    fn block_set_async(&self, request: &BlockSetRequest) -> Result<BlockSetAccepted, StateError> {
        let source = self.source(&request.world)?;
        let state = u16::try_from(request.state_id)
            .map_err(|_| StateError::unknown_block_state(request.state_id))?;
        if ferrite_data::blocks::state(state).is_none() {
            return Err(StateError::unknown_block_state(request.state_id));
        }
        if !source.chunk_resident(request.x.div_euclid(16), request.z.div_euclid(16)) {
            return Err(StateError::chunk_not_loaded(
                request.x.div_euclid(16),
                request.z.div_euclid(16),
            ));
        }
        if let Some(expected) = request.expected_state_id
            && source.block(request.x, request.y, request.z) != Some(expected)
        {
            return Err(StateError::stale_request(
                expected,
                source.block(request.x, request.y, request.z).unwrap_or(0),
            ));
        }
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if pending.len() >= MAX_PENDING_BLOCK_WRITES {
            return Err(StateError::queue_full());
        }
        pending.push_back(QueuedWrite {
            source,
            x: request.x,
            y: request.y,
            z: request.z,
            state_id: request.state_id,
            expected_state_id: request.expected_state_id,
        });
        Ok(BlockSetAccepted {
            queued: true,
            request_id: self.next_request.fetch_add(1, Ordering::SeqCst) + 1,
        })
    }

    fn chunk_loaded(&self, world: &str, x: i32, z: i32) -> Result<bool, StateError> {
        Ok(self.source(world)?.chunk_resident(x, z))
    }

    fn player_inventory_get(&self, uuid: &str) -> Result<PlayerInventorySnapshot, StateError> {
        let parsed = Uuid::parse_str(uuid)
            .map_err(|_| StateError::invalid(format!("invalid player uuid: {uuid}")))?;
        let inventories = self
            .inventories
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let inventory = inventories
            .get(&parsed)
            .ok_or_else(|| StateError::player_not_found(uuid))?;
        let mut slots = BTreeMap::new();
        for slot in 0..ferrite_inventory::SIZE {
            let stack = inventory.get(slot);
            if !stack.is_empty() {
                slots.insert(slot.to_string(), encode_slot_patch(stack)?);
            }
        }
        Ok(PlayerInventorySnapshot {
            uuid: parsed.to_string(),
            held_slot: inventory.selected_slot() as u8,
            slots,
        })
    }

    fn player_inventory_set_async(
        &self,
        request: &PlayerInventorySetRequest,
    ) -> Result<InventorySetAccepted, StateError> {
        let parsed = Uuid::parse_str(&request.uuid)
            .map_err(|_| StateError::invalid(format!("invalid player uuid: {}", request.uuid)))?;
        {
            let inventories = self
                .inventories
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if !inventories.contains_key(&parsed) {
                return Err(StateError::player_not_found(&request.uuid));
            }
        }
        if let Some(held) = request.held_slot
            && usize::from(held) >= ferrite_inventory::HOTBAR
        {
            return Err(StateError::invalid(format!(
                "held_slot {held} out of range 0..9"
            )));
        }
        let mut decoded_slots = Vec::with_capacity(request.slots.len());
        for (slot_key, maybe_patch) in &request.slots {
            let slot: usize = slot_key
                .parse()
                .map_err(|_| StateError::invalid(format!("invalid slot index '{slot_key}'")))?;
            if slot >= ferrite_inventory::SIZE {
                return Err(StateError::invalid(format!(
                    "slot {slot} out of range 0..{}",
                    ferrite_inventory::SIZE
                )));
            }
            let stack = match maybe_patch {
                Some(patch) => decode_slot_patch(patch)?,
                None => ferrite_item::ItemStack::empty(),
            };
            decoded_slots.push((slot, stack));
        }
        let mut pending = self
            .pending_inventory
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if pending.len() >= MAX_PENDING_INVENTORY_WRITES {
            return Err(StateError::queue_full());
        }
        pending.push_back(QueuedInventoryWrite {
            uuid: parsed,
            held_slot: request.held_slot,
            slots: decoded_slots,
        });
        Ok(InventorySetAccepted {
            queued: true,
            request_id: self.next_request.fetch_add(1, Ordering::SeqCst) + 1,
        })
    }

    fn player_state_get(&self, uuid: &str) -> Result<Value, StateError> {
        let parsed = Uuid::parse_str(uuid)
            .map_err(|_| StateError::invalid(format!("invalid player uuid: {uuid}")))?;
        let snapshot = {
            let players = self
                .players
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            players
                .get(&parsed)
                .cloned()
                .ok_or_else(|| StateError::player_not_found(uuid))?
        };
        let vehicle = ride::vehicle_of(parsed).map(|id| id.to_string());
        let passengers: Vec<String> = ride::passengers_of(parsed)
            .into_iter()
            .map(|id| id.to_string())
            .collect();
        let op = self
            .ops
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(&parsed);
        Ok(json!({
            "uuid": snapshot.uuid.to_string(),
            "type": "minecraft:player",
            "op": op,
            "name": snapshot.name,
            "world": snapshot.world,
            "x": snapshot.x,
            "y": snapshot.y,
            "z": snapshot.z,
            "yaw": snapshot.yaw,
            "pitch": snapshot.pitch,
            "vx": snapshot.vx,
            "vy": snapshot.vy,
            "vz": snapshot.vz,
            "game_mode": snapshot.game_mode,
            "health": snapshot.health,
            "food_level": snapshot.food_level,
            "network_id": snapshot.network_id,
            "vehicle": vehicle,
            "passengers": passengers,
        }))
    }

    fn entity_state_get(&self, uuid: &str) -> Result<Value, StateError> {
        let parsed = Uuid::parse_str(uuid)
            .map_err(|_| StateError::invalid(format!("invalid entity uuid: {uuid}")))?;
        let (index_entry, index_equipment) = {
            let guard = self
                .entity_index
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard
                .as_ref()
                .and_then(|index| {
                    index
                        .lock()
                        .ok()
                        .map(|idx| (idx.get(parsed).cloned(), idx.equipment_of(parsed).cloned()))
                })
                .unwrap_or((None, None))
        };
        if let Some(entity) = index_entry {
            let vehicle = ride::vehicle_of(parsed).map(|id| id.to_string());
            let passengers: Vec<String> = ride::passengers_of(parsed)
                .into_iter()
                .map(|id| id.to_string())
                .collect();
            let mut equipment_map = serde_json::Map::new();
            let mut drop_chances_map = serde_json::Map::new();
            if let Some(eq) = index_equipment {
                for (slot, stack) in eq.iter() {
                    if let Ok(patch) = encode_slot_patch(stack)
                        && let Ok(val) = serde_json::to_value(patch)
                    {
                        equipment_map.insert(slot.as_name().to_owned(), val);
                    }
                }
                for (slot, chance) in eq.drop_chances_iter() {
                    if let Ok(val) = serde_json::to_value(chance) {
                        drop_chances_map.insert(slot.as_name().to_owned(), val);
                    }
                }
            }
            return Ok(json!({
                "uuid": entity.uuid.to_string(),
                "type": entity.type_name(),
                "name": entity.display_name(),
                "custom_name": entity.custom_name,
                "world": entity.dimension.key(),
                "x": entity.pos[0],
                "y": entity.pos[1],
                "z": entity.pos[2],
                "yaw": entity.yaw,
                "pitch": entity.pitch,
                "vx": 0.0,
                "vy": 0.0,
                "vz": 0.0,
                "health": entity.health,
                "network_id": entity.network_id,
                "vehicle": vehicle,
                "passengers": passengers,
                "equipment": Value::Object(equipment_map),
                "drop_chances": Value::Object(drop_chances_map),
            }));
        }
        let direct_entry = self
            .entities
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&parsed)
            .cloned();
        if let Some(entity) = direct_entry {
            let vehicle = entity
                .vehicle
                .or_else(|| ride::vehicle_of(parsed))
                .map(|id| id.to_string());
            let passengers: Vec<String> = if entity.passengers.is_empty() {
                ride::passengers_of(parsed)
                    .into_iter()
                    .map(|id| id.to_string())
                    .collect()
            } else {
                entity.passengers.iter().map(Uuid::to_string).collect()
            };
            return Ok(json!({
                "uuid": entity.uuid.to_string(),
                "type": entity.type_key,
                "name": entity.name,
                "custom_name": entity.custom_name,
                "world": entity.world,
                "x": entity.x,
                "y": entity.y,
                "z": entity.z,
                "yaw": entity.yaw,
                "pitch": entity.pitch,
                "vx": entity.vx,
                "vy": entity.vy,
                "vz": entity.vz,
                "health": entity.health,
                "network_id": entity.network_id,
                "vehicle": vehicle,
                "passengers": passengers,
            }));
        }
        if let Ok(player_json) = self.player_state_get(uuid) {
            return Ok(player_json);
        }
        Err(StateError::new(
            "entity_not_found",
            format!("unknown entity '{uuid}'"),
        ))
    }

    fn supported_ops(&self) -> Vec<&'static str> {
        vec![
            "world.block_get",
            "world.block_set_async",
            "world.chunk_loaded",
            "player.state_get",
            "player.inventory_get",
            "player.inventory_set_async",
            "entity.state_get",
        ]
    }

    fn chunk_status(&self, world: &str, x: i32, z: i32) -> Result<ChunkStatusSnapshot, StateError> {
        let source = self.source(world)?;
        let loaded = source.chunk_resident(x, z);
        let generated = loaded || source.chunk_saved(x, z);
        let in_use = loaded
            && self
                .players
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .values()
                .any(|p| {
                    p.world == world && {
                        let pcx = (p.x.floor() as i32).div_euclid(16);
                        let pcz = (p.z.floor() as i32).div_euclid(16);
                        (pcx - x).abs().max((pcz - z).abs()) <= DEFAULT_CHUNK_VIEW_DISTANCE
                    }
                });
        Ok(ChunkStatusSnapshot {
            world: world.to_owned(),
            x,
            z,
            loaded,
            generated,
            in_use,
        })
    }

    fn loaded_chunks(
        &self,
        world: &str,
        limit: Option<usize>,
    ) -> Result<LoadedChunksSnapshot, StateError> {
        if limit == Some(0) {
            return Err(StateError::invalid("limit must be at least 1"));
        }
        let cap = limit
            .unwrap_or(MAX_LOADED_CHUNKS_SNAPSHOT)
            .min(MAX_LOADED_CHUNKS_SNAPSHOT);
        let coords = self.source(world)?.resident_chunks();
        let total = coords.len();
        let truncated = total > cap;
        let chunks = coords
            .into_iter()
            .take(cap)
            .map(|(x, z)| ChunkCoord { x, z })
            .collect();
        Ok(LoadedChunksSnapshot {
            world: world.to_owned(),
            chunks,
            total,
            truncated,
        })
    }

    fn entities_snapshot(
        &self,
        world: &str,
        limit: Option<usize>,
    ) -> Result<WorldEntitiesSnapshot, StateError> {
        let _ = self.source(world)?;
        if limit == Some(0) {
            return Err(StateError::invalid("limit must be at least 1"));
        }
        let cap = limit
            .unwrap_or(MAX_ENTITIES_SNAPSHOT)
            .min(MAX_ENTITIES_SNAPSHOT);
        let mut by_uuid: BTreeMap<Uuid, WorldEntitySnapshotEntry> = BTreeMap::new();
        {
            let players = self
                .players
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for player in players.values() {
                if player.world == world {
                    by_uuid.insert(
                        player.uuid,
                        WorldEntitySnapshotEntry {
                            id: player.network_id,
                            uuid: player.uuid.to_string(),
                            type_key: "minecraft:player".to_owned(),
                            name: player.name.clone(),
                            custom_name: None,
                            world: player.world.clone(),
                            x: player.x,
                            y: player.y,
                            z: player.z,
                            yaw: player.yaw,
                            pitch: player.pitch,
                            vx: player.vx,
                            vy: player.vy,
                            vz: player.vz,
                            health: Some(player.health),
                        },
                    );
                }
            }
        }
        {
            let index_guard = self
                .entity_index
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(index) = index_guard.as_ref()
                && let Ok(idx) = index.lock()
            {
                for entity in idx.entries() {
                    if entity.dimension.key() == world {
                        by_uuid
                            .entry(entity.uuid)
                            .or_insert_with(|| WorldEntitySnapshotEntry {
                                id: entity.network_id,
                                uuid: entity.uuid.to_string(),
                                type_key: entity.type_name().to_owned(),
                                name: entity.display_name(),
                                custom_name: entity.custom_name.clone(),
                                world: world.to_owned(),
                                x: entity.pos[0],
                                y: entity.pos[1],
                                z: entity.pos[2],
                                yaw: entity.yaw,
                                pitch: entity.pitch,
                                vx: 0.0,
                                vy: 0.0,
                                vz: 0.0,
                                health: entity.health,
                            });
                    }
                }
            }
        }
        {
            let entities = self
                .entities
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for entity in entities.values() {
                if entity.world == world {
                    by_uuid
                        .entry(entity.uuid)
                        .or_insert_with(|| WorldEntitySnapshotEntry {
                            id: entity.network_id,
                            uuid: entity.uuid.to_string(),
                            type_key: entity.type_key.clone(),
                            name: entity.name.clone(),
                            custom_name: entity.custom_name.clone(),
                            world: entity.world.clone(),
                            x: entity.x,
                            y: entity.y,
                            z: entity.z,
                            yaw: entity.yaw,
                            pitch: entity.pitch,
                            vx: entity.vx,
                            vy: entity.vy,
                            vz: entity.vz,
                            health: entity.health,
                        });
                }
            }
        }
        let total = by_uuid.len();
        let truncated = total > cap;
        let entities = by_uuid.into_values().take(cap).collect();
        Ok(WorldEntitiesSnapshot {
            world: world.to_owned(),
            entities,
            total,
            truncated,
        })
    }

    fn biome_get(&self, world: &str, x: i32, y: i32, z: i32) -> Result<BiomeSnapshot, StateError> {
        let source = self.source(world)?;
        let (cx, cz) = (x.div_euclid(16), z.div_euclid(16));
        if !source.chunk_resident(cx, cz) {
            return Err(StateError::chunk_not_loaded(cx, cz));
        }
        let biome = source.biome_name(x, y, z).ok_or_else(|| {
            StateError::new(
                "biome_unavailable",
                format!("no biome cell at ({x}, {y}, {z}) in '{world}'"),
            )
        })?;
        Ok(BiomeSnapshot {
            world: world.to_owned(),
            x,
            y,
            z,
            biome,
        })
    }

    fn entity_equipment_set_async(
        &self,
        request: &EntityEquipmentSetRequest,
    ) -> Result<EntityEquipmentSetAccepted, StateError> {
        let parsed = Uuid::parse_str(&request.uuid)
            .map_err(|_| StateError::invalid(format!("invalid entity uuid: {}", request.uuid)))?;
        let slot = ferrite_entity::EquipmentSlot::from_name(&request.slot).ok_or_else(|| {
            StateError::invalid(format!("unsupported equipment slot '{}'", request.slot))
        })?;
        let item = match &request.item {
            Some(patch) => Some(decode_slot_patch(patch)?),
            None => None,
        };
        let in_index = {
            let guard = self
                .entity_index
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard
                .as_ref()
                .and_then(|idx| idx.lock().ok().map(|i| i.get(parsed).is_some()))
                .unwrap_or(false)
        };
        let in_direct = self
            .entities
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains_key(&parsed);
        let in_players = self
            .players
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains_key(&parsed);
        if !in_index && !in_direct && !in_players {
            return Err(StateError::new(
                "entity_not_found",
                format!("unknown entity '{}'", request.uuid),
            ));
        }
        let mut pending = self
            .pending_equipment
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if pending.len() >= MAX_PENDING_EQUIPMENT_WRITES {
            return Err(StateError::queue_full());
        }
        pending.push_back(QueuedEntityEquipmentWrite {
            uuid: parsed,
            slot,
            item,
        });
        Ok(EntityEquipmentSetAccepted {
            queued: true,
            request_id: self.next_request.fetch_add(1, Ordering::SeqCst) + 1,
        })
    }

    fn entity_drop_chance_set_async(
        &self,
        request: &EntityDropChanceSetRequest,
    ) -> Result<EntityEquipmentSetAccepted, StateError> {
        let parsed = Uuid::parse_str(&request.uuid)
            .map_err(|_| StateError::invalid(format!("invalid entity uuid: {}", request.uuid)))?;
        let slot = ferrite_entity::EquipmentSlot::from_name(&request.slot).ok_or_else(|| {
            StateError::invalid(format!("unsupported equipment slot '{}'", request.slot))
        })?;
        if !request.chance.is_finite() {
            return Err(StateError::invalid(format!(
                "drop chance must be finite, got {}",
                request.chance
            )));
        }
        let in_index = {
            let guard = self
                .entity_index
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard
                .as_ref()
                .and_then(|idx| idx.lock().ok().map(|i| i.get(parsed).is_some()))
                .unwrap_or(false)
        };
        let in_direct = self
            .entities
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains_key(&parsed);
        let in_players = self
            .players
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains_key(&parsed);
        if !in_index && !in_direct && !in_players {
            return Err(StateError::new(
                "entity_not_found",
                format!("unknown entity '{}'", request.uuid),
            ));
        }
        let mut pending = self
            .pending_drop_chances
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if pending.len() >= MAX_PENDING_EQUIPMENT_WRITES {
            return Err(StateError::queue_full());
        }
        pending.push_back(QueuedEntityDropChanceWrite {
            uuid: parsed,
            slot,
            chance: request.chance,
        });
        Ok(EntityEquipmentSetAccepted {
            queued: true,
            request_id: self.next_request.fetch_add(1, Ordering::SeqCst) + 1,
        })
    }

    fn send_plugin_message(
        &self,
        player: Option<&str>,
        channel: &str,
        data: Vec<u8>,
    ) -> Result<(), StateError> {
        let tx = self
            .command_tx
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .ok_or_else(StateError::unavailable)?;
        let packet = crate::game::plugin_types::PluginPacket::PluginMessage {
            channel: channel.to_owned(),
            data,
        };
        if let Some(uuid_str) = player {
            let uuid = Uuid::parse_str(uuid_str)
                .map_err(|_| StateError::invalid(format!("invalid player uuid: {uuid_str}")))?;
            let known = self
                .players
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .contains_key(&uuid);
            if !known {
                return Err(StateError::player_not_found(uuid_str));
            }
            tx.send(crate::game::plugin_types::PlayerCommand::Packet {
                player: uuid,
                packet,
            })
            .map_err(|_| StateError::unavailable())?;
        } else {
            let uuids: Vec<Uuid> = self
                .players
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .keys()
                .copied()
                .collect();
            for uuid in uuids {
                let _ = tx.send(crate::game::plugin_types::PlayerCommand::Packet {
                    player: uuid,
                    packet: packet.clone(),
                });
            }
        }
        Ok(())
    }

    fn send_player_title(
        &self,
        player_uuid: &str,
        title: Option<String>,
        subtitle: Option<String>,
        fade_in: i32,
        stay: i32,
        fade_out: i32,
        clear: bool,
        reset: bool,
    ) -> Result<(), StateError> {
        let tx = self
            .command_tx
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .ok_or_else(StateError::unavailable)?;
        let uuid = Uuid::parse_str(player_uuid)
            .map_err(|_| StateError::invalid(format!("invalid player uuid: {player_uuid}")))?;
        let cmd = if clear || reset {
            crate::game::plugin_types::PlayerCommand::ClearTitle {
                player: uuid,
                reset,
            }
        } else {
            crate::game::plugin_types::PlayerCommand::Title {
                player: uuid,
                title: title.map(|t| json!({"text": t}).to_string()),
                subtitle: subtitle.map(|s| json!({"text": s}).to_string()),
                fade_in,
                stay,
                fade_out,
            }
        };
        tx.send(cmd).map_err(|_| StateError::unavailable())
    }

    fn send_player_action_bar(&self, player_uuid: &str, text: &str) -> Result<(), StateError> {
        let tx = self
            .command_tx
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .ok_or_else(StateError::unavailable)?;
        let uuid = Uuid::parse_str(player_uuid)
            .map_err(|_| StateError::invalid(format!("invalid player uuid: {player_uuid}")))?;
        tx.send(crate::game::plugin_types::PlayerCommand::ActionBar {
            player: uuid,
            component_json: json!({"text": text}).to_string(),
        })
        .map_err(|_| StateError::unavailable())
    }

    fn play_player_sound(
        &self,
        player_uuid: &str,
        sound: &str,
        category: i32,
        x: f64,
        y: f64,
        z: f64,
        volume: f32,
        pitch: f32,
        seed: i64,
    ) -> Result<(), StateError> {
        let tx = self
            .command_tx
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .ok_or_else(StateError::unavailable)?;
        let uuid = Uuid::parse_str(player_uuid)
            .map_err(|_| StateError::invalid(format!("invalid player uuid: {player_uuid}")))?;
        let sound_category = match category {
            1 => crate::game::plugin_types::SoundCategory::Music,
            2 => crate::game::plugin_types::SoundCategory::Records,
            3 => crate::game::plugin_types::SoundCategory::Weather,
            4 => crate::game::plugin_types::SoundCategory::Blocks,
            5 => crate::game::plugin_types::SoundCategory::Hostile,
            6 => crate::game::plugin_types::SoundCategory::Neutral,
            7 => crate::game::plugin_types::SoundCategory::Players,
            8 => crate::game::plugin_types::SoundCategory::Ambient,
            9 => crate::game::plugin_types::SoundCategory::Voice,
            10 => crate::game::plugin_types::SoundCategory::Ui,
            _ => crate::game::plugin_types::SoundCategory::Master,
        };
        tx.send(crate::game::plugin_types::PlayerCommand::PlaySound {
            player: uuid,
            world: crate::game::plugin_types::default_world_id(),
            sound: sound.to_owned(),
            category: sound_category,
            x,
            y,
            z,
            volume,
            pitch,
            seed: Some(seed),
        })
        .map_err(|_| StateError::unavailable())
    }

    fn supported_v3_ops(&self) -> Vec<&'static str> {
        vec![
            "world.chunk_status",
            "world.loaded_chunks",
            "world.entities_snapshot",
            "world.biome_get",
            "entity.equipment_set_async",
            "entity.drop_chance_set_async",
            "plugin_message.send",
            "player.title_send",
            "player.action_bar_send",
            "player.sound_play",
        ]
    }

    fn tick(&self) {
        self.drain();
    }

    fn shutdown(&self) {
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        self.pending_inventory
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        self.pending_equipment
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        self.pending_drop_chances
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn provider() -> (WorldStateProvider, Arc<AnvilChunkSource>) {
        let source = AnvilChunkSource::new_memory();
        assert!(source.ensure_chunk_loaded(0, 0, Duration::from_secs(10)));
        let dimensions =
            DimensionRegistry::with_source(Dimension::Overworld, Arc::clone(&source), [0, 65, 0]);
        (WorldStateProvider::new(dimensions), source)
    }

    #[tokio::test]
    async fn reads_real_source_and_applies_async_write_only_on_tick() {
        let (provider, source) = provider();
        let before = provider
            .block_get(&BlockGetRequest {
                world: "minecraft:overworld".into(),
                x: 0,
                y: 64,
                z: 0,
            })
            .expect("loaded block read");
        assert!(before.loaded);

        let stone = u32::from(ferrite_data::blocks::default_state("minecraft:stone").unwrap());
        let accepted = provider
            .block_set_async(&BlockSetRequest {
                world: "minecraft:overworld".into(),
                x: 0,
                y: 64,
                z: 0,
                state_id: stone,
                expected_state_id: Some(before.state_id),
            })
            .expect("write accepted");
        assert!(accepted.queued);
        assert_eq!(accepted.request_id, 1);
        assert_ne!(
            source.block(0, 64, 0),
            Some(stone),
            "acceptance is not application"
        );

        provider.tick();
        assert_eq!(source.block(0, 64, 0), Some(stone));
    }

    #[tokio::test]
    async fn unknown_world_state_and_stale_guard_are_explicit() {
        let (provider, _) = provider();
        assert_eq!(
            provider
                .block_get(&BlockGetRequest {
                    world: "minecraft:nowhere".into(),
                    x: 0,
                    y: 64,
                    z: 0,
                })
                .unwrap_err()
                .code,
            "unknown_world"
        );
        assert_eq!(
            provider
                .block_set_async(&BlockSetRequest {
                    world: "minecraft:overworld".into(),
                    x: 0,
                    y: 64,
                    z: 0,
                    state_id: 65535,
                    expected_state_id: None,
                })
                .unwrap_err()
                .code,
            "unknown_block_state"
        );
        assert_eq!(
            provider
                .block_set_async(&BlockSetRequest {
                    world: "minecraft:overworld".into(),
                    x: 0,
                    y: 64,
                    z: 0,
                    state_id: 1,
                    expected_state_id: Some(0),
                })
                .unwrap_err()
                .code,
            "stale_request"
        );
    }

    #[tokio::test]
    async fn player_inventory_get_and_async_set_applies_only_on_tick() {
        let (provider, _) = provider();
        let uuid = Uuid::new_v4();
        let mut inv = ferrite_inventory::PlayerInventory::new();
        let mut sword = ferrite_item::ItemStack::from_name("minecraft:diamond_sword").unwrap();
        sword.count = 1;
        sword.components.insert(
            ferrite_item::component_ids::DAMAGE,
            ferrite_item::Component::Damage(12),
        );
        inv.set(0, sword);
        inv.selected = 0;
        provider.register_player_inventory(uuid, inv);

        let snap = provider
            .player_inventory_get(&uuid.to_string())
            .expect("inventory snapshot");
        assert_eq!(snap.held_slot, 0);
        let slot0 = snap.slots.get("0").expect("slot 0 present");
        assert_eq!(slot0.id, "minecraft:diamond_sword");
        assert_eq!(slot0.count, 1);
        assert!(slot0.nbt_base64.is_some());

        let mut next_slots = BTreeMap::new();
        next_slots.insert("0".to_string(), None);
        next_slots.insert(
            "40".to_string(),
            Some(InventorySlotPatch {
                id: "minecraft:shield".to_string(),
                count: 1,
                nbt_base64: slot0.nbt_base64.as_ref().map(|_| {
                    let mut apple = ferrite_item::ItemStack::from_name("minecraft:shield").unwrap();
                    apple.components.insert(
                        ferrite_item::component_ids::UNBREAKABLE,
                        ferrite_item::Component::Unbreakable,
                    );
                    let nbt = apple.to_nbt().unwrap();
                    let bytes =
                        ferrite_nbt::binary::write_named("", &ferrite_nbt::Tag::Compound(nbt))
                            .unwrap();
                    base64::engine::general_purpose::STANDARD.encode(bytes)
                }),
            }),
        );
        let accepted = provider
            .player_inventory_set_async(&PlayerInventorySetRequest {
                uuid: uuid.to_string(),
                held_slot: Some(3),
                slots: next_slots,
            })
            .expect("inventory write accepted");
        assert!(accepted.queued);

        // Acceptance is not application.
        let before_tick = provider.player_inventory_get(&uuid.to_string()).unwrap();
        assert_eq!(before_tick.held_slot, 0);
        assert!(before_tick.slots.contains_key("0"));
        assert!(!before_tick.slots.contains_key("40"));

        provider.tick();

        let after_tick = provider.player_inventory_get(&uuid.to_string()).unwrap();
        assert_eq!(after_tick.held_slot, 3);
        assert!(!after_tick.slots.contains_key("0"));
        let offhand = after_tick.slots.get("40").expect("offhand populated");
        assert_eq!(offhand.id, "minecraft:shield");
        assert!(offhand.nbt_base64.is_some());
    }

    #[tokio::test]
    async fn player_inventory_errors_are_explicit() {
        let (provider, _) = provider();
        let missing = Uuid::new_v4().to_string();
        assert_eq!(
            provider.player_inventory_get(&missing).unwrap_err().code,
            "player_not_found"
        );

        let uuid = Uuid::new_v4();
        provider.register_player_inventory(uuid, ferrite_inventory::PlayerInventory::new());

        let mut bad_item = BTreeMap::new();
        bad_item.insert(
            "0".to_string(),
            Some(InventorySlotPatch {
                id: "minecraft:nonexistent_item_26_3".to_string(),
                count: 1,
                nbt_base64: None,
            }),
        );
        assert_eq!(
            provider
                .player_inventory_set_async(&PlayerInventorySetRequest {
                    uuid: uuid.to_string(),
                    held_slot: None,
                    slots: bad_item,
                })
                .unwrap_err()
                .code,
            "unknown_item"
        );

        let mut bad_slot = BTreeMap::new();
        bad_slot.insert("99".to_string(), None);
        assert_eq!(
            provider
                .player_inventory_set_async(&PlayerInventorySetRequest {
                    uuid: uuid.to_string(),
                    held_slot: None,
                    slots: bad_slot,
                })
                .unwrap_err()
                .code,
            "invalid_request"
        );

        assert_eq!(
            provider
                .player_inventory_set_async(&PlayerInventorySetRequest {
                    uuid: uuid.to_string(),
                    held_slot: Some(9),
                    slots: BTreeMap::new(),
                })
                .unwrap_err()
                .code,
            "invalid_request"
        );
    }

    #[tokio::test]
    async fn inventory_sync_callback_fires_on_drain_and_cleanup_removes_player() {
        let (provider, _) = provider();
        let uuid = Uuid::new_v4();
        provider.register_player_inventory(uuid, ferrite_inventory::PlayerInventory::new());

        let synced = Arc::new(Mutex::new(Vec::new()));
        let synced_clone = Arc::clone(&synced);
        provider.set_inventory_sync(Arc::new(move |id, inv| {
            synced_clone.lock().unwrap().push((id, inv));
        }));

        let mut slots = BTreeMap::new();
        slots.insert(
            "36".to_string(),
            Some(InventorySlotPatch {
                id: "minecraft:iron_boots".into(),
                count: 1,
                nbt_base64: None,
            }),
        );
        slots.insert(
            "40".to_string(),
            Some(InventorySlotPatch {
                id: "minecraft:shield".into(),
                count: 1,
                nbt_base64: None,
            }),
        );
        provider
            .player_inventory_set_async(&PlayerInventorySetRequest {
                uuid: uuid.to_string(),
                held_slot: Some(4),
                slots,
            })
            .unwrap();

        assert!(synced.lock().unwrap().is_empty());
        provider.tick();

        let guard = synced.lock().unwrap();
        assert_eq!(guard.len(), 1);
        assert_eq!(guard[0].0, uuid);
        assert_eq!(guard[0].1.selected, 4);
        assert_eq!(guard[0].1.get(36).name(), Some("minecraft:iron_boots"));
        assert_eq!(guard[0].1.get(40).name(), Some("minecraft:shield"));
        drop(guard);

        provider.remove_player_inventory(uuid);
        assert_eq!(
            provider
                .player_inventory_get(&uuid.to_string())
                .unwrap_err()
                .code,
            "player_not_found"
        );
    }

    #[tokio::test]
    async fn player_inventory_nbt_base64_round_trips_and_rejects_corrupt_nbt() {
        let (provider, _) = provider();
        let uuid = Uuid::new_v4();
        provider.register_player_inventory(uuid, ferrite_inventory::PlayerInventory::new());

        let mut stack = ferrite_item::ItemStack::from_name("minecraft:diamond_sword").unwrap();
        stack.components.insert(
            ferrite_item::component_ids::DAMAGE,
            ferrite_item::Component::Damage(7),
        );
        stack.components.insert(
            ferrite_item::component_ids::UNBREAKABLE,
            ferrite_item::Component::Unbreakable,
        );
        let compound = stack.to_nbt().unwrap();
        let bytes =
            ferrite_nbt::binary::write_named("", &ferrite_nbt::Tag::Compound(compound)).unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);

        let mut slots = BTreeMap::new();
        slots.insert(
            "0".to_string(),
            Some(InventorySlotPatch {
                id: "minecraft:diamond_sword".into(),
                count: 1,
                nbt_base64: Some(encoded),
            }),
        );
        provider
            .player_inventory_set_async(&PlayerInventorySetRequest {
                uuid: uuid.to_string(),
                held_slot: Some(0),
                slots,
            })
            .unwrap();
        provider.tick();

        let snap = provider.player_inventory_get(&uuid.to_string()).unwrap();
        let patch = snap.slots.get("0").unwrap();
        assert_eq!(patch.id, "minecraft:diamond_sword");
        assert!(patch.nbt_base64.is_some());

        let mut bad = BTreeMap::new();
        bad.insert(
            "0".to_string(),
            Some(InventorySlotPatch {
                id: "minecraft:diamond_sword".into(),
                count: 1,
                nbt_base64: Some("not-valid-base64!!!".into()),
            }),
        );
        assert_eq!(
            provider
                .player_inventory_set_async(&PlayerInventorySetRequest {
                    uuid: uuid.to_string(),
                    held_slot: None,
                    slots: bad,
                })
                .unwrap_err()
                .code,
            "invalid_request"
        );
    }

    #[tokio::test]
    async fn player_state_get_returns_live_snapshot_and_rejects_offline_or_malformed() {
        let (provider, _source) = provider();
        assert!(provider.supported_ops().contains(&"player.state_get"));
        assert!(provider.supported_ops().contains(&"entity.state_get"));

        let offline = Uuid::new_v4();
        assert_eq!(
            provider
                .player_state_get(&offline.to_string())
                .unwrap_err()
                .code,
            "player_not_found"
        );
        assert_eq!(
            provider.player_state_get("not-a-uuid").unwrap_err().code,
            "invalid_request"
        );

        let uuid = Uuid::new_v4();
        provider.register_player_state(LivePlayerState {
            uuid,
            name: "Alice".into(),
            world: "minecraft:overworld".into(),
            x: 10.5,
            y: 64.0,
            z: -3.25,
            yaw: 90.0,
            pitch: 15.0,
            vx: 0.0,
            vy: 0.0,
            vz: 0.0,
            game_mode: "CREATIVE".into(),
            health: 20.0,
            food_level: 20,
            network_id: 42,
        });

        provider.update_player_location(
            uuid,
            Some("minecraft:the_nether"),
            [1.0, 70.0, 2.0],
            45.0,
            -5.0,
        );
        provider.update_player_game_mode(uuid, "SURVIVAL");
        provider.update_player_health(uuid, 14.5);
        provider.update_player_food(uuid, 16);

        let snap = provider.player_state_get(&uuid.to_string()).unwrap();
        assert_eq!(snap["uuid"], uuid.to_string());
        assert_eq!(snap["type"], "minecraft:player");
        assert_eq!(snap["name"], "Alice");
        assert_eq!(snap["world"], "minecraft:the_nether");
        assert_eq!(snap["x"], 1.0);
        assert_eq!(snap["y"], 70.0);
        assert_eq!(snap["z"], 2.0);
        assert_eq!(snap["game_mode"], "SURVIVAL");
        assert_eq!(snap["health"], 14.5);
        assert_eq!(snap["food_level"], 16);

        // Players are also queryable via entity.state_get.
        let as_entity = provider.entity_state_get(&uuid.to_string()).unwrap();
        assert_eq!(as_entity["type"], "minecraft:player");
        assert_eq!(as_entity["name"], "Alice");

        provider.remove_player_state(uuid);
        assert_eq!(
            provider
                .player_state_get(&uuid.to_string())
                .unwrap_err()
                .code,
            "player_not_found"
        );
    }

    #[tokio::test]
    async fn entity_state_get_reads_entity_index_and_rejects_missing_or_malformed() {
        let (provider, _source) = provider();
        let missing = Uuid::new_v4();
        assert_eq!(
            provider
                .entity_state_get(&missing.to_string())
                .unwrap_err()
                .code,
            "entity_not_found"
        );
        assert_eq!(
            provider.entity_state_get("bad-uuid").unwrap_err().code,
            "invalid_request"
        );

        let index = Arc::new(Mutex::new(EntityIndex::default()));
        let pig_uuid = Uuid::new_v4();
        let pig_kind = ferrite_entity::mob::MobKind::Pig.entity_type();
        index
            .lock()
            .unwrap()
            .insert(crate::game::entity_index::CommandEntity {
                uuid: pig_uuid,
                network_id: 77,
                kind: pig_kind,
                pos: [5.5, 65.0, -8.5],
                yaw: 180.0,
                pitch: 0.0,
                dimension: Dimension::Overworld,
                tags: Default::default(),
                custom_name: Some("Wilbur".into()),
                health: Some(10.0),
                attacker: None,
                owner: None,
                leasher: None,
                target: None,
            });
        provider.set_entity_index(index);

        let snap = provider.entity_state_get(&pig_uuid.to_string()).unwrap();
        assert_eq!(snap["uuid"], pig_uuid.to_string());
        assert_eq!(snap["type"], "minecraft:pig");
        assert_eq!(snap["name"], "Wilbur");
        assert_eq!(snap["custom_name"], "Wilbur");
        assert_eq!(snap["world"], "minecraft:overworld");
        assert_eq!(snap["x"], 5.5);
        assert_eq!(snap["y"], 65.0);
        assert_eq!(snap["z"], -8.5);
        assert_eq!(snap["health"], 10.0);
        assert_eq!(snap["network_id"], 77);
    }

    #[tokio::test]
    async fn v3_chunk_status_loaded_chunks_and_entities_snapshot_are_bounded_and_non_mutating() {
        let (provider, source) = provider();
        assert_eq!(
            provider.supported_v3_ops(),
            vec![
                "world.chunk_status",
                "world.loaded_chunks",
                "world.entities_snapshot",
                "world.biome_get",
                "entity.equipment_set_async",
                "entity.drop_chance_set_async",
                "plugin_message.send",
                "player.title_send",
                "player.action_bar_send",
                "player.sound_play",
            ]
        );

        // Unloaded chunk is not loaded or generated as a side effect of chunk_status.
        let before = provider.chunk_status("minecraft:overworld", 4, -5).unwrap();
        assert!(!before.loaded);
        assert!(!before.generated);
        assert!(!before.in_use);
        assert!(!source.chunk_resident(4, -5));

        // Load two chunks and place a player in chunk (0, 0).
        assert!(source.ensure_chunk_loaded(1, 2, std::time::Duration::from_secs(10)));
        let loaded_status = provider.chunk_status("minecraft:overworld", 0, 0).unwrap();
        assert!(loaded_status.loaded);
        assert!(loaded_status.generated);
        assert!(!loaded_status.in_use);

        let player_uuid = Uuid::new_v4();
        provider.register_player_snapshot(
            player_uuid,
            "Viewer".into(),
            "minecraft:overworld".into(),
            [4.0, 65.0, 4.0],
            0.0,
            0.0,
            "SURVIVAL",
            20.0,
            20,
            10,
        );
        let in_use_status = provider.chunk_status("minecraft:overworld", 0, 0).unwrap();
        assert!(in_use_status.in_use);

        // loaded_chunks respects limit and reports truncation.
        let full_chunks = provider.loaded_chunks("minecraft:overworld", None).unwrap();
        assert_eq!(full_chunks.total, 2);
        assert!(!full_chunks.truncated);
        let limited_chunks = provider
            .loaded_chunks("minecraft:overworld", Some(1))
            .unwrap();
        assert_eq!(limited_chunks.total, 2);
        assert_eq!(limited_chunks.chunks.len(), 1);
        assert!(limited_chunks.truncated);

        // entities_snapshot merges players and EntityIndex entries per world with bounds.
        let index = Arc::new(Mutex::new(EntityIndex::default()));
        let pig_uuid = Uuid::new_v4();
        index
            .lock()
            .unwrap()
            .insert(crate::game::entity_index::CommandEntity {
                uuid: pig_uuid,
                network_id: 77,
                kind: ferrite_entity::mob::MobKind::Pig.entity_type(),
                pos: [5.5, 65.0, 5.5],
                yaw: 0.0,
                pitch: 0.0,
                dimension: Dimension::Overworld,
                tags: Default::default(),
                custom_name: Some("Wilbur".into()),
                health: Some(10.0),
                attacker: None,
                owner: None,
                leasher: None,
                target: None,
            });
        provider.set_entity_index(index);

        let entities = provider
            .entities_snapshot("minecraft:overworld", None)
            .unwrap();
        assert_eq!(entities.total, 2);
        assert!(!entities.truncated);
        assert!(
            entities
                .entities
                .iter()
                .any(|e| e.uuid == player_uuid.to_string())
        );
        assert!(
            entities
                .entities
                .iter()
                .any(|e| e.uuid == pig_uuid.to_string() && e.type_key == "minecraft:pig")
        );

        let nether_entities = provider
            .entities_snapshot("minecraft:the_nether", None)
            .unwrap();
        assert_eq!(nether_entities.total, 0);

        let limited_entities = provider
            .entities_snapshot("minecraft:overworld", Some(1))
            .unwrap();
        assert_eq!(limited_entities.total, 2);
        assert_eq!(limited_entities.entities.len(), 1);
        assert!(limited_entities.truncated);

        // Unknown world and invalid limit=0 errors.
        assert_eq!(
            provider
                .chunk_status("minecraft:missing", 0, 0)
                .unwrap_err()
                .code,
            "unknown_world"
        );
        assert_eq!(
            provider
                .loaded_chunks("minecraft:overworld", Some(0))
                .unwrap_err()
                .code,
            "invalid_request"
        );
        assert_eq!(
            provider
                .entities_snapshot("minecraft:overworld", Some(0))
                .unwrap_err()
                .code,
            "invalid_request"
        );
    }

    #[tokio::test]
    async fn biome_get_reads_resident_chunk_and_refuses_unloaded_or_unknown_world() {
        let (provider, source) = provider();
        assert!(provider.supported_v3_ops().contains(&"world.biome_get"));

        // Resident chunk (0, 0) returns its biome without side effects.
        let snap = provider.biome_get("minecraft:overworld", 2, 64, 3).unwrap();
        assert_eq!(snap.world, "minecraft:overworld");
        assert_eq!(snap.x, 2);
        assert_eq!(snap.y, 64);
        assert_eq!(snap.z, 3);
        assert_eq!(snap.biome, "minecraft:plains");

        // Unloaded chunk (7, -9) fails explicitly and is not loaded or generated.
        let err = provider
            .biome_get("minecraft:overworld", 7 * 16 + 1, 64, -9 * 16 + 2)
            .unwrap_err();
        assert_eq!(err.code, "chunk_not_loaded");
        assert!(!source.chunk_resident(7, -9));

        // Unknown world fails explicitly.
        assert_eq!(
            provider
                .biome_get("minecraft:missing", 0, 64, 0)
                .unwrap_err()
                .code,
            "unknown_world"
        );
    }

    #[tokio::test]
    async fn entity_equipment_set_async_validates_drains_and_reads_via_entity_state_get() {
        let (provider, _source) = provider();
        assert!(
            provider
                .supported_v3_ops()
                .contains(&"entity.equipment_set_async")
        );

        let missing = Uuid::new_v4();
        assert_eq!(
            provider
                .entity_equipment_set_async(&EntityEquipmentSetRequest {
                    uuid: missing.to_string(),
                    slot: "HEAD".into(),
                    item: None,
                })
                .unwrap_err()
                .code,
            "entity_not_found"
        );

        let index = Arc::new(Mutex::new(EntityIndex::default()));
        let mut store = ferrite_entity::EntityStore::default();
        let mut zombie = ferrite_entity::mob::new_mob(
            ferrite_entity::mob::MobKind::Zombie,
            ferrite_math::Vec3::new(2.5, 64.0, 2.5),
            &mut ferrite_entity::Rng::default(),
        );
        let zombie_uuid = zombie.data.uuid;
        let helmet = ferrite_item::ItemStack::from_name("minecraft:iron_helmet").unwrap();
        zombie
            .data
            .equipment
            .set(ferrite_entity::EquipmentSlot::Head, Some(helmet));
        store.insert(zombie);
        let mut owned = std::collections::HashSet::new();
        index
            .lock()
            .unwrap()
            .sync(&mut owned, &store, Dimension::Overworld);
        provider.set_entity_index(index);

        let state = provider.entity_state_get(&zombie_uuid.to_string()).unwrap();
        assert_eq!(state["equipment"]["HEAD"]["id"], "minecraft:iron_helmet");
        assert_eq!(state["equipment"]["HEAD"]["count"], 1);

        assert_eq!(
            provider
                .entity_equipment_set_async(&EntityEquipmentSetRequest {
                    uuid: zombie_uuid.to_string(),
                    slot: "INVALID_SLOT".into(),
                    item: None,
                })
                .unwrap_err()
                .code,
            "invalid_request"
        );

        assert_eq!(
            provider
                .entity_equipment_set_async(&EntityEquipmentSetRequest {
                    uuid: zombie_uuid.to_string(),
                    slot: "HAND".into(),
                    item: Some(InventorySlotPatch {
                        id: "minecraft:not_an_item".into(),
                        count: 1,
                        nbt_base64: None,
                    }),
                })
                .unwrap_err()
                .code,
            "unknown_item"
        );

        let routed = Arc::new(Mutex::new(Vec::new()));
        let routed_sink = Arc::clone(&routed);
        provider.set_entity_equipment_sync(Arc::new(move |uuid, slot, item| {
            routed_sink.lock().unwrap().push((uuid, slot, item));
        }));

        let accepted = provider
            .entity_equipment_set_async(&EntityEquipmentSetRequest {
                uuid: zombie_uuid.to_string(),
                slot: "HAND".into(),
                item: Some(InventorySlotPatch {
                    id: "minecraft:diamond_sword".into(),
                    count: 1,
                    nbt_base64: None,
                }),
            })
            .unwrap();
        assert!(accepted.queued);
        provider.tick();

        let drained = routed.lock().unwrap();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].0, zombie_uuid);
        assert_eq!(drained[0].1, ferrite_entity::EquipmentSlot::MainHand);
        assert_eq!(
            drained[0].2.as_ref().and_then(|s| s.name()),
            Some("minecraft:diamond_sword")
        );
    }

    #[tokio::test]
    async fn entity_drop_chance_set_async_validates_drains_and_reads_via_entity_state_get() {
        let (provider, _source) = provider();
        assert!(
            provider
                .supported_v3_ops()
                .contains(&"entity.drop_chance_set_async")
        );

        let missing = Uuid::new_v4();
        assert_eq!(
            provider
                .entity_drop_chance_set_async(&EntityDropChanceSetRequest {
                    uuid: missing.to_string(),
                    slot: "HEAD".into(),
                    chance: 0.5,
                })
                .unwrap_err()
                .code,
            "entity_not_found"
        );

        let index = Arc::new(Mutex::new(EntityIndex::default()));
        let mut store = ferrite_entity::EntityStore::default();
        let mut zombie = ferrite_entity::mob::new_mob(
            ferrite_entity::mob::MobKind::Zombie,
            ferrite_math::Vec3::new(2.5, 64.0, 2.5),
            &mut ferrite_entity::Rng::default(),
        );
        let zombie_uuid = zombie.data.uuid;
        zombie
            .data
            .equipment
            .set_drop_chance(ferrite_entity::EquipmentSlot::Body, 0.5);
        store.insert(zombie);
        let mut owned = std::collections::HashSet::new();
        index
            .lock()
            .unwrap()
            .sync(&mut owned, &store, Dimension::Overworld);
        provider.set_entity_index(index);

        let state = provider.entity_state_get(&zombie_uuid.to_string()).unwrap();
        assert_eq!(state["drop_chances"]["BODY"], 0.5);

        assert_eq!(
            provider
                .entity_drop_chance_set_async(&EntityDropChanceSetRequest {
                    uuid: zombie_uuid.to_string(),
                    slot: "HEAD".into(),
                    chance: f32::NAN,
                })
                .unwrap_err()
                .code,
            "invalid_request"
        );

        let routed = Arc::new(Mutex::new(Vec::new()));
        let routed_ref = Arc::clone(&routed);
        provider.set_entity_drop_chance_sync(Arc::new(move |uuid, slot, chance| {
            routed_ref.lock().unwrap().push((uuid, slot, chance));
        }));

        let accepted = provider
            .entity_drop_chance_set_async(&EntityDropChanceSetRequest {
                uuid: zombie_uuid.to_string(),
                slot: "HEAD".into(),
                chance: 1.0,
            })
            .unwrap();
        assert!(accepted.queued);
        provider.tick();

        let drained = routed.lock().unwrap();
        assert_eq!(drained.len(), 1);
        assert_eq!(
            drained[0],
            (zombie_uuid, ferrite_entity::EquipmentSlot::Head, 1.0)
        );
    }
}
