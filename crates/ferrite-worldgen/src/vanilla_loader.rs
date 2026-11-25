//! Loader for Minecraft 26.3 data-driven `worldgen/` JSON definitions.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

use crate::biome::{
    ClimateSampler, EndBiomeSource, MultiNoiseBiomeSource, Parameter, ParameterPoint,
};
use crate::chunk_gen::{
    DimensionKind, NoiseChunkGenerator, NoiseGeneratorSettings, SurfaceBiomeIds, TerrainPalette,
};
use crate::density::{
    Axis, BinaryOp, CubicSpline, DensityFunction, MappedOp, NoiseMap, SplinePoint, SplineValue,
};
use crate::noise::{BlendedNoise, NormalNoise, SimplexNoise};
use crate::random::XoroshiroRandom;

#[derive(Debug, thiserror::Error)]
pub enum LoaderError {
    #[error("I/O error reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("JSON parse error in {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("unknown density function reference: {0}")]
    MissingRef(String),
    #[error("unsupported density function schema: {0}")]
    Schema(String),
}

/// Loads all `worldgen/noise/*.json` and builds a `NoiseMap` seeded from `seed`.
pub fn load_noises(worldgen_dir: &Path, seed: u64) -> Result<NoiseMap, LoaderError> {
    let noise_dir = worldgen_dir.join("noise");
    let mut files = Vec::new();
    collect_json_files(&noise_dir, &noise_dir, &mut files)?;
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut base_rng = XoroshiroRandom::from_seed(seed);
    let factory = base_rng.fork_positional();
    let mut noises = NoiseMap::new();

    for (id, path) in files {
        let raw = std::fs::read_to_string(&path).map_err(|source| LoaderError::Io {
            path: path.clone(),
            source,
        })?;
        let val: Value = serde_json::from_str(&raw).map_err(|source| LoaderError::Json {
            path: path.clone(),
            source,
        })?;
        let base_octave = val["base_octave"].as_i64().unwrap_or(0) as i32;
        let octave_count = val["octave_count"].as_u64().unwrap_or(1) as usize;
        let base_amplitude = val["base_amplitude"].as_f64().unwrap_or(1.0);
        let amps: Vec<f64> = if let Some(arr) = val["amplitude_modifiers"].as_array() {
            arr.iter().map(|v| v.as_f64().unwrap_or(1.0)).collect()
        } else {
            vec![1.0; octave_count]
        };
        let mut n_rng = factory.from_hash_of(&id);
        noises.insert(
            id,
            Arc::new(NormalNoise::with_base_amplitude(
                &mut n_rng,
                base_octave,
                &amps,
                base_amplitude,
            )),
        );
    }

    Ok(noises)
}

/// Loads all `worldgen/density_function/**/*.json` into raw JSON values keyed by `minecraft:<rel>`.
pub fn load_density_function_map(
    worldgen_dir: &Path,
) -> Result<HashMap<String, Value>, LoaderError> {
    let df_dir = worldgen_dir.join("density_function");
    let mut files = Vec::new();
    collect_json_files(&df_dir, &df_dir, &mut files)?;

    let mut raw_map = HashMap::new();
    for (id, path) in files {
        let raw = std::fs::read_to_string(&path).map_err(|source| LoaderError::Io {
            path: path.clone(),
            source,
        })?;
        let val: Value = serde_json::from_str(&raw).map_err(|source| LoaderError::Json {
            path: path.clone(),
            source,
        })?;
        raw_map.insert(id, val);
    }
    Ok(raw_map)
}

