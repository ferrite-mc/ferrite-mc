//! Unlocks recipes from play, like the `recipes/**` advancements do: a recipe comes into the
//! book once the player holds an item its advancement asks for (`inventory_changed`), or already
//! has the recipe (`recipe_unlocked`). Counts, components and the other triggers (`enter_block`,
//! `tick`) are not evaluated. The rules are read from the extracted advancement data.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use uuid::Uuid;

/// Item names each player holds, refreshed by the region threads on `Command::ScanInventory`.
pub static HELD: Mutex<BTreeMap<Uuid, BTreeSet<String>>> = Mutex::new(BTreeMap::new());

#[derive(Debug, Clone)]
pub enum Criterion {
    /// Every entry must match a held item; an entry is `(names, is_tag)` alternatives.
    Holds(Vec<Vec<String>>),
    Recipe(String),
    Other,
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub id: String,
    pub recipes: Vec<String>,
    pub criteria: BTreeMap<String, Criterion>,
    pub requirements: Vec<Vec<String>>,
}

fn alternatives(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::String(s) => vec![s.clone()],
        serde_json::Value::Array(a) => a
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

pub fn parse_rule(id: &str, json: &serde_json::Value) -> Option<Rule> {
    let recipes: Vec<String> = json["rewards"]["recipes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r.as_str().map(str::to_owned))
        .collect();
    let mut criteria = BTreeMap::new();
    for (name, c) in json["criteria"].as_object()? {
        let kind = match c["trigger"].as_str() {
            Some("minecraft:inventory_changed") => Criterion::Holds(
                c["conditions"]["items"]
                    .as_array()
                    .map(|a| a.iter().map(|e| alternatives(&e["items"])).collect())
                    .unwrap_or_default(),
            ),
            Some("minecraft:recipe_unlocked") => {
                Criterion::Recipe(c["conditions"]["recipes"].as_str()?.to_owned())
            }
            _ => Criterion::Other,
        };
        criteria.insert(name.clone(), kind);
    }
    let requirements = match json["requirements"].as_array() {
        Some(groups) => groups.iter().map(alternatives).collect(),
        None => criteria.keys().map(|k| vec![k.clone()]).collect(),
    };
    Some(Rule {
        id: id.to_owned(),
        recipes,
        criteria,
        requirements,
    })
}

fn matches(entry: &[String], held: &BTreeSet<String>) -> bool {
    entry.iter().any(|alt| match alt.strip_prefix('#') {
        Some(tag) => {
            let tag = tag.strip_prefix("minecraft:").unwrap_or(tag);
            held.iter().any(|item| {
                ferrite_data::tags::contains(
                    "item",
                    tag,
                    item.strip_prefix("minecraft:").unwrap_or(item),
                )
            })
        }
        None => held.contains(alt),
    })
}

/// Recipes the rule gives to a player holding `held` who already has `owned`.
pub fn unlocked(rule: &Rule, held: &BTreeSet<String>, owned: &BTreeSet<String>) -> bool {
    let met = |name: &String| match rule.criteria.get(name) {
        Some(Criterion::Holds(entries)) => {
            !entries.is_empty() && entries.iter().all(|e| matches(e, held))
        }
        Some(Criterion::Recipe(id)) => owned.contains(id),
        _ => false,
    };
    !rule.requirements.is_empty() && rule.requirements.iter().all(|g| g.iter().any(met))
}

fn rules() -> &'static Vec<Rule> {
    static ALL: OnceLock<Vec<Rule>> = OnceLock::new();
    ALL.get_or_init(|| {
        let relative = Path::new("vanilla/26.3/generated/data/minecraft/advancement");
        let Some(dir) = [
            relative.to_path_buf(),
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(relative),
        ]
        .into_iter()
        .find(|d| d.is_dir()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut stack = vec![dir.clone()];
        while let Some(current) = stack.pop() {
            for entry in std::fs::read_dir(current).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let Ok(rel) = path.strip_prefix(&dir) else {
                    continue;
                };
                let rel = rel.to_string_lossy().replace('\\', "/");
                let Some(stem) = rel.strip_suffix(".json") else {
                    continue;
                };
                let id = format!("minecraft:{stem}");
                if let Some(rule) = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|t| serde_json::from_str(&t).ok())
                    .and_then(|j: serde_json::Value| parse_rule(&id, &j))
                {
                    out.push(rule);
                }
            }
        }
        out
    })
}

