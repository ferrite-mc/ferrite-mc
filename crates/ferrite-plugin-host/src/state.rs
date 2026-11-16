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
pub const V2_OPS: [&str; 5] = [
    "world.block_get",
    "world.block_set_async",
    "world.chunk_loaded",
    "player.state_get",
    "entity.state_get",
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

    /// Entity snapshot candidate. Not implemented in this vertical slice.
    fn entity_state_get(&self, uuid: &str) -> Result<Value, StateError> {
        let _ = uuid;
        Err(StateError::unsupported("entity.state_get"))
    }

    /// Ops this provider actually implements, advertised by `host.info`.
    fn supported_ops(&self) -> Vec<&'static str> {
        vec!["world.block_get", "world.block_set_async"]
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