/// Loads a 6D multi-noise biome parameter list JSON (e.g. `overworld.json` or `nether.json`).
pub fn load_multi_noise_biomes<F: Fn(&str) -> u32>(
    report_path: &Path,
    resolve_biome: F,
) -> Result<MultiNoiseBiomeSource, LoaderError> {
    let raw = std::fs::read_to_string(report_path).map_err(|source| LoaderError::Io {
        path: report_path.to_path_buf(),
        source,
    })?;
    let val: Value = serde_json::from_str(&raw).map_err(|source| LoaderError::Json {
        path: report_path.to_path_buf(),
        source,
    })?;
    let mut entries = Vec::new();
    if let Some(arr) = val["biomes"].as_array() {
        for item in arr {
            let Some(name) = item["biome"].as_str() else {
                continue;
            };
            let id = resolve_biome(name);
            let p = &item["parameters"];
            let pt = ParameterPoint {
                temperature: parse_param(&p["temperature"]),
                humidity: parse_param(&p["humidity"]),
                continentalness: parse_param(&p["continentalness"]),
                erosion: parse_param(&p["erosion"]),
                depth: parse_param(&p["depth"]),
                weirdness: parse_param(&p["weirdness"]),
                offset: p["offset"].as_f64().unwrap_or(0.0) as f32,
            };
            entries.push((pt, id));
        }
    }
    Ok(MultiNoiseBiomeSource::new(entries))
}

/// Loads the vanilla 6D Overworld biome parameter list from `reports/biome_parameters/minecraft/overworld.json`.
pub fn load_overworld_biomes<F: Fn(&str) -> u32>(
    report_path: &Path,
    resolve_biome: F,
) -> Result<MultiNoiseBiomeSource, LoaderError> {
    load_multi_noise_biomes(report_path, resolve_biome)
}

fn parse_param(v: &Value) -> Parameter {
    if let Some(num) = v.as_f64() {
        Parameter::point(num as f32)
    } else if let Some(arr) = v.as_array() {
        let min = arr.first().and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
        let max = arr.get(1).and_then(|x| x.as_f64()).unwrap_or(min as f64) as f32;
        Parameter::span(min, max)
    } else {
        Parameter::point(0.0)
    }
}

/// Biome source configuration passed to [`load_dimension_generator`].
#[derive(Debug, Clone)]
pub enum BiomeConfig {
    MultiNoise(MultiNoiseBiomeSource),
    End(EndBiomeSource),
}

/// Compiles a `NoiseChunkGenerator` for a dimension from `vanilla/26.3/generated/data/minecraft/worldgen`.
pub fn load_dimension_generator(
    worldgen_dir: &Path,
    seed: u64,
    noise_settings_file: &str,
    dimension: DimensionKind,
    chunk_height: i32,
    palette: TerrainPalette,
    biomes: BiomeConfig,
) -> Result<NoiseChunkGenerator, LoaderError> {
    let (biome_source, end_biomes) = match biomes {
        BiomeConfig::MultiNoise(source) => (source, None),
        BiomeConfig::End(source) => (MultiNoiseBiomeSource::new(Vec::new()), Some(source)),
    };
    let noises = load_noises(worldgen_dir, seed)?;
    let raw_dfs = load_density_function_map(worldgen_dir)?;
    let mut resolved = HashMap::new();
    let mut next_cache_id = 0usize;
    let mut base_rng = XoroshiroRandom::from_seed(seed);
    let factory = base_rng.fork_positional();
    let mut blended_rng = factory.from_hash_of("minecraft:terrain");
    let end_islands = Arc::new(SimplexNoise::new(&mut XoroshiroRandom::from_seed(seed)));

    let ns_path = worldgen_dir.join(format!("noise_settings/{noise_settings_file}.json"));
    let ns_raw = std::fs::read_to_string(&ns_path).map_err(|source| LoaderError::Io {
        path: ns_path.clone(),
        source,
    })?;
    let ns: Value = serde_json::from_str(&ns_raw).map_err(|source| LoaderError::Json {
        path: ns_path,
        source,
    })?;

    let min_y = ns["noise"]["min_y"].as_i64().unwrap_or(-64) as i32;
    let gen_height = ns["noise"]["height"].as_i64().unwrap_or(384) as i32;
    let sea_level = ns["sea_level"].as_i64().unwrap_or(63) as i32;
    let (cell_w, cell_h) =
        extract_cell_size(&ns["noise_router"]["final_density"]).unwrap_or((4, 8));
    let router = &ns["noise_router"];

    let mut resolve = |val: &Value| {
        compile_density(
            val,
            &raw_dfs,
            &mut resolved,
            &mut blended_rng,
            &end_islands,
            &mut next_cache_id,
        )
    };

    let mut final_density = resolve(&router["final_density"])?;
    let temperature = resolve(&router["temperature"])?;
    let humidity = resolve(&router["vegetation"])?;
    let continentalness = resolve(&router["continents"])?;
    let erosion = resolve(&router["erosion"])?;
    let depth = resolve(&router["depth"])?;
    let weirdness = resolve(&router["ridges"])?;

    let mut climate = ClimateSampler {
        temperature,
        humidity,
        continentalness,
        erosion,
        depth,
        weirdness,
    };
    final_density.bind_noises(&noises);
    climate.bind_noises(&noises);

    Ok(NoiseChunkGenerator {
        seed,
        noises,
        cache_count: next_cache_id,
        settings: NoiseGeneratorSettings {
            dimension,
            min_y,
            height: chunk_height,
            gen_height,
            sea_level,
            cell_w,
            cell_h,
            final_density,
            climate,
            biome_source,
            end_biomes,
            surface_biomes: SurfaceBiomeIds::default(),
            palette,
        },
    })
}

