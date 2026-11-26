//! Threads and the JVM.

use std::cell::Cell;

use jni::JavaVM;

use crate::error::Result;

thread_local! {
    /// Whether this thread has already attached itself to the JVM.
    static ATTACHED: Cell<bool> = const { Cell::new(false) };
}

/// How the plugin bridge will schedule JVM callbacks.
///
/// **Placeholder**: the Folia-style scheduler integration lands in a later task. The value is
/// carried on [`Jvm`](crate::Jvm) so that wiring does not have to change when it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum JvmThreadMode {
    /// Plugin callbacks run on the region thread that owns the chunks they touch.
    RegionAware,
    /// Plugin callbacks run on a single shared "main" thread, like legacy Bukkit.
    #[default]
    LegacyMainThread,
}

/// Proof that the current thread is attached to the JVM as a daemon thread.
///
/// The attachment is cached in thread-local storage: the first call on a thread performs the JNI
/// attach, later calls only check the cache, and the thread stays attached until it exits
/// (the `jni` crate registers its own thread-local guard that detaches daemon threads at thread
/// exit). Dropping this token therefore does **not** detach the thread.
#[derive(Debug)]
pub struct ThreadAttach {
    _private: (),
}

/// Attaches the current thread to `vm` as a daemon if it is not attached already.
pub(crate) fn attach(vm: &'static JavaVM) -> Result<ThreadAttach> {
    if !ATTACHED.with(Cell::get) {
        let _env = vm.attach_current_thread_as_daemon()?;
        ATTACHED.with(|attached| attached.set(true));
    }
    Ok(ThreadAttach { _private: () })
}
