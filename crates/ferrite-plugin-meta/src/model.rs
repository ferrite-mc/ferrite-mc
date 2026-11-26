//! The metadata model shared by Bukkit/Spigot `plugin.yml` and Paper
//! `paper-plugin.yml` descriptors.

use std::collections::BTreeMap;
use std::fmt;

/// The descriptor file a plugin's metadata was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ManifestFormat {
    /// Bukkit/Spigot `plugin.yml`.
    PluginYml,
    /// Paper `paper-plugin.yml`.
    PaperPluginYml,
}

impl ManifestFormat {
    /// File name of this descriptor inside the plugin jar.
    pub const fn file_name(self) -> &'static str {
        match self {
            Self::PluginYml => "plugin.yml",
            Self::PaperPluginYml => "paper-plugin.yml",
        }
    }
}

impl fmt::Display for ManifestFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.file_name())
    }
}

/// When a plugin is loaded relative to world loading (`load` in the descriptor).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PluginLoadOrder {
    /// Loaded before the world is loaded; needed by world generators.
    Startup,
    /// Loaded after the world is loaded. The default.
    #[default]
    PostWorld,
}

impl PluginLoadOrder {
    /// Descriptor spelling of this load order.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Startup => "STARTUP",
            Self::PostWorld => "POSTWORLD",
        }
    }

    /// Parses Bukkit's spelling: case-insensitive with non-word characters removed,
    /// so `post-world` and `POSTWORLD` both parse.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let normalized: String = text
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_uppercase())
            .collect();
        match normalized.as_str() {
            "STARTUP" => Some(Self::Startup),
            "POSTWORLD" => Some(Self::PostWorld),
            _ => None,
        }
    }
}

impl fmt::Display for PluginLoadOrder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How Ferrite schedules a plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadingMode {
    /// `folia-supported: true`: the plugin may run on region threads.
    RegionAware,
    /// Legacy plugins run on the main thread only.
    LegacyMainThread,
}

/// A command declared in the descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommandDescription {
    /// `description` entry.
    pub description: Option<String>,
    /// `usage` entry.
    pub usage: Option<String>,
    /// `aliases` list.
    pub aliases: Vec<String>,
    /// `permission` entry required to run the command.
    pub permission: Option<String>,
}

/// Default state of a declared permission node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PermissionDefault {
    /// Granted to everyone (`true`).
    True,
    /// Granted to nobody (`false`).
    False,
    /// Granted to operators (`op`). The default when the descriptor is silent.
    #[default]
    Op,
    /// Granted to everyone except operators (`not op`).
    NotOp,
}

impl PermissionDefault {
    /// Descriptor spelling of this default.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::True => "true",
            Self::False => "false",
            Self::Op => "op",
            Self::NotOp => "not op",
        }
    }

    /// Parses Bukkit's `PermissionDefault#getByName`: either the enum name
    /// (`TRUE`, `NOT_OP`) or the display name (`true`, `not op`), case-insensitive.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().replace('_', " ").as_str() {
            "true" => Some(Self::True),
            "false" => Some(Self::False),
            "op" => Some(Self::Op),
            "not op" => Some(Self::NotOp),
            _ => None,
        }
    }
}

impl fmt::Display for PermissionDefault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A permission node declared in the descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PermissionDescription {
    /// `description` entry.
    pub description: Option<String>,
    /// `default` entry, defaulting to [`PermissionDefault::Op`].
    pub default: PermissionDefault,
    /// `children` map: child node name to granted/denied.
    pub children: BTreeMap<String, bool>,
}

/// `load` of a Paper dependency entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PaperDependencyLoad {
    /// The dependency loads before this plugin.
    Before,
    /// The dependency loads after this plugin.
    After,
    /// No ordering is imposed. The default.
    #[default]
    Omit,
}

impl PaperDependencyLoad {
    /// Descriptor spelling of this load order.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Before => "BEFORE",
            Self::After => "AFTER",
            Self::Omit => "OMIT",
        }
    }

    pub(crate) fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_uppercase().as_str() {
            "BEFORE" => Some(Self::Before),
            "AFTER" => Some(Self::After),
            "OMIT" => Some(Self::Omit),
            _ => None,
        }
    }
}

impl fmt::Display for PaperDependencyLoad {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A single entry of Paper's `dependencies.<lifecycle>` maps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaperDependency {
    /// `load` entry, defaulting to [`PaperDependencyLoad::Omit`].
    pub load: PaperDependencyLoad,
    /// `required` entry, defaulting to `true`.
    pub required: bool,
    /// `join-classpath` entry, defaulting to `true`.
    pub join_classpath: bool,
}

impl Default for PaperDependency {
    fn default() -> Self {
        Self {
            load: PaperDependencyLoad::Omit,
            required: true,
            join_classpath: true,
        }
    }
}

/// Paper's `dependencies` section, split by life cycle.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PaperDependencies {
    /// `dependencies.bootstrap`: needed by the bootstrapper, before the server loads.
    pub bootstrap: BTreeMap<String, PaperDependency>,
    /// `dependencies.server`: regular plugin dependencies.
    pub server: BTreeMap<String, PaperDependency>,
}

/// The Paper-only half of the metadata.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PaperMeta {
    /// `bootstrapper` class.
    pub bootstrapper: Option<String>,
    /// `loader` class.
    pub loader: Option<String>,
    /// `dependencies` section.
    pub dependencies: PaperDependencies,
    /// `has-open-classloader`, defaulting to `false`.
    pub has_open_classloader: bool,
}

/// Validated plugin metadata, regardless of which descriptor carried it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginMeta {
    /// Plugin name, with spaces normalized to `_` as Bukkit does.
    pub name: String,
    /// Plugin version.
    pub version: String,
    /// Main class entry point.
    pub main: String,
    /// `api-version`.
    pub api_version: Option<String>,
    /// `author` and `authors`, author first, as Bukkit merges them.
    pub authors: Vec<String>,
    /// `description`.
    pub description: Option<String>,
    /// `website`.
    pub website: Option<String>,
    /// `prefix`.
    pub prefix: Option<String>,
    /// `load`, defaulting to [`PluginLoadOrder::PostWorld`].
    pub load: PluginLoadOrder,
    /// Hard dependencies.
    pub depend: Vec<String>,
    /// Soft dependencies: ordered if present, never required.
    pub softdepend: Vec<String>,
    /// Plugins this one must load before.
    pub loadbefore: Vec<String>,
    /// Extra names this plugin answers to.
    pub provides: Vec<String>,
    /// Maven coordinates of libraries to download.
    pub libraries: Vec<String>,
    /// Declared commands, keyed by command name.
    pub commands: BTreeMap<String, CommandDescription>,
    /// Declared permissions, keyed by permission node.
    pub permissions: BTreeMap<String, PermissionDescription>,
    /// `folia-supported`, defaulting to `false`.
    pub folia_supported: bool,
    /// Which descriptor file this came from.
    pub format: ManifestFormat,
    /// Paper-only metadata; `None` for `plugin.yml`.
    pub paper: Option<PaperMeta>,
}

impl PluginMeta {
    /// How Ferrite should schedule this plugin: Folia-supported plugins are
    /// region-aware, everything else stays on the main thread.
    pub fn threading_mode(&self) -> ThreadingMode {
        if self.folia_supported {
            ThreadingMode::RegionAware
        } else {
            ThreadingMode::LegacyMainThread
        }
    }

    /// Names this plugin answers to: its own name plus every `provides` alias.
    pub fn identity_names(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.name.as_str()).chain(self.provides.iter().map(String::as_str))
    }
}
