//! JVM `.class` bytecode `Code`-attribute decoder and zero-dependency Rust C-ABI v2
//! auto-transpiler/compiler for simple-to-moderate Bukkit/Paper plugins.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use ferrite_plugin_meta::PluginMeta;
use zip::ZipArchive;

use crate::compat::{CompatReport, CompatVerdict, ConvertError, inspect_jar};

/// Recognized Bukkit/Paper event hook target extracted from a listener method descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NativeEventKind {
    PlayerJoin,
    PlayerQuit,
    PlayerChat,
    BlockBreak,
    BlockPlace,
    PlayerMove,
    PlayerInteract,
    EntityDamage,
    PlayerDeath,
    PlayerRespawn,
    InventoryClick,
}

/// High-level action extracted from JVM method bytecode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandlerAction {
    SendMessage(String),
    Broadcast(String),
    SendActionBar(String),
    SendTitle { title: String, subtitle: String },
    CancelEvent,
    KeepInventory,
    KickPlayer(String),
    PerformCommand(String),
}

/// One extracted event listener handler or `onCommand` body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedHandler {
    pub event: Option<NativeEventKind>,
    pub is_command: bool,
    pub actions: Vec<HandlerAction>,
}

/// Result of analyzing a `.jar` for automatic zero-JNI Rust native compilation.
#[derive(Debug, Clone)]
pub struct AutoConvertPlan {
    pub meta: PluginMeta,
    pub report: CompatReport,
    pub handlers: Vec<ExtractedHandler>,
    pub auto_convertible: bool,
    pub fallback_reason: Option<String>,
}

/// Outcome of [`auto_build_native_plugin`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoBuildOutcome {
    /// Compiled directly to a drop-in C-ABI v2 shared library at `path`.
    Built(PathBuf),
    /// Not eligible for automatic native transpilation; run via embedded JVM host instead.
    FallbackToJvm(String),
}

enum Cp {
    Empty,
    Utf8(String),
    StringLit(u16),
    NameAndType(u16),
    MethodRef(u16),
    Other,
}

struct ByteCur<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> ByteCur<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    fn u8(&mut self) -> Option<u8> {
        let b = *self.bytes.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }
    fn u16(&mut self) -> Option<u16> {
        let s = self.bytes.get(self.pos..self.pos + 2)?;
        self.pos += 2;
        Some(u16::from_be_bytes([s[0], s[1]]))
    }
    fn u32(&mut self) -> Option<u32> {
        let s = self.bytes.get(self.pos..self.pos + 4)?;
        self.pos += 4;
        Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn skip(&mut self, n: usize) -> Option<()> {
        let end = self.pos.checked_add(n)?;
        if end > self.bytes.len() {
            return None;
        }
        self.pos = end;
        Some(())
    }
    fn slice(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let s = self.bytes.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }
}

fn event_from_descriptor(desc: &str) -> Option<NativeEventKind> {
    if desc.contains("Lorg/bukkit/event/player/PlayerJoinEvent;") {
        Some(NativeEventKind::PlayerJoin)
    } else if desc.contains("Lorg/bukkit/event/player/PlayerQuitEvent;") {
        Some(NativeEventKind::PlayerQuit)
    } else if desc.contains("Lorg/bukkit/event/player/AsyncPlayerChatEvent;")
        || desc.contains("Lio/papermc/paper/event/player/AsyncChatEvent;")
    {
        Some(NativeEventKind::PlayerChat)
    } else if desc.contains("Lorg/bukkit/event/block/BlockBreakEvent;") {
        Some(NativeEventKind::BlockBreak)
    } else if desc.contains("Lorg/bukkit/event/block/BlockPlaceEvent;") {
        Some(NativeEventKind::BlockPlace)
    } else if desc.contains("Lorg/bukkit/event/player/PlayerMoveEvent;") {
        Some(NativeEventKind::PlayerMove)
    } else if desc.contains("Lorg/bukkit/event/player/PlayerInteractEvent;") {
        Some(NativeEventKind::PlayerInteract)
    } else if desc.contains("Lorg/bukkit/event/entity/EntityDamageEvent;")
        || desc.contains("Lorg/bukkit/event/entity/EntityDamageByEntityEvent;")
    {
        Some(NativeEventKind::EntityDamage)
    } else if desc.contains("Lorg/bukkit/event/entity/PlayerDeathEvent;") {
        Some(NativeEventKind::PlayerDeath)
    } else if desc.contains("Lorg/bukkit/event/player/PlayerRespawnEvent;") {
        Some(NativeEventKind::PlayerRespawn)
    } else if desc.contains("Lorg/bukkit/event/inventory/InventoryClickEvent;") {
        Some(NativeEventKind::InventoryClick)
    } else {
        None
    }
}

