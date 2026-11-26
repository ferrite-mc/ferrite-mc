//! Generates a ready-to-compile Rust native Ferrite plugin scaffold from a `.jar` plugin.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use ferrite_plugin_meta::PluginMeta;

use crate::compat::{CompatReport, ConvertError};

fn to_snake_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    let mut prev_sep = true;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            if ch.is_ascii_uppercase() && !prev_sep {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
            prev_sep = false;
        } else if !prev_sep {
            out.push('_');
            prev_sep = true;
        }
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        "ferrite_plugin".to_owned()
    } else if trimmed.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("plugin_{trimmed}")
    } else {
        trimmed.to_owned()
    }
}

fn to_pascal_case(name: &str) -> String {
    let mut out = String::new();
    for part in name.split(|c: char| !c.is_ascii_alphanumeric()) {
        if part.is_empty() {
            continue;
        }
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.extend(chars);
        }
    }
    if out.is_empty() {
        "FerritePlugin".to_owned()
    } else {
        out
    }
}

fn infer_config_struct(config_yml: Option<&str>) -> String {
    let Some(raw) = config_yml else {
        return "#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]\npub struct PluginConfig {}\n".to_owned();
    };
    let Ok(serde_norway::Value::Mapping(map)) = serde_norway::from_str::<serde_norway::Value>(raw)
    else {
        return "#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]\npub struct PluginConfig {}\n".to_owned();
    };

    let mut out = String::from(
        "#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]\n#[serde(default)]\npub struct PluginConfig {\n",
    );
    for (k, v) in map {
        let Some(key_str) = k.as_str() else {
            continue;
        };
        let field = to_snake_case(key_str);
        let ty = match v {
            serde_norway::Value::Bool(_) => "bool",
            serde_norway::Value::Number(n) if n.is_i64() || n.is_u64() => "i64",
            serde_norway::Value::Number(_) => "f64",
            serde_norway::Value::String(_) => "String",
            serde_norway::Value::Sequence(_) => "Vec<serde_json::Value>",
            serde_norway::Value::Mapping(_) => "serde_json::Value",
            _ => "Option<String>",
        };
        if field != key_str {
            let _ = writeln!(out, "    #[serde(rename = {key_str:?})]");
        }
        let _ = writeln!(out, "    pub {field}: {ty},");
    }
    out.push_str("}\n");
    out
}

