//! Placed and configured features for the 26.3 overworld.
//!
//! The decoration pipeline follows `ChunkGenerator.applyBiomeDecoration` and
//! `FeaturePlacer` from the 26.3 server jar:
//!
//! * the per-chunk seed is `WorldgenRandom.setDecorationSeed(levelSeed, minX, minZ)`,
//! * each placed feature is seeded with `setFeatureSeed(decorationSeed, index, step)`
//!   (`decorationSeed + index + 10000 * step`),
//! * placement modifiers run in JSON order, depth first, sharing one
//!   `XoroshiroRandomSource` per placed feature.
//!
//! Only the feature types that the overworld needs for terrain decoration are
//! implemented: `minecraft:ore`, `minecraft:simple_block`,
//! `minecraft:random_selector` and `minecraft:tree` (straight trunks with blob,
//! spruce or pine foliage). Other configured features are skipped, which is why the fixture
//! differential reports rates instead of demanding an exact whole-chunk match.
//!
//! Feature placement is clipped to the target chunk: vanilla's `WorldGenRegion`
//! lets trees and other features write into neighbouring chunks, but the
//! server generates one column at a time and keeps the blocks that belong to
//! the chunk being generated.

use crate::{ResourceId, WorldgenData};
use ferrite_noise::{
    LegacyRandomSource, MarsagliaPolarGaussian, Noise, NormalNoise, PositionalRandomFactory,
    RandomSource, SimplexNoise, XoroshiroRandomSource,
};
use ferrite_world::{BlockStateRegistry, Chunk, HeightmapKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

/// A `worldgen/feature` entry. The type-specific payload is kept as raw JSON so
/// the strict loader accepts every field of every vanilla feature type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConfiguredFeature {
    #[serde(rename = "type")]
    pub kind: ResourceId,
    #[serde(flatten)]
    pub config: BTreeMap<String, Value>,
}

impl ConfiguredFeature {
    pub fn field(&self, name: &str) -> Option<&Value> {
        self.config.get(name)
    }
}

/// A `worldgen/placed_feature` entry: a configured feature plus its placement
/// modifier list in vanilla order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlacedFeature {
    pub feature: ResourceId,
    #[serde(default)]
    pub placement: Vec<Value>,
}

/// A `worldgen/block_state_provider` entry, or an inline provider object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlockStateProvider {
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ResourceId>,
    #[serde(flatten)]
    pub config: BTreeMap<String, Value>,
}

/// A `tags/block` entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlockTag {
    #[serde(default)]
    pub values: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replace: Option<bool>,
}

#[derive(Debug, Error)]
pub enum FeatureError {
    #[error("missing overworld biome {0}")]
    MissingBiome(String),
    #[error("missing placed feature {0}")]
    MissingPlaced(ResourceId),
    #[error("missing configured feature {0}")]
    MissingConfigured(ResourceId),
    #[error("missing block state provider {0}")]
    MissingProvider(ResourceId),
    #[error(transparent)]
    Chunk(#[from] ferrite_world::ChunkError),
}

/// `net.minecraft.world.level.levelgen.WorldgenRandom`: a `WorldgenRandom` wraps a
/// `RandomSource` but keeps `BitRandomSource`'s bit-sliced `nextLong`/`nextInt(bound)`
/// semantics. Feature placement uses those methods, so the raw Xoroshiro methods are
/// not interchangeable with it.
pub struct WorldgenRandom {
    random: Box<dyn RandomSource>,
    gaussian: MarsagliaPolarGaussian,
    count: i32,
}

impl WorldgenRandom {
    pub fn new(seed: i64) -> Self {
        Self {
            random: Box::new(XoroshiroRandomSource::new(seed)),
            gaussian: MarsagliaPolarGaussian::new(),
            count: 0,
        }
    }

    pub fn count(&self) -> i32 {
        self.count
    }

    /// `WorldgenRandom.next(int)`: draws one `RandomSource.nextLong` and keeps
    /// the top `bits` bits.
    fn next_bits(&mut self, bits: i32) -> i32 {
        self.count += 1;
        ((self.random.next_long() as u64) >> (64 - bits)) as i32
    }
}

impl RandomSource for WorldgenRandom {
    fn fork(&mut self) -> Box<dyn RandomSource> {
        self.random.fork()
    }

    fn fork_positional(&mut self) -> Box<dyn PositionalRandomFactory> {
        self.random.fork_positional()
    }

    fn set_seed(&mut self, seed: i64) {
        self.random.set_seed(seed);
        self.gaussian.reset();
    }

    fn next_int(&mut self) -> i32 {
        self.next_bits(32)
    }

    fn next_int_bound(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "bound must be positive");
        if bound.isolate_lowest_one() == bound {
            let product = (bound as i64) * (self.next_bits(31) as i64);
            return (product >> 31) as i32;
        }
        loop {
            let bits = self.next_bits(31);
            let value = bits % bound;
            if bits.wrapping_sub(value).wrapping_add(bound.wrapping_sub(1)) >= 0 {
                return value;
            }
        }
    }

    fn next_long(&mut self) -> i64 {
        let high = (self.next_bits(32) as i64) << 32;
        high.wrapping_add(self.next_bits(32) as i64)
    }

    fn next_boolean(&mut self) -> bool {
        self.next_bits(1) != 0
    }

    fn next_float(&mut self) -> f32 {
        self.next_bits(24) as f32 * 5.960_464_5E-8
    }

    fn next_double(&mut self) -> f64 {
        let high = (self.next_bits(26) as i64) << 27;
        let low = self.next_bits(27) as i64;
        (high + low) as f64 * 1.110_223_024_625_156_5E-16
    }

    fn next_gaussian(&mut self) -> f64 {
        let mut gaussian = std::mem::take(&mut self.gaussian);
        let value = gaussian.next_gaussian(self);
        self.gaussian = gaussian;
        value
    }
}

/// The per-chunk decoration seed: `WorldgenRandom.setDecorationSeed`.
pub fn decoration_seed(seed: i64, x: i32, z: i32) -> i64 {
    let mut random = WorldgenRandom::new(seed);
    let a = random.next_long() | 1;
    let b = random.next_long() | 1;
    (i64::from(x)
        .wrapping_mul(a)
        .wrapping_add(i64::from(z).wrapping_mul(b)))
        ^ seed
}

/// `WorldgenRandom.setFeatureSeed`: the seed for one placed feature.
pub fn feature_seed(decoration_seed: i64, index: i32, step: i32) -> i64 {
    decoration_seed + i64::from(index) + 10_000 * i64::from(step)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BlockPos {
    x: i32,
    y: i32,
    z: i32,
}

impl BlockPos {
    const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    const fn offset(self, dx: i32, dy: i32, dz: i32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.z + dz)
    }
}

/// A block state written by a feature or a provider.
#[derive(Clone, Debug, PartialEq, Eq)]
struct StateSpec {
    name: String,
    properties: BTreeMap<String, String>,
}

impl StateSpec {
    fn from_value(value: &Value) -> Option<Self> {
        if let Some(name) = value.as_str() {
            return Some(Self {
                name: name.to_owned(),
                properties: BTreeMap::new(),
            });
        }
        let name = value.get("id")?.as_str()?.to_owned();
        let mut properties = BTreeMap::new();
        if let Some(map) = value.get("properties").and_then(Value::as_object) {
            for (key, value) in map {
                properties.insert(key.clone(), scalar(value));
            }
        }
        Some(Self { name, properties })
    }

    fn with(&self, key: &str, value: &str) -> Self {
        let mut out = self.clone();
        out.properties.insert(key.to_owned(), value.to_owned());
        out
    }
}

fn scalar(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        other => other.to_string(),
    }
}

#[derive(Clone, Debug)]
enum ProviderRef {
    Inline(BlockStateProvider),
    Id(ResourceId),
}

impl ProviderRef {
    fn from_value(value: &Value) -> Option<Self> {
        if let Some(id) = value.as_str() {
            return Some(Self::Id(ResourceId::parse(id).ok()?));
        }
        let provider: BlockStateProvider = serde_json::from_value(value.clone()).ok()?;
        Some(Self::Inline(provider))
    }
}

#[derive(Clone, Debug)]
enum IntProvider {
    Constant(i32),
    Uniform { min: i32, max: i32 },
    Weighted(Vec<(i32, i32)>),
    Trapezoid { min: i32, max: i32, plateau: i32 },
}

