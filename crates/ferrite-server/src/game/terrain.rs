//! Vanilla overworld terrain for newly generated chunks.
//!
//! The pipeline mirrors `NoiseBasedChunkGenerator.buildTerrain` in the 26.3
//! server: a `NoiseChunk` fill (`NoiseChunkGenerator::fill_noise_chunk`), then
//! `MaterialSystem.buildSurface` ([`ferrite_worldgen::SurfaceGenerator`]), then
//! the carvers. It runs on a dedicated thread so a region tick never evaluates
//! density functions; callers wait for a result from a `spawn_blocking` task.
use crossbeam::channel::{Receiver, Sender, bounded};
use ferrite_world::{
    BlockInfo, BlockStateRegistry, Chunk, HeightmapKind, LightData, LightSection, RuntimeRegistry,
};
use ferrite_worldgen::{FeatureGenerator, NoiseChunkGenerator, SurfaceGenerator, WorldgenData};
use std::{path::PathBuf, sync::Arc};

/// Bounded worker pool size: the happy medium between the 20 available cores and the
/// memory each generator copy needs. `FERRITE_WORLDGEN_THREADS` overrides it.
const DEFAULT_WORLDGEN_THREADS: usize = 8;

fn worldgen_threads() -> usize {
    std::env::var("FERRITE_WORLDGEN_THREADS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|threads| *threads > 0)
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map_or(1, |threads| threads.get().min(DEFAULT_WORLDGEN_THREADS))
        })
}

/// How new chunks are produced.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TerrainKind {
    /// The in-memory test world used before world generation existed.
    #[default]
    Superflat,
    /// The 26.3 overworld noise generator.
    Vanilla,
}

/// A generated column plus the light data the wire encoder needs.
pub struct Generated {
    pub chunk: Chunk,
    pub light: LightData,
    /// Wall-clock milliseconds per pipeline stage, for the throughput benchmark.
    pub timings: StageTimings,
}

/// Wall-clock time spent in each worldgen stage, in milliseconds.
#[derive(Clone, Copy, Debug, Default)]
pub struct StageTimings {
    pub noise_ms: f64,
    pub surface_ms: f64,
    pub carve_ms: f64,
    pub biomes_ms: f64,
    pub features_ms: f64,
    pub light_ms: f64,
}

enum Job {
    Chunk {
        dimension: super::Dimension,
        x: i32,
        z: i32,
        reply: Sender<Result<Generated, String>>,
    },
    Height {
        dimension: super::Dimension,
        x: i32,
        z: i32,
        reply: Sender<Result<i32, String>>,
    },
    LocateBiome {
        target: String,
        x: i32,
        z: i32,
        radius: i32,
        reply: Sender<Result<Option<(i32, i32)>, String>>,
    },
}

/// A handle to the bounded vanilla terrain worker pool.
pub struct VanillaTerrain {
    jobs: Sender<Job>,
}

fn vanilla_root() -> PathBuf {
    std::env::var_os("FERRITE_VANILLA_ROOT").map_or_else(
        || std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3/generated"),
        PathBuf::from,
    )
}

impl VanillaTerrain {
    /// Loads the vanilla data and starts the bounded worker pool.
    ///
    /// Chunk columns are independent (the carver mask is seeded per chunk), so each
    /// worker keeps its own reusable generator set and pulls jobs from the shared
    /// bounded queue. The queue bounds memory under a burst of chunk requests.
    pub fn spawn(seed: i64) -> anyhow::Result<Self> {
        let root = vanilla_root();
        let data = Arc::new(
            WorldgenData::load(&root)
                .map_err(|error| anyhow::anyhow!("loading worldgen data: {error}"))?,
        );
        let registry = Arc::new(
            RuntimeRegistry::load(&root)
                .map_err(|error| anyhow::anyhow!("loading vanilla registry: {error}"))?,
        );
        let threads = worldgen_threads();
        let (jobs, receiver) = bounded::<Job>(threads * 8);
        for index in 0..threads {
            let data = Arc::clone(&data);
            let registry = Arc::clone(&registry);
            let receiver = receiver.clone();
            std::thread::Builder::new()
                .name(format!("ferrite-worldgen-{index}"))
                .spawn(move || worker(data, registry, seed, receiver))
                .map_err(|error| anyhow::anyhow!("starting worldgen worker: {error}"))?;
        }
        drop(receiver);
        Ok(Self { jobs })
    }

    /// Generates a full overworld column (noise, surface rules and carvers) off the region
    /// threads.
    pub fn generate(&self, x: i32, z: i32) -> anyhow::Result<Generated> {
        self.generate_dimension(super::Dimension::Overworld, x, z)
    }

