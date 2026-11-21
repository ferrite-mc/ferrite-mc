//! Pure-Rust, lock-free, parallelizable [`ChunkGenerator`] backed by `ferrite-worldgen`.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use ferrite_world::{
    BlockStateRegistry, Chunk, HeightmapKind, LightData, LightSection, RuntimeRegistry,
};
use ferrite_worldgen::biome::{EndBiomeSource, MultiNoiseBiomeSource, Parameter, ParameterPoint};
use ferrite_worldgen::chunk_gen::{NoiseChunkGenerator, TerrainPalette};
use ferrite_worldgen::vanilla_loader::{
    load_end_generator, load_multi_noise_biomes, load_nether_generator, load_overworld_generator,
};

use super::Dimension;
use super::generator::{ChunkGenerator, Generated, Placed, TreeContext};

/// Pure-Rust world generator safe for concurrent multi-region chunk generation.
pub struct RustGenerator {
    overworld: NoiseChunkGenerator,
    nether: NoiseChunkGenerator,
    end: NoiseChunkGenerator,
    registry: Arc<RuntimeRegistry>,
    biome_tags: HashMap<String, Vec<u32>>,
}

impl RustGenerator {
    /// Builds a `RustGenerator` for `seed` using vanilla data under `vanilla_data` when present.
    pub fn new(seed: u64, registry: Arc<RuntimeRegistry>) -> Self {
        let default_data =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3/generated");
        Self::with_vanilla_data(seed, registry, &default_data)
    }

    /// Builds a `RustGenerator` loading vanilla Overworld/Nether/End `worldgen/` JSON from `vanilla_data`.
    pub fn with_vanilla_data(
        seed: u64,
        registry: Arc<RuntimeRegistry>,
        vanilla_data: &Path,
    ) -> Self {
        let state = |name: &str| ferrite_data::blocks::default_state(name).unwrap_or(0);
        let palette = TerrainPalette {
            air: state("minecraft:air"),
            stone: state("minecraft:stone"),
            deepslate: state("minecraft:deepslate"),
            bedrock: state("minecraft:bedrock"),
            water: state("minecraft:water"),
            grass_block: state("minecraft:grass_block"),
            dirt: state("minecraft:dirt"),
            sand: state("minecraft:sand"),
            netherrack: state("minecraft:netherrack"),
            lava: state("minecraft:lava"),
            end_stone: state("minecraft:end_stone"),
        };

        let resolve_biome = |name: &str| -> u32 {
            let full = if name.contains(':') {
                name.to_owned()
            } else {
                format!("minecraft:{name}")
            };
            let short = full.strip_prefix("minecraft:").unwrap_or(&full);
            registry.biome_id(&full).unwrap_or_else(|| {
                ferrite_data::registries::id("worldgen/biome", short).unwrap_or(0) as u32
            })
        };

        let fallback_multi = |biome_name: &str| {
            MultiNoiseBiomeSource::new(vec![(
                ParameterPoint {
                    temperature: Parameter::span(-1.0, 1.0),
                    humidity: Parameter::span(-1.0, 1.0),
                    continentalness: Parameter::span(-1.0, 1.0),
                    erosion: Parameter::span(-1.0, 1.0),
                    depth: Parameter::span(-1.0, 1.0),
                    weirdness: Parameter::span(-1.0, 1.0),
                    offset: 0.0,
                },
                resolve_biome(biome_name),
            )])
        };

        let worldgen_dir = vanilla_data.join("data/minecraft/worldgen");

        let overworld_biomes = load_multi_noise_biomes(
            &vanilla_data.join("reports/biome_parameters/minecraft/overworld.json"),
            resolve_biome,
        )
        .unwrap_or_else(|_| fallback_multi("plains"));
        let overworld = load_overworld_generator(
            &worldgen_dir,
            seed,
            palette,
            overworld_biomes.clone(),
        )
        .unwrap_or_else(|err| {
            tracing::warn!(%err, "failed to load vanilla overworld JSON; falling back to built-in overworld");
            NoiseChunkGenerator::overworld(seed, palette, overworld_biomes)
        });

        let nether_biomes = load_multi_noise_biomes(
            &vanilla_data.join("reports/biome_parameters/minecraft/nether.json"),
            resolve_biome,
        )
        .unwrap_or_else(|_| fallback_multi("nether_wastes"));
        let nether = load_nether_generator(&worldgen_dir, seed, palette, nether_biomes.clone())
            .unwrap_or_else(|err| {
                tracing::warn!(%err, "failed to load vanilla nether JSON; falling back to built-in nether");
                NoiseChunkGenerator::nether(seed, palette, nether_biomes)
            });

        let end_biomes = EndBiomeSource {
            the_end: resolve_biome("minecraft:the_end"),
            highlands: resolve_biome("minecraft:end_highlands"),
            midlands: resolve_biome("minecraft:end_midlands"),
            islands: resolve_biome("minecraft:small_end_islands"),
            barrens: resolve_biome("minecraft:end_barrens"),
        };
        let end = load_end_generator(&worldgen_dir, seed, palette, end_biomes).unwrap_or_else(
            |err| {
                tracing::warn!(%err, "failed to load vanilla end JSON; falling back to built-in end");
                NoiseChunkGenerator::end(seed, palette, end_biomes)
            },
        );

        let biome_tags = load_biome_tags(
            &vanilla_data.join("data/minecraft/tags/worldgen/biome"),
            resolve_biome,
        );

        Self {
            overworld,
            nether,
            end,
            registry,
            biome_tags,
        }
    }

