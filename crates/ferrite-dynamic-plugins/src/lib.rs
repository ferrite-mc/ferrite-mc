//! Drop-in C-ABI dynamic native plugin loader (`.so` / `.dll` / `.dylib`) for Ferrite.
//!
//! Allows external Rust (or C/C++/Zig) plugins compiled as `cdylib` to be dropped into
//! `plugins/native/` and loaded at server startup without recompiling `ferrite-server` or
//! launching a JVM.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::slice;
use std::sync::Mutex;

use libloading::{Library, Symbol};
use thiserror::Error;
use uuid::Uuid;

/// Current C-ABI version expected from `ferrite_plugin_entry()`.
pub const FERRITE_NATIVE_ABI_VERSION: u32 = 1;

/// Symbol name exported by every dynamic Ferrite native plugin library.
pub const ENTRY_SYMBOL: &[u8] = b"ferrite_plugin_entry\0";

/// Maximum command reply buffer allocated per C-ABI `on_command` call.
const COMMAND_REPLY_CAP: usize = 4096;

/// Errors produced when loading a dynamic native plugin.
#[derive(Debug, Error)]
pub enum DynamicLoadError {
    #[error("failed to load library '{path}': {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: libloading::Error,
    },
    #[error("missing symbol `ferrite_plugin_entry` in '{path}': {source}")]
    MissingEntry {
        path: PathBuf,
        #[source]
        source: libloading::Error,
    },
    #[error("`ferrite_plugin_entry()` returned null in '{path}'")]
    NullVTable { path: PathBuf },
    #[error("ABI mismatch in '{path}': expected v{expected}, found v{found}")]
    AbiMismatch {
        path: PathBuf,
        expected: u32,
        found: u32,
    },
}

/// Stable C-ABI v1 plugin virtual table exported by a dynamic library.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FerritePluginVTable {
    /// Must equal [`FERRITE_NATIVE_ABI_VERSION`].
    pub abi_version: u32,
    /// UTF-8 plugin name bytes.
    pub name_ptr: *const u8,
    pub name_len: usize,
    /// UTF-8 plugin version bytes.
    pub version_ptr: *const u8,
    pub version_len: usize,
    /// Called once immediately after ABI validation.
    pub on_enable: Option<extern "C" fn()>,
    /// Called once during server shutdown before unloading.
    pub on_disable: Option<extern "C" fn()>,
    /// Called when a player joins (`uuid_ptr` points to 16 big-endian UUID bytes).
    pub on_player_join:
        Option<extern "C" fn(uuid_ptr: *const u8, name_ptr: *const u8, name_len: usize)>,
    /// Called when a player leaves (`uuid_ptr` points to 16 big-endian UUID bytes).
    pub on_player_quit:
        Option<extern "C" fn(uuid_ptr: *const u8, name_ptr: *const u8, name_len: usize)>,
    /// Called on chat; return `1` to allow or `0` to cancel.
    pub on_chat:
        Option<extern "C" fn(uuid_ptr: *const u8, msg_ptr: *const u8, msg_len: usize) -> u8>,
    /// Called on block break; return `1` to allow or `0` to cancel.
    pub on_block_break: Option<extern "C" fn(uuid_ptr: *const u8, x: i32, y: i32, z: i32) -> u8>,
    /// Called on block place; return `1` to allow or `0` to cancel.
    pub on_block_place: Option<extern "C" fn(uuid_ptr: *const u8, x: i32, y: i32, z: i32) -> u8>,
    /// Called on command; return `>= 0` (UTF-8 reply length written to `out_buf`) if handled,
    /// or `-1` if this plugin does not own the command.
    pub on_command: Option<
        extern "C" fn(
            uuid_ptr: *const u8,
            line_ptr: *const u8,
            line_len: usize,
            out_buf: *mut u8,
            out_cap: usize,
        ) -> isize,
    >,
}

// SAFETY: Function pointers and immutable static byte slices inside a loaded library's vtable
// are safe to read across threads while `_library` remains alive.
unsafe impl Send for FerritePluginVTable {}
unsafe impl Sync for FerritePluginVTable {}

/// Metadata describing one loaded dynamic native plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedDynamicPlugin {
    pub name: String,
    pub version: String,
    pub path: PathBuf,
}

struct DynamicPluginInstance {
    meta: LoadedDynamicPlugin,
    vtable: FerritePluginVTable,
    disabled: bool,
    // Kept alive until after `on_disable` runs; field order ensures drop runs last.
    _library: Option<Library>,
}

