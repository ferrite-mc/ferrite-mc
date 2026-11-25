//! Noise-based chunk generator using 4x8x4 trilinear density interpolation.

use std::sync::Arc;

use ferrite_world::{BlockInfo, Chunk};

use crate::biome::{ClimateSampler, EndBiomeSource, MultiNoiseBiomeSource};
use crate::density::{Axis, BinaryOp, DensityFunction, EvalCache, FunctionContext, NoiseMap};
use crate::noise::{NormalNoise, SimplexNoise};
use crate::random::XoroshiroRandom;

thread_local! {
    static EVAL_CACHE_POOL: std::cell::RefCell<EvalCache> =
        std::cell::RefCell::new(EvalCache::new(0));
}

struct EvalCacheGuard(Option<EvalCache>);

impl std::ops::Deref for EvalCacheGuard {
    type Target = EvalCache;
    fn deref(&self) -> &Self::Target {
        self.0.as_ref().expect("EvalCache present")
    }
}

impl std::ops::DerefMut for EvalCacheGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut().expect("EvalCache present")
    }
}

impl Drop for EvalCacheGuard {
    fn drop(&mut self) {
        if let Some(cache) = self.0.take() {
            let _ = EVAL_CACHE_POOL.try_with(|cell| {
                if let Ok(mut slot) = cell.try_borrow_mut() {
                    *slot = cache;
                }
            });
        }
    }
}

fn take_eval_cache(count: usize) -> EvalCacheGuard {
    let mut cache = EVAL_CACHE_POOL
        .try_with(|cell| {
            cell.try_borrow_mut()
                .map(|mut slot| std::mem::replace(&mut *slot, EvalCache::new(0)))
                .unwrap_or_else(|_| EvalCache::new(count))
        })
        .unwrap_or_else(|_| EvalCache::new(count));
    cache.reset(count);
    EvalCacheGuard(Some(cache))
}

fn with_eval_cache<R>(count: usize, f: impl FnOnce(&mut EvalCache) -> R) -> R {
    let mut guard = take_eval_cache(count);
    f(&mut guard)
}

/// Target Minecraft dimension rules for chunk height, bedrock, fluid, and surface placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DimensionKind {
    #[default]
    Overworld,
    Nether,
    End,
}

/// Block state IDs needed by the base terrain & surface pass.
#[derive(Debug, Clone, Copy, Default)]
pub struct TerrainPalette {
    pub air: u16,
    pub stone: u16,
    pub deepslate: u16,
    pub bedrock: u16,
    pub water: u16,
    pub grass_block: u16,
    pub dirt: u16,
    pub sand: u16,
    pub netherrack: u16,
    pub lava: u16,
    pub end_stone: u16,
    pub red_sand: u16,
    pub terracotta: u16,
    pub snow_block: u16,
    pub gravel: u16,
    pub podzol: u16,
    pub mycelium: u16,
    pub soul_sand: u16,
    pub crimson_nylium: u16,
    pub warped_nylium: u16,
    pub blackstone: u16,
    pub basalt: u16,
}

/// Pre-resolved biome ID sets used by the fast surface pass.
#[derive(Debug, Clone, Default)]
pub struct SurfaceBiomeIds {
    pub sandy: Vec<u32>,
    pub badlands: Vec<u32>,
    pub snowy: Vec<u32>,
    pub podzol: Vec<u32>,
    pub mycelium: Vec<u32>,
    pub crimson: Vec<u32>,
    pub warped: Vec<u32>,
    pub soul_sand: Vec<u32>,
    pub basalt: Vec<u32>,
}

impl SurfaceBiomeIds {
    #[inline]
    fn has_nether(&self) -> bool {
        !(self.crimson.is_empty()
            && self.warped.is_empty()
            && self.soul_sand.is_empty()
            && self.basalt.is_empty())
    }
}

/// Configuration for a noise-based dimension (e.g. Overworld `-64..320`, sea level `63`).
#[derive(Debug, Clone)]
pub struct NoiseGeneratorSettings {
    pub dimension: DimensionKind,
    pub min_y: i32,
    pub height: i32,
    pub gen_height: i32,
    pub sea_level: i32,
    pub cell_w: usize,
    pub cell_h: usize,
    pub final_density: DensityFunction,
    pub climate: ClimateSampler,
    pub biome_source: MultiNoiseBiomeSource,
    pub end_biomes: Option<EndBiomeSource>,
    pub surface_biomes: SurfaceBiomeIds,
    pub palette: TerrainPalette,
}

/// Vanilla-style noise chunk generator with cell density interpolation.
#[derive(Debug, Clone)]
pub struct NoiseChunkGenerator {
    pub seed: u64,
    pub noises: NoiseMap,
    pub cache_count: usize,
    pub settings: NoiseGeneratorSettings,
}