/// Writes a standalone Rust native plugin crate scaffold + compatibility report into `out_dir`.
pub fn generate_scaffold(
    meta: &PluginMeta,
    config_yml: Option<&str>,
    report: &CompatReport,
    out_dir: &Path,
) -> Result<(), ConvertError> {
    let crate_name = format!(
        "ferrite-plugin-{}",
        to_snake_case(&meta.name).replace('_', "-")
    );
    let struct_name = format!("{}Plugin", to_pascal_case(&meta.name));

    fs::create_dir_all(out_dir.join("src"))?;

    let cargo_toml = format!(
        r#"[package]
name = "{crate_name}"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["rlib", "cdylib"]

[dependencies]
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
uuid = {{ version = "1", features = ["v4", "serde"] }}
"#
    );
    fs::write(out_dir.join("Cargo.toml"), cargo_toml)?;

    let mut lib_rs = String::new();
    let _ = writeln!(
        lib_rs,
        "//! Rust native Ferrite plugin scaffold converted from `{}` v{}.",
        meta.name, meta.version
    );
    let _ = writeln!(lib_rs, "//! Original main class: `{}`\n", meta.main);
    let _ = writeln!(lib_rs, "use uuid::Uuid;\n");
    let _ = writeln!(lib_rs, "{}", infer_config_struct(config_yml));
    let _ = writeln!(
        lib_rs,
        "/// Native plugin state for `{}`.\n#[derive(Debug, Default)]\npub struct {struct_name} {{\n    pub config: PluginConfig,\n}}\n",
        meta.name
    );

    // Emit permission constants extracted from plugin.yml command permissions.
    let mut perm_nodes = std::collections::BTreeSet::new();
    for cmd_desc in meta.commands.values() {
        if let Some(perm) = &cmd_desc.permission
            && !perm.trim().is_empty()
        {
            perm_nodes.insert(perm.trim().to_owned());
        }
    }
    if !perm_nodes.is_empty() {
        let _ = writeln!(lib_rs, "/// Permission nodes migrated from `plugin.yml`.");
        let _ = writeln!(lib_rs, "pub mod permissions {{");
        for node in &perm_nodes {
            let const_name = to_snake_case(node).to_ascii_uppercase();
            let _ = writeln!(lib_rs, "    pub const {const_name}: &str = {node:?};");
        }
        let _ = writeln!(lib_rs, "}}\n");
    }

    let _ = writeln!(lib_rs, "impl {struct_name} {{");
    let _ = writeln!(
        lib_rs,
        "    pub fn new(config: PluginConfig) -> Self {{\n        Self {{ config }}\n    }}\n"
    );
    let _ = writeln!(
        lib_rs,
        "    /// Declared commands migrated from `plugin.yml`."
    );
    let _ = writeln!(
        lib_rs,
        "    pub fn command_names(&self) -> &'static [&'static str] {{"
    );
    let _ = write!(lib_rs, "        &[");
    for (idx, cmd) in meta.commands.keys().enumerate() {
        if idx > 0 {
            lib_rs.push_str(", ");
        }
        let _ = write!(lib_rs, "{cmd:?}");
    }
    let _ = writeln!(lib_rs, "]\n    }}\n");

    let _ = writeln!(
        lib_rs,
        "    /// Dispatches a command line; returns `Some(response)` when handled."
    );
    let _ = writeln!(
        lib_rs,
        "    pub fn on_command(&self, sender: Option<Uuid>, line: &str) -> Option<String> {{"
    );
    let _ = writeln!(
        lib_rs,
        "        let trimmed = line.trim().trim_start_matches('/');"
    );
    let _ = writeln!(
        lib_rs,
        "        let mut parts = trimmed.split_whitespace();"
    );
    let _ = writeln!(
        lib_rs,
        "        let cmd = parts.next()?.to_ascii_lowercase();"
    );
    let _ = writeln!(lib_rs, "        let _args: Vec<&str> = parts.collect();");
    let _ = writeln!(lib_rs, "        let _ = sender;");
    let _ = writeln!(lib_rs, "        match cmd.as_str() {{");
    for (cmd_name, cmd_desc) in &meta.commands {
        let mut patterns = vec![format!("{:?}", cmd_name.to_ascii_lowercase())];
        for alias in &cmd_desc.aliases {
            patterns.push(format!("{:?}", alias.to_ascii_lowercase()));
        }
        let _ = writeln!(
            lib_rs,
            "            {} => Some(format!(\"[{}:/{{cmd}}] TODO: port command logic\")),",
            patterns.join(" | "),
            meta.name
        );
    }
    let _ = writeln!(lib_rs, "            _ => None,");
    let _ = writeln!(lib_rs, "        }}\n    }}\n");

    let mut generated_event_calls: Vec<String> = Vec::new();
    let mut seen_event_fns = std::collections::BTreeSet::new();
    for event in &report.detected_events {
        let short = event.rsplit('.').next().unwrap_or(event);
        let (fn_name, signature, body, test_call) = match short {
            "PlayerJoinEvent" => (
                "on_player_join".to_owned(),
                "(&self, _player: Uuid, _name: &str)".to_owned(),
                "",
                "plugin.on_player_join(Uuid::nil(), \"TestPlayer\");".to_owned(),
            ),
            "PlayerQuitEvent" => (
                "on_player_quit".to_owned(),
                "(&self, _player: Uuid, _name: &str)".to_owned(),
                "",
                "plugin.on_player_quit(Uuid::nil(), \"TestPlayer\");".to_owned(),
            ),
            "AsyncPlayerChatEvent" | "AsyncChatEvent" => (
                "on_player_chat".to_owned(),
                "(&self, _player: Uuid, message: &str) -> Option<String>".to_owned(),
                "Some(message.to_owned())",
                "assert_eq!(plugin.on_player_chat(Uuid::nil(), \"hi\").as_deref(), Some(\"hi\"));"
                    .to_owned(),
            ),
            "BlockBreakEvent" => (
                "on_block_break".to_owned(),
                "(&self, _player: Uuid, _pos: [i32; 3], _material: &str) -> bool".to_owned(),
                "true",
                "assert!(plugin.on_block_break(Uuid::nil(), [0, 64, 0], \"minecraft:stone\"));"
                    .to_owned(),
            ),
            "BlockPlaceEvent" => (
                "on_block_place".to_owned(),
                "(&self, _player: Uuid, _pos: [i32; 3], _material: &str) -> bool".to_owned(),
                "true",
                "assert!(plugin.on_block_place(Uuid::nil(), [0, 64, 0], \"minecraft:stone\"));"
                    .to_owned(),
            ),
            "PlayerMoveEvent" => (
                "on_player_move".to_owned(),
                "(&self, _player: Uuid, _from: [f64; 3], _to: [f64; 3]) -> bool".to_owned(),
                "true",
                "assert!(plugin.on_player_move(Uuid::nil(), [0.0, 64.0, 0.0], [1.0, 64.0, 0.0]));"
                    .to_owned(),
            ),
            "PlayerInteractEvent" => (
                "on_player_interact".to_owned(),
                "(&self, _player: Uuid, _pos: [i32; 3]) -> bool".to_owned(),
                "true",
                "assert!(plugin.on_player_interact(Uuid::nil(), [0, 64, 0]));".to_owned(),
            ),
            "EntityDamageEvent" | "EntityDamageByEntityEvent" => (
                "on_entity_damage".to_owned(),
                "(&self, _entity: Uuid, _cause: &str, amount: f64) -> f64".to_owned(),
                "amount",
                "assert_eq!(plugin.on_entity_damage(Uuid::nil(), \"FALL\", 4.0), 4.0);".to_owned(),
            ),
            "PlayerRespawnEvent" => (
                "on_player_respawn".to_owned(),
                "(&self, _player: Uuid, spawn: [f64; 3]) -> [f64; 3]".to_owned(),
                "spawn",
                "assert_eq!(plugin.on_player_respawn(Uuid::nil(), [0.0, 64.0, 0.0]), [0.0, 64.0, 0.0]);".to_owned(),
            ),
            _ => {
                let name = format!("on_{}", to_snake_case(short));
                let call = format!("assert!(plugin.{name}(Uuid::nil()));");
                (name, "(&self, _actor: Uuid) -> bool".to_owned(), "true", call)
            }
        };
        if !seen_event_fns.insert(fn_name.clone()) {
            continue;
        }
        let _ = writeln!(
            lib_rs,
            "    /// Typed event handler stub migrated from `{event}`."
        );
        if body.is_empty() {
            let _ = writeln!(lib_rs, "    pub fn {fn_name}{signature} {{}}\n");
        } else {
            let _ = writeln!(
                lib_rs,
                "    pub fn {fn_name}{signature} {{\n        {body}\n    }}\n"
            );
        }
        generated_event_calls.push(test_call);
    }
    let _ = writeln!(lib_rs, "}}\n");

    let has_join = seen_event_fns.contains("on_player_join");
    let has_quit = seen_event_fns.contains("on_player_quit");
    let has_chat = seen_event_fns.contains("on_player_chat");
    let has_break = seen_event_fns.contains("on_block_break");
    let has_place = seen_event_fns.contains("on_block_place");
    let has_move = seen_event_fns.contains("on_player_move");
    let has_interact = seen_event_fns.contains("on_player_interact");
    let has_damage = seen_event_fns.contains("on_entity_damage");

    let mut abi_trampolines = String::new();
    if has_join {
        abi_trampolines.push_str(
            r#"extern "C" fn abi_on_player_join(uuid_ptr: *const u8, name_ptr: *const u8, name_len: usize) {
    let player = read_uuid(uuid_ptr);
    let name = read_str(name_ptr, name_len);
    plugin_instance().on_player_join(player, name);
}
"#,
        );
    }
    if has_quit {
        abi_trampolines.push_str(
            r#"extern "C" fn abi_on_player_quit(uuid_ptr: *const u8, name_ptr: *const u8, name_len: usize) {
    let player = read_uuid(uuid_ptr);
    let name = read_str(name_ptr, name_len);
    plugin_instance().on_player_quit(player, name);
}
"#,
        );
    }
    if has_chat {
        abi_trampolines.push_str(
            r#"extern "C" fn abi_on_chat(uuid_ptr: *const u8, msg_ptr: *const u8, msg_len: usize) -> u8 {
    let player = read_uuid(uuid_ptr);
    let msg = read_str(msg_ptr, msg_len);
    u8::from(plugin_instance().on_player_chat(player, msg).is_some())
}
"#,
        );
    }
    if has_break {
        abi_trampolines.push_str(
            r#"extern "C" fn abi_on_block_break(uuid_ptr: *const u8, x: i32, y: i32, z: i32) -> u8 {
    let player = read_uuid(uuid_ptr);
    u8::from(plugin_instance().on_block_break(player, [x, y, z], "minecraft:stone"))
}
"#,
        );
    }
    if has_place {
        abi_trampolines.push_str(
            r#"extern "C" fn abi_on_block_place(uuid_ptr: *const u8, x: i32, y: i32, z: i32) -> u8 {
    let player = read_uuid(uuid_ptr);
    u8::from(plugin_instance().on_block_place(player, [x, y, z], "minecraft:stone"))
}
"#,
        );
    }
    if has_move {
        abi_trampolines.push_str(
            r#"extern "C" fn abi_on_player_move(uuid_ptr: *const u8, fx: f64, fy: f64, fz: f64, tx: f64, ty: f64, tz: f64) -> u8 {
    let player = read_uuid(uuid_ptr);
    u8::from(plugin_instance().on_player_move(player, [fx, fy, fz], [tx, ty, tz]))
}
"#,
        );
    }
    if has_interact {
        abi_trampolines.push_str(
            r#"extern "C" fn abi_on_player_interact(uuid_ptr: *const u8, _action: u8, x: i32, y: i32, z: i32) -> u8 {
    let player = read_uuid(uuid_ptr);
    u8::from(plugin_instance().on_player_interact(player, [x, y, z]))
}
"#,
        );
    }
    if has_damage {
        abi_trampolines.push_str(
            r#"extern "C" fn abi_on_entity_damage(uuid_ptr: *const u8, amount: f64) -> f64 {
    let entity = read_uuid(uuid_ptr);
    plugin_instance().on_entity_damage(entity, "GENERIC", amount)
}
"#,
        );
    }

    let join_slot = if has_join {
        "Some(abi_on_player_join)"
    } else {
        "None"
    };
    let quit_slot = if has_quit {
        "Some(abi_on_player_quit)"
    } else {
        "None"
    };
    let chat_slot = if has_chat {
        "Some(abi_on_chat)"
    } else {
        "None"
    };
    let break_slot = if has_break {
        "Some(abi_on_block_break)"
    } else {
        "None"
    };
    let place_slot = if has_place {
        "Some(abi_on_block_place)"
    } else {
        "None"
    };
    let move_slot = if has_move {
        "Some(abi_on_player_move)"
    } else {
        "None"
    };
    let interact_slot = if has_interact {
        "Some(abi_on_player_interact)"
    } else {
        "None"
    };
    let damage_slot = if has_damage {
        "Some(abi_on_entity_damage)"
    } else {
        "None"
    };

    // Emit a self-contained Ferrite C-ABI v1 + v2 entrypoint matching `ferrite-dynamic-plugins`
    // so `cargo build --release` yields a hot-loadable `.so`/`.dll`/`.dylib` ready for
    // `plugins/native/` and `/dynplugins reload`.
    let name_lit = meta.name.as_str();
    let version_lit = meta.version.as_str();
    let _ = writeln!(
        lib_rs,
        r#"// ── Ferrite Dynamic Native Plugin C-ABI v1 Entrypoint ─────────────────────

const FERRITE_ABI_VERSION: u32 = 1;

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
    pub on_command: Option<
        extern "C" fn(*const u8, *const u8, usize, *mut u8, usize) -> isize,
    >,
}}