impl DynamicPluginInstance {
    fn disable_once(&mut self) {
        if !self.disabled {
            self.disabled = true;
            if let Some(on_disable) = self.vtable.on_disable {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| on_disable()));
            }
        }
    }
}

impl Drop for DynamicPluginInstance {
    fn drop(&mut self) {
        self.disable_once();
    }
}

/// Manages zero or more dynamically loaded C-ABI native plugins.
#[derive(Default)]
pub struct DynamicPluginManager {
    plugins: Mutex<Vec<DynamicPluginInstance>>,
}

impl DynamicPluginManager {
    /// Creates an empty manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Scans `dir` for dynamic libraries matching the host platform extension (`.so`, `.dll`,
    /// `.dylib`) and loads every valid plugin. Missing directories are treated as empty.
    pub fn load_dir(dir: &Path) -> Self {
        let manager = Self::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return manager;
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file() && is_dynamic_library(p))
            .collect();
        paths.sort();

        for path in paths {
            match manager.load_library(&path) {
                Ok(meta) => tracing::info!(
                    plugin = %meta.name,
                    version = %meta.version,
                    path = %meta.path.display(),
                    "loaded dynamic native plugin"
                ),
                Err(error) => tracing::warn!(%error, "skipping dynamic native plugin"),
            }
        }
        manager
    }

    /// Loads one dynamic library file and invokes its `on_enable` hook.
    pub fn load_library(&self, path: &Path) -> Result<LoadedDynamicPlugin, DynamicLoadError> {
        // SAFETY: We validate the exported symbol signature, non-null vtable pointer, and ABI
        // version before calling any function pointer, and retain `Library` for the lifetime of
        // the instance.
        let library = unsafe {
            Library::new(path).map_err(|source| DynamicLoadError::Open {
                path: path.to_path_buf(),
                source,
            })?
        };
        let vtable = unsafe {
            let entry: Symbol<'_, unsafe extern "C" fn() -> *const FerritePluginVTable> = library
                .get(ENTRY_SYMBOL)
                .map_err(|source| DynamicLoadError::MissingEntry {
                    path: path.to_path_buf(),
                    source,
                })?;
            let ptr = entry();
            if ptr.is_null() {
                return Err(DynamicLoadError::NullVTable {
                    path: path.to_path_buf(),
                });
            }
            *ptr
        };
        if vtable.abi_version != FERRITE_NATIVE_ABI_VERSION {
            return Err(DynamicLoadError::AbiMismatch {
                path: path.to_path_buf(),
                expected: FERRITE_NATIVE_ABI_VERSION,
                found: vtable.abi_version,
            });
        }
        let name = read_utf8(vtable.name_ptr, vtable.name_len).unwrap_or_else(|| {
            path.file_stem()
                .and_then(OsStr::to_str)
                .unwrap_or("unknown")
                .to_owned()
        });
        let version =
            read_utf8(vtable.version_ptr, vtable.version_len).unwrap_or_else(|| "0.0.0".to_owned());
        if let Some(on_enable) = vtable.on_enable {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| on_enable()));
        }
        let meta = LoadedDynamicPlugin {
            name,
            version,
            path: path.to_path_buf(),
        };
        self.plugins
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(DynamicPluginInstance {
                meta: meta.clone(),
                vtable,
                disabled: false,
                _library: Some(library),
            });
        Ok(meta)
    }

    /// Registers an in-process vtable directly (useful for static extensions and unit tests).
    pub fn register_static_vtable(
        &self,
        vtable: FerritePluginVTable,
    ) -> Result<LoadedDynamicPlugin, DynamicLoadError> {
        if vtable.abi_version != FERRITE_NATIVE_ABI_VERSION {
            return Err(DynamicLoadError::AbiMismatch {
                path: PathBuf::from("<static>"),
                expected: FERRITE_NATIVE_ABI_VERSION,
                found: vtable.abi_version,
            });
        }
        let name = read_utf8(vtable.name_ptr, vtable.name_len).unwrap_or_else(|| "static".into());
        let version =
            read_utf8(vtable.version_ptr, vtable.version_len).unwrap_or_else(|| "0.0.0".into());
        if let Some(on_enable) = vtable.on_enable {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| on_enable()));
        }
        let meta = LoadedDynamicPlugin {
            name,
            version,
            path: PathBuf::from("<static>"),
        };
        self.plugins
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(DynamicPluginInstance {
                meta: meta.clone(),
                vtable,
                disabled: false,
                _library: None,
            });
        Ok(meta)
    }

    /// Returns whether zero dynamic plugins are loaded.
    pub fn is_empty(&self) -> bool {
        self.plugins
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_empty()
    }

    /// Returns metadata for all loaded dynamic plugins.
    pub fn loaded_plugins(&self) -> Vec<LoadedDynamicPlugin> {
        self.plugins
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .map(|p| p.meta.clone())
            .collect()
    }

    /// Dispatches player join to all loaded dynamic plugins.
    pub fn dispatch_player_join(&self, uuid: Uuid, name: &str) {
        let bytes = uuid.into_bytes();
        let guard = self.plugins.lock().unwrap_or_else(|p| p.into_inner());
        for plugin in guard.iter() {
            if let Some(f) = plugin.vtable.on_player_join {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    f(bytes.as_ptr(), name.as_ptr(), name.len());
                }));
            }
        }
    }

    /// Dispatches player quit to all loaded dynamic plugins.
    pub fn dispatch_player_quit(&self, uuid: Uuid, name: &str) {
        let bytes = uuid.into_bytes();
        let guard = self.plugins.lock().unwrap_or_else(|p| p.into_inner());
        for plugin in guard.iter() {
            if let Some(f) = plugin.vtable.on_player_quit {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    f(bytes.as_ptr(), name.as_ptr(), name.len());
                }));
            }
        }
    }

    /// Dispatches chat; returns `false` as soon as any plugin cancels it.
    pub fn dispatch_chat(&self, uuid: Uuid, message: &str) -> bool {
        let bytes = uuid.into_bytes();
        let guard = self.plugins.lock().unwrap_or_else(|p| p.into_inner());
        for plugin in guard.iter() {
            if let Some(f) = plugin.vtable.on_chat {
                let allowed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    f(bytes.as_ptr(), message.as_ptr(), message.len()) != 0
                }))
                .unwrap_or(true);
                if !allowed {
                    return false;
                }
            }
        }
        true
    }

    /// Dispatches block break; returns `false` if cancelled.
    pub fn dispatch_block_break(&self, uuid: Uuid, x: i32, y: i32, z: i32) -> bool {
        let bytes = uuid.into_bytes();
        let guard = self.plugins.lock().unwrap_or_else(|p| p.into_inner());
        for plugin in guard.iter() {
            if let Some(f) = plugin.vtable.on_block_break {
                let allowed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    f(bytes.as_ptr(), x, y, z) != 0
                }))
                .unwrap_or(true);
                if !allowed {
                    return false;
                }
            }
        }
        true
    }

    /// Dispatches block place; returns `false` if cancelled.
    pub fn dispatch_block_place(&self, uuid: Uuid, x: i32, y: i32, z: i32) -> bool {
        let bytes = uuid.into_bytes();
        let guard = self.plugins.lock().unwrap_or_else(|p| p.into_inner());
        for plugin in guard.iter() {
            if let Some(f) = plugin.vtable.on_block_place {
                let allowed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    f(bytes.as_ptr(), x, y, z) != 0
                }))
                .unwrap_or(true);
                if !allowed {
                    return false;
                }
            }
        }
        true
    }

    /// Dispatches a command line; returns `Some(reply)` for the first plugin that handles it.
    pub fn dispatch_command(&self, sender: Uuid, line: &str) -> Option<String> {
        let bytes = sender.into_bytes();
        let mut buf = vec![0u8; COMMAND_REPLY_CAP];
        let guard = self.plugins.lock().unwrap_or_else(|p| p.into_inner());
        for plugin in guard.iter() {
            if let Some(f) = plugin.vtable.on_command {
                let written = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    f(
                        bytes.as_ptr(),
                        line.as_ptr(),
                        line.len(),
                        buf.as_mut_ptr(),
                        buf.len(),
                    )
                }))
                .unwrap_or(-1);
                if written >= 0 {
                    let len = (written as usize).min(buf.len());
                    return Some(String::from_utf8_lossy(&buf[..len]).into_owned());
                }
            }
        }
        None
    }

    /// Calls `on_disable` on every loaded dynamic plugin in reverse load order.
    pub fn shutdown(&self) {
        let mut guard = self.plugins.lock().unwrap_or_else(|p| p.into_inner());
        for plugin in guard.iter_mut().rev() {
            plugin.disable_once();
        }
    }
}

