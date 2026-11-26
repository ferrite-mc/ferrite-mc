#![cfg_attr(windows, windows_subsystem = "windows")]

//! Ferrite server entry point.
use anyhow::Context;
use ferrite_server::{game, plugin_host};
use serde::Deserialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

mod log_i18n;

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
    /// Wire packet compression threshold in bytes (`-1` disables compression, `0` compresses all).
    compression_threshold: i32,
    forwarding: ForwardingMode,
    forwarding_secret: String,
    world: Option<std::path::PathBuf>,
    /// New chunks come from the flat test world or the pure-Rust terrain generator.
    generator: WorldGenerator,
    /// Chunk storage backend: `"fworld"` (default) or `"anvil"`.
    storage: StorageMode,
    /// Chunk compression: `"auto"`, `"lz4"`, `"zlib"`, `"gzip"`, or `"none"`.
    compression: CompressionMode,
    /// Worker thread count for Tokio & region schedulers (`0` or unset = hardware parallelism).
    threads: Option<usize>,
    /// Enable batched parallel chunk relighting via `ferrite-light`'s `LightPool`.
    lighting_pool: bool,
    gamemode: GameMode,
    /// Log output language (`"auto"`, `"en"`, `"ko"`, `"ja"`, `"zh"`, `"es"`).
    log_language: String,
    ops: Vec<OpConfig>,
    /// Enforce `whitelist.json` for new connections.
    whitelist: bool,
    /// Settings for pure-Rust world generation (`generator = 'rust'`).
    worldgen: WorldgenSettings,
    /// Fine-grained traffic compression & batching controls (`[network]`).
    network: NetworkSettings,
    /// Chat and command rate limits from the `[rate-limits]` table.
    rate_limits: RateLimitSettings,
    /// Settings for the embedded plugin host (`[plugins]`).
    plugins: plugin_host::PluginsSettings,
    /// Optional embedded HTTP admin & Folia telemetry dashboard (`[admin-http]`).
    admin_http: ferrite_server::admin_http::AdminHttpSettings,
}

/// The `[network]` table: fine-grained packet compression and TCP batching knobs.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
struct NetworkSettings {
    /// Zlib compression level (`1..=12` for libdeflate; higher = smaller traffic).
    compression_level: u32,
    /// Emit uncompressed wire frame when zlib does not shrink payload.
    skip_incompressible: bool,
    /// Raw batch size threshold (bytes) to offload compression + encryption to blocking pool.
    writer_offload_bytes: usize,
    /// Outbound TCP batching coalesce window in milliseconds (`0..=100`).
    batch_linger_ms: u64,
    /// Per-connection max queued uncompressed bytes before dropping slow clients.
    send_queue_bytes: usize,
    /// Maximum concurrent TCP connections accepted by `ferrite-net` (`0` = unlimited).
    max_connections: usize,
    /// Per-source-IP TCP connect burst allowance (`0` = disabled, loopback excluded).
    connection_burst: u32,
    /// Per-source-IP TCP connect refill rate per second (`0` = disabled).
    connection_per_second: u32,
}

impl Default for NetworkSettings {
    fn default() -> Self {
        Self {
            compression_level: 4,
            skip_incompressible: true,
            writer_offload_bytes: 4096,
            batch_linger_ms: 2,
            send_queue_bytes: 2 * 1024 * 1024,
            max_connections: 4096,
            connection_burst: 20,
            connection_per_second: 5,
        }
    }
}

impl NetworkSettings {
    fn to_connection_rate_limit(&self) -> Option<ferrite_net::rate_limit::BucketConfig> {
        (self.connection_burst > 0).then_some(ferrite_net::rate_limit::BucketConfig {
            burst: self.connection_burst,
            per_second: f64::from(self.connection_per_second),
        })
    }

    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (1..=12).contains(&self.compression_level),
            "network.compression-level must be between 1 and 12, got {}",
            self.compression_level
        );
        anyhow::ensure!(
            self.batch_linger_ms <= 100,
            "network.batch-linger-ms must be <= 100, got {}",
            self.batch_linger_ms
        );
        anyhow::ensure!(
            self.send_queue_bytes >= 1024,
            "network.send-queue-bytes must be at least 1024, got {}",
            self.send_queue_bytes
        );
        Ok(())
    }
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

/// The `[worldgen]` table for pure-Rust terrain generation (`generator = "rust"`).
/// Unknown legacy JVM keys from older `ferrite.toml` files are safely ignored.
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct WorldgenSettings {
    /// The world seed (a number or text, as in `server.properties`).
    seed: String,
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
#[derive(Debug, Clone, Copy, Default, Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum WorldGenerator {
    /// The superflat world used by the existing tests and examples.
    Superflat,
    /// Pure-Rust, lock-free parallel world generator (`ferrite-worldgen`).
    #[default]
    Rust,
}
#[derive(Debug, Clone, Copy, Default, Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum StorageMode {
    /// Ferrite single-blob compressed world (`.fworld`) loaded wholly into RAM and flushed atomically.
    #[default]
    Fworld,
    /// Standard Mojang Anvil per-region files (`r.<rx>.<rz>.mca`).
    Anvil,
}
#[derive(Debug, Clone, Copy, Default, Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CompressionMode {
    /// Uses `zlib` for Anvil regions and `lz4` for `.fworld` blobs.
    #[default]
    Auto,
    Lz4,
    Zlib,
    Gzip,
    None,
}
impl CompressionMode {
    fn to_region(self) -> ferrite_world::RegionCompression {
        match self {
            Self::Auto | Self::Zlib => ferrite_world::RegionCompression::Zlib,
            Self::Lz4 => ferrite_world::RegionCompression::Lz4,
            Self::Gzip => ferrite_world::RegionCompression::Gzip,
            Self::None => ferrite_world::RegionCompression::None,
        }
    }

