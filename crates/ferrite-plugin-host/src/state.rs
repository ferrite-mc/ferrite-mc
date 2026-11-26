//! Contract v2 state types and the server-owned adapter seam.
//!
//! Lane 1 owns parsing, validation, lifecycle and the async acceptance boundary. The actual
//! world/region implementation is supplied by the server through [`HostStateProvider`]. The
//! deterministic fake in the tests is deliberately not a world: it only proves the transport
//! and validation path.
//!
//! Reads may return a point-in-time snapshot from any thread that can safely read the source.
//! Writes are validated and queued; `queued = true` never means the block was applied.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// RPC ops reserved by contract v2.
pub const V2_OPS: [&str; 7] = [
    "world.block_get",
    "world.block_set_async",
    "world.chunk_loaded",
    "player.state_get",
    "player.inventory_get",
    "player.inventory_set_async",
    "entity.state_get",
];

/// Provider ops added in contract v3.
pub const V3_OPS: [&str; 7] = [
    "world.chunk_status",
    "world.loaded_chunks",
    "world.entities_snapshot",
    "world.biome_get",
    "entity.equipment_set_async",
    "entity.drop_chance_set_async",
    "plugin_message.send",
];

/// A provider-level failure with a stable contract error code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateError {
    /// Stable code returned inside the v2 error envelope.
    pub code: String,
    /// Human-readable detail for logs and Java callers.
    pub message: String,
}

impl StateError {
    /// Builds one provider error.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    /// `invalid_request`: malformed coordinates or a field outside its range.
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid_request", message)
    }

    /// `unknown_world`: the world key is not known to this server.
    pub fn unknown_world(world: &str) -> Self {
        Self::new("unknown_world", format!("unknown world '{world}'"))
    }

    /// `unknown_block_state`: the registry has no such state id.
    pub fn unknown_block_state(id: u32) -> Self {
        Self::new(
            "unknown_block_state",
            format!("block state id {id} is not in the 26.3 registry"),
        )
    }

    /// `chunk_not_loaded`: a read or queued write needs a resident chunk.
    pub fn chunk_not_loaded(x: i32, z: i32) -> Self {
        Self::new(
            "chunk_not_loaded",
            format!("chunk ({x}, {z}) is not loaded"),
        )
    }

    /// `stale_request`: compare-and-set expected a different current state.
    pub fn stale_request(expected: u32, actual: u32) -> Self {
        Self::new(
            "stale_request",
            format!("expected block state {expected}, found {actual}"),
        )
    }

    /// `queue_full`: the owner queue rejected the write instead of dropping it.
    pub fn queue_full() -> Self {
        Self::new("queue_full", "the block write queue is full")
    }

    /// `state_unavailable`: no server state adapter is installed yet.
    pub fn unavailable() -> Self {
        Self::new(
            "state_unavailable",
            "no server state provider is installed; the world may not be ready",
        )
    }

    /// `unsupported`: this provider intentionally does not expose the op yet.
    pub fn unsupported(op: &str) -> Self {
        Self::new(
            "unsupported",
            format!("{op} is not implemented by the installed state provider"),
        )
    }

    /// `player_not_found`: no live player exists for the requested UUID.
    pub fn player_not_found(uuid: &str) -> Self {
        Self::new("player_not_found", format!("unknown player '{uuid}'"))
    }

    /// `unknown_item`: item id is not in the 26.3 registry.
    pub fn unknown_item(item: &str) -> Self {
        Self::new(
            "unknown_item",
            format!("item '{item}' is not in the 26.3 registry"),
        )
    }
}

impl fmt::Display for StateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for StateError {}

impl From<StateError> for crate::protocol::RpcError {
    fn from(error: StateError) -> Self {
        Self::new(&error.code, error.message)
    }
}

/// Request of `world.block_get`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BlockGetRequest {
    /// Namespaced world key, for example `minecraft:overworld`.
    pub world: String,
    /// Block X.
    pub x: i32,
    /// Block Y.
    pub y: i32,
    /// Block Z.
    pub z: i32,
}

/// Immutable read result of `world.block_get`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockSnapshot {
    /// Namespaced world key.
    pub world: String,
    /// Block X.
    pub x: i32,
    /// Block Y.
    pub y: i32,
    /// Block Z.
    pub z: i32,
    /// Global block-state id in the 26.3 registry.
    pub state_id: u32,
    /// Namespaced material key.
    pub material: String,
    /// Vanilla state properties, for display/debugging only.
    pub properties: BTreeMap<String, String>,
    /// Always `true` on success; a missing chunk is an error instead.
    pub loaded: bool,
}