impl NoiseChunkGenerator {
    /// Creates a default Overworld-like generator for `seed` with the given `palette`.
    pub fn overworld(
        seed: u64,
        palette: TerrainPalette,
        biome_source: MultiNoiseBiomeSource,
    ) -> Self {
        let mut rng = XoroshiroRandom::from_seed(seed);
        let mut noises = NoiseMap::new();
        for (name, first_oct, amps) in [
            (
                "minecraft:temperature",
                -10,
                &[1.5, 0.0, 1.0, 0.0, 0.0, 0.0][..],
            ),
            (
                "minecraft:vegetation",
                -8,
                &[1.0, 1.0, 0.0, 0.0, 0.0, 0.0][..],
            ),
            (
                "minecraft:continentalness",
                -9,
                &[1.0, 1.0, 2.0, 2.0, 2.0, 1.0, 1.0, 1.0, 1.0][..],
            ),
            ("minecraft:erosion", -9, &[1.0, 1.0, 0.0, 1.0, 1.0][..]),
            ("minecraft:ridge", -7, &[1.0, 2.0, 1.0, 0.0, 0.0, 0.0][..]),
            (
                "minecraft:jagged",
                -16,
                &[
                    1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0,
                ][..],
            ),
        ] {
            let mut n_rng = rng.fork();
            noises.insert(
                name.to_string(),
                Arc::new(NormalNoise::new(&mut n_rng, first_oct, amps)),
            );
        }

        let continents = DensityFunction::Noise {
            noise_id: "minecraft:continentalness".to_string(),
            noise: None,
            xz_scale: 0.25,
            y_scale: 0.0,
            shift_x: None,
            shift_z: None,
        };
        let y_grad = DensityFunction::Gradient {
            axis: Axis::Y,
            from_coordinate: -64,
            to_coordinate: 320,
            from_value: 1.5,
            to_value: -1.5,
        };
        let mut final_density = DensityFunction::Binary {
            op: BinaryOp::Add,
            arg1: Box::new(y_grad),
            arg2: Box::new(continents.clone()),
        };

        let mut climate = ClimateSampler {
            temperature: DensityFunction::Noise {
                noise_id: "minecraft:temperature".to_string(),
                noise: None,
                xz_scale: 0.25,
                y_scale: 0.0,
                shift_x: None,
                shift_z: None,
            },
            humidity: DensityFunction::Noise {
                noise_id: "minecraft:vegetation".to_string(),
                noise: None,
                xz_scale: 0.25,
                y_scale: 0.0,
                shift_x: None,
                shift_z: None,
            },
            continentalness: continents,
            erosion: DensityFunction::Noise {
                noise_id: "minecraft:erosion".to_string(),
                noise: None,
                xz_scale: 0.25,
                y_scale: 0.0,
                shift_x: None,
                shift_z: None,
            },
            depth: DensityFunction::Gradient {
                axis: Axis::Y,
                from_coordinate: -64,
                to_coordinate: 320,
                from_value: 1.5,
                to_value: -1.5,
            },
            weirdness: DensityFunction::Noise {
                noise_id: "minecraft:ridge".to_string(),
                noise: None,
                xz_scale: 0.25,
                y_scale: 0.0,
                shift_x: None,
                shift_z: None,
            },
        };
        final_density.bind_noises(&noises);
        climate.bind_noises(&noises);

        Self {
            seed,
            noises,
            cache_count: 0,
            settings: NoiseGeneratorSettings {
                dimension: DimensionKind::Overworld,
                min_y: -64,
                height: 384,
                gen_height: 384,
                sea_level: 63,
                cell_w: 4,
                cell_h: 8,
                final_density,
                climate,
                biome_source,
                end_biomes: None,
                surface_biomes: SurfaceBiomeIds::default(),
                palette,
            },
        }
    }

    /// Creates a fallback Nether-like generator when vanilla JSON is unavailable.
    pub fn nether(seed: u64, palette: TerrainPalette, biome_source: MultiNoiseBiomeSource) -> Self {
        let mut rng = XoroshiroRandom::from_seed(seed);
        let mut noises = NoiseMap::new();
        for (name, oct) in [
            ("minecraft:nether/base_3d_noise", -7),
            ("minecraft:nether/temperature", -7),
            ("minecraft:nether/vegetation", -7),
        ] {
            let mut n_rng = rng.fork();
            noises.insert(
                name.to_string(),
                Arc::new(NormalNoise::new(&mut n_rng, oct, &[1.0, 1.0, 0.5])),
            );
        }
        let ceiling = DensityFunction::Gradient {
            axis: Axis::Y,
            from_coordinate: 104,
            to_coordinate: 128,
            from_value: 1.0,
            to_value: 0.0,
        };
        let floor = DensityFunction::Gradient {
            axis: Axis::Y,
            from_coordinate: -8,
            to_coordinate: 24,
            from_value: 0.0,
            to_value: 1.0,
        };
        let mut final_density = DensityFunction::Lerp {
            alpha: Box::new(floor),
            first: Box::new(DensityFunction::Constant(2.5)),
            second: Box::new(DensityFunction::Lerp {
                alpha: Box::new(ceiling),
                first: Box::new(DensityFunction::Constant(0.9375)),
                second: Box::new(DensityFunction::Noise {
                    noise_id: "minecraft:nether/base_3d_noise".to_string(),
                    noise: None,
                    xz_scale: 0.25,
                    y_scale: 0.25,
                    shift_x: None,
                    shift_z: None,
                }),
            }),
        };
        let mut climate = ClimateSampler {
            temperature: DensityFunction::Noise {
                noise_id: "minecraft:nether/temperature".to_string(),
                noise: None,
                xz_scale: 0.25,
                y_scale: 0.0,
                shift_x: None,
                shift_z: None,
            },
            humidity: DensityFunction::Noise {
                noise_id: "minecraft:nether/vegetation".to_string(),
                noise: None,
                xz_scale: 0.25,
                y_scale: 0.0,
                shift_x: None,
                shift_z: None,
            },
            continentalness: DensityFunction::Constant(0.0),
            erosion: DensityFunction::Constant(0.0),
            depth: DensityFunction::Constant(0.0),
            weirdness: DensityFunction::Constant(0.0),
        };
        final_density.bind_noises(&noises);
        climate.bind_noises(&noises);
        Self {
            seed,
            noises,
            cache_count: 0,
            settings: NoiseGeneratorSettings {
                dimension: DimensionKind::Nether,
                min_y: 0,
                height: 256,
                gen_height: 128,
                sea_level: 32,
                cell_w: 4,
                cell_h: 8,
                final_density,
                climate,
                biome_source,
                end_biomes: None,
                surface_biomes: SurfaceBiomeIds::default(),
                palette,
            },
        }
    }

