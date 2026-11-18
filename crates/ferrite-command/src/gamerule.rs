//! Vanilla 26.3 game rules: names, types, defaults, bounds and world persistence.
//!
//! The names, argument types and integer bounds come from the generated command tree
//! (`vanilla/26.3/generated/reports/commands.json`, the `gamerule` node, which accepts
//! both `random_tick_speed` and `minecraft:random_tick_speed`). Defaults are the ones
//! the 26.3 server's `GameRules` class registers, and the registry ids match
//! `reports/registries.json` under `minecraft:game_rule`. `tests/gamerule.rs` re-reads
//! both reports so this table cannot drift silently.
//!
//! 26.3 stores the live values in `data/minecraft/game_rules.dat` (a `SavedData` with
//! `{data: {"minecraft:<rule>": <int>}, DataVersion: <int>}`), not in `level.dat`.
//! [`GameRules::from_level_dat`] still reads the old `Data.GameRules` compound so worlds
//! created by older versions keep their rules.
use ferrite_nbt::{Compound, Tag};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `DataVersion` written when the world has no `level.dat` to copy the version from.
pub const DATA_VERSION: i32 = 5023;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleValue {
    Boolean(bool),
    Integer(i32),
}

impl RuleValue {
    pub fn as_bool(self) -> Option<bool> {
        match self {
            Self::Boolean(value) => Some(value),
            Self::Integer(_) => None,
        }
    }

    pub fn as_integer(self) -> Option<i32> {
        match self {
            Self::Boolean(_) => None,
            Self::Integer(value) => Some(value),
        }
    }

    pub fn text(self) -> String {
        match self {
            Self::Boolean(value) => value.to_string(),
            Self::Integer(value) => value.to_string(),
        }
    }

