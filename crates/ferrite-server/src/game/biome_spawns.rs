//! The biomes' natural spawn lists (`minecraft:gameplay/natural_mob_spawns` in the biome files of
//! the user's own server jar), reduced to the mob kinds this server models.

use ferrite_entity::mob::MobKind;
use ferrite_entity::spawn::SpawnEntry;
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

struct Lists {
    creature: Vec<SpawnEntry>,
    monster: Vec<SpawnEntry>,
}

static BIOMES: OnceLock<HashMap<String, Lists>> = OnceLock::new();

/// The creature list of a biome (`minecraft:plains`); `None` when the biome files are missing,
/// so the caller falls back to its default spawns.
pub fn creature(biome: &str) -> Option<&'static [SpawnEntry]> {
    Some(&table().get(biome)?.creature)
}

/// The monster list of a biome.
pub fn monster(biome: &str) -> Option<&'static [SpawnEntry]> {
    Some(&table().get(biome)?.monster)
}

fn table() -> &'static HashMap<String, Lists> {
    BIOMES.get_or_init(|| {
        let relative = Path::new("vanilla/26.3/generated/data/minecraft/worldgen/biome");
        let Some(dir) = [
            relative.to_path_buf(),
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(relative),
        ]
        .into_iter()
        .find(|d| d.is_dir()) else {
            return HashMap::new();
        };
        let mut out = HashMap::new();
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let Some(name) = path
                .file_stem()
                .filter(|_| path.extension().is_some_and(|e| e == "json"))
            else {
                continue;
            };
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            out.insert(
                format!("minecraft:{}", name.to_string_lossy()),
                parse(&json),
            );
        }
        out
    })
}

fn parse(biome: &serde_json::Value) -> Lists {
    let spawns = &biome["attributes"]["minecraft:gameplay/natural_mob_spawns"]["argument"]["spawns_by_category"];
    Lists {
        creature: entries(&spawns["creature"]),
        monster: entries(&spawns["monster"]),
    }
}

/// The modelled kinds of one category. A fixed `count` or a `uniform` range gives the group size.
fn entries(list: &serde_json::Value) -> Vec<SpawnEntry> {
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let kind = MobKind::from_name(entry["type"].as_str()?)?;
            let (min, max) = match &entry["count"] {
                serde_json::Value::Number(n) => {
                    let n = i32::try_from(n.as_i64()?).ok()?;
                    (n, n)
                }
                range => (
                    i32::try_from(range["min_inclusive"].as_i64()?).ok()?,
                    i32::try_from(range["max_inclusive"].as_i64()?).ok()?,
                ),
            };
            Some(SpawnEntry {
                kind,
                weight: i32::try_from(entry["weight"].as_i64()?).ok()?,
                min,
                max: max.max(min),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_fixed_or_uniform_and_unmodelled_kinds_are_dropped() {
        let json = serde_json::json!({"attributes": {"minecraft:gameplay/natural_mob_spawns": {
            "argument": {"spawns_by_category": {"creature": [
                {"type": "minecraft:wolf", "count": 4, "weight": 8},
                {"type": "minecraft:cow", "count": {"type": "minecraft:uniform",
                    "min_inclusive": 2, "max_inclusive": 3}, "weight": 4},
                {"type": "minecraft:rabbit", "count": 3, "weight": 4}]}}}}});
        let lists = parse(&json);
        assert_eq!(
            lists.creature,
            vec![
                SpawnEntry {
                    kind: MobKind::Wolf,
                    weight: 8,
                    min: 4,
                    max: 4
                },
                SpawnEntry {
                    kind: MobKind::Cow,
                    weight: 4,
                    min: 2,
                    max: 3
                },
            ]
        );
        assert!(lists.monster.is_empty());
    }

    #[test]
    fn the_real_taiga_spawns_wolves_when_the_data_is_there() {
        let Some(list) = creature("minecraft:taiga") else {
            return;
        };
        assert!(list.iter().any(|e| e.kind == MobKind::Wolf));
        assert!(
            !creature("minecraft:plains")
                .unwrap()
                .iter()
                .any(|e| e.kind == MobKind::Wolf)
        );
    }
}