    /// Creates a fallback End-like generator when vanilla JSON is unavailable.
    pub fn end(seed: u64, palette: TerrainPalette, end_biomes: EndBiomeSource) -> Self {
        let mut rng = XoroshiroRandom::from_seed(seed);
        let simplex = Arc::new(SimplexNoise::new(&mut rng));
        let islands = DensityFunction::EndOuterIslands(simplex);
        let y_grad = DensityFunction::Gradient {
            axis: Axis::Y,
            from_coordinate: 0,
            to_coordinate: 128,
            from_value: 0.6,
            to_value: -0.9,
        };
        let final_density = DensityFunction::Binary {
            op: BinaryOp::Add,
            arg1: Box::new(y_grad),
            arg2: Box::new(islands.clone()),
        };
        let climate = ClimateSampler {
            temperature: DensityFunction::Constant(0.0),
            humidity: DensityFunction::Constant(0.0),
            continentalness: DensityFunction::Constant(0.0),
            erosion: islands,
            depth: DensityFunction::Constant(0.0),
            weirdness: DensityFunction::Constant(0.0),
        };
        Self {
            seed,
            noises: NoiseMap::new(),
            cache_count: 0,
            settings: NoiseGeneratorSettings {
                dimension: DimensionKind::End,
                min_y: 0,
                height: 256,
                gen_height: 128,
                sea_level: 0,
                cell_w: 8,
                cell_h: 4,
                final_density,
                climate,
                biome_source: MultiNoiseBiomeSource::new(Vec::new()),
                end_biomes: Some(end_biomes),
                surface_biomes: SurfaceBiomeIds::default(),
                palette,
            },
        }
    }

    #[inline]
    fn biome_from_sample(&self, wx: i32, wz: i32, target: [f32; 6]) -> u32 {
        if let Some(end_biomes) = &self.settings.end_biomes {
            end_biomes.find_biome(wx, wz, target[3])
        } else {
            self.settings.biome_source.find_biome(target)
        }
    }

    /// Returns the biome registry ID at block coordinates `(wx, wy, wz)`.
    pub fn biome_id_at(&self, wx: i32, wy: i32, wz: i32) -> u32 {
        with_eval_cache(self.cache_count, |cache| {
            let target = self.settings.climate.sample_cached(
                FunctionContext::new(wx, wy, wz),
                &self.noises,
                cache,
            );
            self.biome_from_sample(wx, wz, target)
        })
    }

    /// Searches outward in 32-block steps up to `radius` for a biome matching `target_ids`.
    pub fn locate_biome(
        &self,
        target_ids: &[u32],
        origin_x: i32,
        origin_y: i32,
        origin_z: i32,
        radius: i32,
    ) -> Option<(i32, i32, i32)> {
        if target_ids.is_empty() {
            return None;
        }
        let step = 32;
        let max_ring = radius.max(0) / step;
        with_eval_cache(self.cache_count, |cache| {
            for ring in 0..=max_ring {
                for dz in -ring..=ring {
                    for dx in -ring..=ring {
                        if ring > 0 && dx.abs() != ring && dz.abs() != ring {
                            continue;
                        }
                        let wx = origin_x + dx * step;
                        let wz = origin_z + dz * step;
                        if (wx - origin_x).abs() > radius || (wz - origin_z).abs() > radius {
                            continue;
                        }
                        let target = self.settings.climate.sample_cached(
                            FunctionContext::new(wx, origin_y, wz),
                            &self.noises,
                            cache,
                        );
                        let id = self.biome_from_sample(wx, wz, target);
                        if target_ids.contains(&id) {
                            return Some((wx, origin_y, wz));
                        }
                    }
                }
            }
            None
        })
    }

    /// Generates a full chunk at `(chunk_x, chunk_z)` using cell trilinear density interpolation.
    pub fn generate_chunk(&self, chunk_x: i32, chunk_z: i32, info: &impl BlockInfo) -> Chunk {
        thread_local! {
            static GEN_BLOCKS: std::cell::RefCell<Vec<u16>> = const { std::cell::RefCell::new(Vec::new()) };
        }
        let mut blocks = GEN_BLOCKS.with(|cell| std::mem::take(&mut *cell.borrow_mut()));
        let (mut chunk, active_sections) =
            self.generate_chunk_into_u16(chunk_x, chunk_z, &mut blocks);
        for sy in 0..active_sections {
            let _ =
                chunk.sections_mut()[sy].set_blocks_u16(&blocks[sy * 4096..(sy + 1) * 4096], info);
        }
        chunk.compute_heightmaps_from_u16(&blocks[..active_sections * 4096], info);
        GEN_BLOCKS.with(|cell| {
            *cell.borrow_mut() = blocks;
        });
        chunk
    }