fn extract_cell_size(val: &Value) -> Option<(usize, usize)> {
    let obj = val.as_object()?;
    if obj.get("type").and_then(|v| v.as_str()) == Some("minecraft:interpolated") {
        let xz = obj.get("cell_size_xz").and_then(|v| v.as_u64())? as usize;
        let y = obj.get("cell_size_y").and_then(|v| v.as_u64())? as usize;
        return Some((xz.max(1), y.max(1)));
    }
    for key in ["left", "right", "input"] {
        if let Some(child) = obj.get(key)
            && let Some(sz) = extract_cell_size(child)
        {
            return Some(sz);
        }
    }
    None
}

/// Compiles an Overworld `NoiseChunkGenerator` from `vanilla/26.3/generated/data/minecraft/worldgen`.
pub fn load_overworld_generator(
    worldgen_dir: &Path,
    seed: u64,
    palette: TerrainPalette,
    biome_source: MultiNoiseBiomeSource,
) -> Result<NoiseChunkGenerator, LoaderError> {
    load_dimension_generator(
        worldgen_dir,
        seed,
        "overworld",
        DimensionKind::Overworld,
        384,
        palette,
        BiomeConfig::MultiNoise(biome_source),
    )
}

/// Compiles a Nether `NoiseChunkGenerator` from `vanilla/26.3/generated/data/minecraft/worldgen`.
pub fn load_nether_generator(
    worldgen_dir: &Path,
    seed: u64,
    palette: TerrainPalette,
    biome_source: MultiNoiseBiomeSource,
) -> Result<NoiseChunkGenerator, LoaderError> {
    load_dimension_generator(
        worldgen_dir,
        seed,
        "nether",
        DimensionKind::Nether,
        256,
        palette,
        BiomeConfig::MultiNoise(biome_source),
    )
}

/// Compiles an End `NoiseChunkGenerator` from `vanilla/26.3/generated/data/minecraft/worldgen`.
pub fn load_end_generator(
    worldgen_dir: &Path,
    seed: u64,
    palette: TerrainPalette,
    end_biomes: EndBiomeSource,
) -> Result<NoiseChunkGenerator, LoaderError> {
    load_dimension_generator(
        worldgen_dir,
        seed,
        "end",
        DimensionKind::End,
        256,
        palette,
        BiomeConfig::End(end_biomes),
    )
}

