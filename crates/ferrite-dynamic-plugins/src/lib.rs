//! Drop-in C-ABI dynamic native plugin loader (`.so` / `.dll` / `.dylib`) with runtime
//! hot-load, hot-unload, and shadow-copy hot-reload support for Ferrite.

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::slice;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::SystemTime;

use libloading::{Library, Symbol};
use thiserror::Error;
use uuid::Uuid;

/// Current C-ABI version expected from `ferrite_plugin_entry()`.
pub const FERRITE_NATIVE_ABI_VERSION: u32 = 1;

/// Optional C-ABI v2 extension version expected from `ferrite_plugin_entry_v2()`.
pub const FERRITE_NATIVE_ABI_V2_VERSION: u32 = 2;

/// Symbol name exported by every dynamic Ferrite native plugin library.
pub const ENTRY_SYMBOL: &[u8] = b"ferrite_plugin_entry\0";

/// Optional symbol exported by C-ABI v2 dynamic plugins for extended hooks and host callbacks.
pub const ENTRY_SYMBOL_V2: &[u8] = b"ferrite_plugin_entry_v2\0";

/// Maximum command reply buffer allocated per C-ABI `on_command` call.
const COMMAND_REPLY_CAP: usize = 4096;

static STAGE_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Errors produced when loading a dynamic native plugin.
#[derive(Debug, Error)]
pub enum DynamicLoadError {
    #[error("failed to stage library '{path}': {source}")]
    Stage {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
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
    /// Called once during server shutdown or hot-unload before unloading.
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

/// Host-side bridge implemented by `ferrite-server` so C-ABI v2 dynamic plugins can interact
/// directly with live players and world chunks without crossing a JVM boundary.
pub trait DynamicHostBridge: Send + Sync {
    /// Sends a plain chat message to one online player.
    fn send_message(&self, player: Uuid, message: &str);
    /// Broadcasts a plain chat message to all online players.
    fn broadcast(&self, message: &str);
    /// Sends an action-bar message to one online player.
    fn send_action_bar(&self, player: Uuid, message: &str);
    /// Teleports a player to `(world, x, y, z, yaw, pitch)`.
    #[allow(clippy::too_many_arguments)]
    fn teleport(&self, player: Uuid, world: &str, x: f64, y: f64, z: f64, yaw: f32, pitch: f32);
    /// Reads a loaded block state id, or `None` if the world/chunk is not loaded.
    fn get_block(&self, world: &str, x: i32, y: i32, z: i32) -> Option<u32>;
    /// Writes a block state id into a loaded chunk; returns `true` on success.
    fn set_block(&self, world: &str, x: i32, y: i32, z: i32, state_id: u32) -> bool;
    /// Sends a title and subtitle to one online player.
    fn send_title(
        &self,
        _player: Uuid,
        _title: &str,
        _subtitle: &str,
        _fade_in: i32,
        _stay: i32,
        _fade_out: i32,
    ) {
    }
    /// Plays a sound effect for one online player.
    #[allow(clippy::too_many_arguments)]
    fn play_sound(
        &self,
        _player: Uuid,
        _sound: &str,
        _x: f64,
        _y: f64,
        _z: f64,
        _volume: f32,
        _pitch: f32,
    ) {
    }
    /// Kicks a player with an optional plain-text reason.
    fn kick_player(&self, _player: Uuid, _reason: &str) {}
    /// Sets a player's gamemode (`0=survival, 1=creative, 2=adventure, 3=spectator`).
    fn set_gamemode(&self, _player: Uuid, _mode: u8) {}
    /// Runs a command on the live server as `player`.
    fn perform_command(&self, _player: Uuid, _command: &str) {}
}

static HOST_BRIDGE: RwLock<Option<Arc<dyn DynamicHostBridge>>> = RwLock::new(None);

/// Installs the active [`DynamicHostBridge`] used by [`FerriteHostCallbacks`].
pub fn install_host_bridge(bridge: Arc<dyn DynamicHostBridge>) {
    *HOST_BRIDGE.write().unwrap_or_else(|p| p.into_inner()) = Some(bridge);
}

/// Clears the active [`DynamicHostBridge`].
pub fn clear_host_bridge() {
    *HOST_BRIDGE.write().unwrap_or_else(|p| p.into_inner()) = None;
}

/// Host-to-plugin callback table handed to C-ABI v2 plugins during `on_host_ready`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FerriteHostCallbacks {
    /// Logs a UTF-8 message to Ferrite's structured tracing logger (`level`: 0=trace..4=error).
    pub log: extern "C" fn(level: u8, msg_ptr: *const u8, msg_len: usize),
    /// Sends a UTF-8 message to the player identified by 16 big-endian UUID bytes.
    pub send_message: extern "C" fn(uuid_ptr: *const u8, msg_ptr: *const u8, msg_len: usize),
    /// Broadcasts a UTF-8 message to all online players.
    pub broadcast: extern "C" fn(msg_ptr: *const u8, msg_len: usize),
    /// Sends a UTF-8 action-bar message to the player identified by 16 big-endian UUID bytes.
    pub send_action_bar: extern "C" fn(uuid_ptr: *const u8, msg_ptr: *const u8, msg_len: usize),
    /// Teleports a player to `(world, x, y, z, yaw, pitch)`.
    pub teleport: extern "C" fn(
        uuid_ptr: *const u8,
        world_ptr: *const u8,
        world_len: usize,
        x: f64,
        y: f64,
        z: f64,
        yaw: f32,
        pitch: f32,
    ),
    /// Reads a loaded block state id (`-1` if world/chunk is unloaded or bridge unavailable).
    pub get_block:
        extern "C" fn(world_ptr: *const u8, world_len: usize, x: i32, y: i32, z: i32) -> i32,
    /// Writes a block state id into a loaded chunk (`1` on success, `0` on failure).
    pub set_block: extern "C" fn(
        world_ptr: *const u8,
        world_len: usize,
        x: i32,
        y: i32,
        z: i32,
        state_id: u32,
    ) -> u8,
    /// Sends a title + subtitle to one player.
    pub send_title: extern "C" fn(
        uuid_ptr: *const u8,
        title_ptr: *const u8,
        title_len: usize,
        sub_ptr: *const u8,
        sub_len: usize,
        fade_in: i32,
        stay: i32,
        fade_out: i32,
    ),
    /// Plays a sound effect to one player at `(x, y, z)`.
    pub play_sound: extern "C" fn(
        uuid_ptr: *const u8,
        sound_ptr: *const u8,
        sound_len: usize,
        x: f64,
        y: f64,
        z: f64,
        volume: f32,
        pitch: f32,
    ),
    /// Kicks a player with a UTF-8 reason string.
    pub kick_player: extern "C" fn(uuid_ptr: *const u8, reason_ptr: *const u8, reason_len: usize),
    /// Sets a player's gamemode (`0=survival, 1=creative, 2=adventure, 3=spectator`).
    pub set_gamemode: extern "C" fn(uuid_ptr: *const u8, mode: u8),
    /// Executes a slash command on the live server as the given player.
    pub perform_command: extern "C" fn(uuid_ptr: *const u8, cmd_ptr: *const u8, cmd_len: usize),
}

unsafe impl Send for FerriteHostCallbacks {}
unsafe impl Sync for FerriteHostCallbacks {}

/// Optional C-ABI v2 extension vtable for high-frequency events, ticks, and host callbacks.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FerritePluginVTableV2 {
    /// Must equal [`FERRITE_NATIVE_ABI_V2_VERSION`].
    pub abi_version: u32,
    /// Called immediately after load with host callback pointers.
    pub on_host_ready: Option<extern "C" fn(callbacks: *const FerriteHostCallbacks)>,
    /// Called when a player crosses a block boundary; return `1` to allow or `0` to cancel.
    pub on_player_move: Option<
        extern "C" fn(
            uuid_ptr: *const u8,
            from_x: f64,
            from_y: f64,
            from_z: f64,
            to_x: f64,
            to_y: f64,
            to_z: f64,
        ) -> u8,
    >,
    /// Called on player block interaction (`action`: 0=left, 1=right); return `1` to allow or `0` to cancel.
    pub on_player_interact:
        Option<extern "C" fn(uuid_ptr: *const u8, action: u8, x: i32, y: i32, z: i32) -> u8>,
    /// Called on entity damage; return adjusted damage (`<= 0.0` cancels).
    pub on_entity_damage: Option<extern "C" fn(uuid_ptr: *const u8, amount: f64) -> f64>,
    /// Called once per server tick.
    pub on_tick: Option<extern "C" fn()>,
    /// Called on player death; return `1` to keep inventory or `0` to use default.
    pub on_player_death: Option<extern "C" fn(uuid_ptr: *const u8) -> u8>,
    /// Called on player respawn.
    pub on_player_respawn: Option<extern "C" fn(uuid_ptr: *const u8, x: f64, y: f64, z: f64)>,
    /// Called on inventory click; return `1` to allow or `0` to cancel.
    pub on_inventory_click: Option<extern "C" fn(uuid_ptr: *const u8, slot: i32) -> u8>,
}

