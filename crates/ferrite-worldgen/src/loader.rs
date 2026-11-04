//! Filesystem loading of the generated worldgen data.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use thiserror::Error;

use crate::WorldgenData;
use crate::feature::{BlockStateProvider, BlockTag};
use crate::id::ResourceId;

/// Failure while loading a worldgen data set.
#[derive(Debug, Error)]
pub enum LoadError {
    #[error(
        "worldgen data directory `{path}` not found; expected the generated root containing \
         `data/minecraft`, or a `data/minecraft` directory"
    )]
    MissingData { path: PathBuf },
    #[error("failed to read `{path}`: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid JSON in `{path}`: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("could not re-serialize `{path}` for the strict check: {source}")]
    Serialize {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("`{path}` would drop field `{field}`; update the typed model")]
    DroppedField { path: PathBuf, field: String },
    #[error("invalid resource id `{value}` derived from `{path}`")]
    InvalidId { path: PathBuf, value: String },
    #[error("duplicate resource id `{id}` while loading `{path}`")]
    DuplicateId { path: PathBuf, id: ResourceId },
}

pub(crate) fn load(dir: &Path, strict: bool) -> Result<WorldgenData, LoadError> {
    let (data_root, reports_root) = resolve_roots(dir)?;
    let biome_parameters = match reports_root {
        // Reports are grouped by namespace directory (`.../biome_parameters/minecraft/...`).
        Some(reports_root) => load_dir(
            &reports_root.join("biome_parameters").join("minecraft"),
            strict,
        )?,
        None => BTreeMap::new(),
    };

    Ok(WorldgenData {
        noises: load_dir(&data_root.join("worldgen").join("noise"), strict)?,
        density_functions: load_dir(&data_root.join("worldgen").join("density_function"), strict)?,
        noise_settings: load_dir(&data_root.join("worldgen").join("noise_settings"), strict)?,
        biomes: load_dir(&data_root.join("worldgen").join("biome"), strict)?,
        material_conditions: load_dir(
            &data_root.join("worldgen").join("material_condition"),
            strict,
        )?,
        material_rules: load_dir(&data_root.join("worldgen").join("material_rule"), strict)?,
        configured_features: load_dir(&data_root.join("worldgen").join("feature"), strict)?,
        placed_features: load_dir(&data_root.join("worldgen").join("placed_feature"), strict)?,
        structure_sets: load_dir(&data_root.join("worldgen").join("structure_set"), strict)?,
        structures: load_dir(&data_root.join("worldgen").join("structure"), strict)?,
        biome_tags: load_dir(
            &data_root.join("tags").join("worldgen").join("biome"),
            strict,
        )?,
        block_state_providers: load_dir::<BlockStateProvider>(
            &data_root.join("worldgen").join("block_state_provider"),
            strict,
        )?,
        block_tags: load_tags(&data_root.join("tags").join("block"), strict)?,
        dimension_types: load_dir(&data_root.join("dimension_type"), strict)?,
        dimensions: load_dir(&data_root.join("dimension"), strict)?,
        world_presets: load_dir(&data_root.join("worldgen").join("world_preset"), strict)?,
        multi_noise_presets: load_dir(
            &data_root
                .join("worldgen")
                .join("multi_noise_biome_source_parameter_list"),
            strict,
        )?,
        biome_parameters,
    })
}

fn load_tags(root: &Path, strict: bool) -> Result<BTreeMap<ResourceId, BlockTag>, LoadError> {
    load_dir(root, strict)
}

fn resolve_roots(dir: &Path) -> Result<(PathBuf, Option<PathBuf>), LoadError> {
    let generated_data = dir.join("data").join("minecraft");
    if generated_data.is_dir() {
        let reports = dir.join("reports");
        return Ok((generated_data, reports.is_dir().then_some(reports)));
    }
    if dir.join("worldgen").is_dir() {
        // `dir` is `.../generated/data/minecraft`; reports sit two levels up.
        let reports = dir
            .parent()
            .and_then(Path::parent)
            .map(|generated| generated.join("reports"))
            .filter(|reports| reports.is_dir());
        return Ok((dir.to_path_buf(), reports));
    }
    Err(LoadError::MissingData {
        path: dir.to_path_buf(),
    })
}

fn load_dir<T>(root: &Path, strict: bool) -> Result<BTreeMap<ResourceId, T>, LoadError>
where
    T: DeserializeOwned + Serialize,
{
    let mut entries = BTreeMap::new();
    if !root.is_dir() {
        return Ok(entries);
    }
    let mut files = Vec::new();
    collect_json_files(root, &mut files)?;
    files.sort();

    for path in files {
        let relative = path
            .strip_prefix(root)
            .expect("collected paths are below the root")
            .to_string_lossy()
            .replace('\\', "/");
        let path_id = relative
            .strip_suffix(".json")
            .expect("only .json files are collected");
        let value = format!("minecraft:{path_id}");
        let id = ResourceId::parse(&value).map_err(|_| LoadError::InvalidId {
            path: path.clone(),
            value,
        })?;
        let entry: T = load_json(&path, strict)?;
        if entries.insert(id.clone(), entry).is_some() {
            return Err(LoadError::DuplicateId { path, id });
        }
    }
    Ok(entries)
}

fn collect_json_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<(), LoadError> {
    let read_dir = fs::read_dir(root).map_err(|source| LoadError::Io {
        path: root.to_path_buf(),
        source,
    })?;
    for entry in read_dir {
        let entry = entry.map_err(|source| LoadError::Io {
            path: root.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|source| LoadError::Io {
            path: path.clone(),
            source,
        })?;
        if file_type.is_dir() {
            collect_json_files(&path, files)?;
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
            files.push(path);
        }
    }
    Ok(())
}

fn load_json<T>(path: &Path, strict: bool) -> Result<T, LoadError>
where
    T: DeserializeOwned + Serialize,
{
    let text = fs::read_to_string(path).map_err(|source| LoadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let value: T = serde_json::from_str(&text).map_err(|source| LoadError::Json {
        path: path.to_path_buf(),
        source,
    })?;

    if strict {
        let original: Value = serde_json::from_str(&text).map_err(|source| LoadError::Json {
            path: path.to_path_buf(),
            source,
        })?;
        let round_trip = serde_json::to_value(&value).map_err(|source| LoadError::Serialize {
            path: path.to_path_buf(),
            source,
        })?;
        if let Some(field) = crate::strict::first_dropped_key(&original, &round_trip) {
            return Err(LoadError::DroppedField {
                path: path.to_path_buf(),
                field,
            });
        }
    }

    Ok(value)
}
