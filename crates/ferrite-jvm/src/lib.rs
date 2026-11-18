//! Locating a Java 25+ installation for Ferrite's vanilla world generation host.
//!
//! Java 25 or newer is required. The installation is located from, in order:
//!
//! 1. an explicit Java home path, which must be valid;
//! 2. the `JAVA_HOME` environment variable;
//! 3. common install locations (`C:/Program Files/Eclipse Adoptium/jdk-*`, `/usr/lib/jvm/*`, ...).
//!
//! Every candidate is validated against its `release` file (`JAVA_VERSION`).

mod discovery;
mod error;

pub use discovery::{JavaHome, MIN_JAVA_VERSION, find_java_home};
pub use error::{JvmError, Result};