unsafe impl Send for FerritePluginVTableV2 {}
unsafe impl Sync for FerritePluginVTableV2 {}

fn read_uuid_ptr(uuid_ptr: *const u8) -> Option<Uuid> {
    if uuid_ptr.is_null() {
        return None;
    }
    // SAFETY: Caller passes a valid pointer to 16 big-endian UUID bytes.
    let raw = unsafe { slice::from_raw_parts(uuid_ptr, 16) };
    let bytes: [u8; 16] = raw.try_into().ok()?;
    Some(Uuid::from_bytes(bytes))
}

fn read_bounded_utf8<'a>(ptr: *const u8, len: usize, max_len: usize) -> Option<&'a str> {
    if ptr.is_null() || len == 0 || len > max_len {
        return None;
    }
    // SAFETY: Caller passes a valid UTF-8 byte slice for the duration of the callback.
    let bytes = unsafe { slice::from_raw_parts(ptr, len) };
    std::str::from_utf8(bytes).ok()
}

extern "C" fn host_callback_log(level: u8, msg_ptr: *const u8, msg_len: usize) {
    if msg_ptr.is_null() || msg_len == 0 || msg_len > 16_384 {
        return;
    }
    // SAFETY: Caller passes a valid UTF-8 slice for the duration of the callback.
    let bytes = unsafe { slice::from_raw_parts(msg_ptr, msg_len) };
    let msg = String::from_utf8_lossy(bytes);
    match level {
        0 => tracing::trace!(target: "ferrite_dynamic_plugin", "{msg}"),
        1 => tracing::debug!(target: "ferrite_dynamic_plugin", "{msg}"),
        2 => tracing::info!(target: "ferrite_dynamic_plugin", "{msg}"),
        3 => tracing::warn!(target: "ferrite_dynamic_plugin", "{msg}"),
        _ => tracing::error!(target: "ferrite_dynamic_plugin", "{msg}"),
    }
}

extern "C" fn host_callback_send_message(uuid_ptr: *const u8, msg_ptr: *const u8, msg_len: usize) {
    let Some(uuid) = read_uuid_ptr(uuid_ptr) else {
        return;
    };
    let Some(msg) = read_bounded_utf8(msg_ptr, msg_len, 16_384) else {
        return;
    };
    let bridge = HOST_BRIDGE
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    if let Some(bridge) = bridge {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            bridge.send_message(uuid, msg);
        }));
    }
}

extern "C" fn host_callback_broadcast(msg_ptr: *const u8, msg_len: usize) {
    let Some(msg) = read_bounded_utf8(msg_ptr, msg_len, 16_384) else {
        return;
    };
    let bridge = HOST_BRIDGE
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    if let Some(bridge) = bridge {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            bridge.broadcast(msg);
        }));
    }
}

