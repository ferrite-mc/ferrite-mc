//! YAML configuration documents (Paper, Bukkit, Spigot).
//!
//! A [`YamlDocument`] keeps both the typed model and the raw `Value` it was parsed from.
//! Saving writes the typed values on top of the raw document, so keys Ferrite does not know
//! about (other Paper versions, custom keys, hand-written notes) survive a rewrite.

pub mod bukkit;
pub mod paper_global;
pub mod paper_world;
pub mod spigot;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};
use serde_norway::Value;

use crate::ConfigError;

/// Which Paper/Bukkit configuration file a document belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFileKind {
    /// `bukkit.yml`
    Bukkit,
    /// `spigot.yml`
    Spigot,
    /// `config/paper-global.yml`
    PaperGlobal,
    /// `config/paper-world-defaults.yml`
    PaperWorldDefaults,
    /// `<world>/paper-world.yml`
    PaperWorld,
}

impl ConfigFileKind {
    /// Path relative to the server directory.
    pub const fn relative_path(self) -> &'static str {
        match self {
            Self::Bukkit => "bukkit.yml",
            Self::Spigot => "spigot.yml",
            Self::PaperGlobal => "config/paper-global.yml",
            Self::PaperWorldDefaults => "config/paper-world-defaults.yml",
            Self::PaperWorld => "paper-world.yml",
        }
    }

    /// The file name used in error messages.
    pub const fn file_name(self) -> &'static str {
        match self {
            Self::Bukkit => "bukkit.yml",
            Self::Spigot => "spigot.yml",
            Self::PaperGlobal => "config/paper-global.yml",
            Self::PaperWorldDefaults => "config/paper-world-defaults.yml",
            Self::PaperWorld => "paper-world.yml",
        }
    }

    /// Comment block written at the top of generated files.
    pub const fn header(self) -> &'static str {
        match self {
            Self::Bukkit => {
                "# Bukkit configuration.\n\
                 # Modeled on bukkit.yml from Bukkit/Spigot/Paper; unknown keys are preserved on rewrite.\n"
            }
            Self::Spigot => {
                "# Spigot configuration.\n\
                 # Modeled on spigot.yml from Spigot/Paper; unknown keys are preserved on rewrite.\n"
            }
            Self::PaperGlobal => {
                "# Paper global configuration.\n\
                 # Modeled on config/paper-global.yml; unknown keys are preserved on rewrite.\n"
            }
            Self::PaperWorldDefaults => {
                "# Paper world defaults.\n\
                 # Per-world overrides belong in <world>/paper-world.yml and fall back to these values.\n\
                 # Unknown keys are preserved on rewrite.\n"
            }
            Self::PaperWorld => {
                "# Paper per-world overrides.\n\
                 # Values not set here fall back to config/paper-world-defaults.yml.\n"
            }
        }
    }

    /// Schema version this build writes into generated files.
    pub const fn supported_version(self) -> u32 {
        match self {
            // Paper's own version numbers are tracked separately; see `migrate`.
            Self::PaperGlobal => 1,
            Self::PaperWorldDefaults | Self::PaperWorld => 1,
            Self::Bukkit | Self::Spigot => 1,
        }
    }
}

/// An integer setting that Paper also accepts as one of the keywords `default` or
/// `disabled` (for example `nether-ceiling-void-damage-height: disabled`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MaybeInt {
    /// Use the built-in default.
    #[default]
    Default,
    /// Disable the feature.
    Disabled,
    /// An explicit integer value.
    Value(i32),
}

impl Serialize for MaybeInt {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Default => serializer.serialize_str("default"),
            Self::Disabled => serializer.serialize_str("disabled"),
            Self::Value(value) => serializer.serialize_i32(*value),
        }
    }
}

impl<'de> Deserialize<'de> for MaybeInt {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Int(i32),
            Keyword(String),
        }

        match Raw::deserialize(deserializer)? {
            Raw::Int(value) => Ok(Self::Value(value)),
            Raw::Keyword(keyword) => match keyword.to_ascii_lowercase().as_str() {
                "default" => Ok(Self::Default),
                "disabled" => Ok(Self::Disabled),
                other => Err(serde::de::Error::custom(format!(
                    "expected an integer, `default`, or `disabled`, found `{other}`"
                ))),
            },
        }
    }
}

