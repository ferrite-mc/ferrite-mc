//! Embedded JVM bootstrap for the Bukkit/Paper plugin bridge.
//!
//! This crate is the lowest layer of plugin support: it starts a JVM inside the Ferrite process,
//! lazily and only when plugins exist. The JVM shared library (`jvm.dll`, `libjvm.so`,
//! `libjvm.dylib`) is loaded at runtime, so Ferrite has no link-time dependency on it.
//!
//! Java 25 or newer is required. The installation is located from, in order:
//!
//! 1. [`JvmConfig::java_home`], which must be valid;
//! 2. the `JAVA_HOME` environment variable;
//! 3. common install locations (`C:/Program Files/Eclipse Adoptium/jdk-*`, `/usr/lib/jvm/*`, ...).
//!
//! Every candidate is validated against its `release` file (`JAVA_VERSION`).
//!
//! # Example
//!
//! ```no_run
//! use ferrite_jvm::{Jvm, JvmConfig};
//!
//! let jvm = Jvm::start(JvmConfig {
//!     classpath: vec!["plugins/bridge.jar".into()],
//!     max_heap_mb: Some(2048),
//!     ..JvmConfig::default()
//! })?;
//! let answer: i32 = jvm.call_static("com/example/Bridge", "answer", "()I", &[])?;
//! # Ok::<(), ferrite_jvm::JvmError>(())
//! ```
//!
//! Only one JVM can exist per process (a JNI restriction), enforced with a
//! [`std::sync::OnceLock`]: a second [`Jvm::start`] returns [`JvmError::AlreadyStarted`]. The JVM
//! is never shut down, because JNI cannot fully destroy a VM.
//!
//! Helpers the plugin bridge builds on:
//!
//! * [`Jvm::attach_thread`] attaches the calling thread to the JVM as a daemon, cached
//!   thread-locally.
//! * [`Jvm::call_static`] calls a static Java method by class, name and signature.
//! * [`Jvm::register_natives`] binds Rust functions to Java `native` methods.
//! * Pending Java exceptions become [`JvmError::JavaException`] carrying the Java stack trace.
//! * [`JvmThreadMode`] is the placeholder for the scheduler integration that comes later.

mod config;
mod discovery;
mod error;
mod jvm;
mod native;
mod thread;

pub use config::JvmConfig;
pub use discovery::{JavaHome, MIN_JAVA_VERSION, find_java_home};
pub use error::{JvmError, Result};
pub use jvm::{FromJavaValue, Jvm};
pub use native::NativeMethod;
pub use thread::{JvmThreadMode, ThreadAttach};

/// Re-export of the [`jni`] crate, so callers use the same version for everything crossing this
/// API: [`JValue`](jni::objects::JValue), [`JNIEnv`](jni::JNIEnv), native function signatures,
/// and so on.
pub use jni;