    fn to_fworld(self) -> ferrite_world::BlobCompression {
        match self {
            Self::Auto | Self::Lz4 => ferrite_world::BlobCompression::Lz4,
            Self::Zlib => ferrite_world::BlobCompression::Zlib,
            Self::Gzip => ferrite_world::BlobCompression::Gzip,
            Self::None => ferrite_world::BlobCompression::None,
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct LaunchOptions {
    pub storage: Option<StorageMode>,
    pub compression: Option<CompressionMode>,
    pub threads: Option<usize>,
    pub lighting_pool: Option<bool>,
    pub compression_threshold: Option<i32>,
    pub compression_level: Option<u32>,
    pub skip_incompressible: Option<bool>,
    pub writer_offload_bytes: Option<usize>,
    pub batch_linger_ms: Option<u64>,
    pub max_connections: Option<usize>,
    pub world: Option<std::path::PathBuf>,
    pub generator: Option<WorldGenerator>,
    #[serde(skip)]
    pub lang: Option<log_i18n::LogLang>,
    #[serde(skip)]
    pub headless: bool,
    #[serde(skip)]
    pub show_help: bool,
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
            generator: WorldGenerator::Rust,
            storage: StorageMode::Fworld,
            compression: CompressionMode::Auto,
            threads: None,
            lighting_pool: false,
            gamemode: GameMode::Survival,
            log_language: "auto".into(),
            ops: Vec::new(),
            whitelist: false,
            worldgen: WorldgenSettings::default(),
            network: NetworkSettings::default(),
            rate_limits: RateLimitSettings::default(),
            plugins: plugin_host::PluginsSettings::default(),
            admin_http: ferrite_server::admin_http::AdminHttpSettings::default(),
        }
    }
}

/// Written next to the executable (or wherever the server was started) when `ferrite.toml` is
/// missing. Keeping the full default here means the first run produces an editable file instead
/// of silently using in-memory defaults that leave no world behind.
const DEFAULT_CONFIG: &str = r#"# Ferrite server configuration.
# Relative paths (world, server-icon, ...) are resolved against the process
# working directory. Start the server from this folder to keep them next to the executable.

bind-address = "0.0.0.0:25565"
online-mode = true
enforce-secure-profile = true
motd = "A Ferrite server"
server-icon = "server-icon.png"
max-players = 20
view-distance = 10
# Wire packet compression threshold in bytes (-1 = disable compression, 0 = compress all packets).
compression-threshold = 256

# World directory. It is created on first start and saved on chunk unload, autosave and a
# clean shutdown. Vanilla terrain is the default; use "superflat" explicitly for flat worlds.
world = "world"
generator = "rust"
# Storage backend: "fworld" (single compressed .fworld blob in RAM, default) or "anvil".
storage = "fworld"
# Chunk compression: "auto" (zlib for anvil, lz4 for fworld), "lz4", "zlib", "gzip", or "none".
compression = "auto"
# Worker thread count for Tokio and region schedulers (0 = hardware parallelism).
threads = 0
# Enable batched parallel chunk relighting via ferrite-light (default: false).
lighting-pool = false
gamemode = "survival"
# Log language: "auto" (uses FERRITE_LANG / LANG), "en", "ko", "ja", "zh", or "es".
log-language = "auto"

# Player forwarding: "none", "velocity" or "bungee".
forwarding = "none"
forwarding-secret = ""
whitelist = false

# Fine-grained network traffic compression and TCP batching knobs.
[network]
compression-level = 4
skip-incompressible = true
writer-offload-bytes = 4096
batch-linger-ms = 2
send-queue-bytes = 2097152
max-connections = 4096
connection-burst = 20
connection-per-second = 5

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
# not read. Build the jars with `cd java && ./gradlew :ferrite-api-impl:jar :ferrite-nms:jar :ferrite-host:jar` before enabling.
[plugins]
enabled = false
native-plugins = false
dir = "plugins"
data-dir = "plugins"
heap-mb = 512
api-version = "26.3"
host-jar = "java/ferrite-host/build/libs/ferrite-host.jar"
# The host jar is always first; these Paper/Bukkit/Folia/NMS runtime jars follow in this order.
runtime-classpath = []
# A dev.ferrite.host.HostApi implementation on host-jar/runtime-classpath; empty = NoopHostApi.
api-impl-class = ""
start-timeout-ms = 30000
call-timeout-ms = 10000
"#;

/// Reads `ferrite.toml`, creating it with [`DEFAULT_CONFIG`] on the first run. A failed write is
/// only a warning: the server still starts with the same defaults.
fn load_config() -> anyhow::Result<(Config, bool)> {
    load_config_in(std::path::Path::new("."))
}

fn load_config_in(root: &std::path::Path) -> anyhow::Result<(Config, bool)> {
    let path = root.join("ferrite.toml");
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            let config = toml::from_str(&content).context("invalid ferrite.toml")?;
            let absolute = std::path::absolute(&path).unwrap_or_else(|_| path.clone());
            tracing::info!(config = %absolute.display(), "loaded server configuration");
            Ok((config, false))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let config: Config = toml::from_str(DEFAULT_CONFIG).expect("built-in config is valid");
            match std::fs::write(&path, DEFAULT_CONFIG) {
                Ok(()) => tracing::info!(
                    config = %std::path::absolute(&path).unwrap_or_else(|_| path.clone()).display(),
                    "created the default ferrite.toml"
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

#[cfg(windows)]
struct GuiDefaults {
    address: String,
    storage: StorageMode,
    compression: CompressionMode,
    threads: Option<usize>,
    lighting_pool: bool,
    compression_threshold: i32,
    compression_level: u32,
    skip_incompressible: bool,
    writer_offload_bytes: usize,
    batch_linger_ms: u64,
    max_connections: usize,
}

/// Read UI defaults from `ferrite.toml` without creating or logging the file.
#[cfg(windows)]
fn configured_gui_defaults() -> GuiDefaults {
    let config = std::fs::read_to_string("ferrite.toml")
        .ok()
        .and_then(|content| toml::from_str::<Config>(&content).ok())
        .unwrap_or_default();
    GuiDefaults {
        address: config.bind_address,
        storage: config.storage,
        compression: config.compression,
        threads: config.threads.filter(|&n| n > 0),
        lighting_pool: config.lighting_pool,
        compression_threshold: config.compression_threshold,
        compression_level: config.network.compression_level,
        skip_incompressible: config.network.skip_incompressible,
        writer_offload_bytes: config.network.writer_offload_bytes,
        batch_linger_ms: config.network.batch_linger_ms,
        max_connections: config.network.max_connections,
    }
}

fn read_configured_threads() -> Option<usize> {
    std::fs::read_to_string("ferrite.toml")
        .ok()
        .and_then(|content| toml::from_str::<Config>(&content).ok())
        .and_then(|config| config.threads)
        .filter(|&n| n > 0)
}

const CLI_HELP: &str = "\
Ferrite Server

USAGE:
  ferrite-server [OPTIONS]

OPTIONS:
  --headless                          Run without desktop GUI (Windows)
  --storage <fworld|anvil>            World storage backend (default: fworld / ferrite.toml)
  --fworld                            Shortcut for --storage fworld
  --anvil                             Shortcut for --storage anvil
  --compression <auto|lz4|zlib|gzip|none>
                                      World chunk compression algorithm
  --compression-threshold <N>         Wire packet compression threshold (-1 = off, 0 = all)
  --compression-level <1..=12>        Wire packet zlib compression level (1 = fast, 12 = max)
  --skip-incompressible <true|false>  Send uncompressed frame if zlib bloats packet size
  --writer-offload-bytes <N>          Offload compression+encryption batch >= N bytes
  --batch-linger-ms <N>               Coalesce outbound TCP packets for N ms (0..=100)
  --max-connections <N>               Maximum concurrent TCP connections (0 = unlimited)
  --threads <N>                       Worker threads for Tokio & region schedulers (0 = auto)
  --lighting-pool                     Enable batched parallel chunk relighting (ferrite-light)
  --world <PATH>                      World directory or .fworld blob path
  --generator <rust|superflat>        Terrain generator
  --lang <auto|en|ko|ja|zh|es>        Console log language (default: auto)
  -h, --help                          Print this help message";

fn parse_storage_mode(value: &str) -> anyhow::Result<StorageMode> {
    match value.to_ascii_lowercase().as_str() {
        "anvil" => Ok(StorageMode::Anvil),
        "fworld" => Ok(StorageMode::Fworld),
        _ => anyhow::bail!("invalid --storage value '{value}' (expected fworld or anvil)"),
    }
}

fn parse_compression_mode(value: &str) -> anyhow::Result<CompressionMode> {
    match value.to_ascii_lowercase().as_str() {
        "auto" => Ok(CompressionMode::Auto),
        "lz4" => Ok(CompressionMode::Lz4),
        "zlib" => Ok(CompressionMode::Zlib),
        "gzip" => Ok(CompressionMode::Gzip),
        "none" => Ok(CompressionMode::None),
        _ => anyhow::bail!(
            "invalid --compression value '{value}' (expected auto, lz4, zlib, gzip, or none)"
        ),
    }
}

fn parse_generator(value: &str) -> anyhow::Result<WorldGenerator> {
    match value.to_ascii_lowercase().as_str() {
        "rust" => Ok(WorldGenerator::Rust),
        "superflat" => Ok(WorldGenerator::Superflat),
        _ => anyhow::bail!("invalid --generator value '{value}' (expected rust or superflat)"),
    }
}

fn parse_bool_flag(name: &str, value: &str) -> anyhow::Result<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => anyhow::bail!("invalid {name} value '{value}' (expected true or false)"),
    }
}

fn parse_cli_args<I, S>(args: I) -> anyhow::Result<LaunchOptions>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut opts = LaunchOptions::default();
    let mut iter = args.into_iter().peekable();
    while let Some(arg) = iter.next() {
        let arg = arg.as_ref();
        let (flag, inline) = match arg.split_once('=') {
            Some((k, v)) => (k, Some(v)),
            None => (arg, None),
        };
        let mut next_val = |name: &str| -> anyhow::Result<String> {
            if let Some(v) = inline {
                return Ok(v.to_owned());
            }
            iter.next()
                .map(|s| s.as_ref().to_owned())
                .ok_or_else(|| anyhow::anyhow!("missing value for {name}"))
        };
        match flag {
            "-h" | "--help" => opts.show_help = true,
            "--headless" => opts.headless = true,
            "--anvil" => opts.storage = Some(StorageMode::Anvil),
            "--fworld" => opts.storage = Some(StorageMode::Fworld),
            "--lighting-pool" => opts.lighting_pool = Some(true),
            "--storage" => opts.storage = Some(parse_storage_mode(&next_val(flag)?)?),
            "--compression" => opts.compression = Some(parse_compression_mode(&next_val(flag)?)?),
            "--compression-threshold" => {
                let raw = next_val(flag)?;
                let threshold: i32 = raw
                    .parse()
                    .with_context(|| format!("invalid --compression-threshold value '{raw}'"))?;
                anyhow::ensure!(
                    threshold >= -1,
                    "--compression-threshold must be >= -1, got {threshold}"
                );
                opts.compression_threshold = Some(threshold);
            }
            "--compression-level" => {
                let raw = next_val(flag)?;
                let level: u32 = raw
                    .parse()
                    .with_context(|| format!("invalid --compression-level value '{raw}'"))?;
                anyhow::ensure!(
                    (1..=12).contains(&level),
                    "--compression-level must be between 1 and 12, got {level}"
                );
                opts.compression_level = Some(level);
            }
            "--skip-incompressible" => {
                let raw = next_val(flag)?;
                opts.skip_incompressible = Some(parse_bool_flag(flag, &raw)?);
            }
            "--writer-offload-bytes" => {
                let raw = next_val(flag)?;
                let bytes: usize = raw
                    .parse()
                    .with_context(|| format!("invalid --writer-offload-bytes value '{raw}'"))?;
                opts.writer_offload_bytes = Some(bytes);
            }
            "--batch-linger-ms" => {
                let raw = next_val(flag)?;
                let ms: u64 = raw
                    .parse()
                    .with_context(|| format!("invalid --batch-linger-ms value '{raw}'"))?;
                anyhow::ensure!(ms <= 100, "--batch-linger-ms must be <= 100, got {ms}");
                opts.batch_linger_ms = Some(ms);
            }
            "--max-connections" => {
                let raw = next_val(flag)?;
                let max_conns: usize = raw
                    .parse()
                    .with_context(|| format!("invalid --max-connections value '{raw}'"))?;
                opts.max_connections = Some(max_conns);
            }
            "--lang" => {
                let raw = next_val(flag)?;
                let parsed = log_i18n::LogLang::parse(&raw).ok_or_else(|| {
                    anyhow::anyhow!(
                        "invalid --lang value '{raw}' (expected auto, en, ko, ja, zh, or es)"
                    )
                })?;
                opts.lang = Some(parsed);
            }
            "--threads" => {
                let raw = next_val(flag)?;
                let count: usize = raw
                    .parse()
                    .with_context(|| format!("invalid --threads value '{raw}'"))?;
                opts.threads = Some(count);
            }
            "--world" => opts.world = Some(std::path::PathBuf::from(next_val(flag)?)),
            "--generator" => opts.generator = Some(parse_generator(&next_val(flag)?)?),
            other => anyhow::bail!("unknown argument '{other}' (run with --help for usage)"),
        }
    }
    Ok(opts)
}

fn build_tokio_runtime(threads: Option<usize>) -> anyhow::Result<tokio::runtime::Runtime> {
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    builder.enable_all();
    if let Some(n) = threads.filter(|&n| n > 0) {
        builder.worker_threads(n);
    }
    Ok(builder.build()?)
}

fn init_tracing(cli_lang: Option<log_i18n::LogLang>) {
    let initial = cli_lang.unwrap_or_else(log_i18n::LogLang::detect_from_env);
    log_i18n::set_active_lang(initial);
    tracing_subscriber::fmt()
        .event_format(log_i18n::LocalizedColorFormatter {
            ansi: log_i18n::colors_enabled(),
        })
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
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
/// snapshot all see the same effective distance.
const MIN_VIEW_DISTANCE: u8 = 2;
const MAX_VIEW_DISTANCE: u8 = 32;
fn effective_view_distance(configured: u8) -> u8 {
    configured.clamp(MIN_VIEW_DISTANCE, MAX_VIEW_DISTANCE)
}

#[cfg(not(windows))]
fn main() -> anyhow::Result<()> {
    let launch = parse_cli_args(std::env::args().skip(1))?;
    if launch.show_help {
        println!("{CLI_HELP}");
        return Ok(());
    }
    init_tracing(launch.lang);
    let runtime_threads = match launch.threads {
        Some(n) => (n > 0).then_some(n),
        None => read_configured_threads(),
    };
    let runtime = build_tokio_runtime(runtime_threads)?;
    runtime.block_on(run_server(stdin_commands(), None, None, launch))
}

#[cfg(windows)]
fn main() {
    let launch = match parse_cli_args(std::env::args().skip(1)) {
        Ok(opts) => opts,
        Err(error) => {
            eprintln!("Ferrite Server argument error: {error:#}");
            std::process::exit(2);
        }
    };
    if launch.show_help {
        println!("{CLI_HELP}");
        return;
    }
    // Keep a console-capable mode for service managers, scripts and the integration tests.
    // Normal Windows launches use the embedded GUI; Linux always enters the headless path above.
    if launch.headless {
        if let Err(error) = run_headless(launch) {
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
fn run_headless(launch: LaunchOptions) -> anyhow::Result<()> {
    init_tracing(launch.lang);
    let runtime_threads = match launch.threads {
        Some(n) => (n > 0).then_some(n),
        None => read_configured_threads(),
    };
    let runtime = build_tokio_runtime(runtime_threads)?;
    runtime.block_on(run_server(stdin_commands(), None, None, launch))
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
    launch: LaunchOptions,
) -> anyhow::Result<()> {
    if startup_cancelled(&cancelled) {
        return Ok(());
    }
    let (config, _created) = load_config()?;
    if launch.lang.is_none()
        && let Some(lang) = log_i18n::LogLang::parse(&config.log_language)
    {
        log_i18n::set_active_lang(lang);
    }
    // Ferrite never accepts the Mojang EULA for the user; the world generation host reads the
    // same `eula.txt`.
    ferrite_server::eula::ensure_accepted(std::path::Path::new("eula.txt"))?;
    let vanilla_data =
        ferrite_server::vanilla_setup::ensure_vanilla_generated(cancelled.as_deref())
            .await
            .context("preparing vanilla data")?;
    let effective_storage = launch.storage.unwrap_or(config.storage);
    let effective_compression = launch.compression.unwrap_or(config.compression);
    let effective_threads = match launch.threads {
        Some(n) => (n > 0).then_some(n),
        None => config.threads.filter(|&n| n > 0),
    };
    let effective_generator = launch.generator.unwrap_or(config.generator);
    let effective_world = Some(
        launch
            .world
            .or(config.world)
            .unwrap_or_else(|| std::path::PathBuf::from("world")),
    );
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
    let native_hooks: Option<Arc<plugin_host::NativePluginHooks>> =
        config.plugins.native_plugins.then(|| {
            let dir = config.plugins.data_dir.join("native");
            tracing::info!(dir = %dir.display(), "Rust native plugin suite enabled");
            Arc::new(plugin_host::NativePluginHooks::load_or_default(dir))
        });
    let dynamic_hooks: Option<Arc<plugin_host::DynamicPluginHooks>> =
        if config.plugins.native_plugins || config.plugins.enabled {
            Some(plugin_host::DynamicPluginHooks::for_dir(
                &config.plugins.dir.join("native"),
            ))
        } else {
            plugin_host::DynamicPluginHooks::load_dir_if_nonempty(
                &config.plugins.dir.join("native"),
            )
        };
    let jvm_hooks = running_host.hooks();
    let mut hook_layers: Vec<Arc<dyn game::plugin_types::ServerHooks>> = Vec::new();
    if let Some(native) = &native_hooks {
        hook_layers.push(Arc::clone(native) as Arc<dyn game::plugin_types::ServerHooks>);
    }
    if let Some(dynamic) = &dynamic_hooks {
        hook_layers.push(Arc::clone(dynamic) as Arc<dyn game::plugin_types::ServerHooks>);
    }
    if let Some(jvm) = &jvm_hooks {
        hook_layers.push(Arc::clone(jvm) as Arc<dyn game::plugin_types::ServerHooks>);
    }
    let plugin_hooks: Option<Arc<dyn game::plugin_types::ServerHooks>> =
        plugin_host::CompositeHooks::combine(hook_layers).map(|hooks| {
            plugin_host::ProfiledHooks::wrap(hooks) as Arc<dyn game::plugin_types::ServerHooks>
        });
    let ops = parse_ops(config.ops)?;
    let forwarding = build_forwarding(config.forwarding, &config.forwarding_secret)?;
    let rate_limits = config.rate_limits.to_net()?;
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
    tracing::info!(
        address = %listener.local_addr()?,
        online_mode = config.online_mode,
        view_distance = view_distance,
        storage = ?effective_storage,
        compression = ?effective_compression,
        threads = ?effective_threads,
        "Ferrite listener started"
    );
    if startup_cancelled(&cancelled) {
        tracing::info!("server startup cancelled");
        return Ok(());
    }

    let layout_world = match effective_storage {
        StorageMode::Anvil => effective_world.as_deref(),
        StorageMode::Fworld => None,
    };
    let startup_dirs = ferrite_server::startup_dirs::StartupDirectories::plan(
        &ferrite_server::startup_dirs::StartupLayout {
            root: std::path::Path::new("."),
            world: layout_world,
            worldgen: None,
        },
    );
    let created_dirs = startup_dirs.ensure()?;
    if !created_dirs.is_empty() {
        tracing::info!(
            count = created_dirs.len(),
            "created missing startup directories"
        );
    }

    let fworld_path = (effective_storage == StorageMode::Fworld).then(|| {
        let base = effective_world
            .as_deref()
            .unwrap_or_else(|| std::path::Path::new("world"));
        if base.extension().is_some_and(|ext| ext == "fworld") {
            base.to_path_buf()
        } else {
            std::path::PathBuf::from(format!("{}.fworld", base.display()))
        }
    });
    let existing_world = startup_dirs
        .world
        .as_ref()
        .is_some_and(|path| path.join("level.dat").exists())
        || fworld_path.as_ref().is_some_and(|path| path.exists());
    let spawn = match &startup_dirs.world {
        Some(path) => spawn_for_world(path)?,
        None => [0, 65, 0],
    };
    if startup_cancelled(&cancelled) {
        tracing::info!("server startup cancelled");
        return Ok(());
    }
    let need_registry = effective_generator == WorldGenerator::Rust
        || effective_storage == StorageMode::Fworld
        || startup_dirs.world.is_some();
    let shared_registry = if need_registry {
        Some(std::sync::Arc::new(
            ferrite_world::RuntimeRegistry::load(&vanilla_data)
                .context("loading vanilla registry")?,
        ))
    } else {
        None
    };
    let terrain: Option<std::sync::Arc<dyn game::ChunkGenerator>> = match effective_generator {
        WorldGenerator::Rust => {
            let registry = std::sync::Arc::clone(
                shared_registry
                    .as_ref()
                    .expect("shared registry loaded for rust worldgen"),
            );
            let seed = config.worldgen.seed.trim().parse::<u64>().unwrap_or(0);
            tracing::info!(seed, "using pure-Rust parallel world generator");
            Some(std::sync::Arc::new(game::RustGenerator::with_vanilla_data(
                seed,
                registry,
                &vanilla_data,
            )))
        }
        WorldGenerator::Superflat => None,
    };
    if startup_cancelled(&cancelled) {
        tracing::info!("server startup cancelled");
        return Ok(());
    }
    let source = match effective_storage {
        StorageMode::Fworld => {
            let blob_path = fworld_path.expect(".fworld path resolved");
            if let Some(parent) = blob_path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("creating .fworld parent {}", parent.display()))?;
            }
            let registry = shared_registry
                .as_deref()
                .expect("shared registry loaded for fworld")
                .clone();
            game::new_fworld_source(
                blob_path,
                registry,
                terrain,
                effective_compression.to_fworld(),
                spawn,
            )?
        }
        StorageMode::Anvil => {
            if let Some(path) = &startup_dirs.world {
                let registry = shared_registry
                    .as_deref()
                    .expect("shared registry loaded for anvil")
                    .clone();
                game::new_anvil_source_with_compression(
                    path.clone(),
                    registry,
                    terrain,
                    effective_compression.to_region(),
                )
            } else {
                game::new_memory_source(terrain)
            }
        }
    };
    // Vanilla terrain chooses a dry spawn above the generated surface; the flat world keeps the
    // fixed `[0, 65, 0]` the existing tests rely on.
    fn find_overworld_spawn(source: &game::AnvilChunkSource) -> [i32; 3] {
        let sea_level = game::Dimension::Overworld.sea_level();
        let origin_y = source.spawn_height(0, 0).unwrap_or(100);
        if origin_y >= sea_level {
            return [0, origin_y, 0];
        }
        for radius in (4..=64).step_by(4) {
            for dx in [-radius, 0, radius] {
                for dz in [-radius, 0, radius] {
                    if dx == 0 && dz == 0 {
                        continue;
                    }
                    if let Some(y) = source.spawn_height(dx, dz)
                        && y >= sea_level
                    {
                        return [dx, y, dz];
                    }
                }
            }
        }
        [0, origin_y, 0]
    }
    let spawn = if source.is_flat() || existing_world {
        spawn
    } else {
        find_overworld_spawn(&source)
    };
    if let Some(path) = &startup_dirs.world {
        // The vanilla save format puts the world spawn in `level.dat`. Write it once for a new
        // world so a restart (and `read_spawn`) sees the same position.
        if !path.join("level.dat").exists()
            && let Err(error) = game::write_spawn(path, spawn)
        {
            tracing::warn!(%error, world = %path.display(), "could not write level.dat");
        }
    }
    game::set_map_server_root(startup_dirs.world.clone());
    // The plugin bridge and the game loop share these sources, so a plugin write lands in the
    // same chunk data the server ticks.
    let dimensions = game::DimensionRegistry::with_source(
        game::Dimension::Overworld,
        std::sync::Arc::clone(&source),
        spawn,
    );
    let (disable_end_credits, perform_username_validation) =
        match ferrite_config::FerriteConfig::load(&startup_dirs.config) {
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
    let effective_threshold = launch
        .compression_threshold
        .unwrap_or(config.compression_threshold);
    anyhow::ensure!(
        effective_threshold >= -1,
        "compression-threshold must be >= -1, got {effective_threshold}"
    );
    let disable_compression = effective_threshold < 0;
    let net_compression_threshold = if disable_compression {
        0
    } else {
        effective_threshold as usize
    };
    let mut network = config.network;
    if let Some(level) = launch.compression_level {
        network.compression_level = level;
    }
    if let Some(skip) = launch.skip_incompressible {
        network.skip_incompressible = skip;
    }
    if let Some(offload) = launch.writer_offload_bytes {
        network.writer_offload_bytes = offload;
    }
    if let Some(linger) = launch.batch_linger_ms {
        network.batch_linger_ms = linger;
    }
    if let Some(max_conns) = launch.max_connections {
        network.max_connections = max_conns;
    }
    network.validate()?;
    let shared_threshold = (!disable_compression).then_some(net_compression_threshold);
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
        playerdata: Some(startup_dirs.playerdata.clone()),
        plugin_commands: None,
        dimensions: Some(dimensions),
        state_provider: None,
        threads: effective_threads,
        lighting_pool: launch.lighting_pool.unwrap_or(config.lighting_pool),
        shared_compression: Some((
            shared_threshold,
            network.compression_level,
            network.skip_incompressible,
        )),
    };
    // POSTWORLD plugins see a ready world and load before the game coordinator builds its
    // command tree and before the network listener accepts players.
    // v2 block reads/writes use the real dimension sources; writes are queued and applied by
    // `WorldStateProvider::tick` via the plugin host's game-loop tick.
    if running_host.host().is_some() || native_hooks.is_some() || dynamic_hooks.is_some() {
        let (plugin_cmd_tx, plugin_cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        if let Some(native) = &native_hooks {
            native.set_command_sender(plugin_cmd_tx.clone());
            native.set_dimensions(std::sync::Arc::clone(&host_dimensions));
        }
        if let Some(dynamic) = &dynamic_hooks {
            dynamic.set_command_sender(plugin_cmd_tx.clone());
            dynamic.set_dimensions(std::sync::Arc::clone(&host_dimensions));
        }
        if let Some(jvm) = &jvm_hooks {
            jvm.set_command_sender(plugin_cmd_tx.clone());
        }
        if let Some(host) = running_host.host() {
            let provider =
                std::sync::Arc::new(plugin_host::WorldStateProvider::new(host_dimensions));
            provider.set_command_sender(plugin_cmd_tx);
            settings.state_provider = Some(std::sync::Arc::clone(&provider));
            if let Err(error) = host.install_state_provider(provider) {
                tracing::warn!(%error, "installing the plugin state provider failed");
            }
        }
        settings.plugin_commands = Some(plugin_cmd_rx);
    }
    let native_autosave = native_hooks.as_ref().map(|native| {
        let native = Arc::clone(native);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
            interval.tick().await;
            loop {
                interval.tick().await;
                native.save();
            }
        })
    });
    let dynamic_watcher = dynamic_hooks.as_ref().map(|dynamic| {
        let dynamic = Arc::clone(dynamic);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
            interval.tick().await;
            loop {
                interval.tick().await;
                let summary = dynamic.auto_reload_if_changed();
                if !summary.loaded.is_empty()
                    || !summary.reloaded.is_empty()
                    || !summary.unloaded.is_empty()
                    || !summary.failed.is_empty()
                {
                    tracing::info!(
                        loaded = ?summary.loaded,
                        reloaded = ?summary.reloaded,
                        unloaded = ?summary.unloaded,
                        failed = ?summary.failed,
                        "dynamic native plugin auto-reload applied"
                    );
                }
            }
        })
    });
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
    let admin_http_handle = if config.admin_http.enabled {
        let bind = if config.admin_http.bind_address.trim().is_empty() {
            "127.0.0.1:8080"
        } else {
            config.admin_http.bind_address.trim()
        };
        match tokio::net::TcpListener::bind(bind).await {
            Ok(http_listener) => {
                tracing::info!(address = %bind, "v2.0 embedded admin HTTP dashboard started");
                let dyn_ref = dynamic_hooks.clone();
                let jvm_ref = running_host.host().cloned();
                let plugins_snapshot: ferrite_server::admin_http::PluginsSnapshotFn =
                    Arc::new(move || {
                        let dyn_list = dyn_ref
                            .as_ref()
                            .map(|d| d.loaded_plugin_summaries())
                            .unwrap_or_default();
                        let jvm_list: Vec<String> = jvm_ref
                            .as_ref()
                            .map(|h| {
                                h.loaded()
                                    .into_iter()
                                    .map(|p| format!("{} v{}", p.name, p.version))
                                    .collect()
                            })
                            .unwrap_or_default();
                        serde_json::json!({
                            "dynamic_native": dyn_list,
                            "jvm": jvm_list,
                        })
                    });
                Some(ferrite_server::admin_http::spawn_admin_http(
                    http_listener,
                    ferrite_server::admin_http::AdminHttpState {
                        profiler: world.profiler(),
                        plugins_snapshot,
                        bearer_token: config.admin_http.bearer_token.clone(),
                    },
                ))
            }
            Err(error) => {
                tracing::warn!(%error, address = %bind, "failed to bind admin HTTP dashboard");
                None
            }
        }
    } else {
        None
    };
    let mut server = tokio::spawn(ferrite_net::serve(
        listener,
        ferrite_net::ServerConfig {
            motd: config.motd,
            max_players: config.max_players,
            online_mode: config.online_mode,
            encrypt_offline: false,
            enforce_secure_profile: config.enforce_secure_profile,
            compression_threshold: net_compression_threshold,
            disable_compression,
            compression_level: network.compression_level,
            skip_incompressible: network.skip_incompressible,
            writer_offload_bytes: network.writer_offload_bytes,
            batch_linger: std::time::Duration::from_millis(network.batch_linger_ms),
            send_queue_bytes: network.send_queue_bytes,
            max_connections: (network.max_connections > 0).then_some(network.max_connections),
            connection_rate_limit: network.to_connection_rate_limit(),
            view_distance,
            forwarding,
            favicon: favicon.map(std::sync::Arc::from),
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
    if let Some(handle) = native_autosave {
        handle.abort();
    }
    if let Some(handle) = dynamic_watcher {
        handle.abort();
    }
    if let Some(handle) = admin_http_handle {
        handle.abort();
    }
    running_host.shutdown();
    if let Some(dynamic) = &dynamic_hooks {
        dynamic.shutdown();
    }
    if let Some(native) = &native_hooks {
        native.save();
    }
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
        assert_eq!(defaults.generator, WorldGenerator::Rust);
        assert_eq!(defaults.storage, StorageMode::Fworld);
        assert_eq!(
            toml::from_str::<Config>(DEFAULT_CONFIG).unwrap().generator,
            WorldGenerator::Rust
        );
        assert_eq!(
            toml::from_str::<Config>(DEFAULT_CONFIG).unwrap().storage,
            StorageMode::Fworld
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
        assert!(defaults.worldgen.seed.is_empty());
        let custom_wg: Config =
            toml::from_str("[worldgen]\nseed = '42'\nwork-dir = 'legacy-wg'\n").unwrap();
        assert_eq!(custom_wg.worldgen.seed, "42");
    }

    #[test]
    fn load_config_in_creates_default_once_and_preserves_existing() {
        let dir = tempfile::tempdir().unwrap();
        let (first, created) = load_config_in(dir.path()).unwrap();
        assert!(created);
        assert_eq!(first.world.as_deref(), Some(std::path::Path::new("world")));
        assert!(dir.path().join("ferrite.toml").is_file());

        let custom_text = "motd = 'Custom'\nworld = 'my-world'\n";
        std::fs::write(dir.path().join("ferrite.toml"), custom_text).unwrap();
        let (second, created_again) = load_config_in(dir.path()).unwrap();
        assert!(!created_again);
        assert_eq!(second.motd, "Custom");
        assert_eq!(
            second.world.as_deref(),
            Some(std::path::Path::new("my-world"))
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("ferrite.toml")).unwrap(),
            custom_text
        );
    }

    #[test]
    fn cli_args_parse_storage_compression_and_threads() {
        let parsed = parse_cli_args([
            "--headless",
            "--storage",
            "fworld",
            "--compression=lz4",
            "--threads",
            "6",
            "--world",
            "saves/custom",
            "--generator",
            "superflat",
        ])
        .unwrap();
        assert!(parsed.headless);
        assert_eq!(parsed.storage, Some(StorageMode::Fworld));
        assert_eq!(parsed.compression, Some(CompressionMode::Lz4));
        assert_eq!(parsed.threads, Some(6));
        assert_eq!(
            parsed.world.as_deref(),
            Some(std::path::Path::new("saves/custom"))
        );
        assert_eq!(parsed.generator, Some(WorldGenerator::Superflat));

        let fw = parse_cli_args(["--fworld", "--threads=0"]).unwrap();
        assert_eq!(fw.storage, Some(StorageMode::Fworld));
        assert_eq!(fw.threads, Some(0));
        assert!(parse_cli_args(["--memory"]).is_err());
        assert!(parse_cli_args(["--storage", "memory"]).is_err());

        let anvil = parse_cli_args(["--anvil", "--lighting-pool"]).unwrap();
        assert_eq!(anvil.storage, Some(StorageMode::Anvil));
        assert_eq!(anvil.lighting_pool, Some(true));

        let traffic = parse_cli_args([
            "--compression-threshold",
            "-1",
            "--compression-level=11",
            "--skip-incompressible",
            "false",
            "--writer-offload-bytes",
            "8192",
            "--batch-linger-ms",
            "5",
            "--max-connections",
            "2048",
        ])
        .unwrap();
        assert_eq!(traffic.compression_threshold, Some(-1));
        assert_eq!(traffic.compression_level, Some(11));
        assert_eq!(traffic.skip_incompressible, Some(false));
        assert_eq!(traffic.writer_offload_bytes, Some(8192));
        assert_eq!(traffic.batch_linger_ms, Some(5));
        assert_eq!(traffic.max_connections, Some(2048));

        assert!(parse_cli_args(["--compression-threshold", "-2"]).is_err());
        assert!(parse_cli_args(["--compression-level", "13"]).is_err());
        assert!(parse_cli_args(["--batch-linger-ms", "150"]).is_err());
        assert!(parse_cli_args(["--storage", "unknown"]).is_err());
        assert!(parse_cli_args(["--threads", "abc"]).is_err());
        assert!(parse_cli_args(["--unknown-flag"]).is_err());
    }

    #[test]
    fn config_parses_storage_compression_and_threads() {
        let cfg: Config = toml::from_str(
            "storage = 'fworld'\ncompression = 'lz4'\nthreads = 8\ncompression-threshold = 64\n\
             [network]\ncompression-level = 9\nskip-incompressible = false\nwriter-offload-bytes = 2048\nbatch-linger-ms = 4\nsend-queue-bytes = 4194304\nmax-connections = 1024\nconnection-burst = 12\nconnection-per-second = 3\n",
        )
        .unwrap();
        assert_eq!(cfg.storage, StorageMode::Fworld);
        assert_eq!(cfg.compression, CompressionMode::Lz4);
        assert_eq!(cfg.threads, Some(8));
        assert_eq!(cfg.compression_threshold, 64);
        assert_eq!(cfg.network.compression_level, 9);
        assert!(!cfg.network.skip_incompressible);
        assert_eq!(cfg.network.writer_offload_bytes, 2048);
        assert_eq!(cfg.network.batch_linger_ms, 4);
        assert_eq!(cfg.network.send_queue_bytes, 4_194_304);
        assert_eq!(cfg.network.max_connections, 1024);
        assert_eq!(cfg.network.connection_burst, 12);
        assert_eq!(cfg.network.connection_per_second, 3);
        assert!(cfg.network.to_connection_rate_limit().is_some());
        assert!(cfg.network.validate().is_ok());
        assert_eq!(
            cfg.compression.to_fworld(),
            ferrite_world::BlobCompression::Lz4
        );
        assert!(matches!(
            cfg.compression.to_region(),
            ferrite_world::RegionCompression::Lz4
        ));
    }

    #[test]
    fn tauri_launch_options_json_roundtrips() {
        let json = r#"{
            "storage": "fworld",
            "compression": "lz4",
            "threads": 6,
            "compressionThreshold": 128,
            "compressionLevel": 9,
            "skipIncompressible": false,
            "writerOffloadBytes": 8192,
            "batchLingerMs": 3,
            "maxConnections": 2048,
            "lightingPool": true
        }"#;
        let opts: LaunchOptions = serde_json::from_str(json).unwrap();
        assert_eq!(opts.storage, Some(StorageMode::Fworld));
        assert_eq!(opts.compression, Some(CompressionMode::Lz4));
        assert_eq!(opts.threads, Some(6));
        assert_eq!(opts.compression_threshold, Some(128));
        assert_eq!(opts.compression_level, Some(9));
        assert_eq!(opts.skip_incompressible, Some(false));
        assert_eq!(opts.writer_offload_bytes, Some(8192));
        assert_eq!(opts.batch_linger_ms, Some(3));
        assert_eq!(opts.max_connections, Some(2048));
        assert_eq!(opts.lighting_pool, Some(true));
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
        assert_eq!(example.plugins.runtime_classpath.len(), 2);
        assert_eq!(
            example.plugins.api_impl_class.as_deref(),
            Some("dev.ferrite.api.FerriteHostApi")
        );
        assert_eq!(
            example
                .plugins
                .to_host_config(ferrite_plugin_host::ServerInfo::default())
                .api_impl_class
                .as_deref(),
            Some("dev.ferrite.api.FerriteHostApi")
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
