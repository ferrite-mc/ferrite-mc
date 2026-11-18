//! `config/paper-global.yml` — Paper settings that apply to the whole server.
//!
//! Defaults were checked against the PaperMC configuration reference (2026 edition). Keys
//! that only exist in older Paper releases are kept for compatibility and marked
//! `UNVERIFIED` in the documentation and generated comments.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{ConfigFileKind, KeyComment, MaybeInt, YamlConfig};

/// `config/paper-global.yml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct PaperGlobal {
    /// Schema version. Paper writes `_version_`; older configs may use `config-version`.
    #[serde(rename = "_version_", alias = "config-version")]
    pub config_version: u32,
    /// Chunk loading limits that Paper tunes automatically by default.
    pub chunk_loading_advanced: ChunkLoadingAdvanced,
    /// Basic per-player chunk loading rates.
    pub chunk_loading_basic: ChunkLoadingBasic,
    /// Chunk system thread counts.
    pub chunk_system: ChunkSystem,
    /// Player and entity collision behavior.
    pub collisions: GlobalCollisions,
    /// Command handling.
    pub commands: GlobalCommands,
    /// Console behavior.
    pub console: Console,
    /// Limits for item names, lore, and books.
    pub item_validation: ItemValidation,
    /// Logging behavior.
    ///
    /// UNVERIFIED: this section is not present in current Paper docs; it is retained for
    /// older `paper-global.yml` files.
    pub logging: Logging,
    /// Player-facing messages.
    pub messages: Messages,
    /// Assorted gameplay and performance settings.
    pub misc: GlobalMisc,
    /// Packet rate limiting.
    pub packet_limiter: PacketLimiter,
    /// Player data auto-save.
    pub player_auto_save: PlayerAutoSave,
    /// Proxy support.
    pub proxies: Proxies,
    /// Scoreboard behavior.
    pub scoreboards: GlobalScoreboards,
    /// Chat spam limiting.
    pub spam_limiter: SpamLimiter,
    /// Bundled spark profiler.
    pub spark: Spark,
    /// World time behavior.
    pub time: TimeSettings,
    /// Aikar timings.
    ///
    /// UNVERIFIED: timings were removed from recent Paper releases; this section is kept so
    /// older files round-trip with a typed model.
    pub timings: Timings,
    /// Settings Paper does not officially support.
    pub unsupported_settings: UnsupportedSettings,
    /// Update checker.
    pub update_checker: UpdateChecker,
    /// Watchdog thread dump timings.
    pub watchdog: Watchdog,
}

impl Default for PaperGlobal {
    fn default() -> Self {
        Self {
            config_version: ConfigFileKind::PaperGlobal.supported_version(),
            chunk_loading_advanced: ChunkLoadingAdvanced::default(),
            chunk_loading_basic: ChunkLoadingBasic::default(),
            chunk_system: ChunkSystem::default(),
            collisions: GlobalCollisions::default(),
            commands: GlobalCommands::default(),
            console: Console::default(),
            item_validation: ItemValidation::default(),
            logging: Logging::default(),
            messages: Messages::default(),
            misc: GlobalMisc::default(),
            packet_limiter: PacketLimiter::default(),
            player_auto_save: PlayerAutoSave::default(),
            proxies: Proxies::default(),
            scoreboards: GlobalScoreboards::default(),
            spam_limiter: SpamLimiter::default(),
            spark: Spark::default(),
            time: TimeSettings::default(),
            timings: Timings::default(),
            unsupported_settings: UnsupportedSettings::default(),
            update_checker: UpdateChecker::default(),
            watchdog: Watchdog::default(),
        }
    }
}

/// `chunk-loading-advanced`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct ChunkLoadingAdvanced {
    /// Match the client's configured view distance when it is lower than the server's.
    pub auto_config_send_distance: bool,
    /// Maximum concurrent chunk generations per player; 0 auto-configures, -1 disables.
    pub player_max_concurrent_chunk_generates: i32,
    /// Maximum concurrent chunk loads per player; 0 auto-configures, -1 disables.
    pub player_max_concurrent_chunk_loads: i32,
}

