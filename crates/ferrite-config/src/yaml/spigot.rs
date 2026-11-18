//! `spigot.yml` — Spigot/Paper gameplay and performance settings.
//!
//! Only commonly changed keys are typed; everything else is preserved as an unknown key in
//! the raw document.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{ConfigFileKind, KeyComment, YamlConfig};

/// `spigot.yml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct SpigotConfig {
    /// Global settings.
    pub settings: SpigotSettings,
    /// Kick and informational messages.
    pub messages: SpigotMessages,
    /// Advancement saving.
    pub advancements: Advancements,
    /// Statistics saving.
    pub stats: Stats,
    /// Command handling.
    pub commands: SpigotCommands,
    /// Player data handling.
    pub players: SpigotPlayers,
    /// Per-world settings; always contains a `default` entry.
    pub world_settings: BTreeMap<String, SpigotWorld>,
}

impl Default for SpigotConfig {
    fn default() -> Self {
        let mut world_settings = BTreeMap::new();
        world_settings.insert("default".to_string(), SpigotWorld::default());
        Self {
            settings: SpigotSettings::default(),
            messages: SpigotMessages::default(),
            advancements: Advancements::default(),
            stats: Stats::default(),
            commands: SpigotCommands::default(),
            players: SpigotPlayers::default(),
            world_settings,
        }
    }
}

/// `spigot.yml` `settings` section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct SpigotSettings {
    /// Enable debug logging.
    pub debug: bool,
    /// Enable BungeeCord support.
    pub bungeecord: bool,
    /// Only save player caches when the server stops.
    pub save_user_cache_on_stop_only: bool,
    /// Number of players sampled for the player list.
    pub sample_count: i32,
    /// Shuffle the player list by this amount.
    pub player_shuffle: i32,
    /// Player cache size.
    pub user_cache_size: i32,
    /// Netty worker threads.
    pub netty_threads: i32,
    /// Vanilla attribute caps.
    pub attribute: SpigotAttributes,
    /// Seconds before a non-responsive client is disconnected.
    pub timeout_time: i32,
    /// Restart the server after a crash.
    pub restart_on_crash: bool,
    /// Script used for restarts.
    pub restart_script: String,
    /// Maximum distance for a player move to be accepted.
    pub moved_wrongly_threshold: f64,
    /// Multiplier for speed checks.
    pub moved_too_quickly_multiplier: f64,
    /// Log villager deaths.
    pub log_villager_deaths: bool,
    /// Log named entity deaths.
    ///
    /// UNVERIFIED: recent Spigot addition; default `true`.
    pub log_named_deaths: bool,
}

impl Default for SpigotSettings {
    fn default() -> Self {
        Self {
            debug: false,
            bungeecord: false,
            save_user_cache_on_stop_only: false,
            sample_count: 12,
            player_shuffle: 0,
            user_cache_size: 1000,
            netty_threads: 4,
            attribute: SpigotAttributes::default(),
            timeout_time: 60,
            restart_on_crash: true,
            restart_script: "./start.sh".to_string(),
            moved_wrongly_threshold: 0.0625,
            moved_too_quickly_multiplier: 10.0,
            log_villager_deaths: true,
            log_named_deaths: true,
        }
    }
}

/// `spigot.yml` `settings.attribute` section (camelCase keys, as Spigot writes them).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct SpigotAttributes {
    /// Maximum absorption value.
    #[serde(rename = "maxAbsorption")]
    pub max_absorption: f64,
    /// Maximum health value.
    #[serde(rename = "maxHealth")]
    pub max_health: f64,
    /// Maximum movement speed.
    #[serde(rename = "movementSpeed")]
    pub movement_speed: f64,
    /// Maximum attack damage.
    #[serde(rename = "attackDamage")]
    pub attack_damage: f64,
}

impl Default for SpigotAttributes {
    fn default() -> Self {
        Self {
            max_absorption: 2048.0,
            max_health: 2048.0,
            movement_speed: 1024.0,
            attack_damage: 2048.0,
        }
    }
}

