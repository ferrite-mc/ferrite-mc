//! 26.3 material rules and conditions; see vanilla `MaterialSystem` and `MaterialRuleContext`.
use crate::{
    DensityFunction, FunctionContext, MaterialCondition, MaterialConditionNode, MaterialRule,
    MaterialRuleNode, MultiNoiseBiomeSource, MultiNoiseError, NoiseChunkGenerator, NoiseRouter,
    ResolveError, ResourceId, SurfaceType, VerticalAnchor, WorldgenData,
};
use ferrite_noise::{LegacyRandomSource, PositionalRandomFactory, RandomSource, SimplexNoise};
use ferrite_world::{BlockStateRegistry, Chunk, ChunkError, HeightmapKind};
use sha2::{Digest, Sha256};
use std::{cell::RefCell, collections::HashMap};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SurfaceError {
    #[error("missing overworld noise settings")]
    MissingSettings,
    #[error("missing material rule: {0}")]
    MissingRule(ResourceId),
    #[error("missing material condition: {0}")]
    MissingCondition(ResourceId),
    #[error("missing block state: {0}")]
    MissingBlock(ResourceId),
    #[error("recursive material rule or condition")]
    Recursion,
    #[error("chunk coordinate outside block-coordinate range")]
    Coordinates,
    #[error(transparent)]
    Chunk(#[from] ChunkError),
    #[error(transparent)]
    Resolve(#[from] ResolveError),
    #[error(transparent)]
    Biome(#[from] MultiNoiseError),
}

/// Applies the overworld material rule tree to a noise-filled chunk.
pub struct SurfaceGenerator<'a> {
    data: &'a WorldgenData,
    noise: &'a NoiseChunkGenerator,
    /// The material rule tree with every named reference inlined once at construction:
    /// evaluating a block must not look a rule or condition up by id again.
    root: MaterialRule,
    router: NoiseRouter,
    biomes: MultiNoiseBiomeSource,
    biome_seed: i64,
    biome_cache: RefCell<BiomeCache>,
    bands: Vec<ResourceId>,
    frozen_temperature: [SimplexNoise; 3],
    biome_info: SimplexNoise,
    surface_noise: ResourceId,
    secondary_noise: ResourceId,
    clay_bands_offset: ResourceId,
    /// Positional factories by `random_name`, for `vertical_gradient` (one per surface
    /// column in vanilla; forking one for every block was measurable).
    positional_factories: RefCell<Vec<(usize, Box<dyn PositionalRandomFactory>)>>,
}

/// Dense memo of biome selections for the chunk currently being processed.
///
/// Surface rules ask for the biome of nearly every block, always at a 4-block quart cell.
/// A flat array of parameter-list indices replaces a hash map plus an id clone per query.
/// Positions outside the chunk (`/locate biome`, feature placement at chunk borders) fall
/// back to a direct lookup.
struct BiomeCache {
    origin: [i32; 3],
    dims: [i32; 3],
    entries: Vec<i32>,
}

impl BiomeCache {
    fn empty() -> Self {
        Self {
            origin: [0; 3],
            dims: [0; 3],
            entries: Vec::new(),
        }
    }

    /// Covers every quart cell a block of this chunk can select, plus one cell of margin.
    fn reset(&mut self, chunk_x: i32, chunk_z: i32, min_y: i32, height: i32) {
        self.origin = [chunk_x * 4 - 1, min_y.div_euclid(4) - 1, chunk_z * 4 - 1];
        self.dims = [6, height / 4 + 2, 6];
        let len = (self.dims[0] * self.dims[1] * self.dims[2]) as usize;
        self.entries.clear();
        self.entries.resize(len, -1);
    }

    fn slot(&self, quart: (i32, i32, i32)) -> Option<usize> {
        let x = quart.0 - self.origin[0];
        let y = quart.1 - self.origin[1];
        let z = quart.2 - self.origin[2];
        if x < 0 || y < 0 || z < 0 || x >= self.dims[0] || y >= self.dims[1] || z >= self.dims[2] {
            return None;
        }
        Some((x + self.dims[0] * (y + self.dims[1] * z)) as usize)
    }

    fn get(&self, slot: usize) -> Option<usize> {
        let value = self.entries[slot];
        (value >= 0).then_some(value as usize)
    }

    fn set(&mut self, slot: usize, index: usize) {
        self.entries[slot] = index as i32;
    }
}

/// Inlines every named rule and condition reference so evaluating a block never looks one
/// up by id. The vanilla overworld tree is a chain of named sequences and conditions, so
/// this showed up once per block per reference in the surface pass.
fn flatten_rule(
    data: &WorldgenData,
    rule: &MaterialRule,
    depth: usize,
) -> Result<MaterialRule, SurfaceError> {
    if depth > 128 {
        return Err(SurfaceError::Recursion);
    }
    Ok(match rule {
        MaterialRule::Reference(id) => {
            let target = data
                .material_rules
                .get(id)
                .ok_or_else(|| SurfaceError::MissingRule(id.clone()))?;
            flatten_rule(data, target, depth + 1)?
        }
        MaterialRule::Node(MaterialRuleNode::Sequence { sequence }) => {
            MaterialRule::Node(MaterialRuleNode::Sequence {
                sequence: sequence
                    .iter()
                    .map(|entry| flatten_rule(data, entry, depth + 1))
                    .collect::<Result<_, _>>()?,
            })
        }
        MaterialRule::Node(MaterialRuleNode::Condition { if_true, then_run }) => {
            MaterialRule::Node(MaterialRuleNode::Condition {
                if_true: flatten_condition(data, if_true, depth + 1)?,
                then_run: Box::new(flatten_rule(data, then_run, depth + 1)?),
            })
        }
        MaterialRule::Node(MaterialRuleNode::OreVein {
            density,
            filler_block,
            filler_gap,
            ore_block,
            raw_ore_block,
            raw_ore_chance,
            richness,
        }) => MaterialRule::Node(MaterialRuleNode::OreVein {
            density: Box::new(density.resolve_references(data)?),
            filler_block: filler_block.clone(),
            filler_gap: Box::new(filler_gap.resolve_references(data)?),
            ore_block: ore_block.clone(),
            raw_ore_block: raw_ore_block.clone(),
            raw_ore_chance: *raw_ore_chance,
            richness: Box::new(richness.resolve_references(data)?),
        }),
        MaterialRule::Node(MaterialRuleNode::Block { result_state }) => {
            MaterialRule::Node(MaterialRuleNode::Block {
                result_state: result_state.clone(),
            })
        }
        MaterialRule::Node(MaterialRuleNode::Bandlands) => {
            MaterialRule::Node(MaterialRuleNode::Bandlands)
        }
    })
}

fn flatten_condition(
    data: &WorldgenData,
    condition: &MaterialCondition,
    depth: usize,
) -> Result<MaterialCondition, SurfaceError> {
    if depth > 128 {
        return Err(SurfaceError::Recursion);
    }
    Ok(match condition {
        MaterialCondition::Reference(id) => {
            let target = data
                .material_conditions
                .get(id)
                .ok_or_else(|| SurfaceError::MissingCondition(id.clone()))?;
            flatten_condition(data, target, depth + 1)?
        }
        MaterialCondition::Node(MaterialConditionNode::Not { invert }) => {
            MaterialCondition::Node(MaterialConditionNode::Not {
                invert: Box::new(flatten_condition(data, invert, depth + 1)?),
            })
        }
        MaterialCondition::Node(node) => MaterialCondition::Node(node.clone()),
    })
}

struct Point {
    x: i32,
    y: i32,
    z: i32,
    min_y: i32,
    max_y: i32,
    surface_depth: i32,
    secondary: f64,
    stone_above: i32,
    stone_below: i32,
    water: i32,
    min_surface: i32,
    gradient_x: i32,
    gradient_z: i32,
}
impl Point {
    fn anchor(&self, anchor: VerticalAnchor) -> i32 {
        match anchor {
            VerticalAnchor::Absolute { absolute } => absolute,
            VerticalAnchor::AboveBottom { above_bottom } => self.min_y + above_bottom,
            VerticalAnchor::BelowTop { below_top } => self.max_y - below_top,
        }
    }
}

impl<'a> SurfaceGenerator<'a> {
    /// The overworld surface pass (material rule `minecraft:overworld`, biome preset
    /// `minecraft:overworld`).
    pub fn new(
        data: &'a WorldgenData,
        noise: &'a NoiseChunkGenerator,
        seed: i64,
    ) -> Result<Self, SurfaceError> {
        Self::with_biome_preset(data, noise, seed, "minecraft:overworld")
    }

    /// A surface pass for the dimension `noise` fills. The noise settings come from the
    /// generator, so the nether uses its own material rule instead of overworld grass.
    pub fn with_biome_preset(
        data: &'a WorldgenData,
        noise: &'a NoiseChunkGenerator,
        seed: i64,
        biome_preset: &str,
    ) -> Result<Self, SurfaceError> {
        let settings = data
            .noise_settings(&noise.settings_id().to_string())
            .ok_or(SurfaceError::MissingSettings)?;
        let root = data
            .material_rules
            .get(&settings.material_rule)
            .ok_or_else(|| SurfaceError::MissingRule(settings.material_rule.clone()))?;
        let digest = Sha256::digest(seed.to_le_bytes());
        let biome_seed = i64::from_le_bytes(digest[..8].try_into().expect("digest length"));
        let mut random = noise.noise.named_random("minecraft:clay_bands");
        let bands = Self::make_bands(&mut *random);
        let mut random = LegacyRandomSource::new(3456);
        let frozen_temperature =
            std::array::from_fn(|_| SimplexNoise::new_legacy(&mut random, true));
        let biome_info = SimplexNoise::new_legacy(&mut LegacyRandomSource::new(2345), true);
        Ok(Self {
            frozen_temperature,
            biome_info,
            data,
            noise,
            root: flatten_rule(data, root, 0)?,
            router: settings.noise_router.resolve_references(data)?,
            biomes: MultiNoiseBiomeSource::from_worldgen(data, biome_preset)?,
            biome_seed,
            biome_cache: RefCell::new(BiomeCache::empty()),
            bands,
            surface_noise: ResourceId::parse("surface").expect("known id"),
            secondary_noise: ResourceId::parse("surface_secondary").expect("known id"),
            clay_bands_offset: ResourceId::parse("clay_bands_offset").expect("known id"),
            positional_factories: RefCell::new(Vec::new()),
        })
    }

    /// The positional random for `name`, forked once and reused.
    fn positional_float(&self, name: &ResourceId, x: i32, y: i32, z: i32) -> f32 {
        let key = name as *const ResourceId as usize;
        {
            let factories = self.positional_factories.borrow();
            if let Some((_, factory)) = factories.iter().find(|(cached, _)| *cached == key) {
                return factory.at(x, y, z).next_float();
            }
        }
        let factory = self.noise.noise.positional_factory(name);
        let value = factory.at(x, y, z).next_float();
        self.positional_factories.borrow_mut().push((key, factory));
        value
    }
    fn make_bands(random: &mut dyn RandomSource) -> Vec<ResourceId> {
        let id = |name: &str| ResourceId::parse(name).expect("known id");
        let mut bands = vec![id("terracotta"); 192];
        let mut i = 0;
        while i < bands.len() {
            i += random.next_int_bound(5) as usize + 1;
            if i < bands.len() {
                bands[i] = id("orange_terracotta");
            }
            i += 1;
        }
        for (base, name) in [
            (1, "yellow_terracotta"),
            (2, "brown_terracotta"),
            (1, "red_terracotta"),
        ] {
            let count = random.next_int_between_inclusive(6, 15);
            for _ in 0..count {
                let width = base + random.next_int_bound(3);
                let start = random.next_int_bound(192) as usize;
                for band in bands
                    .iter_mut()
                    .take((start + width as usize).min(192))
                    .skip(start)
                {
                    *band = id(name);
                }
            }
        }
        let count = random.next_int_between_inclusive(9, 15);
        let mut start = 0_usize;
        for _ in 0..count {
            if start >= 192 {
                break;
            }
            bands[start] = id("white_terracotta");
            if start > 1 && random.next_boolean() {
                bands[start - 1] = id("light_gray_terracotta");
            }
            if start + 1 < 192 && random.next_boolean() {
                bands[start + 1] = id("light_gray_terracotta");
            }
            start += random.next_int_bound(16) as usize + 4;
        }
        bands
    }
    fn noise_value(&self, id: &ResourceId, x: i32, y: i32, z: i32) -> f64 {
        f64::from(self.noise.noise.at(x, y, z).sample_noise(
            id,
            f64::from(x),
            f64::from(y),
            f64::from(z),
        ))
    }
    fn scaled_noise(&self, name: &str, x: f64, z: f64) -> f32 {
        let id = ResourceId::parse(name).expect("known noise id");
        self.noise.noise.at(0, 0, 0).sample_noise(&id, x, 0.0, z)
    }
    // Biome.TemperatureModifier.FROZEN, queried at sea level by MaterialSystem.
    fn iceberg_melts(&self, biome: &ResourceId, x: i32, z: i32) -> bool {
        let x = f64::from(x);
        let z = f64::from(z);
        let mut frozen = 0.0_f32;
        for (noise, (frequency, amplitude)) in self.frozen_temperature.iter().zip([
            (1.0, 0.14285715_f32),
            (0.5, 0.2857143),
            (0.25, 0.5714286),
        ]) {
            frozen += amplitude * noise.get_2d(x * 0.05 * frequency, z * 0.05 * frequency);
        }
        let modified =
            f64::from(frozen * 7.0) + f64::from(self.biome_info.get_2d(x * 0.2, z * 0.2)) < 0.3
                && f64::from(self.biome_info.get_2d(x * 0.09, z * 0.09)) < 0.8;
        modified || (biome.namespace() == "minecraft" && biome.path() == "deep_frozen_ocean")
    }
    fn badlands_extension(
        &self,
        chunk: &mut Chunk,
        registry: &impl BlockStateRegistry,
        column: (u8, u8, i32, i32),
        height: i32,
    ) -> Result<(), SurfaceError> {
        let (x, z, wx, wz) = column;
        let xf = f64::from(wx);
        let zf = f64::from(wz);
        let pillar = (f64::from(self.scaled_noise("badlands_surface", xf, zf)) * 8.25)
            .abs()
            .min(f64::from(
                self.scaled_noise("badlands_pillar", xf * 0.2, zf * 0.2) * 15.0,
            ));
        if pillar <= 0.0 {
            return Ok(());
        }
        let roof = (f64::from(self.scaled_noise("badlands_pillar_roof", xf * 0.75, zf * 0.75))
            * 1.5)
            .abs();
        let top = (64.0 + (pillar * pillar * 2.5).min((roof * 50.0).ceil() + 24.0)).floor() as i32;
        if height > top {
            return Ok(());
        }
        let settings = self
            .data
            .noise_settings(&self.noise.settings_id().to_string())
            .expect("settings");
        let stone = registry
            .block_id(&settings.default_block.to_string(), &Default::default())
            .ok_or_else(|| SurfaceError::MissingBlock(settings.default_block.clone()))?;
        let top = top.min(chunk.min_y() + chunk.height_blocks() as i32 - 1);
        for y in (chunk.min_y()..=top).rev() {
            let old = chunk.block(x, y, z).expect("in bounds");
            if old == stone {
                break;
            }
            if registry
                .block_state(old)
                .is_some_and(|s| s.name == "minecraft:water")
            {
                return Ok(());
            }
        }
        for y in (chunk.min_y()..=top).rev() {
            let old = chunk.block(x, y, z).expect("in bounds");
            if !registry.flags(old).air {
                break;
            }
            chunk.set_block(x, y, z, stone, registry)?;
        }
        Ok(())
    }
    fn frozen_extension(
        &self,
        chunk: &mut Chunk,
        registry: &impl BlockStateRegistry,
        biome: &ResourceId,
        column: (u8, u8, i32, i32),
        bounds: (i32, i32),
    ) -> Result<(), SurfaceError> {
        let (x, z, wx, wz) = column;
        let (height, min_surface) = bounds;
        let xf = f64::from(wx);
        let zf = f64::from(wz);
        let iceberg = (f64::from(self.scaled_noise("iceberg_surface", xf, zf)) * 8.25)
            .abs()
            .min(f64::from(
                self.scaled_noise("iceberg_pillar", xf * 1.28, zf * 1.28) * 15.0,
            ));
        if iceberg <= 1.8 {
            return Ok(());
        }
        let roof =
            (f64::from(self.scaled_noise("iceberg_pillar_roof", xf * 1.17, zf * 1.17)) * 1.5).abs();
        let mut top = (iceberg * iceberg * 1.2).min((roof * 40.0).ceil() + 14.0);
        if self.iceberg_melts(biome, wx, wz) {
            top -= 2.0;
        }
        if top <= 2.0 {
            return Ok(());
        }
        let sea = self
            .data
            .noise_settings("overworld")
            .expect("settings")
            .sea_level;
        let bottom = f64::from(sea) - top - 7.0;
        top += f64::from(sea);
        let mut random = self.noise.noise.positional_random(None, wx, 0, wz);
        let max_snow = 2 + random.next_int_bound(4);
        let min_snow = sea + 18 + random.next_int_bound(10);
        let mut snow = 0;
        let packed = registry
            .block_id("minecraft:packed_ice", &Default::default())
            .ok_or_else(|| SurfaceError::MissingBlock(ResourceId::parse("packed_ice").unwrap()))?;
        let snow_block = registry
            .block_id("minecraft:snow_block", &Default::default())
            .ok_or_else(|| SurfaceError::MissingBlock(ResourceId::parse("snow_block").unwrap()))?;
        for y in (min_surface.max(chunk.min_y())
            ..=height
                .max(top as i32 + 1)
                .min(chunk.min_y() + chunk.height_blocks() as i32 - 1))
            .rev()
        {
            let old = chunk.block(x, y, z).expect("in bounds");
            let flags = registry.flags(old);
            let air = flags.air && y < top as i32 && random.next_double() > 0.01;
            let water = !air
                && registry
                    .block_state(old)
                    .is_some_and(|s| s.name == "minecraft:water")
                && f64::from(y) > bottom
                && y < sea
                && random.next_double() > 0.15;
            if air || water {
                let state = if snow <= max_snow && y > min_snow {
                    snow += 1;
                    snow_block
                } else {
                    packed
                };
                chunk.set_block(x, y, z, state, registry)?;
            }
        }
        Ok(())
    }
    fn density(&self, function: &DensityFunction, p: &Point) -> Result<f32, SurfaceError> {
        Ok(function.compute(&self.noise.noise.at(p.x, p.y, p.z)) as f32)
    }
    /// The parameter-list index of the biome selected for a block position.
    fn biome_index_at(&self, x: i32, y: i32, z: i32) -> usize {
        let key = self.biome_quart_at(x, y, z);
        let slot = self.biome_cache.borrow().slot(key);
        if let Some(slot) = slot
            && let Some(index) = self.biome_cache.borrow().get(slot)
        {
            return index;
        }
        let ctx = self.noise.noise.at(key.0 * 4, key.1 * 4, key.2 * 4);
        let sample = |df: &DensityFunction| df.compute(&ctx) as f32;
        let index = self
            .biomes
            .find_biome_vanilla_index(crate::ClimateTargetPoint::quantized(
                sample(&self.router.temperature),
                sample(&self.router.vegetation),
                sample(&self.router.continents),
                sample(&self.router.erosion),
                sample(&self.router.depth),
                sample(&self.router.ridges),
            ));
        if let Some(slot) = slot {
            self.biome_cache.borrow_mut().set(slot, index);
        }
        index
    }
    /// The biome selected for a block position, borrowed from the parameter list.
    fn biome_ref_at(&self, x: i32, y: i32, z: i32) -> &ResourceId {
        self.biomes.biome_by_index(self.biome_index_at(x, y, z))
    }
    /// Returns the biome selected for a block position by the overworld biome source.
    pub fn biome_at(&self, x: i32, y: i32, z: i32) -> ResourceId {
        self.biome_ref_at(x, y, z).clone()
    }
    #[doc(hidden)]
    pub fn biome_quart_at(&self, x: i32, y: i32, z: i32) -> (i32, i32, i32) {
        fn next(seed: i64, salt: i64) -> i64 {
            seed.wrapping_mul(
                seed.wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407),
            )
            .wrapping_add(salt)
        }
        fn fiddle(seed: i64) -> f64 {
            (((seed >> 24).rem_euclid(1024) as f64 / 1024.0) - 0.5) * 0.9
        }
        let xyz = [x - 2, y - 2, z - 2];
        let base = [xyz[0] >> 2, xyz[1] >> 2, xyz[2] >> 2];
        let fract = [
            f64::from(xyz[0] & 3) / 4.0,
            f64::from(xyz[1] & 3) / 4.0,
            f64::from(xyz[2] & 3) / 4.0,
        ];
        let mut best = f64::INFINITY;
        let mut corner = base;
        for i in 0..8 {
            let off = [(i >> 2) & 1, (i >> 1) & 1, i & 1];
            let q = [base[0] + off[0], base[1] + off[1], base[2] + off[2]];
            let mut r = self.biome_seed;
            for value in [q[0], q[1], q[2], q[0], q[1], q[2]] {
                r = next(r, i64::from(value));
            }
            let fx = fiddle(r);
            r = next(r, self.biome_seed);
            let fy = fiddle(r);
            r = next(r, self.biome_seed);
            let fz = fiddle(r);
            let dx = fract[0] - f64::from(off[0]) + fx;
            let dy = fract[1] - f64::from(off[1]) + fy;
            let dz = fract[2] - f64::from(off[2]) + fz;
            let distance = dz * dz + dy * dy + dx * dx;
            if distance < best {
                best = distance;
                corner = q;
            }
        }
        (corner[0], corner[1], corner[2])
    }
    fn condition(
        &self,
        condition: &MaterialCondition,
        p: &Point,
        nesting: usize,
    ) -> Result<bool, SurfaceError> {
        if nesting > 128 {
            return Err(SurfaceError::Recursion);
        }
        Ok(match condition {
            MaterialCondition::Reference(id) => self.condition(
                self.data
                    .material_conditions
                    .get(id)
                    .ok_or_else(|| SurfaceError::MissingCondition(id.clone()))?,
                p,
                nesting + 1,
            )?,
            MaterialCondition::Node(node) => match node {
                MaterialConditionNode::AbovePreliminarySurface => p.y >= p.min_surface,
                MaterialConditionNode::Biome { biome_is } => biome_is
                    .as_slice()
                    .contains(self.biome_ref_at(p.x, p.y, p.z)),
                MaterialConditionNode::Hole => p.surface_depth <= 0,
                MaterialConditionNode::NoiseThreshold {
                    noise,
                    min_threshold,
                    max_threshold,
                    is_3d,
                } => {
                    let value = self.noise_value(
                        noise,
                        p.x,
                        if is_3d.unwrap_or(false) { p.y } else { 0 },
                        p.z,
                    );
                    value >= *min_threshold && value <= *max_threshold
                }
                MaterialConditionNode::Not { invert } => !self.condition(invert, p, nesting + 1)?,
                MaterialConditionNode::Steep => p.gradient_x <= -4 || p.gradient_z >= 4,
                MaterialConditionNode::VerticalGradient {
                    true_at_and_below,
                    false_at_and_above,
                    random_name,
                } => {
                    let bottom = p.anchor(*true_at_and_below);
                    let top = p.anchor(*false_at_and_above);
                    if p.y <= bottom {
                        true
                    } else if p.y >= top {
                        false
                    } else {
                        let probability = 1.0 - f64::from(p.y - bottom) / f64::from(top - bottom);
                        f64::from(self.positional_float(random_name, p.x, p.y, p.z)) < probability
                    }
                }
                MaterialConditionNode::YAbove {
                    anchor,
                    surface_depth_multiplier,
                    add_stone_depth,
                } => {
                    p.y + if *add_stone_depth { p.stone_above } else { 0 }
                        >= p.anchor(*anchor) + p.surface_depth * surface_depth_multiplier
                }
                MaterialConditionNode::StoneDepth {
                    offset,
                    surface_type,
                    secondary_depth_range,
                    add_surface_depth,
                } => {
                    let depth = if *surface_type == SurfaceType::Ceiling {
                        p.stone_below
                    } else {
                        p.stone_above
                    };
                    let extra = if *add_surface_depth {
                        p.surface_depth
                    } else {
                        0
                    };
                    let secondary = if *secondary_depth_range == 0 {
                        0
                    } else {
                        ((p.secondary + 1.0) / 2.0 * f64::from(*secondary_depth_range)) as i32
                    };
                    depth <= 1 + offset + extra + secondary
                }
                MaterialConditionNode::Water {
                    offset,
                    surface_depth_multiplier,
                    add_stone_depth,
                } => {
                    p.water == i32::MIN
                        || p.y + if *add_stone_depth { p.stone_above } else { 0 }
                            >= p.water + offset + p.surface_depth * surface_depth_multiplier
                }
            },
        })
    }
    fn rule<'r>(
        &'r self,
        rule: &'r MaterialRule,
        p: &Point,
        nesting: usize,
    ) -> Result<Option<&'r ResourceId>, SurfaceError> {
        if nesting > 128 {
            return Err(SurfaceError::Recursion);
        }
        match rule {
            MaterialRule::Reference(id) => self.rule(
                self.data
                    .material_rules
                    .get(id)
                    .ok_or_else(|| SurfaceError::MissingRule(id.clone()))?,
                p,
                nesting + 1,
            ),
            MaterialRule::Node(node) => match node {
                MaterialRuleNode::Block { result_state } => Ok(Some(result_state)),
                MaterialRuleNode::Sequence { sequence } => {
                    for entry in sequence {
                        if let Some(result) = self.rule(entry, p, nesting + 1)? {
                            return Ok(Some(result));
                        }
                    }
                    Ok(None)
                }
                MaterialRuleNode::Condition { if_true, then_run } => {
                    if self.condition(if_true, p, nesting + 1)? {
                        self.rule(then_run, p, nesting + 1)
                    } else {
                        Ok(None)
                    }
                }
                MaterialRuleNode::Bandlands => {
                    let offset =
                        (self.noise_value(&self.clay_bands_offset, p.x, 0, p.z) as f32 * 4.0 + 0.5)
                            .floor() as i32;
                    Ok(Some(&self.bands[(p.y + offset).rem_euclid(192) as usize]))
                }
                MaterialRuleNode::OreVein {
                    density,
                    filler_block,
                    filler_gap,
                    ore_block,
                    raw_ore_block,
                    raw_ore_chance,
                    richness,
                } => {
                    let value = self.density(density, p)?;
                    if value <= 0.0 {
                        return Ok(None);
                    }
                    let mut random =
                        self.noise
                            .noise
                            .positional_random(Some("minecraft:ore"), p.x, p.y, p.z);
                    if random.next_float() > value {
                        return Ok(None);
                    }
                    let richness = self.density(richness, p)?;
                    if random.next_float() < richness && self.density(filler_gap, p)? < 0.0 {
                        Ok(Some(if random.next_float() < *raw_ore_chance {
                            raw_ore_block
                        } else {
                            ore_block
                        }))
                    } else {
                        Ok(Some(filler_block))
                    }
                }
            },
        }
    }
    /// Applies material rules in top-down column order. Generate carvers afterward.
    pub fn apply(
        &self,
        chunk: &mut Chunk,
        chunk_x: i32,
        chunk_z: i32,
        registry: &impl BlockStateRegistry,
    ) -> Result<(), SurfaceError> {
        let x0 = chunk_x
            .checked_mul(16)
            .and_then(|n| n.checked_add(15).map(|_| n))
            .ok_or(SurfaceError::Coordinates)?;
        let z0 = chunk_z
            .checked_mul(16)
            .and_then(|n| n.checked_add(15).map(|_| n))
            .ok_or(SurfaceError::Coordinates)?;
        let min_y = chunk.min_y();
        let max_y = min_y + chunk.height_blocks() as i32 - 1;
        self.biome_cache
            .borrow_mut()
            .reset(chunk_x, chunk_z, min_y, chunk.height_blocks() as i32);
        let mut states = HashMap::<&ResourceId, u32>::new();
        for x in 0_u8..16 {
            for z in 0_u8..16 {
                let wx = x0 + i32::from(x);
                let wz = z0 + i32::from(z);
                let starting_height = min_y
                    + i32::from(
                        chunk
                            .height(HeightmapKind::WorldSurfaceWg, x, z)
                            .expect("column"),
                    );
                let surface_biome = self.biome_ref_at(wx, starting_height, wz);
                let minecraft_biome = |name: &str| {
                    surface_biome.namespace() == "minecraft" && surface_biome.path() == name
                };
                if minecraft_biome("eroded_badlands") {
                    self.badlands_extension(chunk, registry, (x, z, wx, wz), starting_height)?;
                }
                let height = |xx, zz| {
                    i32::from(
                        chunk
                            .height(HeightmapKind::WorldSurfaceWg, xx, zz)
                            .expect("column"),
                    )
                };
                let gradient_x = height((x + 1).min(15), z) - height(x.saturating_sub(1), z);
                let gradient_z = height(x, (z + 1).min(15)) - height(x, z.saturating_sub(1));
                let start = min_y + height(x, z);

                let mut random = self.noise.noise.positional_random(None, wx, 0, wz);
                let surface_depth = (self.noise_value(&self.surface_noise, wx, 0, wz) * 2.75
                    + 3.0
                    + random.next_double() * 0.25) as i32;
                let secondary = self.noise_value(&self.secondary_noise, wx, 0, wz);
                let min_surface = self
                    .router
                    .chunk_surface_level
                    .compute(&self.noise.noise.at(wx, 0, wz))
                    .floor() as i32
                    + surface_depth
                    - 8;
                let (mut above, mut water, mut next_ceiling) = (0, i32::MIN, i32::MAX);
                for y in (min_y..=start.min(max_y + 1)).rev() {
                    let Some(old) = chunk.block(x, y, z) else {
                        above = 0;
                        water = i32::MIN;
                        continue;
                    };
                    let flags = registry.flags(old);
                    if flags.air {
                        above = 0;
                        water = i32::MIN;
                        continue;
                    }
                    if flags.fluid {
                        if water == i32::MIN {
                            water = y + 1;
                        }
                        continue;
                    }
                    if next_ceiling >= y {
                        next_ceiling = min_y;
                        for below in (min_y..y).rev() {
                            let f = registry.flags(chunk.block(x, below, z).expect("in bounds"));
                            if f.air || f.fluid {
                                next_ceiling = below + 1;
                                break;
                            }
                        }
                    }
                    above += 1;
                    let point = Point {
                        x: wx,
                        y,
                        z: wz,
                        min_y,
                        max_y,
                        surface_depth,
                        secondary,
                        stone_above: above,
                        stone_below: y - next_ceiling + 1,
                        water,
                        min_surface,
                        gradient_x,
                        gradient_z,
                    };
                    if let Some(id) = self.rule(&self.root, &point, 0)? {
                        let block = if let Some(value) = states.get(id) {
                            *value
                        } else {
                            let value = registry
                                .block_id(&id.to_string(), &Default::default())
                                .ok_or_else(|| SurfaceError::MissingBlock(id.clone()))?;
                            states.insert(id, value);
                            value
                        };
                        if old != block {
                            chunk.set_block(x, y, z, block, registry)?;
                        }
                    }
                }
                if minecraft_biome("frozen_ocean") || minecraft_biome("deep_frozen_ocean") {
                    self.frozen_extension(
                        chunk,
                        registry,
                        surface_biome,
                        (x, z, wx, wz),
                        (start, min_surface),
                    )?;
                }
            }
        }
        Ok(())
    }
}