    fn generator_for(&self, dimension: Dimension) -> &NoiseChunkGenerator {
        match dimension {
            Dimension::Overworld => &self.overworld,
            Dimension::Nether => &self.nether,
            Dimension::End => &self.end,
        }
    }

    fn resolve_biome_targets(&self, target: &str) -> Vec<u32> {
        if let Some(tag) = target.strip_prefix('#') {
            let key = if tag.contains(':') {
                tag.to_owned()
            } else {
                format!("minecraft:{tag}")
            };
            return self.biome_tags.get(&key).cloned().unwrap_or_default();
        }
        let full = if target.contains(':') {
            target.to_owned()
        } else {
            format!("minecraft:{target}")
        };
        let short = full.strip_prefix("minecraft:").unwrap_or(&full);
        if let Some(id) = self.registry.biome_id(&full) {
            vec![id]
        } else if let Some(id) = ferrite_data::registries::id("worldgen/biome", short) {
            vec![id as u32]
        } else {
            Vec::new()
        }
    }
}

fn load_biome_tags<F: Fn(&str) -> u32>(
    tag_dir: &Path,
    resolve_biome: F,
) -> HashMap<String, Vec<u32>> {
    let mut raw: HashMap<String, Vec<String>> = HashMap::new();
    fn walk(root: &Path, dir: &Path, out: &mut HashMap<String, Vec<String>>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("json")
                && let Ok(rel) = path.strip_prefix(root)
                && let Ok(text) = std::fs::read_to_string(&path)
                && let Ok(val) = serde_json::from_str::<serde_json::Value>(&text)
                && let Some(values) = val["values"].as_array()
            {
                let key = format!(
                    "minecraft:{}",
                    rel.with_extension("").to_string_lossy().replace('\\', "/")
                );
                let items = values
                    .iter()
                    .filter_map(|v| v.as_str().or_else(|| v["id"].as_str()).map(str::to_owned))
                    .collect();
                out.insert(key, items);
            }
        }
    }
    walk(tag_dir, tag_dir, &mut raw);

    fn expand(
        key: &str,
        raw: &HashMap<String, Vec<String>>,
        resolve: &impl Fn(&str) -> u32,
        seen: &mut HashSet<String>,
        out: &mut Vec<u32>,
    ) {
        if !seen.insert(key.to_owned()) {
            return;
        }
        let Some(items) = raw.get(key) else {
            return;
        };
        for item in items {
            if let Some(sub) = item.strip_prefix('#') {
                let sub_key = if sub.contains(':') {
                    sub.to_owned()
                } else {
                    format!("minecraft:{sub}")
                };
                expand(&sub_key, raw, resolve, seen, out);
            } else {
                out.push(resolve(item));
            }
        }
    }

    let mut resolved = HashMap::new();
    for key in raw.keys() {
        let mut seen = HashSet::new();
        let mut ids = Vec::new();
        expand(key, &raw, &resolve_biome, &mut seen, &mut ids);
        ids.sort_unstable();
        ids.dedup();
        resolved.insert(key.clone(), ids);
    }
    resolved
}

fn empty_light_for(chunk: &Chunk) -> LightData {
    let count = chunk.sections().len() + 2;
    LightData {
        sky: vec![LightSection::Empty; count],
        block: vec![LightSection::Empty; count],
    }
}

fn tree_blocks_for_feature(feature: &str) -> (&'static str, &'static str, bool) {
    let short = feature.strip_prefix("minecraft:").unwrap_or(feature);
    match short {
        "spruce" | "pine" => ("spruce_log", "spruce_leaves", false),
        "mega_spruce" | "mega_pine" => ("spruce_log", "spruce_leaves", true),
        "birch" | "super_birch" | "birch_bees_005" => ("birch_log", "birch_leaves", false),
        "jungle" | "jungle_tree" | "jungle_tree_no_vine" => ("jungle_log", "jungle_leaves", false),
        "mega_jungle_tree" => ("jungle_log", "jungle_leaves", true),
        "acacia" => ("acacia_log", "acacia_leaves", false),
        "dark_oak" => ("dark_oak_log", "dark_oak_leaves", true),
        "cherry" | "cherry_bees_005" => ("cherry_log", "cherry_leaves", false),
        "mangrove" | "tall_mangrove" => ("mangrove_log", "mangrove_leaves", false),
        "pale_oak" => ("pale_oak_log", "pale_oak_leaves", true),
        "azalea_tree" => ("oak_log", "azalea_leaves", false),
        _ => ("oak_log", "oak_leaves", false),
    }
}