impl Default for ChunkLoadingAdvanced {
    fn default() -> Self {
        Self {
            auto_config_send_distance: true,
            player_max_concurrent_chunk_generates: 0,
            player_max_concurrent_chunk_loads: 0,
        }
    }
}

/// `chunk-loading-basic`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct ChunkLoadingBasic {
    /// Maximum chunk generation rate per player; -1 disables the limit.
    pub player_max_chunk_generate_rate: f64,
    /// Maximum chunk load rate per player; -1 disables the limit.
    pub player_max_chunk_load_rate: f64,
    /// Maximum chunk send rate per player; -1 disables the limit.
    pub player_max_chunk_send_rate: f64,
}

impl Default for ChunkLoadingBasic {
    fn default() -> Self {
        Self {
            player_max_chunk_generate_rate: -1.0,
            player_max_chunk_load_rate: 100.0,
            player_max_chunk_send_rate: 75.0,
        }
    }
}

/// `chunk-system`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct ChunkSystem {
    /// Chunk generation parallelism.
    ///
    /// UNVERIFIED: present in older Paper releases, absent from the current reference.
    pub gen_parallelism: i32,
    /// Threads used for chunk read/write; below zero uses a single thread.
    pub io_threads: i32,
    /// Threads used for parallel chunk generation; below zero auto-detects.
    pub worker_threads: i32,
}

impl Default for ChunkSystem {
    fn default() -> Self {
        Self {
            gen_parallelism: -1,
            io_threads: -1,
            worker_threads: -1,
        }
    }
}

/// `collisions`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct GlobalCollisions {
    /// Allow players to collide with each other.
    pub enable_player_collisions: bool,
    /// Send precise positions for hard-colliding entities.
    pub send_full_pos_for_hard_colliding_entities: bool,
}

impl Default for GlobalCollisions {
    fn default() -> Self {
        Self {
            enable_player_collisions: true,
            send_full_pos_for_hard_colliding_entities: true,
        }
    }
}

/// `commands`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct GlobalCommands {
    /// Allow `/ride` to mount entities on a player.
    pub ride_command_allow_player_as_vehicle: bool,
    /// Suggest player names when no other completions exist.
    ///
    /// UNVERIFIED: present in older Paper releases, absent from the current reference.
    pub suggest_player_names_when_null_tab_completions: bool,
    /// Make `/time` affect all worlds.
    ///
    /// UNVERIFIED: older spelling; current Paper uses `time.affects-all-worlds`.
    pub time_command_affects_all_worlds: bool,
}

impl Default for GlobalCommands {
    fn default() -> Self {
        Self {
            ride_command_allow_player_as_vehicle: false,
            suggest_player_names_when_null_tab_completions: true,
            time_command_affects_all_worlds: false,
        }
    }
}

/// `console`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Console {
    /// Enable Brigadier completions in the console.
    pub enable_brigadier_completions: bool,
    /// Enable Brigadier highlighting in the console.
    pub enable_brigadier_highlighting: bool,
    /// Give the console sender all permissions.
    pub has_all_permissions: bool,
}

impl Default for Console {
    fn default() -> Self {
        Self {
            enable_brigadier_completions: true,
            enable_brigadier_highlighting: true,
            has_all_permissions: false,
        }
    }
}

/// `item-validation`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct ItemValidation {
    /// Book limits.
    pub book: BookValidation,
    /// Book byte-size limits.
    pub book_size: BookSizeValidation,
    /// Maximum display name length.
    pub display_name: i32,
    /// Maximum lore line length.
    pub lore_line: i32,
    /// Resolve selectors in books.
    pub resolve_selectors_in_books: bool,
}

impl Default for ItemValidation {
    fn default() -> Self {
        Self {
            book: BookValidation::default(),
            book_size: BookSizeValidation::default(),
            display_name: 8192,
            lore_line: 8192,
            resolve_selectors_in_books: false,
        }
    }
}