fn is_dynamic_library(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(OsStr::to_str) else {
        return false;
    };
    #[cfg(target_os = "windows")]
    {
        ext.eq_ignore_ascii_case("dll")
    }
    #[cfg(target_os = "macos")]
    {
        ext.eq_ignore_ascii_case("dylib")
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        ext.eq_ignore_ascii_case("so")
    }
}

fn read_utf8(ptr: *const u8, len: usize) -> Option<String> {
    if ptr.is_null() || len == 0 || len > 256 {
        return None;
    }
    // SAFETY: Caller guarantees `(ptr, len)` points to valid static bytes from the vtable.
    let bytes = unsafe { slice::from_raw_parts(ptr, len) };
    std::str::from_utf8(bytes).ok().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    static ENABLED: AtomicBool = AtomicBool::new(false);
    static DISABLED: AtomicBool = AtomicBool::new(false);

    extern "C" fn test_enable() {
        ENABLED.store(true, Ordering::SeqCst);
    }

    extern "C" fn test_disable() {
        DISABLED.store(true, Ordering::SeqCst);
    }

    extern "C" fn test_chat(_uuid: *const u8, msg_ptr: *const u8, msg_len: usize) -> u8 {
        let msg = unsafe { slice::from_raw_parts(msg_ptr, msg_len) };
        if msg == b"forbidden" { 0 } else { 1 }
    }

    extern "C" fn test_command(
        _uuid: *const u8,
        line_ptr: *const u8,
        line_len: usize,
        out_buf: *mut u8,
        out_cap: usize,
    ) -> isize {
        let line =
            unsafe { std::str::from_utf8_unchecked(slice::from_raw_parts(line_ptr, line_len)) };
        if line == "/dynping" {
            let reply = b"pong-from-dynamic";
            let n = reply.len().min(out_cap);
            unsafe {
                std::ptr::copy_nonoverlapping(reply.as_ptr(), out_buf, n);
            }
            n as isize
        } else {
            -1
        }
    }

    #[test]
    fn static_vtable_lifecycle_events_and_commands_round_trip() {
        ENABLED.store(false, Ordering::SeqCst);
        DISABLED.store(false, Ordering::SeqCst);

        let name = b"DemoDynamic";
        let version = b"1.2.3";
        let manager = DynamicPluginManager::new();
        let meta = manager
            .register_static_vtable(FerritePluginVTable {
                abi_version: FERRITE_NATIVE_ABI_VERSION,
                name_ptr: name.as_ptr(),
                name_len: name.len(),
                version_ptr: version.as_ptr(),
                version_len: version.len(),
                on_enable: Some(test_enable),
                on_disable: Some(test_disable),
                on_player_join: None,
                on_player_quit: None,
                on_chat: Some(test_chat),
                on_block_break: None,
                on_block_place: None,
                on_command: Some(test_command),
            })
            .unwrap();

        assert_eq!(meta.name, "DemoDynamic");
        assert_eq!(meta.version, "1.2.3");
        assert!(ENABLED.load(Ordering::SeqCst));
        assert!(manager.dispatch_chat(Uuid::nil(), "hello"));
        assert!(!manager.dispatch_chat(Uuid::nil(), "forbidden"));
        assert_eq!(
            manager.dispatch_command(Uuid::nil(), "/dynping").as_deref(),
            Some("pong-from-dynamic")
        );
        assert_eq!(manager.dispatch_command(Uuid::nil(), "/other"), None);

        manager.shutdown();
        assert!(DISABLED.load(Ordering::SeqCst));
    }

    #[test]
    fn rejects_mismatched_abi_version_and_handles_missing_dir() {
        let manager = DynamicPluginManager::load_dir(Path::new("/nonexistent/ferrite/dir"));
        assert!(manager.is_empty());

        let err = manager
            .register_static_vtable(FerritePluginVTable {
                abi_version: 999,
                name_ptr: std::ptr::null(),
                name_len: 0,
                version_ptr: std::ptr::null(),
                version_len: 0,
                on_enable: None,
                on_disable: None,
                on_player_join: None,
                on_player_quit: None,
                on_chat: None,
                on_block_break: None,
                on_block_place: None,
                on_command: None,
            })
            .unwrap_err();
        assert!(matches!(
            err,
            DynamicLoadError::AbiMismatch { found: 999, .. }
        ));
    }
}