    /// Populates biomes on a fresh [`Chunk`] and writes raw terrain + surface block state IDs into
    /// `blocks` (`blocks[y_idx * 256 + lz * 16 + lx]`) without packing block sections or computing
    /// heightmaps yet. Returns `(chunk, active_sections)`.
    pub fn generate_chunk_into_u16(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        blocks: &mut Vec<u16>,
    ) -> (Chunk, usize) {
        let min_y = self.settings.min_y;
        let chunk_height = self.settings.height;
        let gen_height = self.settings.gen_height.min(chunk_height);
        let mut chunk = Chunk::new(
            min_y,
            chunk_height as u32,
            self.settings.palette.air as u32,
            0,
            16,
            6,
        )
        .expect("valid chunk dimensions");

        let cell_w = self.settings.cell_w.max(1);
        let cell_h = self.settings.cell_h.max(1);
        let cells_x = 16 / cell_w + 1;
        let cells_z = 16 / cell_w + 1;
        let cells_y = (gen_height as usize) / cell_h + 1;

        let base_x = chunk_x * 16;
        let base_z = chunk_z * 16;

        // Sample density at cell corners (column-major so 2D caches stay hot across all Y).
        let mut eval_cache = take_eval_cache(self.cache_count);
        thread_local! {
            static GEN_CORNERS: std::cell::RefCell<Vec<f64>> = const { std::cell::RefCell::new(Vec::new()) };
        }
        let corner_count = cells_x * cells_y * cells_z;
        let mut corners = GEN_CORNERS.with(|cell| std::mem::take(&mut *cell.borrow_mut()));
        corners.clear();
        corners.resize(corner_count, 0.0);
        let idx = |cx: usize, cy: usize, cz: usize| (cy * cells_z + cz) * cells_x + cx;

        if let Some((primary, secondary)) = self.settings.final_density.as_top_level_min()
            && !secondary.has_cached_nodes()
        {
            for cz in 0..cells_z {
                let wz = base_z + (cz * cell_w) as i32;
                for cx in 0..cells_x {
                    let wx = base_x + (cx * cell_w) as i32;
                    for cy in 0..cells_y {
                        let wy = min_y + (cy * cell_h) as i32;
                        corners[idx(cx, cy, cz)] = primary.compute_cached_clamped(
                            FunctionContext::new(wx, wy, wz),
                            &self.noises,
                            &mut eval_cache,
                            f64::NEG_INFINITY,
                            f64::INFINITY,
                        );
                    }
                }
            }
            for cz in 0..cells_z {
                let wz = base_z + (cz * cell_w) as i32;
                let z0 = cz.saturating_sub(1);
                let z1 = (cz + 1).min(cells_z - 1);
                for cx in 0..cells_x {
                    let wx = base_x + (cx * cell_w) as i32;
                    let x0 = cx.saturating_sub(1);
                    let x1 = (cx + 1).min(cells_x - 1);
                    for cy in 0..cells_y {
                        let i = idx(cx, cy, cz);
                        let mut near_pos = corners[i] > 0.0;
                        if !near_pos {
                            let y0 = cy.saturating_sub(1);
                            let y1 = (cy + 1).min(cells_y - 1);
                            'nbr: for ny in y0..=y1 {
                                for nz in z0..=z1 {
                                    for nx in x0..=x1 {
                                        if corners[idx(nx, ny, nz)] > 0.0 {
                                            near_pos = true;
                                            break 'nbr;
                                        }
                                    }
                                }
                            }
                        }
                        if near_pos {
                            let wy = min_y + (cy * cell_h) as i32;
                            let s =
                                secondary.compute(FunctionContext::new(wx, wy, wz), &self.noises);
                            if s < corners[i] {
                                corners[i] = s;
                            }
                        }
                    }
                }
            }
        } else {
            for cz in 0..cells_z {
                let wz = base_z + (cz * cell_w) as i32;
                for cx in 0..cells_x {
                    let wx = base_x + (cx * cell_w) as i32;
                    for cy in 0..cells_y {
                        let wy = min_y + (cy * cell_h) as i32;
                        corners[idx(cx, cy, cz)] =
                            self.settings.final_density.compute_cached_clamped(
                                FunctionContext::new(wx, wy, wz),
                                &self.noises,
                                &mut eval_cache,
                                f64::NEG_INFINITY,
                                f64::INFINITY,
                            );
                    }
                }
            }
        }

        let pal = self.settings.palette;
        let sea_level = self.settings.sea_level;
        let dim = self.settings.dimension;
        let top_gen_y = min_y + gen_height - 1;
        let mut max_solid_y = [min_y; 256];
        let total_blocks = (chunk_height as usize) * 256;
        blocks.clear();
        blocks.resize(total_blocks, pal.air);

