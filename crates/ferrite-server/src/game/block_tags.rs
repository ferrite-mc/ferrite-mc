//! Block and item tags (`data/minecraft/tags/{block,item}/*.json`) for block and item
//! predicates (`#minecraft:logs`).
//!
//! Tags are read from the extracted vanilla data once and cached. Data-pack block tags are not
//! consulted (see `docs/COMMANDS-STATUS.md`).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

type Members = Arc<HashSet<String>>;

type Cache = Mutex<HashMap<(&'static str, String), Option<Members>>>;

fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn tag_file(kind: &str, name: &str) -> Option<serde_json::Value> {
    if name.contains("..") {
        return None;
    }
    let relative = format!("vanilla/26.3/generated/data/minecraft/tags/{kind}/{name}.json");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    [PathBuf::from(&relative), root.join(&relative)]
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
}

fn expand(kind: &str, name: &str, depth: u8, out: &mut HashSet<String>) -> bool {
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    let Some(json) = tag_file(kind, name) else {
        return false;
    };
    if depth > 16 {
        return true;
    }
    for value in json["values"].as_array().into_iter().flatten() {
        let id = value.as_str().or_else(|| value["id"].as_str());
        let Some(id) = id else { continue };
        if let Some(inner) = id.strip_prefix('#') {
            expand(kind, inner, depth + 1, out);
        } else {
            let id = if id.contains(':') {
                id.to_owned()
            } else {
                format!("minecraft:{id}")
            };
            out.insert(id);
        }
    }
    true
}

/// The block ids a tag (with or without `#` and `minecraft:`) expands to, or `None` when no
/// such tag exists.
pub fn members(tag: &str) -> Option<Members> {
    members_of("block", tag)
}

/// The members of a `block` or `item` tag.
pub fn members_of(kind: &'static str, tag: &str) -> Option<Members> {
    let name = tag.trim_start_matches('#');
    let name = name.strip_prefix("minecraft:").unwrap_or(name).to_owned();
    let key = (kind, name.clone());
    if let Some(found) = cache().lock().unwrap().get(&key) {
        return found.clone();
    }
    let mut set = HashSet::new();
    let found = expand(kind, &name, 0, &mut set).then(|| Arc::new(set));
    cache().lock().unwrap().insert(key, found.clone());
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_expands_nested_tags_and_unknown_tags_are_none() {
        let logs = members("#minecraft:logs").expect("logs tag");
        assert!(
            logs.contains("minecraft:oak_log"),
            "nested via logs_that_burn"
        );
        assert!(logs.contains("minecraft:crimson_stem"));
        assert!(!logs.contains("minecraft:stone"));
        assert!(members("logs").is_some(), "namespace and # are optional");
        assert!(members("#minecraft:no_such_tag").is_none());
        let planks = members_of("item", "#minecraft:planks").expect("item tag");
        assert!(planks.contains("minecraft:oak_planks"));
    }
}