/// `item-validation.book`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct BookValidation {
    /// Maximum author length.
    pub author: i32,
    /// Maximum page length.
    pub page: i32,
    /// Maximum title length.
    pub title: i32,
}

impl Default for BookValidation {
    fn default() -> Self {
        Self {
            author: 8192,
            page: 16384,
            title: 8192,
        }
    }
}

/// `item-validation.book-size`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct BookSizeValidation {
    /// Maximum bytes contributed by a single page.
    pub page_max: i32,
    /// Multiplier applied per page.
    pub total_multiplier: f64,
}

impl Default for BookSizeValidation {
    fn default() -> Self {
        Self {
            page_max: 2560,
            total_multiplier: 0.98,
        }
    }
}

/// `logging` (legacy).
///
/// UNVERIFIED: not present in the current Paper reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Logging {
    /// Deobfuscate stack traces in the console.
    pub deobfuscate_stacktraces: bool,
    /// Log player IP addresses.
    pub log_player_ip_addresses: bool,
    /// Use RGB colors for named text colors.
    pub use_rgb_for_named_text_colors: bool,
}

impl Default for Logging {
    fn default() -> Self {
        Self {
            deobfuscate_stacktraces: true,
            log_player_ip_addresses: true,
            use_rgb_for_named_text_colors: true,
        }
    }
}

/// `messages`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Messages {
    /// Kick messages.
    pub kick: KickMessages,
    /// Permission failure message.
    pub no_permission: String,
    /// Use the player's display name in quit messages.
    pub use_display_name_in_quit_message: bool,
}

impl Default for Messages {
    fn default() -> Self {
        Self {
            kick: KickMessages::default(),
            no_permission: "<red>I'm sorry, but you do not have permission to perform this command. Please contact the server administrators if you believe that this is in error.".to_string(),
            use_display_name_in_quit_message: false,
        }
    }
}

/// `messages.kick`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct KickMessages {
    /// Sent when authentication servers are unreachable.
    pub authentication_servers_down: String,
    /// Sent when a connection is throttled.
    pub connection_throttle: String,
    /// Sent to players detected flying.
    pub flying_player: String,
    /// Sent to players riding a flying vehicle.
    pub flying_vehicle: String,
}

impl Default for KickMessages {
    fn default() -> Self {
        Self {
            authentication_servers_down: "<lang:multiplayer.disconnect.authservers_down>"
                .to_string(),
            connection_throttle: "Connection throttled! Please wait before reconnecting."
                .to_string(),
            flying_player: "<lang:multiplayer.disconnect.flying>".to_string(),
            flying_vehicle: "<lang:multiplayer.disconnect.flying>".to_string(),
        }
    }
}

/// `misc`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct GlobalMisc {
    /// Maximum ticks the server may catch up after a lag spike.
    pub catchup_ticks: MaybeInt,
    /// Chat thread pool sizing.
    pub chat_threads: ChatThreads,
    /// Extra interaction leniency distance.
    pub client_interaction_leniency_distance: MaybeInt,
    /// Network compression level.
    pub compression_level: MaybeInt,
    /// Enable the nether dimension.
    pub enable_nether: bool,
    /// Maximum players that may join in one tick.
    pub max_joins_per_tick: i32,
    /// Maximum tracked combat entries.
    pub max_tracking_combat_entries: i32,
    /// Prevent negative villager demand (MC-163962).
    pub prevent_negative_villager_demand: bool,
    /// Region file cache size.
    pub region_file_cache_size: i32,
    /// Strict advancement dimension checks.
    pub strict_advancement_dimension_check: bool,
    /// Use the alternative luck formula.
    pub use_alternative_luck_formula: bool,
    /// Use the dimension type for custom spawners.
    pub use_dimension_type_for_custom_spawners: bool,
    /// Experience orb groups per area.
    pub xp_orb_groups_per_area: MaybeInt,
}