fn compile_density(
    val: &Value,
    raw_dfs: &HashMap<String, Value>,
    resolved: &mut HashMap<String, DensityFunction>,
    blended_rng: &mut XoroshiroRandom,
    end_islands: &Arc<SimplexNoise>,
    next_cache_id: &mut usize,
) -> Result<DensityFunction, LoaderError> {
    if let Some(num) = val.as_f64() {
        return Ok(DensityFunction::Constant(num));
    }
    if let Some(id) = val.as_str() {
        if let Some(cached) = resolved.get(id) {
            return Ok(cached.clone());
        }
        let target = raw_dfs
            .get(id)
            .ok_or_else(|| LoaderError::MissingRef(id.to_string()))?;
        let df = compile_density(
            target,
            raw_dfs,
            resolved,
            blended_rng,
            end_islands,
            next_cache_id,
        )?;
        resolved.insert(id.to_string(), df.clone());
        return Ok(df);
    }

    let obj = val
        .as_object()
        .ok_or_else(|| LoaderError::Schema(val.to_string()))?;
    let ty = obj
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| LoaderError::Schema(val.to_string()))?;

    let mut sub = |key: &str| -> Result<DensityFunction, LoaderError> {
        let child = obj
            .get(key)
            .ok_or_else(|| LoaderError::Schema(format!("{ty} missing {key}")))?;
        compile_density(
            child,
            raw_dfs,
            resolved,
            blended_rng,
            end_islands,
            next_cache_id,
        )
    };

    match ty {
        "minecraft:abs" => Ok(DensityFunction::Mapped {
            op: MappedOp::Abs,
            input: Box::new(sub("input")?),
        }),
        "minecraft:square" => Ok(DensityFunction::Mapped {
            op: MappedOp::Square,
            input: Box::new(sub("input")?),
        }),
        "minecraft:cube" => Ok(DensityFunction::Mapped {
            op: MappedOp::Cube,
            input: Box::new(sub("input")?),
        }),
        "minecraft:half_negative" => Ok(DensityFunction::Mapped {
            op: MappedOp::HalfNegative,
            input: Box::new(sub("input")?),
        }),
        "minecraft:quarter_negative" => Ok(DensityFunction::Mapped {
            op: MappedOp::QuarterNegative,
            input: Box::new(sub("input")?),
        }),
        "minecraft:squeeze" => Ok(DensityFunction::Mapped {
            op: MappedOp::Squeeze,
            input: Box::new(sub("input")?),
        }),
        "minecraft:negate" => Ok(DensityFunction::Mapped {
            op: MappedOp::Negate,
            input: Box::new(sub("input")?),
        }),
        "minecraft:add" => Ok(DensityFunction::Binary {
            op: BinaryOp::Add,
            arg1: Box::new(sub("left")?),
            arg2: Box::new(sub("right")?),
        }),
        "minecraft:sub" => Ok(DensityFunction::Binary {
            op: BinaryOp::Sub,
            arg1: Box::new(sub("left")?),
            arg2: Box::new(sub("right")?),
        }),
        "minecraft:mul" => Ok(DensityFunction::Binary {
            op: BinaryOp::Mul,
            arg1: Box::new(sub("left")?),
            arg2: Box::new(sub("right")?),
        }),
        "minecraft:div" => Ok(DensityFunction::Binary {
            op: BinaryOp::Div,
            arg1: Box::new(sub("left")?),
            arg2: Box::new(sub("right")?),
        }),
        "minecraft:min" => Ok(DensityFunction::Binary {
            op: BinaryOp::Min,
            arg1: Box::new(sub("left")?),
            arg2: Box::new(sub("right")?),
        }),
        "minecraft:max" => Ok(DensityFunction::Binary {
            op: BinaryOp::Max,
            arg1: Box::new(sub("left")?),
            arg2: Box::new(sub("right")?),
        }),
        "minecraft:lerp" => Ok(DensityFunction::Lerp {
            alpha: Box::new(sub("alpha")?),
            first: Box::new(sub("first")?),
            second: Box::new(sub("second")?),
        }),
        "minecraft:clamp" => Ok(DensityFunction::Clamp {
            input: Box::new(sub("input")?),
            min: obj.get("min").and_then(|v| v.as_f64()).unwrap_or(0.0),
            max: obj.get("max").and_then(|v| v.as_f64()).unwrap_or(0.0),
        }),
        "minecraft:gradient" => Ok(DensityFunction::Gradient {
            axis: parse_axis(obj.get("axis").and_then(|v| v.as_str())),
            from_coordinate: obj
                .get("from_coordinate")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32,
            to_coordinate: obj
                .get("to_coordinate")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32,
            from_value: obj
                .get("from_value")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0),
            to_value: obj.get("to_value").and_then(|v| v.as_f64()).unwrap_or(0.0),
        }),
        "minecraft:slice" => Ok(DensityFunction::Slice {
            axis: parse_axis(obj.get("axis").and_then(|v| v.as_str())),
            coordinate: obj.get("coordinate").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
            input: Box::new(sub("input")?),
        }),
        "minecraft:noise" => {
            let noise_id = obj
                .get("noise")
                .and_then(|v| v.as_str())
                .ok_or_else(|| LoaderError::Schema("noise missing noise id".into()))?
                .to_string();
            let shift_x = match obj.get("shift_x") {
                Some(sx) => Some(Box::new(compile_density(
                    sx,
                    raw_dfs,
                    resolved,
                    blended_rng,
                    end_islands,
                    next_cache_id,
                )?)),
                None => None,
            };
            let shift_z = match obj.get("shift_z") {
                Some(sz) => Some(Box::new(compile_density(
                    sz,
                    raw_dfs,
                    resolved,
                    blended_rng,
                    end_islands,
                    next_cache_id,
                )?)),
                None => None,
            };
            Ok(DensityFunction::Noise {
                noise_id,
                noise: None,
                xz_scale: obj.get("xz_scale").and_then(|v| v.as_f64()).unwrap_or(1.0),
                y_scale: obj.get("y_scale").and_then(|v| v.as_f64()).unwrap_or(1.0),
                shift_x,
                shift_z,
            })
        }
        "minecraft:shift_a" => Ok(DensityFunction::ShiftA {
            noise_id: obj
                .get("noise")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            noise: None,
        }),
        "minecraft:shift_b" => Ok(DensityFunction::ShiftB {
            noise_id: obj
                .get("noise")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            noise: None,
        }),
        "minecraft:old_blended_noise" => {
            let mut rng = blended_rng.clone();
            let bn = BlendedNoise::new(
                &mut rng,
                obj.get("xz_scale").and_then(|v| v.as_f64()).unwrap_or(0.25),
                obj.get("y_scale").and_then(|v| v.as_f64()).unwrap_or(0.125),
                obj.get("xz_factor")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(80.0),
                obj.get("y_factor")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(160.0),
                obj.get("smear_scale_multiplier")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(8.0),
            );
            Ok(DensityFunction::OldBlendedNoise {
                blended: Some(Arc::new(bn)),
            })
        }
        "minecraft:range_choice" => Ok(DensityFunction::RangeChoice {
            input: Box::new(sub("input")?),
            min_inclusive: obj
                .get("min_inclusive")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0),
            max_exclusive: obj
                .get("max_exclusive")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0),
            when_in_range: Box::new(sub("when_in_range")?),
            when_out_of_range: Box::new(sub("when_out_of_range")?),
        }),
        "minecraft:interval_select" => {
            let input = Box::new(sub("input")?);
            let thresholds = obj
                .get("thresholds")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|x| x.as_f64()).collect())
                .unwrap_or_default();
            let mut functions = Vec::new();
            if let Some(arr) = obj.get("functions").and_then(|v| v.as_array()) {
                for f in arr {
                    functions.push(compile_density(
                        f,
                        raw_dfs,
                        resolved,
                        blended_rng,
                        end_islands,
                        next_cache_id,
                    )?);
                }
            }
            Ok(DensityFunction::IntervalSelect {
                input,
                thresholds,
                functions,
            })
        }
        "minecraft:distance_to_point" => {
            let pt = obj
                .get("point")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    [
                        arr.first().and_then(|x| x.as_f64()).unwrap_or(0.0),
                        arr.get(1).and_then(|x| x.as_f64()).unwrap_or(0.0),
                        arr.get(2).and_then(|x| x.as_f64()).unwrap_or(0.0),
                    ]
                })
                .unwrap_or([0.0, 0.0, 0.0]);
            Ok(DensityFunction::DistanceToPoint { point: pt })
        }
        "minecraft:find_top_surface" => Ok(DensityFunction::FindTopSurface {
            density: Box::new(sub("density")?),
            upper_bound: Box::new(sub("upper_bound")?),
            lower_bound: obj
                .get("lower_bound")
                .and_then(|v| v.as_i64())
                .unwrap_or(-64) as i32,
            cell_height: obj.get("cell_height").and_then(|v| v.as_i64()).unwrap_or(8) as i32,
        }),
        "minecraft:spline" => {
            let spline_val = obj
                .get("spline")
                .ok_or_else(|| LoaderError::Schema("spline missing spline".into()))?;
            let spline = compile_spline(
                spline_val,
                raw_dfs,
                resolved,
                blended_rng,
                end_islands,
                next_cache_id,
            )?;
            Ok(DensityFunction::Spline(Box::new(spline)))
        }
        "minecraft:cache" => {
            let child = obj
                .get("input")
                .ok_or_else(|| LoaderError::Schema(format!("{ty} missing input")))?;
            let cache_key = format!("__inline_cache:{child}");
            if let Some(existing) = resolved.get(&cache_key) {
                return Ok(existing.clone());
            }
            let inner = compile_density(
                child,
                raw_dfs,
                resolved,
                blended_rng,
                end_islands,
                next_cache_id,
            )?;
            let id = *next_cache_id;
            *next_cache_id += 1;
            let is_2d = !inner.depends_on_y();
            let node = DensityFunction::Cached {
                id,
                is_2d,
                input: Box::new(inner),
            };
            resolved.insert(cache_key, node.clone());
            Ok(node)
        }
        "minecraft:interpolated" | "minecraft:blend_density" => {
            Ok(DensityFunction::Wrapper(Box::new(sub("input")?)))
        }
        "minecraft:blend_alpha" => Ok(DensityFunction::Constant(1.0)),
        "minecraft:blend_offset" | "minecraft:beardifier" => Ok(DensityFunction::Constant(0.0)),
        "minecraft:end_outer_islands" => {
            Ok(DensityFunction::EndOuterIslands(Arc::clone(end_islands)))
        }
        other => Err(LoaderError::Schema(format!("unsupported type {other}"))),
    }
}