    /// Generates a full column for one dimension. The nether uses its own noise settings,
    /// material rule and biome preset; the end has no generator yet.
    pub fn generate_dimension(
        &self,
        dimension: super::Dimension,
        x: i32,
        z: i32,
    ) -> anyhow::Result<Generated> {
        let (reply, result) = bounded(1);
        self.jobs
            .send(Job::Chunk {
                dimension,
                x,
                z,
                reply,
            })
            .map_err(|_| anyhow::anyhow!("worldgen thread stopped"))?;
        result
            .recv()
            .map_err(|_| anyhow::anyhow!("worldgen reply lost"))?
            .map_err(anyhow::Error::msg)
    }

    /// The first free block above the generated overworld surface at `(x, z)`.
    pub fn spawn_height(&self, x: i32, z: i32) -> anyhow::Result<i32> {
        self.spawn_height_dimension(super::Dimension::Overworld, x, z)
    }

    /// The first free block above `dimension`'s generated surface at `(x, z)`.
    pub fn spawn_height_dimension(
        &self,
        dimension: super::Dimension,
        x: i32,
        z: i32,
    ) -> anyhow::Result<i32> {
        let (reply, result) = bounded(1);
        self.jobs
            .send(Job::Height {
                dimension,
                x,
                z,
                reply,
            })
            .map_err(|_| anyhow::anyhow!("worldgen thread stopped"))?;
        result
            .recv()
            .map_err(|_| anyhow::anyhow!("worldgen reply lost"))?
            .map_err(anyhow::Error::msg)
    }

    /// The nearest block position whose surface biome is `target`, searched in 32-block rings
    /// like vanilla's `/locate biome`.
    pub fn locate_biome(
        &self,
        target: &str,
        x: i32,
        z: i32,
        radius: i32,
    ) -> anyhow::Result<Option<(i32, i32)>> {
        let (reply, result) = bounded(1);
        self.jobs
            .send(Job::LocateBiome {
                target: target.to_owned(),
                x,
                z,
                radius,
                reply,
            })
            .map_err(|_| anyhow::anyhow!("worldgen thread stopped"))?;
        result
            .recv()
            .map_err(|_| anyhow::anyhow!("worldgen reply lost"))?
            .map_err(anyhow::Error::msg)
    }
}

