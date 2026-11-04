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
    /// Effective view distance, already clamped to the client's 2..=32 range.
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
    /// Receives the console commands that carry out plugin world and server mutations
    /// (`World#setBlock`, IP bans, the whitelist toggle); see [`forward_console`].
    pub console: Option<tokio::sync::mpsc::UnboundedSender<String>>,
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
                    world_handler: settings.console.clone().map(|console| {
                        let handler: ferrite_plugin::WorldHandler =
                            Arc::new(move |command| match world_console_line(&command) {
                                Some(line) => {
                                    let _ = console.send(line);
                                }
                                None => tracing::debug!(
                                    ?command,
                                    "plugin world command for a world the server does not own"
                                ),
                            });
                        handler
                    }),
                    server_handler: settings.console.map(|console| {
                        let handler: ferrite_plugin::ServerHandler = Arc::new(move |command| {
                            let _ = console.send(server_console_line(&command));
                        });
                        handler
                    }),
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

/// The vanilla command that performs a plugin's world mutation, or `None` for another world.
///
/// Bukkit `Block#setType` places the material's default state, exactly like `/setblock`.
pub fn world_console_line(command: &ferrite_plugin::WorldCommand) -> Option<String> {
    match command {
        ferrite_plugin::WorldCommand::SetBlock {
            world,
            position,
            material,
        } => (*world == ferrite_plugin::default_world_id()).then(|| {
            format!(
                "setblock {} {} {} {material}",
                position.x, position.y, position.z
            )
        }),
    }
}

/// The vanilla command that performs a plugin's server mutation.
///
/// `/ban-ip` has no expiry or source, so a temporary Bukkit ban is stored as a permanent one;
/// the reason keeps its text on one line because the command reads it as a single argument.
pub fn server_console_line(command: &ferrite_plugin::ServerCommand) -> String {
    match command {
        ferrite_plugin::ServerCommand::BanIp {
            address, reason, ..
        } => match reason
            .as_deref()
            .map(|reason| reason.replace(['\r', '\n'], " "))
        {
            Some(reason) if !reason.trim().is_empty() => format!("ban-ip {address} {reason}"),
            _ => format!("ban-ip {address}"),
        },
        ferrite_plugin::ServerCommand::PardonIp { address } => format!("pardon-ip {address}"),
        ferrite_plugin::ServerCommand::SetWhitelist { enabled } => {
            format!("whitelist {}", if *enabled { "on" } else { "off" })
        }
    }
}

/// Runs plugin-issued console commands on the game loop until either side closes.
pub fn forward_console(
    mut lines: tokio::sync::mpsc::UnboundedReceiver<String>,
    game: tokio::sync::mpsc::UnboundedSender<ferrite_net::game::GameMessage>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(command) = lines.recv().await {
            let (complete, done) = tokio::sync::oneshot::channel();
            let message = ferrite_net::game::GameMessage::Console {
                command: command.clone(),
                complete,
            };
            if game.send(message).is_err() {
                break;
            }
            match done.await {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => tracing::warn!(%command, %error, "plugin command failed"),
                Err(_) => break,
            }
        }
    })
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

    #[test]
    fn plugin_world_and_server_mutations_become_console_commands() {
        use ferrite_plugin::{BlockPosition, ServerCommand, WorldCommand};
        let set = |world| WorldCommand::SetBlock {
            world,
            position: BlockPosition { x: 1, y: -2, z: 3 },
            material: "minecraft:stone".into(),
        };
        assert_eq!(
            world_console_line(&set(ferrite_plugin::default_world_id())).as_deref(),
            Some("setblock 1 -2 3 minecraft:stone")
        );
        assert_eq!(world_console_line(&set(uuid::Uuid::nil())), None);

        let address = "10.0.0.1".parse().unwrap();
        let ban = |reason: Option<&str>| ServerCommand::BanIp {
            address,
            reason: reason.map(str::to_owned),
            expires_unix_millis: Some(1),
            source: Some("Plugin".into()),
        };
        assert_eq!(
            server_console_line(&ban(Some("spam\nbot"))),
            "ban-ip 10.0.0.1 spam bot"
        );
        assert_eq!(server_console_line(&ban(None)), "ban-ip 10.0.0.1");
        assert_eq!(server_console_line(&ban(Some(" "))), "ban-ip 10.0.0.1");
        assert_eq!(
            server_console_line(&ServerCommand::PardonIp { address }),
            "pardon-ip 10.0.0.1"
        );
        assert_eq!(
            server_console_line(&ServerCommand::SetWhitelist { enabled: true }),
            "whitelist on"
        );
        assert_eq!(
            server_console_line(&ServerCommand::SetWhitelist { enabled: false }),
            "whitelist off"
        );
    }
}
