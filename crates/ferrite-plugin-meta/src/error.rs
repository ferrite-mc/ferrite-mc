//! Typed errors produced while reading, validating, and ordering plugins.

use std::fmt;
use std::path::PathBuf;

/// Metadata that was decoded from YAML but is not valid for Bukkit.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ValidationError {
    /// The descriptor does not define `name`.
    #[error("name is not defined")]
    MissingName,
    /// The descriptor does not define `version`.
    #[error("version is not defined")]
    MissingVersion,
    /// The descriptor does not define `main`.
    #[error("main is not defined")]
    MissingMain,
    /// `name` contains characters outside Bukkit's `[A-Za-z0-9 _.-]`.
    #[error("name '{0}' contains invalid characters")]
    InvalidName(String),
    /// `load` is neither `STARTUP` nor `POSTWORLD`.
    #[error("load '{value}' is not a valid choice")]
    InvalidLoad {
        /// The offending value.
        value: String,
    },
    /// A permission node's `default` is not one of `true`, `false`, `op`, `not op`.
    #[error("permission node '{node}' has invalid default '{value}'")]
    InvalidPermissionDefault {
        /// The permission node.
        node: String,
        /// The offending value.
        value: String,
    },
    /// A Paper dependency's `load` is not one of `BEFORE`, `AFTER`, `OMIT`.
    #[error("dependency '{dependency}' has invalid load order '{value}'")]
    InvalidDependencyLoad {
        /// The dependency name.
        dependency: String,
        /// The offending value.
        value: String,
    },
}

/// A descriptor that could not be turned into [`crate::PluginMeta`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MetadataError {
    /// The descriptor is not valid YAML, or does not have the expected shape.
    #[error("invalid YAML: {0}")]
    Yaml(String),
    /// The descriptor decoded but failed validation.
    #[error(transparent)]
    Invalid(#[from] ValidationError),
}

/// Everything that can keep a plugin jar out of the load plan.
#[derive(Debug)]
pub enum PluginError {
    /// The jar file could not be opened.
    Open {
        /// Jar path.
        path: PathBuf,
        source: std::io::Error,
    },
    /// The jar could not be read as a zip archive, or an entry is corrupt.
    Zip {
        /// Jar path.
        path: PathBuf,
        source: zip::result::ZipError,
    },
    /// The jar carries neither `paper-plugin.yml` nor `plugin.yml`.
    MissingManifest {
        /// Jar path.
        path: PathBuf,
    },
    /// An entry of the jar could not be read.
    Entry {
        /// Jar path.
        path: PathBuf,
        entry: &'static str,
        source: std::io::Error,
    },
    /// A plugin descriptor exceeds the size limit.
    DescriptorTooLarge {
        /// Jar path.
        path: PathBuf,
        /// Descriptor file name.
        entry: &'static str,
        /// Maximum accepted uncompressed size.
        max_bytes: u64,
    },
    /// A descriptor entry is not valid UTF-8.
    NotUtf8 {
        /// Jar path.
        path: PathBuf,
        /// Descriptor file name.
        entry: &'static str,
    },
    /// The descriptor could not be decoded or validated.
    Metadata {
        /// Jar path.
        path: PathBuf,
        /// Descriptor file name.
        entry: &'static str,
        source: MetadataError,
    },
    /// Another jar already claimed this plugin name.
    DuplicateName {
        /// The duplicated plugin name.
        name: String,
        /// Jar that already claimed the name.
        first: PathBuf,
    },
    /// A required dependency is not installed. Paper's wording, with every
    /// missing dependency listed at once.
    MissingDependencies {
        /// The plugin that cannot load.
        plugin: String,
        /// Missing dependency names, as written in the descriptor.
        dependencies: Vec<String>,
    },
    /// The plugin is part of a dependency cycle, so it is skipped.
    /// Paper's wording, with the cycle chain.
    Cycle {
        /// Cycle chain, `A -> B -> A`.
        cycle: String,
    },
}

impl PluginError {
    /// The missing dependency names, for variants that carry them.
    pub fn missing_dependencies(&self) -> Option<&[String]> {
        match self {
            Self::MissingDependencies { dependencies, .. } => Some(dependencies),
            _ => None,
        }
    }
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open { path, source } => {
                write!(f, "failed to open jar '{}': {source}", path.display())
            }
            Self::Zip { path, source } => {
                write!(f, "failed to read jar '{}': {source}", path.display())
            }
            Self::MissingManifest { path } => {
                write!(
                    f,
                    "'{}' contains neither plugin.yml nor paper-plugin.yml",
                    path.display()
                )
            }
            Self::Entry {
                path,
                entry,
                source,
            } => {
                write!(
                    f,
                    "failed to read {entry} from '{}': {source}",
                    path.display()
                )
            }
            Self::DescriptorTooLarge {
                path,
                entry,
                max_bytes,
            } => write!(
                f,
                "{entry} in '{}' exceeds the maximum size of {max_bytes} bytes",
                path.display()
            ),
            Self::NotUtf8 { path, entry } => {
                write!(f, "{entry} in '{}' is not valid UTF-8", path.display())
            }
            Self::Metadata {
                path,
                entry,
                source,
            } => {
                write!(
                    f,
                    "failed to parse {entry} in '{}': {source}",
                    path.display()
                )
            }
            Self::DuplicateName { name, first } => {
                write!(
                    f,
                    "ambiguous plugin name '{name}': already provided by '{}'",
                    first.display()
                )
            }
            Self::MissingDependencies {
                plugin,
                dependencies,
            } => write!(
                f,
                "Unknown/missing dependency plugins: [{}]. Please download and install these plugins to run '{plugin}'.",
                dependencies.join(", ")
            ),
            Self::Cycle { cycle } => write!(f, "Circular plugin loading detected: {cycle}"),
        }
    }
}

impl std::error::Error for PluginError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Open { source, .. } => Some(source),
            Self::Zip { source, .. } => Some(source),
            Self::Entry { source, .. } => Some(source),
            Self::Metadata { source, .. } => Some(source),
            Self::MissingManifest { .. }
            | Self::DescriptorTooLarge { .. }
            | Self::NotUtf8 { .. }
            | Self::DuplicateName { .. }
            | Self::MissingDependencies { .. }
            | Self::Cycle { .. } => None,
        }
    }
}