fn worker(data: Arc<WorldgenData>, registry: Arc<RuntimeRegistry>, seed: i64, jobs: Receiver<Job>) {
    let noise = match NoiseChunkGenerator::new(&data, seed) {
        Ok(noise) => noise,
        Err(error) => {
            for job in jobs {
                let message = format!("creating the overworld generator: {error}");
                match job {
                    Job::Chunk { reply, .. } => {
                        let _ = reply.send(Err(message.clone()));
                    }
                    Job::Height { reply, .. } => {
                        let _ = reply.send(Err(message.clone()));
                    }
                    Job::LocateBiome { reply, .. } => {
                        let _ = reply.send(Err(message.clone()));
                    }
                }
            }
            return;
        }
    };
    // One generator set per worker: building the surface rules costs ~16 ms and the
    // feature placement tables ~2 ms, so they are reused for every chunk instead of
    // being rebuilt inside `generate`.
    let surface = match SurfaceGenerator::new(&data, &noise, seed) {
        Ok(source) => Some(source),
        Err(error) => {
            tracing::error!(%error, "creating the worldgen biome source failed");
            None
        }
    };
    let mut features = FeatureGenerator::new(&data, seed, noise.min_y(), noise.height());
    // The nether has its own noise settings, material rule and biome preset. It is optional
    // so a data set without the nether settings still serves the overworld; a nether chunk
    // request then fails with the construction error instead of silently generating stone.
    let nether_noise = NoiseChunkGenerator::with_settings(&data, seed, "minecraft:nether")
        .map_err(|error| error.to_string());
    let nether_surface = nether_noise.as_ref().ok().and_then(|noise| {
        SurfaceGenerator::with_biome_preset(&data, noise, seed, "minecraft:nether")
            .map_err(|error| {
                tracing::error!(%error, "creating the nether biome source failed");
            })
            .ok()
    });
    let mut nether_features = nether_noise
        .as_ref()
        .ok()
        .map(|noise| FeatureGenerator::new(&data, seed, noise.min_y(), noise.height()));
    for job in jobs {
        match job {
            Job::LocateBiome {
                target,
                x,
                z,
                radius,
                reply,
            } => {
                let result = match &surface {
                    Some(source) => Ok(locate_biome_on_worker(source, &target, x, z, radius)),
                    None => Err("the overworld biome source is unavailable".into()),
                };
                let _ = reply.send(result);
            }
            Job::Chunk {
                dimension,
                x,
                z,
                reply,
            } => {
                let result = match dimension {
                    super::Dimension::Overworld => match &surface {
                        Some(source) => generate(
                            &data,
                            &registry,
                            &noise,
                            source,
                            &mut features,
                            dimension,
                            x,
                            z,
                        ),
                        None => Err("the overworld surface generator is unavailable".into()),
                    },
                    super::Dimension::Nether => {
                        match (&nether_noise, &nether_surface, nether_features.as_mut()) {
                            (Ok(noise), Some(surface), Some(features)) => generate(
                                &data, &registry, noise, surface, features, dimension, x, z,
                            ),
                            (Err(error), _, _) => {
                                Err(format!("creating the nether generator: {error}"))
                            }
                            _ => Err("the nether surface generator is unavailable".into()),
                        }
                    }
                    super::Dimension::End => Err("the end generator is not implemented yet".into()),
                };
                let _ = reply.send(result);
            }
            Job::Height {
                dimension,
                x,
                z,
                reply,
            } => {
                let result = match dimension {
                    super::Dimension::Overworld => match &surface {
                        Some(source) => generate(
                            &data,
                            &registry,
                            &noise,
                            source,
                            &mut features,
                            dimension,
                            x,
                            z,
                        ),
                        None => Err("the overworld surface generator is unavailable".into()),
                    },
                    super::Dimension::Nether => {
                        match (&nether_noise, &nether_surface, nether_features.as_mut()) {
                            (Ok(noise), Some(surface), Some(features)) => generate(
                                &data, &registry, noise, surface, features, dimension, x, z,
                            ),
                            (Err(error), _, _) => {
                                Err(format!("creating the nether generator: {error}"))
                            }
                            _ => Err("the nether surface generator is unavailable".into()),
                        }
                    }
                    super::Dimension::End => Err("the end generator is not implemented yet".into()),
                }
                .map(|column| {
                    let local_x = x.rem_euclid(16) as u8;
                    let local_z = z.rem_euclid(16) as u8;
                    let height = column
                        .chunk
                        .height(HeightmapKind::WorldSurface, local_x, local_z)
                        .unwrap_or(0) as i32;
                    let y = column.chunk.min_y() + height;
                    if height == 0 {
                        column.chunk.min_y() + column.chunk.height_blocks() as i32 / 2
                    } else {
                        y
                    }
                });
                let _ = reply.send(result);
            }
        }
    }
}

