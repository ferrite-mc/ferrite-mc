//! Overworld noise, aquifers, and carving before surface rules and features.

use crate::{
    DensityFunction, FunctionContext, ResolveError, ResourceId, SeededNoise, WorldgenData,
};
use ferrite_world::{BlockStateRegistry, Chunk, ChunkError};
use std::cell::RefCell;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum NoiseChunkError {
    #[error("missing overworld noise settings")]
    MissingSettings,
    #[error("unknown default block or biome: {0}")]
    MissingState(String),
    #[error("chunk coordinate outside block-coordinate range")]
    Coordinates,
    #[error(transparent)]
    Resolve(#[from] ResolveError),
    #[error(transparent)]
    Chunk(#[from] ChunkError),
}

/// Seeded overworld density router for filling new chunks.
pub struct NoiseChunkGenerator {
    density: DensityFunction,
    pub(crate) noise: SeededNoise,
    min_y: i32,
    height: i32,
    sea_level: i32,
    default_block: String,
    default_fluid: String,
    seed: i64,
    aquifer: Option<crate::aquifer::AquiferSpec>,
    /// The `worldgen/noise_settings` id this generator was built from.
    settings: ResourceId,
}

/// The trilinear corner grid of one `minecraft:interpolated` density node for one chunk.
///
/// Vanilla's `NoiseChunk` keeps a tiny interpolator per interpolated node over the chunk's
/// 4x8x4 cell grid and trilinearly blends it for every block. Evaluating the node
/// generically needs the eight corners of the block's cell, so they live in a flat array
/// (`NaN` marks an unsampled corner) indexed by the cell grid instead of a hash map.
struct CellGrid {
    origin: [i32; 3],
    shift: [u32; 3],
    dims: [usize; 3],
    values: Vec<f32>,
}

impl CellGrid {
    /// Builds a grid over `extent` blocks starting at `origin`, or `None` when the cell
    /// size is not a power-of-two divisor of the extent (the caller then falls back).
    fn new(origin: [i32; 3], extent: [i32; 3], cell: [i32; 2]) -> Option<Self> {
        if cell[0] <= 0 || cell[1] <= 0 {
            return None;
        }
        let size = [cell[0] as usize, cell[1] as usize, cell[0] as usize];
        for axis in 0..3 {
            if !size[axis].is_power_of_two() || !(extent[axis] as usize).is_multiple_of(size[axis])
            {
                return None;
            }
        }
        let dims = [
            extent[0] as usize / size[0] + 1,
            extent[1] as usize / size[1] + 1,
            extent[2] as usize / size[2] + 1,
        ];
        let aligned = |value: i32, size: usize| {
            let size = size as i32;
            value.div_euclid(size) * size
        };
        Some(Self {
            origin: [
                aligned(origin[0], size[0]),
                aligned(origin[1], size[1]),
                aligned(origin[2], size[2]),
            ],
            shift: [
                size[0].trailing_zeros(),
                size[1].trailing_zeros(),
                size[2].trailing_zeros(),
            ],
            dims,
            values: vec![f32::NAN; dims[0] * dims[1] * dims[2]],
        })
    }

    fn index(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        let ix = (x - self.origin[0]) >> self.shift[0];
        let iy = (y - self.origin[1]) >> self.shift[1];
        let iz = (z - self.origin[2]) >> self.shift[2];
        if ix < 0 || iy < 0 || iz < 0 {
            return None;
        }
        let (ix, iy, iz) = (ix as usize, iy as usize, iz as usize);
        if ix >= self.dims[0] || iy >= self.dims[1] || iz >= self.dims[2] {
            return None;
        }
        Some(ix + self.dims[0] * (iy + self.dims[1] * iz))
    }
}

/// One grid per interpolated node of the chunk's density tree, built on first use.
struct CellCache {
    origin: [i32; 3],
    extent: [i32; 3],
    grids: RefCell<Vec<(usize, CellGrid)>>,
}

impl CellCache {
    fn new(origin: [i32; 3], extent: [i32; 3]) -> Self {
        Self {
            origin,
            extent,
            grids: RefCell::new(Vec::new()),
        }
    }

    /// The grid slot for a node, creating it when the node is seen for the first time.
    fn slot(&self, node: usize, cell: [i32; 2]) -> Option<usize> {
        {
            let grids = self.grids.borrow();
            if let Some(slot) = grids.iter().position(|(key, _)| *key == node) {
                return Some(slot);
            }
        }
        let grid = CellGrid::new(self.origin, self.extent, cell)?;
        let mut grids = self.grids.borrow_mut();
        grids.push((node, grid));
        Some(grids.len() - 1)
    }

    fn with<T>(&self, slot: usize, read: impl FnOnce(&CellGrid) -> T) -> T {
        let grids = self.grids.borrow();
        read(&grids[slot].1)
    }

    fn with_mut<T>(&self, slot: usize, update: impl FnOnce(&mut CellGrid) -> T) -> T {
        let mut grids = self.grids.borrow_mut();
        update(&mut grids[slot].1)
    }
}

struct ChunkContext<'a> {
    parent: crate::SeededContext<'a>,
    cache: &'a CellCache,
}
impl FunctionContext for ChunkContext<'_> {
    fn block_x(&self) -> i32 {
        self.parent.block_x()
    }
    fn block_y(&self) -> i32 {
        self.parent.block_y()
    }
    fn block_z(&self) -> i32 {
        self.parent.block_z()
    }
    fn sample_noise(&self, id: &ResourceId, x: f64, y: f64, z: f64) -> f32 {
        self.parent.sample_noise(id, x, y, z)
    }
    fn sample_blended(&self, settings: [f64; 5], x: i32, y: i32, z: i32) -> f32 {
        self.parent.sample_blended(settings, x, y, z)
    }
    fn interpolated(
        &self,
        node: usize,
        cell: [i32; 2],
        x: i32,
        y: i32,
        z: i32,
        sample: &dyn Fn(i32, i32, i32) -> f32,
    ) -> Option<f32> {
        let [sx, sy] = cell;
        if sx <= 0 || sy <= 0 {
            return None;
        }
        let rx = x.rem_euclid(sx);
        let ry = y.rem_euclid(sy);
        let rz = z.rem_euclid(sx);
        if rx == 0 && ry == 0 && rz == 0 {
            return None;
        }
        let base = [x - rx, y - ry, z - rz];
        let corners = [
            [base[0], base[1], base[2]],
            [base[0] + sx, base[1], base[2]],
            [base[0], base[1] + sy, base[2]],
            [base[0] + sx, base[1] + sy, base[2]],
            [base[0], base[1], base[2] + sx],
            [base[0] + sx, base[1], base[2] + sx],
            [base[0], base[1] + sy, base[2] + sx],
            [base[0] + sx, base[1] + sy, base[2] + sx],
        ];
        let slot = self.cache.slot(node, cell)?;
        let indices: [Option<usize>; 8] = self.cache.with(slot, |grid| {
            corners.map(|[cx, cy, cz]| grid.index(cx, cy, cz))
        });
        let mut values = self.cache.with(slot, |grid| {
            indices.map(|index| index.map_or(f32::NAN, |position| grid.values[position]))
        });
        if indices.iter().any(Option::is_none) {
            return None;
        }
        for (index, value) in values.iter_mut().enumerate() {
            if value.is_nan() {
                let [cx, cy, cz] = corners[index];
                let sampled = sample(cx, cy, cz);
                *value = sampled;
                let position = indices[index].expect("checked above");
                self.cache
                    .with_mut(slot, |grid| grid.values[position] = sampled);
            }
        }
        let lerp = |t: f32, a: f32, b: f32| a + t * (b - a);
        let tx = rx as f32 / sx as f32;
        let ty = ry as f32 / sy as f32;
        let tz = rz as f32 / sx as f32;
        let below = lerp(
            ty,
            lerp(tx, values[0], values[1]),
            lerp(tx, values[2], values[3]),
        );
        let above = lerp(
            ty,
            lerp(tx, values[4], values[5]),
            lerp(tx, values[6], values[7]),
        );
        Some(lerp(tz, below, above))
    }
}