impl Default for GlobalMisc {
    fn default() -> Self {
        Self {
            catchup_ticks: MaybeInt::Default,
            chat_threads: ChatThreads::default(),
            client_interaction_leniency_distance: MaybeInt::Default,
            compression_level: MaybeInt::Default,
            enable_nether: true,
            max_joins_per_tick: 5,
            max_tracking_combat_entries: 10240,
            prevent_negative_villager_demand: false,
            region_file_cache_size: 256,
            strict_advancement_dimension_check: false,
            use_alternative_luck_formula: false,
            use_dimension_type_for_custom_spawners: false,
            xp_orb_groups_per_area: MaybeInt::Default,
        }
    }
}

/// `misc.chat-threads`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct ChatThreads {
    /// Core thread count; values below zero behave like zero.
    pub chat_executor_core_size: i32,
    /// Maximum thread count; values at or below zero are unlimited.
    pub chat_executor_max_size: i32,
}

impl Default for ChatThreads {
    fn default() -> Self {
        Self {
            chat_executor_core_size: -1,
            chat_executor_max_size: -1,
        }
    }
}

/// `packet-limiter`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct PacketLimiter {
    /// Limit applied to every packet.
    pub all_packets: PacketRate,
    /// Kick message for rate limit violations.
    pub kick_message: String,
    /// Per-packet overrides.
    pub overrides: BTreeMap<String, PacketRate>,
}

impl Default for PacketLimiter {
    fn default() -> Self {
        let mut overrides = BTreeMap::new();
        overrides.insert(
            "minecraft:place_recipe".to_string(),
            PacketRate {
                action: PacketLimiterAction::Kick,
                interval: 7.0,
                max_packet_rate: 500.0,
            },
        );
        Self {
            all_packets: PacketRate {
                action: PacketLimiterAction::Kick,
                interval: 7.0,
                max_packet_rate: 500.0,
            },
            kick_message: "<red><lang:disconnect.exceeded_packet_rate>".to_string(),
            overrides,
        }
    }
}

/// Packet limiter action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PacketLimiterAction {
    /// Drop packets over the limit.
    Drop,
    /// Kick players over the limit.
    Kick,
}

/// One packet limiter rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct PacketRate {
    /// Action to take when the limit is exceeded.
    pub action: PacketLimiterAction,
    /// Window length in seconds.
    pub interval: f64,
    /// Maximum packets per window.
    pub max_packet_rate: f64,
}

impl Default for PacketRate {
    fn default() -> Self {
        Self {
            action: PacketLimiterAction::Kick,
            interval: 7.0,
            max_packet_rate: 500.0,
        }
    }
}

/// `player-auto-save`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct PlayerAutoSave {
    /// Maximum players saved per tick; -1 picks a recommended value.
    pub max_per_tick: i32,
    /// Save interval in ticks; -1 uses `ticks-per.autosave` from bukkit.yml.
    pub rate: i32,
}

impl Default for PlayerAutoSave {
    fn default() -> Self {
        Self {
            max_per_tick: -1,
            rate: -1,
        }
    }
}

/// `proxies`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Proxies {
    /// BungeeCord support.
    pub bungee_cord: BungeeCord,
    /// Process PROXY protocol headers.
    pub proxy_protocol: bool,
    /// Velocity modern forwarding.
    pub velocity: Velocity,
}

/// `proxies.bungee-cord`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct BungeeCord {
    /// Online mode expected from the proxy.
    pub online_mode: bool,
}

impl Default for BungeeCord {
    fn default() -> Self {
        Self { online_mode: true }
    }
}

/// `proxies.velocity`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Velocity {
    /// Accept Velocity modern forwarding.
    pub enabled: bool,
    /// Online mode expected from the proxy.
    pub online_mode: bool,
    /// Shared forwarding secret.
    pub secret: String,
}

impl Default for Velocity {
    fn default() -> Self {
        Self {
            enabled: false,
            online_mode: true,
            secret: String::new(),
        }
    }
}

/// `scoreboards` (global).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct GlobalScoreboards {
    /// Remove empty scoreboard teams on login.
    pub save_empty_scoreboard_teams: bool,
}

