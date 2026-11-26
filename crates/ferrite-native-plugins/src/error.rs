//! Error types for the Ferrite native plugin suite.

use std::io;
use std::path::PathBuf;

use thiserror::Error;

/// Crate-wide result alias.
pub type Result<T, E = PluginError> = std::result::Result<T, E>;

/// Errors returned by native plugin engines and persistence helpers.
#[derive(Debug, Error)]
pub enum PluginError {
    /// Reading or writing a persistence file failed.
    #[error("I/O error on {path}: {source}")]
    Io {
        /// File path involved in the operation.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: io::Error,
    },

    /// Serializing or deserializing JSON state failed.
    #[error("JSON error on {path}: {source}")]
    Json {
        /// File path involved in the operation.
        path: PathBuf,
        /// Underlying JSON error.
        #[source]
        source: serde_json::Error,
    },

    /// An identifier (permission node, group, region, home, or warp name) was invalid.
    #[error("invalid identifier '{0}'")]
    InvalidIdentifier(String),

    /// A permission group already exists.
    #[error("permission group '{0}' already exists")]
    GroupAlreadyExists(String),

    /// A referenced permission group does not exist.
    #[error("permission group '{0}' does not exist")]
    GroupNotFound(String),

    /// Adding a parent group would introduce an inheritance cycle.
    #[error("adding parent '{parent}' to group '{group}' would create an inheritance cycle")]
    CyclicInheritance {
        /// Child group being modified.
        group: String,
        /// Candidate parent group.
        parent: String,
    },

    /// A region with the given name already exists in the world.
    #[error("region '{name}' already exists in world '{world}'")]
    RegionAlreadyExists {
        /// World name.
        world: String,
        /// Region name.
        name: String,
    },

    /// A region with the given name does not exist in the world.
    #[error("region '{name}' does not exist in world '{world}'")]
    RegionNotFound {
        /// World name.
        world: String,
        /// Region name.
        name: String,
    },

    /// A named home was not found for the player.
    #[error("home '{0}' not found")]
    HomeNotFound(String),

    /// A named global warp was not found.
    #[error("warp '{0}' not found")]
    WarpNotFound(String),

    /// Global spawn has not been set yet.
    #[error("global spawn is not set")]
    SpawnNotSet,

    /// No pending teleport request exists for the target player.
    #[error("no pending TPA request")]
    NoPendingTpa,

    /// The pending teleport request has expired.
    #[error("TPA request has expired")]
    TpaExpired,

    /// A player attempted to send a TPA request to themselves.
    #[error("cannot send a TPA request to yourself")]
    CannotTpaSelf,

    /// An economy amount was negative, zero, NaN, or infinite.
    #[error("invalid currency amount: {0}")]
    InvalidAmount(f64),

    /// An account did not have enough funds for a withdrawal or payment.
    #[error("insufficient funds: balance is {balance:.2}, required {required:.2}")]
    InsufficientFunds {
        /// Available balance.
        balance: f64,
        /// Requested debit amount.
        required: f64,
    },

    /// A player attempted to pay themselves.
    #[error("cannot pay yourself")]
    CannotPaySelf,

    /// One or both corners of a WorldEdit cuboid selection are missing.
    #[error("selection is incomplete; set both pos1 and pos2")]
    IncompleteSelection,

    /// The cuboid selection exceeds the configured `max_blocks` safety cap.
    #[error("selection of {blocks} blocks exceeds the safety cap of {max_blocks} blocks")]
    SelectionTooLarge {
        /// Computed block count of the selection.
        blocks: u64,
        /// Configured maximum block limit.
        max_blocks: usize,
    },

    /// The player's WorldEdit clipboard is empty.
    #[error("clipboard is empty")]
    EmptyClipboard,

    /// There is no edit in the player's undo stack.
    #[error("nothing to undo")]
    NothingToUndo,

    /// There is no edit in the player's redo stack.
    #[error("nothing to redo")]
    NothingToRedo,
}