fn context_blocked(ctx: &TreeContext, pos: [i32; 3], air: u32) -> bool {
    let lx = pos[0] - ctx.min[0];
    let ly = pos[1] - ctx.min[1];
    let lz = pos[2] - ctx.min[2];
    if lx < 0
        || ly < 0
        || lz < 0
        || lx as usize >= ctx.size[0]
        || ly as usize >= ctx.size[1]
        || lz as usize >= ctx.size[2]
    {
        return false;
    }
    let idx = (ly as usize * ctx.size[2] + lz as usize) * ctx.size[0] + lx as usize;
    ctx.blocks.get(idx).is_some_and(|&b| b != air)
}

impl ChunkGenerator for RustGenerator {
    fn generate(&self, dimension: Dimension, x: i32, z: i32) -> anyhow::Result<Generated> {
        let chunk = self
            .generator_for(dimension)
            .generate_chunk(x, z, self.registry.as_ref());
        let light = empty_light_for(&chunk);
        Ok(Generated { chunk, light })
    }

    fn spawn_height(&self, dimension: Dimension, x: i32, z: i32) -> anyhow::Result<i32> {
        let cx = x.div_euclid(16);
        let cz = z.div_euclid(16);
        let lx = x.rem_euclid(16) as u8;
        let lz = z.rem_euclid(16) as u8;
        let chunk = self
            .generator_for(dimension)
            .generate_chunk(cx, cz, self.registry.as_ref());
        let h = chunk
            .height(HeightmapKind::MotionBlocking, lx, lz)
            .unwrap_or(64);
        Ok(chunk.min_y() + h as i32)
    }

    fn locate_biome(
        &self,
        dimension: Dimension,
        target: &str,
        x: i32,
        z: i32,
        radius: i32,
    ) -> anyhow::Result<Option<(i32, i32, i32)>> {
        let ids = self.resolve_biome_targets(target);
        let sample_y = match dimension {
            Dimension::Overworld => 64,
            Dimension::Nether | Dimension::End => 64,
        };
        Ok(self
            .generator_for(dimension)
            .locate_biome(&ids, x, sample_y, z, radius))
    }

    fn biome_at(&self, dimension: Dimension, x: i32, y: i32, z: i32) -> anyhow::Result<String> {
        let id = self.generator_for(dimension).biome_id_at(x, y, z);
        if let Some(name) = self.registry.biome_name(id) {
            return Ok(name.to_owned());
        }
        Ok(ferrite_data::registries::data_entries("worldgen/biome")
            .and_then(|all| all.get(id as usize).copied())
            .map(|n| format!("minecraft:{n}"))
            .unwrap_or_else(|| "minecraft:plains".to_owned()))
    }

    fn place_tree(
        &self,
        _dimension: Dimension,
        feature: &str,
        pos: [i32; 3],
        context: Option<&TreeContext>,
    ) -> anyhow::Result<Placed> {
        let (log_name, leaves_name, two_by_two) = tree_blocks_for_feature(feature);
        let air = u32::from(ferrite_data::blocks::default_state("air").unwrap_or(0));
        let log = ferrite_data::blocks::default_state(log_name)
            .ok_or_else(|| anyhow::anyhow!("unknown log block {log_name}"))?;
        let leaves = ferrite_data::blocks::default_state(leaves_name)
            .ok_or_else(|| anyhow::anyhow!("unknown leaves block {leaves_name}"))?;
        let dirt = ferrite_data::blocks::default_state("dirt").unwrap_or(0);

        let [x, y, z] = pos;
        let trunk_height = if two_by_two { 6 } else { 5 };
        if let Some(ctx) = context {
            for dy in 1..trunk_height {
                if context_blocked(ctx, [x, y + dy, z], air) {
                    anyhow::bail!("insufficient vertical clearance for {feature}");
                }
            }
        }

        let mut blocks = vec![([x, y - 1, z], dirt)];
        if two_by_two {
            for dy in 0..trunk_height {
                for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    blocks.push(([x + dx, y + dy, z + dz], log));
                }
            }
            for dy in (trunk_height - 2)..=trunk_height {
                for dx in -1..=2 {
                    for dz in -1..=2 {
                        if dy < trunk_height && (dx == 0 || dx == 1) && (dz == 0 || dz == 1) {
                            continue;
                        }
                        blocks.push(([x + dx, y + dy, z + dz], leaves));
                    }
                }
            }
        } else {
            for dy in 0..trunk_height {
                blocks.push(([x, y + dy, z], log));
            }
            for dx in -1..=1 {
                for dz in -1..=1 {
                    if (dx, dz) != (0, 0) {
                        blocks.push(([x + dx, y + trunk_height - 2, z + dz], leaves));
                        blocks.push(([x + dx, y + trunk_height - 1, z + dz], leaves));
                    }
                    blocks.push(([x + dx, y + trunk_height, z + dz], leaves));
                }
            }
        }
        Ok(Placed {
            blocks,
            entities: Vec::new(),
        })
    }
}