        // Trilinearly interpolate within each cell and place base blocks (heightmaps updated once at end).
        for cy in 0..(cells_y - 1) {
            let cell_wy_base = min_y + (cy * cell_h) as i32;
            let cell_wy_end = cell_wy_base + cell_h as i32;
            for cz in 0..(cells_z - 1) {
                for cx in 0..(cells_x - 1) {
                    let c000 = corners[idx(cx, cy, cz)];
                    let c100 = corners[idx(cx + 1, cy, cz)];
                    let c010 = corners[idx(cx, cy + 1, cz)];
                    let c110 = corners[idx(cx + 1, cy + 1, cz)];
                    let c001 = corners[idx(cx, cy, cz + 1)];
                    let c101 = corners[idx(cx + 1, cy, cz + 1)];
                    let c011 = corners[idx(cx, cy + 1, cz + 1)];
                    let c111 = corners[idx(cx + 1, cy + 1, cz + 1)];

                    // Fast skip: if all 8 corners are <= 0 above sea level (and in End or Overworld), the cell is air.
                    let all_non_pos = c000 <= 0.0
                        && c100 <= 0.0
                        && c010 <= 0.0
                        && c110 <= 0.0
                        && c001 <= 0.0
                        && c101 <= 0.0
                        && c011 <= 0.0
                        && c111 <= 0.0;
                    if all_non_pos
                        && ((dim != DimensionKind::Nether && cell_wy_base >= sea_level)
                            || (dim == DimensionKind::Nether
                                && cell_wy_base > sea_level
                                && cell_wy_end <= top_gen_y))
                    {
                        continue;
                    }
                    // Fast fill: all-water cell below sea level in Overworld without trilinear math.
                    if dim == DimensionKind::Overworld
                        && all_non_pos
                        && cell_wy_base > min_y
                        && cell_wy_end <= sea_level
                    {
                        for dy in 0..cell_h {
                            let y_off = ((cell_wy_base + dy as i32 - min_y) as usize) * 256;
                            for dz in 0..cell_w {
                                let row = y_off + (cz * cell_w + dz) * 16 + cx * cell_w;
                                blocks[row..row + cell_w].fill(pal.water);
                            }
                        }
                        continue;
                    }
                    // Fast fill: all-solid underground/interior cell with uniform material.
                    let all_pos = c000 > 0.0
                        && c100 > 0.0
                        && c010 > 0.0
                        && c110 > 0.0
                        && c001 > 0.0
                        && c101 > 0.0
                        && c011 > 0.0
                        && c111 > 0.0;
                    if all_pos {
                        let solid_state = match dim {
                            DimensionKind::Overworld
                                if cell_wy_base > min_y && cell_wy_end <= 0 =>
                            {
                                Some(pal.deepslate)
                            }
                            DimensionKind::Overworld
                                if cell_wy_base > min_y && cell_wy_base >= 0 =>
                            {
                                Some(pal.stone)
                            }
                            DimensionKind::Nether
                                if cell_wy_base > min_y && cell_wy_end <= top_gen_y =>
                            {
                                Some(pal.netherrack)
                            }
                            DimensionKind::End => Some(pal.end_stone),
                            _ => None,
                        };
                        if let Some(fill_state) = solid_state {
                            for dy in 0..cell_h {
                                let y_off = ((cell_wy_base + dy as i32 - min_y) as usize) * 256;
                                for dz in 0..cell_w {
                                    let row = y_off + (cz * cell_w + dz) * 16 + cx * cell_w;
                                    blocks[row..row + cell_w].fill(fill_state);
                                }
                            }
                            if dim == DimensionKind::Overworld {
                                let top_y = cell_wy_end - 1;
                                for dz in 0..cell_w {
                                    let lz = cz * cell_w + dz;
                                    for dx in 0..cell_w {
                                        let col = lz * 16 + cx * cell_w + dx;
                                        if top_y > max_solid_y[col] {
                                            max_solid_y[col] = top_y;
                                        }
                                    }
                                }
                            }
                            continue;
                        }
                    }

                    let inv_h = 1.0 / cell_h as f64;
                    let inv_w = 1.0 / cell_w as f64;
                    let d_y00 = (c010 - c000) * inv_h;
                    let d_y10 = (c110 - c100) * inv_h;
                    let d_y01 = (c011 - c001) * inv_h;
                    let d_y11 = (c111 - c101) * inv_h;
                    let mut y00 = c000;
                    let mut y10 = c100;
                    let mut y01 = c001;
                    let mut y11 = c101;
                    for dy in 0..cell_h {
                        let wy = cell_wy_base + dy as i32;
                        let y_off = ((wy - min_y) as usize) * 256;
                        let d_z0 = (y01 - y00) * inv_w;
                        let d_z1 = (y11 - y10) * inv_w;
                        let mut z0 = y00;
                        let mut z1 = y10;
                        for dz in 0..cell_w {
                            let lz = cz * cell_w + dz;
                            let z_off = y_off + lz * 16;
                            let d_x = (z1 - z0) * inv_w;
                            let mut density = z0;
                            for dx in 0..cell_w {
                                let lx = cx * cell_w + dx;

                                let state = match dim {
                                    DimensionKind::Overworld => {
                                        let is_bedrock = wy == min_y
                                            || (wy < min_y + 5 && {
                                                let wx = base_x + lx as i32;
                                                let wz = base_z + lz as i32;
                                                let h = (wx as u64)
                                                    .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                                                    ^ (wy as u64)
                                                        .wrapping_mul(0xBF58_476D_1CE4_E5B9)
                                                    ^ (wz as u64)
                                                        .wrapping_mul(0x94D0_49BB_1331_11EB);
                                                let h = (h ^ (h >> 30))
                                                    .wrapping_mul(0xBF58_476D_1CE4_E5B9);
                                                ((h >> 32) % 5) < (min_y + 5 - wy) as u64
                                            });
                                        if is_bedrock {
                                            pal.bedrock
                                        } else if density > 0.0 {
                                            if wy < 0 { pal.deepslate } else { pal.stone }
                                        } else if wy < sea_level {
                                            pal.water
                                        } else {
                                            pal.air
                                        }
                                    }
                                    DimensionKind::Nether => {
                                        if wy == min_y || wy == top_gen_y {
                                            pal.bedrock
                                        } else if density > 0.0 {
                                            pal.netherrack
                                        } else if wy <= sea_level {
                                            pal.lava
                                        } else {
                                            pal.air
                                        }
                                    }
                                    DimensionKind::End => {
                                        if density > 0.0 {
                                            pal.end_stone
                                        } else {
                                            pal.air
                                        }
                                    }
                                };

                                if state != pal.air {
                                    blocks[z_off + lx] = state;
                                    if dim == DimensionKind::Overworld && state != pal.water {
                                        let col = lz * 16 + lx;
                                        if wy > max_solid_y[col] {
                                            max_solid_y[col] = wy;
                                        }
                                    }
                                }
                                density += d_x;
                            }
                            z0 += d_z0;
                            z1 += d_z1;
                        }
                        y00 += d_y00;
                        y10 += d_y10;
                        y01 += d_y01;
                        y11 += d_y11;
                    }
                }
            }
        }
        GEN_CORNERS.with(|cell| {
            let mut slot = cell.borrow_mut();
            if corners.capacity() > slot.capacity() {
                *slot = corners;
            }
        });