    fn raw(self) -> i32 {
        match self {
            Self::Boolean(value) => i32::from(value),
            Self::Integer(value) => value,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleKind {
    Boolean,
    Integer { min: i32, max: i32 },
}

#[derive(Debug, Clone, Copy)]
pub struct RuleSpec {
    pub name: &'static str,
    pub default: RuleValue,
    pub kind: RuleKind,
}

const fn boolean(name: &'static str, default: bool) -> RuleSpec {
    RuleSpec {
        name,
        default: RuleValue::Boolean(default),
        kind: RuleKind::Boolean,
    }
}

const fn integer(name: &'static str, default: i32, min: i32, max: i32) -> RuleSpec {
    RuleSpec {
        name,
        default: RuleValue::Integer(default),
        kind: RuleKind::Integer { min, max },
    }
}

/// Every rule in the 26.3 `minecraft:game_rule` registry, in command-tree order.
pub const SPECS: &[RuleSpec] = &[
    boolean("advance_time", true),
    boolean("advance_weather", true),
    boolean("allow_entering_nether_using_portals", true),
    boolean("block_drops", true),
    boolean("block_explosion_drop_decay", true),
    boolean("command_block_output", true),
    boolean("command_blocks_work", true),
    boolean("drowning_damage", true),
    boolean("elytra_movement_check", true),
    boolean("ender_pearls_vanish_on_death", true),
    boolean("entity_drops", true),
    boolean("fall_damage", true),
    boolean("fire_damage", true),
    integer("fire_spread_radius_around_player", 128, -1, i32::MAX),
    boolean("forgive_dead_players", true),
    boolean("freeze_damage", true),
    boolean("global_sound_events", true),
    boolean("immediate_respawn", false),
    boolean("keep_inventory", false),
    boolean("lava_source_conversion", false),
    boolean("limited_crafting", false),
    boolean("locator_bar", true),
    boolean("log_admin_commands", true),
    integer("max_block_modifications", 32768, 1, i32::MAX),
    integer("max_command_forks", 65536, 0, i32::MAX),
    integer("max_command_sequence_length", 65536, 0, i32::MAX),
    integer("max_entity_cramming", 24, 0, i32::MAX),
    integer("max_minecart_speed", 8, 1, 1000),
    integer("max_snow_accumulation_height", 1, 0, 8),
    boolean("mob_drops", true),
    boolean("mob_explosion_drop_decay", true),
    boolean("mob_griefing", true),
    boolean("natural_health_regeneration", true),
    boolean("player_movement_check", true),
    integer("players_nether_portal_creative_delay", 0, 0, i32::MAX),
    integer("players_nether_portal_default_delay", 80, 0, i32::MAX),
    integer("players_sleeping_percentage", 100, 0, i32::MAX),
    boolean("projectiles_can_break_blocks", true),
    boolean("pvp", true),
    boolean("raids", true),
    integer("random_tick_speed", 3, 0, i32::MAX),
    boolean("reduced_debug_info", false),
    integer("respawn_radius", 10, 0, i32::MAX),
    boolean("send_command_feedback", true),
    boolean("show_advancement_messages", true),
    boolean("show_death_messages", true),
    boolean("spawner_blocks_work", true),
    boolean("spawn_mobs", true),
    boolean("spawn_monsters", true),
    boolean("spawn_patrols", true),
    boolean("spawn_phantoms", true),
    boolean("spawn_wandering_traders", true),
    boolean("spawn_wardens", true),
    boolean("spectators_generate_chunks", true),
    boolean("spread_vines", true),
    boolean("tnt_explodes", true),
    boolean("tnt_explosion_drop_decay", false),
    boolean("universal_anger", false),
    boolean("water_source_conversion", true),
];

/// Names older worlds and console users still type. The snake_case form is the
/// 26.3 command name; aliases are registered as extra literals that map onto it.
pub const ALIASES: &[(&str, &str)] = &[
    ("doDaylightCycle", "advance_time"),
    ("doWeatherCycle", "advance_weather"),
    ("doMobSpawning", "spawn_mobs"),
    ("doMobLoot", "mob_drops"),
    ("doTileDrops", "block_drops"),
    ("doEntityDrops", "entity_drops"),
    ("naturalRegeneration", "natural_health_regeneration"),
    ("announceAdvancements", "show_advancement_messages"),
    ("maxCommandChainLength", "max_command_sequence_length"),
    ("spawnRadius", "respawn_radius"),
    ("doInsomnia", "spawn_phantoms"),
    ("doPatrolSpawning", "spawn_patrols"),
    ("doTraderSpawning", "spawn_wandering_traders"),
    ("doWardenSpawning", "spawn_wardens"),
    ("doVinesSpread", "spread_vines"),
    ("doImmediateRespawn", "immediate_respawn"),
    ("mobGriefing", "mob_griefing"),
    ("doLimitedCrafting", "limited_crafting"),
];

/// The canonical snake_case name, accepting `minecraft:` ids, legacy camelCase names and
/// the alias list above.
pub fn canonical_name(name: &str) -> String {
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    if let Some((_, canonical)) = ALIASES.iter().find(|(alias, _)| *alias == name) {
        return (*canonical).to_owned();
    }
    let mut out = String::with_capacity(name.len() + 4);
    for ch in name.chars() {
        if ch.is_ascii_uppercase() {
            out.push('_');
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

pub fn spec(name: &str) -> Option<&'static RuleSpec> {
    SPECS.iter().find(|spec| spec.name == name)
}

pub fn specs() -> &'static [RuleSpec] {
    SPECS
}

pub fn aliases() -> &'static [(&'static str, &'static str)] {
    ALIASES
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameRules {
    values: BTreeMap<String, RuleValue>,
}

impl Default for GameRules {
    fn default() -> Self {
        Self {
            values: SPECS
                .iter()
                .map(|spec| (spec.name.to_owned(), spec.default))
                .collect(),
        }
    }
}

impl GameRules {
    pub fn specs() -> &'static [RuleSpec] {
        SPECS
    }

    pub fn get(&self, name: &str) -> Option<RuleValue> {
        self.values.get(&canonical_name(name)).copied()
    }

    pub fn get_bool(&self, name: &str) -> Option<bool> {
        self.get(name)?.as_bool()
    }

    pub fn get_integer(&self, name: &str) -> Option<i32> {
        self.get(name)?.as_integer()
    }

    pub fn set(&mut self, name: &str, value: &str) -> Result<RuleValue, String> {
        let name = canonical_name(name);
        let spec = spec(&name).ok_or_else(|| format!("Unknown game rule: {name}"))?;
        let parsed = match spec.kind {
            RuleKind::Boolean => RuleValue::Boolean(
                value
                    .parse::<bool>()
                    .map_err(|_| "Expected true or false".to_owned())?,
            ),
            RuleKind::Integer { min, max } => {
                let parsed = value
                    .parse::<i32>()
                    .map_err(|_| "Expected an integer".to_owned())?;
                if parsed < min || parsed > max {
                    return Err(format!("Value must be between {min} and {max}"));
                }
                RuleValue::Integer(parsed)
            }
        };
        self.values.insert(name, parsed);
        Ok(parsed)
    }

    /// Reads the 26.3 `data/minecraft/game_rules.dat` payload. Missing rules keep their
    /// defaults, so a partial file from an older snapshot still loads.
    pub fn from_nbt(tag: &Tag) -> Option<Self> {
        let Tag::Compound(root) = tag else {
            return None;
        };
        let Tag::Compound(data) = root.get("data")? else {
            return None;
        };
        let mut rules = Self::default();
        for (key, value) in data {
            let name = canonical_name(key);
            let Some(spec) = spec(&name) else {
                continue;
            };
            let raw = match value {
                Tag::Byte(value) => i32::from(*value),
                Tag::Short(value) => i32::from(*value),
                Tag::Int(value) => *value,
                _ => continue,
            };
            let value = match spec.kind {
                RuleKind::Boolean => RuleValue::Boolean(raw != 0),
                RuleKind::Integer { .. } => RuleValue::Integer(raw),
            };
            rules.values.insert(name, value);
        }
        Some(rules)
    }

    /// Reads the pre-26.3 `Data.GameRules` compound in `level.dat` (string values).
    pub fn from_level_dat(tag: &Tag) -> Option<Self> {
        let Tag::Compound(root) = tag else {
            return None;
        };
        let Tag::Compound(data) = root.get("Data")? else {
            return None;
        };
        let Tag::Compound(stored) = data.get("GameRules")? else {
            return None;
        };
        let mut rules = Self::default();
        for (key, value) in stored {
            let Tag::String(text) = value else {
                continue;
            };
            let name = canonical_name(key);
            let Some(spec) = spec(&name) else {
                continue;
            };
            let parsed = match spec.kind {
                RuleKind::Boolean => text.parse::<bool>().ok().map(RuleValue::Boolean),
                RuleKind::Integer { min, max } => text
                    .parse::<i32>()
                    .ok()
                    .filter(|value| *value >= min && *value <= max)
                    .map(RuleValue::Integer),
            };
            if let Some(value) = parsed {
                rules.values.insert(name, value);
            }
        }
        Some(rules)
    }

    /// Encodes the 26.3 world data file: every registered rule, including defaults.
    pub fn to_nbt(&self, data_version: i32) -> Tag {
        let mut data = Compound::new();
        for spec in SPECS {
            let value = self.values.get(spec.name).copied().unwrap_or(spec.default);
            data.insert(format!("minecraft:{}", spec.name), Tag::Int(value.raw()));
        }
        let mut root = Compound::new();
        root.insert("data".into(), Tag::Compound(data));
        root.insert("DataVersion".into(), Tag::Int(data_version));
        Tag::Compound(root)
    }
}
