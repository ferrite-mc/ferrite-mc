//! Vanilla Minecraft Java 26.3 loot tables: loading and evaluation.
//!
//! [`LootTables::load`] reads every `*.json` below a generated
//! `data/<namespace>/loot_table` directory, together with the sibling
//! `predicate` and `tags/item` directories that 26.3 loot data references.
//! Tables can reference other tables (`minecraft:loot_table` entries,
//! including inline table objects), so references are resolved once while
//! loading and the resulting [`LootTable`] is self-contained.
//!
//! Evaluation walks pools, entries, conditions and functions and returns plain
//! `(item id, count)` stacks, in the order vanilla would add them. Components,
//! enchantments, names and other item metadata are outside this crate's model,
//! so functions that only touch that metadata (`set_components`, `set_name`,
//! `copy_components`, `enchant_randomly`, ...) are kept opaque and pass an item
//! through unchanged. Conditions this crate cannot evaluate (such as
//! `entity_properties`) are likewise kept opaque and treated as satisfied, so a
//! table still drops its unconditioned items under a partial context.
//!
//! Item ids returned by [`LootTable::roll`] are in their namespaced form
//! (`minecraft:cobblestone`). Ids supplied by callers, such as
//! [`LootContext::tool`], may omit the namespace.
//!
//! ```
//! use ferrite_loot::{LootContext, LootTables, Rng};
//! use std::path::Path;
//!
//! struct StepRng(u64);
//!
//! impl Rng for StepRng {
//!     fn next_int(&mut self, bound: i32) -> i32 {
//!         self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
//!         ((self.0 >> 33) % bound.max(1) as u64) as i32
//!     }
//!
//!     fn next_float(&mut self) -> f32 {
//!         self.next_int(1 << 24) as f32 / (1 << 24) as f32
//!     }
//! }
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let dir = Path::new("vanilla/26.3/generated/data/minecraft/loot_table");
//! if dir.is_dir() {
//!     let tables = LootTables::load(dir)?;
//!     let stone = tables.get("minecraft:blocks/stone").expect("stone table");
//!     let drops = stone.roll(&LootContext::default(), &mut StepRng(1));
//!     assert_eq!(drops, [("minecraft:cobblestone".to_owned(), 1)]);
//! }
//! # Ok(())
//! # }
//! ```

mod condition;
mod entry;
mod files;
mod function;
mod number;
mod table;
mod tags;

use serde_json::Value;
use std::collections::BTreeMap;

pub use condition::{BlockPredicate, Condition, EnchantmentPredicate, ToolPredicate};
pub use entry::{Entry, EntryKind, LootPool};
pub use function::{BonusFormula, Function, FunctionKind};
pub use number::{IntRange, NumberProvider};
pub use table::{LootTable, LootTables};

/// Errors produced while loading loot data.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A directory or file could not be read.
    #[error("failed to read {path}: {source}")]
    Io {
        /// Path that could not be read.
        path: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// A JSON file was malformed.
    #[error("failed to parse {path}: {source}")]
    Json {
        /// Path that could not be parsed.
        path: String,
        /// Underlying JSON error.
        #[source]
        source: serde_json::Error,
    },
    /// Loot tables reference each other in a cycle.
    #[error("cyclic loot table reference: {0}")]
    Cycle(String),
}

impl Error {
    pub(crate) fn io(path: &std::path::Path, source: std::io::Error) -> Self {
        Self::Io {
            path: path.display().to_string(),
            source,
        }
    }

    pub(crate) fn json(path: &std::path::Path, source: serde_json::Error) -> Self {
        Self::Json {
            path: path.display().to_string(),
            source,
        }
    }
}

/// Result type used while loading loot data.
pub type Result<T> = std::result::Result<T, Error>;

/// The random source loot evaluation draws from.
///
/// Implement this over the server's own random so loot rolls can use whatever source the
/// caller prefers without this crate depending on it.
pub trait Rng {
    /// A uniform integer in `0..bound`. Evaluation only calls this with a
    /// positive `bound`.
    fn next_int(&mut self, bound: i32) -> i32;

    /// A uniform float in `[0, 1)`.
    fn next_float(&mut self) -> f32;
}

/// Everything known about the drop being rolled.
///
/// Loot tables inspect only the part of the context they need; fields left at
/// their [`Default`] values make the corresponding conditions behave as if the
/// information were unavailable.
#[derive(Debug, Clone, Default)]
pub struct LootContext {
    /// Item id of the tool that broke the block or killed the entity, if any.
    pub tool: Option<String>,
    /// Enchantment levels on the tool, keyed by enchantment id.
    ///
    /// Fortune and looting have dedicated fields; this map is what
    /// `match_tool` predicates (such as vanilla's `tool/can_silk_touch`) read.
    pub tool_enchantments: BTreeMap<String, i32>,
    /// Fortune level used by `apply_bonus` and `table_bonus`.
    pub fortune: i32,
    /// Looting level used by `apply_bonus` and `table_bonus`.
    pub looting: i32,
    /// Radius of the explosion the block was destroyed by, if any.
    pub explosion_radius: Option<f32>,
    /// Block id being broken, if known. Required by block id conditions.
    pub block: Option<String>,
    /// Block state properties, mapped to their string values.
    pub block_state: BTreeMap<String, String>,
    /// Whether the entity was killed by a player.
    pub killed_by_player: bool,
}

impl LootContext {
    /// Level of `enchantment` relevant to loot functions and conditions.
    pub(crate) fn enchantment_level(&self, enchantment: &str) -> i32 {
        match normalize_id(enchantment).as_str() {
            "minecraft:fortune" => self.fortune,
            "minecraft:looting" => self.looting,
            _ => self.tool_enchantment_level(enchantment),
        }
    }

    /// Level of `enchantment` on the tool, ignoring the fortune/looting fields.
    pub(crate) fn tool_enchantment_level(&self, enchantment: &str) -> i32 {
        let wanted = normalize_id(enchantment);
        self.tool_enchantments
            .iter()
            .find(|(name, _)| normalize_id(name) == wanted)
            .map_or(0, |(_, level)| *level)
    }
}

/// Returns `id` with an explicit namespace, defaulting to `minecraft`.
pub(crate) fn normalize_id(id: &str) -> String {
    if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    }
}

/// Read-only data shared by every table while loading.
pub(crate) struct Definitions {
    /// Namespace of the loaded data pack (`minecraft` for vanilla).
    pub(crate) namespace: String,
    /// Raw predicate files (`data/<namespace>/predicate/**`), keyed by id.
    pub(crate) predicates: BTreeMap<String, Value>,
    /// Resolved item tags (`data/<namespace>/tags/item/**`).
    pub(crate) tags: tags::ItemTags,
}

impl Definitions {
    /// Qualifies an id with the pack namespace when it has none.
    pub(crate) fn qualify(&self, id: &str) -> String {
        if id.contains(':') {
            id.to_owned()
        } else {
            format!("{}:{id}", self.namespace)
        }
    }
}