unsafe impl Sync for FerritePluginVTable {{}}

static INSTANCE: std::sync::OnceLock<{struct_name}> = std::sync::OnceLock::new();

fn plugin_instance() -> &'static {struct_name} {{
    INSTANCE.get_or_init(|| {struct_name}::new(PluginConfig::default()))
}}

#[allow(dead_code)]
fn read_uuid(ptr: *const u8) -> Uuid {{
    if ptr.is_null() {{
        return Uuid::nil();
    }}
    let raw = unsafe {{ std::slice::from_raw_parts(ptr, 16) }};
    Uuid::from_bytes(raw.try_into().unwrap_or([0; 16]))
}}

#[allow(dead_code)]
fn read_str<'a>(ptr: *const u8, len: usize) -> &'a str {{
    if ptr.is_null() || len == 0 {{
        return "";
    }}
    let bytes = unsafe {{ std::slice::from_raw_parts(ptr, len) }};
    std::str::from_utf8(bytes).unwrap_or("")
}}

extern "C" fn abi_on_enable() {{
    let _ = plugin_instance();
}}

{abi_trampolines}extern "C" fn abi_on_command(
    uuid_ptr: *const u8,
    line_ptr: *const u8,
    line_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
) -> isize {{
    if line_ptr.is_null() {{
        return -1;
    }}
    let id = read_uuid(uuid_ptr);
    let sender = if id.is_nil() {{ None }} else {{ Some(id) }};
    let line = read_str(line_ptr, line_len);
    let Some(reply) = plugin_instance().on_command(sender, line) else {{
        return -1;
    }};
    let bytes = reply.as_bytes();
    let copy_len = bytes.len().min(out_cap);
    if !out_buf.is_null() && copy_len > 0 {{
        unsafe {{
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), out_buf, copy_len);
        }}
    }}
    copy_len as isize
}}