/// Ring search for `/locate biome`: vanilla steps 32 blocks per sample and stops at the first
/// ring with a match, so the returned block is the closest sampled one.
fn locate_biome_on_worker(
    surface: &SurfaceGenerator<'_>,
    target: &str,
    x: i32,
    z: i32,
    radius: i32,
) -> Option<(i32, i32)> {
    const STEP: i32 = 32;
    let rings = radius.max(0) / STEP;
    for ring in 0..=rings {
        let mut best: Option<(i32, i32, i64)> = None;
        for dx in -ring..=ring {
            for dz in -ring..=ring {
                if ring > 0 && dx.abs() != ring && dz.abs() != ring {
                    continue;
                }
                let sample_x = x.saturating_add(dx.saturating_mul(STEP));
                let sample_z = z.saturating_add(dz.saturating_mul(STEP));
                if surface.biome_at(sample_x, 64, sample_z).to_string() != target {
                    continue;
                }
                let distance = i64::from(sample_x - x).pow(2) + i64::from(sample_z - z).pow(2);
                if best.is_none_or(|(_, _, best_distance)| distance < best_distance) {
                    best = Some((sample_x, sample_z, distance));
                }
            }
        }
        if let Some((x, z, _)) = best {
            return Some((x, z));
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn generate(
    data: &WorldgenData,
    registry: &RuntimeRegistry,
    noise: &NoiseChunkGenerator,
    surface: &SurfaceGenerator<'_>,
    features: &mut FeatureGenerator,
    dimension: super::Dimension,
    x: i32,
    z: i32,
) -> Result<Generated, String> {
    let mut timings = StageTimings::default();
    let mark = std::time::Instant::now();
    let mut chunk = noise
        .fill_noise_chunk(x, z, registry)
        .map_err(|error| format!("noise fill for chunk {x},{z}: {error}"))?;
    timings.noise_ms = mark.elapsed().as_secs_f64() * 1000.0;
    let mark = std::time::Instant::now();
    surface
        .apply(&mut chunk, x, z, registry)
        .map_err(|error| format!("surface for chunk {x},{z}: {error}"))?;
    timings.surface_ms = mark.elapsed().as_secs_f64() * 1000.0;
    let mark = std::time::Instant::now();
    noise
        .carve_chunk(&mut chunk, x, z, registry)
        .map_err(|error| format!("carving chunk {x},{z}: {error}"))?;
    timings.carve_ms = mark.elapsed().as_secs_f64() * 1000.0;
    let mark = std::time::Instant::now();
    fill_biomes(&mut chunk, surface, registry, x, z)
        .map_err(|error| format!("biomes for chunk {x},{z}: {error}"))?;
    timings.biomes_ms = mark.elapsed().as_secs_f64() * 1000.0;
    let _ = data;
    let mark = std::time::Instant::now();
    features
        .decorate(
            &mut chunk,
            x,
            z,
            &|x, y, z| surface.biome_at(x, y, z),
            registry,
        )
        .map_err(|error| format!("features for chunk {x},{z}: {error}"))?;
    timings.features_ms = mark.elapsed().as_secs_f64() * 1000.0;
    // The nether noise settings fill only y=0..128, but the dimension is 256 blocks tall.
    // Pad the column to the dimension's declared range, as a vanilla 26.3 chunk is saved.
    let chunk = fit_vertical(chunk, dimension, registry)?;
    let mark = std::time::Instant::now();
    let light = if dimension.has_skylight() {
        sky_light(&chunk, registry)
    } else {
        // A sky without skylight (the nether) has no sky-light data at all.
        let count = chunk.sections().len() + 2;
        LightData {
            sky: vec![LightSection::Empty; count],
            block: vec![LightSection::Empty; count],
        }
    };
    timings.light_ms = mark.elapsed().as_secs_f64() * 1000.0;
    Ok(Generated {
        chunk,
        light,
        timings,
    })
}

/// Resizes a generated column to the dimension's vertical range, keeping every block and
/// biome the generator produced.
fn fit_vertical(
    chunk: Chunk,
    dimension: super::Dimension,
    registry: &RuntimeRegistry,
) -> Result<Chunk, String> {
    let min_y = dimension.min_y();
    let height = dimension.height() as u32;
    if chunk.min_y() == min_y && chunk.height_blocks() == height {
        return Ok(chunk);
    }
    let air = registry
        .block_id("minecraft:air", &Default::default())
        .ok_or_else(|| "missing air".to_string())?;
    let biome = chunk
        .sections()
        .first()
        .and_then(|section| section.biomes().get(0))
        .unwrap_or(0);
    let mut out = Chunk::new(
        min_y,
        height,
        air,
        biome,
        registry.block_bits(),
        registry.biome_bits(),
    )
    .map_err(|error| format!("resizing the chunk: {error}"))?;
    for y in chunk.min_y()..chunk.min_y() + chunk.height_blocks() as i32 {
        for z in 0..16u8 {
            for x in 0..16u8 {
                let Some(state) = chunk.block(x, y, z) else {
                    continue;
                };
                if state != air {
                    out.set_block(x, y, z, state, registry)
                        .map_err(|error| format!("copying the chunk: {error}"))?;
                }
            }
        }
    }
    for (index, section) in chunk.sections().iter().enumerate() {
        if index >= out.sections().len() {
            break;
        }
        for cell in 0..64 {
            if let Some(value) = section.biomes().get(cell) {
                out.sections_mut()[index]
                    .set_biome(cell, value)
                    .map_err(|error| format!("copying the biome cells: {error}"))?;
            }
        }
    }
    out.recompute_heightmaps(registry);
    Ok(out)
}

/// Writes vanilla `MultiNoiseBiomeSource` selections into the chunk's 4x4x4 biome cells.
fn fill_biomes(
    chunk: &mut Chunk,
    surface: &SurfaceGenerator<'_>,
    registry: &RuntimeRegistry,
    chunk_x: i32,
    chunk_z: i32,
) -> Result<(), String> {
    let min_y = chunk.min_y();
    // 1,536 biome cells per chunk return a handful of distinct ResourceIds; resolve each
    // name once instead of allocating a string and hashing it for every cell.
    let mut ids: std::collections::HashMap<_, _> = std::collections::HashMap::new();
    for (section_index, section) in chunk.sections_mut().iter_mut().enumerate() {
        for qy in 0..4 {
            let y = min_y + section_index as i32 * 16 + qy * 4 + 2;
            for qz in 0..4 {
                let z = chunk_z * 16 + qz * 4 + 2;
                for qx in 0..4 {
                    let x = chunk_x * 16 + qx * 4 + 2;
                    let name = surface.biome_at(x, y, z);
                    let id = match ids.get(&name) {
                        Some(id) => *id,
                        None => {
                            let id = registry
                                .biome_id(&name.to_string())
                                .ok_or_else(|| format!("unknown biome {name}"))?;
                            ids.insert(name.clone(), id);
                            id
                        }
                    };
                    section
                        .set_biome((qy * 16 + qz * 4 + qx) as usize, id)
                        .map_err(|error| format!("setting biome: {error}"))?;
                }
            }
        }
    }
    Ok(())
}

/// A conservative sky-light pass: 15 from the top, one level lost per water or
/// leaf block, and zero below the first opaque block. Vanilla also propagates
/// light horizontally; the client's first relight smooths the section edges.
fn sky_light(chunk: &Chunk, registry: &RuntimeRegistry) -> LightData {
    let min_y = chunk.min_y();
    let height = chunk.height_blocks() as usize;
    let mut values = vec![0u8; height * 256];
    for z in 0..16u8 {
        for x in 0..16u8 {
            let mut level = 15u8;
            for y in (min_y..min_y + height as i32).rev() {
                let state = chunk.block(x, y, z).unwrap_or(0);
                let flags = registry.flags(state);
                if flags.motion_blocking && !flags.leaves {
                    level = 0;
                } else if flags.fluid || flags.leaves {
                    level = level.saturating_sub(1);
                }
                let index = (y - min_y) as usize * 256 + usize::from(z) * 16 + usize::from(x);
                values[index] = level;
            }
        }
    }
    let count = chunk.sections().len() + 2;
    let mut sky = Vec::with_capacity(count);
    sky.push(LightSection::Empty);
    for section in 0..chunk.sections().len() {
        let mut data = Box::new([0u8; 2048]);
        let mut any = false;
        for local_y in 0..16 {
            for z in 0..16 {
                for x in 0..16 {
                    let value = values[(section * 16 + local_y) * 256 + z * 16 + x];
                    if value == 0 {
                        continue;
                    }
                    any = true;
                    let index = local_y * 256 + z * 16 + x;
                    data[index / 2] |= value << ((index & 1) * 4);
                }
            }
        }
        sky.push(if any {
            LightSection::Data(data)
        } else {
            LightSection::Empty
        });
    }
    sky.push(LightSection::Empty);
    LightData {
        sky,
        block: vec![LightSection::Empty; count],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locate_biome_finds_a_common_biome() {
        let terrain = match VanillaTerrain::spawn(0) {
            Ok(terrain) => terrain,
            Err(error) => {
                eprintln!("skipping worldgen test: {error}");
                return;
            }
        };
        let found = [
            "minecraft:ocean",
            "minecraft:plains",
            "minecraft:forest",
            "minecraft:desert",
        ]
        .iter()
        .any(|target| matches!(terrain.locate_biome(target, 0, 0, 256), Ok(Some(_))));
        assert!(found, "no common biome within 256 blocks of the origin");
    }

    #[test]
    fn generates_overworld_terrain_with_surface_and_biomes() {
        let terrain = match VanillaTerrain::spawn(0) {
            Ok(terrain) => terrain,
            Err(error) => {
                eprintln!("skipping worldgen test: {error}");
                return;
            }
        };
        let generated = terrain.generate(0, 0).unwrap();
        let registry = RuntimeRegistry::load(vanilla_root()).unwrap();
        let bedrock = registry
            .block_id("minecraft:bedrock", &Default::default())
            .unwrap();
        let mut saw_solid = false;
        let mut saw_bedrock = false;
        let mut names = Vec::new();
        for y in -64..=-40 {
            let block = generated.chunk.block(8, y, 8).unwrap();
            saw_solid |= registry.flags(block).motion_blocking;
            saw_bedrock |= block == bedrock;
            names.push(
                registry
                    .block_state(block)
                    .map_or("unknown", |state| state.name.as_str())
                    .to_owned(),
            );
        }
        assert!(saw_solid, "generated column has no solid blocks: {names:?}");
        assert!(
            saw_bedrock,
            "generated column has no bedrock floor: {names:?}"
        );
        let surface = generated
            .chunk
            .height(HeightmapKind::WorldSurface, 8, 8)
            .unwrap();
        assert!(surface > 0, "generated column has no surface");
        assert_eq!(
            generated.light.sky.len(),
            generated.chunk.sections().len() + 2
        );
        let biome = generated.chunk.sections()[0].biomes().get(0).unwrap();
        assert!(registry.biome_name(biome).is_some());
    }
}
