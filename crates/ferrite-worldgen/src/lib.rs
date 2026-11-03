//! Vanilla world generation data (Minecraft Java 26.3).
//!
//! This crate is the phase-6 foundation: it parses the data generator output under
//! `vanilla/26.3/generated/` into typed Rust structures and evaluates overworld density
//! functions using seeded noise.
//!
//! The 26.3 data format differs from the older schemas that most documentation describes:
//!
//! * noise parameters use `base_octave` / `octave_count` / `base_amplitude` /
//!   `amplitude_modifiers` instead of `firstOctave` / `amplitudes`,
//! * density functions use `minecraft:gradient` (with an `axis`), `minecraft:cache`,
//!   `minecraft:lerp`, `minecraft:interval_select`, `minecraft:distance_to_point`, and
//!   `minecraft:end_outer_islands`, and noise shifts are inline `shift_x` / `shift_z`,
//! * surface rules are the separate `worldgen/material_rule` and
//!   `worldgen/material_condition` registries referenced from `noise_settings.material_rule`,
//! * dimensions are only emitted inside `worldgen/world_preset` files; this crate can load
//!   `dimension/*.json` too when a data pack provides them.
//!
//! Everything is loaded from `data/minecraft`, with `minecraft:` assumed for missing
//! namespaces. Biome and dimension-type `attributes` payloads are kept as raw JSON because
//! they are registry-keyed attribute values; the keys themselves are preserved rather than
//! dropped.
//!
//! # Strictness
//!
//! [`WorldgenData::load`] is lenient about fields the typed model does not know.
//! [`WorldgenData::load_strict`] re-serializes every loaded value and checks that every key in
//! the source file survived, which is the equivalent of `deny_unknown_fields` for a schema
//! that contains untagged enums (where `serde_ignored` cannot see dropped fields). The tests
//! load the complete vanilla data set in strict mode so a new vanilla field fails loudly.

mod aquifer;
mod biome;
mod biome_source;
mod carver;
mod density;
mod dimension;
mod evaluator;
mod feature;
mod id;
mod loader;
mod material;
mod multi_noise;
mod noise;
mod noise_chunk;
mod noise_settings;
mod resolve;
mod seeded_noise;
mod strict;
mod structure;
mod surface;

use std::collections::BTreeMap;
use std::path::Path;

pub use biome::{Biome, BiomeEffects, Carvers, GrassColorModifier, TemperatureModifier};
pub use biome_source::{
    ClimateTargetPoint, MultiNoiseBiomeSource, MultiNoiseError, quantize_coord,
};
pub use density::{
    Axis, DensityFunction, DensityNode, DistanceMetric, Spline, SplinePoint, SplineValue,
};
pub use dimension::{
    BiomeSource, Dimension, DimensionType, FlatLayer, FlatSettings, IntProvider, WorldGenerator,
    WorldPreset,
};
pub use evaluator::FunctionContext;
pub use feature::{
    BlockStateProvider, BlockTag, ConfiguredFeature, FeatureError, FeatureGenerator, PlacedFeature,
    WorldgenRandom, decoration_seed, feature_seed,
};
pub use id::{InvalidResourceId, ResourceId};
pub use loader::LoadError;
pub use material::{
    BiomeSet, MaterialCondition, MaterialConditionNode, MaterialRule, MaterialRuleNode,
    SurfaceType, VerticalAnchor,
};
pub use multi_noise::{
    BiomeParameterPoint, MultiNoiseBiomeEntry, MultiNoiseParameters, MultiNoisePreset,
    ParameterRange,
};
pub use noise::NoiseParameters;
pub use noise_chunk::{NoiseChunkError, NoiseChunkGenerator};
pub use structure::random_spread_candidate;

pub use noise_settings::{
    AquiferSettings, DebugFunction, NoiseRouter, NoiseSamplingSettings, NoiseSettings,
};
pub use resolve::ResolveError;
pub use seeded_noise::{SeededContext, SeededNoise};
pub use surface::{SurfaceError, SurfaceGenerator};

/// All world generation data loaded from one generated data set.
///
/// The maps are keyed by fully qualified resource ids (`minecraft:overworld/continents`).
/// Use the lookup methods to resolve ids with or without the `minecraft:` prefix.
#[derive(Clone, Debug, Default)]
pub struct WorldgenData {
    pub noises: BTreeMap<ResourceId, NoiseParameters>,
    pub density_functions: BTreeMap<ResourceId, DensityFunction>,
    pub noise_settings: BTreeMap<ResourceId, NoiseSettings>,
    pub biomes: BTreeMap<ResourceId, Biome>,
    pub material_conditions: BTreeMap<ResourceId, MaterialCondition>,
    pub material_rules: BTreeMap<ResourceId, MaterialRule>,
    pub configured_features: BTreeMap<ResourceId, ConfiguredFeature>,
    pub placed_features: BTreeMap<ResourceId, PlacedFeature>,
    /// Raw structure-set definitions, including placement and weighted entries.
    pub structure_sets: BTreeMap<ResourceId, serde_json::Value>,
    /// Raw structure definitions; individual structure generators are not yet implemented.
    pub structures: BTreeMap<ResourceId, serde_json::Value>,
    /// Biome tags used by structure definitions.
    pub biome_tags: BTreeMap<ResourceId, serde_json::Value>,
    pub block_state_providers: BTreeMap<ResourceId, BlockStateProvider>,
    pub block_tags: BTreeMap<ResourceId, BlockTag>,
    pub dimension_types: BTreeMap<ResourceId, DimensionType>,
    pub dimensions: BTreeMap<ResourceId, Dimension>,
    pub world_presets: BTreeMap<ResourceId, WorldPreset>,
    pub multi_noise_presets: BTreeMap<ResourceId, MultiNoisePreset>,
    pub biome_parameters: BTreeMap<ResourceId, MultiNoiseParameters>,
}

