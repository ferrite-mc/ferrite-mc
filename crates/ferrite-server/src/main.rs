#![cfg_attr(windows, windows_subsystem = "windows")]

//! Ferrite server entry point.
use anyhow::Context;
use ferrite_server::{game, plugin_host};
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
    /// Enforce `whitelist.json` for new connections.
    whitelist: bool,
    /// Settings for the vanilla world generation host (`generator = 'vanilla'`).
    worldgen: WorldgenSettings,
    /// Chat and command rate limits from the `[rate-limits]` table.
    rate_limits: RateLimitSettings,
    /// Settings for the embedded plugin host (`[plugins]`).
    plugins: plugin_host::PluginsSettings,
}

/// What happens to a player who exceeds a rate limit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RateLimitAction {
    /// Disconnect with "Kicked for spamming", like vanilla.
    #[default]
    Kick,
    /// Silently discard the message and keep the connection.
    Drop,
}

/// The `[rate-limits]` table: a token bucket per connection for chat and another for commands.
/// A bucket holds `*-burst` messages and refills `*-per-second` of them each second. The
/// defaults are vanilla's spam threshold (10 messages, one more per second). A burst of 0
/// switches that bucket off, and `enabled = false` switches both off.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
struct RateLimitSettings {
    enabled: bool,
    chat_burst: u32,
    chat_per_second: f64,
    command_burst: u32,
    command_per_second: f64,
    action: RateLimitAction,
}
impl Default for RateLimitSettings {
    fn default() -> Self {
        let vanilla = ferrite_net::rate_limit::BucketConfig::VANILLA;
        Self {
            enabled: true,
            chat_burst: vanilla.burst,
            chat_per_second: vanilla.per_second,
            command_burst: vanilla.burst,
            command_per_second: vanilla.per_second,
            action: RateLimitAction::Kick,
        }
    }
}
impl RateLimitSettings {
    /// The `ferrite-net` limits. A refill rate that is negative, infinite or NaN is a config
    /// error (the server refuses to start) instead of a bucket that silently never refills.
    fn to_net(&self) -> anyhow::Result<ferrite_net::rate_limit::RateLimits> {
        use ferrite_net::rate_limit::{BucketConfig, LimitAction, RateLimits};
        let bucket =
            |key: &str, burst: u32, per_second: f64| -> anyhow::Result<Option<BucketConfig>> {
                anyhow::ensure!(
                    per_second.is_finite() && per_second >= 0.0,
                    "rate-limits.{key} must be a finite number of at least 0, got {per_second}"
                );
                Ok((burst > 0).then_some(BucketConfig { burst, per_second }))
            };
        if !self.enabled {
            return Ok(RateLimits::disabled());
        }
        Ok(RateLimits {
            chat: bucket("chat-per-second", self.chat_burst, self.chat_per_second)?,
            command: bucket(
                "command-per-second",
                self.command_burst,
                self.command_per_second,
            )?,
            action: match self.action {
                RateLimitAction::Kick => LimitAction::Kick,
                RateLimitAction::Drop => LimitAction::Drop,
            },
        })
    }
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
            whitelist: false,
            worldgen: WorldgenSettings::default(),
            rate_limits: RateLimitSettings::default(),
            plugins: plugin_host::PluginsSettings::default(),
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

# Player forwarding: "none", "velocity" or "bungee".
forwarding = "none"
forwarding-secret = ""
whitelist = false

# Chat and command rate limits (a token bucket per connection). The defaults are vanilla's spam
# threshold. A burst of 0 switches that bucket off; action is "kick" or "drop".
[rate-limits]
enabled = true
chat-burst = 10
chat-per-second = 1.0
command-burst = 10
command-per-second = 1.0
action = "kick"

# Plugin host (Plugin B). Off by default: no embedded JVM starts and the plugins directory is
# not read. Build the host jar with `cd java && ./gradlew :ferrite-host:jar` before enabling.
[plugins]
enabled = false
dir = "plugins"
data-dir = "plugins"
heap-mb = 512
api-version = "26.3"
host-jar = "java/ferrite-host/build/libs/ferrite-host.jar"
# The host jar is always first; these Lane 2/API runtime jars follow in this order.
runtime-classpath = []
# A dev.ferrite.host.HostApi implementation on host-jar/runtime-classpath; empty = NoopHostApi.
api-impl-class = ""
start-timeout-ms = 30000
call-timeout-ms = 10000
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
    // STARTUP plugins load before the world/region initialization below. A host failure is
    // logged and the server continues without plugins.
    let host_server = ferrite_plugin_host::ServerInfo {
        name: "Ferrite".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        motd: config.motd.clone(),
        max_players: config.max_players,
        online_mode: config.online_mode,
        view_distance,
    };
    let mut running_host = plugin_host::RunningHost::start(&config.plugins, host_server);
    let plugin_hooks: Option<Arc<dyn game::plugin_types::ServerHooks>> = running_host
        .hooks()
        .map(|hooks| hooks as Arc<dyn game::plugin_types::ServerHooks>);
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
    let vanilla_data = std::env::var_os("FERRITE_VANILLA_ROOT").map_or_else(
        || std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3/generated"),
        std::path::PathBuf::from,
    );
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
    game::set_map_server_root(config.world.clone());
    // The plugin bridge and the game loop share these sources, so a plugin write lands in the
    // same chunk data the server ticks.
    let dimensions = game::DimensionRegistry::with_source(
        game::Dimension::Overworld,
        std::sync::Arc::clone(&source),
        spawn,
    );
    let (disable_end_credits, perform_username_validation) =
        match ferrite_config::FerriteConfig::load("config") {
            Ok(config) => {
                let unsupported = &config.paper_global().unsupported_settings;
                game::piston::ALLOW_HEADLESS_PISTONS.store(
                    unsupported.allow_headless_pistons,
                    std::sync::atomic::Ordering::Relaxed,
                );
                (
                    config.world_settings("world").misc.disable_end_credits,
                    unsupported.perform_username_validation,
                )
            }
            Err(error) => {
                tracing::warn!(%error, "reading Paper config failed; End credits stay enabled and usernames stay validated");
                (false, true)
            }
        };
    let rate_limits = config.rate_limits.to_net()?;
    let host_dimensions = std::sync::Arc::clone(&dimensions);
    let mut settings = game::WorldSettings {
        disable_end_credits,
        dimension: game::Dimension::Overworld,
        raining: false,
        gamemode: match config.gamemode {
            GameMode::Survival => 0,
            GameMode::Creative => 1,
        },
        max_players: i32::try_from(config.max_players).unwrap_or(i32::MAX),
        ops,
        plugins: plugin_hooks.clone(),
        plugin_event_errors: game::PluginEventPolicy::Allow,
        admin: Some(admin),
        whitelist: config.whitelist,
        playerdata: Some(
            config
                .world
                .as_ref()
                .map_or_else(|| "playerdata".into(), |w| w.join("playerdata")),
        ),
        plugin_commands: None,
        dimensions: Some(dimensions),
        state_provider: None,
    };
    // POSTWORLD plugins see a ready world and load before the game coordinator builds its
    // command tree and before the network listener accepts players.
    // v2 block reads/writes use the real dimension sources; writes are queued and applied by
    // `WorldStateProvider::tick` via the plugin host's game-loop tick.
    if let Some(host) = running_host.host() {
        let provider = std::sync::Arc::new(plugin_host::WorldStateProvider::new(host_dimensions));
        settings.state_provider = Some(std::sync::Arc::clone(&provider));
        if let Err(error) = host.install_state_provider(provider) {
            tracing::warn!(%error, "installing the plugin state provider failed");
        }
    }
    if startup_cancelled(&cancelled) {
        tracing::info!("server startup cancelled");
        return Ok(());
    }
    if let Some(host) = running_host.host() {
        let report = host.load_postworld();
        for plugin in &report.loaded {
            tracing::info!(plugin = %plugin.name, "postworld plugin loaded");
        }
        for failure in &report.failed {
            tracing::warn!(
                plugin = failure.plugin.as_deref().unwrap_or("<unknown>"),
                error = %failure.error,
                "postworld plugin failed"
            );
        }
    }
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
            view_distance,
            forwarding,
            favicon,
            status: Default::default(),
            read_timeout: ferrite_net::DEFAULT_READ_TIMEOUT,
            rate_limits,
            perform_username_validation,
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
    // Plugins must disable before the world and network go away, so onDisable can still
    // observe a live server. A failure here never stops the world save/shutdown below.
    running_host.shutdown();
    let world_result = world.shutdown().await;
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