        // Underground noise caves below solid terrain must stay air (or deep lava at y <= -55),
        // never flooded to sea level like open ocean/river basins. Iterate in [y][z][x] memory
        // order so each Y-slice is a contiguous 256-entry L1-cache scan.
        if dim == DimensionKind::Overworld {
            let lava_or_air = |wy: i32| {
                if wy <= -55 && pal.lava != 0 {
                    pal.lava
                } else {
                    pal.air
                }
            };
            let cap_hi = sea_level.min(min_y + chunk_height);
            for wy in (min_y + 1)..cap_hi {
                let row_off = ((wy - min_y) as usize) * 256;
                let row = &mut blocks[row_off..row_off + 256];
                let replacement = lava_or_air(wy);
                for (cell, &top_y) in row.iter_mut().zip(max_solid_y.iter()) {
                    if *cell == pal.water && wy < top_y {
                        *cell = replacement;
                    }
                }
            }
        }

        // Populate 4x4x4 biome cells (fast 2D path when climate does not depend on Y) and
        // capture 4x4 surface biome ids for biome-aware surface material placement.
        let sections = (chunk_height as usize) / 16;
        let climate_varies_y = self.settings.climate.depends_on_y();
        let mut surface_biomes_4x4 = [0u32; 16];
        thread_local! {
            static GEN_BIOMES: std::cell::RefCell<Vec<[u32; 64]>> =
                const { std::cell::RefCell::new(Vec::new()) };
        }
        let mut section_biomes = GEN_BIOMES.with(|cell| std::mem::take(&mut *cell.borrow_mut()));
        section_biomes.clear();
        section_biomes.resize(sections, [0u32; 64]);

        if !climate_varies_y {
            let mut surf_sec = [0u32; 64];
            for bz in 0..4 {
                let wz = base_z + (bz * 4) as i32;
                for bx in 0..4 {
                    let wx = base_x + (bx * 4) as i32;
                    let target = self.settings.climate.sample_cached(
                        FunctionContext::new(wx, min_y + 64, wz),
                        &self.noises,
                        &mut eval_cache,
                    );
                    let biome_id = self.biome_from_sample(wx, wz, target);
                    surface_biomes_4x4[bz * 4 + bx] = biome_id;
                    for by in 0..4 {
                        surf_sec[(by * 4 + bz) * 4 + bx] = biome_id;
                    }
                }
            }
            section_biomes.fill(surf_sec);
        } else if !self.settings.climate.horizontal_channels_depend_on_y() {
            for bz in 0..4 {
                let wz = base_z + (bz * 4) as i32;
                for bx in 0..4 {
                    let wx = base_x + (bx * 4) as i32;
                    let base_2d = self.settings.climate.sample_2d_channels_cached(
                        FunctionContext::new(wx, sea_level + 4, wz),
                        &self.noises,
                        &mut eval_cache,
                    );
                    let col_zero = if self.settings.end_biomes.is_none() {
                        self.settings
                            .biome_source
                            .find_zero_depth_column_biome(base_2d)
                    } else {
                        None
                    };
                    let surf_target = self.settings.climate.sample_depth_with_2d(
                        FunctionContext::new(wx, sea_level + 4, wz),
                        &self.noises,
                        &mut eval_cache,
                        base_2d,
                    );
                    let surf_biome = if let Some((zero_biome, zero_dist_sq)) = col_zero {
                        self.settings
                            .biome_source
                            .find_biome_with_column_zero_depth(
                                surf_target,
                                zero_biome,
                                zero_dist_sq,
                            )
                    } else {
                        self.biome_from_sample(wx, wz, surf_target)
                    };
                    surface_biomes_4x4[bz * 4 + bx] = surf_biome;
                    for (sy, sy_biomes) in section_biomes.iter_mut().enumerate() {
                        let wy = min_y + (sy * 16) as i32;
                        // In Overworld above sea level, depth <= 0 everywhere so the surface biome
                        // holds across all 4 vertical biome cells in the section without cave queries.
                        if dim == DimensionKind::Overworld && wy >= sea_level + 4 {
                            for by in 0..4 {
                                sy_biomes[(by * 4 + bz) * 4 + bx] = surf_biome;
                            }
                            continue;
                        }
                        for by in 0..4 {
                            let target = self.settings.climate.sample_depth_with_2d(
                                FunctionContext::new(wx, wy + (by * 4) as i32, wz),
                                &self.noises,
                                &mut eval_cache,
                                base_2d,
                            );
                            let biome_id = if let Some((zero_biome, zero_dist_sq)) = col_zero {
                                self.settings
                                    .biome_source
                                    .find_biome_with_column_zero_depth(
                                        target,
                                        zero_biome,
                                        zero_dist_sq,
                                    )
                            } else {
                                self.biome_from_sample(wx, wz, target)
                            };
                            sy_biomes[(by * 4 + bz) * 4 + bx] = biome_id;
                        }
                    }
                }
            }
        } else {
            for bz in 0..4 {
                let wz = base_z + (bz * 4) as i32;
                for bx in 0..4 {
                    let wx = base_x + (bx * 4) as i32;
                    let surf_target = self.settings.climate.sample_cached(
                        FunctionContext::new(wx, sea_level + 4, wz),
                        &self.noises,
                        &mut eval_cache,
                    );
                    surface_biomes_4x4[bz * 4 + bx] = self.biome_from_sample(wx, wz, surf_target);
                    for (sy, sy_biomes) in section_biomes.iter_mut().enumerate() {
                        let wy = min_y + (sy * 16) as i32;
                        for by in 0..4 {
                            let target = self.settings.climate.sample_cached(
                                FunctionContext::new(wx, wy + (by * 4) as i32, wz),
                                &self.noises,
                                &mut eval_cache,
                            );
                            let biome_id = self.biome_from_sample(wx, wz, target);
                            sy_biomes[(by * 4 + bz) * 4 + bx] = biome_id;
                        }
                    }
                }
            }
        }
        for sy in 0..section_biomes.len() {
            if sy > 0 && section_biomes[sy] == section_biomes[sy - 1] {
                let prev = chunk.sections()[sy - 1].biomes().clone();
                chunk.sections_mut()[sy].set_biomes_container(prev);
            } else {
                let _ = chunk.sections_mut()[sy].set_biomes_u32(&section_biomes[sy]);
            }
        }
        GEN_BIOMES.with(|cell| {
            *cell.borrow_mut() = section_biomes;
        });

