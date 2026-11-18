//! The error type shared by Java installation discovery.

use std::path::PathBuf;

use thiserror::Error;

use crate::MIN_JAVA_VERSION;

/// Result alias used throughout this crate.
pub type Result<T, E = JvmError> = std::result::Result<T, E>;

/// Errors returned while locating or validating a Java installation.
#[derive(Debug, Error)]
pub enum JvmError {
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
}