/// `spigot.yml` `messages` section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct SpigotMessages {
    /// Message sent to non-whitelisted players.
    pub whitelist: String,
    /// Message sent for unknown commands.
    ///
    /// UNVERIFIED: exact default text may have changed.
    pub unknown_command: String,
    /// Message sent when the server is full.
    pub server_full: String,
    /// Message sent to outdated clients.
    pub outdated_client: String,
    /// Message sent to outdated servers.
    pub outdated_server: String,
    /// Message sent when the server restarts.
    pub restart: String,
}

impl Default for SpigotMessages {
    fn default() -> Self {
        Self {
            whitelist: "You are not whitelisted on this server!".to_string(),
            unknown_command: "Unknown command. Type \"/help\" for help. You can use \"/help <command>\" to get specific help on a command.".to_string(),
            server_full: "The server is full!".to_string(),
            outdated_client: "Outdated client! Please use {0}".to_string(),
            outdated_server: "Outdated server! I'm still on {0}".to_string(),
            restart: "Server is restarting".to_string(),
        }
    }
}

/// `spigot.yml` `advancements` section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Advancements {
    /// Disable advancement saving.
    pub disable_saving: bool,
    /// Advancements that are disabled.
    pub disabled: Vec<String>,
}

/// `spigot.yml` `stats` section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Stats {
    /// Disable statistics saving.
    pub disable_saving: bool,
    /// Forced statistic values.
    pub forced_stats: BTreeMap<String, i32>,
}

/// `spigot.yml` `commands` section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct SpigotCommands {
    /// Silence command block console output.
    pub silent_commandblock_console: bool,
    /// Commands excluded from spam checks.
    pub spam_exclusions: Vec<String>,
    /// Tab-completion mode.
    pub tab_complete: i32,
    /// Send namespaced command names.
    pub send_namespaced: bool,
    /// Log commands to the console.
    pub log: bool,
}

impl Default for SpigotCommands {
    fn default() -> Self {
        Self {
            silent_commandblock_console: false,
            spam_exclusions: vec!["-skill".to_string()],
            tab_complete: 0,
            send_namespaced: true,
            log: true,
        }
    }
}

/// `spigot.yml` `players` section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct SpigotPlayers {
    /// Disable player data saving.
    pub disable_saving: bool,
}

/// One entry of `spigot.yml` `world-settings` (usually `default`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct SpigotWorld {
    /// Verbose per-world logging.
    pub verbose: bool,
    /// View distance for this world.
    pub view_distance: i32,
    /// Simulation distance for this world.
    ///
    /// UNVERIFIED: availability in spigot.yml depends on the server version.
    pub simulation_distance: i32,
    /// Ticks before dropped items despawn.
    pub item_despawn_rate: i32,
    /// Item and experience merge radii.
    pub merge_radius: MergeRadius,
    /// Ticks a hopper waits between transfers.
    pub hopper_transfer: i32,
    /// Ticks between hopper checks.
    pub hopper_check: i32,
    /// Maximum entities colliding with one entity.
    pub max_entity_collisions: i32,
    /// Maximum TNT entities primed per tick.
    pub max_tnt_per_tick: i32,
    /// Remove AI from mobs spawned by spawners.
    pub nerf_spawner_mobs: bool,
    /// Entity activation ranges.
    pub entity_activation_range: EntityActivationRange,
    /// Entity tracking ranges.
    pub entity_tracking_range: EntityTrackingRange,
}

impl Default for SpigotWorld {
    fn default() -> Self {
        Self {
            verbose: false,
            view_distance: 10,
            simulation_distance: 10,
            item_despawn_rate: 6000,
            merge_radius: MergeRadius::default(),
            hopper_transfer: 8,
            hopper_check: 1,
            max_entity_collisions: 8,
            max_tnt_per_tick: 100,
            nerf_spawner_mobs: false,
            entity_activation_range: EntityActivationRange::default(),
            entity_tracking_range: EntityTrackingRange::default(),
        }
    }
}

/// `spigot.yml` `world-settings.<world>.merge-radius`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct MergeRadius {
    /// Item merge radius in blocks.
    pub item: f64,
    /// Experience orb merge radius in blocks.
    pub exp: f64,
}

impl Default for MergeRadius {
    fn default() -> Self {
        Self {
            item: 2.5,
            exp: 3.0,
        }
    }
}