    fn parse(toml_text: &str) -> Config {
        toml::from_str(toml_text).expect("config parses")
    }

    #[test]
    fn rate_limits_default_to_vanillas_spam_threshold() {
        let net = Config::default().rate_limits.to_net().unwrap();
        assert_eq!(net, ferrite_net::rate_limit::RateLimits::default());
        // A config file without the table behaves the same.
        assert_eq!(parse("").rate_limits, RateLimitSettings::default());
    }

    #[test]
    fn the_built_in_config_file_parses_to_the_defaults() {
        let config = parse(DEFAULT_CONFIG);
        assert_eq!(config.rate_limits, RateLimitSettings::default());
        assert_eq!(
            config.rate_limits.to_net().unwrap(),
            ferrite_net::rate_limit::RateLimits::default()
        );
    }

    #[test]
    fn rate_limits_table_is_read_and_converted() {
        use ferrite_net::rate_limit::{BucketConfig, LimitAction};
        let config = parse(
            "[rate-limits]\nchat-burst = 4\nchat-per-second = 0.5\ncommand-burst = 20\n\
             command-per-second = 5\naction = \"drop\"\n",
        );
        let net = config.rate_limits.to_net().unwrap();
        assert_eq!(
            net.chat,
            Some(BucketConfig {
                burst: 4,
                per_second: 0.5
            })
        );
        assert_eq!(
            net.command,
            Some(BucketConfig {
                burst: 20,
                per_second: 5.0
            })
        );
        assert_eq!(net.action, LimitAction::Drop);
    }