fn compile_spline(
    val: &Value,
    raw_dfs: &HashMap<String, Value>,
    resolved: &mut HashMap<String, DensityFunction>,
    blended_rng: &mut XoroshiroRandom,
    end_islands: &Arc<SimplexNoise>,
    next_cache_id: &mut usize,
) -> Result<CubicSpline, LoaderError> {
    let coord = compile_density(
        &val["coordinate"],
        raw_dfs,
        resolved,
        blended_rng,
        end_islands,
        next_cache_id,
    )?;
    let mut points = Vec::new();
    if let Some(arr) = val["points"].as_array() {
        for p in arr {
            let location = p["location"].as_f64().unwrap_or(0.0);
            let derivative = p["derivative"].as_f64().unwrap_or(0.0);
            let value = if let Some(num) = p["value"].as_f64() {
                SplineValue::Constant(num)
            } else {
                SplineValue::Spline(Box::new(compile_spline(
                    &p["value"],
                    raw_dfs,
                    resolved,
                    blended_rng,
                    end_islands,
                    next_cache_id,
                )?))
            };
            points.push(SplinePoint {
                location,
                value,
                derivative,
            });
        }
    }
    Ok(CubicSpline {
        coordinate: Box::new(coord),
        points,
    })
}

fn parse_axis(s: Option<&str>) -> Axis {
    match s {
        Some("x") => Axis::X,
        Some("z") => Axis::Z,
        _ => Axis::Y,
    }
}

fn collect_json_files(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(String, PathBuf)>,
) -> Result<(), LoaderError> {
    let entries = std::fs::read_dir(dir).map_err(|source| LoaderError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| LoaderError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_json_files(root, &path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("json")
            && let Ok(rel) = path.strip_prefix(root)
        {
            let mut key = rel.with_extension("").to_string_lossy().replace('\\', "/");
            key.insert_str(0, "minecraft:");
            out.push((key, path));
        }
    }
    Ok(())
}