        // Biome-aware top-down surface pass starting from each column's highest solid block.
        let sb = &self.settings.surface_biomes;
        if dim == DimensionKind::Overworld {
            let mut surface_materials_4x4 = [(pal.grass_block, pal.dirt, false); 16];
            for (i, &biome) in surface_biomes_4x4.iter().enumerate() {
                surface_materials_4x4[i] = if sb.badlands.contains(&biome) {
                    (
                        if pal.red_sand != 0 {
                            pal.red_sand
                        } else {
                            pal.sand
                        },
                        if pal.terracotta != 0 {
                            pal.terracotta
                        } else {
                            pal.dirt
                        },
                        true,
                    )
                } else if sb.sandy.contains(&biome) {
                    (pal.sand, pal.sand, false)
                } else if sb.snowy.contains(&biome) && pal.snow_block != 0 {
                    (pal.snow_block, pal.dirt, false)
                } else if sb.podzol.contains(&biome) && pal.podzol != 0 {
                    (pal.podzol, pal.dirt, false)
                } else if sb.mycelium.contains(&biome) && pal.mycelium != 0 {
                    (pal.mycelium, pal.dirt, false)
                } else {
                    (pal.grass_block, pal.dirt, false)
                };
            }
            for lz in 0..16 {
                for lx in 0..16 {
                    let top_y = max_solid_y[lz * 16 + lx];
                    if top_y <= min_y {
                        continue;
                    }
                    let (biome_top, biome_sub, is_badlands) =
                        surface_materials_4x4[(lz >> 2) * 4 + (lx >> 2)];

                    let mut depth_below_air = 0i32;
                    let col_off = lz * 16 + lx;
                    let mut in_air = top_y == min_y + gen_height - 1
                        || blocks[((top_y + 1 - min_y) as usize) * 256 + col_off] == pal.air;
                    for wy in (min_y + 1..=top_y).rev() {
                        let block_idx = ((wy - min_y) as usize) * 256 + col_off;
                        let current = blocks[block_idx];
                        if current == pal.air {
                            in_air = true;
                            depth_below_air = 0;
                        } else if current == pal.stone || current == pal.deepslate {
                            if in_air {
                                let top_block = if wy < sea_level {
                                    if is_badlands && pal.red_sand != 0 {
                                        pal.red_sand
                                    } else if wy <= sea_level - 4 {
                                        biome_sub
                                    } else {
                                        pal.sand
                                    }
                                } else if wy == sea_level && biome_top == pal.sand {
                                    pal.sand
                                } else if wy >= 120 {
                                    if biome_top == pal.snow_block {
                                        pal.snow_block
                                    } else {
                                        pal.stone
                                    }
                                } else {
                                    biome_top
                                };
                                blocks[block_idx] = top_block;
                                in_air = false;
                                depth_below_air = 1;
                            } else if depth_below_air < 4 && wy < 120 {
                                blocks[block_idx] = biome_sub;
                                depth_below_air += 1;
                            }
                        } else if current == pal.water {
                            in_air = false;
                        }
                    }
                }
            }
        } else if dim == DimensionKind::Nether && sb.has_nether() {
            let mut nether_top_4x4 = [0u16; 16];
            for (i, &biome) in surface_biomes_4x4.iter().enumerate() {
                nether_top_4x4[i] = if sb.crimson.contains(&biome) && pal.crimson_nylium != 0 {
                    pal.crimson_nylium
                } else if sb.warped.contains(&biome) && pal.warped_nylium != 0 {
                    pal.warped_nylium
                } else if sb.soul_sand.contains(&biome) && pal.soul_sand != 0 {
                    pal.soul_sand
                } else if sb.basalt.contains(&biome) && pal.basalt != 0 {
                    pal.basalt
                } else {
                    0
                };
            }
            for lz in 0..16 {
                for lx in 0..16 {
                    let top_block = nether_top_4x4[(lz >> 2) * 4 + (lx >> 2)];
                    if top_block == 0 {
                        continue;
                    }
                    let col_off = lz * 16 + lx;
                    for wy in (min_y + 1..top_gen_y - 1).rev() {
                        let block_idx = ((wy - min_y) as usize) * 256 + col_off;
                        let current = blocks[block_idx];
                        if current == pal.netherrack && blocks[block_idx + 256] == pal.air {
                            blocks[block_idx] = top_block;
                        }
                    }
                }
            }
        }