impl IntProvider {
    fn from_value(value: &Value) -> Option<Self> {
        if let Some(number) = value.as_i64() {
            return i32::try_from(number).ok().map(Self::Constant);
        }
        let kind = value.get("type")?.as_str()?;
        Some(match kind {
            "minecraft:constant" => {
                Self::Constant(i32::try_from(value.get("value")?.as_i64()?).ok()?)
            }
            "minecraft:uniform" => Self::Uniform {
                min: i32::try_from(value.get("min_inclusive")?.as_i64()?).ok()?,
                max: i32::try_from(value.get("max_inclusive")?.as_i64()?).ok()?,
            },
            "minecraft:weighted_list" => Self::Weighted(
                value
                    .get("distribution")?
                    .as_array()?
                    .iter()
                    .filter_map(|entry| {
                        Some((
                            i32::try_from(entry.get("data")?.as_i64()?).ok()?,
                            i32::try_from(entry.get("weight")?.as_i64()?).ok()?,
                        ))
                    })
                    .collect(),
            ),
            "minecraft:trapezoid" => Self::Trapezoid {
                min: i32::try_from(value.get("min")?.as_i64()?).ok()?,
                max: i32::try_from(value.get("max")?.as_i64()?).ok()?,
                // `TrapezoidInt` defaults `plateau` to 0 when it is absent.
                plateau: value
                    .get("plateau")
                    .and_then(Value::as_i64)
                    .and_then(|plateau| i32::try_from(plateau).ok())
                    .unwrap_or(0),
            },
            _ => return None,
        })
    }

    fn sample(&self, random: &mut dyn RandomSource) -> i32 {
        match self {
            Self::Constant(value) => *value,
            Self::Uniform { min, max } => {
                if max <= min {
                    *min
                } else {
                    min + random.next_int_bound(max - min + 1)
                }
            }
            Self::Weighted(entries) => {
                let total: i32 = entries.iter().map(|(_, weight)| *weight).sum();
                if total <= 0 {
                    return 0;
                }
                let mut choice = random.next_int_bound(total);
                for (data, weight) in entries {
                    if choice < *weight {
                        return *data;
                    }
                    choice -= *weight;
                }
                entries.last().map_or(0, |(data, _)| *data)
            }
            Self::Trapezoid { min, max, plateau } => {
                let span = (max - min + 1).max(1);
                min + random.next_int_bound(span) + random.next_int_bound(plateau + 1)
                    - random.next_int_bound(plateau + 1)
            }
        }
    }
}

/// `VerticalAnchor` in the 26.3 data format.
#[derive(Clone, Copy, Debug)]
enum VerticalAnchor {
    Absolute(i32),
    AboveBottom(i32),
    BelowTop(i32),
}

impl VerticalAnchor {
    fn from_value(value: &Value) -> Option<Self> {
        if let Some(number) = value.as_i64() {
            return i32::try_from(number).ok().map(Self::Absolute);
        }
        if let Some(number) = value.get("absolute") {
            return i32::try_from(number.as_i64()?).ok().map(Self::Absolute);
        }
        if let Some(number) = value.get("above_bottom") {
            return i32::try_from(number.as_i64()?).ok().map(Self::AboveBottom);
        }
        if let Some(number) = value.get("below_top") {
            return i32::try_from(number.as_i64()?).ok().map(Self::BelowTop);
        }
        None
    }

    fn resolve(self, min_y: i32, height: i32) -> i32 {
        match self {
            Self::Absolute(value) => value,
            Self::AboveBottom(value) => min_y + value,
            // `WorldGenerationContext.getMaxY()` is exclusive, matching
            // `VerticalAnchor.BelowTop.resolveY`.
            Self::BelowTop(value) => min_y + height - value,
        }
    }
}

#[derive(Clone, Debug)]
enum HeightProvider {
    Constant(VerticalAnchor),
    Uniform {
        min: VerticalAnchor,
        max: VerticalAnchor,
    },
    Trapezoid {
        min: VerticalAnchor,
        max: VerticalAnchor,
        plateau: i32,
    },
}

impl HeightProvider {
    fn from_value(value: &Value) -> Option<Self> {
        let kind = value.get("type").and_then(Value::as_str);
        let (min, max) = (
            VerticalAnchor::from_value(value.get("min_inclusive")?)?,
            VerticalAnchor::from_value(value.get("max_inclusive")?)?,
        );
        Some(match kind {
            Some("minecraft:uniform") => Self::Uniform { min, max },
            Some("minecraft:trapezoid") => Self::Trapezoid {
                min,
                max,
                // `TrapezoidHeight` defaults `plateau` to 0 when it is absent.
                plateau: value
                    .get("plateau")
                    .and_then(Value::as_i64)
                    .and_then(|plateau| i32::try_from(plateau).ok())
                    .unwrap_or(0),
            },
            Some("minecraft:constant") | None => {
                Self::Constant(if kind.is_some() { max } else { min })
            }
            Some("minecraft:very_biased_to_bottom") => Self::Trapezoid {
                min,
                max,
                plateau: 1,
            },
            _ => return None,
        })
    }