static PLUGIN_NAME: &str = {name_lit:?};
static PLUGIN_VERSION: &str = {version_lit:?};

static VTABLE: FerritePluginVTable = FerritePluginVTable {{
    abi_version: FERRITE_ABI_VERSION,
    name_ptr: PLUGIN_NAME.as_ptr(),
    name_len: PLUGIN_NAME.len(),
    version_ptr: PLUGIN_VERSION.as_ptr(),
    version_len: PLUGIN_VERSION.len(),
    on_enable: Some(abi_on_enable),
    on_disable: None,
    on_player_join: {join_slot},
    on_player_quit: {quit_slot},
    on_chat: {chat_slot},
    on_block_break: {break_slot},
    on_block_place: {place_slot},
    on_command: Some(abi_on_command),
}};

#[unsafe(no_mangle)]
pub extern "C" fn ferrite_plugin_entry() -> *const FerritePluginVTable {{
    &VTABLE
}}

// ── Ferrite Dynamic Native Plugin C-ABI v2 Entrypoint & Host Callbacks ────

const FERRITE_ABI_V2_VERSION: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FerriteHostCallbacks {{
    pub log: extern "C" fn(level: u8, msg_ptr: *const u8, msg_len: usize),
    pub send_message: extern "C" fn(uuid_ptr: *const u8, msg_ptr: *const u8, msg_len: usize),
    pub broadcast: extern "C" fn(msg_ptr: *const u8, msg_len: usize),
    pub send_action_bar: extern "C" fn(uuid_ptr: *const u8, msg_ptr: *const u8, msg_len: usize),
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
    pub get_block: extern "C" fn(world_ptr: *const u8, world_len: usize, x: i32, y: i32, z: i32) -> i32,
    pub set_block: extern "C" fn(
        world_ptr: *const u8,
        world_len: usize,
        x: i32,
        y: i32,
        z: i32,
        state_id: u32,
    ) -> u8,
}}