/// Decodes methods and `Code` attributes from one JVM `.class` buffer and extracts high-level
/// Bukkit event/command actions.
pub fn extract_handlers_from_class(bytes: &[u8]) -> Vec<ExtractedHandler> {
    let mut cur = ByteCur::new(bytes);
    if cur.u32() != Some(0xCAFE_BABE) {
        return Vec::new();
    }
    if cur.u16().is_none() || cur.u16().is_none() {
        return Vec::new();
    }
    let Some(cp_count) = cur.u16().map(usize::from) else {
        return Vec::new();
    };
    let mut cp = Vec::with_capacity(cp_count);
    cp.push(Cp::Empty);
    let mut i = 1;
    while i < cp_count {
        let Some(tag) = cur.u8() else {
            return Vec::new();
        };
        match tag {
            1 => {
                let Some(len) = cur.u16().map(usize::from) else {
                    return Vec::new();
                };
                let Some(raw) = cur.slice(len) else {
                    return Vec::new();
                };
                cp.push(Cp::Utf8(String::from_utf8_lossy(raw).into_owned()));
            }
            3 | 4 => {
                if cur.skip(4).is_none() {
                    return Vec::new();
                }
                cp.push(Cp::Other);
            }
            5 | 6 => {
                if cur.skip(8).is_none() {
                    return Vec::new();
                }
                cp.push(Cp::Other);
                cp.push(Cp::Empty);
                i += 1;
            }
            7 => {
                if cur.u16().is_none() {
                    return Vec::new();
                }
                cp.push(Cp::Other);
            }
            8 => {
                let Some(idx) = cur.u16() else {
                    return Vec::new();
                };
                cp.push(Cp::StringLit(idx));
            }
            16 | 19 | 20 => {
                if cur.skip(2).is_none() {
                    return Vec::new();
                }
                cp.push(Cp::Other);
            }
            9 | 17 | 18 => {
                if cur.skip(4).is_none() {
                    return Vec::new();
                }
                cp.push(Cp::Other);
            }
            10 | 11 => {
                let (Some(_c), Some(n)) = (cur.u16(), cur.u16()) else {
                    return Vec::new();
                };
                cp.push(Cp::MethodRef(n));
            }
            12 => {
                let (Some(n), Some(_d)) = (cur.u16(), cur.u16()) else {
                    return Vec::new();
                };
                cp.push(Cp::NameAndType(n));
            }
            15 => {
                if cur.skip(3).is_none() {
                    return Vec::new();
                }
                cp.push(Cp::Other);
            }
            _ => return Vec::new(),
        }
        i += 1;
    }

    let utf8 = |idx: u16| -> Option<&str> {
        match cp.get(usize::from(idx)) {
            Some(Cp::Utf8(s)) => Some(s.as_str()),
            _ => None,
        }
    };
    let string_lit = |idx: u16| -> Option<&str> {
        match cp.get(usize::from(idx)) {
            Some(Cp::StringLit(u)) => utf8(*u),
            Some(Cp::Utf8(s)) => Some(s.as_str()),
            _ => None,
        }
    };
    let method_name = |idx: u16| -> Option<&str> {
        let Some(Cp::MethodRef(nat_idx)) = cp.get(usize::from(idx)) else {
            return None;
        };
        let Some(Cp::NameAndType(name_idx)) = cp.get(usize::from(*nat_idx)) else {
            return None;
        };
        utf8(*name_idx)
    };

    // access_flags, this_class, super_class
    if cur.skip(6).is_none() {
        return Vec::new();
    }
    let Some(ifaces) = cur.u16().map(usize::from) else {
        return Vec::new();
    };
    if cur.skip(ifaces * 2).is_none() {
        return Vec::new();
    }
    // Skip fields
    let Some(fields) = cur.u16().map(usize::from) else {
        return Vec::new();
    };
    for _ in 0..fields {
        if cur.skip(6).is_none() {
            return Vec::new();
        }
        let Some(attrs) = cur.u16().map(usize::from) else {
            return Vec::new();
        };
        for _ in 0..attrs {
            if cur.skip(2).is_none() {
                return Vec::new();
            }
            let Some(len) = cur.u32().map(|v| v as usize) else {
                return Vec::new();
            };
            if cur.skip(len).is_none() {
                return Vec::new();
            }
        }
    }

    // Parse methods
    let Some(methods) = cur.u16().map(usize::from) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for _ in 0..methods {
        let (Some(_acc), Some(name_idx), Some(desc_idx), Some(attrs)) =
            (cur.u16(), cur.u16(), cur.u16(), cur.u16())
        else {
            break;
        };
        let mname = utf8(name_idx).unwrap_or("");
        let mdesc = utf8(desc_idx).unwrap_or("");
        let event = event_from_descriptor(mdesc);
        let is_command = mname == "onCommand";

        let mut code_bytes: Option<&[u8]> = None;
        for _ in 0..attrs {
            let (Some(attr_name_idx), Some(attr_len)) = (cur.u16(), cur.u32()) else {
                return out;
            };
            let Some(attr_data) = cur.slice(attr_len as usize) else {
                return out;
            };
            if utf8(attr_name_idx) == Some("Code") && attr_data.len() >= 8 {
                let code_len =
                    u32::from_be_bytes([attr_data[4], attr_data[5], attr_data[6], attr_data[7]])
                        as usize;
                if 8 + code_len <= attr_data.len() {
                    code_bytes = Some(&attr_data[8..8 + code_len]);
                }
            }
        }

        if (event.is_some() || is_command)
            && let Some(code) = code_bytes
        {
            let actions = decode_actions(code, &string_lit, &method_name);
            if !actions.is_empty() {
                out.push(ExtractedHandler {
                    event,
                    is_command,
                    actions,
                });
            }
        }
    }
    out
}