impl NoiseChunkGenerator {
    /// The overworld generator (`minecraft:overworld` noise settings).
    pub fn new(data: &WorldgenData, seed: i64) -> Result<Self, NoiseChunkError> {
        Self::with_settings(data, seed, "minecraft:overworld")
    }

    /// A generator for one dimension's `worldgen/noise_settings` entry, so the nether can be
    /// filled with netherrack and lava instead of overworld stone and water.
    pub fn with_settings(
        data: &WorldgenData,
        seed: i64,
        settings_id: &str,
    ) -> Result<Self, NoiseChunkError> {
        let settings = data
            .noise_settings(settings_id)
            .ok_or(NoiseChunkError::MissingSettings)?;
        Ok(Self {
            density: settings
                .noise_router
                .final_density
                .resolve_references(data)?,
            noise: SeededNoise::new(data, seed),
            min_y: settings.noise.min_y,
            height: settings.noise.height,
            sea_level: settings.sea_level,
            default_block: settings.default_block.to_string(),
            default_fluid: settings.default_fluid.to_string(),
            seed,
            aquifer: settings
                .aquifers
                .as_ref()
                .map(|value| crate::aquifer::AquiferSpec::new(value, data))
                .transpose()?,
            settings: ResourceId::parse(settings_id).expect("settings id"),
        })
    }

    /// The noise settings this generator fills, for the surface pass and biome selection.
    pub fn settings_id(&self) -> &ResourceId {
        &self.settings
    }

    /// Lowest block y of the generated column.
    pub fn min_y(&self) -> i32 {
        self.min_y
    }

    /// Height of the generated column in blocks.
    pub fn height(&self) -> i32 {
        self.height
    }

    /// Fills the noise terrain, then applies the vanilla carving mask.
    pub fn fill_chunk(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        registry: &impl BlockStateRegistry,
    ) -> Result<Chunk, NoiseChunkError> {
        let mut chunk = self.fill_noise_chunk(chunk_x, chunk_z, registry)?;
        self.carve_chunk(&mut chunk, chunk_x, chunk_z, registry)?;
        Ok(chunk)
    }