unsafe impl Send for FerriteHostCallbacks {{}}
unsafe impl Sync for FerriteHostCallbacks {{}}

#[repr(C)]
pub struct FerritePluginVTableV2 {{
    pub abi_version: u32,
    pub on_host_ready: Option<extern "C" fn(callbacks: *const FerriteHostCallbacks)>,
    pub on_player_move: Option<extern "C" fn(*const u8, f64, f64, f64, f64, f64, f64) -> u8>,
    pub on_player_interact: Option<extern "C" fn(*const u8, u8, i32, i32, i32) -> u8>,
    pub on_entity_damage: Option<extern "C" fn(*const u8, f64) -> f64>,
    pub on_tick: Option<extern "C" fn()>,
}}

unsafe impl Sync for FerritePluginVTableV2 {{}}

static HOST_CB: std::sync::OnceLock<FerriteHostCallbacks> = std::sync::OnceLock::new();

extern "C" fn abi_on_host_ready(callbacks: *const FerriteHostCallbacks) {{
    if !callbacks.is_null() {{
        let _ = HOST_CB.set(unsafe {{ *callbacks }});
    }}
}}

/// Safe ergonomic wrappers over Ferrite's C-ABI v2 live host callbacks.
pub mod host {{
    use super::*;

    pub fn log(level: u8, msg: &str) {{
        if let Some(cb) = HOST_CB.get() {{
            (cb.log)(level, msg.as_ptr(), msg.len());
        }}
    }}