impl Default for GlobalScoreboards {
    fn default() -> Self {
        Self {
            save_empty_scoreboard_teams: true,
        }
    }
}

/// `spam-limiter`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct SpamLimiter {
    /// Incoming packet threshold before packets are ignored.
    pub incoming_packet_threshold: i32,
    /// Recipe spam counter increment.
    pub recipe_spam_increment: i32,
    /// Recipe spam limit.
    pub recipe_spam_limit: i32,
    /// Tab spam counter increment.
    pub tab_spam_increment: i32,
    /// Tab spam limit.
    pub tab_spam_limit: i32,
}

impl Default for SpamLimiter {
    fn default() -> Self {
        Self {
            incoming_packet_threshold: 300,
            recipe_spam_increment: 1,
            recipe_spam_limit: 20,
            tab_spam_increment: 1,
            tab_spam_limit: 500,
        }
    }
}

/// `spark`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Spark {
    /// Enable spark on startup.
    pub enabled: bool,
    /// Enable spark before the server finishes starting.
    pub enable_immediately: bool,
}

impl Default for Spark {
    fn default() -> Self {
        Self {
            enabled: true,
            enable_immediately: false,
        }
    }
}

/// `time`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct TimeSettings {
    /// Share one clock between worlds of the same dimension type.
    pub affects_all_worlds: bool,
}

/// `timings` (legacy).
///
/// UNVERIFIED: removed from current Paper; retained for older files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Timings {
    /// Enable Aikar timings.
    pub enabled: bool,
    /// Config entries hidden from timing reports.
    pub hidden_config_entries: Vec<String>,
    /// History interval in seconds.
    pub history_interval: i32,
    /// History length in seconds.
    pub history_length: i32,
    /// Server name shown in reports.
    pub server_name: String,
    /// Hide the server name in reports.
    pub server_name_privacy: bool,
    /// Upload URL.
    pub url: String,
    /// Verbose timing output.
    pub verbose: bool,
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            enabled: true,
            hidden_config_entries: vec!["proxies.velocity.secret".to_string()],
            history_interval: 300,
            history_length: 3600,
            server_name: "Unknown Server".to_string(),
            server_name_privacy: false,
            url: "https://timings.aikar.co/".to_string(),
            verbose: true,
        }
    }
}

/// `unsupported-settings`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct UnsupportedSettings {
    /// Allow headless pistons.
    pub allow_headless_pistons: bool,
    /// Allow exploits that break permanent blocks.
    pub allow_permanent_block_break_exploits: bool,
    /// Allow piston duplication of TNT, carpets, and rails.
    pub allow_piston_duplication: bool,
    /// Allow unsafe end portal teleportation.
    pub allow_unsafe_end_portal_teleportation: bool,
    /// Oversized item component sanitizer.
    pub oversized_item_component_sanitizer: OversizedItemComponentSanitizer,
    /// Validate usernames.
    pub perform_username_validation: bool,
    /// Skip tripwire hook placement validation.
    pub skip_tripwire_hook_placement_validation: bool,
    /// Skip the vanilla damage tick when a shield blocks damage.
    pub skip_vanilla_damage_tick_when_shield_blocked: bool,
    /// Update equipment on player actions.
    pub update_equipment_on_player_actions: bool,
}

impl Default for UnsupportedSettings {
    fn default() -> Self {
        Self {
            allow_headless_pistons: false,
            allow_permanent_block_break_exploits: false,
            allow_piston_duplication: false,
            allow_unsafe_end_portal_teleportation: false,
            oversized_item_component_sanitizer: OversizedItemComponentSanitizer::default(),
            perform_username_validation: true,
            skip_tripwire_hook_placement_validation: false,
            skip_vanilla_damage_tick_when_shield_blocked: false,
            update_equipment_on_player_actions: true,
        }
    }
}

/// `unsupported-settings.oversized-item-component-sanitizer`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct OversizedItemComponentSanitizer {
    /// Components that should not be sanitized.
    pub dont_sanitize: Vec<String>,
}