/// Request of `world.block_set_async`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BlockSetRequest {
    /// Namespaced world key.
    pub world: String,
    /// Block X.
    pub x: i32,
    /// Block Y.
    pub y: i32,
    /// Block Z.
    pub z: i32,
    /// Target global block-state id.
    pub state_id: u32,
    /// Optional compare-and-set guard against the current state.
    #[serde(default)]
    pub expected_state_id: Option<u32>,
}

/// Acceptance result of `world.block_set_async`.
///
/// This is not an application result. The server applies the queued write on a later game-loop
/// tick; a future status op (not part of this slice) will expose the final outcome.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BlockSetAccepted {
    /// Always `true` for a successfully enqueued write.
    pub queued: bool,
    /// Monotonic request id for logs and a future completion query.
    pub request_id: u64,
}

/// Request shared by the player/entity snapshot candidates.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UuidRequest {
    /// Player or entity UUID as lowercase hyphenated text.
    pub uuid: String,
}

/// Wire representation of one inventory slot for `player.inventory_get` / `player.inventory_set_async`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InventorySlotPatch {
    /// Namespaced item id, e.g. `minecraft:diamond_sword`.
    pub id: String,
    /// Stack count (`1..=99`).
    pub count: i32,
    /// Optional base64-encoded named NBT save compound (`ferrite-item` 26.3 save format).
    #[serde(default)]
    pub nbt_base64: Option<String>,
}

/// Point-in-time snapshot returned by `player.inventory_get`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlayerInventorySnapshot {
    /// Player UUID as lowercase hyphenated text.
    pub uuid: String,
    /// Selected hotbar slot (`0..=8`).
    pub held_slot: u8,
    /// Non-empty slots keyed by slot index string (`"0"`..`"40"`).
    pub slots: BTreeMap<String, InventorySlotPatch>,
}

/// Request of `player.inventory_set_async`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlayerInventorySetRequest {
    /// Player UUID as lowercase hyphenated text.
    pub uuid: String,
    /// Optional new selected hotbar slot (`0..=8`).
    #[serde(default)]
    pub held_slot: Option<u8>,
    /// Slot updates keyed by slot index string (`"0"`..`"40"`); `None` clears the slot.
    #[serde(default)]
    pub slots: BTreeMap<String, Option<InventorySlotPatch>>,
}

/// Acceptance result of `player.inventory_set_async`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InventorySetAccepted {
    /// Always `true` for a successfully enqueued write.
    pub queued: bool,
    /// Monotonic request id for logs and tracing.
    pub request_id: u64,
}

/// Maximum plugin channel name length (matches Bukkit `Messenger::MAX_CHANNEL_SIZE`).
pub const MAX_PLUGIN_CHANNEL_LEN: usize = 256;

/// Maximum outbound plugin message payload bytes (matches Bukkit `Messenger::MAX_MESSAGE_SIZE`).
pub const MAX_PLUGIN_MESSAGE_BYTES: usize = 1_048_576;

/// Maximum inbound (serverbound play custom payload) bytes (matches vanilla 26.3 limit).
pub const MAX_SERVERBOUND_PLUGIN_MESSAGE_BYTES: usize = 32_767;

/// Read-only chunk status returned by v3 `world.chunk_status`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChunkStatusSnapshot {
    pub world: String,
    pub x: i32,
    pub z: i32,
    pub loaded: bool,
    pub generated: bool,
    pub in_use: bool,
}

/// Chunk coordinate pair in `world.loaded_chunks`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChunkCoord {
    pub x: i32,
    pub z: i32,
}

/// Bounded snapshot of loaded chunks returned by v3 `world.loaded_chunks`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoadedChunksSnapshot {
    pub world: String,
    pub chunks: Vec<ChunkCoord>,
    pub total: usize,
    pub truncated: bool,
}

/// One entity entry in v3 `world.entities_snapshot`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorldEntitySnapshotEntry {
    pub id: i32,
    pub uuid: String,
    #[serde(rename = "type")]
    pub type_key: String,
    pub name: String,
    #[serde(default)]
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
    #[serde(default)]
    pub health: Option<f32>,
}

