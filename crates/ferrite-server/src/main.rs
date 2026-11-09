#![cfg_attr(windows, windows_subsystem = "windows")]

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
    compression_threshold: usize,
    forwarding: ForwardingMode,
    forwarding_secret: String,
    world: Option<std::path::PathBuf>,
    /// New chunks come from the flat test world or the vanilla world generation host.
    generator: WorldGenerator,
    gamemode: GameMode,
    ops: Vec<OpConfig>,
    plugins_dir: Option<std::path::PathBuf>,
    plugin_bridge: Option<std::path::PathBuf>,
    /// Plugin bridge settings from the `[plugins]` table.
    plugins: PluginSettings,
    /// Enforce `whitelist.json` for new connections.
    whitelist: bool,
    /// Settings for the vanilla world generation host (`generator = 'vanilla'`).
    worldgen: WorldgenSettings,
}

/// The `[worldgen]` table: how the vanilla world generation host is run. It uses the user's own
/// Minecraft server jar in a separate JVM and needs `eula=true` in `eula.txt`.
#[derive(Debug, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct WorldgenSettings {
    /// Heap of the host JVM in MiB.
    heap_mb: u32,
    /// Chunk requests the host serves at the same time.
    threads: u32,
    /// Extra JVM options for the host.
    jvm_args: Vec<String>,
    /// A Java 25+ installation to use instead of the discovered one.
    java_home: Option<std::path::PathBuf>,
    /// The world seed (a number or any text, as in `server.properties`).
    seed: String,
    /// Build a patched copy of the jar on this machine (rules in `patch-rules`) that keeps only
    /// world generation, and run that instead of the unmodified jar.
    patch: bool,
    /// The rule file for `patch`.
    patch_rules: std::path::PathBuf,
}
impl Default for WorldgenSettings {
    fn default() -> Self {
        Self {
            heap_mb: 1024,
            threads: 4,
            jvm_args: Vec::new(),
            java_home: None,
            seed: "0".into(),
            patch: true,
            patch_rules: "java/ferrite-worldgen-host/rules/worldgen.rules".into(),
        }
    }
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
    #[default]
    Superflat,
    /// Mojang's own generator, run by the world generation host from the user's server jar.
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
            compression_threshold: 256,
            forwarding: ForwardingMode::None,
            forwarding_secret: String::new(),
            world: Some(std::path::PathBuf::from("world")),
            generator: WorldGenerator::Superflat,
            gamemode: GameMode::Creative,
            ops: Vec::new(),
            plugins_dir: Some("plugins".into()),
            plugin_bridge: None,
            plugins: PluginSettings::default(),
            whitelist: false,
            worldgen: WorldgenSettings::default(),
        }
    }
}

/// Written next to the executable (or wherever the server was started) when `ferrite.toml` is
/// missing. Keeping the full default here means the first run produces an editable file instead
/// of silently using in-memory defaults that leave no world behind.
const DEFAULT_CONFIG: &str = r#"# Ferrite server configuration.
# Relative paths (world, plugins-dir, server-icon, ...) are resolved against the process
# working directory. Start the server from this folder to keep them next to the executable.

bind-address = "0.0.0.0:25565"
online-mode = true
enforce-secure-profile = true
motd = "A Ferrite server"
server-icon = "server-icon.png"
max-players = 20
view-distance = 10
compression-threshold = 256

# World directory. It is created on first start and saved on chunk unload, autosave and a
# clean shutdown. Use "generator = 'vanilla'" for Mojang terrain, or "superflat" for tests.
world = "world"
generator = "superflat"
gamemode = "creative"

plugins-dir = "plugins"

# Player forwarding: "none", "velocity" or "bungee".
forwarding = "none"
forwarding-secret = ""
whitelist = false

[plugins]
on-event-error = "allow"
"#;

/// Reads `ferrite.toml`, creating it with [`DEFAULT_CONFIG`] on the first run. A failed write is
/// only a warning: the server still starts with the same defaults.
fn load_config() -> anyhow::Result<(Config, bool)> {
    let path = std::path::Path::new("ferrite.toml");
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let config = toml::from_str(&content).context("invalid ferrite.toml")?;
            let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
            tracing::info!(config = %absolute.display(), "loaded server configuration");
            Ok((config, false))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let config: Config = toml::from_str(DEFAULT_CONFIG).expect("built-in config is valid");
            match std::fs::write(path, DEFAULT_CONFIG) {
                Ok(()) => tracing::info!(
                    config = %std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()).display(),
                    "created the default ferrite.toml; it saves to ./world"
                ),
                Err(error) => tracing::warn!(
                    %error,
                    "could not create ferrite.toml; using the defaults without saving the file"
                ),
            }
            Ok((config, true))
        }
        Err(error) => Err(error).context("reading ferrite.toml"),
    }
}

/// First-run spawn: an existing world's `level.dat` wins; a new world starts at the fixed flat
/// spawn and gets a `level.dat` written after the terrain spawn is known.
fn spawn_for_world(path: &std::path::Path) -> anyhow::Result<[i32; 3]> {
    if path.join("level.dat").exists() {
        game::load_spawn(path).context("reading world spawn")
    } else {
        std::fs::create_dir_all(path)
            .with_context(|| format!("creating world {}: ", path.display()))?;
        Ok([0, 65, 0])
    }
}

/// Clients accept 2..=32 chunks. Clamp once so the wire, chunk streaming and the server
/// snapshot plugins read all see the same effective distance.
const MIN_VIEW_DISTANCE: u8 = 2;
const MAX_VIEW_DISTANCE: u8 = 32;
fn effective_view_distance(configured: u8) -> u8 {
    configured.clamp(MIN_VIEW_DISTANCE, MAX_VIEW_DISTANCE)
}

#[cfg(not(windows))]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    run_server(stdin_commands()).await
}

