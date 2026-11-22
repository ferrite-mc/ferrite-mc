use anyhow::{Context, Result, bail};
use ferrite_protocol::{
    VarInt,
    packets::configuration::{RegistryTags, Tag, UpdateTags},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

/// Mirrors `RegistryDataLoader.SYNCHRONIZED_REGISTRIES` of 26.3 (32 entries). The client
/// refuses to finish configuration if one is missing, so keep this in sync on version bumps.
const SYNCHRONIZED: &[&str] = &[
    "block_transformer",
    "decorated_pot_pattern",
    "worldgen/block_state_provider",
    "banner_pattern",
    "chat_type",
    "damage_type",
    "dialog",
    "dimension_type",
    "enchantment",
    "instrument",
    "jukebox_song",
    "painting_variant",
    "sulfur_cube_archetype",
    "test_environment",
    "test_instance",
    "timeline",
    "trim_material",
    "trim_pattern",
    "world_clock",
    "worldgen/biome",
    "cat_variant",
    "cat_sound_variant",
    "chicken_variant",
    "chicken_sound_variant",
    "cow_variant",
    "cow_sound_variant",
    "frog_variant",
    "pig_variant",
    "pig_sound_variant",
    "wolf_variant",
    "wolf_sound_variant",
    "zombie_nautilus_variant",
];
#[derive(Clone)]
pub struct VanillaData {
    pub registries: BTreeMap<String, Vec<String>>,
    pub tags: UpdateTags,
}
fn json_files(base: &Path) -> Result<Vec<(String, PathBuf)>> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<()> {
        if !dir.exists() {
            return Ok(());
        }
        for item in fs::read_dir(dir)? {
            let item = item?;
            let path = item.path();
            if item.file_type()?.is_dir() {
                walk(root, &path, out)?;
            } else if path.extension().is_some_and(|x| x == "json") {
                let relative = path.strip_prefix(root)?.with_extension("");
                out.push((relative.to_string_lossy().replace('\\', "/"), path));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(base, base, &mut out)?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}
fn resolve(
    name: &str,
    tags: &BTreeMap<String, Vec<(String, bool)>>,
    ids: &BTreeMap<String, i32>,
    visiting: &mut BTreeSet<String>,
) -> Result<Vec<VarInt>> {
    if !visiting.insert(name.to_owned()) {
        bail!("cyclic tag {name}");
    }
    let values = tags
        .get(name)
        .with_context(|| format!("missing tag {name}"))?;
    let mut result = Vec::new();
    for (value, required) in values {
        if let Some(tag) = value.strip_prefix('#') {
            if tags.contains_key(tag) {
                result.extend(resolve(tag, tags, ids, visiting)?);
            } else if *required {
                bail!("missing tag {tag}");
            }
        } else if let Some(id) = ids.get(value) {
            result.push(VarInt(*id));
        } else if *required {
            bail!("missing registry entry {value} in {name}");
        }
    }
    visiting.remove(name);
    Ok(result)
}
static VANILLA_CACHE: OnceLock<Arc<VanillaData>> = OnceLock::new();

/// Returns the already-cached vanilla registry data if initialized.
pub fn try_cached_vanilla() -> Option<Arc<VanillaData>> {
    VANILLA_CACHE.get().map(Arc::clone)
}

/// Loads (once per process) and shares the synchronized vanilla registry and tag tables.
pub fn cached_vanilla() -> Result<Arc<VanillaData>> {
    if let Some(cached) = VANILLA_CACHE.get() {
        return Ok(Arc::clone(cached));
    }
    let loaded = Arc::new(load_vanilla_uncached()?);
    let _ = VANILLA_CACHE.set(Arc::clone(&loaded));
    Ok(VANILLA_CACHE.get().map_or(loaded, Arc::clone))
}

#[cfg(test)]
pub fn load_vanilla() -> Result<VanillaData> {
    Ok((*cached_vanilla()?).clone())
}

fn load_vanilla_uncached() -> Result<VanillaData> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3/generated");
    let data = root.join("data/minecraft");
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("reports/registries.json"))?)?;
    let mut registries = BTreeMap::new();
    for name in SYNCHRONIZED {
        let entries = json_files(&data.join(name))?
            .into_iter()
            .map(|(n, _)| format!("minecraft:{n}"))
            .collect::<Vec<_>>();
        if entries.is_empty() {
            bail!("empty synchronized registry {name}");
        }
        registries.insert(format!("minecraft:{name}"), entries);
    }
    let mut maps = BTreeMap::<String, BTreeMap<String, i32>>::new();
    for (name, value) in report.as_object().context("registry report root")? {
        if let Some(entries) = value["entries"].as_object() {
            maps.insert(
                name.clone(),
                entries
                    .iter()
                    .filter_map(|(key, v)| {
                        v["protocol_id"].as_i64().map(|n| (key.clone(), n as i32))
                    })
                    .collect(),
            );
        }
    }
    for (name, entries) in &registries {
        maps.insert(
            name.clone(),
            entries
                .iter()
                .enumerate()
                .map(|(n, e)| (e.clone(), n as i32))
                .collect(),
        );
    }
    let mut registry_tags = Vec::new();
    for (registry, ids) in maps {
        let Some(path) = registry.strip_prefix("minecraft:") else {
            continue;
        };
        let files = json_files(&data.join("tags").join(path))?;
        if files.is_empty() {
            continue;
        }
        let mut tags = BTreeMap::new();
        for (name, path) in files {
            let value: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
            let mut values = Vec::new();
            for item in value["values"]
                .as_array()
                .with_context(|| format!("invalid tag {}", path.display()))?
            {
                let id = item
                    .as_str()
                    .or_else(|| item["id"].as_str())
                    .context("tag id")?;
                let required = item["required"].as_bool().unwrap_or(true);
                let id = if id.starts_with('#') {
                    let s = id.trim_start_matches('#');
                    format!(
                        "#{}",
                        if s.contains(':') {
                            s.to_string()
                        } else {
                            format!("minecraft:{s}")
                        }
                    )
                } else if id.contains(':') {
                    id.to_string()
                } else {
                    format!("minecraft:{id}")
                };
                values.push((id, required));
            }
            tags.insert(format!("minecraft:{name}"), values);
        }
        let mut resolved = Vec::new();
        for name in tags.keys() {
            resolved.push(Tag {
                name: name.clone(),
                entries: resolve(name, &tags, &ids, &mut BTreeSet::new())
                    .with_context(|| format!("registry {registry}"))?,
            });
        }
        registry_tags.push(RegistryTags {
            registry,
            tags: resolved,
        });
    }
    Ok(VanillaData {
        registries,
        tags: UpdateTags {
            registries: registry_tags,
        },
    })
}
#[cfg(test)]
mod tests {
    /// 26.3 `RegistryDataLoader.SYNCHRONIZED_REGISTRIES`, dumped from the vanilla classes.
    #[test]
    fn synchronized_registries_match_vanilla_26_3() {
        let vanilla = [
            "worldgen/biome",
            "chat_type",
            "trim_pattern",
            "trim_material",
            "wolf_variant",
            "wolf_sound_variant",
            "pig_variant",
            "pig_sound_variant",
            "frog_variant",
            "cat_variant",
            "cat_sound_variant",
            "cow_sound_variant",
            "cow_variant",
            "chicken_sound_variant",
            "chicken_variant",
            "zombie_nautilus_variant",
            "painting_variant",
            "sulfur_cube_archetype",
            "dimension_type",
            "damage_type",
            "banner_pattern",
            "enchantment",
            "jukebox_song",
            "instrument",
            "test_environment",
            "test_instance",
            "dialog",
            "world_clock",
            "timeline",
            "decorated_pot_pattern",
            "block_transformer",
            "worldgen/block_state_provider",
        ];
        let mut ours: Vec<_> = super::SYNCHRONIZED.to_vec();
        let mut vanilla = vanilla.to_vec();
        ours.sort_unstable();
        vanilla.sort_unstable();
        assert_eq!(ours, vanilla);
    }

    use super::*;
    #[test]
    fn resolves_nested_and_optional_tags_and_rejects_cycles() {
        let ids = BTreeMap::from([("minecraft:stone".to_string(), 5)]);
        let tags = BTreeMap::from([
            (
                "minecraft:inner".to_string(),
                vec![
                    ("minecraft:stone".to_string(), true),
                    ("minecraft:missing".to_string(), false),
                ],
            ),
            (
                "minecraft:outer".to_string(),
                vec![("#minecraft:inner".to_string(), true)],
            ),
            (
                "minecraft:cycle".to_string(),
                vec![("#minecraft:cycle".to_string(), true)],
            ),
        ]);
        assert_eq!(
            resolve("minecraft:outer", &tags, &ids, &mut BTreeSet::new()).unwrap(),
            vec![VarInt(5)]
        );
        assert!(resolve("minecraft:cycle", &tags, &ids, &mut BTreeSet::new()).is_err());
    }
}