/// `update-checker`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct UpdateChecker {
    /// Check for Paper updates on startup.
    pub enabled: bool,
}

impl Default for UpdateChecker {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// `watchdog`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Watchdog {
    /// Milliseconds before the first thread dump.
    pub early_warning_delay: i32,
    /// Milliseconds between thread dumps.
    pub early_warning_every: i32,
}

impl Default for Watchdog {
    fn default() -> Self {
        Self {
            early_warning_delay: 10000,
            early_warning_every: 5000,
        }
    }
}

const COMMENTS: &[KeyComment] = &[
    KeyComment {
        path: "_version_",
        lines: &["Paper configuration version. Paper writes this as `_version_`."],
        unverified: false,
    },
    KeyComment {
        path: "chunk-loading-advanced",
        lines: &["Chunk loading limits that Paper tunes automatically by default."],
        unverified: false,
    },
    KeyComment {
        path: "chunk-loading-advanced.auto-config-send-distance",
        lines: &["Match a client's lower view distance."],
        unverified: false,
    },
    KeyComment {
        path: "chunk-loading-advanced.player-max-concurrent-chunk-generates",
        lines: &["Concurrent chunk generations per player; 0 auto-configures, -1 disables."],
        unverified: false,
    },
    KeyComment {
        path: "chunk-loading-advanced.player-max-concurrent-chunk-loads",
        lines: &["Concurrent chunk loads per player; 0 auto-configures, -1 disables."],
        unverified: false,
    },
    KeyComment {
        path: "chunk-loading-basic",
        lines: &["Basic per-player chunk loading rates."],
        unverified: false,
    },
    KeyComment {
        path: "chunk-loading-basic.player-max-chunk-generate-rate",
        lines: &["Chunks generated per second per player; -1 disables the limit."],
        unverified: false,
    },
    KeyComment {
        path: "chunk-loading-basic.player-max-chunk-load-rate",
        lines: &["Chunks loaded per second per player; -1 disables the limit."],
        unverified: false,
    },
    KeyComment {
        path: "chunk-loading-basic.player-max-chunk-send-rate",
        lines: &["Chunks sent per second per player; -1 disables the limit."],
        unverified: false,
    },
    KeyComment {
        path: "chunk-system",
        lines: &["Chunk system thread counts."],
        unverified: false,
    },
    KeyComment {
        path: "chunk-system.gen-parallelism",
        lines: &["Chunk generation parallelism."],
        unverified: true,
    },
    KeyComment {
        path: "chunk-system.io-threads",
        lines: &["Chunk read/write threads; below zero uses one thread."],
        unverified: false,
    },
    KeyComment {
        path: "chunk-system.worker-threads",
        lines: &["Parallel chunk generation threads; below zero auto-detects."],
        unverified: false,
    },
    KeyComment {
        path: "collisions",
        lines: &["Collision behavior."],
        unverified: false,
    },
    KeyComment {
        path: "collisions.enable-player-collisions",
        lines: &["Allow players to collide with each other."],
        unverified: false,
    },
    KeyComment {
        path: "collisions.send-full-pos-for-hard-colliding-entities",
        lines: &["Send precise positions for hard-colliding entities."],
        unverified: false,
    },
    KeyComment {
        path: "commands",
        lines: &["Command handling."],
        unverified: false,
    },
    KeyComment {
        path: "commands.ride-command-allow-player-as-vehicle",
        lines: &["Allow mounting entities on a player with /ride."],
        unverified: false,
    },
    KeyComment {
        path: "commands.suggest-player-names-when-null-tab-completions",
        lines: &["Suggest player names when no other completions exist."],
        unverified: true,
    },
    KeyComment {
        path: "commands.time-command-affects-all-worlds",
        lines: &["Make /time affect all worlds."],
        unverified: true,
    },
    KeyComment {
        path: "console",
        lines: &["Console behavior."],
        unverified: false,
    },
    KeyComment {
        path: "console.enable-brigadier-completions",
        lines: &["Enable Brigadier completions in the console."],
        unverified: false,
    },
    KeyComment {
        path: "console.enable-brigadier-highlighting",
        lines: &["Enable Brigadier highlighting in the console."],
        unverified: false,
    },
    KeyComment {
        path: "console.has-all-permissions",
        lines: &["Give the console sender all permissions."],
        unverified: false,
    },
    KeyComment {
        path: "item-validation",
        lines: &["Limits for item names, lore, and books."],
        unverified: false,
    },
    KeyComment {
        path: "item-validation.book",
        lines: &["Book text limits."],
        unverified: false,
    },
    KeyComment {
        path: "item-validation.book-size",
        lines: &["Book byte-size limits."],
        unverified: false,
    },
    KeyComment {
        path: "item-validation.display-name",
        lines: &["Maximum display name length."],
        unverified: false,
    },
    KeyComment {
        path: "item-validation.lore-line",
        lines: &["Maximum lore line length."],
        unverified: false,
    },
    KeyComment {
        path: "item-validation.resolve-selectors-in-books",
        lines: &["Resolve selectors in books."],
        unverified: false,
    },
    KeyComment {
        path: "logging",
        lines: &["Legacy logging section."],
        unverified: true,
    },
    KeyComment {
        path: "messages",
        lines: &["Player-facing messages, in MiniMessage format."],
        unverified: false,
    },
    KeyComment {
        path: "messages.kick",
        lines: &["Kick messages."],
        unverified: false,
    },
    KeyComment {
        path: "messages.no-permission",
        lines: &["Sent when a player lacks permission for a command."],
        unverified: false,
    },
    KeyComment {
        path: "messages.use-display-name-in-quit-message",
        lines: &["Use the player's display name in quit messages."],
        unverified: false,
    },
    KeyComment {
        path: "misc",
        lines: &["Assorted gameplay and performance settings."],
        unverified: false,
    },
    KeyComment {
        path: "misc.catchup-ticks",
        lines: &["Maximum catch-up ticks after a lag spike; `default` means 5, 0 disables."],
        unverified: false,
    },
    KeyComment {
        path: "misc.compression-level",
        lines: &["Network compression level; `default` means -1 (no compression)."],
        unverified: false,
    },
    KeyComment {
        path: "misc.enable-nether",
        lines: &["Enable and load the nether dimension."],
        unverified: false,
    },
    KeyComment {
        path: "misc.max-joins-per-tick",
        lines: &["Maximum players that may join in one tick."],
        unverified: false,
    },
    KeyComment {
        path: "misc.region-file-cache-size",
        lines: &["Region file cache size."],
        unverified: false,
    },
    KeyComment {
        path: "misc.use-alternative-luck-formula",
        lines: &["Use the alternative luck formula for fishing."],
        unverified: false,
    },
    KeyComment {
        path: "misc.use-dimension-type-for-custom-spawners",
        lines: &["Allow custom overworld spawners to use the dimension type."],
        unverified: false,
    },
    KeyComment {
        path: "packet-limiter",
        lines: &["Packet rate limiting."],
        unverified: false,
    },
    KeyComment {
        path: "packet-limiter.all-packets",
        lines: &["Limit applied to every packet."],
        unverified: false,
    },
    KeyComment {
        path: "packet-limiter.kick-message",
        lines: &["Message used when kicking for exceeding the packet rate."],
        unverified: false,
    },
    KeyComment {
        path: "packet-limiter.overrides",
        lines: &["Per-packet limits; keys are packet identifiers such as minecraft:place_recipe."],
        unverified: false,
    },
    KeyComment {
        path: "player-auto-save",
        lines: &["Player data auto-save."],
        unverified: false,
    },
    KeyComment {
        path: "player-auto-save.max-per-tick",
        lines: &["Maximum players saved per tick; -1 picks a recommended value."],
        unverified: false,
    },
    KeyComment {
        path: "player-auto-save.rate",
        lines: &["Save interval in ticks; -1 uses ticks-per.autosave in bukkit.yml."],
        unverified: false,
    },
    KeyComment {
        path: "proxies",
        lines: &["Proxy support."],
        unverified: false,
    },
    KeyComment {
        path: "proxies.bungee-cord",
        lines: &["BungeeCord support."],
        unverified: false,
    },
    KeyComment {
        path: "proxies.bungee-cord.online-mode",
        lines: &["Online mode expected from the BungeeCord proxy."],
        unverified: false,
    },
    KeyComment {
        path: "proxies.proxy-protocol",
        lines: &["Process PROXY protocol headers (HAProxy and similar)."],
        unverified: false,
    },
    KeyComment {
        path: "proxies.velocity",
        lines: &["Velocity modern forwarding."],
        unverified: false,
    },
    KeyComment {
        path: "proxies.velocity.enabled",
        lines: &["Accept Velocity modern forwarding."],
        unverified: false,
    },
    KeyComment {
        path: "proxies.velocity.online-mode",
        lines: &["Online mode expected from the Velocity proxy."],
        unverified: false,
    },
    KeyComment {
        path: "proxies.velocity.secret",
        lines: &["Shared forwarding secret; must match forwarding.secret on the proxy."],
        unverified: false,
    },
    KeyComment {
        path: "scoreboards",
        lines: &["Scoreboard behavior."],
        unverified: false,
    },
    KeyComment {
        path: "scoreboards.save-empty-scoreboard-teams",
        lines: &["Remove empty scoreboard teams on login."],
        unverified: false,
    },
    KeyComment {
        path: "spam-limiter",
        lines: &["Chat and recipe spam limits."],
        unverified: false,
    },
    KeyComment {
        path: "spark",
        lines: &["Bundled spark profiler."],
        unverified: false,
    },
    KeyComment {
        path: "time",
        lines: &["World time behavior."],
        unverified: false,
    },
    KeyComment {
        path: "time.affects-all-worlds",
        lines: &["Share one clock between worlds by dimension type."],
        unverified: false,
    },
    KeyComment {
        path: "timings",
        lines: &["Legacy Aikar timings section; removed from current Paper."],
        unverified: true,
    },
    KeyComment {
        path: "unsupported-settings",
        lines: &["Settings Paper does not officially support."],
        unverified: false,
    },
    KeyComment {
        path: "unsupported-settings.allow-headless-pistons",
        lines: &["Allow headless pistons."],
        unverified: false,
    },
    KeyComment {
        path: "unsupported-settings.allow-permanent-block-break-exploits",
        lines: &["Allow breaking normally unbreakable blocks with vanilla exploits."],
        unverified: false,
    },
    KeyComment {
        path: "unsupported-settings.allow-piston-duplication",
        lines: &["Allow piston duplication of TNT, carpets, and rails."],
        unverified: false,
    },
    KeyComment {
        path: "unsupported-settings.allow-unsafe-end-portal-teleportation",
        lines: &["Allow unsafe end portal teleportation exploits."],
        unverified: false,
    },
    KeyComment {
        path: "unsupported-settings.perform-username-validation",
        lines: &["Validate player usernames."],
        unverified: false,
    },
    KeyComment {
        path: "unsupported-settings.update-equipment-on-player-actions",
        lines: &["Update equipment when handling player actions."],
        unverified: false,
    },
    KeyComment {
        path: "update-checker",
        lines: &["Update checker."],
        unverified: false,
    },
    KeyComment {
        path: "update-checker.enabled",
        lines: &["Check for Paper updates on startup."],
        unverified: false,
    },
    KeyComment {
        path: "watchdog",
        lines: &["Watchdog thread dump timings."],
        unverified: false,
    },
    KeyComment {
        path: "watchdog.early-warning-delay",
        lines: &["Milliseconds before the first thread dump."],
        unverified: false,
    },
    KeyComment {
        path: "watchdog.early-warning-every",
        lines: &["Milliseconds between thread dumps."],
        unverified: false,
    },
];

impl YamlConfig for PaperGlobal {
    const KIND: ConfigFileKind = ConfigFileKind::PaperGlobal;

    fn key_comments() -> &'static [KeyComment] {
        COMMENTS
    }
}
