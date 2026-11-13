//! Registry data loot functions need while rolling: enchantments, smelting recipes and the tags
//! of the registries the shipped loot tables mention (enchantment, entity type, damage type,
//! instrument, structure).
//!
//! Everything is read once by [`crate::LootTables::load`] from the sibling directories of the
//! loot tables (`enchantment/`, `recipe/`, `tags/<registry>/`) and resolved, so rolling never
//! touches the filesystem. A pack that lacks a directory simply has none of that data.

use crate::files::{json_files, read_json, relative_stem};
use crate::tags::ItemTags;
use crate::{Result, normalize_id};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

/// A cost curve: `base + per_level_above_first * (level - 1)`.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Cost {
    pub(crate) base: i32,
    pub(crate) per_level: i32,
}

impl Cost {
    pub(crate) fn at(self, level: i32) -> i32 {
        self.base + self.per_level * (level - 1)
    }
}

/// The parts of a vanilla enchantment definition loot functions read.
#[derive(Debug, Clone)]
pub(crate) struct Enchantment {
    pub(crate) id: String,
    pub(crate) weight: i32,
    pub(crate) max_level: i32,
    pub(crate) min_cost: Cost,
    pub(crate) max_cost: Cost,
    /// Items the enchantment can be applied to (`supported_items`).
    pub(crate) supported: Vec<String>,
    /// Items the enchanting table offers it for (`primary_items`, defaulting to `supported`).
    pub(crate) primary: Vec<String>,
    /// Enchantments it cannot coexist with (`exclusive_set`).
    pub(crate) exclusive: Vec<String>,
}

/// One smelting recipe, reduced to what `furnace_smelt` needs.
#[derive(Debug, Clone)]
pub(crate) struct Smelting {
    pub(crate) inputs: Vec<String>,
    pub(crate) result: String,
    pub(crate) count: i32,
}

/// Registry data shared by every function of one loaded pack.
#[derive(Debug, Default)]
pub struct GameData {
    pub(crate) item_tags: ItemTags,
    pub(crate) enchantments: Vec<Enchantment>,
    pub(crate) smelting: Vec<Smelting>,
    /// `tags/<registry>` per registry name (`enchantment`, `entity_type`, `damage_type`, ...).
    pub(crate) tags: BTreeMap<String, ItemTags>,
}

impl GameData {
    pub(crate) fn load(base: &Path, namespace: &str, item_tags: ItemTags) -> Result<Self> {
        let mut tags = BTreeMap::new();
        for registry in [
            "enchantment",
            "entity_type",
            "damage_type",
            "instrument",
            "potion",
            "worldgen/structure",
            "worldgen/biome",
            "block",
        ] {
            tags.insert(
                registry.to_owned(),
                ItemTags::load(&base.join("tags").join(registry), namespace)?,
            );
        }
        let mut data = Self {
            item_tags,
            tags,
            ..Self::default()
        };
        data.enchantments = load_enchantments(&base.join("enchantment"), namespace, &data)?;
        data.smelting = load_smelting(&base.join("recipe"), &data)?;
        Ok(data)
    }

    /// The members of a `#tag`, a plain id or an array of either, resolved against `registry`.
    /// Unknown tags resolve to nothing.
    pub(crate) fn resolve_ids(&self, registry: &str, value: &Value) -> Vec<String> {
        match value {
            Value::String(text) => self.resolve_one(registry, text),
            Value::Array(values) => values
                .iter()
                .filter_map(Value::as_str)
                .flat_map(|text| self.resolve_one(registry, text))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn resolve_one(&self, registry: &str, text: &str) -> Vec<String> {
        match text.strip_prefix('#') {
            Some(tag) => self
                .registry_tags(registry)
                .and_then(|tags| tags.items(tag))
                .map(<[String]>::to_vec)
                .unwrap_or_default(),
            None => vec![normalize_id(text)],
        }
    }

    fn registry_tags(&self, registry: &str) -> Option<&ItemTags> {
        if registry == "item" {
            Some(&self.item_tags)
        } else {
            self.tags.get(registry)
        }
    }

    /// Whether `id` is in the tag `tag` of `registry` (`tag` without the `#`).
    pub(crate) fn in_tag(&self, registry: &str, tag: &str, id: &str) -> bool {
        let id = normalize_id(id);
        self.registry_tags(registry)
            .and_then(|tags| tags.items(tag))
            .is_some_and(|items| items.contains(&id))
    }

    /// The smelting result for `item`, `(result item, result count)`.
    pub(crate) fn smelt(&self, item: &str) -> Option<(&str, i32)> {
        let item = normalize_id(item);
        self.smelting
            .iter()
            .find(|recipe| recipe.inputs.contains(&item))
            .map(|recipe| (recipe.result.as_str(), recipe.count))
    }
}

fn cost(value: Option<&Value>) -> Cost {
    let field = |name: &str| {
        value
            .and_then(|value| value.get(name))
            .and_then(Value::as_i64)
            .unwrap_or(0) as i32
    };
    Cost {
        base: field("base"),
        per_level: field("per_level_above_first"),
    }
}

fn load_enchantments(dir: &Path, namespace: &str, data: &GameData) -> Result<Vec<Enchantment>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for path in json_files(dir)? {
        let Some(relative) = relative_stem(&path, dir) else {
            continue;
        };
        let value = read_json(&path)?;
        let items = |key: &str| {
            value
                .get(key)
                .map(|value| data.resolve_ids("item", value))
                .unwrap_or_default()
        };
        let supported = items("supported_items");
        let primary = if value.get("primary_items").is_some() {
            items("primary_items")
        } else {
            supported.clone()
        };
        let exclusive = value
            .get("exclusive_set")
            .map(|value| data.resolve_ids("enchantment", value))
            .unwrap_or_default();
        out.push(Enchantment {
            id: format!("{namespace}:{relative}"),
            weight: value.get("weight").and_then(Value::as_i64).unwrap_or(1) as i32,
            max_level: value.get("max_level").and_then(Value::as_i64).unwrap_or(1) as i32,
            min_cost: cost(value.get("min_cost")),
            max_cost: cost(value.get("max_cost")),
            supported,
            primary,
            exclusive,
        });
    }
    Ok(out)
}

fn load_smelting(dir: &Path, data: &GameData) -> Result<Vec<Smelting>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for path in json_files(dir)? {
        let value = read_json(&path)?;
        if value.get("type").and_then(Value::as_str) != Some("minecraft:smelting") {
            continue;
        }
        let Some(ingredient) = value.get("ingredient") else {
            continue;
        };
        let (result, count) = match value.get("result") {
            Some(Value::String(id)) => (normalize_id(id), 1),
            Some(object) => {
                let Some(id) = object.get("id").and_then(Value::as_str) else {
                    continue;
                };
                (
                    normalize_id(id),
                    object.get("count").and_then(Value::as_i64).unwrap_or(1) as i32,
                )
            }
            None => continue,
        };
        out.push(Smelting {
            inputs: data.resolve_ids("item", ingredient),
            result,
            count,
        });
    }
    Ok(out)
}