/// Bounded snapshot of loaded entities in one world returned by v3 `world.entities_snapshot`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorldEntitiesSnapshot {
    pub world: String,
    pub entities: Vec<WorldEntitySnapshotEntry>,
    pub total: usize,
    pub truncated: bool,
}

/// Point-in-time 3D biome read returned by `world.biome_get` for a resident chunk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BiomeSnapshot {
    pub world: String,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub biome: String,
}

/// Request of `entity.equipment_set_async`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EntityEquipmentSetRequest {
    /// Target living entity UUID as lowercase hyphenated text.
    pub uuid: String,
    /// Slot name (`HAND`/`MAIN_HAND`, `OFF_HAND`, `FEET`, `LEGS`, `CHEST`, `HEAD`).
    pub slot: String,
    /// Item to equip, or `None` to clear the slot.
    #[serde(default)]
    pub item: Option<InventorySlotPatch>,
}

/// Acceptance result of `entity.equipment_set_async`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntityEquipmentSetAccepted {
    /// Always `true` for a successfully enqueued write.
    pub queued: bool,
    /// Monotonic request id for logs and tracing.
    pub request_id: u64,
}

/// Request of `entity.drop_chance_set_async`.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EntityDropChanceSetRequest {
    /// Target living entity UUID as lowercase hyphenated text.
    pub uuid: String,
    /// Slot name (`HAND`/`MAIN_HAND`, `OFF_HAND`, `FEET`, `LEGS`, `CHEST`, `HEAD`, `BODY`, `SADDLE`).
    pub slot: String,
    /// Drop chance float (finite).
    pub chance: f32,
}

/// The server-owned state adapter.
///
/// Implementations must be thread-safe. `block_get` may be called from the Java host thread;
/// `block_set_async` must validate and enqueue without waiting for a region/tick. `tick` is
/// called from the game-loop thread through [`crate::PluginHost::tick`].
pub trait HostStateProvider: Send + Sync {
    /// Point-in-time block read. Return `chunk_not_loaded`, never invented air.
    fn block_get(&self, request: &BlockGetRequest) -> Result<BlockSnapshot, StateError>;

    /// Validate and enqueue a write. Acceptance is not application.
    fn block_set_async(&self, request: &BlockSetRequest) -> Result<BlockSetAccepted, StateError>;

    /// Whether a chunk is resident. Providers that do not support this return `unsupported`.
    fn chunk_loaded(&self, world: &str, x: i32, z: i32) -> Result<bool, StateError> {
        let _ = (world, x, z);
        Err(StateError::unsupported("world.chunk_loaded"))
    }

    /// Player snapshot candidate. Not implemented in this vertical slice.
    fn player_state_get(&self, uuid: &str) -> Result<Value, StateError> {
        let _ = uuid;
        Err(StateError::unsupported("player.state_get"))
    }

    /// Point-in-time player inventory read.
    fn player_inventory_get(&self, uuid: &str) -> Result<PlayerInventorySnapshot, StateError> {
        let _ = uuid;
        Err(StateError::unsupported("player.inventory_get"))
    }

    /// Validate and enqueue a player inventory write for the owning region/loop tick.
    fn player_inventory_set_async(
        &self,
        request: &PlayerInventorySetRequest,
    ) -> Result<InventorySetAccepted, StateError> {
        let _ = request;
        Err(StateError::unsupported("player.inventory_set_async"))
    }

    /// Entity snapshot candidate. Not implemented in this vertical slice.
    fn entity_state_get(&self, uuid: &str) -> Result<Value, StateError> {
        let _ = uuid;
        Err(StateError::unsupported("entity.state_get"))
    }

    /// Sends a plugin message on a registered outgoing channel to `player` (or all online
    /// players when `player` is `None`).
    fn send_plugin_message(
        &self,
        player: Option<&str>,
        channel: &str,
        data: Vec<u8>,
    ) -> Result<(), StateError> {
        let _ = (player, channel, data);
        Err(StateError::unsupported("plugin_message.send"))
    }

