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

    for event in &report.detected_events {
        let short = event.rsplit('.').next().unwrap_or(event);
        let fn_name = format!("on_{}", to_snake_case(short));
        let _ = writeln!(
            lib_rs,
            "    /// Event handler stub migrated from `{event}`."
        );
        let _ = writeln!(
            lib_rs,
            "    pub fn {fn_name}(&self, _actor: Uuid) -> bool {{\n        true\n    }}\n"
        );
    }
    let _ = writeln!(lib_rs, "}}");
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
    fs::write(out_dir.join("PORTING_GUIDE.md"), guide)?;

    Ok(())
}