extern "C" fn host_callback_send_action_bar(
    uuid_ptr: *const u8,
    msg_ptr: *const u8,
    msg_len: usize,
) {
    let Some(uuid) = read_uuid_ptr(uuid_ptr) else {
        return;
    };
    let Some(msg) = read_bounded_utf8(msg_ptr, msg_len, 4_096) else {
        return;
    };
    let bridge = HOST_BRIDGE
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    if let Some(bridge) = bridge {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            bridge.send_action_bar(uuid, msg);
        }));
    }
}

extern "C" fn host_callback_teleport(
    uuid_ptr: *const u8,
    world_ptr: *const u8,
    world_len: usize,
    x: f64,
    y: f64,
    z: f64,
    yaw: f32,
    pitch: f32,
) {
    let Some(uuid) = read_uuid_ptr(uuid_ptr) else {
        return;
    };
    let world = read_bounded_utf8(world_ptr, world_len, 256).unwrap_or("minecraft:overworld");
    let bridge = HOST_BRIDGE
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    if let Some(bridge) = bridge {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            bridge.teleport(uuid, world, x, y, z, yaw, pitch);
        }));
    }
}

extern "C" fn host_callback_get_block(
    world_ptr: *const u8,
    world_len: usize,
    x: i32,
    y: i32,
    z: i32,
) -> i32 {
    let world = read_bounded_utf8(world_ptr, world_len, 256).unwrap_or("minecraft:overworld");
    let bridge = HOST_BRIDGE
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    let Some(bridge) = bridge else {
        return -1;
    };
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        bridge
            .get_block(world, x, y, z)
            .and_then(|s| i32::try_from(s).ok())
            .unwrap_or(-1)
    }))
    .unwrap_or(-1)
}

extern "C" fn host_callback_set_block(
    world_ptr: *const u8,
    world_len: usize,
    x: i32,
    y: i32,
    z: i32,
    state_id: u32,
) -> u8 {
    let world = read_bounded_utf8(world_ptr, world_len, 256).unwrap_or("minecraft:overworld");
    let bridge = HOST_BRIDGE
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    let Some(bridge) = bridge else {
        return 0;
    };
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        u8::from(bridge.set_block(world, x, y, z, state_id))
    }))
    .unwrap_or(0)
}

extern "C" fn host_callback_send_title(
    uuid_ptr: *const u8,
    title_ptr: *const u8,
    title_len: usize,
    sub_ptr: *const u8,
    sub_len: usize,
    fade_in: i32,
    stay: i32,
    fade_out: i32,
) {
    let Some(uuid) = read_uuid_ptr(uuid_ptr) else {
        return;
    };
    let title = read_bounded_utf8(title_ptr, title_len, 4_096).unwrap_or("");
    let subtitle = read_bounded_utf8(sub_ptr, sub_len, 4_096).unwrap_or("");
    let bridge = HOST_BRIDGE
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    if let Some(bridge) = bridge {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            bridge.send_title(uuid, title, subtitle, fade_in, stay, fade_out);
        }));
    }
}

extern "C" fn host_callback_play_sound(
    uuid_ptr: *const u8,
    sound_ptr: *const u8,
    sound_len: usize,
    x: f64,
    y: f64,
    z: f64,
    volume: f32,
    pitch: f32,
) {
    let Some(uuid) = read_uuid_ptr(uuid_ptr) else {
        return;
    };
    let Some(sound) = read_bounded_utf8(sound_ptr, sound_len, 256) else {
        return;
    };
    let bridge = HOST_BRIDGE
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    if let Some(bridge) = bridge {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            bridge.play_sound(uuid, sound, x, y, z, volume, pitch);
        }));
    }
}

extern "C" fn host_callback_kick_player(
    uuid_ptr: *const u8,
    reason_ptr: *const u8,
    reason_len: usize,
) {
    let Some(uuid) = read_uuid_ptr(uuid_ptr) else {
        return;
    };
    let reason = read_bounded_utf8(reason_ptr, reason_len, 4_096).unwrap_or("Kicked by plugin");
    let bridge = HOST_BRIDGE
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    if let Some(bridge) = bridge {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            bridge.kick_player(uuid, reason);
        }));
    }
}

extern "C" fn host_callback_set_gamemode(uuid_ptr: *const u8, mode: u8) {
    let Some(uuid) = read_uuid_ptr(uuid_ptr) else {
        return;
    };
    let bridge = HOST_BRIDGE
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    if let Some(bridge) = bridge {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            bridge.set_gamemode(uuid, mode);
        }));
    }
}

extern "C" fn host_callback_perform_command(
    uuid_ptr: *const u8,
    cmd_ptr: *const u8,
    cmd_len: usize,
) {
    let Some(uuid) = read_uuid_ptr(uuid_ptr) else {
        return;
    };
    let Some(cmd) = read_bounded_utf8(cmd_ptr, cmd_len, 4_096) else {
        return;
    };
    let bridge = HOST_BRIDGE
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    if let Some(bridge) = bridge {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            bridge.perform_command(uuid, cmd);
        }));
    }
}

static HOST_CALLBACKS: FerriteHostCallbacks = FerriteHostCallbacks {
    log: host_callback_log,
    send_message: host_callback_send_message,
    broadcast: host_callback_broadcast,
    send_action_bar: host_callback_send_action_bar,
    teleport: host_callback_teleport,
    get_block: host_callback_get_block,
    set_block: host_callback_set_block,
    send_title: host_callback_send_title,
    play_sound: host_callback_play_sound,
    kick_player: host_callback_kick_player,
    set_gamemode: host_callback_set_gamemode,
    perform_command: host_callback_perform_command,
};

/// Metadata describing one loaded dynamic native plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedDynamicPlugin {
    pub name: String,
    pub version: String,
    pub path: PathBuf,
}

/// Detailed diagnostic info for one loaded dynamic native plugin (`/dynplugins info <name>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicPluginInfo {
    pub name: String,
    pub version: String,
    pub path: PathBuf,
    pub abi_v2: bool,
    pub hooks: Vec<&'static str>,
}

/// Summary returned by [`DynamicPluginManager::reload_dir`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReloadSummary {
    pub loaded: Vec<String>,
    pub reloaded: Vec<String>,
    pub unloaded: Vec<String>,
    pub failed: Vec<String>,
}

