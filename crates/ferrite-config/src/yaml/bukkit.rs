//! `bukkit.yml` — the base Bukkit configuration.

use serde::{Deserialize, Serialize};

use super::{ConfigFileKind, KeyComment, YamlConfig};

/// `bukkit.yml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct BukkitConfig {
    /// Global settings.
    pub settings: BukkitSettings,
    /// Per-world mob caps.
    pub spawn_limits: SpawnLimits,
    /// Chunk garbage collector settings.
    pub chunk_gc: ChunkGc,
    /// Tick intervals for background tasks.
    pub ticks_per: TicksPer,
    /// Legacy aliases file name.
    ///
    /// UNVERIFIED: current Bukkit writes `now-in-commands.yml`; older servers used
    /// `commands.yml`.
    pub aliases: String,
}

impl Default for BukkitConfig {
    fn default() -> Self {
        Self {
            settings: BukkitSettings::default(),
            spawn_limits: SpawnLimits::default(),
            chunk_gc: ChunkGc::default(),
            ticks_per: TicksPer::default(),
            aliases: "now-in-commands.yml".to_string(),
        }
    }
}

/// `bukkit.yml` `settings` section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct BukkitSettings {
    /// Whether the end dimension is enabled.
    pub allow_end: bool,
    /// Warn when a single tick takes too long.
    pub warn_on_overload: bool,
    /// Permissions file.
    pub permissions_file: String,
    /// Minimum milliseconds between connection attempts from one address.
    pub connection_throttle: i64,
    /// Verbosity for deprecated API warnings (`default`, `true`, or `false`).
    ///
    /// UNVERIFIED: Bukkit accepts three different forms, so both booleans and strings are
    /// accepted when reading.
    pub deprecated_verbose: DeprecatedVerbose,
    /// Message sent during shutdown.
    pub shutdown_message: String,
    /// Cache map colors.
    ///
    /// UNVERIFIED: recent Bukkit setting, default `true`.
    pub use_map_color_cache: bool,
}

impl Default for BukkitSettings {
    fn default() -> Self {
        Self {
            allow_end: true,
            warn_on_overload: true,
            permissions_file: "permissions.yml".to_string(),
            connection_throttle: 4000,
            deprecated_verbose: DeprecatedVerbose::default(),
            shutdown_message: "Server closed".to_string(),
            use_map_color_cache: true,
        }
    }
}

/// `settings.deprecated-verbose`, which Bukkit accepts as a boolean or a keyword.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DeprecatedVerbose {
    /// `true` or `false`.
    Bool(bool),
    /// `default` (and future keywords).
    Keyword(String),
}

impl Default for DeprecatedVerbose {
    fn default() -> Self {
        Self::Keyword("default".to_string())
    }
}

/// `bukkit.yml` `spawn-limits` section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct SpawnLimits {
    /// Monsters per player.
    pub monsters: i32,
    /// Animals per player.
    pub animals: i32,
    /// Water animals per player.
    pub water_animals: i32,
    /// Ambient water mobs per player.
    pub water_ambient: i32,
    /// Underground water creatures per player.
    pub water_underground_creature: i32,
    /// Axolotls per player.
    pub axolotls: i32,
    /// Ambient mobs per player.
    pub ambient: i32,
}

impl Default for SpawnLimits {
    fn default() -> Self {
        Self {
            monsters: 70,
            animals: 10,
            water_animals: 5,
            water_ambient: 20,
            water_underground_creature: 5,
            axolotls: 5,
            ambient: 15,
        }
    }
}

/// `bukkit.yml` `chunk-gc` section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct ChunkGc {
    /// Ticks between chunk unload passes.
    pub period_in_ticks: i64,
}

impl Default for ChunkGc {
    fn default() -> Self {
        Self {
            period_in_ticks: 600,
        }
    }
}

/// `bukkit.yml` `ticks-per` section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct TicksPer {
    /// Ticks between animal spawn attempts.
    pub animal_spawns: i64,
    /// Ticks between monster spawn attempts.
    pub monster_spawns: i64,
    /// Ticks between water creature spawn attempts.
    pub water_spawns: i64,
    /// Ticks between ambient water spawn attempts.
    pub water_ambient_spawns: i64,
    /// Ticks between underground water creature spawn attempts.
    pub water_underground_creature_spawns: i64,
    /// Ticks between axolotl spawn attempts.
    pub axolotl_spawns: i64,
    /// Ticks between ambient mob spawn attempts.
    pub ambient_spawns: i64,
    /// Ticks between player data saves.
    pub autosave: i64,
}