/// Adds the recipes `held` unlocks to `owned`; returns whether anything was added.
pub fn apply(held: &BTreeSet<String>, owned: &mut BTreeSet<String>) -> bool {
    let before = owned.len();
    // Repeat: a recipe given by one rule can satisfy another's `recipe_unlocked`.
    loop {
        let size = owned.len();
        for rule in rules() {
            if rule.recipes.is_empty() || rule.recipes.iter().all(|r| owned.contains(r)) {
                continue;
            }
            if unlocked(rule, held, owned) {
                owned.extend(rule.recipes.iter().cloned());
            }
        }
        if owned.len() == size {
            break;
        }
    }
    owned.len() != before
}

/// Recipes an advancement rewards when completed.
pub fn reward_recipes(advancement: &str) -> Vec<String> {
    rules()
        .iter()
        .find(|r| r.id == advancement)
        .map(|r| r.recipes.clone())
        .unwrap_or_default()
}

/// Advancement criteria (`<advancement>#<criterion>`) that holding `held` completes, for every
/// advancement with an `inventory_changed` criterion except the recipe ones.
pub fn criteria_for(held: &BTreeSet<String>) -> Vec<String> {
    let mut out = Vec::new();
    for rule in rules() {
        if rule.id.starts_with("minecraft:recipes/") {
            continue;
        }
        for (name, criterion) in &rule.criteria {
            if let Criterion::Holds(entries) = criterion
                && !entries.is_empty()
                && entries.iter().all(|e| matches(e, held))
            {
                out.push(format!("{}#{name}", rule.id));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule() -> Rule {
        parse_rule("t:stick", &serde_json::json!({
            "criteria": {
                "has_log": {"trigger": "minecraft:inventory_changed",
                    "conditions": {"items": [{"items": "#minecraft:logs"}]}},
                "has_planks": {"trigger": "minecraft:inventory_changed",
                    "conditions": {"items": [{"items": ["minecraft:oak_planks", "minecraft:birch_planks"]}]}},
                "has_the_recipe": {"trigger": "minecraft:recipe_unlocked",
                    "conditions": {"recipes": "minecraft:stick"}}
            },
            "requirements": [["has_the_recipe", "has_log", "has_planks"]],
            "rewards": {"recipes": ["minecraft:stick"]}
        }))
        .unwrap()
    }

    #[test]
    fn a_held_item_or_the_recipe_itself_satisfies_the_rule() {
        let rule = rule();
        let held = |names: &[&str]| {
            names
                .iter()
                .map(|n| (*n).to_owned())
                .collect::<BTreeSet<_>>()
        };
        let none = BTreeSet::new();
        assert!(!unlocked(&rule, &held(&["minecraft:stone"]), &none));
        assert!(unlocked(&rule, &held(&["minecraft:birch_planks"]), &none));
        assert!(unlocked(&rule, &held(&["minecraft:oak_log"]), &none));
        assert!(unlocked(&rule, &none, &held(&["minecraft:stick"])));
    }

    #[test]
    fn criteria_for_skips_recipe_advancements() {
        let held = BTreeSet::from([
            "minecraft:oak_planks".to_owned(),
            "minecraft:iron_ingot".to_owned(),
        ]);
        assert!(
            criteria_for(&held)
                .iter()
                .all(|c| !c.starts_with("minecraft:recipes/"))
        );
    }

    #[test]
    fn apply_unlocks_the_rewarded_recipes_once() {
        // The extracted data may be absent on this machine; with it, oak planks unlock recipes.
        let mut owned = BTreeSet::new();
        let held = BTreeSet::from(["minecraft:oak_planks".to_owned()]);
        let changed = apply(&held, &mut owned);
        assert_eq!(changed, !rules().is_empty());
        assert!(!apply(&held, &mut owned));
    }
}