struct DynamicPluginInstance {
    meta: LoadedDynamicPlugin,
    vtable: FerritePluginVTable,
    vtable_v2: Option<FerritePluginVTableV2>,
    source_mtime: Option<SystemTime>,
    staged_path: Option<PathBuf>,
    disabled: bool,
    // Kept alive until after `on_disable` runs; field order ensures drop runs before file cleanup.
    library: Option<Library>,
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
        // Close library handle before deleting the staged shadow copy.
        drop(self.library.take());
        if let Some(staged) = self.staged_path.take() {
            let _ = std::fs::remove_file(staged);
        }
    }
}

/// Manages zero or more dynamically loaded C-ABI native plugins with runtime hot-reload.
pub struct DynamicPluginManager {
    watch_dir: Mutex<Option<PathBuf>>,
    staging_dir: PathBuf,
    plugins: Mutex<Vec<DynamicPluginInstance>>,
}

impl Default for DynamicPluginManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DynamicPluginManager {
    /// Creates an empty manager.
    pub fn new() -> Self {
        let staging_dir =
            std::env::temp_dir().join(format!("ferrite-dyn-stage-{}", std::process::id()));
        Self {
            watch_dir: Mutex::new(None),
            staging_dir,
            plugins: Mutex::new(Vec::new()),
        }
    }

    /// Scans `dir` for dynamic libraries matching the host platform extension (`.so`, `.dll`,
    /// `.dylib`), records `dir` as the hot-reload watch directory, and loads every valid plugin.
    pub fn load_dir(dir: &Path) -> Self {
        let manager = Self::new();
        *manager.watch_dir.lock().unwrap_or_else(|p| p.into_inner()) = Some(dir.to_path_buf());
        let summary = manager.reload_dir();
        if !summary.failed.is_empty() {
            for err in summary.failed {
                tracing::warn!(error = %err, "skipping dynamic native plugin");
            }
        }
        manager
    }