        let active_sections = if dim == DimensionKind::Overworld {
            let max_y = max_solid_y
                .iter()
                .copied()
                .max()
                .unwrap_or(min_y)
                .max(sea_level);
            (((max_y - min_y) as usize) / 16 + 1).min(sections)
        } else {
            ((gen_height as usize).div_ceil(16)).min(sections)
        };

        (chunk, active_sections)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::biome::{Parameter, ParameterPoint};

    use ferrite_world::BlockFlags;

    struct TestBlockInfo;
    impl BlockInfo for TestBlockInfo {
        fn flags(&self, state: u32) -> BlockFlags {
            BlockFlags {
                air: state == 0,
                fluid: state == 4,
                motion_blocking: state != 0 && state != 4,
                leaves: false,
            }
        }
    }

    fn test_palette() -> TerrainPalette {
        TerrainPalette {
            air: 0,
            stone: 1,
            deepslate: 2,
            bedrock: 3,
            water: 4,
            grass_block: 5,
            dirt: 6,
            sand: 7,
            netherrack: 8,
            lava: 9,
            end_stone: 10,
            ..TerrainPalette::default()
        }
    }

    #[test]
    fn generates_non_empty_overworld_chunk() {
        let palette = test_palette();
        let biomes = MultiNoiseBiomeSource::new(vec![(
            ParameterPoint {
                temperature: Parameter::span(-1.0, 1.0),
                humidity: Parameter::span(-1.0, 1.0),
                continentalness: Parameter::span(-1.0, 1.0),
                erosion: Parameter::span(-1.0, 1.0),
                depth: Parameter::span(-1.0, 1.0),
                weirdness: Parameter::span(-1.0, 1.0),
                offset: 0.0,
            },
            1,
        )]);
        let generator = NoiseChunkGenerator::overworld(12345, palette, biomes);
        let chunk = generator.generate_chunk(0, 0, &TestBlockInfo);
        assert_eq!(chunk.block(0, -64, 0), Some(palette.bedrock as u32));
    }

    #[test]
    fn generates_nether_and_end_dimensions() {
        let palette = test_palette();
        let biomes = MultiNoiseBiomeSource::new(vec![(
            ParameterPoint {
                temperature: Parameter::span(-1.0, 1.0),
                humidity: Parameter::span(-1.0, 1.0),
                continentalness: Parameter::span(-1.0, 1.0),
                erosion: Parameter::span(-1.0, 1.0),
                depth: Parameter::span(-1.0, 1.0),
                weirdness: Parameter::span(-1.0, 1.0),
                offset: 0.0,
            },
            1,
        )]);
        let nether = NoiseChunkGenerator::nether(42, palette, biomes);
        let n_chunk = nether.generate_chunk(0, 0, &TestBlockInfo);
        assert_eq!(n_chunk.sections().len(), 16);
        assert_eq!(n_chunk.block(0, 0, 0), Some(palette.bedrock as u32));
        assert_eq!(n_chunk.block(0, 127, 0), Some(palette.bedrock as u32));

        let end = NoiseChunkGenerator::end(
            42,
            palette,
            EndBiomeSource {
                the_end: 10,
                highlands: 11,
                midlands: 12,
                islands: 13,
                barrens: 14,
            },
        );
        let e_chunk = end.generate_chunk(0, 0, &TestBlockInfo);
        assert_eq!(e_chunk.sections().len(), 16);
        assert_eq!(end.biome_id_at(0, 64, 0), 10);
    }

    #[test]
    fn generates_chunks_in_parallel_deterministically() {
        let palette = test_palette();
        let biomes = MultiNoiseBiomeSource::new(vec![(
            ParameterPoint {
                temperature: Parameter::span(-1.0, 1.0),
                humidity: Parameter::span(-1.0, 1.0),
                continentalness: Parameter::span(-1.0, 1.0),
                erosion: Parameter::span(-1.0, 1.0),
                depth: Parameter::span(-1.0, 1.0),
                weirdness: Parameter::span(-1.0, 1.0),
                offset: 0.0,
            },
            1,
        )]);
        let generator = Arc::new(NoiseChunkGenerator::overworld(777, palette, biomes));
        let mut handles = Vec::new();
        for tid in 0..4 {
            let g = Arc::clone(&generator);
            handles.push(std::thread::spawn(move || {
                let c1 = g.generate_chunk(tid, -tid, &TestBlockInfo);
                let c2 = g.generate_chunk(tid, -tid, &TestBlockInfo);
                assert_eq!(c1.block(3, 64, 3), c2.block(3, 64, 3));
                assert_eq!(c1.block(0, -64, 0), Some(palette.bedrock as u32));
            }));
        }
        for h in handles {
            h.join().expect("worker thread panicked");
        }
    }
}
