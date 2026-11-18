//! Embedded JVM plugin host for Ferrite (Plugin B, Lane 1).
//!
//! This crate owns the one embedded JVM per process, the plugin class loaders, the
//! `onLoad`/`onEnable`/`onDisable` life cycle and the small RPC surface the Java host uses.
//! Bukkit/Paper facade classes and NMS wrappers are owned by other lanes; see
//! `docs/plugin-b-host-contract.md` for the frozen v2 contract (v1 is compatible).
//!
//! # Example
//!
//! ```no_run
//! use ferrite_plugin_host::{PluginHost, PluginHostConfig};
//!
//! let config = PluginHostConfig {
//!     enabled: true,
//!     plugins_dir: "plugins".into(),
//!     ..PluginHostConfig::default()
//! };
//! match PluginHost::start(config) {
//!     Ok(host) => {
//!         let _ = host.load_postworld();
//!         // ... server runs and calls host.tick() every tick ...
//!         let _ = host.shutdown();
//!     }
//!     Err(error) => eprintln!("plugin host: {error}"),
//! }
//! ```
//!
//! Invariants:
//!
//! * [`PluginHost::start`] never creates a second JVM. If [`ferrite_jvm::Jvm::get`] already
//!   returns a VM whose classpath does not contain the host jar, it fails instead of starting
//!   another one.
//! * `plugins.enabled = false` returns [`HostError::Disabled`] without touching the plugins
//!   directory or starting a JVM.
//! * A missing plugins directory or an empty directory starts no JVM.
//! * Every Java call is made on the dedicated `ferrite-plugin-host` thread with a timeout; a
//!   hung plugin call becomes [`HostError::Timeout`] instead of blocking the server forever.

mod config;
mod error;
mod host;
mod native;
mod protocol;
mod scheduler;
mod state;

pub use config::{PluginHostConfig, ServerInfo};
pub use error::{HostError, Result};
pub use host::{
    HostCommandInfo, LoadReport, LoadedPluginInfo, PluginFailure, PluginHost, PluginPhase,
    ShutdownReport,
};
pub use protocol::{CommandOrigin, CommandOutcome, EventOutcome, HostMessage};
pub use scheduler::TaskId;
pub use state::{
    BiomeSnapshot, BlockGetRequest, BlockSetAccepted, BlockSetRequest, BlockSnapshot, ChunkCoord,
    ChunkStatusSnapshot, HostStateProvider, InventorySetAccepted, InventorySlotPatch,
    LoadedChunksSnapshot, MAX_PLUGIN_CHANNEL_LEN, MAX_PLUGIN_MESSAGE_BYTES,
    MAX_SERVERBOUND_PLUGIN_MESSAGE_BYTES, PlayerInventorySetRequest, PlayerInventorySnapshot,
    StateError, UuidRequest, V2_OPS, V3_OPS, WorldEntitiesSnapshot, WorldEntitySnapshotEntry,
};

/// Minimum supported Rust↔Java host contract version (v2 remains backward-compatible).
pub const MIN_CONTRACT_VERSION: i64 = 2;

/// Current Rust↔Java host contract version in `docs/plugin-b-host-contract.md`.
pub const CONTRACT_VERSION: i64 = 3;