    /// Returns the configured hot-reload watch directory, if any.
    pub fn watch_dir(&self) -> Option<PathBuf> {
        self.watch_dir
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Shadow-copies `path` into a unique staging file and loads it, invoking `on_enable`.
    ///
    /// Shadow-copying prevents `dlopen` / `LoadLibrary` path caching and avoids file locking on
    /// Windows so the original `.so`/`.dll` can be overwritten and hot-reloaded at any time.
    pub fn load_library(&self, path: &Path) -> Result<LoadedDynamicPlugin, DynamicLoadError> {
        let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let source_mtime = file_mtime(&canonical);
        let staged_path = self.stage_copy(&canonical)?;

        // SAFETY: We validate the exported symbol signature, non-null vtable pointer, and ABI
        // version before calling any function pointer, and retain `Library` for the lifetime of
        // the instance.
        let library = unsafe {
            Library::new(&staged_path).map_err(|source| {
                let _ = std::fs::remove_file(&staged_path);
                DynamicLoadError::Open {
                    path: canonical.clone(),
                    source,
                }
            })?
        };
        let vtable = unsafe {
            let entry: Symbol<'_, unsafe extern "C" fn() -> *const FerritePluginVTable> = library
                .get(ENTRY_SYMBOL)
                .map_err(|source| DynamicLoadError::MissingEntry {
                    path: canonical.clone(),
                    source,
                })?;
            let ptr = entry();
            if ptr.is_null() {
                return Err(DynamicLoadError::NullVTable {
                    path: canonical.clone(),
                });
            }
            *ptr
        };
        if vtable.abi_version != FERRITE_NATIVE_ABI_VERSION {
            return Err(DynamicLoadError::AbiMismatch {
                path: canonical.clone(),
                expected: FERRITE_NATIVE_ABI_VERSION,
                found: vtable.abi_version,
            });
        }
        let name = read_utf8(vtable.name_ptr, vtable.name_len).unwrap_or_else(|| {
            canonical
                .file_stem()
                .and_then(OsStr::to_str)
                .unwrap_or("unknown")
                .to_owned()
        });
        let version =
            read_utf8(vtable.version_ptr, vtable.version_len).unwrap_or_else(|| "0.0.0".to_owned());

        let vtable_v2 = unsafe {
            library
                .get::<unsafe extern "C" fn() -> *const FerritePluginVTableV2>(ENTRY_SYMBOL_V2)
                .ok()
                .and_then(|entry| {
                    let ptr = entry();
                    if ptr.is_null() {
                        None
                    } else {
                        let v2 = *ptr;
                        (v2.abi_version == FERRITE_NATIVE_ABI_V2_VERSION).then_some(v2)
                    }
                })
        };

        // If a plugin with the same name is already loaded, disable and replace it atomically.
        self.unload_by_name(&name);

        if let Some(v2) = &vtable_v2
            && let Some(on_host_ready) = v2.on_host_ready
        {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                on_host_ready(&HOST_CALLBACKS);
            }));
        }
        if let Some(on_enable) = vtable.on_enable {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| on_enable()));
        }
        let meta = LoadedDynamicPlugin {
            name,
            version,
            path: canonical,
        };
        tracing::info!(
            plugin = %meta.name,
            version = %meta.version,
            path = %meta.path.display(),
            abi_v2 = vtable_v2.is_some(),
            "loaded dynamic native plugin"
        );
        self.plugins
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(DynamicPluginInstance {
                meta: meta.clone(),
                vtable,
                vtable_v2,
                source_mtime,
                staged_path: Some(staged_path),
                disabled: false,
                library: Some(library),
            });
        Ok(meta)
    }

    fn stage_copy(&self, source: &Path) -> Result<PathBuf, DynamicLoadError> {
        std::fs::create_dir_all(&self.staging_dir).map_err(|e| DynamicLoadError::Stage {
            path: source.to_path_buf(),
            source: e,
        })?;
        let seq = STAGE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let stem = source
            .file_stem()
            .and_then(OsStr::to_str)
            .unwrap_or("plugin");
        let ext = source.extension().and_then(OsStr::to_str).unwrap_or("so");
        let staged = self.staging_dir.join(format!("{stem}-{seq}.{ext}"));
        std::fs::copy(source, &staged).map_err(|e| DynamicLoadError::Stage {
            path: source.to_path_buf(),
            source: e,
        })?;
        Ok(staged)
    }

    /// Unloads a dynamic plugin by name (case-insensitive), running `on_disable` first.
    pub fn unload_by_name(&self, name: &str) -> bool {
        let mut guard = self.plugins.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(idx) = guard
            .iter()
            .position(|p| p.meta.name.eq_ignore_ascii_case(name))
        {
            let removed = guard.remove(idx);
            let plugin_name = removed.meta.name.clone();
            drop(removed);
            tracing::info!(plugin = %plugin_name, "unloaded dynamic native plugin");
            true
        } else {
            false
        }
    }

    /// Re-scans the configured watch directory for new, modified, or deleted dynamic libraries
    /// and hot-loads / hot-reloads / hot-unloads them live without restarting the server.
    pub fn reload_dir(&self) -> ReloadSummary {
        let mut summary = ReloadSummary::default();
        let Some(dir) = self.watch_dir() else {
            return summary;
        };
        let mut on_disk: HashMap<PathBuf, Option<SystemTime>> = HashMap::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            let mut paths: Vec<PathBuf> = entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_file() && is_dynamic_library(p))
                .collect();
            paths.sort();
            for p in paths {
                let canon = std::fs::canonicalize(&p).unwrap_or(p);
                let mtime = file_mtime(&canon);
                on_disk.insert(canon, mtime);
            }
        }

        // 1) Identify removed or modified loaded plugins.
        let mut to_unload: Vec<String> = Vec::new();
        let mut to_reload: Vec<PathBuf> = Vec::new();
        let mut unchanged: HashSet<PathBuf> = HashSet::new();
        {
            let guard = self.plugins.lock().unwrap_or_else(|p| p.into_inner());
            for inst in guard.iter() {
                if inst.library.is_none() {
                    continue;
                }
                match on_disk.get(&inst.meta.path) {
                    None => to_unload.push(inst.meta.name.clone()),
                    Some(&disk_mtime) => {
                        if disk_mtime != inst.source_mtime {
                            to_reload.push(inst.meta.path.clone());
                        } else {
                            unchanged.insert(inst.meta.path.clone());
                        }
                    }
                }
            }
        }

        for name in to_unload {
            if self.unload_by_name(&name) {
                summary.unloaded.push(name);
            }
        }

        // 2) Reload modified libraries (sorted deterministically).
        to_reload.sort();
        for path in &to_reload {
            match self.load_library(path) {
                Ok(meta) => summary.reloaded.push(meta.name),
                Err(err) => summary.failed.push(err.to_string()),
            }
        }

        // 3) Load newly added libraries.
        let mut new_paths: Vec<PathBuf> = on_disk
            .keys()
            .filter(|p| !unchanged.contains(*p) && !to_reload.contains(*p))
            .cloned()
            .collect();
        new_paths.sort();
        for path in new_paths {
            match self.load_library(&path) {
                Ok(meta) => summary.loaded.push(meta.name),
                Err(err) => summary.failed.push(err.to_string()),
            }
        }

        summary
    }

    /// Lightweight mtime check returning `true` only when a dynamic library was added, modified,
    /// or removed in the watch directory.
    pub fn has_changes_in_watch_dir(&self) -> bool {
        let Some(dir) = self.watch_dir() else {
            return false;
        };
        let mut on_disk: HashMap<PathBuf, Option<SystemTime>> = HashMap::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() && is_dynamic_library(&p) {
                    let canon = std::fs::canonicalize(&p).unwrap_or(p);
                    let mtime = file_mtime(&canon);
                    on_disk.insert(canon, mtime);
                }
            }
        }
        let guard = self.plugins.lock().unwrap_or_else(|p| p.into_inner());
        let mut loaded_count = 0usize;
        for inst in guard.iter() {
            if inst.library.is_none() {
                continue;
            }
            loaded_count += 1;
            match on_disk.get(&inst.meta.path) {
                None => return true,
                Some(&mtime) if mtime != inst.source_mtime => return true,
                _ => {}
            }
        }
        on_disk.len() != loaded_count
    }

    /// Registers an in-process vtable directly (useful for static extensions and unit tests).
    pub fn register_static_vtable(
        &self,
        vtable: FerritePluginVTable,
    ) -> Result<LoadedDynamicPlugin, DynamicLoadError> {
        self.register_static_vtable_with_v2(vtable, None)
    }

    /// Registers an in-process v1 + optional v2 vtable directly.
    pub fn register_static_vtable_with_v2(
        &self,
        vtable: FerritePluginVTable,
        vtable_v2: Option<FerritePluginVTableV2>,
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
        if let Some(v2) = &vtable_v2
            && let Some(on_host_ready) = v2.on_host_ready
        {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                on_host_ready(&HOST_CALLBACKS);
            }));
        }
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
                vtable_v2,
                source_mtime: None,
                staged_path: None,
                disabled: false,
                library: None,
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

    /// Returns detailed diagnostic metadata for a loaded plugin by name (case-insensitive).
    pub fn plugin_info(&self, name: &str) -> Option<DynamicPluginInfo> {
        let guard = self.plugins.lock().unwrap_or_else(|p| p.into_inner());
        let inst = guard
            .iter()
            .find(|p| p.meta.name.eq_ignore_ascii_case(name))?;
        let mut hooks = Vec::new();
        if inst.vtable.on_enable.is_some() {
            hooks.push("on_enable");
        }
        if inst.vtable.on_disable.is_some() {
            hooks.push("on_disable");
        }
        if inst.vtable.on_player_join.is_some() {
            hooks.push("on_player_join");
        }
        if inst.vtable.on_player_quit.is_some() {
            hooks.push("on_player_quit");
        }
        if inst.vtable.on_chat.is_some() {
            hooks.push("on_chat");
        }
        if inst.vtable.on_block_break.is_some() {
            hooks.push("on_block_break");
        }
        if inst.vtable.on_block_place.is_some() {
            hooks.push("on_block_place");
        }
        if inst.vtable.on_command.is_some() {
            hooks.push("on_command");
        }
        if let Some(v2) = &inst.vtable_v2 {
            if v2.on_host_ready.is_some() {
                hooks.push("on_host_ready");
            }
            if v2.on_player_move.is_some() {
                hooks.push("on_player_move");
            }
            if v2.on_player_interact.is_some() {
                hooks.push("on_player_interact");
            }
            if v2.on_entity_damage.is_some() {
                hooks.push("on_entity_damage");
            }
            if v2.on_tick.is_some() {
                hooks.push("on_tick");
            }
            if v2.on_player_death.is_some() {
                hooks.push("on_player_death");
            }
            if v2.on_player_respawn.is_some() {
                hooks.push("on_player_respawn");
            }
            if v2.on_inventory_click.is_some() {
                hooks.push("on_inventory_click");
            }
        }
        Some(DynamicPluginInfo {
            name: inst.meta.name.clone(),
            version: inst.meta.version.clone(),
            path: inst.meta.path.clone(),
            abi_v2: inst.vtable_v2.is_some(),
            hooks,
        })
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

    /// Dispatches player movement (C-ABI v2); returns `false` if any plugin cancels.
    pub fn dispatch_player_move(&self, uuid: Uuid, from: [f64; 3], to: [f64; 3]) -> bool {
        let bytes = uuid.into_bytes();
        let guard = self.plugins.lock().unwrap_or_else(|p| p.into_inner());
        for plugin in guard.iter() {
            if let Some(v2) = &plugin.vtable_v2
                && let Some(f) = v2.on_player_move
            {
                let allowed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    f(
                        bytes.as_ptr(),
                        from[0],
                        from[1],
                        from[2],
                        to[0],
                        to[1],
                        to[2],
                    ) != 0
                }))
                .unwrap_or(true);
                if !allowed {
                    return false;
                }
            }
        }
        true
    }

    /// Dispatches player block interaction (C-ABI v2); returns `false` if cancelled.
    pub fn dispatch_player_interact(&self, uuid: Uuid, action: u8, x: i32, y: i32, z: i32) -> bool {
        let bytes = uuid.into_bytes();
        let guard = self.plugins.lock().unwrap_or_else(|p| p.into_inner());
        for plugin in guard.iter() {
            if let Some(v2) = &plugin.vtable_v2
                && let Some(f) = v2.on_player_interact
            {
                let allowed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    f(bytes.as_ptr(), action, x, y, z) != 0
                }))
                .unwrap_or(true);
                if !allowed {
                    return false;
                }
            }
        }
        true
    }

    /// Dispatches entity damage (C-ABI v2); returns adjusted damage amount.
    pub fn dispatch_entity_damage(&self, uuid: Uuid, mut amount: f64) -> f64 {
        let bytes = uuid.into_bytes();
        let guard = self.plugins.lock().unwrap_or_else(|p| p.into_inner());
        for plugin in guard.iter() {
            if let Some(v2) = &plugin.vtable_v2
                && let Some(f) = v2.on_entity_damage
            {
                amount = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    f(bytes.as_ptr(), amount)
                }))
                .unwrap_or(amount);
                if amount <= 0.0 {
                    return 0.0;
                }
            }
        }
        amount
    }

    /// Dispatches one server tick to C-ABI v2 plugins.
    pub fn dispatch_tick(&self) {
        let guard = self.plugins.lock().unwrap_or_else(|p| p.into_inner());
        for plugin in guard.iter() {
            if let Some(v2) = &plugin.vtable_v2
                && let Some(f) = v2.on_tick
            {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f()));
            }
        }
    }

    /// Dispatches player death to C-ABI v2 plugins; returns `true` if any plugin requests keep-inventory.
    pub fn dispatch_player_death(&self, uuid: Uuid) -> bool {
        let bytes = uuid.into_bytes();
        let guard = self.plugins.lock().unwrap_or_else(|p| p.into_inner());
        let mut keep = false;
        for plugin in guard.iter() {
            if let Some(v2) = &plugin.vtable_v2
                && let Some(f) = v2.on_player_death
            {
                let req = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    f(bytes.as_ptr()) != 0
                }))
                .unwrap_or(false);
                keep |= req;
            }
        }
        keep
    }

    /// Dispatches player respawn to C-ABI v2 plugins.
    pub fn dispatch_player_respawn(&self, uuid: Uuid, pos: [f64; 3]) {
        let bytes = uuid.into_bytes();
        let guard = self.plugins.lock().unwrap_or_else(|p| p.into_inner());
        for plugin in guard.iter() {
            if let Some(v2) = &plugin.vtable_v2
                && let Some(f) = v2.on_player_respawn
            {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    f(bytes.as_ptr(), pos[0], pos[1], pos[2]);
                }));
            }
        }
    }

    /// Dispatches inventory click to C-ABI v2 plugins; returns `false` if any plugin cancels.
    pub fn dispatch_inventory_click(&self, uuid: Uuid, slot: i32) -> bool {
        let bytes = uuid.into_bytes();
        let guard = self.plugins.lock().unwrap_or_else(|p| p.into_inner());
        for plugin in guard.iter() {
            if let Some(v2) = &plugin.vtable_v2
                && let Some(f) = v2.on_inventory_click
            {
                let allowed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    f(bytes.as_ptr(), slot) != 0
                }))
                .unwrap_or(true);
                if !allowed {
                    return false;
                }
            }
        }
        true
    }

    /// Calls `on_disable` on every loaded dynamic plugin in reverse load order.
    pub fn shutdown(&self) {
        let mut guard = self.plugins.lock().unwrap_or_else(|p| p.into_inner());
        for plugin in guard.iter_mut().rev() {
            plugin.disable_once();
        }
    }
}

fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
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
    use std::process::Command;
    use std::sync::atomic::AtomicBool;

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

    fn cdylib_source(version: &str, reply: &str) -> String {
        format!(
            r#"
#[repr(C)]
pub struct FerritePluginVTable {{
    pub abi_version: u32,
    pub name_ptr: *const u8,
    pub name_len: usize,
    pub version_ptr: *const u8,
    pub version_len: usize,
    pub on_enable: Option<extern "C" fn()>,
    pub on_disable: Option<extern "C" fn()>,
    pub on_player_join: Option<extern "C" fn(*const u8, *const u8, usize)>,
    pub on_player_quit: Option<extern "C" fn(*const u8, *const u8, usize)>,
    pub on_chat: Option<extern "C" fn(*const u8, *const u8, usize) -> u8>,
    pub on_block_break: Option<extern "C" fn(*const u8, i32, i32, i32) -> u8>,
    pub on_block_place: Option<extern "C" fn(*const u8, i32, i32, i32) -> u8>,
    pub on_command: Option<extern "C" fn(*const u8, *const u8, usize, *mut u8, usize) -> isize>,
}}
unsafe impl Sync for FerritePluginVTable {{}}

const NAME: &[u8] = b"HotTest";
const VER: &[u8] = b"{version}";
const REPLY: &[u8] = b"{reply}";

extern "C" fn on_cmd(_u: *const u8, line_ptr: *const u8, line_len: usize, out: *mut u8, cap: usize) -> isize {{
    let line = unsafe {{ std::slice::from_raw_parts(line_ptr, line_len) }};
    if line == b"/hotping" {{
        let n = REPLY.len().min(cap);
        unsafe {{ std::ptr::copy_nonoverlapping(REPLY.as_ptr(), out, n); }}
        n as isize
    }} else {{
        -1
    }}
}}

static VTABLE: FerritePluginVTable = FerritePluginVTable {{
    abi_version: 1,
    name_ptr: NAME.as_ptr(),
    name_len: NAME.len(),
    version_ptr: VER.as_ptr(),
    version_len: VER.len(),
    on_enable: None,
    on_disable: None,
    on_player_join: None,
    on_player_quit: None,
    on_chat: None,
    on_block_break: None,
    on_block_place: None,
    on_command: Some(on_cmd),
}};

#[unsafe(no_mangle)]
pub extern "C" fn ferrite_plugin_entry() -> *const FerritePluginVTable {{
    &VTABLE
}}
"#
        )
    }

    #[test]
    fn real_cdylib_compiles_loads_hot_reloads_and_unloads_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("hot_test.rs");
        let ext = if cfg!(target_os = "windows") {
            "dll"
        } else if cfg!(target_os = "macos") {
            "dylib"
        } else {
            "so"
        };
        let lib_path = dir.path().join(format!("libhot_test.{ext}"));

        // Compile v1.
        std::fs::write(&src, cdylib_source("1.0.0", "reply-v1")).unwrap();
        let status = Command::new("rustc")
            .args(["--edition=2024", "--crate-type=cdylib", "-o"])
            .arg(&lib_path)
            .arg(&src)
            .status()
            .expect("invoke rustc");
        assert!(status.success(), "rustc v1 failed");

        let manager = DynamicPluginManager::load_dir(dir.path());
        assert_eq!(manager.loaded_plugins().len(), 1);
        assert_eq!(manager.loaded_plugins()[0].version, "1.0.0");
        assert_eq!(
            manager.dispatch_command(Uuid::nil(), "/hotping").as_deref(),
            Some("reply-v1")
        );

        // Sleep briefly to guarantee filesystem mtime advances, then compile v2 in place.
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(&src, cdylib_source("2.0.0", "reply-v2")).unwrap();
        let status2 = Command::new("rustc")
            .args(["--edition=2024", "--crate-type=cdylib", "-o"])
            .arg(&lib_path)
            .arg(&src)
            .status()
            .expect("invoke rustc v2");
        assert!(status2.success(), "rustc v2 failed");

        let reloaded = manager.reload_dir();
        assert_eq!(reloaded.reloaded, vec!["HotTest".to_owned()]);
        assert_eq!(manager.loaded_plugins()[0].version, "2.0.0");
        assert_eq!(
            manager.dispatch_command(Uuid::nil(), "/hotping").as_deref(),
            Some("reply-v2")
        );

        // Hot-unload by name.
        assert!(manager.unload_by_name("HotTest"));
        assert!(manager.is_empty());
        assert_eq!(manager.dispatch_command(Uuid::nil(), "/hotping"), None);
    }

    #[test]
    fn abi_v2_hooks_and_host_callbacks_work() {
        static HOST_READY: AtomicBool = AtomicBool::new(false);
        static TICKS: AtomicU64 = AtomicU64::new(0);

        extern "C" fn on_host_ready(cb: *const FerriteHostCallbacks) {
            if !cb.is_null() {
                HOST_READY.store(true, Ordering::SeqCst);
                let msg = b"v2 ready";
                unsafe { ((*cb).log)(2, msg.as_ptr(), msg.len()) };
            }
        }
        extern "C" fn on_move(
            _uuid: *const u8,
            _fx: f64,
            _fy: f64,
            _fz: f64,
            tx: f64,
            _ty: f64,
            _tz: f64,
        ) -> u8 {
            if tx > 100.0 { 0 } else { 1 }
        }
        extern "C" fn on_interact(_uuid: *const u8, action: u8, _x: i32, _y: i32, _z: i32) -> u8 {
            if action == 1 { 0 } else { 1 }
        }
        extern "C" fn on_damage(_uuid: *const u8, amount: f64) -> f64 {
            amount * 0.5
        }
        extern "C" fn on_tick() {
            TICKS.fetch_add(1, Ordering::SeqCst);
        }

        static NAME: &str = "v2_demo";
        static VER: &str = "2.0.0";
        let v1 = FerritePluginVTable {
            abi_version: FERRITE_NATIVE_ABI_VERSION,
            name_ptr: NAME.as_ptr(),
            name_len: NAME.len(),
            version_ptr: VER.as_ptr(),
            version_len: VER.len(),
            on_enable: None,
            on_disable: None,
            on_player_join: None,
            on_player_quit: None,
            on_chat: None,
            on_block_break: None,
            on_block_place: None,
            on_command: None,
        };
        let v2 = FerritePluginVTableV2 {
            abi_version: FERRITE_NATIVE_ABI_V2_VERSION,
            on_host_ready: Some(on_host_ready),
            on_player_move: Some(on_move),
            on_player_interact: Some(on_interact),
            on_entity_damage: Some(on_damage),
            on_tick: Some(on_tick),
            on_player_death: None,
            on_player_respawn: None,
            on_inventory_click: None,
        };
        let manager = DynamicPluginManager::new();
        manager
            .register_static_vtable_with_v2(v1, Some(v2))
            .unwrap();
        assert!(HOST_READY.load(Ordering::SeqCst));
        assert!(manager.dispatch_player_move(Uuid::nil(), [0.0, 64.0, 0.0], [10.0, 64.0, 0.0]));
        assert!(!manager.dispatch_player_move(Uuid::nil(), [0.0, 64.0, 0.0], [150.0, 64.0, 0.0]));
        assert!(manager.dispatch_player_interact(Uuid::nil(), 0, 1, 64, 1));
        assert!(!manager.dispatch_player_interact(Uuid::nil(), 1, 1, 64, 1));
        assert_eq!(manager.dispatch_entity_damage(Uuid::nil(), 8.0), 4.0);
        manager.dispatch_tick();
        manager.dispatch_tick();
        assert_eq!(TICKS.load(Ordering::SeqCst), 2);
        let info = manager.plugin_info("v2_demo").expect("plugin info");
        assert!(info.abi_v2);
        assert!(info.hooks.contains(&"on_host_ready"));
        assert!(info.hooks.contains(&"on_tick"));
    }

    #[test]
    fn c_abi_v2_host_callbacks_forward_to_bridge() {
        use std::sync::Mutex as StdMutex;

        struct RecordingBridge {
            messages: StdMutex<Vec<(Uuid, String)>>,
            broadcasts: StdMutex<Vec<String>>,
            action_bars: StdMutex<Vec<(Uuid, String)>>,
            teleports: StdMutex<Vec<(Uuid, String, [f64; 3])>>,
            blocks: StdMutex<HashMap<(String, i32, i32, i32), u32>>,
        }
        impl DynamicHostBridge for RecordingBridge {
            fn send_message(&self, player: Uuid, message: &str) {
                self.messages
                    .lock()
                    .unwrap()
                    .push((player, message.to_owned()));
            }
            fn broadcast(&self, message: &str) {
                self.broadcasts.lock().unwrap().push(message.to_owned());
            }
            fn send_action_bar(&self, player: Uuid, message: &str) {
                self.action_bars
                    .lock()
                    .unwrap()
                    .push((player, message.to_owned()));
            }
            fn teleport(
                &self,
                player: Uuid,
                world: &str,
                x: f64,
                y: f64,
                z: f64,
                _yaw: f32,
                _pitch: f32,
            ) {
                self.teleports
                    .lock()
                    .unwrap()
                    .push((player, world.to_owned(), [x, y, z]));
            }
            fn get_block(&self, world: &str, x: i32, y: i32, z: i32) -> Option<u32> {
                self.blocks
                    .lock()
                    .unwrap()
                    .get(&(world.to_owned(), x, y, z))
                    .copied()
            }
            fn set_block(&self, world: &str, x: i32, y: i32, z: i32, state_id: u32) -> bool {
                self.blocks
                    .lock()
                    .unwrap()
                    .insert((world.to_owned(), x, y, z), state_id);
                true
            }
        }

        let bridge = Arc::new(RecordingBridge {
            messages: StdMutex::new(Vec::new()),
            broadcasts: StdMutex::new(Vec::new()),
            action_bars: StdMutex::new(Vec::new()),
            teleports: StdMutex::new(Vec::new()),
            blocks: StdMutex::new(HashMap::new()),
        });
        install_host_bridge(Arc::clone(&bridge) as Arc<dyn DynamicHostBridge>);

        let player = Uuid::from_u128(0x1234_5678);
        let uuid_bytes = player.into_bytes();
        let msg = b"hello player";
        let bc = b"server wide";
        let bar = b"HP 20/20";
        let world = b"minecraft:overworld";

        (HOST_CALLBACKS.send_message)(uuid_bytes.as_ptr(), msg.as_ptr(), msg.len());
        (HOST_CALLBACKS.broadcast)(bc.as_ptr(), bc.len());
        (HOST_CALLBACKS.send_action_bar)(uuid_bytes.as_ptr(), bar.as_ptr(), bar.len());
        (HOST_CALLBACKS.teleport)(
            uuid_bytes.as_ptr(),
            world.as_ptr(),
            world.len(),
            10.5,
            70.0,
            -4.5,
            90.0,
            0.0,
        );
        assert_eq!(
            (HOST_CALLBACKS.get_block)(world.as_ptr(), world.len(), 1, 64, 2),
            -1
        );
        assert_eq!(
            (HOST_CALLBACKS.set_block)(world.as_ptr(), world.len(), 1, 64, 2, 42),
            1
        );
        assert_eq!(
            (HOST_CALLBACKS.get_block)(world.as_ptr(), world.len(), 1, 64, 2),
            42
        );

        assert_eq!(
            bridge.messages.lock().unwrap().as_slice(),
            &[(player, "hello player".to_owned())]
        );
        assert_eq!(
            bridge.broadcasts.lock().unwrap().as_slice(),
            &["server wide".to_owned()]
        );
        assert_eq!(
            bridge.action_bars.lock().unwrap().as_slice(),
            &[(player, "HP 20/20".to_owned())]
        );
        assert_eq!(
            bridge.teleports.lock().unwrap().as_slice(),
            &[(player, "minecraft:overworld".to_owned(), [10.5, 70.0, -4.5])]
        );

        clear_host_bridge();
    }
}
