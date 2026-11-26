//! The error type shared by everything in this crate.

use std::path::PathBuf;

use thiserror::Error;

use crate::MIN_JAVA_VERSION;

/// Result alias used throughout this crate.
pub type Result<T, E = JvmError> = std::result::Result<T, E>;

/// Everything that can go wrong while locating Java, starting the JVM or calling into it.
#[derive(Debug, Error)]
pub enum JvmError {
    /// [`Jvm::start`](crate::Jvm::start) was called while a JVM was already running in this
    /// process. JNI allows exactly one VM per process.
    #[error("a JVM is already running in this process")]
    AlreadyStarted,

    /// No Java installation with the required version was found in any searched location.
    #[error("no Java {min}+ installation found (searched: {})", tried.join(", "), min = MIN_JAVA_VERSION)]
    JavaNotFound {
        /// Human-readable description of every location that was tried.
        tried: Vec<String>,
    },

    /// A location that looked like a Java home is not a Java [`MIN_JAVA_VERSION`]+ JDK.
    #[error(
        "{} is not a Java {min}+ JDK (found version: {})",
        path.display(),
        found.as_deref().unwrap_or("unknown"),
        min = MIN_JAVA_VERSION
    )]
    UnsupportedJava {
        /// The directory that was checked.
        path: PathBuf,
        /// Value of `JAVA_VERSION` in its `release` file, if it has one.
        found: Option<String>,
    },

    /// A supported JDK was found, but it does not ship a JVM shared library.
    #[error("no JVM shared library found under {}", root.display())]
    JvmLibraryMissing {
        /// Root of the Java installation.
        root: PathBuf,
    },

    /// The configuration itself is invalid.
    #[error("invalid JVM configuration: {0}")]
    Config(String),

    /// A JVM option could not be encoded for the platform.
    #[error("invalid JVM option: {0}")]
    InitArgs(#[from] jni::JvmError),

    /// The JVM shared library could not be loaded, or `JNI_CreateJavaVM` failed.
    #[error("failed to start the JVM: {0}")]
    Start(#[from] jni::errors::StartJvmError),

    /// A JNI call failed.
    #[error("JNI error: {0}")]
    Jni(#[from] jni::errors::Error),

    /// A Java method threw. The string is the output of `Throwable.printStackTrace()`, so it
    /// includes both the message and the Java stack trace.
    #[error("Java exception:\n{0}")]
    JavaException(String),

    /// An I/O error while inspecting a Java installation.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
