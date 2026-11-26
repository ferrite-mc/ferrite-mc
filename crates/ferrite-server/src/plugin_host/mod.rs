//! Server-side wiring for the embedded JVM plugin host (Plugin B, Lane 1).

mod composite;
mod hooks;
mod native;
mod state;

pub use composite::CompositeHooks;
pub use hooks::HostHooks;
pub use native::NativePluginHooks;
pub use state::WorldStateProvider;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ferrite_plugin_host::{PluginHost, PluginHostConfig, ServerInfo};
use serde::Deserialize;

/// The `[plugins]` table of `ferrite.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct PluginsSettings {
    /// Start the embedded JVM. Off by default; the plugins directory is not read when off.
    pub enabled: bool,
    /// Enable the zero-overhead Rust native plugin suite (permissions, regions, essentials, worldedit).
    pub native_plugins: bool,
    /// Directory scanned for plugin jars.
    pub dir: PathBuf,
    /// Parent of the per-plugin data directories.
    pub data_dir: PathBuf,
    /// A Java 25+ installation. `None` uses discovery.
    pub java_home: Option<PathBuf>,
    /// `-Xmx` for the embedded JVM in MiB. `0` leaves the JVM default.
    pub heap_mb: u32,
    /// Extra JVM options.
    pub jvm_args: Vec<String>,
    /// Paper API version reported to plugins.
    pub api_version: String,
    /// The `ferrite-host` jar produced by `cd java && ./gradlew :ferrite-host:jar`.
    pub host_jar: PathBuf,
    /// Additional host/API runtime jars (Lane 2's facade).
    pub runtime_classpath: Vec<PathBuf>,
    /// Optional `dev.ferrite.host.HostApi` implementation class.
    pub api_impl_class: Option<String>,
    /// Timeout for starting the JVM and the Java bootstrap.
    pub start_timeout_ms: u64,
    /// Timeout for load/event/command/shutdown calls.
    pub call_timeout_ms: u64,
}

impl Default for PluginsSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            native_plugins: false,
            dir: PathBuf::from("plugins"),
            data_dir: PathBuf::from("plugins"),
            java_home: None,
            heap_mb: 512,
            jvm_args: Vec::new(),
            api_version: "26.3".into(),
            host_jar: PathBuf::from("java/ferrite-host/build/libs/ferrite-host.jar"),
            runtime_classpath: Vec::new(),
            api_impl_class: None,
            start_timeout_ms: 30_000,
            call_timeout_ms: 10_000,
        }
    }
}

impl PluginsSettings {
    /// Converts the table into the host crate's configuration.
    pub fn to_host_config(&self, server: ServerInfo) -> PluginHostConfig {
        PluginHostConfig {
            enabled: self.enabled,
            plugins_dir: self.dir.clone(),
            data_dir: self.data_dir.clone(),
            java_home: self.java_home.clone(),
            max_heap_mb: (self.heap_mb > 0).then_some(self.heap_mb),
            jvm_args: self.jvm_args.clone(),
            api_version: self.api_version.clone(),
            host_jar: self.host_jar.clone(),
            runtime_classpath: self.runtime_classpath.clone(),
            api_impl_class: self
                .api_impl_class
                .clone()
                .filter(|class| !class.is_empty()),
            start_timeout: Duration::from_millis(self.start_timeout_ms.max(1)),
            call_timeout: Duration::from_millis(self.call_timeout_ms.max(1)),
            server,
        }
    }
}

/// Owns the running host for the server's lifetime.
///
/// Dropping it shuts the host down, so every early return from startup still disables plugins and
/// cancels their tasks. An explicit [`RunningHost::shutdown`] before world shutdown is
/// idempotent.
pub struct RunningHost {
    host: Option<Arc<PluginHost>>,
}

impl RunningHost {
    /// Starts the host when `plugins.enabled` is true. A host/JVM failure is logged and the
    /// server continues without plugins.
    pub fn start(settings: &PluginsSettings, server: ServerInfo) -> Self {
        if !settings.enabled {
            return Self { host: None };
        }
        match PluginHost::start(settings.to_host_config(server)) {
            Ok(host) if host.is_running() => {
                tracing::info!(plugins = host.loaded().len(), "plugin host started");
                Self {
                    host: Some(Arc::new(host)),
                }
            }
            Ok(host) => {
                tracing::info!(
                    failures = host.failures().len(),
                    "no plugins loaded; the plugin host is dormant"
                );
                Self { host: None }
            }
            Err(error) => {
                tracing::error!(%error, "the plugin host failed to start; continuing without plugins");
                Self { host: None }
            }
        }
    }

    /// The running host, when there is one.
    pub fn host(&self) -> Option<&Arc<PluginHost>> {
        self.host.as_ref()
    }

    /// The game-loop event hooks, when the host is running.
    pub fn hooks(&self) -> Option<Arc<HostHooks>> {
        self.host
            .as_ref()
            .map(|host| Arc::new(HostHooks::new(Arc::clone(host))))
    }

    /// Installs the v2 world/region adapter after the world exists. A dormant host is a no-op.
    pub fn install_state_provider(
        &self,
        provider: std::sync::Arc<dyn ferrite_plugin_host::HostStateProvider>,
    ) {
        let Some(host) = &self.host else {
            return;
        };
        if let Err(error) = host.install_state_provider(provider) {
            tracing::warn!(%error, "installing the plugin state provider failed");
        }
    }

    /// Disables plugins and cancels their tasks. Safe to call more than once.
    pub fn shutdown(&mut self) {
        let Some(host) = self.host.take() else {
            return;
        };
        let report = host.shutdown();
        if !report.disabled.is_empty() {
            tracing::info!(plugins = ?report.disabled, "plugins disabled");
        }
        for error in report.errors {
            tracing::warn!(%error, "a plugin failed to disable");
        }
    }
}

impl Drop for RunningHost {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_never_touches_the_plugins_directory() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("plugins");
        let settings = PluginsSettings {
            dir: missing.clone(),
            ..PluginsSettings::default()
        };
        let host = RunningHost::start(&settings, ServerInfo::default());
        assert!(host.host().is_none());
        assert!(!missing.exists());
    }

    #[test]
    fn settings_map_to_the_host_config() {
        let settings = PluginsSettings {
            enabled: true,
            heap_mb: 0,
            start_timeout_ms: 250,
            call_timeout_ms: 500,
            ..PluginsSettings::default()
        };
        let config = settings.to_host_config(ServerInfo::default());
        assert!(config.enabled);
        assert_eq!(config.max_heap_mb, None);
        assert_eq!(config.start_timeout, Duration::from_millis(250));
        assert_eq!(config.call_timeout, Duration::from_millis(500));
        assert_eq!(config.api_version, "26.3");
    }
}