fn decode_actions<'a>(
    code: &[u8],
    string_lit: &impl Fn(u16) -> Option<&'a str>,
    method_name: &impl Fn(u16) -> Option<&'a str>,
) -> Vec<HandlerAction> {
    let mut actions = Vec::new();
    let mut str_stack: Vec<String> = Vec::new();
    let mut last_bool: Option<bool> = None;
    let mut pc = 0usize;

    while pc < code.len() {
        let op = code[pc];
        pc += 1;
        match op {
            0x03 => last_bool = Some(false), // iconst_0
            0x04 => last_bool = Some(true),  // iconst_1
            0x12 => {
                // ldc (1-byte index)
                if let Some(&idx) = code.get(pc) {
                    pc += 1;
                    if let Some(s) = string_lit(u16::from(idx)) {
                        str_stack.push(s.to_owned());
                    }
                }
            }
            0x13 => {
                // ldc_w (2-byte index)
                if pc + 2 <= code.len() {
                    let idx = u16::from_be_bytes([code[pc], code[pc + 1]]);
                    pc += 2;
                    if let Some(s) = string_lit(idx) {
                        str_stack.push(s.to_owned());
                    }
                }
            }
            0xb6..=0xb9 => {
                // invokevirtual, invokespecial, invokestatic, invokeinterface
                if pc + 2 <= code.len() {
                    let idx = u16::from_be_bytes([code[pc], code[pc + 1]]);
                    pc += if op == 0xb9 { 4 } else { 2 };
                    if let Some(name) = method_name(idx) {
                        match name {
                            "sendMessage" => {
                                if let Some(msg) = str_stack.pop() {
                                    actions.push(HandlerAction::SendMessage(msg));
                                }
                            }
                            "broadcastMessage" | "broadcast" => {
                                if let Some(msg) = str_stack.pop() {
                                    actions.push(HandlerAction::Broadcast(msg));
                                }
                            }
                            "sendActionBar" => {
                                if let Some(msg) = str_stack.pop() {
                                    actions.push(HandlerAction::SendActionBar(msg));
                                }
                            }
                            "sendTitle" => {
                                let subtitle = str_stack.pop().unwrap_or_default();
                                let title = str_stack.pop().unwrap_or_default();
                                if !title.is_empty() || !subtitle.is_empty() {
                                    actions.push(HandlerAction::SendTitle { title, subtitle });
                                }
                            }
                            "setCancelled" => {
                                if last_bool == Some(true) {
                                    actions.push(HandlerAction::CancelEvent);
                                }
                                last_bool = None;
                            }
                            "setKeepInventory" => {
                                if last_bool == Some(true) {
                                    actions.push(HandlerAction::KeepInventory);
                                }
                                last_bool = None;
                            }
                            "kickPlayer" | "kick" => {
                                let reason = str_stack
                                    .pop()
                                    .unwrap_or_else(|| "Kicked by plugin".to_owned());
                                actions.push(HandlerAction::KickPlayer(reason));
                            }
                            "performCommand" => {
                                if let Some(cmd) = str_stack.pop() {
                                    actions.push(HandlerAction::PerformCommand(cmd));
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            // Skip known multi-byte opcodes so pc stays aligned
            0x10 | 0x15..=0x19 | 0x36..=0x3a | 0xbc => pc = pc.saturating_add(1),
            0x11
            | 0x14
            | 0x84
            | 0x99..=0xa8
            | 0xb2..=0xb5
            | 0xbb
            | 0xbd
            | 0xc0
            | 0xc1
            | 0xc6
            | 0xc7 => pc = pc.saturating_add(2),
            0xc5 => pc = pc.saturating_add(3),
            0xba | 0xc8 | 0xc9 => pc = pc.saturating_add(4),
            _ => {}
        }
    }
    actions
}

/// Inspects a `.jar` archive and determines whether it can be automatically transpiled and
/// compiled into a zero-JNI Rust C-ABI v2 native plugin.
pub fn analyze_jar_for_auto_convert(jar_path: &Path) -> Result<AutoConvertPlan, ConvertError> {
    let (meta, _config, report) = inspect_jar(jar_path)?;
    let file = File::open(jar_path)?;
    let mut archive = ZipArchive::new(file)?;
    let mut handlers = Vec::new();
    for idx in 0..archive.len() {
        let mut entry = archive.by_index(idx)?;
        if entry.name()?.ends_with(".class") {
            let mut bytes = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut bytes)?;
            handlers.extend(extract_handlers_from_class(&bytes));
        }
    }

    let fallback_reason = if report.verdict != CompatVerdict::ReadyForJvmHost {
        Some(format!(
            "verdict is {:?} (blockers={}, unverified={})",
            report.verdict,
            report.blockers.len(),
            report.unverified_apis.len()
        ))
    } else if report.class_files_scanned > 8 {
        Some(format!(
            "plugin has {} class files (> 8); routing to embedded JVM host for full fidelity",
            report.class_files_scanned
        ))
    } else if handlers.is_empty() {
        Some("no auto-transpilable event/command actions detected".to_owned())
    } else {
        None
    };

    let auto_convertible = fallback_reason.is_none();
    Ok(AutoConvertPlan {
        meta,
        report,
        handlers,
        auto_convertible,
        fallback_reason,
    })
}

fn emit_actions_rs(actions: &[HandlerAction], has_player: bool) -> String {
    let mut out = String::new();
    for action in actions {
        match action {
            HandlerAction::SendMessage(msg) if has_player => {
                out.push_str(&format!(
                    "        if let Some(cb) = HOST_CB.get() {{ let m = {msg:?}; (cb.send_message)(uuid_ptr, m.as_ptr(), m.len()); }}\n"
                ));
            }
            HandlerAction::SendMessage(msg) | HandlerAction::Broadcast(msg) => {
                out.push_str(&format!(
                    "        if let Some(cb) = HOST_CB.get() {{ let m = {msg:?}; (cb.broadcast)(m.as_ptr(), m.len()); }}\n"
                ));
            }
            HandlerAction::SendActionBar(msg) if has_player => {
                out.push_str(&format!(
                    "        if let Some(cb) = HOST_CB.get() {{ let m = {msg:?}; (cb.send_action_bar)(uuid_ptr, m.as_ptr(), m.len()); }}\n"
                ));
            }
            HandlerAction::SendTitle { title, subtitle } if has_player => {
                out.push_str(&format!(
                    "        if let Some(cb) = HOST_CB.get() {{ let t = {title:?}; let s = {subtitle:?}; (cb.send_title)(uuid_ptr, t.as_ptr(), t.len(), s.as_ptr(), s.len(), 10, 70, 20); }}\n"
                ));
            }
            HandlerAction::KickPlayer(reason) if has_player => {
                out.push_str(&format!(
                    "        if let Some(cb) = HOST_CB.get() {{ let r = {reason:?}; (cb.kick_player)(uuid_ptr, r.as_ptr(), r.len()); }}\n"
                ));
            }
            HandlerAction::PerformCommand(cmd) if has_player => {
                out.push_str(&format!(
                    "        if let Some(cb) = HOST_CB.get() {{ let c = {cmd:?}; (cb.perform_command)(uuid_ptr, c.as_ptr(), c.len()); }}\n"
                ));
            }
            _ => {}
        }
    }
    out
}

/// Generates a zero-dependency standalone Rust C-ABI v1 + v2 source file ready for direct
/// compilation via `rustc --crate-type cdylib --edition 2024`.
pub fn generate_standalone_cdylib_rs(plan: &AutoConvertPlan) -> String {
    let mut actions_by_event =
        std::collections::BTreeMap::<NativeEventKind, Vec<HandlerAction>>::new();
    let mut cmd_actions = Vec::<HandlerAction>::new();
    for h in &plan.handlers {
        if let Some(ev) = h.event {
            actions_by_event
                .entry(ev)
                .or_default()
                .extend(h.actions.clone());
        }
        if h.is_command {
            cmd_actions.extend(h.actions.clone());
        }
    }

    let mut fns = String::new();
    let join_slot = if let Some(acts) = actions_by_event.get(&NativeEventKind::PlayerJoin) {
        let body = emit_actions_rs(acts, true);
        fns.push_str(&format!(
            "extern \"C\" fn abi_on_join(uuid_ptr: *const u8, _n: *const u8, _l: usize) {{\n{body}}}\n"
        ));
        "Some(abi_on_join)"
    } else {
        "None"
    };
    let quit_slot = if let Some(acts) = actions_by_event.get(&NativeEventKind::PlayerQuit) {
        let body = emit_actions_rs(acts, true);
        fns.push_str(&format!(
            "extern \"C\" fn abi_on_quit(uuid_ptr: *const u8, _n: *const u8, _l: usize) {{\n{body}}}\n"
        ));
        "Some(abi_on_quit)"
    } else {
        "None"
    };
    let chat_slot = if let Some(acts) = actions_by_event.get(&NativeEventKind::PlayerChat) {
        let body = emit_actions_rs(acts, true);
        let ret = if acts.iter().any(|a| matches!(a, HandlerAction::CancelEvent)) {
            "0"
        } else {
            "1"
        };
        fns.push_str(&format!(
            "extern \"C\" fn abi_on_chat(uuid_ptr: *const u8, _m: *const u8, _l: usize) -> u8 {{\n{body}    {ret}\n}}\n"
        ));
        "Some(abi_on_chat)"
    } else {
        "None"
    };
    let break_slot = if let Some(acts) = actions_by_event.get(&NativeEventKind::BlockBreak) {
        let body = emit_actions_rs(acts, true);
        let ret = if acts.iter().any(|a| matches!(a, HandlerAction::CancelEvent)) {
            "0"
        } else {
            "1"
        };
        fns.push_str(&format!(
            "extern \"C\" fn abi_on_break(uuid_ptr: *const u8, _x: i32, _y: i32, _z: i32) -> u8 {{\n{body}    {ret}\n}}\n"
        ));
        "Some(abi_on_break)"
    } else {
        "None"
    };
    let place_slot = if let Some(acts) = actions_by_event.get(&NativeEventKind::BlockPlace) {
        let body = emit_actions_rs(acts, true);
        let ret = if acts.iter().any(|a| matches!(a, HandlerAction::CancelEvent)) {
            "0"
        } else {
            "1"
        };
        fns.push_str(&format!(
            "extern \"C\" fn abi_on_place(uuid_ptr: *const u8, _x: i32, _y: i32, _z: i32) -> u8 {{\n{body}    {ret}\n}}\n"
        ));
        "Some(abi_on_place)"
    } else {
        "None"
    };
    let death_slot = if let Some(acts) = actions_by_event.get(&NativeEventKind::PlayerDeath) {
        let body = emit_actions_rs(acts, true);
        let ret = if acts
            .iter()
            .any(|a| matches!(a, HandlerAction::KeepInventory))
        {
            "1"
        } else {
            "0"
        };
        fns.push_str(&format!(
            "extern \"C\" fn abi_on_death(uuid_ptr: *const u8) -> u8 {{\n{body}    {ret}\n}}\n"
        ));
        "Some(abi_on_death)"
    } else {
        "None"
    };

    let cmd_slot = if !plan.meta.commands.is_empty() && !cmd_actions.is_empty() {
        let mut patterns = Vec::new();
        for (name, desc) in &plan.meta.commands {
            patterns.push(format!("{:?}", name.to_ascii_lowercase()));
            for alias in &desc.aliases {
                patterns.push(format!("{:?}", alias.to_ascii_lowercase()));
            }
        }
        let reply_msg = cmd_actions
            .iter()
            .find_map(|a| match a {
                HandlerAction::SendMessage(m) | HandlerAction::Broadcast(m) => Some(m.as_str()),
                _ => None,
            })
            .unwrap_or("OK");
        let extra_body = emit_actions_rs(&cmd_actions, true);
        fns.push_str(&format!(
            r#"extern "C" fn abi_on_command(uuid_ptr: *const u8, line_ptr: *const u8, line_len: usize, out_buf: *mut u8, out_cap: usize) -> isize {{
    if line_ptr.is_null() {{ return -1; }}
    let bytes = unsafe {{ std::slice::from_raw_parts(line_ptr, line_len) }};
    let Ok(line) = std::str::from_utf8(bytes) else {{ return -1; }};
    let cmd = line.trim().trim_start_matches('/').split_whitespace().next().unwrap_or("").to_ascii_lowercase();
    match cmd.as_str() {{
        {pats} => {{
{extra_body}            let reply = {reply_msg:?};
            let rb = reply.as_bytes();
            let n = rb.len().min(out_cap);
            if !out_buf.is_null() && n > 0 {{
                unsafe {{ std::ptr::copy_nonoverlapping(rb.as_ptr(), out_buf, n); }}
            }}
            n as isize
        }}
        _ => -1,
    }}
}}
"#,
            pats = patterns.join(" | "),
        ));
        "Some(abi_on_command)"
    } else {
        "None"
    };

    let name_lit = plan.meta.name.as_str();
    let version_lit = plan.meta.version.as_str();
    format!(
        r#"//! Auto-transpiled zero-JNI Ferrite C-ABI v2 native plugin from `{name_lit}` v{version_lit}.

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FerriteHostCallbacks {{
    pub log: extern "C" fn(u8, *const u8, usize),
    pub send_message: extern "C" fn(*const u8, *const u8, usize),
    pub broadcast: extern "C" fn(*const u8, usize),
    pub send_action_bar: extern "C" fn(*const u8, *const u8, usize),
    pub teleport: extern "C" fn(*const u8, *const u8, usize, f64, f64, f64, f32, f32),
    pub get_block: extern "C" fn(*const u8, usize, i32, i32, i32) -> i32,
    pub set_block: extern "C" fn(*const u8, usize, i32, i32, i32, u32) -> u8,
    pub send_title: extern "C" fn(*const u8, *const u8, usize, *const u8, usize, i32, i32, i32),
    pub play_sound: extern "C" fn(*const u8, *const u8, usize, f64, f64, f64, f32, f32),
    pub kick_player: extern "C" fn(*const u8, *const u8, usize),
    pub set_gamemode: extern "C" fn(*const u8, u8),
    pub perform_command: extern "C" fn(*const u8, *const u8, usize),
}}
unsafe impl Send for FerriteHostCallbacks {{}}
unsafe impl Sync for FerriteHostCallbacks {{}}

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

#[repr(C)]
pub struct FerritePluginVTableV2 {{
    pub abi_version: u32,
    pub on_host_ready: Option<extern "C" fn(*const FerriteHostCallbacks)>,
    pub on_player_move: Option<extern "C" fn(*const u8, f64, f64, f64, f64, f64, f64) -> u8>,
    pub on_player_interact: Option<extern "C" fn(*const u8, u8, i32, i32, i32) -> u8>,
    pub on_entity_damage: Option<extern "C" fn(*const u8, f64) -> f64>,
    pub on_tick: Option<extern "C" fn()>,
    pub on_player_death: Option<extern "C" fn(*const u8) -> u8>,
    pub on_player_respawn: Option<extern "C" fn(*const u8, f64, f64, f64)>,
    pub on_inventory_click: Option<extern "C" fn(*const u8, i32) -> u8>,
}}
unsafe impl Sync for FerritePluginVTableV2 {{}}

static HOST_CB: std::sync::OnceLock<FerriteHostCallbacks> = std::sync::OnceLock::new();
extern "C" fn abi_on_host_ready(cb: *const FerriteHostCallbacks) {{
    if !cb.is_null() {{ let _ = HOST_CB.set(unsafe {{ *cb }}); }}
}}

{fns}
static NAME: &str = {name_lit:?};
static VERSION: &str = {version_lit:?};

static VTABLE: FerritePluginVTable = FerritePluginVTable {{
    abi_version: 1,
    name_ptr: NAME.as_ptr(),
    name_len: NAME.len(),
    version_ptr: VERSION.as_ptr(),
    version_len: VERSION.len(),
    on_enable: None,
    on_disable: None,
    on_player_join: {join_slot},
    on_player_quit: {quit_slot},
    on_chat: {chat_slot},
    on_block_break: {break_slot},
    on_block_place: {place_slot},
    on_command: {cmd_slot},
}};

static VTABLE_V2: FerritePluginVTableV2 = FerritePluginVTableV2 {{
    abi_version: 2,
    on_host_ready: Some(abi_on_host_ready),
    on_player_move: None,
    on_player_interact: None,
    on_entity_damage: None,
    on_tick: None,
    on_player_death: {death_slot},
    on_player_respawn: None,
    on_inventory_click: None,
}};

#[unsafe(no_mangle)]
pub extern "C" fn ferrite_plugin_entry() -> *const FerritePluginVTable {{ &VTABLE }}

#[unsafe(no_mangle)]
pub extern "C" fn ferrite_plugin_entry_v2() -> *const FerritePluginVTableV2 {{ &VTABLE_V2 }}
"#
    )
}

fn native_lib_extension() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "dll"
    }
    #[cfg(target_os = "macos")]
    {
        "dylib"
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        "so"
    }
}

/// Automatically transpiles and compiles `jar_path` into a drop-in C-ABI v2 `.so`/`.dll`/`.dylib`
/// inside `out_native_dir` when eligible, or returns [`AutoBuildOutcome::FallbackToJvm`].
pub fn auto_build_native_plugin(
    jar_path: &Path,
    out_native_dir: &Path,
) -> Result<AutoBuildOutcome, ConvertError> {
    let plan = analyze_jar_for_auto_convert(jar_path)?;
    if !plan.auto_convertible {
        return Ok(AutoBuildOutcome::FallbackToJvm(
            plan.fallback_reason
                .unwrap_or_else(|| "not eligible for auto-conversion".to_owned()),
        ));
    }
    std::fs::create_dir_all(out_native_dir)?;
    let slug: String = plan
        .meta
        .name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let rs_path = std::env::temp_dir().join(format!(
        "ferrite-autoconv-{}-{}.rs",
        slug,
        std::process::id()
    ));
    let out_lib = out_native_dir.join(format!("lib{slug}.{}", native_lib_extension()));
    let source = generate_standalone_cdylib_rs(&plan);
    std::fs::write(&rs_path, source)?;

    let status = Command::new("rustc")
        .args(["--crate-type", "cdylib", "--edition", "2024", "-O", "-o"])
        .arg(&out_lib)
        .arg(&rs_path)
        .output();
    let _ = std::fs::remove_file(&rs_path);

    match status {
        Ok(output) if output.status.success() => Ok(AutoBuildOutcome::Built(out_lib)),
        Ok(output) => Ok(AutoBuildOutcome::FallbackToJvm(format!(
            "rustc failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))),
        Err(err) => Ok(AutoBuildOutcome::FallbackToJvm(format!(
            "rustc unavailable: {err}"
        ))),
    }
}