    /// Performs the noise and aquifer fill without surface rules or carvers.
    pub fn fill_noise_chunk(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        registry: &impl BlockStateRegistry,
    ) -> Result<Chunk, NoiseChunkError> {
        let x0 = chunk_x
            .checked_mul(16)
            .and_then(|x| x.checked_add(15).map(|_| x))
            .ok_or(NoiseChunkError::Coordinates)?;
        let z0 = chunk_z
            .checked_mul(16)
            .and_then(|z| z.checked_add(15).map(|_| z))
            .ok_or(NoiseChunkError::Coordinates)?;
        let state = |id: &str| {
            registry
                .block_id(id, &Default::default())
                .ok_or_else(|| NoiseChunkError::MissingState(id.to_owned()))
        };
        let air = state("minecraft:air")?;
        let solid = state(&self.default_block)?;
        let fluid = state(&self.default_fluid)?;
        let lava = state("minecraft:lava")?;
        let biome = registry
            .biome_id("minecraft:plains")
            .ok_or_else(|| NoiseChunkError::MissingState("minecraft:plains".into()))?;
        let mut chunk = Chunk::new(
            self.min_y,
            self.height as u32,
            air,
            biome,
            registry.block_bits(),
            registry.biome_bits(),
        )?;
        let cache = CellCache::new([x0, self.min_y, z0], [16, self.height, 16]);
        let mut aquifer = self.aquifer.as_ref().map(|spec| {
            crate::aquifer::Aquifer::new(
                spec,
                &self.noise,
                self.seed,
                self.sea_level,
                chunk_x,
                chunk_z,
            )
        });
        for y in self.min_y..self.min_y + self.height {
            for z in 0_u8..16 {
                for x in 0_u8..16 {
                    let ctx = ChunkContext {
                        parent: self.noise.at(x0 + i32::from(x), y, z0 + i32::from(z)),
                        cache: &cache,
                    };
                    let density = self.density.compute(&ctx);
                    let world_x = x0 + i32::from(x);
                    let world_z = z0 + i32::from(z);
                    let substance = |fluid_type| match fluid_type {
                        crate::aquifer::Fluid::Air => air,
                        crate::aquifer::Fluid::Water => fluid,
                        crate::aquifer::Fluid::Lava => lava,
                    };
                    let block = if let Some(aquifer) = aquifer.as_mut() {
                        aquifer
                            .substance(world_x, y, world_z, density)
                            .map_or(solid, substance)
                    } else if density > 0.0 {
                        solid
                    } else if y < self.sea_level {
                        fluid
                    } else {
                        air
                    };
                    if block != air {
                        chunk.set_block(x, y, z, block, registry)?;
                    }
                }
            }
        }
        Ok(chunk)
    }

    /// Applies carving after surface rules, as in the vanilla terrain pipeline.
    pub fn carve_chunk(
        &self,
        chunk: &mut Chunk,
        chunk_x: i32,
        chunk_z: i32,
        registry: &impl BlockStateRegistry,
    ) -> Result<(), NoiseChunkError> {
        let x0 = chunk_x
            .checked_mul(16)
            .and_then(|x| x.checked_add(15).map(|_| x))
            .ok_or(NoiseChunkError::Coordinates)?;
        let z0 = chunk_z
            .checked_mul(16)
            .and_then(|z| z.checked_add(15).map(|_| z))
            .ok_or(NoiseChunkError::Coordinates)?;
        let state = |id: &str| {
            registry
                .block_id(id, &Default::default())
                .ok_or_else(|| NoiseChunkError::MissingState(id.to_owned()))
        };
        let air = state("minecraft:air")?;
        let water = state(&self.default_fluid)?;
        let lava = state("minecraft:lava")?;
        let bedrock = state("minecraft:bedrock")?;
        let mask =
            crate::carver::CarveMask::new(self.seed, chunk_x, chunk_z, self.min_y, self.height);
        let mut aquifer = self.aquifer.as_ref().map(|spec| {
            crate::aquifer::Aquifer::new(
                spec,
                &self.noise,
                self.seed,
                self.sea_level,
                chunk_x,
                chunk_z,
            )
        });
        for y in self.min_y..self.min_y + self.height {
            for z in 0_u8..16 {
                for x in 0_u8..16 {
                    if !mask.contains(x, y, z) || chunk.block(x, y, z) == Some(bedrock) {
                        continue;
                    }
                    let substance = if let Some(aquifer) = aquifer.as_mut() {
                        aquifer.substance(x0 + i32::from(x), y, z0 + i32::from(z), 0.0)
                    } else {
                        Some(if y < self.sea_level {
                            crate::aquifer::Fluid::Water
                        } else {
                            crate::aquifer::Fluid::Air
                        })
                    };
                    let block = match substance {
                        None => continue,
                        Some(crate::aquifer::Fluid::Air) => air,
                        Some(crate::aquifer::Fluid::Water) => water,
                        Some(crate::aquifer::Fluid::Lava) => lava,
                    };
                    chunk.set_block(x, y, z, block, registry)?;
                }
            }
        }
        Ok(())
    }
}
