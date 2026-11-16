//! Errors reported by the plugin host.

use std::path::PathBuf;

use thiserror::Error;

/// Result alias used throughout this crate.
pub type Result<T, E = HostError> = std::result::Result<T, E>;

/// Everything that can go wrong while starting or driving the plugin host.
#[derive(Debug, Error)]
pub enum HostError {
    /// `plugins.enabled = false`.
    #[error("the plugin host is disabled")]
    Disabled,

    /// A host was already started in this process. The embedded JVM cannot be restarted.
    #[error("a plugin host is already running in this process")]
    AlreadyStarted,

    /// A JVM exists but its classpath does not include this host's jar.
    #[error(
        "the embedded JVM is already running without {} on its classpath",
        path.display()
    )]
    JvmAlreadyRunning {
        /// Host jar that is missing from the running JVM's classpath.
        path: PathBuf,
    },

    /// The configured `ferrite-host` jar/classes directory does not exist.
    #[error("the plugin host jar is missing: {}", path.display())]
    HostJarMissing {
        /// Configured host jar path.
        path: PathBuf,
    },

    /// Reading or validating plugin metadata failed.
    #[error("plugin metadata: {0}")]
    Metadata(String),

    /// The configuration or contract version is invalid.
    #[error("invalid plugin host configuration: {0}")]
    InvalidConfig(String),

    /// Starting or calling into the JVM failed.
    #[error("JVM error: {0}")]
    Jvm(String),

    /// A Java method threw. The string includes the Java stack trace when one was available.
    #[error("Java exception: {0}")]
    JavaException(String),

    /// A host call did not finish inside its timeout. JNI cannot cancel it, so the Java work
    /// may still be running.
    #[error("Java call '{operation}' timed out after {millis} ms")]
    Timeout {
        /// Which call timed out.
        operation: &'static str,
        /// Configured timeout in milliseconds.
        millis: u64,
    },

    /// The Java host returned JSON that does not match the contract.
    #[error("invalid response from the Java host: {0}")]
    InvalidResponse(String),

    /// The host has not started or has already shut down.
    #[error("the plugin host is not running")]
    NotRunning,

    /// The dedicated Java host thread is gone.
    #[error("the Java plugin host thread stopped")]
    HostThreadStopped,

    /// One plugin failed during load or enable. Other plugins keep loading.
    #[error("plugin '{plugin}' failed during {phase}: {message}")]
    PluginFailed {
        /// Plugin name.
        plugin: String,
        /// Java phase: `load`, `enable` or `api`.
        phase: String,
        /// Exception text with stack trace.
        message: String,
    },

    /// An event name is not part of contract v1.
    #[error("unknown plugin event '{0}'")]
    UnknownEvent(String),

    /// An RPC op is not part of contract v1.
    #[error("unknown plugin host op '{0}'")]
    UnknownOp(String),

    /// A Java→Rust RPC failed with a contract error code.
    #[error("{message} ({code})")]
    Rpc {
        /// Contract error code.
        code: String,
        /// Human-readable message.
        message: String,
    },

    /// Filesystem error while scanning or preparing plugin directories.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// JSON encoding/decoding error.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl HostError {
    /// Contract error code for this error.
    pub fn code(&self) -> &str {
        match self {
            Self::Disabled => "disabled",
            Self::AlreadyStarted => "already_started",
            Self::JvmAlreadyRunning { .. } => "jvm_already_running",
            Self::HostJarMissing { .. } => "host_jar_missing",
            Self::Metadata(_) => "metadata",
            Self::InvalidConfig(_) => "invalid_config",
            Self::Jvm(_) => "java_exception",
            Self::JavaException(_) => "java_exception",
            Self::Timeout { .. } => "timeout",
            Self::InvalidResponse(_) => "invalid_response",
            Self::NotRunning | Self::HostThreadStopped => "not_running",
            Self::PluginFailed { .. } => "plugin_failed",
            Self::UnknownEvent(_) => "unknown_event",
            Self::UnknownOp(_) => "unknown_op",
            Self::Rpc { code, .. } => code,
            Self::Io(_) => "io",
            Self::Json(_) => "invalid_response",
        }
    }
}