impl WorldgenData {
    /// Loads the generated data set rooted at `dir`.
    ///
    /// `dir` may be either the generated root (the directory containing `data/` and
    /// `reports/`, normally `vanilla/26.3/generated`) or the `data/minecraft` directory
    /// itself. Missing directories are treated as empty; `data/minecraft` must exist.
    pub fn load(dir: impl AsRef<Path>) -> Result<Self, LoadError> {
        loader::load(dir.as_ref(), false)
    }

    /// Like [`Self::load`], but fails when a JSON field would be dropped by the typed model.
    ///
    /// This is meant for tests and data upgrades, not for serving players.
    pub fn load_strict(dir: impl AsRef<Path>) -> Result<Self, LoadError> {
        loader::load(dir.as_ref(), true)
    }

    /// Number of entries in each registry, for logging and data-set sanity checks.
    pub fn counts(&self) -> Counts {
        Counts {
            noises: self.noises.len(),
            density_functions: self.density_functions.len(),
            noise_settings: self.noise_settings.len(),
            biomes: self.biomes.len(),
            material_conditions: self.material_conditions.len(),
            material_rules: self.material_rules.len(),
            configured_features: self.configured_features.len(),
            placed_features: self.placed_features.len(),
            block_state_providers: self.block_state_providers.len(),
            block_tags: self.block_tags.len(),
            dimension_types: self.dimension_types.len(),
            dimensions: self.dimensions.len(),
            dimension_definitions: self
                .world_presets
                .values()
                .map(|preset| preset.dimensions.len())
                .sum(),
            world_presets: self.world_presets.len(),
            multi_noise_presets: self.multi_noise_presets.len(),
            biome_parameters: self.biome_parameters.len(),
        }
    }

    /// Noise parameters by id.
    pub fn noise(&self, id: &str) -> Option<&NoiseParameters> {
        lookup(&self.noises, id)
    }

    /// Density function by id.
    pub fn density_function(&self, id: &str) -> Option<&DensityFunction> {
        lookup(&self.density_functions, id)
    }

    /// Noise settings by id.
    pub fn noise_settings(&self, id: &str) -> Option<&NoiseSettings> {
        lookup(&self.noise_settings, id)
    }

    /// Biome by id.
    pub fn biome(&self, id: &str) -> Option<&Biome> {
        lookup(&self.biomes, id)
    }

    /// Material condition by id.
    pub fn material_condition(&self, id: &str) -> Option<&MaterialCondition> {
        lookup(&self.material_conditions, id)
    }

    /// Material (surface) rule by id.
    pub fn material_rule(&self, id: &str) -> Option<&MaterialRule> {
        lookup(&self.material_rules, id)
    }

    /// Configured feature by id.
    pub fn configured_feature(&self, id: &ResourceId) -> Option<&ConfiguredFeature> {
        self.configured_features.get(id)
    }

    /// Placed feature by id.
    pub fn placed_feature(&self, id: &ResourceId) -> Option<&PlacedFeature> {
        self.placed_features.get(id)
    }

    /// Block state provider by id.
    pub fn block_state_provider(&self, id: &ResourceId) -> Option<&BlockStateProvider> {
        self.block_state_providers.get(id)
    }

    /// Block tag by id, accepting it with or without the namespace.
    pub fn block_tag(&self, id: &str) -> Option<&BlockTag> {
        lookup(&self.block_tags, id)
    }

    /// Dimension type by id.
    pub fn dimension_type(&self, id: &str) -> Option<&DimensionType> {
        lookup(&self.dimension_types, id)
    }

    /// Standalone dimension definition by id (data packs may provide these).
    pub fn dimension(&self, id: &str) -> Option<&Dimension> {
        lookup(&self.dimensions, id)
    }

    /// World preset by id.
    pub fn world_preset(&self, id: &str) -> Option<&WorldPreset> {
        lookup(&self.world_presets, id)
    }

    /// Multi-noise parameter-list preset (`{ "preset": ... }`) by id.
    pub fn multi_noise_preset(&self, id: &str) -> Option<&MultiNoisePreset> {
        lookup(&self.multi_noise_presets, id)
    }

    /// Generated multi-noise biome parameters (from `reports/biome_parameters`) by id.
    pub fn biome_parameters(&self, id: &str) -> Option<&MultiNoiseParameters> {
        lookup(&self.biome_parameters, id)
    }
}

/// Registry sizes in a loaded data set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub noises: usize,
    pub density_functions: usize,
    pub noise_settings: usize,
    pub biomes: usize,
    pub material_conditions: usize,
    pub material_rules: usize,
    pub configured_features: usize,
    pub placed_features: usize,
    pub block_state_providers: usize,
    pub block_tags: usize,
    pub dimension_types: usize,
    /// Standalone `dimension/*.json` files. The vanilla 26.3 data generator emits none.
    pub dimensions: usize,
    /// Dimension definitions nested in world presets (three per preset in vanilla).
    pub dimension_definitions: usize,
    pub world_presets: usize,
    pub multi_noise_presets: usize,
    pub biome_parameters: usize,
}

/// Looks up an id, accepting it with or without the `minecraft:` namespace.
fn lookup<'a, T>(map: &'a BTreeMap<ResourceId, T>, id: &str) -> Option<&'a T> {
    map.get(&ResourceId::parse(id).ok()?)
}
