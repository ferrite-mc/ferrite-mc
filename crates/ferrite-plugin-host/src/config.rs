//! Configuration for [`PluginHost::start`](crate::PluginHost::start).

use std::path::PathBuf;
use std::time::Duration;

/// Server identity and settings exposed to plugins through `host.info`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInfo {
    /// Server name shown in `host.info` (currently `Ferrite`).
    pub name: String,
    /// Ferrite's package version.
    pub version: String,
    /// MOTD from `ferrite.toml`.
    pub motd: String,
    /// Configured player limit.
    pub max_players: u32,
    /// Whether Mojang authentication is on.
    pub online_mode: bool,
    /// Effective chunk view distance.
    pub view_distance: u8,
}

impl Default for ServerInfo {
    fn default() -> Self {
        Self {
            name: "Ferrite".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            motd: "A Ferrite server".into(),
            max_players: 20,
            online_mode: true,
            view_distance: 10,
        }
    }
}

/// Everything needed to start the embedded plugin host.
#[derive(Debug, Clone)]
pub struct PluginHostConfig {
    /// `plugins.enabled`. Disabled means no JVM and no directory access.
    pub enabled: bool,
    /// Directory scanned for plugin jars.
    pub plugins_dir: PathBuf,
    /// Parent of the per-plugin data directories (`<data_dir>/<plugin name>/`).
    pub data_dir: PathBuf,
    /// A Java 25+ installation. `None` uses [`ferrite_jvm::find_java_home`] discovery.
    pub java_home: Option<PathBuf>,
    /// `-Xmx` for the embedded JVM. `None` leaves the JVM default.
    pub max_heap_mb: Option<u32>,
    /// Extra JVM options, passed through verbatim.
    pub jvm_args: Vec<String>,
    /// Paper API version reported to the Java host, for example `26.3`.
    pub api_version: String,
    /// The `ferrite-host` jar (or a classes directory) placed on the JVM classpath.
    pub host_jar: PathBuf,
    /// Additional host/API runtime jars (Lane 2's facade jar). Plugin jars are never added.
    pub runtime_classpath: Vec<PathBuf>,
    /// Optional [`dev.ferrite.host.HostApi`] implementation loaded by the Java host.
    pub api_impl_class: Option<String>,
    /// Timeout for starting the JVM and the Java `Bootstrap`.
    pub start_timeout: Duration,
    /// Timeout for `loadPlugin`, `fireEvent`, `runCommand` and `shutdown`.
    pub call_timeout: Duration,
    /// Server metadata for `host.info`.
    pub server: ServerInfo,
}

impl Default for PluginHostConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            plugins_dir: PathBuf::from("plugins"),
            data_dir: PathBuf::from("plugins"),
            java_home: None,
            max_heap_mb: Some(512),
            jvm_args: Vec::new(),
            api_version: "26.3".into(),
            host_jar: PathBuf::from("java/ferrite-host/build/libs/ferrite-host.jar"),
            runtime_classpath: Vec::new(),
            api_impl_class: None,
            start_timeout: Duration::from_secs(30),
            call_timeout: Duration::from_secs(10),
            server: ServerInfo::default(),
        }
    }
}

impl PluginHostConfig {
    /// An enabled configuration with the default host jar and directories.
    pub fn new(plugins_dir: impl Into<PathBuf>) -> Self {
        Self {
            enabled: true,
            plugins_dir: plugins_dir.into(),
            ..Self::default()
        }
    }
}
