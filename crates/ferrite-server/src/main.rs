//! Ferrite server entry point.
use anyhow::Context;
use ferrite_server::game;
use serde::Deserialize;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[derive(Debug, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct Config {
    bind_address: String,
    online_mode: bool,
    enforce_secure_profile: bool,
    motd: String,
    /// 64x64 PNG shown in the server list. Missing or malformed files are only a warning.
    server_icon: Option<std::path::PathBuf>,
    max_players: u32,
    view_distance: u8,
    /// Which client protocol versions are accepted: `all` translates older clients,
    /// `native` serves only the core protocol.
    supported_versions: SupportedVersionsMode,
    compression_threshold: usize,
    forwarding: ForwardingMode,
    forwarding_secret: String,
    world: Option<std::path::PathBuf>,
    /// New chunks are generated with vanilla terrain or the flat test world.
    generator: WorldGenerator,
    gamemode: GameMode,
    ops: Vec<OpConfig>,
    plugins_dir: Option<std::path::PathBuf>,
    plugin_bridge: Option<std::path::PathBuf>,
    /// Plugin bridge settings from the `[plugins]` table.
    plugins: PluginSettings,
    /// Enforce `whitelist.json` for new connections.
    whitelist: bool,
}

/// Plugin bridge settings from the `[plugins]` table.
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct PluginSettings {
    /// What a failed plugin event means for the action it guards.
    on_event_error: EventErrorPolicy,
}

/// What a failed plugin event means for the action it guards.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum EventErrorPolicy {
    #[default]
    Allow,
    Deny,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OpConfig {
    Uuid(String),
    Detailed {
        uuid: String,
        #[serde(default = "default_op_level")]
        level: u8,
    },
}
fn default_op_level() -> u8 {
    4
}
fn build_forwarding(
    mode: ForwardingMode,
    secret: &str,
) -> anyhow::Result<ferrite_net::forwarding::Forwarding> {
    Ok(match mode {
        ForwardingMode::None => ferrite_net::forwarding::Forwarding::None,
        ForwardingMode::Bungee => ferrite_net::forwarding::Forwarding::Bungee,
        ForwardingMode::Velocity => {
            anyhow::ensure!(
                !secret.is_empty(),
                "forwarding-secret must be set when forwarding = 'velocity'"
            );
            ferrite_net::forwarding::Forwarding::Velocity {
                secret: secret.to_owned(),
            }
        }
    })
}
fn parse_ops(ops: Vec<OpConfig>) -> anyhow::Result<Vec<game::Operator>> {
    ops.into_iter()
        .map(|op| {
            let (id, level) = match op {
                OpConfig::Uuid(id) => (id, 4),
                OpConfig::Detailed { uuid, level } => (uuid, level),
            };
            anyhow::ensure!(
                level <= 4,
                "operator permission level must be between 0 and 4"
            );
            let uuid = uuid::Uuid::parse_str(&id)
                .with_context(|| format!("invalid operator UUID: {id}"))?;
            Ok(game::Operator { uuid, level })
        })
        .collect()
}
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum WorldGenerator {
    /// The superflat world used by the existing tests and examples.
    Superflat,
    /// The 26.3 overworld noise generator with surface rules and carvers.
    #[default]
    Vanilla,
}
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum GameMode {
    Survival,
    #[default]
    Creative,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SupportedVersionsMode {
    #[default]
    All,
    Native,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ForwardingMode {
    #[default]
    None,
    Velocity,
    Bungee,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0:25565".into(),
            online_mode: true,
            enforce_secure_profile: true,
            motd: "A Ferrite server".into(),
            server_icon: Some("server-icon.png".into()),
            max_players: 20,
            view_distance: 10,
            supported_versions: SupportedVersionsMode::All,
            compression_threshold: 256,
            forwarding: ForwardingMode::None,
            forwarding_secret: String::new(),
            world: None,
            generator: WorldGenerator::Vanilla,
            gamemode: GameMode::Creative,
            ops: Vec::new(),
            plugins_dir: Some("plugins".into()),
            plugin_bridge: None,
            plugins: PluginSettings::default(),
            whitelist: false,
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let config: Config = match std::fs::read_to_string("ferrite.toml") {
        Ok(content) => toml::from_str(&content).context("invalid ferrite.toml")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Config::default(),
        Err(error) => return Err(error).context("reading ferrite.toml"),
    };
    let ops = parse_ops(config.ops)?;
    let forwarding = build_forwarding(config.forwarding, &config.forwarding_secret)?;
    let favicon = config.server_icon.as_deref().and_then(load_favicon);
    if forwarding.is_proxy() && config.online_mode {
        tracing::warn!(
            "online-mode is ignored when player forwarding is enabled; authenticate players on the proxy"
        );
    }
    let admin =
        std::sync::Arc::new(ferrite_admin::AdminLists::load(".").context("loading admin lists")?);
    if !config.online_mode && !ops.is_empty() {
        tracing::warn!(
            "operator UUIDs in offline mode must be offline-mode UUIDs; player names are not authenticated"
        );
    }
    let listener = tokio::net::TcpListener::bind(&config.bind_address)
        .await
        .context("binding server listener")?;
    tracing::info!(address = %listener.local_addr()?, online_mode = config.online_mode, view_distance = config.view_distance, "Ferrite listener started");

    let spawn = if let Some(path) = &config.world {
        game::load_spawn(path).context("reading world spawn")?
    } else {
        [0, 65, 0]
    };
    let bind_host = config
        .bind_address
        .rsplit_once(':')
        .map_or_else(|| config.bind_address.clone(), |(host, _)| host.to_owned());
    let vanilla_data = std::env::var_os("FERRITE_VANILLA_ROOT").map_or_else(
        || std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3/generated"),
        std::path::PathBuf::from,
    );
    // Plugins call the Bukkit API from their own threads; the game loop owns the world, so
    // player mutations (inventory, teleport, gamemode, ...) arrive through this channel and are
    // routed to the region that owns the player.
    let (plugin_command_tx, plugin_commands) = tokio::sync::mpsc::unbounded_channel();
    let terrain = match config.generator {
        WorldGenerator::Vanilla => Some(std::sync::Arc::new(
            game::VanillaTerrain::spawn(0).context("starting the vanilla world generator")?,
        )),
        WorldGenerator::Superflat => None,
    };
    let source = if let Some(path) = &config.world {
        let registry = ferrite_world::RuntimeRegistry::load("vanilla/26.3/generated")
            .context("loading vanilla registry")?;
        game::new_anvil_source_with(path.clone(), registry, terrain)
    } else {
        game::new_memory_source(terrain)
    };
    // Vanilla terrain chooses a spawn above the generated surface; the flat world keeps the
    // fixed `[0, 65, 0]` the existing tests rely on.
    let spawn = if source.is_flat() || config.world.is_some() {
        spawn
    } else {
        [0, source.spawn_height(0, 0).unwrap_or(100), 0]
    };
    let plugin_bridge =
        ferrite_server::plugins::PluginBridge::start(ferrite_server::plugins::Settings {
            directory: config.plugins_dir.unwrap_or_else(|| "plugins".into()),
            bridge_jar: config.plugin_bridge.unwrap_or_else(|| {
                "java/ferrite-api-impl/build/libs/ferrite-api-impl-all.jar".into()
            }),
            data_dir: vanilla_data,
            bind_address: bind_host,
            motd: config.motd.clone(),
            max_players: u16::try_from(config.max_players).unwrap_or(u16::MAX),
            port: listener.local_addr()?.port(),
            view_distance: config.view_distance,
            online_mode: config.online_mode,
            whitelist: false,
            world: ferrite_plugin::WorldSnapshot {
                id: ferrite_plugin::default_world_id(),
                name: "world".into(),
                time: 0,
                spawn: ferrite_plugin::BlockPosition {
                    x: spawn[0],
                    y: spawn[1],
                    z: spawn[2],
                },
                blocks: Vec::new(),
                heights: Vec::new(),
            },
            player_handler: Some(std::sync::Arc::new(move |command| {
                let _ = plugin_command_tx.send(command);
            })),
            world_view: Some(ferrite_server::plugins::world_view(std::sync::Arc::clone(
                &source,
            ))),
        })
        .context("starting plugins")?;
    let settings = game::WorldSettings {
        dimension: game::Dimension::Overworld,
        raining: false,
        gamemode: match config.gamemode {
            GameMode::Survival => 0,
            GameMode::Creative => 1,
        },
        max_players: i32::try_from(config.max_players).unwrap_or(i32::MAX),
        ops,
        plugins: plugin_bridge
            .as_ref()
            .map(ferrite_server::plugins::PluginBridge::hooks),
        plugin_event_errors: match config.plugins.on_event_error {
            EventErrorPolicy::Allow => game::PluginEventPolicy::Allow,
            EventErrorPolicy::Deny => game::PluginEventPolicy::Deny,
        },
        admin: Some(admin),
        whitelist: config.whitelist,
        playerdata: Some(
            config
                .world
                .as_ref()
                .map_or_else(|| "playerdata".into(), |w| w.join("playerdata")),
        ),
        plugin_commands: Some(plugin_commands),
        dimensions: None,
    };
    let world = game::World::start_configured(Some(source), spawn, settings);
    let mut server = tokio::spawn(ferrite_net::serve(
        listener,
        ferrite_net::ServerConfig {
            motd: config.motd,
            max_players: config.max_players,
            online_mode: config.online_mode,
            encrypt_offline: false,
            enforce_secure_profile: config.enforce_secure_profile,
            compression_threshold: config.compression_threshold,
            view_distance: config.view_distance,
            supported_versions: match config.supported_versions {
                SupportedVersionsMode::All => ferrite_net::SupportedVersions::All,
                SupportedVersionsMode::Native => ferrite_net::SupportedVersions::Native,
            },
            forwarding,
            favicon,
            status: Default::default(),
            read_timeout: ferrite_net::DEFAULT_READ_TIMEOUT,
            game: Some(world.sender.clone()),
            ..Default::default()
        },
    ));
    let (console_tx, mut console_rx) = tokio::sync::mpsc::unbounded_channel();
    std::thread::spawn(move || {
        use std::io::BufRead;
        for line in std::io::stdin().lock().lines() {
            match line {
                Ok(line) => {
                    if console_tx.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    let mut stopped = world.stop_signal();
    let result = loop {
        tokio::select! {
            result = &mut server => break Some(result),
            _ = tokio::signal::ctrl_c() => break None,
            Some(command) = console_rx.recv() => match world.console_command(command).await {
                Ok(true) => break None,
                Ok(false) => {},
                Err(error) => tracing::warn!(%error, "console command failed"),
            },
            _ = async {
                loop {
                    if *stopped.borrow() { break; }
                    if stopped.changed().await.is_err() { break; }
                }
            } => break None,
        }
    };
    let world_result = world.shutdown().await;
    if let Some(bridge) = plugin_bridge
        && let Err(error) = bridge.shutdown()
    {
        tracing::error!(%error, "plugin shutdown failed");
    }
    world_result?;
    if let Some(result) = result {
        result??;
    } else {
        server.abort();
    }
    Ok(())
}
/// Load the server-list icon, keeping the server up when it is missing or malformed.
fn load_favicon(path: &std::path::Path) -> Option<String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tracing::warn!(
                path = %path.display(),
                "server icon not found; continuing without a favicon"
            );
            return None;
        }
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                %error,
                "cannot read server icon; continuing without a favicon"
            );
            return None;
        }
    };
    match favicon_data_uri(&bytes) {
        Ok(uri) => Some(uri),
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                %error,
                "ignoring server icon; continuing without a favicon"
            );
            None
        }
    }
}

/// Validate a 64x64 PNG and encode it as the `data:image/png;base64,...` favicon vanilla expects.
fn favicon_data_uri(png: &[u8]) -> Result<String, String> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if png.len() < 33 || png[..8] != SIGNATURE {
        return Err("not a PNG file".into());
    }
    if png[8..12] != 13u32.to_be_bytes() || &png[12..16] != b"IHDR" {
        return Err("missing PNG IHDR header".into());
    }
    let width = u32::from_be_bytes(png[16..20].try_into().expect("four bytes"));
    let height = u32::from_be_bytes(png[20..24].try_into().expect("four bytes"));
    if (width, height) != (64, 64) {
        return Err(format!("server icon must be 64x64, got {width}x{height}"));
    }
    use base64::Engine;
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG signature plus an IHDR chunk with the given dimensions (payload otherwise absent).
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        out.extend_from_slice(&13u32.to_be_bytes());
        out.extend_from_slice(b"IHDR");
        out.extend_from_slice(&width.to_be_bytes());
        out.extend_from_slice(&height.to_be_bytes());
        out.extend_from_slice(&[8, 6, 0, 0, 0]);
        out.extend_from_slice(&[0; 4]);
        out
    }

    #[test]
    fn favicon_encodes_a_64x64_png() {
        let source = png(64, 64);
        let uri = favicon_data_uri(&source).unwrap();
        assert!(uri.starts_with("data:image/png;base64,"));
        use base64::Engine;
        let encoded = uri.trim_start_matches("data:image/png;base64,");
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .unwrap(),
            source
        );
    }

    #[test]
    fn favicon_rejects_wrong_size_and_non_png() {
        let wrong = favicon_data_uri(&png(32, 64)).unwrap_err();
        assert!(wrong.contains("64x64"), "{wrong}");
        assert!(favicon_data_uri(b"not a png").is_err());
    }

    #[test]
    fn missing_icon_is_only_a_warning() {
        assert!(load_favicon(std::path::Path::new("missing-server-icon.png")).is_none());
    }

    #[test]
    fn config_defaults_and_overrides() {
        let defaults: Config = toml::from_str("").unwrap();
        assert_eq!(defaults.bind_address, "0.0.0.0:25565");
        assert_eq!(defaults.max_players, 20);
        assert_eq!(defaults.supported_versions, SupportedVersionsMode::All);
        let native: Config = toml::from_str("supported-versions = 'native'").unwrap();
        assert_eq!(native.supported_versions, SupportedVersionsMode::Native);
        assert_eq!(defaults.compression_threshold, 256);
        assert_eq!(defaults.forwarding, ForwardingMode::None);
        assert!(defaults.forwarding_secret.is_empty());
        assert_eq!(defaults.world, None);
        assert_eq!(defaults.generator, WorldGenerator::Vanilla);
        assert_eq!(
            toml::from_str::<Config>("generator = 'superflat'")
                .unwrap()
                .generator,
            WorldGenerator::Superflat
        );
        assert!(matches!(
            build_forwarding(ForwardingMode::None, "").unwrap(),
            ferrite_net::forwarding::Forwarding::None
        ));
        assert!(build_forwarding(ForwardingMode::Velocity, "").is_err());
        assert_eq!(
            build_forwarding(ForwardingMode::Velocity, "hunter2").unwrap(),
            ferrite_net::forwarding::Forwarding::Velocity {
                secret: "hunter2".into()
            }
        );
        assert_eq!(
            defaults.plugins_dir.as_deref(),
            Some(std::path::Path::new("plugins"))
        );
        assert_eq!(defaults.plugin_bridge, None);
        assert_eq!(defaults.plugins.on_event_error, EventErrorPolicy::Allow);
        let denied: Config = toml::from_str("[plugins]\non-event-error = 'deny'").unwrap();
        assert_eq!(denied.plugins.on_event_error, EventErrorPolicy::Deny);
        assert_eq!(
            defaults.server_icon.as_deref(),
            Some(std::path::Path::new("server-icon.png"))
        );
        assert!(parse_ops(vec![OpConfig::Uuid("Alice".into())]).is_err());
        let parsed = parse_ops(vec![OpConfig::Detailed {
            uuid: "00000000-0000-0000-0000-00000000000a".into(),
            level: 3,
        }])
        .unwrap();
        assert_eq!(parsed[0].level, 3);
        assert_eq!(parsed[0].uuid, uuid::Uuid::from_u128(10));
        let custom: Config =
            toml::from_str("bind-address = '127.0.0.1:0'\nmotd = 'Hello'\nmax-players = 42")
                .unwrap();
        assert_eq!(custom.motd, "Hello");
        assert_eq!(custom.max_players, 42);
        let configured: Config = toml::from_str("world = 'fixture-world'").unwrap();
        assert_eq!(
            configured.world.as_deref(),
            Some(std::path::Path::new("fixture-world"))
        );
    }
}