    /// Ops this provider actually implements, advertised by `host.info`.
    fn supported_ops(&self) -> Vec<&'static str> {
        vec!["world.block_get", "world.block_set_async"]
    }

    /// Contract v3 read-only bulk chunk status without loading or generating.
    fn chunk_status(&self, world: &str, x: i32, z: i32) -> Result<ChunkStatusSnapshot, StateError> {
        let _ = (world, x, z);
        Err(StateError::unsupported("world.chunk_status"))
    }

    /// Contract v3 bounded snapshot of loaded chunks in one world.
    fn loaded_chunks(
        &self,
        world: &str,
        limit: Option<usize>,
    ) -> Result<LoadedChunksSnapshot, StateError> {
        let _ = (world, limit);
        Err(StateError::unsupported("world.loaded_chunks"))
    }

    /// Contract v3 bounded snapshot of loaded entities in one world.
    fn entities_snapshot(
        &self,
        world: &str,
        limit: Option<usize>,
    ) -> Result<WorldEntitiesSnapshot, StateError> {
        let _ = (world, limit);
        Err(StateError::unsupported("world.entities_snapshot"))
    }

    /// Contract v3 read-only biome read for a resident chunk without loading or generating.
    fn biome_get(&self, world: &str, x: i32, y: i32, z: i32) -> Result<BiomeSnapshot, StateError> {
        let _ = (world, x, y, z);
        Err(StateError::unsupported("world.biome_get"))
    }

    /// Validate and enqueue a living entity equipment write for the owning region tick.
    fn entity_equipment_set_async(
        &self,
        request: &EntityEquipmentSetRequest,
    ) -> Result<EntityEquipmentSetAccepted, StateError> {
        let _ = request;
        Err(StateError::unsupported("entity.equipment_set_async"))
    }

    /// Validate and enqueue a living entity equipment drop-chance write for the owning region tick.
    fn entity_drop_chance_set_async(
        &self,
        request: &EntityDropChanceSetRequest,
    ) -> Result<EntityEquipmentSetAccepted, StateError> {
        let _ = request;
        Err(StateError::unsupported("entity.drop_chance_set_async"))
    }

    /// Contract v3 ops this provider actually implements, advertised by `host.info`.
    fn supported_v3_ops(&self) -> Vec<&'static str> {
        Vec::new()
    }

    /// Sends a title/subtitle or clear-title packet to one online player.
    #[allow(clippy::too_many_arguments)]
    fn send_player_title(
        &self,
        _player_uuid: &str,
        _title: Option<String>,
        _subtitle: Option<String>,
        _fade_in: i32,
        _stay: i32,
        _fade_out: i32,
        _clear: bool,
        _reset: bool,
    ) -> Result<(), StateError> {
        Err(StateError::unavailable())
    }

    /// Sends an action-bar message packet to one online player.
    fn send_player_action_bar(&self, _player_uuid: &str, _text: &str) -> Result<(), StateError> {
        Err(StateError::unavailable())
    }

    /// Sends a sound packet to one online player.
    #[allow(clippy::too_many_arguments)]
    fn play_player_sound(
        &self,
        _player_uuid: &str,
        _sound: &str,
        _category: i32,
        _x: f64,
        _y: f64,
        _z: f64,
        _volume: f32,
        _pitch: f32,
        _seed: i64,
    ) -> Result<(), StateError> {
        Err(StateError::unavailable())
    }

    /// Sends a particle burst packet to one online player.
    #[allow(clippy::too_many_arguments)]
    fn spawn_player_particle(
        &self,
        _player_uuid: &str,
        _particle: &str,
        _x: f64,
        _y: f64,
        _z: f64,
        _count: i32,
        _offset_x: f32,
        _offset_y: f32,
        _offset_z: f32,
        _extra: f32,
        _force: bool,
    ) -> Result<(), StateError> {
        Err(StateError::unavailable())
    }

    /// Sends a boss bar add/remove/update packet to one online player.
    #[allow(clippy::too_many_arguments)]
    fn update_player_boss_bar(
        &self,
        _player_uuid: &str,
        _bar_uuid: &str,
        _operation: &str,
        _title: Option<String>,
        _progress: f32,
        _color: i32,
        _overlay: u8,
        _flags: u8,
    ) -> Result<(), StateError> {
        Err(StateError::unavailable())
    }

    /// Broadcasts a world border update to all online players in `world`.
    fn set_world_border(
        &self,
        _world: &str,
        _x: f64,
        _z: f64,
        _diameter: f64,
        _warning_blocks: i32,
        _warning_time: i32,
    ) -> Result<(), StateError> {
        Err(StateError::unavailable())
    }

    /// Sends one scoreboard update packet (`objective`, `display`, `score`, `reset`, or `team`)
    /// to a player.
    #[allow(clippy::too_many_arguments)]
    fn update_player_scoreboard(
        &self,
        _player_uuid: &str,
        _kind: &str,
        _name: &str,
        _method: u8,
        _display: &str,
        _hearts: bool,
        _slot: i32,
        _owner: &str,
        _score: i32,
        _params: Vec<String>,
        _players: Vec<String>,
    ) -> Result<(), StateError> {
        Err(StateError::unavailable())
    }

    /// Updates a player's flight/invulnerability/speed abilities packet.
    #[allow(clippy::too_many_arguments)]
    fn set_player_abilities(
        &self,
        _player_uuid: &str,
        _allow_flight: bool,
        _flying: bool,
        _invulnerable: bool,
        _fly_speed: f32,
        _walk_speed: f32,
    ) -> Result<(), StateError> {
        Err(StateError::unavailable())
    }

    /// Updates a player's tab list header and footer text.
    fn set_player_list_header_footer(
        &self,
        _player_uuid: &str,
        _header: &str,
        _footer: &str,
    ) -> Result<(), StateError> {
        Err(StateError::unavailable())
    }

    /// Updates a player's experience level/progress/total.
    fn set_player_experience(
        &self,
        _player_uuid: &str,
        _level: Option<i32>,
        _progress: Option<f32>,
        _total: Option<i32>,
    ) -> Result<(), StateError> {
        Err(StateError::unavailable())
    }

    /// Teleports a player to `(world, x, y, z, yaw, pitch)`.
    #[allow(clippy::too_many_arguments)]
    fn teleport_player(
        &self,
        _player_uuid: &str,
        _world: &str,
        _x: f64,
        _y: f64,
        _z: f64,
        _yaw: f32,
        _pitch: f32,
    ) -> Result<(), StateError> {
        Err(StateError::unavailable())
    }

    /// Sets a player's health (`0.0..=20.0`).
    fn set_player_health(&self, _player_uuid: &str, _health: f64) -> Result<(), StateError> {
        Err(StateError::unavailable())
    }

    /// Sets a player's food level (`0..=20`).
    fn set_player_food(&self, _player_uuid: &str, _food: u8) -> Result<(), StateError> {
        Err(StateError::unavailable())
    }

    /// Sets a player's game mode (`SURVIVAL`, `CREATIVE`, `ADVENTURE`, `SPECTATOR`).
    fn set_player_game_mode(&self, _player_uuid: &str, _mode: &str) -> Result<(), StateError> {
        Err(StateError::unavailable())
    }

    /// Disconnects a player with an optional plain-text kick reason.
    fn kick_player(&self, _player_uuid: &str, _reason: Option<String>) -> Result<(), StateError> {
        Err(StateError::unavailable())
    }

    /// Sends a system chat message to a player.
    fn send_player_message(&self, _player_uuid: &str, _message: &str) -> Result<(), StateError> {
        Err(StateError::unavailable())
    }

    /// Drain accepted writes on the game-loop thread. Default does nothing.
    fn tick(&self) {}

    /// Release pending work. Called once when the host starts shutting down.
    fn shutdown(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_request_and_snapshot_json_round_trip() {
        let request: BlockGetRequest = serde_json::from_value(
            serde_json::json!({"world": "minecraft:overworld", "x": 1, "y": 64, "z": -2}),
        )
        .unwrap();
        assert_eq!(request.x, 1);
        let snapshot = BlockSnapshot {
            world: request.world,
            x: request.x,
            y: request.y,
            z: request.z,
            state_id: 1,
            material: "minecraft:stone".into(),
            properties: BTreeMap::from([("facing".into(), "north".into())]),
            loaded: true,
        };
        let value = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(value["properties"]["facing"], "north");
        assert_eq!(value["loaded"], true);
        let parsed: BlockSnapshot = serde_json::from_value(value).unwrap();
        assert_eq!(parsed, snapshot);
    }

    #[test]
    fn unknown_fields_and_negative_state_ids_are_rejected() {
        assert!(
            serde_json::from_value::<BlockGetRequest>(
                serde_json::json!({"world": "minecraft:overworld", "x": 0, "y": 0, "z": 0, "extra": true})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<BlockSetRequest>(
                serde_json::json!({"world": "minecraft:overworld", "x": 0, "y": 0, "z": 0, "state_id": -1})
            )
            .is_err()
        );
    }
}