#[cfg(windows)]
fn main() {
    // Keep a console-capable mode for service managers, scripts and the integration tests.
    // Normal Windows launches use the embedded GUI; Linux always enters the headless path above.
    if std::env::args().any(|arg| arg == "--headless") {
        if let Err(error) = run_headless() {
            eprintln!("Ferrite Server failed: {error:#}");
            std::process::exit(1);
        }
        return;
    }

    if let Err(error) = windows_gui::run() {
        native_windows_gui::simple_message(
            "Ferrite Server 시작 실패",
            &format!("Ferrite Server GUI를 시작하지 못했습니다.\n\n{error:#}"),
        );
    }
}

#[cfg(windows)]
fn run_headless() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(run_server(stdin_commands()))
}

fn stdin_commands() -> tokio::sync::mpsc::UnboundedReceiver<String> {
    let (console_tx, console_rx) = tokio::sync::mpsc::unbounded_channel();
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
    console_rx
}

async fn run_server(
    mut console_rx: tokio::sync::mpsc::UnboundedReceiver<String>,
) -> anyhow::Result<()> {
    let (config, _created) = load_config()?;
    // Ferrite never accepts the Mojang EULA for the user; the world generation host reads the
    // same `eula.txt`.
    ferrite_server::eula::ensure_accepted(std::path::Path::new("eula.txt"))?;
    let view_distance = effective_view_distance(config.view_distance);
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
    tracing::info!(address = %listener.local_addr()?, online_mode = config.online_mode, view_distance = view_distance, "Ferrite listener started");

    let spawn = match &config.world {
        Some(path) => spawn_for_world(path)?,
        None => [0, 65, 0],
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
    let (plugin_console_tx, plugin_console) = tokio::sync::mpsc::unbounded_channel();
    let terrain: Option<std::sync::Arc<dyn game::ChunkGenerator>> = match config.generator {
        WorldGenerator::Vanilla => {
            let registry = std::sync::Arc::new(
                ferrite_world::RuntimeRegistry::load(&vanilla_data)
                    .context("loading vanilla registry")?,
            );
            let mut host = game::HostSettings::standard(std::path::Path::new("."));
            host.heap_mb = config.worldgen.heap_mb;
            host.threads = config.worldgen.threads.max(1);
            host.jvm_args = config.worldgen.jvm_args.clone();
            host.java_home = config.worldgen.java_home.clone();
            host.seed = config.worldgen.seed.clone();
            host.patch_rules = config
                .worldgen
                .patch
                .then(|| config.worldgen.patch_rules.clone());
            tracing::info!("starting the vanilla world generation host (this takes a few seconds)");
            let generator =
                tokio::task::block_in_place(|| game::VanillaGenerator::spawn(&host, registry))
                    .context("starting the vanilla world generator")?;
            Some(std::sync::Arc::new(generator))
        }
        WorldGenerator::Superflat => None,
    };
    let terrain_for_plugins = terrain.clone();
    let source = if let Some(path) = &config.world {
        let registry = ferrite_world::RuntimeRegistry::load(&vanilla_data)
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
    if let Some(path) = &config.world {
        // The vanilla save format puts the world spawn in `level.dat`. Write it once for a new
        // world so a restart (and `read_spawn`) sees the same position.
        if !path.join("level.dat").exists()
            && let Err(error) = game::write_spawn(path, spawn)
        {
            tracing::warn!(%error, world = %path.display(), "could not write level.dat");
        }
    }
    ferrite_server::plugins::chunks::set_world_dir(config.world.clone());
    // The plugin bridge and the game loop share these sources, so a plugin write lands in the
    // same chunk data the server ticks.
    let dimensions = game::DimensionRegistry::with_source(
        game::Dimension::Overworld,
        std::sync::Arc::clone(&source),
        spawn,
    );
    let plugin_worlds = [game::Dimension::Nether, game::Dimension::End]
        .into_iter()
        .map(|dimension| {
            let spawn = dimensions.spawn(dimension);
            ferrite_plugin::WorldSnapshot {
                id: ferrite_server::plugins::world_id(dimension),
                name: ferrite_server::plugins::world_name(dimension).into(),
                time: 0,
                spawn: ferrite_plugin::BlockPosition {
                    x: spawn[0],
                    y: spawn[1],
                    z: spawn[2],
                },
                blocks: Vec::new(),
                heights: Vec::new(),
            }
        })
        .collect();
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
            view_distance,
            online_mode: config.online_mode,
            whitelist: config.whitelist,
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
            worlds: plugin_worlds,
            player_handler: Some(std::sync::Arc::new(move |command| {
                let _ = plugin_command_tx.send(command);
            })),
            terrain: terrain_for_plugins,
            world_view: Some(ferrite_server::plugins::world_view(std::sync::Arc::clone(
                &dimensions,
            ))),
            console: Some(plugin_console_tx),
        })
        .context("starting plugins")?;
    let disable_end_credits = match ferrite_config::FerriteConfig::load("config") {
        Ok(config) => config.world_settings("world").misc.disable_end_credits,
        Err(error) => {
            tracing::warn!(%error, "reading Paper world config failed; End credits stay enabled");
            false
        }
    };
    let settings = game::WorldSettings {
        disable_end_credits,
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
        dimensions: Some(dimensions),
    };
    let world = game::World::start_configured(Some(source), spawn, settings);
    ferrite_server::plugins::forward_console(plugin_console, world.sender.clone());
    let mut server = tokio::spawn(ferrite_net::serve(
        listener,
        ferrite_net::ServerConfig {
            motd: config.motd,
            max_players: config.max_players,
            online_mode: config.online_mode,
            encrypt_offline: false,
            enforce_secure_profile: config.enforce_secure_profile,
            compression_threshold: config.compression_threshold,
            view_distance,
            forwarding,
            favicon,
            status: Default::default(),
            read_timeout: ferrite_net::DEFAULT_READ_TIMEOUT,
            game: Some(world.sender.clone()),
            ..Default::default()
        },
    ));
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
    fn view_distance_clamps_to_the_client_range() {
        assert_eq!(effective_view_distance(0), 2);
        assert_eq!(effective_view_distance(1), 2);
        assert_eq!(effective_view_distance(2), 2);
        assert_eq!(effective_view_distance(16), 16);
        assert_eq!(effective_view_distance(32), 32);
        assert_eq!(effective_view_distance(33), 32);
        assert_eq!(effective_view_distance(u8::MAX), 32);
    }

    #[test]
    fn config_defaults_and_overrides() {
        let defaults: Config = toml::from_str("").unwrap();
        assert_eq!(defaults.bind_address, "0.0.0.0:25565");
        assert_eq!(defaults.max_players, 20);
        assert_eq!(defaults.compression_threshold, 256);
        assert_eq!(defaults.forwarding, ForwardingMode::None);
        assert!(defaults.forwarding_secret.is_empty());
        assert_eq!(
            defaults.world.as_deref(),
            Some(std::path::Path::new("world"))
        );
        assert_eq!(defaults.generator, WorldGenerator::Superflat);
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

#[cfg(windows)]
mod windows_gui {
    use std::cell::RefCell;
    use std::io::{self, Write};
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::thread;
    use std::time::{Duration, Instant};

    use anyhow::Context;
    use native_windows_derive::NwgUi;
    use native_windows_gui as nwg;
    use nwg::NativeUi;
    use tracing_subscriber::fmt::MakeWriter;

    use super::run_server;

    const INITIAL_WIDTH: i32 = 900;
    const INITIAL_HEIGHT: i32 = 640;
    const MIN_WIDTH: i32 = 780;
    const MIN_HEIGHT: i32 = 540;
    const POLL_INTERVAL: Duration = Duration::from_millis(150);
    /// The console text is trimmed once the edit control reports more lines
    /// than this. Trimming drops the oldest block instead of throwing the whole
    /// log away.
    const LOG_MAX_LINES: i32 = 4000;
    /// Lines kept after a trim.
    const LOG_KEEP_LINES: usize = 3000;
    const HISTORY_LIMIT: usize = 200;
    const PALETTE_HANDLER_ID: usize = 0xF000_0001;

    /// Small Win32 surface that `native-windows-gui` does not expose directly.
    mod win32 {
        #[repr(C)]
        #[derive(Default)]
        pub struct Rect {
            pub left: i32,
            pub top: i32,
            pub right: i32,
            pub bottom: i32,
        }

        #[link(name = "user32")]
        unsafe extern "system" {
            pub fn SetProcessDPIAware() -> i32;
            pub fn GetSystemMetrics(index: i32) -> i32;
            pub fn GetSysColor(index: i32) -> u32;
            pub fn SetClassLongPtrW(hwnd: *mut std::ffi::c_void, index: i32, value: isize)
            -> isize;
            pub fn InvalidateRect(
                hwnd: *mut std::ffi::c_void,
                rect: *const std::ffi::c_void,
                erase: i32,
            ) -> i32;
            pub fn GetClientRect(hwnd: *mut std::ffi::c_void, rect: *mut Rect) -> i32;
            pub fn UpdateWindow(hwnd: *mut std::ffi::c_void) -> i32;
            pub fn FillRect(hdc: *mut std::ffi::c_void, rect: *const Rect, brush: isize) -> i32;
            pub fn SendMessageW(
                hwnd: *mut std::ffi::c_void,
                message: u32,
                wparam: usize,
                lparam: isize,
            ) -> isize;
        }

        #[link(name = "gdi32")]
        unsafe extern "system" {
            pub fn CreateSolidBrush(color: u32) -> isize;
            pub fn SetTextColor(hdc: *mut std::ffi::c_void, color: u32) -> u32;
            pub fn SetBkColor(hdc: *mut std::ffi::c_void, color: u32) -> u32;
        }

        pub const COLOR_WINDOW: i32 = 5;
        pub const COLOR_WINDOWTEXT: i32 = 8;
        pub const EM_GETFIRSTVISIBLELINE: u32 = 0x00CE;
        pub const GCLP_HBRBACKGROUND: i32 = -10;
        pub const WM_ERASEBKGND: u32 = 0x0014;
        pub const WM_CTLCOLORSTATIC: u32 = 0x0138;
        pub const SM_CXSCREEN: i32 = 0;
        pub const SM_CYSCREEN: i32 = 1;
    }

    enum Action {
        Start,
        Stop,
        Command(String),
        Exit,
    }

    enum UiMessage {
        Log(String),
        ServerStarted,
        ServerStopped,
    }

    struct UiState {
        actions: Option<Sender<Action>>,
        messages: Option<Receiver<UiMessage>>,
        /// A server session exists, from Start until ServerStopped.
        active: bool,
        /// The runtime reported ServerStarted and is still running.
        running: bool,
        /// Stop was requested and the runtime has not reported ServerStopped.
        stopping: bool,
        /// A close was confirmed; the window goes away after ServerStopped.
        closing: bool,
        /// Auto-scroll is on until the user toggles it.
        autoscroll_enabled: bool,
        started_at: Option<Instant>,
        scale: f32,
        last_status: String,
        last_uptime: String,
        history: Vec<String>,
        history_cursor: Option<usize>,
        history_draft: String,
    }

    impl Default for UiState {
        fn default() -> Self {
            Self {
                actions: None,
                messages: None,
                active: false,
                running: false,
                stopping: false,
                closing: false,
                autoscroll_enabled: true,
                started_at: None,
                scale: 1.0,
                last_status: String::new(),
                last_uptime: String::new(),
                history: Vec::new(),
                history_cursor: None,
                history_draft: String::new(),
            }
        }
    }

    struct UiFonts {
        status: nwg::Font,
        body: nwg::Font,
        small: nwg::Font,
        mono: nwg::Font,
    }

    impl UiFonts {
        fn new(scale: f32) -> anyhow::Result<Self> {
            fn build(
                family: &str,
                size: i32,
                weight: u32,
                scale: f32,
            ) -> anyhow::Result<nwg::Font> {
                let mut font = nwg::Font::default();
                nwg::Font::builder()
                    .family(family)
                    .size(scaled(size, scale) as u32)
                    .weight(weight)
                    .build(&mut font)
                    .map_err(|error| anyhow::anyhow!("building the {family} font: {error}"))?;
                Ok(font)
            }

            Ok(Self {
                status: build("Segoe UI", 16, 600, scale)?,
                body: build("Segoe UI", 15, 400, scale)?,
                small: build("Segoe UI", 12, 400, scale)?,
                mono: build("Consolas", 14, 400, scale)?,
            })
        }
    }

    #[derive(Default, NwgUi)]
    pub struct FerriteServerGui {
        state: RefCell<UiState>,
        /// The window-level WM_CTLCOLORSTATIC hook that paints the labels.
        palette_handler: RefCell<Option<nwg::RawEventHandler>>,

        #[nwg_control(size: (INITIAL_WIDTH, INITIAL_HEIGHT), position: (0, 0), title: "Ferrite Server 콘솔", flags: "MAIN_WINDOW")]
        #[nwg_events(
            OnWindowClose: [FerriteServerGui::close(SELF, EVT_DATA)],
            OnResize: [FerriteServerGui::relayout],
            OnMinMaxInfo: [FerriteServerGui::min_max_info(SELF, EVT_DATA)],
            OnKeyEnter: [FerriteServerGui::send_command]
        )]
        window: nwg::Window,

        #[nwg_control(text: "● 중지됨")]
        status: nwg::Label,

        #[nwg_control(text: "")]
        uptime: nwg::Label,

        #[nwg_control(text: "서버 시작(&S)")]
        #[nwg_events(OnButtonClick: [FerriteServerGui::start])]
        start_button: nwg::Button,

        #[nwg_control(text: "서버 종료(&T)", enabled: false)]
        #[nwg_events(OnButtonClick: [FerriteServerGui::stop])]
        stop_button: nwg::Button,

        #[nwg_control(text: "콘솔 로그")]
        log_title: nwg::Label,

        #[nwg_control(text: "자동 스크롤(&A): 켜짐")]
        #[nwg_events(OnButtonClick: [FerriteServerGui::autoscroll_toggled])]
        autoscroll: nwg::Button,

        #[nwg_control(text: "로그 지우기(&L)")]
        #[nwg_events(OnButtonClick: [FerriteServerGui::clear_log])]
        clear_button: nwg::Button,

        #[nwg_control(
            text: "Ferrite 서버 콘솔\r\n시작을 누르면 서버 로그가 여기에 표시됩니다.\r\n",
            readonly: true
        )]
        log: nwg::TextBox,

        #[nwg_control(text: "명령")]
        command_label: nwg::Label,

        #[nwg_control(size: (200, 26))]
        #[nwg_events(
            OnKeyEnter: [FerriteServerGui::send_command],
            OnKeyPress: [FerriteServerGui::command_key(SELF, EVT_DATA)]
        )]
        command: nwg::TextInput,

        #[nwg_control(text: "보내기(&D)", enabled: false)]
        #[nwg_events(OnButtonClick: [FerriteServerGui::send_command])]
        send_button: nwg::Button,

        #[nwg_control(text: "Enter 전송 · ↑/↓ 명령 기록 · Tab 이동 · Alt+S 시작 · Alt+T 종료")]
        hint: nwg::Label,

        #[nwg_control(parent: window, interval: POLL_INTERVAL, active: true)]
        #[nwg_events(OnTimerTick: [FerriteServerGui::poll_messages])]
        poll_timer: nwg::AnimationTimer,
    }

    impl FerriteServerGui {
        fn start(&self) {
            {
                let mut state = self.state.borrow_mut();
                if state.active {
                    return;
                }
                let Some(actions) = state.actions.clone() else {
                    return;
                };
                if actions.send(Action::Start).is_err() {
                    return;
                }
                state.active = true;
                state.running = false;
                state.stopping = false;
                state.closing = false;
                state.started_at = None;
            }

            self.set_status("● 시작 중…");
            self.set_uptime("");
            self.uptime.set_visible(false);
            self.start_button.set_enabled(false);
            self.stop_button.set_enabled(true);
            self.command.set_enabled(true);
            self.send_button.set_enabled(true);
            self.command.set_focus();
        }

        fn stop(&self) {
            {
                let mut state = self.state.borrow_mut();
                if !state.active || state.stopping {
                    return;
                }
                let Some(actions) = state.actions.clone() else {
                    return;
                };
                if actions.send(Action::Stop).is_err() {
                    return;
                }
                state.stopping = true;
            }

            self.set_shutdown_ui();
        }

        fn send_command(&self) {
            let command = self.command.text().trim().to_owned();
            if command.is_empty() {
                return;
            }

            let actions = {
                let mut state = self.state.borrow_mut();
                if !state.active || state.stopping {
                    return;
                }
                if state.history.last().map(String::as_str) != Some(command.as_str()) {
                    state.history.push(command.clone());
                    if state.history.len() > HISTORY_LIMIT {
                        state.history.remove(0);
                    }
                }
                state.history_cursor = None;
                state.history_draft.clear();
                state.actions.clone()
            };

            let Some(actions) = actions else {
                return;
            };
            if actions.send(Action::Command(command)).is_ok() {
                self.command.set_text("");
            }
            self.command.set_focus();
        }

        fn command_key(&self, evt_data: &nwg::EventData) {
            if let nwg::EventData::OnKey(key) = evt_data {
                match *key {
                    nwg::keys::UP => self.history_previous(),
                    nwg::keys::DOWN => self.history_next(),
                    // The dialog manager may deliver Enter as WM_COMMAND/IDOK
                    // instead of WM_KEYDOWN; the window-level OnKeyEnter above
                    // covers that path. Handle the WM_KEYDOWN path here too.
                    nwg::keys::RETURN => self.send_command(),
                    _ => {}
                }
            }
        }

        fn history_previous(&self) {
            let mut state = self.state.borrow_mut();
            if state.history.is_empty() {
                return;
            }
            let next = match state.history_cursor {
                None => {
                    state.history_draft = self.command.text();
                    state.history.len() - 1
                }
                Some(0) => 0,
                Some(index) => index - 1,
            };
            state.history_cursor = Some(next);
            let text = state.history[next].clone();
            drop(state);
            self.show_history(&text);
        }

        fn history_next(&self) {
            let mut state = self.state.borrow_mut();
            let next = match state.history_cursor {
                Some(index) if index + 1 < state.history.len() => Some(index + 1),
                Some(_) => None,
                None => return,
            };
            state.history_cursor = next;
            let text = match next {
                Some(index) => state.history[index].clone(),
                None => std::mem::take(&mut state.history_draft),
            };
            drop(state);
            self.show_history(&text);
        }

        fn show_history(&self, text: &str) {
            self.command.set_text(text);
            let end = self.command.len();
            self.command.set_selection(0..end);
            self.command.set_focus();
        }

        fn clear_log(&self) {
            self.log.clear();
            let can_type = {
                let state = self.state.borrow();
                state.active && !state.stopping
            };
            if can_type {
                self.command.set_focus();
            } else if self.start_button.enabled() {
                self.start_button.set_focus();
            }
        }

        fn autoscroll_toggled(&self) {
            let enabled = {
                let mut state = self.state.borrow_mut();
                state.autoscroll_enabled = !state.autoscroll_enabled;
                state.autoscroll_enabled
            };
            self.autoscroll.set_text(if enabled {
                "자동 스크롤(&A): 켜짐"
            } else {
                "자동 스크롤(&A): 꺼짐"
            });
            if enabled {
                self.log.scroll_lastline();
            }
        }

        fn close(&self, evt_data: &nwg::EventData) {
            let nwg::EventData::OnWindowClose(data) = evt_data else {
                return;
            };
            let (active, stopping, closing) = {
                let state = self.state.borrow();
                (state.active, state.stopping, state.closing)
            };

            if !active {
                data.close(true);
                if let Some(actions) = self.state.borrow().actions.clone() {
                    let _ = actions.send(Action::Exit);
                }
                nwg::stop_thread_dispatch();
                return;
            }

            // Keep the window alive until the save-and-stop path has finished.
            data.close(false);
            if closing {
                return;
            }
            if stopping {
                // Stop is already in flight; don't ask twice.
                self.begin_exit();
                return;
            }

            let params = nwg::MessageParams {
                title: "Ferrite Server 종료",
                content: "실행 중인 서버를 저장하고 종료할까요?\n\n'예'를 누르면 월드 저장과 정상 종료가 끝난 뒤 창이 닫힙니다.",
                buttons: nwg::MessageButtons::YesNo,
                icons: nwg::MessageIcons::Question,
            };
            if nwg::modal_message(&self.window, &params) == nwg::MessageChoice::Yes {
                self.begin_exit();
            }
        }

        fn begin_exit(&self) {
            let sent = {
                let mut state = self.state.borrow_mut();
                if state.closing {
                    return;
                }
                state.closing = true;
                state.stopping = true;
                match state.actions.clone() {
                    Some(actions) => actions.send(Action::Exit).is_ok(),
                    None => false,
                }
            };

            if sent {
                self.set_shutdown_ui();
            } else {
                // The manager is gone, so no ServerStopped message will arrive.
                nwg::stop_thread_dispatch();
            }
        }

        fn set_shutdown_ui(&self) {
            self.set_status("● 저장 후 종료 중…");
            self.start_button.set_enabled(false);
            self.stop_button.set_enabled(false);
            self.command.set_enabled(false);
            self.send_button.set_enabled(false);
        }

        fn poll_messages(&self) {
            let messages = self
                .state
                .borrow()
                .messages
                .as_ref()
                .map(|receiver| receiver.try_iter().collect::<Vec<_>>())
                .unwrap_or_default();

            let mut lines = Vec::new();
            let mut started = false;
            let mut stopped = false;
            for message in messages {
                match message {
                    UiMessage::Log(line) => lines.push(line),
                    UiMessage::ServerStarted => started = true,
                    UiMessage::ServerStopped => stopped = true,
                }
            }

            if started {
                self.server_started();
            }
            if stopped {
                self.server_stopped();
            }
            if !lines.is_empty() {
                self.append_log(&lines);
            }
            self.refresh_uptime();
        }

        fn server_started(&self) {
            let (closing, stopping) = {
                let mut state = self.state.borrow_mut();
                state.running = true;
                state.started_at = Some(Instant::now());
                (state.closing, state.stopping)
            };
            if closing || stopping {
                return;
            }

            self.set_status("● 실행 중");
            self.set_uptime(&format_uptime(Duration::ZERO));
            self.uptime.set_visible(true);
            self.start_button.set_enabled(false);
            self.stop_button.set_enabled(true);
            self.command.set_enabled(true);
            self.send_button.set_enabled(true);
            self.command.set_focus();
        }

        fn server_stopped(&self) {
            let closing = {
                let mut state = self.state.borrow_mut();
                state.active = false;
                state.running = false;
                state.stopping = false;
                state.started_at = None;
                state.closing
            };

            self.stop_button.set_enabled(false);
            self.command.set_enabled(false);
            self.send_button.set_enabled(false);
            self.set_uptime("");
            self.uptime.set_visible(false);

            if closing {
                self.set_status("● 종료됨");
                if let Some(actions) = self.state.borrow().actions.clone() {
                    let _ = actions.send(Action::Exit);
                }
                nwg::stop_thread_dispatch();
            } else {
                self.start_button.set_enabled(true);
                self.set_status("● 중지됨");
                self.start_button.set_focus();
            }
        }

        fn refresh_uptime(&self) {
            let uptime = {
                let state = self.state.borrow();
                if state.running {
                    state
                        .started_at
                        .map(|started_at| format_uptime(started_at.elapsed()))
                } else {
                    None
                }
            };
            if let Some(uptime) = uptime {
                self.set_uptime(&uptime);
            }
        }

        fn set_status(&self, text: &str) {
            let changed = {
                let mut state = self.state.borrow_mut();
                if state.last_status == text {
                    false
                } else {
                    state.last_status.clear();
                    state.last_status.push_str(text);
                    true
                }
            };
            if changed {
                self.status.set_text(text);
            }
        }

        fn set_uptime(&self, text: &str) {
            let changed = {
                let mut state = self.state.borrow_mut();
                if state.last_uptime == text {
                    false
                } else {
                    state.last_uptime.clear();
                    state.last_uptime.push_str(text);
                    true
                }
            };
            if changed {
                self.uptime.set_text(text);
            }
        }

        fn append_log(&self, lines: &[String]) {
            let autoscroll = self.state.borrow().autoscroll_enabled;
            // `TextBox::append` always scrolls to the bottom. Remember where the
            // user was reading so auto-scroll off does not jump on every batch.
            let anchor = if autoscroll {
                0
            } else {
                self.first_visible_line()
            };

            let mut batch = String::new();
            for line in lines {
                batch.push_str(line);
                batch.push_str("\r\n");
            }
            self.log.append(&batch);

            if self.log.linecount() > LOG_MAX_LINES {
                self.trim_log(autoscroll, anchor);
            } else if !autoscroll {
                self.restore_scroll(anchor);
            }
        }

        fn trim_log(&self, autoscroll: bool, anchor: i32) {
            // The edit control API has no "delete a range of lines" message, so
            // take the whole text, drop the oldest block, and set it back. This
            // only happens after thousands of lines, so the full read/write is
            // rare and much cheaper than clearing the log outright.
            let text = self.log.text();
            let total = text.lines().count();
            if total == 0 {
                return;
            }
            let keep = if total > LOG_KEEP_LINES {
                LOG_KEEP_LINES
            } else {
                total.div_ceil(2)
            };
            let drop_count = total.saturating_sub(keep);
            if drop_count == 0 {
                return;
            }

            let kept = keep_newest_lines(&text, drop_count);
            self.log.set_text_unix2dos(&kept);
            self.log.scroll(-self.log.linecount());
            if autoscroll {
                self.log.scroll_lastline();
            } else {
                let target = (anchor - drop_count as i32).max(0);
                self.log.scroll(target);
            }
        }

        fn first_visible_line(&self) -> i32 {
            self.log.handle.hwnd().map_or(0, |handle| unsafe {
                win32::SendMessageW(
                    handle as *mut std::ffi::c_void,
                    win32::EM_GETFIRSTVISIBLELINE,
                    0,
                    0,
                ) as i32
            })
        }

        fn restore_scroll(&self, anchor: i32) {
            let current = self.first_visible_line();
            let delta = anchor - current;
            if delta != 0 {
                self.log.scroll(delta);
            }
        }

        fn min_max_info(&self, evt_data: &nwg::EventData) {
            if let nwg::EventData::OnMinMaxInfo(info) = evt_data {
                let scale = self.state.borrow().scale;
                // The `high-dpi` feature of native-windows-gui is off, so this
                // conversion is not done by the library.
                info.set_min_size(scaled(MIN_WIDTH, scale), scaled(MIN_HEIGHT, scale));
            }
        }

        /// Manual grid: the header, toolbar, command row and footer keep fixed
        /// heights while the log takes every remaining pixel. A `GridLayout`
        /// splits rows evenly and cannot express that.
        fn relayout(&self) {
            let scale = self.state.borrow().scale;
            let window_size = self.window.size();
            let width = (window_size.0 as i32).max(1);
            let height = (window_size.1 as i32).max(1);

            let pad = scaled(16, scale);
            let gap = scaled(8, scale);
            let content_width = (width - pad * 2).max(1);

            let status_height = scaled(36, scale);
            let button_width = scaled(112, scale);
            let button_height = scaled(28, scale);
            let action_width = button_width * 2 + gap;
            let toolbar_height = scaled(26, scale);
            let command_height = scaled(32, scale);
            let footer_height = scaled(16, scale);
            let uptime_width = scaled(190, scale)
                .min(content_width / 2)
                .max(scaled(120, scale));
            let status_width =
                (content_width - action_width - uptime_width - gap * 2).max(scaled(150, scale));

            let button_y = pad + (status_height - button_height) / 2;
            let stop_x = width - pad - button_width;
            let start_x = width - pad - action_width;

            self.status.set_position(pad, pad);
            self.status
                .set_size(status_width as u32, status_height as u32);

            let uptime_x = start_x - gap - uptime_width;
            self.uptime.set_position(uptime_x, pad);
            self.uptime
                .set_size(uptime_width as u32, status_height as u32);

            self.stop_button.set_position(stop_x, button_y);
            self.stop_button
                .set_size(button_width as u32, button_height as u32);
            self.start_button.set_position(start_x, button_y);
            self.start_button
                .set_size(button_width as u32, button_height as u32);

            let toolbar_y = pad + status_height + gap;
            self.log_title.set_position(pad, toolbar_y);
            self.log_title
                .set_size(scaled(140, scale) as u32, toolbar_height as u32);

            let clear_width = scaled(116, scale);
            let clear_x = width - pad - clear_width;
            self.clear_button.set_position(clear_x, toolbar_y);
            self.clear_button
                .set_size(clear_width as u32, toolbar_height as u32);

            let autoscroll_width = scaled(180, scale);
            let autoscroll_x = clear_x - gap - autoscroll_width;
            self.autoscroll.set_position(autoscroll_x, toolbar_y);
            self.autoscroll
                .set_size(autoscroll_width as u32, toolbar_height as u32);

            let log_top = toolbar_y + toolbar_height + gap;
            let footer_y = height - pad - footer_height;
            let command_y = footer_y - gap - command_height;
            let log_height = (command_y - gap - log_top).max(scaled(80, scale));
            self.log.set_position(pad, log_top);
            self.log.set_size(content_width as u32, log_height as u32);

            let command_label_width = scaled(40, scale);
            self.command_label
                .set_position(pad, command_y + (command_height - scaled(20, scale)) / 2);
            self.command_label
                .set_size(command_label_width as u32, scaled(20, scale) as u32);

            let send_width = scaled(96, scale);
            let send_x = width - pad - send_width;
            self.send_button
                .set_position(send_x, command_y + (command_height - button_height) / 2);
            self.send_button
                .set_size(send_width as u32, button_height as u32);

            let command_x = pad + command_label_width + gap;
            let input_height = scaled(26, scale);
            self.command
                .set_position(command_x, command_y + (command_height - input_height) / 2);
            self.command.set_size(
                (send_x - gap - command_x).max(scaled(80, scale)) as u32,
                input_height as u32,
            );

            self.hint.set_position(pad, footer_y);
            self.hint
                .set_size(content_width as u32, footer_height as u32);

            self.repaint_surface();
        }

        /// `NativeWindowsGuiWindow` has no class background brush and does not
        /// erase itself when it is shown. Invalidate the whole client so the
        /// `WM_ERASEBKGND` hook paints it with the sampled surface color.
        fn repaint_surface(&self) {
            let Some(hwnd) = self.window.handle.hwnd() else {
                return;
            };
            unsafe {
                win32::InvalidateRect(hwnd as *mut std::ffi::c_void, std::ptr::null(), 1);
                win32::UpdateWindow(hwnd as *mut std::ffi::c_void);
            }
        }

        fn install_palette(&self) -> anyhow::Result<()> {
            let background = unsafe { win32::GetSysColor(win32::COLOR_WINDOW) };
            let system_text = unsafe { win32::GetSysColor(win32::COLOR_WINDOWTEXT) };
            let dark = brightness(system_text) > brightness(background);
            let surface = if dark { [37, 38, 47] } else { [255, 255, 255] };
            let text = if dark { [240, 240, 240] } else { [24, 24, 27] };
            let brush = unsafe { win32::CreateSolidBrush(colorref(surface)) };
            if brush == 0 {
                anyhow::bail!("creating the console background brush");
            }
            let Some(hwnd) = self.window.handle.hwnd() else {
                anyhow::bail!("the server window has no handle");
            };

            // The class brush and the raw colour handler both own this GDI
            // object for the life of the window. One leaked brush per process
            // is cheaper than threading a Drop guard through the event loop.
            unsafe {
                win32::SetClassLongPtrW(
                    hwnd as *mut std::ffi::c_void,
                    win32::GCLP_HBRBACKGROUND,
                    brush,
                );
                win32::InvalidateRect(hwnd as *mut std::ffi::c_void, std::ptr::null(), 1);
            }

            let handler = nwg::bind_raw_event_handler(
                &self.window.handle,
                PALETTE_HANDLER_ID,
                move |hwnd, message, wparam, _lparam| match message {
                    win32::WM_ERASEBKGND => {
                        let hdc = wparam as *mut std::ffi::c_void;
                        let mut rect = win32::Rect::default();
                        unsafe {
                            win32::GetClientRect(hwnd as *mut std::ffi::c_void, &mut rect);
                            win32::FillRect(hdc, &rect, brush);
                        }
                        Some(1)
                    }
                    win32::WM_CTLCOLORSTATIC => {
                        let hdc = wparam as *mut std::ffi::c_void;
                        unsafe {
                            win32::SetTextColor(hdc, colorref(text));
                            win32::SetBkColor(hdc, colorref(surface));
                        }
                        Some(brush)
                    }
                    _ => None,
                },
            )
            .map_err(|error| anyhow::anyhow!("installing the console palette: {error}"))?;
            self.palette_handler.replace(Some(handler));
            Ok(())
        }

        fn apply_fonts(&self, fonts: &UiFonts) {
            self.status.set_font(Some(&fonts.status));
            self.uptime.set_font(Some(&fonts.mono));
            self.log_title.set_font(Some(&fonts.body));
            self.autoscroll.set_font(Some(&fonts.body));
            self.start_button.set_font(Some(&fonts.body));
            self.stop_button.set_font(Some(&fonts.body));
            self.clear_button.set_font(Some(&fonts.body));
            self.log.set_font(Some(&fonts.mono));
            self.command_label.set_font(Some(&fonts.body));
            self.command.set_font(Some(&fonts.mono));
            self.send_button.set_font(Some(&fonts.body));
            self.hint.set_font(Some(&fonts.small));
        }
    }

    #[derive(Clone)]
    struct GuiLogWriter(Sender<UiMessage>);

    struct GuiLogBuffer {
        sender: Sender<UiMessage>,
        bytes: Vec<u8>,
    }

    impl<'a> MakeWriter<'a> for GuiLogWriter {
        type Writer = GuiLogBuffer;

        fn make_writer(&'a self) -> Self::Writer {
            GuiLogBuffer {
                sender: self.0.clone(),
                bytes: Vec::with_capacity(256),
            }
        }
    }

    impl Write for GuiLogBuffer {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            if !self.bytes.is_empty() {
                let line = String::from_utf8_lossy(&self.bytes).trim_end().to_owned();
                self.bytes.clear();
                if !line.is_empty() {
                    let _ = self.sender.send(UiMessage::Log(line));
                }
            }
            Ok(())
        }
    }

    impl Drop for GuiLogBuffer {
        fn drop(&mut self) {
            let _ = self.flush();
        }
    }

    struct RunningServer {
        commands: tokio::sync::mpsc::UnboundedSender<String>,
        thread: thread::JoinHandle<()>,
    }

    fn server_manager(actions: Receiver<Action>, messages: Sender<UiMessage>) {
        let mut running: Option<RunningServer> = None;
        let mut exiting = false;

        while !exiting || running.is_some() {
            match actions.recv_timeout(Duration::from_millis(100)) {
                Ok(Action::Start) if running.is_none() => {
                    let (command_tx, command_rx) = tokio::sync::mpsc::unbounded_channel();
                    let server_thread = thread::Builder::new()
                        .name("ferrite-server-runtime".into())
                        .spawn(move || {
                            let result = tokio::runtime::Builder::new_multi_thread()
                                .enable_all()
                                .build()
                                .map(|runtime| runtime.block_on(run_server(command_rx)));
                            match result {
                                Ok(Ok(())) => {}
                                Ok(Err(error)) => {
                                    tracing::error!(error = %format!("{error:#}"), "server stopped with an error");
                                }
                                Err(error) => {
                                    tracing::error!(%error, "could not create the server runtime");
                                }
                            }
                        });

                    match server_thread {
                        Ok(server_thread) => {
                            running = Some(RunningServer {
                                commands: command_tx,
                                thread: server_thread,
                            });
                            let _ = messages.send(UiMessage::ServerStarted);
                        }
                        Err(error) => {
                            tracing::error!(%error, "could not start the server thread");
                            let _ = messages.send(UiMessage::ServerStopped);
                        }
                    }
                }
                Ok(Action::Stop) => {
                    if let Some(server) = &running {
                        let _ = server.commands.send("stop".into());
                    }
                }
                Ok(Action::Command(command)) => {
                    if let Some(server) = &running {
                        let _ = server.commands.send(command);
                    }
                }
                Ok(Action::Exit) => {
                    exiting = true;
                    if let Some(server) = &running {
                        let _ = server.commands.send("stop".into());
                    }
                }
                Ok(Action::Start) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    exiting = true;
                    if let Some(server) = &running {
                        let _ = server.commands.send("stop".into());
                    }
                }
            }

            if running
                .as_ref()
                .is_some_and(|server| server.thread.is_finished())
                && let Some(server) = running.take()
            {
                let _ = server.thread.join();
                let _ = messages.send(UiMessage::ServerStopped);
            }
        }
    }

    fn scaled(value: i32, scale: f32) -> i32 {
        (value as f32 * scale).round().max(1.0) as i32
    }

    fn colorref(color: [u8; 3]) -> u32 {
        u32::from(color[0]) | (u32::from(color[1]) << 8) | (u32::from(color[2]) << 16)
    }

    fn brightness(color: u32) -> u32 {
        (color & 0xFF) * 299 + ((color >> 8) & 0xFF) * 587 + ((color >> 16) & 0xFF) * 114
    }

    fn dpi_scale() -> f32 {
        let dpi = unsafe { nwg::dpi() };
        if dpi <= 0 {
            1.0
        } else {
            (dpi as f32 / 96.0).clamp(0.75, 3.0)
        }
    }

    /// Drop the oldest `drop_count` logical lines and keep a trailing newline so the next
    /// `TextBox::append` starts on its own line.
    fn keep_newest_lines(text: &str, drop_count: usize) -> String {
        let mut kept = text.lines().skip(drop_count).collect::<Vec<_>>().join("\n");
        if !kept.is_empty() {
            kept.push('\n');
        }
        kept
    }

    fn format_uptime(elapsed: Duration) -> String {
        let total = elapsed.as_secs();
        let hours = total / 3600;
        let minutes = (total % 3600) / 60;
        let seconds = total % 60;
        format!("가동 {hours:02}:{minutes:02}:{seconds:02}")
    }

    pub(super) fn run() -> anyhow::Result<()> {
        let executable = std::env::current_exe().context("locating ferrite-server.exe")?;
        if let Some(directory) = executable.parent() {
            std::env::set_current_dir(directory)
                .with_context(|| format!("using server directory {}", directory.display()))?;
        }

        // Opt into system DPI awareness before any window is created so the
        // scale factor below is the real one, not a 96-DPI virtualization.
        unsafe {
            win32::SetProcessDPIAware();
        }
        nwg::init().context("initializing the Windows GUI")?;
        nwg::Font::set_global_family("Segoe UI").context("setting the GUI font")?;

        let scale = dpi_scale();
        let fonts = UiFonts::new(scale)?;

        let (actions_tx, actions_rx) = mpsc::channel();
        let (messages_tx, messages_rx) = mpsc::channel();
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
            )
            .with_ansi(false)
            .with_writer(GuiLogWriter(messages_tx.clone()))
            .try_init()
            .map_err(|error| anyhow::anyhow!("initializing GUI log capture: {error}"))?;

        let app = FerriteServerGui::build_ui(Default::default())
            .map_err(|error| anyhow::anyhow!("building the server window: {error}"))?;
        *app.state.borrow_mut() = UiState {
            actions: Some(actions_tx),
            messages: Some(messages_rx),
            scale,
            ..Default::default()
        };

        app.install_palette()?;
        app.apply_fonts(&fonts);
        app.window.set_size(
            scaled(INITIAL_WIDTH, scale) as u32,
            scaled(INITIAL_HEIGHT, scale) as u32,
        );

        let window_size = app.window.size();
        let screen_width = unsafe { win32::GetSystemMetrics(win32::SM_CXSCREEN) };
        let screen_height = unsafe { win32::GetSystemMetrics(win32::SM_CYSCREEN) };
        if screen_width > 0 && screen_height > 0 {
            app.window.set_position(
                ((screen_width - window_size.0 as i32) / 2).max(0),
                ((screen_height - window_size.1 as i32) / 2).max(0),
            );
        }

        app.relayout();
        app.command.set_enabled(false);
        app.send_button.set_enabled(false);
        app.uptime.set_visible(false);
        app.window.set_visible(true);
        app.repaint_surface();
        app.start_button.set_focus();

        let manager = thread::Builder::new()
            .name("ferrite-server-manager".into())
            .spawn(move || server_manager(actions_rx, messages_tx))
            .context("starting the server manager")?;

        nwg::dispatch_thread_events();
        manager
            .join()
            .map_err(|_| anyhow::anyhow!("server manager thread panicked"))?;
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::{format_uptime, keep_newest_lines};
        use std::time::Duration;

        #[test]
        fn uptime_is_formatted_as_hours_minutes_seconds() {
            assert_eq!(format_uptime(Duration::ZERO), "가동 00:00:00");
            assert_eq!(format_uptime(Duration::from_secs(3661)), "가동 01:01:01");
            assert_eq!(
                format_uptime(Duration::from_secs(99 * 3600 + 59 * 60 + 59)),
                "가동 99:59:59"
            );
        }

        #[test]
        fn trimmed_log_text_keeps_a_trailing_newline() {
            assert_eq!(
                keep_newest_lines("one\r\ntwo\r\nthree\r\n", 1),
                "two\nthree\n"
            );
            assert_eq!(keep_newest_lines("one\r\n", 1), "");
        }
    }
}
