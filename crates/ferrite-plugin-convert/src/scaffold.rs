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
        "- Implement Ferrite `ServerHooks` in `crates/ferrite-server/src/plugin_host/` or register this crate via `CompositeHooks` for zero-JNI overhead."
    );
    fs::write(out_dir.join("PORTING_GUIDE.md"), guide)?;

    Ok(())
}