    pub fn send_message(player: Uuid, msg: &str) {{
        if let Some(cb) = HOST_CB.get() {{
            let bytes = player.into_bytes();
            (cb.send_message)(bytes.as_ptr(), msg.as_ptr(), msg.len());
        }}
    }}

    pub fn broadcast(msg: &str) {{
        if let Some(cb) = HOST_CB.get() {{
            (cb.broadcast)(msg.as_ptr(), msg.len());
        }}
    }}

    pub fn send_action_bar(player: Uuid, msg: &str) {{
        if let Some(cb) = HOST_CB.get() {{
            let bytes = player.into_bytes();
            (cb.send_action_bar)(bytes.as_ptr(), msg.as_ptr(), msg.len());
        }}
    }}

    pub fn teleport(player: Uuid, world: &str, x: f64, y: f64, z: f64, yaw: f32, pitch: f32) {{
        if let Some(cb) = HOST_CB.get() {{
            let bytes = player.into_bytes();
            (cb.teleport)(bytes.as_ptr(), world.as_ptr(), world.len(), x, y, z, yaw, pitch);
        }}
    }}

    pub fn get_block(world: &str, x: i32, y: i32, z: i32) -> Option<u32> {{
        let cb = HOST_CB.get()?;
        let state = (cb.get_block)(world.as_ptr(), world.len(), x, y, z);
        u32::try_from(state).ok()
    }}

    pub fn set_block(world: &str, x: i32, y: i32, z: i32, state_id: u32) -> bool {{
        HOST_CB
            .get()
            .is_some_and(|cb| (cb.set_block)(world.as_ptr(), world.len(), x, y, z, state_id) != 0)
    }}
}}

static VTABLE_V2: FerritePluginVTableV2 = FerritePluginVTableV2 {{
    abi_version: FERRITE_ABI_V2_VERSION,
    on_host_ready: Some(abi_on_host_ready),
    on_player_move: {move_slot},
    on_player_interact: {interact_slot},
    on_entity_damage: {damage_slot},
    on_tick: None,
}};