impl Default for TicksPer {
    fn default() -> Self {
        Self {
            animal_spawns: 400,
            monster_spawns: 1,
            water_spawns: 1,
            water_ambient_spawns: 1,
            water_underground_creature_spawns: 1,
            axolotl_spawns: 1,
            ambient_spawns: 1,
            autosave: 6000,
        }
    }
}

const COMMENTS: &[KeyComment] = &[
    KeyComment {
        path: "settings",
        lines: &["General server settings."],
        unverified: false,
    },
    KeyComment {
        path: "settings.allow-end",
        lines: &["Whether the end dimension is enabled."],
        unverified: false,
    },
    KeyComment {
        path: "settings.warn-on-overload",
        lines: &["Warn in the console when the server cannot keep up."],
        unverified: false,
    },
    KeyComment {
        path: "settings.permissions-file",
        lines: &["Permissions file used by the built-in permission system."],
        unverified: false,
    },
    KeyComment {
        path: "settings.connection-throttle",
        lines: &[
            "Minimum milliseconds between connection attempts from the same address.",
            "Set to -1 to disable.",
        ],
        unverified: false,
    },
    KeyComment {
        path: "settings.deprecated-verbose",
        lines: &["Deprecated API warning verbosity: `default`, `true`, or `false`."],
        unverified: true,
    },
    KeyComment {
        path: "settings.shutdown-message",
        lines: &["Message sent to players during shutdown."],
        unverified: false,
    },
    KeyComment {
        path: "settings.use-map-color-cache",
        lines: &["Cache map colors."],
        unverified: true,
    },
    KeyComment {
        path: "spawn-limits",
        lines: &["Mob spawn caps per player."],
        unverified: false,
    },
    KeyComment {
        path: "spawn-limits.monsters",
        lines: &["Monsters per player."],
        unverified: false,
    },
    KeyComment {
        path: "spawn-limits.animals",
        lines: &["Animals per player."],
        unverified: false,
    },
    KeyComment {
        path: "spawn-limits.water-animals",
        lines: &["Water animals per player."],
        unverified: false,
    },
    KeyComment {
        path: "spawn-limits.water-ambient",
        lines: &["Ambient water mobs per player."],
        unverified: false,
    },
    KeyComment {
        path: "spawn-limits.water-underground-creature",
        lines: &["Underground water creatures per player."],
        unverified: false,
    },
    KeyComment {
        path: "spawn-limits.axolotls",
        lines: &["Axolotls per player."],
        unverified: false,
    },
    KeyComment {
        path: "spawn-limits.ambient",
        lines: &["Ambient mobs per player."],
        unverified: false,
    },
    KeyComment {
        path: "chunk-gc",
        lines: &["Chunk garbage collection."],
        unverified: false,
    },
    KeyComment {
        path: "chunk-gc.period-in-ticks",
        lines: &["Ticks between chunk unload passes."],
        unverified: false,
    },
    KeyComment {
        path: "ticks-per",
        lines: &["Tick intervals for background tasks."],
        unverified: false,
    },
    KeyComment {
        path: "ticks-per.animal-spawns",
        lines: &["Ticks between animal spawn attempts."],
        unverified: false,
    },
    KeyComment {
        path: "ticks-per.monster-spawns",
        lines: &["Ticks between monster spawn attempts."],
        unverified: false,
    },
    KeyComment {
        path: "ticks-per.water-spawns",
        lines: &["Ticks between water creature spawn attempts."],
        unverified: false,
    },
    KeyComment {
        path: "ticks-per.water-ambient-spawns",
        lines: &["Ticks between ambient water spawn attempts."],
        unverified: false,
    },
    KeyComment {
        path: "ticks-per.water-underground-creature-spawns",
        lines: &["Ticks between underground water creature spawn attempts."],
        unverified: false,
    },
    KeyComment {
        path: "ticks-per.axolotl-spawns",
        lines: &["Ticks between axolotl spawn attempts."],
        unverified: false,
    },
    KeyComment {
        path: "ticks-per.ambient-spawns",
        lines: &["Ticks between ambient mob spawn attempts."],
        unverified: false,
    },
    KeyComment {
        path: "ticks-per.autosave",
        lines: &["Ticks between player data saves."],
        unverified: false,
    },
    KeyComment {
        path: "aliases",
        lines: &["Legacy command aliases file."],
        unverified: true,
    },
];

impl YamlConfig for BukkitConfig {
    const KIND: ConfigFileKind = ConfigFileKind::Bukkit;

    fn key_comments() -> &'static [KeyComment] {
        COMMENTS
    }
}