    #[test]
    fn a_zero_burst_or_enabled_false_switches_limits_off() {
        let net = parse("[rate-limits]\nchat-burst = 0\n")
            .rate_limits
            .to_net()
            .unwrap();
        assert!(net.chat.is_none());
        assert!(net.command.is_some());
        let off = parse("[rate-limits]\nenabled = false\n")
            .rate_limits
            .to_net()
            .unwrap();
        assert_eq!(off, ferrite_net::rate_limit::RateLimits::disabled());
    }

    #[test]
    fn bad_rate_limit_values_are_rejected() {
        for bad in [
            "[rate-limits]\nchat-per-second = -1.0\n",
            "[rate-limits]\ncommand-per-second = nan\n",
            "[rate-limits]\nchat-per-second = inf\n",
        ] {
            let error = parse(bad).rate_limits.to_net().unwrap_err().to_string();
            assert!(error.contains("rate-limits."), "{bad}: {error}");
        }
        // Unknown keys and unknown actions fail to parse instead of being ignored.
        assert!(toml::from_str::<Config>("[rate-limits]\nchat-burts = 3\n").is_err());
        assert!(toml::from_str::<Config>("[rate-limits]\naction = \"ban\"\n").is_err());
    }

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

    #[test]
    fn plugin_host_is_disabled_by_default() {
        let config: Config = toml::from_str("").unwrap();
        assert!(!config.plugins.enabled);
        assert_eq!(config.plugins.dir, std::path::Path::new("plugins"));
        assert!(config.plugins.runtime_classpath.is_empty());
        assert!(config.plugins.api_impl_class.is_none());
        assert!(!parse(DEFAULT_CONFIG).plugins.enabled);
    }

    #[test]
    fn example_config_parses_with_the_plugin_host_section() {
        let example: Config =
            toml::from_str(include_str!("../../../ferrite.toml.example")).unwrap();
        assert!(!example.plugins.enabled);
        assert!(example.plugins.runtime_classpath.is_empty());
        assert_eq!(example.plugins.api_impl_class.as_deref(), Some(""));
        assert!(
            example
                .plugins
                .to_host_config(ferrite_plugin_host::ServerInfo::default())
                .api_impl_class
                .is_none()
        );
    }

    #[test]
    fn plugin_host_runtime_classpath_and_api_impl_are_configured() {
        let config: Config = toml::from_str(
            "[plugins]\nenabled = true\nruntime-classpath = ['ferrite-api-impl.jar', 'ferrite-nms.jar']\napi-impl-class = 'dev.ferrite.api.FerriteHostApi'\n",
        )
        .unwrap();
        assert!(config.plugins.enabled);
        assert_eq!(
            config.plugins.runtime_classpath,
            vec![
                std::path::PathBuf::from("ferrite-api-impl.jar"),
                std::path::PathBuf::from("ferrite-nms.jar")
            ]
        );
        let host = config
            .plugins
            .to_host_config(ferrite_plugin_host::ServerInfo::default());
        assert_eq!(host.runtime_classpath, config.plugins.runtime_classpath);
        assert_eq!(
            host.api_impl_class.as_deref(),
            Some("dev.ferrite.api.FerriteHostApi")
        );
    }
}

#[cfg(windows)]
mod tauri_gui;
