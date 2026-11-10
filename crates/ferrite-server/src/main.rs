#![cfg_attr(windows, windows_subsystem = "windows")]

//! Ferrite server entry point.
use anyhow::Context;
use ferrite_server::game;
use serde::Deserialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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
    Superflat,
    /// Mojang's own generator, run by the world generation host from the user's server jar.
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
            generator: WorldGenerator::Vanilla,
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
# clean shutdown. Vanilla terrain is the default; use "superflat" explicitly for flat worlds.
world = "world"
generator = "vanilla"
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

/// Read the configured bind address without creating or logging `ferrite.toml`.
#[cfg(windows)]
fn configured_bind_address() -> String {
    std::fs::read_to_string("ferrite.toml")
        .ok()
        .and_then(|content| toml::from_str::<Config>(&content).ok())
        .map(|config| config.bind_address)
        .unwrap_or_else(|| Config::default().bind_address)
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
    run_server(stdin_commands(), None, None).await
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

    if let Err(error) = tauri_gui::run() {
        tauri_gui::fatal_error(&format!(
            "Ferrite Server GUI를 시작하지 못했습니다.\n\n{error:#}"
        ));
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
    runtime.block_on(run_server(stdin_commands(), None, None))
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

fn startup_cancelled(cancelled: &Option<Arc<AtomicBool>>) -> bool {
    cancelled
        .as_ref()
        .is_some_and(|flag| flag.load(Ordering::SeqCst))
}

async fn run_server(
    mut console_rx: tokio::sync::mpsc::UnboundedReceiver<String>,
    started: Option<tokio::sync::oneshot::Sender<()>>,
    cancelled: Option<Arc<AtomicBool>>,
) -> anyhow::Result<()> {
    if startup_cancelled(&cancelled) {
        return Ok(());
    }
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
    if startup_cancelled(&cancelled) {
        tracing::info!("server startup cancelled");
        return Ok(());
    }

    let existing_world = config
        .world
        .as_ref()
        .is_some_and(|path| path.join("level.dat").exists());
    let spawn = match &config.world {
        Some(path) => spawn_for_world(path)?,
        None => [0, 65, 0],
    };
    if startup_cancelled(&cancelled) {
        tracing::info!("server startup cancelled");
        return Ok(());
    }
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
    if startup_cancelled(&cancelled) {
        tracing::info!("server startup cancelled");
        return Ok(());
    }
    let source = if let Some(path) = &config.world {
        let registry = ferrite_world::RuntimeRegistry::load(&vanilla_data)
            .context("loading vanilla registry")?;
        game::new_anvil_source_with(path.clone(), registry, terrain)
    } else {
        game::new_memory_source(terrain)
    };
    // Vanilla terrain chooses a spawn above the generated surface; the flat world keeps the
    // fixed `[0, 65, 0]` the existing tests rely on.
    let spawn = if source.is_flat() || existing_world {
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
    let runtime_world_view =
        ferrite_server::plugins::dynamic_world_view(std::sync::Arc::clone(&dimensions));
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
    if startup_cancelled(&cancelled) {
        tracing::info!("server startup cancelled");
        return Ok(());
    }
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
        plugins: None,
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
        runtime_world_view: Some(runtime_world_view.clone()),
    };
    let world = game::World::start_configured(Some(source), spawn, settings);
    if startup_cancelled(&cancelled) {
        tracing::info!("server startup cancelled");
        return Ok(());
    }
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
            world_view: Some(runtime_world_view.clone()),
            console: Some(plugin_console_tx),
            runtime_world_handler: Some({
                let sender = world.sender.clone();
                std::sync::Arc::new(move |name: &str, environment: &str| {
                    let dimension = match environment {
                        "NORMAL" => game::Dimension::Overworld,
                        "NETHER" => game::Dimension::Nether,
                        "THE_END" => game::Dimension::End,
                        _ => {
                            return Err(format!(
                                "unsupported Bukkit world environment {environment}"
                            ));
                        }
                    };
                    let name = name.to_owned();
                    let runtime = tokio::runtime::Handle::current();
                    let created = tokio::task::block_in_place(|| {
                        runtime.block_on(async {
                            let (complete, done) = tokio::sync::oneshot::channel();
                            sender
                                .send(ferrite_net::game::GameMessage::CreateRuntimeWorld {
                                    name,
                                    dimension: dimension.key().to_owned(),
                                    spawn: dimension.default_spawn(),
                                    complete,
                                })
                                .map_err(|_| "game loop stopped".to_owned())?;
                            done.await
                                .map_err(|_| "runtime world response lost".to_owned())?
                        })
                    })?;
                    Ok((created.0, created.2))
                })
            }),
            runtime_world_unloader: Some({
                let sender = world.sender.clone();
                std::sync::Arc::new(move |id, save| {
                    let runtime = tokio::runtime::Handle::current();
                    tokio::task::block_in_place(|| {
                        runtime.block_on(async {
                            let (complete, done) = tokio::sync::oneshot::channel();
                            sender
                                .send(ferrite_net::game::GameMessage::UnloadRuntimeWorld {
                                    id,
                                    save,
                                    complete,
                                })
                                .map_err(|_| "game loop stopped".to_owned())?;
                            done.await
                                .map_err(|_| "runtime unload response lost".to_owned())?
                        })
                    })
                })
            }),
        })
        .context("starting plugins")?;
    if let Some(bridge) = &plugin_bridge {
        world.set_plugin_hooks(bridge.hooks()).await?;
    }
    if startup_cancelled(&cancelled) {
        tracing::info!("server startup cancelled");
        return Ok(());
    }
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
    if startup_cancelled(&cancelled) {
        tracing::info!("server startup cancelled");
        return Ok(());
    }
    if let Some(started) = started {
        let _ = started.send(());
    }
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
        assert_eq!(defaults.generator, WorldGenerator::Vanilla);
        assert_eq!(
            toml::from_str::<Config>(DEFAULT_CONFIG).unwrap().generator,
            WorldGenerator::Vanilla
        );
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
mod tauri_gui;