/// A comment block emitted above one key path (dot-separated, for example
/// `proxies.velocity.secret`).
#[derive(Debug, Clone, Copy)]
pub struct KeyComment {
    /// Dot-separated path to the key inside the YAML document.
    pub path: &'static str,
    /// Comment lines, without the leading `#`.
    pub lines: &'static [&'static str],
    /// Whether the key's existence, name, or default could not be verified against Paper.
    pub unverified: bool,
}

/// A typed configuration struct that knows its file and its comments.
pub trait YamlConfig: Serialize + DeserializeOwned + Default {
    /// The file this struct models.
    const KIND: ConfigFileKind;

    /// Comment blocks for the keys of this file.
    fn key_comments() -> &'static [KeyComment];

    /// Render this struct as a Paper-style YAML file with comments.
    ///
    /// Unknown keys are not part of the typed model; use [`YamlDocument::to_yaml_with_comments`]
    /// when loading an existing file so they are preserved.
    fn to_yaml_with_comments(&self) -> Result<String, ConfigError> {
        let value = serde_norway::to_value(self)
            .map_err(|error| ConfigError::serialize(error.to_string()))?;
        Ok(emit_yaml_with_comments(
            &value,
            Self::KIND,
            Self::key_comments(),
        ))
    }
}

/// Render a YAML value as a Paper-style file with comments for known keys.
pub fn emit_yaml_with_comments(
    value: &Value,
    kind: ConfigFileKind,
    comments: &[KeyComment],
) -> String {
    let mut yaml = match serde_norway::to_string(value) {
        Ok(yaml) => yaml,
        Err(error) => format!("# failed to serialize: {error}\n"),
    };
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    inject_comments(&yaml, kind.header(), comments)
}

fn inject_comments(yaml: &str, header: &str, comments: &[KeyComment]) -> String {
    let mut out = String::with_capacity(yaml.len() + 512);
    out.push_str(header);
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut block_indent: Option<usize> = None;

    for line in yaml.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            out.push('\n');
            continue;
        }

        let indent = line.len() - trimmed.len();
        if let Some(block) = block_indent {
            if indent > block {
                out.push_str(line);
                out.push('\n');
                continue;
            }
            block_indent = None;
        }

        while stack.last().is_some_and(|(depth, _)| *depth >= indent) {
            stack.pop();
        }

        if trimmed.starts_with("- ") || trimmed == "-" {
            out.push_str(line);
            out.push('\n');
            continue;
        }

        let Some((key, rest)) = split_key_line(trimmed) else {
            out.push_str(line);
            out.push('\n');
            continue;
        };

        let mut path: Vec<&str> = stack.iter().map(|(_, key)| key.as_str()).collect();
        path.push(key);
        let dotted = path.join(".");

        if let Some(comment) = comments.iter().find(|comment| comment.path == dotted) {
            for comment_line in comment.lines {
                out.push_str(&" ".repeat(indent));
                out.push_str("# ");
                out.push_str(comment_line);
                out.push('\n');
            }
            if comment.unverified {
                out.push_str(&" ".repeat(indent));
                out.push_str("# UNVERIFIED: not confirmed against current Paper sources.\n");
            }
        }

        if matches!(rest.trim_start(), "|" | ">" | "|-" | ">-" | "|+" | ">+") {
            block_indent = Some(indent);
        }

        out.push_str(line);
        out.push('\n');
        stack.push((indent, key.to_string()));
    }

    out
}