/// `spigot.yml` `world-settings.<world>.entity-activation-range`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct EntityActivationRange {
    /// Animals.
    pub animals: i32,
    /// Monsters.
    pub monsters: i32,
    /// Miscellaneous entities.
    pub misc: i32,
    /// Water creatures.
    pub water: i32,
    /// Villagers.
    pub villagers: i32,
    /// Flying monsters.
    pub flying_monsters: i32,
    /// Raiders.
    pub raiders: i32,
}

impl Default for EntityActivationRange {
    fn default() -> Self {
        Self {
            animals: 16,
            monsters: 24,
            misc: 8,
            water: 8,
            villagers: 16,
            flying_monsters: 32,
            raiders: 48,
        }
    }
}

/// `spigot.yml` `world-settings.<world>.entity-tracking-range`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct EntityTrackingRange {
    /// Players.
    pub players: i32,
    /// Animals.
    pub animals: i32,
    /// Monsters.
    pub monsters: i32,
    /// Miscellaneous entities.
    pub misc: i32,
    /// All other entities.
    pub other: i32,
}

impl Default for EntityTrackingRange {
    fn default() -> Self {
        Self {
            players: 48,
            animals: 48,
            monsters: 48,
            misc: 32,
            other: 64,
        }
    }
}

const COMMENTS: &[KeyComment] = &[
    KeyComment {
        path: "settings",
        lines: &["Global Spigot settings."],
        unverified: false,
    },
    KeyComment {
        path: "settings.debug",
        lines: &["Enable debug logging."],
        unverified: false,
    },
    KeyComment {
        path: "settings.bungeecord",
        lines: &["Enable BungeeCord support."],
        unverified: false,
    },
    KeyComment {
        path: "settings.save-user-cache-on-stop-only",
        lines: &["Only save the player cache when the server stops."],
        unverified: false,
    },
    KeyComment {
        path: "settings.sample-count",
        lines: &["Number of players sampled for the server list."],
        unverified: false,
    },
    KeyComment {
        path: "settings.player-shuffle",
        lines: &["Shuffle the server list player sample by this amount."],
        unverified: false,
    },
    KeyComment {
        path: "settings.user-cache-size",
        lines: &["Player cache size."],
        unverified: false,
    },
    KeyComment {
        path: "settings.netty-threads",
        lines: &["Netty worker threads."],
        unverified: false,
    },
    KeyComment {
        path: "settings.attribute",
        lines: &["Vanilla attribute caps."],
        unverified: false,
    },
    KeyComment {
        path: "settings.attribute.maxAbsorption",
        lines: &["Maximum absorption value."],
        unverified: false,
    },
    KeyComment {
        path: "settings.attribute.maxHealth",
        lines: &["Maximum health value."],
        unverified: false,
    },
    KeyComment {
        path: "settings.attribute.movementSpeed",
        lines: &["Maximum movement speed."],
        unverified: false,
    },
    KeyComment {
        path: "settings.attribute.attackDamage",
        lines: &["Maximum attack damage."],
        unverified: false,
    },
    KeyComment {
        path: "settings.timeout-time",
        lines: &["Seconds before an unresponsive client is disconnected."],
        unverified: false,
    },
    KeyComment {
        path: "settings.restart-on-crash",
        lines: &["Restart the server after a crash."],
        unverified: false,
    },
    KeyComment {
        path: "settings.restart-script",
        lines: &["Script used to restart the server."],
        unverified: false,
    },
    KeyComment {
        path: "settings.moved-wrongly-threshold",
        lines: &["Threshold for rejecting a player move."],
        unverified: false,
    },
    KeyComment {
        path: "settings.moved-too-quickly-multiplier",
        lines: &["Multiplier for movement speed checks."],
        unverified: false,
    },
    KeyComment {
        path: "settings.log-villager-deaths",
        lines: &["Log villager deaths."],
        unverified: false,
    },
    KeyComment {
        path: "settings.log-named-deaths",
        lines: &["Log deaths of named entities."],
        unverified: true,
    },
    KeyComment {
        path: "messages",
        lines: &["Messages sent to players."],
        unverified: false,
    },
    KeyComment {
        path: "messages.whitelist",
        lines: &["Sent to players who are not whitelisted."],
        unverified: false,
    },
    KeyComment {
        path: "messages.unknown-command",
        lines: &["Sent for unknown commands."],
        unverified: true,
    },
    KeyComment {
        path: "messages.server-full",
        lines: &["Sent when the server is full."],
        unverified: false,
    },
    KeyComment {
        path: "messages.outdated-client",
        lines: &["Sent to outdated clients; `{0}` is replaced by the server version."],
        unverified: false,
    },
    KeyComment {
        path: "messages.outdated-server",
        lines: &["Sent to newer clients; `{0}` is replaced by the server version."],
        unverified: false,
    },
    KeyComment {
        path: "messages.restart",
        lines: &["Sent while the server restarts."],
        unverified: false,
    },
    KeyComment {
        path: "advancements",
        lines: &["Advancement saving."],
        unverified: false,
    },
    KeyComment {
        path: "advancements.disable-saving",
        lines: &["Disable advancement saving."],
        unverified: false,
    },
    KeyComment {
        path: "advancements.disabled",
        lines: &["Advancements to disable."],
        unverified: false,
    },
    KeyComment {
        path: "stats",
        lines: &["Statistics saving."],
        unverified: false,
    },
    KeyComment {
        path: "stats.disable-saving",
        lines: &["Disable statistics saving."],
        unverified: false,
    },
    KeyComment {
        path: "stats.forced-stats",
        lines: &["Forced statistic values."],
        unverified: false,
    },
    KeyComment {
        path: "commands",
        lines: &["Command handling."],
        unverified: false,
    },
    KeyComment {
        path: "commands.silent-commandblock-console",
        lines: &["Silence command block console output."],
        unverified: false,
    },
    KeyComment {
        path: "commands.spam-exclusions",
        lines: &["Commands excluded from spam checks."],
        unverified: false,
    },
    KeyComment {
        path: "commands.tab-complete",
        lines: &["Tab-completion mode."],
        unverified: false,
    },
    KeyComment {
        path: "commands.send-namespaced",
        lines: &["Send namespaced command names."],
        unverified: false,
    },
    KeyComment {
        path: "commands.log",
        lines: &["Log commands to the console."],
        unverified: false,
    },
    KeyComment {
        path: "players",
        lines: &["Player data handling."],
        unverified: false,
    },
    KeyComment {
        path: "players.disable-saving",
        lines: &["Disable player data saving."],
        unverified: false,
    },
    KeyComment {
        path: "world-settings",
        lines: &["Per-world settings. `default` applies to worlds without an entry."],
        unverified: false,
    },
    KeyComment {
        path: "world-settings.default",
        lines: &["Settings inherited by every world."],
        unverified: false,
    },
    KeyComment {
        path: "world-settings.default.view-distance",
        lines: &["View distance for this world."],
        unverified: false,
    },
    KeyComment {
        path: "world-settings.default.simulation-distance",
        lines: &["Simulation distance for this world."],
        unverified: true,
    },
    KeyComment {
        path: "world-settings.default.item-despawn-rate",
        lines: &["Ticks before dropped items despawn."],
        unverified: false,
    },
    KeyComment {
        path: "world-settings.default.merge-radius",
        lines: &["Item and experience merge radii."],
        unverified: false,
    },
    KeyComment {
        path: "world-settings.default.hopper-transfer",
        lines: &["Ticks a hopper waits between item transfers."],
        unverified: false,
    },
    KeyComment {
        path: "world-settings.default.hopper-check",
        lines: &["Ticks between hopper item checks."],
        unverified: false,
    },
    KeyComment {
        path: "world-settings.default.max-entity-collisions",
        lines: &["Maximum entities colliding with a single entity."],
        unverified: false,
    },
    KeyComment {
        path: "world-settings.default.max-tnt-per-tick",
        lines: &["Maximum TNT entities primed per tick."],
        unverified: false,
    },
    KeyComment {
        path: "world-settings.default.nerf-spawner-mobs",
        lines: &["Remove AI from mobs spawned by spawners."],
        unverified: false,
    },
    KeyComment {
        path: "world-settings.default.entity-activation-range",
        lines: &["Entity activation ranges."],
        unverified: false,
    },
    KeyComment {
        path: "world-settings.default.entity-tracking-range",
        lines: &["Entity tracking ranges."],
        unverified: false,
    },
];

impl YamlConfig for SpigotConfig {
    const KIND: ConfigFileKind = ConfigFileKind::Spigot;

    fn key_comments() -> &'static [KeyComment] {
        COMMENTS
    }
}
