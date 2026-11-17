//! Server-owned v2 block state seam.
//!
//! Reads use the dimension's real [`AnvilChunkSource`]; they never generate a chunk. Writes are
//! validated and queued here, then applied by [`WorldStateProvider::tick`] from the game-loop
//! tick. `world.block_set_async` returning `queued: true` therefore means exactly that: the
//! write was accepted, not applied.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use crate::game::{AnvilChunkSource, Dimension, DimensionRegistry};
use base64::Engine as _;
use ferrite_plugin_host::{
    BlockGetRequest, BlockSetAccepted, BlockSetRequest, BlockSnapshot, HostStateProvider,
    InventorySetAccepted, InventorySlotPatch, PlayerInventorySetRequest, PlayerInventorySnapshot,
    StateError,
};
use uuid::Uuid;

/// Maximum accepted-but-not-yet-applied block writes. Overflow is a hard `queue_full` error.
pub const MAX_PENDING_BLOCK_WRITES: usize = 4096;

/// Maximum accepted-but-not-yet-applied inventory writes. Overflow is a hard `queue_full` error.
pub const MAX_PENDING_INVENTORY_WRITES: usize = 4096;

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

/// Callback invoked after `WorldStateProvider::drain` applies a queued inventory write.
pub type InventorySyncFn = Arc<dyn Fn(Uuid, ferrite_inventory::PlayerInventory) + Send + Sync>;

/// Adapter from the contract-v2 provider seam to Ferrite's actual dimension sources.
pub struct WorldStateProvider {
    dimensions: Arc<DimensionRegistry>,
    inventories: PlayerInventoryRegistry,
    on_inventory_applied: Mutex<Option<InventorySyncFn>>,
    pending: Mutex<VecDeque<QueuedWrite>>,
    pending_inventory: Mutex<VecDeque<QueuedInventoryWrite>>,
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
            on_inventory_applied: Mutex::new(None),
            pending: Mutex::new(VecDeque::new()),
            pending_inventory: Mutex::new(VecDeque::new()),
            next_request: AtomicU64::new(0),
        }
    }

    /// Installs a region-routing callback invoked whenever `drain` applies a queued inventory write.
    pub fn set_inventory_sync(&self, sync: InventorySyncFn) {
        *self
            .on_inventory_applied
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

    fn supported_ops(&self) -> Vec<&'static str> {
        vec![
            "world.block_get",
            "world.block_set_async",
            "world.chunk_loaded",
            "player.inventory_get",
            "player.inventory_set_async",
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
}