    fn sample(&self, random: &mut dyn RandomSource, min_y: i32, height: i32) -> i32 {
        match self {
            Self::Constant(anchor) => anchor.resolve(min_y, height),
            Self::Uniform { min, max } => {
                let min = min.resolve(min_y, height);
                let max = max.resolve(min_y, height);
                if max <= min {
                    min
                } else {
                    min + random.next_int_bound(max - min + 1)
                }
            }
            Self::Trapezoid { min, max, plateau } => {
                let min = min.resolve(min_y, height);
                let max = max.resolve(min_y, height);
                if max <= min {
                    min
                } else {
                    min + random.next_int_bound(max - min + 1) + random.next_int_bound(plateau + 1)
                        - random.next_int_bound(plateau + 1)
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
enum BlockPredicate {
    MatchingBlockTag(String),
    MatchingBlocks(Vec<String>),
    WouldSurvive(String),
    AllOf(Vec<Self>),
    AnyOf(Vec<Self>),
    Not(Box<Self>),
    Unsupported,
}

impl BlockPredicate {
    fn from_value(value: &Value) -> Self {
        let Some(kind) = value.get("type").and_then(Value::as_str) else {
            return Self::Unsupported;
        };
        match kind {
            "minecraft:matching_block_tag" => value
                .get("tag")
                .and_then(Value::as_str)
                .map_or(Self::Unsupported, |tag| {
                    Self::MatchingBlockTag(tag.trim_start_matches('#').to_owned())
                }),
            "minecraft:matching_blocks" => Self::MatchingBlocks(
                value
                    .get("blocks")
                    .and_then(Value::as_array)
                    .map(|blocks| {
                        blocks
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default(),
            ),
            "minecraft:would_survive" => match value.get("state").and_then(StateSpec::from_value) {
                Some(state) => Self::WouldSurvive(state.name),
                None => Self::Unsupported,
            },
            "minecraft:all_of" => Self::AllOf(
                value
                    .get("rules")
                    .and_then(Value::as_array)
                    .map(|rules| rules.iter().map(Self::from_value).collect())
                    .unwrap_or_default(),
            ),
            "minecraft:any_of" => Self::AnyOf(
                value
                    .get("rules")
                    .and_then(Value::as_array)
                    .map(|rules| rules.iter().map(Self::from_value).collect())
                    .unwrap_or_default(),
            ),
            "minecraft:not" => value.get("rule").map_or(Self::Unsupported, |rule| {
                Self::Not(Box::new(Self::from_value(rule)))
            }),
            _ => Self::Unsupported,
        }
    }
}

#[derive(Clone, Debug)]
enum Modifier {
    Count(IntProvider),
    InSquare,
    HeightRange(HeightProvider),
    Heightmap(HeightmapKind),
    Biome,
    BlockPredicate(BlockPredicate),
    Offset(IntProvider, IntProvider, IntProvider),
    SurfaceWaterDepth(i32),
    Rarity(i32),
    NoiseThresholdCount {
        noise_level: f64,
        below: i32,
        above: i32,
    },
    Unsupported,
}

impl Modifier {
    fn from_value(value: &Value) -> Self {
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match kind {
            "minecraft:count" => value
                .get("count")
                .and_then(IntProvider::from_value)
                .map_or(Self::Unsupported, Self::Count),
            "minecraft:in_square" => Self::InSquare,
            "minecraft:height_range" => value
                .get("height")
                .and_then(HeightProvider::from_value)
                .map_or(Self::Unsupported, Self::HeightRange),
            "minecraft:heightmap" => value
                .get("heightmap")
                .and_then(Value::as_str)
                .and_then(heightmap_kind)
                .map_or(Self::Unsupported, Self::Heightmap),
            "minecraft:biome" => Self::Biome,
            "minecraft:block_predicate_filter" => value
                .get("predicate")
                .map_or(Self::Unsupported, |predicate| {
                    Self::BlockPredicate(BlockPredicate::from_value(predicate))
                }),
            "minecraft:offset" => {
                let provider = |name: &str| {
                    value
                        .get(name)
                        .and_then(IntProvider::from_value)
                        .unwrap_or(IntProvider::Constant(0))
                };
                Self::Offset(provider("x"), provider("y"), provider("z"))
            }
            "minecraft:surface_water_depth_filter" => value
                .get("max_water_depth")
                .and_then(Value::as_i64)
                .and_then(|value| i32::try_from(value).ok())
                .map_or(Self::Unsupported, Self::SurfaceWaterDepth),
            "minecraft:rarity_filter" => value
                .get("chance")
                .and_then(Value::as_i64)
                .and_then(|value| i32::try_from(value).ok())
                .map_or(Self::Unsupported, Self::Rarity),
            "minecraft:noise_threshold_count" => {
                let number = |name: &str| value.get(name).and_then(Value::as_f64);
                match (
                    number("noise_level"),
                    number("below_noise"),
                    number("above_noise"),
                ) {
                    (Some(noise_level), Some(below), Some(above)) => Self::NoiseThresholdCount {
                        noise_level,
                        below: below as i32,
                        above: above as i32,
                    },
                    _ => Self::Unsupported,
                }
            }
            _ => Self::Unsupported,
        }
    }
}

fn heightmap_kind(name: &str) -> Option<HeightmapKind> {
    Some(match name {
        "WORLD_SURFACE_WG" => HeightmapKind::WorldSurfaceWg,
        "WORLD_SURFACE" => HeightmapKind::WorldSurface,
        "OCEAN_FLOOR_WG" => HeightmapKind::OceanFloorWg,
        "OCEAN_FLOOR" => HeightmapKind::OceanFloor,
        "MOTION_BLOCKING" => HeightmapKind::MotionBlocking,
        "MOTION_BLOCKING_NO_LEAVES" => HeightmapKind::MotionBlockingNoLeaves,
        _ => return None,
    })
}

#[derive(Clone, Debug)]
enum CompiledFeature {
    Ore {
        size: i32,
        discard_chance_on_air_exposure: f32,
        targets: Vec<(Rule, StateSpec)>,
    },
    SimpleBlock(ProviderRef),
    RandomSelector {
        default: ResourceId,
        choices: Vec<(f32, ResourceId)>,
    },
    Tree(Box<TreeConfig>),
    Unsupported,
}

#[derive(Clone, Debug)]
enum Rule {
    Tag(String),
    Height { min: i32, max: i32 },
    All(Vec<Self>),
    Any(Vec<Self>),
    Not(Box<Self>),
    Unsupported,
}

impl Rule {
    fn from_value(value: &Value) -> Self {
        let Some(kind) = value.get("predicate_type").and_then(Value::as_str) else {
            return Self::Unsupported;
        };
        match kind {
            "minecraft:tag_match" => value
                .get("tag")
                .and_then(Value::as_str)
                .map_or(Self::Unsupported, |tag| {
                    Self::Tag(tag.trim_start_matches('#').to_owned())
                }),
            "minecraft:height_match" => {
                let min = value
                    .get("min_inclusive")
                    .and_then(Value::as_i64)
                    .unwrap_or(i64::MIN);
                let max = value
                    .get("max_inclusive")
                    .and_then(Value::as_i64)
                    .unwrap_or(i64::MAX);
                Self::Height {
                    min: min.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
                    max: max.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
                }
            }
            "minecraft:all_of" => Self::All(
                value
                    .get("rules")
                    .and_then(Value::as_array)
                    .map(|rules| rules.iter().map(Self::from_value).collect())
                    .unwrap_or_default(),
            ),
            "minecraft:any_of" => Self::Any(
                value
                    .get("rules")
                    .and_then(Value::as_array)
                    .map(|rules| rules.iter().map(Self::from_value).collect())
                    .unwrap_or_default(),
            ),
            "minecraft:not" => value.get("rule").map_or(Self::Unsupported, |rule| {
                Self::Not(Box::new(Self::from_value(rule)))
            }),
            _ => Self::Unsupported,
        }
    }
}

#[derive(Clone, Debug)]
struct TreeConfig {
    trunk: ProviderRef,
    foliage: ProviderRef,
    below_trunk: Option<ProviderRef>,
    base_height: i32,
    height_rand_a: i32,
    height_rand_b: i32,
    foliage_placer: TreeFoliage,
    limit: i32,
    lower_size: i32,
    upper_size: i32,
    min_clipped_height: Option<i32>,
}

#[derive(Clone, Debug)]
enum TreeFoliage {
    Blob {
        height: i32,
        radius: i32,
        offset: i32,
    },
    Spruce {
        trunk_height: IntProvider,
        radius: IntProvider,
        offset: IntProvider,
    },
    Pine {
        height: IntProvider,
        radius: IntProvider,
        offset: IntProvider,
    },
}

/// Data-driven feature decorator for one world seed.
pub struct FeatureGenerator<'a> {
    data: &'a WorldgenData,
    seed: i64,
    min_y: i32,
    height: i32,
    chunk_x: i32,
    chunk_z: i32,
    biome_info: SimplexNoise,
    /// `FeatureSorter.buildFeaturesPerStep`: the union of every possible overworld
    /// biome's list, in biome-source order. Vanilla seeds a feature with its index in
    /// this list, not with its position in the chunk's filtered list.
    steps: Vec<Vec<ResourceId>>,
}

impl<'a> FeatureGenerator<'a> {
    pub fn new(data: &'a WorldgenData, seed: i64, min_y: i32, height: i32) -> Self {
        // `Biome.BIOME_INFO_NOISE`: `new SimplexNoise(new WorldgenRandom(new
        // LegacyRandomSource(2345L)), true)`.
        let mut random = LegacyRandomSource::new(2345);
        let biome_info = SimplexNoise::new_legacy(&mut random, true);
        Self {
            data,
            seed,
            min_y,
            height,
            chunk_x: 0,
            chunk_z: 0,
            biome_info,
            steps: global_steps(data),
        }
    }

    /// The global `FeatureSorter` list for one decoration step.
    pub fn step_features(&self, step: usize) -> &[ResourceId] {
        self.steps.get(step).map_or(&[], Vec::as_slice)
    }

    /// Decorates `chunk` in vanilla `GenerationStep.Decoration` order. The
    /// `biome_at` callback resolves the chunk's quart biome for a block
    /// position, normally `SurfaceGenerator::biome_at`.
    pub fn decorate(
        &mut self,
        chunk: &mut Chunk,
        chunk_x: i32,
        chunk_z: i32,
        biome_at: &dyn Fn(i32, i32, i32) -> ResourceId,
        registry: &impl BlockStateRegistry,
    ) -> Result<(), FeatureError> {
        self.chunk_x = chunk_x;
        self.chunk_z = chunk_z;
        let chunk_biomes = self.chunk_biomes(chunk, registry);
        if chunk_biomes.is_empty() {
            return Ok(());
        }
        let decoration = decoration_seed(self.seed, chunk_x * 16, chunk_z * 16);
        for (step, global) in self.steps.iter().enumerate() {
            if global.is_empty() {
                continue;
            }
            // `applyBiomeDecoration` collects the global indices of the features
            // used by the biomes in this chunk, sorts them and seeds each one
            // with its global index.
            let mut present = BTreeSet::new();
            for biome in &chunk_biomes {
                let Some(biome) = self.data.biome(biome) else {
                    continue;
                };
                for feature in biome.features.get(step).into_iter().flatten() {
                    present.insert(feature.clone());
                }
            }
            for (index, placed_id) in global.iter().enumerate() {
                if !present.contains(placed_id) {
                    continue;
                }
                let seed = feature_seed(decoration, index as i32, step as i32);
                let mut random = WorldgenRandom::new(seed);
                self.decorate_feature(
                    chunk,
                    chunk_x,
                    chunk_z,
                    placed_id,
                    &mut random,
                    biome_at,
                    registry,
                )?;
            }
        }
        Ok(())
    }

    fn chunk_biomes(&self, chunk: &Chunk, registry: &impl BlockStateRegistry) -> Vec<String> {
        let mut biomes = Vec::new();
        for section in chunk.sections() {
            for index in 0..64 {
                let Some(id) = section.biomes().get(index) else {
                    continue;
                };
                let Some(name) = registry.biome_name(id) else {
                    continue;
                };
                if !biomes.iter().any(|existing| existing == name) {
                    biomes.push(name.to_owned());
                }
            }
        }
        biomes
    }

    #[allow(clippy::too_many_arguments)]
    fn decorate_feature(
        &self,
        chunk: &mut Chunk,
        chunk_x: i32,
        chunk_z: i32,
        placed_id: &ResourceId,
        random: &mut dyn RandomSource,
        biome_at: &dyn Fn(i32, i32, i32) -> ResourceId,
        registry: &impl BlockStateRegistry,
    ) -> Result<(), FeatureError> {
        let Some(placed) = self.data.placed_feature(placed_id) else {
            return Ok(());
        };
        let origin = BlockPos::new(chunk_x * 16, self.min_y, chunk_z * 16);
        let modifiers: Vec<Modifier> = placed.placement.iter().map(Modifier::from_value).collect();
        if modifiers
            .iter()
            .any(|modifier| matches!(modifier, Modifier::Unsupported))
        {
            return Ok(());
        }
        let mut positions = Vec::new();
        self.apply_modifier(
            chunk,
            &modifiers,
            0,
            origin,
            random,
            &mut positions,
            biome_at,
            registry,
            placed_id,
        );
        for position in positions {
            self.place_feature(chunk, &placed.feature, position, random, registry, biome_at)?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_modifier(
        &self,
        chunk: &Chunk,
        modifiers: &[Modifier],
        index: usize,
        position: BlockPos,
        random: &mut dyn RandomSource,
        output: &mut Vec<BlockPos>,
        biome_at: &dyn Fn(i32, i32, i32) -> ResourceId,
        registry: &impl BlockStateRegistry,
        placed_id: &ResourceId,
    ) {
        if index == modifiers.len() {
            output.push(position);
            return;
        }
        let mut produced = Vec::new();
        match &modifiers[index] {
            Modifier::Count(count) => {
                let count = count.sample(random).max(0);
                produced.extend(std::iter::repeat_n(position, count as usize));
            }
            Modifier::InSquare => produced.push(position.offset(
                random.next_int_bound(16),
                0,
                random.next_int_bound(16),
            )),
            Modifier::HeightRange(provider) => produced.push(BlockPos::new(
                position.x,
                provider.sample(random, self.min_y, self.height),
                position.z,
            )),
            Modifier::Heightmap(kind) => produced.push(BlockPos::new(
                position.x,
                self.height_at(chunk, *kind, position.x, position.z),
                position.z,
            )),
            Modifier::Biome => {
                if self.biome_has_feature(biome_at(position.x, position.y, position.z), placed_id) {
                    produced.push(position);
                }
            }
            Modifier::BlockPredicate(predicate) => {
                if self.predicate_matches(chunk, predicate, position, registry) {
                    produced.push(position);
                }
            }
            Modifier::Offset(x, y, z) => {
                produced.push(position.offset(x.sample(random), y.sample(random), z.sample(random)))
            }
            Modifier::SurfaceWaterDepth(max_depth) => {
                let floor =
                    self.height_at(chunk, HeightmapKind::OceanFloor, position.x, position.z);
                let surface =
                    self.height_at(chunk, HeightmapKind::WorldSurface, position.x, position.z);
                if surface - floor <= *max_depth {
                    produced.push(position);
                }
            }
            Modifier::Rarity(chance) => {
                if *chance > 0 && random.next_float() < 1.0 / *chance as f32 {
                    produced.push(position);
                }
            }
            Modifier::NoiseThresholdCount {
                noise_level,
                below,
                above,
            } => {
                let noise = self.biome_info.get(
                    f64::from(position.x) / 200.0,
                    0.0,
                    f64::from(position.z) / 200.0,
                );
                let count = if f64::from(noise) < *noise_level {
                    *below
                } else {
                    *above
                };
                produced.extend(std::iter::repeat_n(position, count.max(0) as usize));
            }
            Modifier::Unsupported => {}
        }
        for position in produced {
            self.apply_modifier(
                chunk,
                modifiers,
                index + 1,
                position,
                random,
                output,
                biome_at,
                registry,
                placed_id,
            );
        }
    }

    fn biome_has_feature(&self, biome: ResourceId, placed: &ResourceId) -> bool {
        self.data
            .biome(&biome.to_string())
            .is_some_and(|biome| biome.features.iter().flatten().any(|id| id == placed))
    }

    fn height_at(&self, chunk: &Chunk, kind: HeightmapKind, x: i32, z: i32) -> i32 {
        if x.div_euclid(16) != self.chunk_x || z.div_euclid(16) != self.chunk_z {
            return self.min_y - 1;
        }
        let local_x = x.rem_euclid(16) as u8;
        let local_z = z.rem_euclid(16) as u8;
        // `ChunkAccess.getHeight` returns `Heightmap.getFirstAvailable`, the first
        // y above the highest matching block: the block position a heightmap
        // placement modifier starts from.
        chunk
            .height(kind, local_x, local_z)
            .map_or(self.min_y - 1, |height| self.min_y + i32::from(height))
    }

    fn predicate_matches(
        &self,
        chunk: &Chunk,
        predicate: &BlockPredicate,
        position: BlockPos,
        registry: &impl BlockStateRegistry,
    ) -> bool {
        match predicate {
            BlockPredicate::MatchingBlockTag(tag) => self
                .block_at(chunk, position, registry)
                .is_some_and(|state| self.block_in_tag(state, tag, registry)),
            BlockPredicate::MatchingBlocks(blocks) => self
                .block_at(chunk, position, registry)
                .and_then(|state| registry.block_state(state))
                .is_some_and(|state| blocks.contains(&state.name)),
            BlockPredicate::WouldSurvive(block) => {
                self.state_survives_named(chunk, block, position, registry)
            }
            BlockPredicate::AllOf(rules) => rules
                .iter()
                .all(|rule| self.predicate_matches(chunk, rule, position, registry)),
            BlockPredicate::AnyOf(rules) => rules
                .iter()
                .any(|rule| self.predicate_matches(chunk, rule, position, registry)),
            BlockPredicate::Not(rule) => !self.predicate_matches(chunk, rule, position, registry),
            BlockPredicate::Unsupported => false,
        }
    }

    fn block_at(
        &self,
        chunk: &Chunk,
        position: BlockPos,
        _registry: &impl BlockStateRegistry,
    ) -> Option<u32> {
        if position.x.div_euclid(16) != self.chunk_x || position.z.div_euclid(16) != self.chunk_z {
            return None;
        }
        let x = position.x.rem_euclid(16) as u8;
        let z = position.z.rem_euclid(16) as u8;
        chunk.block(x, position.y, z)
    }

    fn block_in_tag(&self, state: u32, tag: &str, registry: &impl BlockStateRegistry) -> bool {
        let Some(state) = registry.block_state(state) else {
            return false;
        };
        self.tag_contains(tag, &state.name, &mut BTreeSet::new())
    }

    fn tag_contains(&self, tag: &str, block: &str, seen: &mut BTreeSet<String>) -> bool {
        if !seen.insert(tag.to_owned()) {
            return false;
        }
        let Some(entry) = self.data.block_tag(tag) else {
            return false;
        };
        entry.values.iter().any(|value| {
            value.strip_prefix('#').map_or_else(
                || normalize_id(value) == normalize_id(block),
                |nested| self.tag_contains(nested, block, seen),
            )
        })
    }

    fn state_survives(
        &self,
        chunk: &Chunk,
        position: BlockPos,
        registry: &impl BlockStateRegistry,
    ) -> bool {
        self.state_survives_named(chunk, "minecraft:short_grass", position, registry)
    }

    fn state_survives_named(
        &self,
        chunk: &Chunk,
        block: &str,
        position: BlockPos,
        registry: &impl BlockStateRegistry,
    ) -> bool {
        if matches!(
            block,
            "minecraft:lily_pad" | "minecraft:seagrass" | "minecraft:kelp"
        ) {
            return self
                .block_at(chunk, position, registry)
                .and_then(|state| registry.block_state(state))
                .is_some_and(|state| state.name == "minecraft:water");
        }
        let below = position.offset(0, -1, 0);
        let below_name = self
            .block_at(chunk, below, registry)
            .and_then(|state| registry.block_state(state))
            .map(|state| state.name.as_str());
        if below_name.is_some_and(is_soil) {
            let state = self
                .block_at(chunk, position, registry)
                .and_then(|state| registry.block_state(state));
            return state.is_none_or(|state| {
                let flags =
                    registry.flags(self.block_at(chunk, position, registry).unwrap_or_default());
                flags.air || is_replaceable(&state.name)
            });
        }
        // `would_survive` is also used for saplings. Vanilla's block
        // `canSurvive` is block-specific; the soil check above covers the
        // overworld tree, grass and flower placements, and other blocks are
        // rejected so a feature does not drop invalid states.
        false
    }

    fn place_feature(
        &self,
        chunk: &mut Chunk,
        feature_id: &ResourceId,
        position: BlockPos,
        random: &mut dyn RandomSource,
        registry: &impl BlockStateRegistry,
        biome_at: &dyn Fn(i32, i32, i32) -> ResourceId,
    ) -> Result<(), FeatureError> {
        let Some(configured) = self.data.configured_feature(feature_id) else {
            return Err(FeatureError::MissingConfigured(feature_id.clone()));
        };
        match self.compile(configured)? {
            CompiledFeature::Ore {
                size,
                discard_chance_on_air_exposure,
                targets,
            } => {
                self.place_ore(
                    chunk,
                    position,
                    random,
                    size,
                    discard_chance_on_air_exposure,
                    &targets,
                    registry,
                );
            }
            CompiledFeature::SimpleBlock(provider) => {
                let Some(state) = self.provider_state(&provider, position, random, registry) else {
                    return Ok(());
                };
                if self.state_survives(chunk, position, registry)
                    && self.is_replaceable_position(chunk, position, registry)
                {
                    self.set_state(chunk, position, &state, registry)?;
                    if is_double_plant(&state.name) {
                        let upper = state.with("half", "upper");
                        self.set_state(chunk, position.offset(0, 1, 0), &upper, registry)?;
                    }
                }
            }
            CompiledFeature::RandomSelector { default, choices } => {
                let mut selected = default;
                for (chance, feature) in choices {
                    if random.next_float() < chance {
                        selected = feature;
                        break;
                    }
                }
                // `RandomSelectorFeature` holds placed features, so the chosen
                // entry runs its own placement modifiers from `position`.
                self.place_reference(chunk, &selected, position, random, registry, biome_at)?;
            }
            CompiledFeature::Tree(tree) => {
                self.place_tree(chunk, position, random, &tree, registry);
            }
            CompiledFeature::Unsupported => {}
        }
        Ok(())
    }

    /// Places a `PlacedFeature` reference (the payload of
    /// `minecraft:random_selector`), falling back to a configured feature for
    /// data that references one directly.
    #[allow(clippy::too_many_arguments)]
    fn place_reference(
        &self,
        chunk: &mut Chunk,
        id: &ResourceId,
        origin: BlockPos,
        random: &mut dyn RandomSource,
        registry: &impl BlockStateRegistry,
        biome_at: &dyn Fn(i32, i32, i32) -> ResourceId,
    ) -> Result<(), FeatureError> {
        let Some(placed) = self.data.placed_feature(id) else {
            return if self.data.configured_feature(id).is_some() {
                self.place_feature(chunk, id, origin, random, registry, biome_at)
            } else {
                Ok(())
            };
        };
        let modifiers: Vec<Modifier> = placed.placement.iter().map(Modifier::from_value).collect();
        if modifiers
            .iter()
            .any(|modifier| matches!(modifier, Modifier::Unsupported))
        {
            return Ok(());
        }
        let mut positions = Vec::new();
        self.apply_modifier(
            chunk,
            &modifiers,
            0,
            origin,
            random,
            &mut positions,
            biome_at,
            registry,
            id,
        );
        for position in positions {
            self.place_feature(chunk, &placed.feature, position, random, registry, biome_at)?;
        }
        Ok(())
    }

    fn compile(&self, configured: &ConfiguredFeature) -> Result<CompiledFeature, FeatureError> {
        let config = &configured.config;
        Ok(match configured.kind.to_string().as_str() {
            "minecraft:ore" => {
                let size = config
                    .get("size")
                    .and_then(Value::as_i64)
                    .and_then(|value| i32::try_from(value).ok())
                    .unwrap_or(0);
                let discard = config
                    .get("discard_chance_on_air_exposure")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0) as f32;
                let targets = config
                    .get("targets")
                    .and_then(Value::as_array)
                    .map(|targets| {
                        targets
                            .iter()
                            .filter_map(|target| {
                                let state = target.get("state").and_then(StateSpec::from_value)?;
                                let rule = target.get("target").map(Rule::from_value)?;
                                Some((rule, state))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                CompiledFeature::Ore {
                    size,
                    discard_chance_on_air_exposure: discard,
                    targets,
                }
            }
            "minecraft:simple_block" => config
                .get("to_place")
                .and_then(ProviderRef::from_value)
                .map_or(CompiledFeature::Unsupported, CompiledFeature::SimpleBlock),
            "minecraft:random_selector" => {
                #[derive(Deserialize)]
                struct Entry {
                    chance: f32,
                    feature: Value,
                }
                let feature_id = |value: &Value| -> Option<ResourceId> {
                    if let Some(id) = value.as_str() {
                        return ResourceId::parse(id).ok();
                    }
                    value
                        .get("feature")
                        .and_then(Value::as_str)
                        .and_then(|id| ResourceId::parse(id).ok())
                };
                let Some(default) = config.get("default").and_then(feature_id) else {
                    return Ok(CompiledFeature::Unsupported);
                };
                let choices = config
                    .get("features")
                    .and_then(Value::as_array)
                    .map(|entries| {
                        entries
                            .iter()
                            .filter_map(|entry| {
                                let entry: Entry = serde_json::from_value(entry.clone()).ok()?;
                                Some((entry.chance, feature_id(&entry.feature)?))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                CompiledFeature::RandomSelector { default, choices }
            }
            "minecraft:tree" => self
                .compile_tree(config)
                .map_or(CompiledFeature::Unsupported, |tree| {
                    CompiledFeature::Tree(Box::new(tree))
                }),
            _ => CompiledFeature::Unsupported,
        })
    }

    fn compile_tree(&self, config: &BTreeMap<String, Value>) -> Option<TreeConfig> {
        let trunk = config
            .get("trunk_provider")
            .and_then(ProviderRef::from_value)?;
        let foliage = config
            .get("foliage_provider")
            .and_then(ProviderRef::from_value)?;
        let below_trunk = config
            .get("below_trunk_provider")
            .and_then(ProviderRef::from_value);
        let trunk_placer = config.get("trunk_placer")?;
        if trunk_placer.get("type")?.as_str()? != "minecraft:straight_trunk_placer" {
            return None;
        }
        let foliage_placer = config.get("foliage_placer")?;
        let foliage_placer = match foliage_placer.get("type")?.as_str()? {
            "minecraft:blob_foliage_placer" => TreeFoliage::Blob {
                height: i32::try_from(foliage_placer.get("height")?.as_i64()?).ok()?,
                radius: i32::try_from(foliage_placer.get("radius")?.as_i64()?).ok()?,
                offset: i32::try_from(foliage_placer.get("offset")?.as_i64()?).ok()?,
            },
            "minecraft:spruce_foliage_placer" => TreeFoliage::Spruce {
                trunk_height: IntProvider::from_value(foliage_placer.get("trunk_height")?)?,
                radius: IntProvider::from_value(foliage_placer.get("radius")?)?,
                offset: IntProvider::from_value(foliage_placer.get("offset")?)?,
            },
            "minecraft:pine_foliage_placer" => TreeFoliage::Pine {
                height: IntProvider::from_value(foliage_placer.get("height")?)?,
                radius: IntProvider::from_value(foliage_placer.get("radius")?)?,
                offset: IntProvider::from_value(foliage_placer.get("offset")?)?,
            },
            _ => return None,
        };
        let size = config.get("minimum_size")?;
        let number = |value: &Value, name: &str, default: i64| {
            value
                .get(name)
                .and_then(Value::as_i64)
                .unwrap_or(default)
                .try_into()
                .ok()
        };
        Some(TreeConfig {
            trunk,
            foliage,
            below_trunk,
            base_height: i32::try_from(trunk_placer.get("base_height")?.as_i64()?).ok()?,
            height_rand_a: i32::try_from(trunk_placer.get("height_rand_a")?.as_i64()?).ok()?,
            height_rand_b: i32::try_from(trunk_placer.get("height_rand_b")?.as_i64()?).ok()?,
            foliage_placer,
            limit: number(size, "limit", 1)?,
            lower_size: number(size, "lower_size", 0)?,
            upper_size: number(size, "upper_size", 1)?,
            min_clipped_height: size
                .get("min_clipped_height")
                .and_then(Value::as_i64)
                .map(i32::try_from)
                .transpose()
                .ok()?,
        })
    }

    fn provider_state(
        &self,
        provider: &ProviderRef,
        position: BlockPos,
        random: &mut dyn RandomSource,
        registry: &impl BlockStateRegistry,
    ) -> Option<StateSpec> {
        let owned;
        let provider = match provider {
            ProviderRef::Inline(provider) => provider,
            ProviderRef::Id(id) => {
                owned = self.data.block_state_provider(id)?;
                owned
            }
        };
        let kind = provider.kind.as_ref().map(ToString::to_string);
        let result = match kind.as_deref() {
            Some("minecraft:noise_threshold") => {
                self.noise_threshold_state(provider, position, random)
            }
            Some("minecraft:weighted_state_provider") | Some("minecraft:weighted") => {
                let entries = provider.config.get("entries")?.as_array()?;
                let total: i32 = entries
                    .iter()
                    .map(|entry| entry.get("weight").and_then(Value::as_i64).unwrap_or(1) as i32)
                    .sum();
                let mut choice = random.next_int_bound(total.max(1));
                for entry in entries {
                    let weight = entry.get("weight").and_then(Value::as_i64).unwrap_or(1) as i32;
                    if choice < weight {
                        return StateSpec::from_value(entry.get("data")?);
                    }
                    choice -= weight;
                }
                entries
                    .last()
                    .and_then(|entry| entry.get("data"))
                    .and_then(StateSpec::from_value)
            }
            _ => {
                if let Some(state) = provider.config.get("state") {
                    StateSpec::from_value(state)
                } else {
                    // An inline provider without a `type` is a plain `{"id":...}`.
                    let value = Value::Object(
                        provider
                            .config
                            .iter()
                            .map(|(key, value)| (key.clone(), value.clone()))
                            .collect(),
                    );
                    StateSpec::from_value(&value)
                }
            }
        };
        let _ = registry;
        result
    }

    fn noise_threshold_state(
        &self,
        provider: &BlockStateProvider,
        position: BlockPos,
        random: &mut dyn RandomSource,
    ) -> Option<StateSpec> {
        let config = &provider.config;
        let seed = config.get("seed")?.as_i64()?;
        let noise = config.get("noise")?;
        let base_amplitude = noise.get("base_amplitude")?.as_f64()?;
        let base_octave = i32::try_from(noise.get("base_octave")?.as_i64()?).ok()?;
        let octaves = noise
            .get("amplitude_modifiers")
            .and_then(Value::as_array)
            .map_or(1, |modifiers| modifiers.len() + 1);
        let mut builder = NormalNoise::builder()
            .set_base_amplitude(base_amplitude)
            .set_base_octave(base_octave)
            .set_octave_count(octaves as i32);
        if let Some(modifiers) = noise.get("amplitude_modifiers").and_then(Value::as_array) {
            for (index, value) in modifiers.iter().enumerate() {
                builder = builder.set_amplitude_modifier(index as i32, value.as_f64()?);
            }
        }
        let mut random_source = XoroshiroRandomSource::new(seed);
        let noise = builder.build().create(&mut random_source);
        let scale = config.get("scale")?.as_f64()? as f32;
        let value = noise.get(
            f64::from(position.x) * f64::from(scale),
            f64::from(position.y) * f64::from(scale),
            f64::from(position.z) * f64::from(scale),
        );
        let threshold = config.get("threshold")?.as_f64()? as f32;
        if value < threshold {
            pick_state(config.get("low_states"), random)
                .or_else(|| config.get("default_state").and_then(StateSpec::from_value))
        } else if random.next_float() < config.get("high_chance")?.as_f64()? as f32 {
            pick_state(config.get("high_states"), random)
                .or_else(|| config.get("default_state").and_then(StateSpec::from_value))
        } else {
            config.get("default_state").and_then(StateSpec::from_value)
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn place_ore(
        &self,
        chunk: &mut Chunk,
        origin: BlockPos,
        random: &mut dyn RandomSource,
        size: i32,
        discard_chance_on_air_exposure: f32,
        targets: &[(Rule, StateSpec)],
        registry: &impl BlockStateRegistry,
    ) {
        if size <= 0 || targets.is_empty() {
            return;
        }
        // `OreFeature.place` in the 26.3 server jar.
        let angle = random.next_float() * std::f32::consts::PI;
        let breadth = size as f32 / 8.0;
        let start = (
            f64::from(origin.x) + f64::from(angle.sin()) * f64::from(breadth),
            f64::from(origin.y) + f64::from(random.next_int_bound(3) - 2),
            f64::from(origin.z) + f64::from(angle.cos()) * f64::from(breadth),
        );
        let end = (
            f64::from(origin.x) - f64::from(angle.sin()) * f64::from(breadth),
            f64::from(origin.y) + f64::from(random.next_int_bound(3) - 2),
            f64::from(origin.z) - f64::from(angle.cos()) * f64::from(breadth),
        );
        let mut segments = Vec::with_capacity(size as usize);
        for index in 0..size {
            let progress = index as f32 / size as f32;
            let x = lerp_f64(f64::from(progress), start.0, end.0);
            let y = lerp_f64(f64::from(progress), start.1, end.1);
            let z = lerp_f64(f64::from(progress), start.2, end.2);
            let radius_base = random.next_double() * f64::from(size) / 16.0;
            let radius = ((f64::from((std::f32::consts::PI * progress).sin()) + 1.0) * radius_base
                + 1.0)
                / 2.0;
            segments.push((x, y, z, radius));
        }
        for a in 0..segments.len() {
            if segments[a].3 <= 0.0 {
                continue;
            }
            for b in (a + 1)..segments.len() {
                if segments[b].3 <= 0.0 {
                    continue;
                }
                let (ax, ay, az, ar) = segments[a];
                let (bx, by, bz, br) = segments[b];
                let distance = (ax - bx).powi(2) + (ay - by).powi(2) + (az - bz).powi(2);
                let delta = ar - br;
                if delta * delta > distance {
                    if delta > 0.0 {
                        segments[b].3 = -1.0;
                    } else {
                        segments[a].3 = -1.0;
                    }
                }
            }
        }
        let mut placed = false;
        for (x, y, z, radius) in segments {
            if radius <= 0.0 {
                continue;
            }
            let min_x = (x - radius).floor() as i32;
            let max_x = (x + radius).floor() as i32;
            let min_y = (y - radius).floor() as i32;
            let max_y = (y + radius).floor() as i32;
            let min_z = (z - radius).floor() as i32;
            let max_z = (z + radius).floor() as i32;
            for block_x in min_x..=max_x {
                for block_y in min_y..=max_y {
                    for block_z in min_z..=max_z {
                        let dx = (f64::from(block_x) + 0.5 - x) / radius;
                        let dy = (f64::from(block_y) + 0.5 - y) / radius;
                        let dz = (f64::from(block_z) + 0.5 - z) / radius;
                        if dx * dx + dy * dy + dz * dz >= 1.0 {
                            continue;
                        }
                        let position = BlockPos::new(block_x, block_y, block_z);
                        let Some(current) = self.block_at(chunk, position, registry) else {
                            continue;
                        };
                        for (rule, state) in targets {
                            if !self.rule_matches(rule, current, block_y, registry) {
                                continue;
                            }
                            if !self.can_place_ore(
                                chunk,
                                position,
                                random,
                                discard_chance_on_air_exposure,
                                registry,
                            ) {
                                continue;
                            }
                            if self.set_state(chunk, position, state, registry).is_ok() {
                                placed = true;
                            }
                            break;
                        }
                    }
                }
            }
        }
        let _ = placed;
    }

    fn can_place_ore(
        &self,
        chunk: &Chunk,
        position: BlockPos,
        random: &mut dyn RandomSource,
        discard_chance: f32,
        registry: &impl BlockStateRegistry,
    ) -> bool {
        if discard_chance <= 0.0 {
            return true;
        }
        if random.next_float() >= discard_chance {
            return true;
        }
        // `AbstractOreFeature`: a chance of 1.0 means "discard only when air
        // exposed", not "discard every block".
        !self.adjacent_to_air(chunk, position, registry)
    }

    fn adjacent_to_air(
        &self,
        chunk: &Chunk,
        position: BlockPos,
        registry: &impl BlockStateRegistry,
    ) -> bool {
        [
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
        ]
        .into_iter()
        .any(|(dx, dy, dz)| {
            self.block_at(chunk, position.offset(dx, dy, dz), registry)
                .and_then(|state| registry.block_state(state))
                .is_some_and(|state| state.name == "minecraft:air")
        })
    }

    fn rule_matches(
        &self,
        rule: &Rule,
        state: u32,
        y: i32,
        registry: &impl BlockStateRegistry,
    ) -> bool {
        match rule {
            Rule::Tag(tag) => self.block_in_tag(state, tag, registry),
            Rule::Height { min, max } => (*min..=*max).contains(&y),
            Rule::All(rules) => rules
                .iter()
                .all(|rule| self.rule_matches(rule, state, y, registry)),
            Rule::Any(rules) => rules
                .iter()
                .any(|rule| self.rule_matches(rule, state, y, registry)),
            Rule::Not(rule) => !self.rule_matches(rule, state, y, registry),
            Rule::Unsupported => false,
        }
    }

    fn place_tree(
        &self,
        chunk: &mut Chunk,
        origin: BlockPos,
        random: &mut dyn RandomSource,
        tree: &TreeConfig,
        registry: &impl BlockStateRegistry,
    ) {
        let height = tree.base_height
            + random.next_int_bound(tree.height_rand_a + 1)
            + random.next_int_bound(tree.height_rand_b + 1);
        // Vanilla samples foliage height and radius before checking the tree's free space.
        let (foliage_height, foliage_radius) = match &tree.foliage_placer {
            TreeFoliage::Blob { height, radius, .. } => (*height, *radius),
            TreeFoliage::Spruce {
                trunk_height,
                radius,
                ..
            } => (
                (height - trunk_height.sample(random)).max(4),
                radius.sample(random),
            ),
            TreeFoliage::Pine {
                height: foliage_height,
                radius,
                ..
            } => {
                let foliage_height = foliage_height.sample(random);
                let radius = radius.sample(random)
                    + random.next_int_bound((height - foliage_height + 1).max(1));
                (foliage_height, radius)
            }
        };
        let free = self.max_free_tree_height(chunk, origin, height, tree, registry);
        if free < height
            && !tree
                .min_clipped_height
                .is_some_and(|minimum| free >= minimum)
        {
            return;
        }
        let height = height.min(free);
        if let Some(below) = &tree.below_trunk
            && let Some(state) =
                self.provider_state(below, origin.offset(0, -1, 0), random, registry)
            && self.valid_tree_pos(chunk, origin.offset(0, -1, 0), registry)
        {
            let _ = self.set_state(chunk, origin.offset(0, -1, 0), &state, registry);
        }
        let Some(trunk) = self.provider_state(&tree.trunk, origin, random, registry) else {
            return;
        };
        for y in 0..height {
            let position = origin.offset(0, y, 0);
            if self.valid_tree_pos(chunk, position, registry) {
                let _ = self.set_state(chunk, position, &trunk, registry);
            }
        }
        let foliage_offset = match &tree.foliage_placer {
            TreeFoliage::Blob { offset, .. } => *offset,
            TreeFoliage::Spruce { offset, .. } | TreeFoliage::Pine { offset, .. } => {
                offset.sample(random)
            }
        };
        let Some(foliage) = self.provider_state(&tree.foliage, origin, random, registry) else {
            return;
        };
        match &tree.foliage_placer {
            TreeFoliage::Blob { .. } => {
                let top = height + foliage_offset;
                for y in (top - foliage_height..=top).rev() {
                    let offset = top - y;
                    let radius = (foliage_radius - 1 - offset / 2).max(0);
                    for dx in -radius..=radius {
                        for dz in -radius..=radius {
                            if dx.abs() == radius
                                && dz.abs() == radius
                                && (random.next_int_bound(2) == 0 || y == top)
                            {
                                continue;
                            }
                            let position = origin.offset(dx, y, dz);
                            if self.valid_tree_pos(chunk, position, registry) {
                                let _ = self.set_state(chunk, position, &foliage, registry);
                            }
                        }
                    }
                }
            }
            TreeFoliage::Spruce { .. } => {
                let mut radius = random.next_int_bound(2);
                let (mut max_radius, mut min_radius) = (1, 0);
                for y in (-foliage_height..=foliage_offset).rev() {
                    self.place_conifer_row(chunk, origin, height + y, radius, &foliage, registry);
                    if radius >= max_radius {
                        radius = min_radius;
                        min_radius = 1;
                        max_radius = (max_radius + 1).min(foliage_radius);
                    } else {
                        radius += 1;
                    }
                }
            }
            TreeFoliage::Pine { .. } => {
                let mut radius = 0;
                let bottom = foliage_offset - foliage_height;
                for y in (bottom..=foliage_offset).rev() {
                    self.place_conifer_row(chunk, origin, height + y, radius, &foliage, registry);
                    if radius >= 1 && y == bottom + 1 {
                        radius -= 1;
                    } else if radius < foliage_radius {
                        radius += 1;
                    }
                }
            }
        }
    }

    fn place_conifer_row(
        &self,
        chunk: &mut Chunk,
        origin: BlockPos,
        y: i32,
        radius: i32,
        foliage: &StateSpec,
        registry: &impl BlockStateRegistry,
    ) {
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                if radius > 0 && dx.abs() == radius && dz.abs() == radius {
                    continue;
                }
                let position = origin.offset(dx, y, dz);
                if self.valid_tree_pos(chunk, position, registry) {
                    let _ = self.set_state(chunk, position, foliage, registry);
                }
            }
        }
    }

    fn max_free_tree_height(
        &self,
        chunk: &Chunk,
        origin: BlockPos,
        height: i32,
        tree: &TreeConfig,
        registry: &impl BlockStateRegistry,
    ) -> i32 {
        for y in 0..=height + 1 {
            let size = if y < tree.limit {
                tree.lower_size
            } else {
                tree.upper_size
            };
            for dx in -size..=size {
                for dz in -size..=size {
                    if !self.valid_tree_pos(chunk, origin.offset(dx, y, dz), registry) {
                        return y - 2;
                    }
                }
            }
        }
        height
    }

    fn valid_tree_pos(
        &self,
        chunk: &Chunk,
        position: BlockPos,
        registry: &impl BlockStateRegistry,
    ) -> bool {
        self.block_at(chunk, position, registry)
            .and_then(|state| registry.block_state(state))
            .is_none_or(|state| {
                let flags =
                    registry.flags(self.block_at(chunk, position, registry).unwrap_or_default());
                flags.air
                    || flags.leaves
                    || state.name.ends_with("_leaves")
                    || state.name == "minecraft:water"
                    || state.name == "minecraft:vine"
            })
    }

    fn is_replaceable_position(
        &self,
        chunk: &Chunk,
        position: BlockPos,
        registry: &impl BlockStateRegistry,
    ) -> bool {
        self.block_at(chunk, position, registry)
            .and_then(|state| registry.block_state(state))
            .is_none_or(|state| {
                let flags =
                    registry.flags(self.block_at(chunk, position, registry).unwrap_or_default());
                flags.air || is_replaceable(&state.name)
            })
    }

    fn set_state(
        &self,
        chunk: &mut Chunk,
        position: BlockPos,
        state: &StateSpec,
        registry: &impl BlockStateRegistry,
    ) -> Result<(), FeatureError> {
        if position.x.div_euclid(16) != self.chunk_x || position.z.div_euclid(16) != self.chunk_z {
            return Ok(());
        }
        let id = registry
            .block_id(&state.name, &state.properties)
            .unwrap_or_else(|| {
                registry
                    .block_id(&state.name, &BTreeMap::new())
                    .expect("feature block state exists in the vanilla registry")
            });
        chunk.set_block(
            position.x.rem_euclid(16) as u8,
            position.y,
            position.z.rem_euclid(16) as u8,
            id,
            registry,
        )?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct FeatureKey {
    index: usize,
    step: usize,
}

/// Builds the global decoration order the way `FeatureSorter.buildFeaturesPerStep`
/// does:
///
/// 1. number every placed feature by first encounter while walking the biome
///    source's possible biomes and their per-step lists,
/// 2. add a graph edge from each feature to the next feature in the same biome,
/// 3. depth-first search in `(step, index)` order, collecting a post-order list,
/// 4. reverse it and group by step.
///
/// Vanilla seeds a placed feature with its position in the resulting step list.
fn global_steps(data: &WorldgenData) -> Vec<Vec<ResourceId>> {
    let mut biomes = Vec::new();
    if let Some(parameters) = data.biome_parameters("minecraft:overworld") {
        for entry in &parameters.biomes {
            if !biomes.contains(&entry.biome) {
                biomes.push(entry.biome.clone());
            }
        }
    }
    let mut indices: BTreeMap<ResourceId, usize> = BTreeMap::new();
    let mut nodes: BTreeMap<FeatureKey, ResourceId> = BTreeMap::new();
    let mut edges: BTreeMap<FeatureKey, Vec<FeatureKey>> = BTreeMap::new();
    let mut max_step = 0;
    for biome_id in &biomes {
        let Some(biome) = data.biome(&biome_id.to_string()) else {
            continue;
        };
        let mut flat = Vec::new();
        for (step, features) in biome.features.iter().enumerate() {
            max_step = max_step.max(step);
            for feature in features {
                let next = indices.len();
                let index = *indices.entry(feature.clone()).or_insert(next);
                let key = FeatureKey { index, step };
                nodes.insert(key, feature.clone());
                flat.push(key);
            }
        }
        // `FeatureSorter` links consecutive entries across the biome's whole
        // step/flattened feature list, not only within one step.
        for pair in flat.windows(2) {
            edges.entry(pair[0]).or_default().push(pair[1]);
        }
    }
    // Vanilla stores neighbours in a `TreeSet` ordered like the graph keys.
    for neighbours in edges.values_mut() {
        neighbours.sort_by_key(|key| (key.index, key.step));
        neighbours.dedup();
    }

    let mut keys: Vec<FeatureKey> = nodes.keys().copied().collect();
    keys.sort_by_key(|key| (key.index, key.step));
    let mut visited = BTreeSet::new();
    let mut in_progress = BTreeSet::new();
    let mut post_order = Vec::new();
    for key in keys {
        visit_feature(key, &edges, &mut visited, &mut in_progress, &mut post_order);
    }
    post_order.reverse();

    let mut steps = vec![Vec::new(); max_step + 1];
    for key in post_order {
        steps[key.step].push(nodes[&key].clone());
    }
    steps
}

fn visit_feature(
    key: FeatureKey,
    edges: &BTreeMap<FeatureKey, Vec<FeatureKey>>,
    visited: &mut BTreeSet<FeatureKey>,
    in_progress: &mut BTreeSet<FeatureKey>,
    post_order: &mut Vec<FeatureKey>,
) {
    if visited.contains(&key) || !in_progress.insert(key) {
        return;
    }
    if let Some(next) = edges.get(&key) {
        for key in next {
            visit_feature(*key, edges, visited, in_progress, post_order);
        }
    }
    in_progress.remove(&key);
    visited.insert(key);
    post_order.push(key);
}

fn lerp_f64(delta: f64, start: f64, end: f64) -> f64 {
    start + delta * (end - start)
}

fn normalize_id(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

fn pick_state(states: Option<&Value>, random: &mut dyn RandomSource) -> Option<StateSpec> {
    let states = states?.as_array()?;
    if states.is_empty() {
        return None;
    }
    let index = random.next_int_bound(states.len() as i32) as usize;
    StateSpec::from_value(&states[index])
}

fn is_soil(name: &str) -> bool {
    matches!(
        name,
        "minecraft:dirt"
            | "minecraft:grass_block"
            | "minecraft:podzol"
            | "minecraft:coarse_dirt"
            | "minecraft:rooted_dirt"
            | "minecraft:mud"
            | "minecraft:moss_block"
            | "minecraft:mycelium"
            | "minecraft:farmland"
            | "minecraft:dirt_path"
            | "minecraft:sand"
            | "minecraft:red_sand"
            | "minecraft:gravel"
    )
}

fn is_replaceable(name: &str) -> bool {
    matches!(
        name,
        "minecraft:short_grass"
            | "minecraft:tall_grass"
            | "minecraft:fern"
            | "minecraft:large_fern"
            | "minecraft:snow"
            | "minecraft:dead_bush"
            | "minecraft:vine"
    )
}

fn is_double_plant(name: &str) -> bool {
    matches!(
        name,
        "minecraft:tall_grass"
            | "minecraft:large_fern"
            | "minecraft:sunflower"
            | "minecraft:lilac"
            | "minecraft:rose_bush"
            | "minecraft:peony"
    )
}

#[cfg(test)]
mod tests {
    use super::{
        BlockPos, FeatureGenerator, ResourceId, WorldgenData, decoration_seed, feature_seed,
    };
    use ferrite_noise::XoroshiroRandomSource;
    use ferrite_world::{BlockStateRegistry, Chunk, RuntimeRegistry};
    use std::path::PathBuf;

    #[test]
    fn feature_seed_uses_the_vanilla_offsets() {
        let decoration = decoration_seed(42, 16, -32);
        assert_eq!(feature_seed(decoration, 0, 0), decoration);
        assert_eq!(feature_seed(decoration, 3, 2), decoration + 3 + 20_000);
    }

    #[test]
    fn decoration_seed_is_stable() {
        assert_eq!(decoration_seed(0, 0, 0), 0);
        assert_eq!(decoration_seed(42, 16, -32), decoration_seed(42, 16, -32));
        assert_ne!(decoration_seed(42, 16, -32), decoration_seed(42, 16, 32));
    }

    #[test]
    fn vanilla_spruce_and_pine_features_place_trunks_and_conifer_canopies() {
        let generated =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3/generated");
        let data = WorldgenData::load(&generated).unwrap();
        let registry = RuntimeRegistry::load(&generated).unwrap();
        let generator = FeatureGenerator::new(&data, 0, 0, 128);
        let air = registry
            .block_id("minecraft:air", &Default::default())
            .unwrap();
        let biome = registry.biome_id("minecraft:plains").unwrap();

        for name in ["spruce", "pine"] {
            let id = ResourceId::parse(name).unwrap();
            let config = &data.configured_features.get(&id).unwrap().config;
            let tree = generator
                .compile_tree(config)
                .expect("vanilla conifer config");
            let mut chunk = Chunk::new(
                0,
                128,
                air,
                biome,
                registry.block_bits(),
                registry.biome_bits(),
            )
            .unwrap();
            let mut random = XoroshiroRandomSource::new(42);
            generator.place_tree(
                &mut chunk,
                BlockPos { x: 8, y: 32, z: 8 },
                &mut random,
                &tree,
                &registry,
            );
            let mut logs = 0;
            let mut leaves = 0;
            let mut wide_leaves = 0;
            for y in 32..48 {
                for z in 3..14 {
                    for x in 3..14 {
                        let state = chunk.block(x, y, z).unwrap();
                        let name = &registry.block_state(state).unwrap().name;
                        if name == "minecraft:spruce_log" {
                            logs += 1;
                        }
                        if name == "minecraft:spruce_leaves" {
                            leaves += 1;
                            if (i32::from(x) - 8).abs().max((i32::from(z) - 8).abs()) >= 2 {
                                wide_leaves += 1;
                            }
                        }
                    }
                }
            }
            assert!((5..=10).contains(&logs), "{name} logs: {logs}");
            assert!(
                leaves > 10 && wide_leaves > 0,
                "{name} leaves: {leaves}, wide: {wide_leaves}"
            );

            let stone = registry
                .block_id("minecraft:stone", &Default::default())
                .unwrap();
            let mut blocked = Chunk::new(
                0,
                128,
                air,
                biome,
                registry.block_bits(),
                registry.biome_bits(),
            )
            .unwrap();
            blocked.set_block(8, 35, 8, stone, &registry).unwrap();
            let mut random = XoroshiroRandomSource::new(42);
            generator.place_tree(
                &mut blocked,
                BlockPos { x: 8, y: 32, z: 8 },
                &mut random,
                &tree,
                &registry,
            );
            for y in 32..48 {
                for z in 3..14 {
                    for x in 3..14 {
                        let state = blocked.block(x, y, z).unwrap();
                        let block = &registry.block_state(state).unwrap().name;
                        assert!(
                            block != "minecraft:spruce_log" && block != "minecraft:spruce_leaves",
                            "{name} created a partial tree at {x},{y},{z}"
                        );
                    }
                }
            }
        }
    }
}