#[unsafe(no_mangle)]
pub extern "C" fn ferrite_plugin_entry_v2() -> *const FerritePluginVTableV2 {{
    &VTABLE_V2
}}
"#
    );

    let _ = writeln!(lib_rs, "#[cfg(test)]\nmod tests {{");
    let _ = writeln!(lib_rs, "    use super::*;\n");
    let _ = writeln!(lib_rs, "    #[test]\n    fn scaffold_smoke_test() {{");
    let _ = writeln!(
        lib_rs,
        "        let plugin = {struct_name}::new(PluginConfig::default());"
    );
    for cmd_name in meta.commands.keys() {
        let _ = writeln!(
            lib_rs,
            "        assert!(plugin.on_command(Some(Uuid::nil()), \"/{cmd_name}\").is_some());"
        );
    }
    for call in &generated_event_calls {
        let _ = writeln!(lib_rs, "        {call}");
    }
    let _ = writeln!(lib_rs, "    }}\n}}");
    fs::write(out_dir.join("src/lib.rs"), lib_rs)?;

    let report_json = serde_json::to_string_pretty(report).unwrap_or_else(|_| "{}".into());
    fs::write(out_dir.join("COMPAT_REPORT.json"), report_json)?;

    let mut guide = String::new();
    let _ = writeln!(
        guide,
        "# Ferrite Porting Guide — {} v{}\n",
        meta.name, meta.version
    );
    let _ = writeln!(guide, "- **Main class**: `{}`", meta.main);
    let _ = writeln!(guide, "- **Folia declared**: `{}`", meta.folia_supported);
    let _ = writeln!(guide, "- **Verdict**: `{:?}`", report.verdict);
    let _ = writeln!(
        guide,
        "- **Class files scanned**: {}\n",
        report.class_files_scanned
    );
    let _ = writeln!(
        guide,
        "## Detected Events ({})",
        report.detected_events.len()
    );
    for e in &report.detected_events {
        let _ = writeln!(guide, "- `{e}`");
    }
    let _ = writeln!(guide, "\n## Hard Blockers ({})", report.blockers.len());
    for b in &report.blockers {
        let _ = writeln!(guide, "- `{b}`");
    }
    let _ = writeln!(
        guide,
        "\n## Unverified APIs ({})",
        report.unverified_apis.len()
    );
    for u in &report.unverified_apis {
        let _ = writeln!(guide, "- `{u}`");
    }
    let uses_bukkit_scheduler = report
        .supported_apis
        .iter()
        .any(|api| api.starts_with("org.bukkit.scheduler."));
    if uses_bukkit_scheduler && !meta.folia_supported {
        let _ = writeln!(guide, "\n## Folia Regionized Scheduler Migration Hints");
        let _ = writeln!(
            guide,
            "- Replace `Bukkit.getScheduler().runTask(...)` with `Bukkit.getGlobalRegionScheduler().run(...)` or `Bukkit.getRegionScheduler().run(plugin, location, ...)`."
        );
        let _ = writeln!(
            guide,
            "- Replace entity-bound tasks with `entity.getScheduler().run(plugin, ..., null)`."
        );
        let _ = writeln!(
            guide,
            "- Replace async DB/HTTP tasks with `Bukkit.getAsyncScheduler().runNow(plugin, ...)`."
        );
    }
    let _ = writeln!(guide, "\n## Rust Native Integration Hints");
    let _ = writeln!(
        guide,
        "- Build as a drop-in dynamic library (`cargo build --release`) and place the resulting `.so`/`.dll`/`.dylib` into `plugins/native/` for Ferrite's C-ABI v1/v2 loader (`ferrite-dynamic-plugins`), or register statically via `CompositeHooks` for zero-JNI overhead."
    );
    fs::write(out_dir.join("PORTING_GUIDE.md"), guide)?;

    Ok(())
}