/// Split a `key: value` YAML line, ignoring comment lines, anchors, and complex keys.
fn split_key_line(line: &str) -> Option<(&str, &str)> {
    if line.starts_with('#') || line.starts_with('?') {
        return None;
    }
    let bytes = line.as_bytes();
    let mut quote: Option<u8> = None;
    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'\'' | b'"' => {
                if quote == Some(*byte) {
                    quote = None;
                } else if quote.is_none() {
                    quote = Some(*byte);
                }
            }
            b':' if quote.is_none() => {
                let next = bytes.get(index + 1).copied();
                if next.is_none() || next == Some(b' ') {
                    let key = line[..index].trim();
                    let key = key.trim_matches(|ch| ch == '\'' || ch == '"');
                    if key.is_empty() {
                        return None;
                    }
                    return Some((key, &line[index + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

/// Load a YAML file as a raw value, applying config-version migrations.
///
/// Returns `None` when the file does not exist.
pub fn load_raw(
    path: impl AsRef<Path>,
    kind: ConfigFileKind,
) -> Result<Option<Value>, ConfigError> {
    let path = path.as_ref();
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(ConfigError::io(path, error)),
    };
    let mut raw: Value =
        serde_norway::from_str(&text).map_err(|error| ConfigError::yaml(path, error))?;
    let _ = crate::migrate::migrate(kind, &mut raw);
    Ok(Some(raw))
}

/// Merge `typed` over `raw`: typed values win, but keys that only exist in `raw` are kept.
pub fn merge_typed_over_raw(typed: Value, raw: &Value) -> Value {
    match (typed, raw) {
        (Value::Mapping(mut typed), Value::Mapping(raw)) => {
            for (key, raw_value) in raw {
                match typed.get_mut(key) {
                    Some(typed_value) if typed_value.is_mapping() && raw_value.is_mapping() => {
                        let merged = merge_typed_over_raw(typed_value.clone(), raw_value);
                        typed.insert(key.clone(), merged);
                    }
                    Some(_) => {}
                    None => {
                        typed.insert(key.clone(), raw_value.clone());
                    }
                }
            }
            Value::Mapping(typed)
        }
        (typed, _) => typed,
    }
}

/// Overlay `overlay` on top of `base`, keeping base keys that the overlay does not mention.
///
/// Used for per-world files: `paper-world.yml` values win, everything else falls back to
/// `paper-world-defaults.yml`.
pub fn overlay(base_doc: &Value, overlay_doc: &Value) -> Value {
    match (base_doc, overlay_doc) {
        (Value::Mapping(base_map), Value::Mapping(overlay_map)) => {
            let mut merged = base_map.clone();
            for (key, overlay_value) in overlay_map {
                let combined = match merged.get(key) {
                    Some(base_value) => overlay(base_value, overlay_value),
                    None => overlay_value.clone(),
                };
                merged.insert(key.clone(), combined);
            }
            Value::Mapping(merged)
        }
        (_, overlay_value) => overlay_value.clone(),
    }
}

/// A typed YAML configuration file together with the raw document it came from.
#[derive(Debug, Clone)]
pub struct YamlDocument<T> {
    kind: ConfigFileKind,
    typed: T,
    raw: Value,
    path: Option<PathBuf>,
}

impl<T: YamlConfig> YamlDocument<T> {
    /// A document built from the compiled-in defaults.
    pub fn defaults(kind: ConfigFileKind) -> Result<Self, ConfigError> {
        let typed = T::default();
        let raw = serde_norway::to_value(&typed)
            .map_err(|error| ConfigError::serialize(error.to_string()))?;
        Ok(Self {
            kind,
            typed,
            raw,
            path: None,
        })
    }

    /// Load a file. Unknown keys are retained in the raw document.
    pub fn load(path: impl AsRef<Path>, kind: ConfigFileKind) -> Result<Option<Self>, ConfigError> {
        let path = path.as_ref();
        let Some(raw) = load_raw(path, kind)? else {
            return Ok(None);
        };
        let typed: T = serde_norway::from_value(raw.clone())
            .map_err(|error| ConfigError::yaml(path, error))?;
        Ok(Some(Self {
            kind,
            typed,
            raw,
            path: Some(path.to_path_buf()),
        }))
    }

    /// The typed model.
    pub fn typed(&self) -> &T {
        &self.typed
    }

    /// The typed model, mutably. Callers must re-save for changes to take effect.
    pub fn typed_mut(&mut self) -> &mut T {
        &mut self.typed
    }

    /// Replace the typed model, keeping the raw document for unknown keys.
    pub fn set_typed(&mut self, typed: T) {
        self.typed = typed;
    }

    /// The file this document was loaded from, if any.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The kind of file.
    pub fn kind(&self) -> ConfigFileKind {
        self.kind
    }

    /// The raw document, including keys the typed model does not know.
    pub fn raw(&self) -> &Value {
        &self.raw
    }

    /// Typed values merged with the raw document's unknown keys.
    pub fn merged_value(&self) -> Result<Value, ConfigError> {
        let typed = serde_norway::to_value(&self.typed)
            .map_err(|error| ConfigError::serialize(error.to_string()))?;
        Ok(merge_typed_over_raw(typed, &self.raw))
    }

    /// Render a Paper-style file: typed values, unknown keys, and comments.
    pub fn to_yaml_with_comments(&self) -> Result<String, ConfigError> {
        Ok(emit_yaml_with_comments(
            &self.merged_value()?,
            self.kind,
            T::key_comments(),
        ))
    }

    /// Write the document back to disk.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), ConfigError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|error| ConfigError::io(parent, error))?;
        }
        std::fs::write(path, self.to_yaml_with_comments()?)
            .map_err(|error| ConfigError::io(path, error))
    }
}
