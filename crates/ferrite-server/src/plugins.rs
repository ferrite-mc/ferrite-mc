//! Loading plugins into the embedded JVM at server startup.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use ferrite_plugin::{PluginConfig, PluginHost, ServerHooks, ServerSnapshot, WorldSnapshot};

/// Everything the plugin bridge needs from the server's startup configuration.
pub struct Settings {
    pub directory: PathBuf,
    pub bridge_jar: PathBuf,
    /// Extracted vanilla data pack the bridge reads recipe and tag tables from.
    pub data_dir: PathBuf,
    /// Host part of the listener address, for `Server#getIp`.
    pub bind_address: String,
    pub motd: String,
    pub max_players: u16,
    pub port: u16,
    pub view_distance: u8,
    pub online_mode: bool,
    pub whitelist: bool,
    /// The main world, published before plugins can query the server.
    pub world: WorldSnapshot,
    /// Receives player mutations a plugin made through the Bukkit API. The game loop routes
    /// them to the region that owns the player.
    pub player_handler: Option<ferrite_plugin::PlayerHandler>,
    /// Read-only world access for the Bukkit block API.
    pub world_view: Option<std::sync::Arc<dyn ferrite_plugin::WorldView>>,
}

/// A running embedded JVM with the server's plugins enabled.
pub struct PluginBridge {
    host: Arc<PluginHost>,
}

impl PluginBridge {
    /// Start the JVM and enable every plugin jar in `settings.directory`.
    ///
    /// Returns `None` when the directory is absent or holds no jars, so a server
    /// without plugins never pays for JVM startup.
    pub fn start(settings: Settings) -> anyhow::Result<Option<Self>> {
        if !has_plugin_jars(&settings.directory) {
            return Ok(None);
        }
        let host = Arc::new(
            PluginHost::start(
                &settings.directory,
                PluginConfig {
                    bridge_jar: settings.bridge_jar.clone(),
                    data_dir: settings.data_dir,
                    server_snapshot: ServerSnapshot {
                        motd: settings.motd,
                        max_players: settings.max_players,
                        port: settings.port,
                        view_distance: settings.view_distance,
                        whitelist: settings.whitelist,
                        online_mode: settings.online_mode,
                        banned_ips: Vec::new(),
                        bind_address: settings.bind_address,
                    },
                    world: Some(settings.world.clone()),
                    player_handler: settings.player_handler,
                    world_view: settings.world_view,
                    ..Default::default()
                },
            )
            .with_context(|| format!("loading plugins from {}", settings.directory.display()))?,
        );
        let bridge = Self { host };
        bridge.flush_logs();
        Ok(Some(bridge))
    }

    /// Server-side hooks that deliver lifecycle events to plugin listeners.
    pub fn hooks(&self) -> Arc<dyn ServerHooks> {
        let hooks: Arc<dyn ServerHooks> = self.host.clone();
        hooks
    }

    /// Disable every plugin, which runs their `onDisable` handlers.
    pub fn shutdown(self) -> anyhow::Result<()> {
        let result = self.host.shutdown();
        self.flush_logs();
        result.context("disabling plugins")
    }

    /// Forward plugin log messages to the server's tracing output.
    pub fn flush_logs(&self) {
        for line in self.host.drain_logs() {
            tracing::info!(target: "ferrite::plugin", "{line}");
        }
    }
}

/// Live world reads for the Bukkit block API, backed by the server's chunk source.
struct SourceWorldView {
    source: Arc<crate::game::AnvilChunkSource>,
}

impl SourceWorldView {
    fn known(&self, world: uuid::Uuid) -> bool {
        world == ferrite_plugin::default_world_id()
    }
}

impl ferrite_plugin::WorldView for SourceWorldView {
    fn block(&self, world: uuid::Uuid, x: i32, y: i32, z: i32) -> Option<String> {
        if !self.known(world) {
            return None;
        }
        let state = u16::try_from(self.source.block(x, y, z)?).ok()?;
        ferrite_data::blocks::state(state).map(|(name, _)| name.to_owned())
    }
    fn sky_light(&self, world: uuid::Uuid, x: i32, y: i32, z: i32) -> Option<u8> {
        self.known(world)
            .then(|| self.source.sky_light(x, y, z))
            .flatten()
    }
    fn highest_block(&self, world: uuid::Uuid, x: i32, z: i32) -> Option<i32> {
        self.known(world)
            .then(|| self.source.highest_block_y(x, z))
            .flatten()
    }
    fn biome(&self, world: uuid::Uuid, x: i32, y: i32, z: i32) -> Option<String> {
        self.known(world)
            .then(|| self.source.biome_name(x, y, z))
            .flatten()
    }
}

/// Build the live world view handed to the plugin bridge.
pub fn world_view(
    source: Arc<crate::game::AnvilChunkSource>,
) -> Arc<dyn ferrite_plugin::WorldView> {
    Arc::new(SourceWorldView { source })
}

fn has_plugin_jars(directory: &Path) -> bool {
    std::fs::read_dir(directory).is_ok_and(|entries| {
        entries.filter_map(Result::ok).any(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_file())
                && entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("jar"))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_jar_files_count_as_plugins() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!has_plugin_jars(dir.path()));
        std::fs::write(dir.path().join("readme.txt"), "not a plugin").unwrap();
        assert!(!has_plugin_jars(dir.path()));
        std::fs::write(dir.path().join("empty.jar"), "jar").unwrap();
        assert!(has_plugin_jars(dir.path()));
        assert!(!has_plugin_jars(&dir.path().join("missing")));
    }
}
