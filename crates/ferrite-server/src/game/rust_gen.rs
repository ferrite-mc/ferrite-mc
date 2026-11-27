//! Pure-Rust, lock-free, parallelizable [`ChunkGenerator`] backed by `ferrite-worldgen`.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use ferrite_nbt::{Compound, Tag};
use ferrite_world::{
    BlockEntity, BlockInfo, BlockStateRegistry, Chunk, HeightmapKind, LightData, LightSection,
    RuntimeRegistry,
};
use ferrite_worldgen::biome::{EndBiomeSource, MultiNoiseBiomeSource, Parameter, ParameterPoint};
use ferrite_worldgen::chunk_gen::{NoiseChunkGenerator, SurfaceBiomeIds, TerrainPalette};
use ferrite_worldgen::vanilla_loader::{
    load_end_generator, load_multi_noise_biomes, load_nether_generator, load_overworld_generator,
};

use super::Dimension;
use super::generator::{ChunkGenerator, Generated, Placed, StructureStart, TreeContext};

#[path = "rust_decorate.rs"]
mod decorate;
#[path = "rust_jigsaw.rs"]
mod jigsaw;

struct StructureSpec {
    id: &'static str,
    dimension: Dimension,
    spacing: i32,
    separation: i32,
    salt: u64,
    half_xz: i32,
    y_min: i32,
    y_max: i32,
    tags: &'static [&'static str],
}

const STRUCTURE_SPECS: &[StructureSpec] = &[
    StructureSpec {
        id: "minecraft:village_plains",
        dimension: Dimension::Overworld,
        spacing: 34,
        separation: 8,
        salt: 10387312,
        half_xz: 24,
        y_min: 60,
        y_max: 96,
        tags: &["minecraft:village", "minecraft:eye_of_ender_located"],
    },
    StructureSpec {
        id: "minecraft:pillager_outpost",
        dimension: Dimension::Overworld,
        spacing: 32,
        separation: 8,
        salt: 165745296,
        half_xz: 12,
        y_min: 64,
        y_max: 96,
        tags: &[],
    },
    StructureSpec {
        id: "minecraft:desert_pyramid",
        dimension: Dimension::Overworld,
        spacing: 32,
        separation: 8,
        salt: 14357617,
        half_xz: 11,
        y_min: 62,
        y_max: 84,
        tags: &[],
    },
    StructureSpec {
        id: "minecraft:jungle_pyramid",
        dimension: Dimension::Overworld,
        spacing: 32,
        separation: 8,
        salt: 14357619,
        half_xz: 8,
        y_min: 64,
        y_max: 84,
        tags: &[],
    },
    StructureSpec {
        id: "minecraft:swamp_hut",
        dimension: Dimension::Overworld,
        spacing: 32,
        separation: 8,
        salt: 14357620,
        half_xz: 5,
        y_min: 62,
        y_max: 74,
        tags: &[],
    },
    StructureSpec {
        id: "minecraft:igloo",
        dimension: Dimension::Overworld,
        spacing: 32,
        separation: 8,
        salt: 14357618,
        half_xz: 5,
        y_min: 62,
        y_max: 80,
        tags: &[],
    },
    StructureSpec {
        id: "minecraft:monument",
        dimension: Dimension::Overworld,
        spacing: 32,
        separation: 5,
        salt: 10387313,
        half_xz: 29,
        y_min: 38,
        y_max: 62,
        tags: &["minecraft:on_ocean_explorer_maps"],
    },
    StructureSpec {
        id: "minecraft:mansion",
        dimension: Dimension::Overworld,
        spacing: 80,
        separation: 20,
        salt: 10387319,
        half_xz: 32,
        y_min: 64,
        y_max: 96,
        tags: &["minecraft:on_woodland_explorer_maps"],
    },
    StructureSpec {
        id: "minecraft:stronghold",
        dimension: Dimension::Overworld,
        spacing: 64,
        separation: 16,
        salt: 91731317,
        half_xz: 24,
        y_min: -16,
        y_max: 24,
        tags: &["minecraft:eye_of_ender_located"],
    },
    StructureSpec {
        id: "minecraft:ancient_city",
        dimension: Dimension::Overworld,
        spacing: 24,
        separation: 8,
        salt: 20083232,
        half_xz: 36,
        y_min: -51,
        y_max: -16,
        tags: &[],
    },
    StructureSpec {
        id: "minecraft:trial_chambers",
        dimension: Dimension::Overworld,
        spacing: 34,
        separation: 12,
        salt: 94251327,
        half_xz: 24,
        y_min: -36,
        y_max: -4,
        tags: &["minecraft:on_trial_chambers_maps"],
    },
    StructureSpec {
        id: "minecraft:ruined_portal",
        dimension: Dimension::Overworld,
        spacing: 40,
        separation: 15,
        salt: 34222645,
        half_xz: 6,
        y_min: 60,
        y_max: 80,
        tags: &["minecraft:ruined_portal"],
    },
    StructureSpec {
        id: "minecraft:fortress",
        dimension: Dimension::Nether,
        spacing: 27,
        separation: 4,
        salt: 30084232,
        half_xz: 32,
        y_min: 48,
        y_max: 84,
        tags: &[],
    },
    StructureSpec {
        id: "minecraft:bastion_remnant",
        dimension: Dimension::Nether,
        spacing: 27,
        separation: 4,
        salt: 30084232,
        half_xz: 28,
        y_min: 36,
        y_max: 80,
        tags: &[],
    },
    StructureSpec {
        id: "minecraft:ruined_portal_nether",
        dimension: Dimension::Nether,
        spacing: 25,
        separation: 10,
        salt: 34222645,
        half_xz: 6,
        y_min: 32,
        y_max: 64,
        tags: &["minecraft:ruined_portal"],
    },
    StructureSpec {
        id: "minecraft:end_city",
        dimension: Dimension::End,
        spacing: 20,
        separation: 11,
        salt: 10387313,
        half_xz: 24,
        y_min: 56,
        y_max: 112,
        tags: &[],
    },
];

fn structure_chunk_in_region(
    seed: u64,
    salt: u64,
    spacing: i32,
    separation: i32,
    reg_x: i32,
    reg_z: i32,
) -> (i32, i32) {
    let range = (spacing - separation).max(1) as u64;
    let mut s = seed
        .wrapping_add(salt)
        .wrapping_add((reg_x as i64 as u64).wrapping_mul(341873128712))
        .wrapping_add((reg_z as i64 as u64).wrapping_mul(132897987541));
    s = (s ^ (s >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    let ox = (s % range) as i32;
    s = (s ^ (s >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    let oz = (s % range) as i32;
    (reg_x * spacing + ox, reg_z * spacing + oz)
}

fn structure_matches(spec: &StructureSpec, target: &str) -> bool {
    if let Some(tag) = target.strip_prefix('#') {
        let full = if tag.contains(':') {
            tag.to_owned()
        } else {
            format!("minecraft:{tag}")
        };
        spec.tags.contains(&full.as_str())
    } else {
        let full = if target.contains(':') {
            target.to_owned()
        } else {
            format!("minecraft:{target}")
        };
        spec.id == full
    }
}

fn structure_bounds(spec: &StructureSpec, chunk_x: i32, chunk_z: i32) -> [i32; 6] {
    let cx = chunk_x * 16 + 8;
    let cz = chunk_z * 16 + 8;
    [
        cx - spec.half_xz,
        spec.y_min,
        cz - spec.half_xz,
        cx + spec.half_xz,
        spec.y_max,
        cz + spec.half_xz,
    ]
}

/// Pure-Rust world generator safe for concurrent multi-region chunk generation.
pub struct RustGenerator {
    seed: u64,
    overworld: NoiseChunkGenerator,
    nether: NoiseChunkGenerator,
    end: NoiseChunkGenerator,
    registry: Arc<RuntimeRegistry>,
    biome_tags: HashMap<String, Vec<u32>>,
    deco: decorate::DecorationPalette,
    jigsaw: jigsaw::JigsawLibrary,
}

impl RustGenerator {
    /// Builds a `RustGenerator` for `seed` using vanilla data under `vanilla_data` when present.
    pub fn new(seed: u64, registry: Arc<RuntimeRegistry>) -> Self {
        let default_data =
            ferrite_protocol::vanilla_generated_dir(Some(Path::new(env!("CARGO_MANIFEST_DIR"))));
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
            red_sand: state("minecraft:red_sand"),
            terracotta: state("minecraft:terracotta"),
            snow_block: state("minecraft:snow_block"),
            gravel: state("minecraft:gravel"),
            podzol: state("minecraft:podzol"),
            mycelium: state("minecraft:mycelium"),
            soul_sand: state("minecraft:soul_sand"),
            crimson_nylium: state("minecraft:crimson_nylium"),
            warped_nylium: state("minecraft:warped_nylium"),
            blackstone: state("minecraft:blackstone"),
            basalt: state("minecraft:basalt"),
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

        let ids_for =
            |names: &[&str]| -> Vec<u32> { names.iter().map(|n| resolve_biome(n)).collect() };
        let surface_biomes = SurfaceBiomeIds {
            sandy: ids_for(&["desert", "beach", "warm_ocean"]),
            badlands: ids_for(&["badlands", "eroded_badlands", "wooded_badlands"]),
            snowy: ids_for(&[
                "snowy_plains",
                "ice_spikes",
                "snowy_taiga",
                "snowy_beach",
                "snowy_slopes",
                "frozen_peaks",
                "jagged_peaks",
                "grove",
            ]),
            podzol: ids_for(&["old_growth_pine_taiga", "old_growth_spruce_taiga"]),
            mycelium: ids_for(&["mushroom_fields"]),
            crimson: ids_for(&["crimson_forest"]),
            warped: ids_for(&["warped_forest"]),
            soul_sand: ids_for(&["soul_sand_valley"]),
            basalt: ids_for(&["basalt_deltas"]),
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
        let mut overworld = load_overworld_generator(
            &worldgen_dir,
            seed,
            palette,
            overworld_biomes.clone(),
        )
        .unwrap_or_else(|err| {
            tracing::warn!(%err, "failed to load vanilla overworld JSON; falling back to built-in overworld");
            NoiseChunkGenerator::overworld(seed, palette, overworld_biomes)
        });
        overworld.settings.surface_biomes = surface_biomes.clone();

        let nether_biomes = load_multi_noise_biomes(
            &vanilla_data.join("reports/biome_parameters/minecraft/nether.json"),
            resolve_biome,
        )
        .unwrap_or_else(|_| fallback_multi("nether_wastes"));
        let mut nether = load_nether_generator(&worldgen_dir, seed, palette, nether_biomes.clone())
            .unwrap_or_else(|err| {
                tracing::warn!(%err, "failed to load vanilla nether JSON; falling back to built-in nether");
                NoiseChunkGenerator::nether(seed, palette, nether_biomes)
            });
        nether.settings.surface_biomes = surface_biomes;

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

        let deco = decorate::DecorationPalette::from_registry(&registry);
        let jigsaw = jigsaw::JigsawLibrary::new(vanilla_data, Arc::clone(&registry));

        Self {
            seed,
            overworld,
            nether,
            end,
            registry,
            biome_tags,
            deco,
            jigsaw,
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

fn initial_light_from_u16(
    dimension: Dimension,
    chunk: &Chunk,
    raw_blocks: &[u16],
    registry: &RuntimeRegistry,
) -> LightData {
    let count = chunk.sections().len() + 2;
    if dimension != Dimension::Overworld {
        return LightData {
            sky: vec![LightSection::Empty; count],
            block: vec![LightSection::Empty; count],
        };
    }

    let min_y = chunk.min_y();
    let num_sec = chunk.sections().len();
    let max_y_idx = raw_blocks.len() / 256;
    let world_surface = chunk.heights(HeightmapKind::WorldSurface);
    let mut top_solid = [min_y - 1; 256];
    let mut top_open = [min_y - 1; 256];
    let mut last_state = 0u16;
    let mut last_opacity = 0u8;
    for col in 0..256usize {
        let h = usize::from(world_surface[col]);
        if h == 0 {
            continue;
        }
        let start_idx = (h - 1).min(max_y_idx.saturating_sub(1));
        let mut found_open = false;
        for y_idx in (0..=start_idx).rev() {
            let state = raw_blocks[y_idx * 256 + col];
            if state != 0 {
                let opacity = if state == last_state {
                    last_opacity
                } else {
                    let op = ferrite_light::vanilla_properties(state).opacity;
                    last_state = state;
                    last_opacity = op;
                    op
                };
                let wy = min_y + y_idx as i32;
                if !found_open && (opacity > 0 || !registry.flags(u32::from(state)).air) {
                    top_open[col] = wy;
                    found_open = true;
                }
                if opacity >= 15 {
                    top_solid[col] = wy;
                    break;
                }
            }
        }
    }

    // Find the section range that actually needs per-block light (between min(top_solid) - 16
    // and max(top_open)). Sections strictly above max(top_open) are full sky (15); sections
    // strictly below min(top_solid) - 16 are completely underground (0).
    let min_mixed_y = top_solid
        .iter()
        .copied()
        .min()
        .unwrap_or(min_y)
        .saturating_sub(16)
        .max(min_y);
    let max_mixed_y = top_open.iter().copied().max().unwrap_or(min_y - 1);
    let min_mixed_sec = ((min_mixed_y - min_y).div_euclid(16) as usize).min(num_sec);
    let max_mixed_sec = if max_mixed_y < min_y {
        0
    } else {
        (((max_mixed_y - min_y).div_euclid(16) as usize) + 1).min(num_sec)
    };

    let mixed_secs = max_mixed_sec.saturating_sub(min_mixed_sec);
    let mut sky = Vec::with_capacity(count);
    // Light section 0 is one section below min_y.
    sky.push(LightSection::Empty);

    if mixed_secs == 0 {
        for sy in 0..num_sec {
            if sy < min_mixed_sec {
                sky.push(LightSection::Empty);
            } else {
                sky.push(LightSection::full_sky());
            }
        }
        sky.push(LightSection::full_sky());
        return LightData {
            sky,
            block: vec![LightSection::Empty; count],
        };
    }

    thread_local! {
        static LIGHT_SCRATCH: std::cell::RefCell<(Vec<u8>, Vec<u8>)> =
            const { std::cell::RefCell::new((Vec::new(), Vec::new())) };
    }
    LIGHT_SCRATCH.with(|cell| {
        let mut guard = cell.borrow_mut();
        let (grid, opac) = &mut *guard;
        let total_cells = mixed_secs * 4096;
        grid.clear();
        grid.resize(total_cells, 0);
        opac.clear();
        opac.resize(total_cells, 0);

        let grid_h = mixed_secs * 16;
        let raw_secs = raw_blocks.len() / 4096;
        let mut mru_state = 0u16;
        let mut mru_op = 0u8;
        for local_sec in 0..mixed_secs {
            let sy = min_mixed_sec + local_sec;
            if sy >= raw_secs {
                continue;
            }
            let src = &raw_blocks[sy * 4096..(sy + 1) * 4096];
            let dst_slice = &mut opac[local_sec * 4096..(local_sec + 1) * 4096];
            for (dst, &state) in dst_slice.iter_mut().zip(src.iter()) {
                if state != 0 {
                    let op = if state == mru_state {
                        mru_op
                    } else {
                        let o = ferrite_light::vanilla_properties(state).opacity;
                        mru_state = state;
                        mru_op = o;
                        o
                    };
                    *dst = op;
                }
            }
        }

        // Top-down vertical sky-light pass in contiguous 256-column slices.
        let mut col_light = [15u8; 256];
        let mut needs_spread = false;
        let mut min_air_y = grid_h;
        for y_off in (0..grid_h).rev() {
            let row = y_off * 256;
            let op_row = &opac[row..row + 256];
            let grid_row = &mut grid[row..row + 256];
            let mut row_has_air = false;
            for col in 0..256 {
                let op = op_row[col];
                if op >= 15 {
                    col_light[col] = 0;
                } else {
                    row_has_air = true;
                    if op > 0 || col_light[col] == 0 {
                        needs_spread = true;
                    }
                    col_light[col] = col_light[col].saturating_sub(op);
                    grid_row[col] = col_light[col];
                }
            }
            if row_has_air {
                min_air_y = y_off;
            }
        }

        // Propagate sky light horizontally/vertically across transparent blocks (e.g. under
        // tree canopies and shallow cliff overhangs) so sunlit ground doesn't stay at light 0.
        if needs_spread {
            for _ in 0..8 {
                let mut changed = false;
                for y_off in min_air_y..grid_h {
                    let row = y_off * 256;
                    for lz in 0..16usize {
                        let z_row = row + lz * 16;
                        for lx in 0..16usize {
                            let idx = z_row + lx;
                            let op = opac[idx];
                            if op >= 15 {
                                continue;
                            }
                            let mut best = grid[idx];
                            if best == 15 {
                                continue;
                            }
                            let step = op.max(1);
                            if lx > 0 {
                                best = best.max(grid[idx - 1].saturating_sub(step));
                            }
                            if lx < 15 {
                                best = best.max(grid[idx + 1].saturating_sub(step));
                            }
                            if lz > 0 {
                                best = best.max(grid[idx - 16].saturating_sub(step));
                            }
                            if lz < 15 {
                                best = best.max(grid[idx + 16].saturating_sub(step));
                            }
                            if y_off > 0 {
                                best = best.max(grid[idx - 256].saturating_sub(step));
                            }
                            if y_off + 1 < grid_h {
                                best = best.max(grid[idx + 256].saturating_sub(op));
                            } else {
                                best = best.max(15u8.saturating_sub(op));
                            }
                            if best > grid[idx] {
                                grid[idx] = best;
                                changed = true;
                            }
                        }
                    }
                }
                if !changed {
                    break;
                }
            }
        }

        for sy in 0..num_sec {
            if sy < min_mixed_sec {
                sky.push(LightSection::Empty);
                continue;
            }
            if sy >= max_mixed_sec {
                sky.push(LightSection::full_sky());
                continue;
            }
            let local_sec = sy - min_mixed_sec;
            let base = local_sec * 4096;
            let slice = &grid[base..base + 4096];
            if slice.iter().all(|&v| v == 0) {
                sky.push(LightSection::Empty);
            } else if slice.iter().all(|&v| v == 15) {
                sky.push(LightSection::full_sky());
            } else {
                let mut nibbles = [0u8; 2048];
                for (dst, pair) in nibbles.iter_mut().zip(slice.as_chunks::<2>().0) {
                    *dst = (pair[0] & 0x0f) | ((pair[1] & 0x0f) << 4);
                }
                sky.push(LightSection::Data(Arc::new(nibbles)));
            }
        }
    });
    // Light section count - 1 is one section above max_y (open sky).
    sky.push(LightSection::full_sky());
    LightData {
        sky,
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
        thread_local! {
            static RAW_BLOCKS: std::cell::RefCell<Vec<u16>> = const { std::cell::RefCell::new(Vec::new()) };
        }
        let mut raw_blocks = RAW_BLOCKS.with(|cell| std::mem::take(&mut *cell.borrow_mut()));
        let (mut chunk, active_sections) =
            self.generator_for(dimension)
                .generate_chunk_into_u16(x, z, &mut raw_blocks);

        let min_y = chunk.min_y();
        let height_blocks = chunk.height_blocks();
        let sections = chunk.sections().len();
        let air_u16 = self.deco.air as u16;
        let mut surface_h = [0u16; 256];
        let mut rem_cols = 256usize;
        for y_idx in (0..active_sections * 16).rev() {
            let row = &raw_blocks[y_idx * 256..(y_idx + 1) * 256];
            let h = (y_idx + 1) as u16;
            for col in 0..256 {
                if surface_h[col] == 0 && row[col] != air_u16 {
                    surface_h[col] = h;
                    rem_cols -= 1;
                }
            }
            if rem_cols == 0 {
                break;
            }
        }

        let starts = self.structure_starts(dimension, x, z).unwrap_or_default();
        let assembled_structure: Option<Vec<([i32; 3], u32)>> =
            starts.first().and_then(|(structure_id, bounds)| {
                let max_y = min_y + height_blocks as i32 - 1;
                let surf = Some(surface_h[8 * 16 + 8])
                    .filter(|&h| h > 0)
                    .map(|h| min_y + i32::from(h) - 1)
                    .unwrap_or_else(|| (bounds[1] + bounds[4]) / 2);
                let anchor_y = if dimension == Dimension::Nether {
                    48
                } else {
                    (surf + 1).clamp(min_y + 2, max_y - 24)
                };
                let s = self.seed ^ ((x as i64 as u64) << 32) ^ (z as i64 as u64 & 0xFFFF_FFFF);
                self.jigsaw
                    .assemble_structure(structure_id, s, [x * 16 + 8, anchor_y, z * 16 + 8], 10)
                    .map(|blocks| {
                        blocks
                            .into_iter()
                            .map(|(p, st)| (p, u32::from(st)))
                            .collect()
                    })
            });
        let surf_sy = ((64 - min_y).div_euclid(16) as usize).min(sections.saturating_sub(1));
        let center_biome_id = chunk
            .sections()
            .get(surf_sy)
            .and_then(|sec| sec.biomes().get(2 * 4 + 2))
            .unwrap_or(0);
        let center_biome = self
            .registry
            .biome_name(center_biome_id)
            .unwrap_or("minecraft:plains");
        let (max_deco_y, deco_block_entities) = decorate::decorate_chunk_u16(
            decorate::DecorateSpec {
                seed: self.seed,
                dimension,
                chunk_x: x,
                chunk_z: z,
                min_y,
                height_blocks,
                pal: &self.deco,
                surface_h: &surface_h,
                structure_starts: &starts,
                assembled_structure: assembled_structure.as_deref(),
                center_biome,
            },
            &mut raw_blocks,
        );

        let deco_sections = if max_deco_y >= min_y {
            (((max_deco_y - min_y) as usize) / 16 + 1).min(sections)
        } else {
            0
        };
        let final_sections = active_sections.max(deco_sections);
        for sy in 0..final_sections {
            let _ = chunk.sections_mut()[sy].set_blocks_u16(
                &raw_blocks[sy * 4096..(sy + 1) * 4096],
                self.registry.as_ref(),
            );
        }
        for (pos, entity) in deco_block_entities {
            match entity {
                decorate::DecoratedBlockEntity::LootChest { loot_table, seed } => {
                    if let Some(type_id) = self.registry.entity_id("minecraft:chest") {
                        let mut data = Compound::new();
                        data.insert("LootTable".into(), Tag::String(loot_table.into()));
                        data.insert("LootTableSeed".into(), Tag::Long(seed));
                        chunk
                            .block_entities
                            .insert(pos, BlockEntity { type_id, data });
                    }
                }
                decorate::DecoratedBlockEntity::MobSpawner { entity_id } => {
                    if let Some(type_id) = self.registry.entity_id("minecraft:mob_spawner") {
                        let mut entity_nbt = Compound::new();
                        entity_nbt.insert("id".into(), Tag::String(entity_id.into()));
                        let mut spawn_data = Compound::new();
                        spawn_data.insert("entity".into(), Tag::Compound(entity_nbt));
                        let mut data = Compound::new();
                        data.insert("Delay".into(), Tag::Short(20));
                        data.insert("MinSpawnDelay".into(), Tag::Short(200));
                        data.insert("MaxSpawnDelay".into(), Tag::Short(800));
                        data.insert("SpawnCount".into(), Tag::Short(4));
                        data.insert("MaxNearbyEntities".into(), Tag::Short(6));
                        data.insert("RequiredPlayerRange".into(), Tag::Short(16));
                        data.insert("SpawnRange".into(), Tag::Short(4));
                        data.insert("SpawnData".into(), Tag::Compound(spawn_data));
                        chunk
                            .block_entities
                            .insert(pos, BlockEntity { type_id, data });
                    }
                }
            }
        }
        chunk.compute_heightmaps_from_u16(
            &raw_blocks[..final_sections * 4096],
            self.registry.as_ref(),
        );
        let light = initial_light_from_u16(
            dimension,
            &chunk,
            &raw_blocks[..final_sections * 4096],
            self.registry.as_ref(),
        );
        RAW_BLOCKS.with(|cell| {
            *cell.borrow_mut() = raw_blocks;
        });

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
        Ok(self
            .generator_for(dimension)
            .locate_biome(&ids, x, 64, z, radius))
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

    fn locate_structure(
        &self,
        dimension: Dimension,
        target: &str,
        x: i32,
        z: i32,
        radius: i32,
    ) -> anyhow::Result<Option<(i32, i32, i32)>> {
        let origin_cx = x.div_euclid(16);
        let origin_cz = z.div_euclid(16);
        let max_chunks = (radius.max(0) + 15) / 16;
        let mut best: Option<(i64, [i32; 3])> = None;

        for spec in STRUCTURE_SPECS
            .iter()
            .filter(|s| s.dimension == dimension && structure_matches(s, target))
        {
            let rx0 = origin_cx.div_euclid(spec.spacing);
            let rz0 = origin_cz.div_euclid(spec.spacing);
            let ring_limit = max_chunks / spec.spacing + 1;
            for ring in 0..=ring_limit {
                for rdz in -ring..=ring {
                    for rdx in -ring..=ring {
                        if ring > 0 && rdx.abs() != ring && rdz.abs() != ring {
                            continue;
                        }
                        let (scx, scz) = structure_chunk_in_region(
                            self.seed,
                            spec.salt,
                            spec.spacing,
                            spec.separation,
                            rx0 + rdx,
                            rz0 + rdz,
                        );
                        let bx = scx * 16 + 8;
                        let bz = scz * 16 + 8;
                        let dx = i64::from(bx - x);
                        let dz = i64::from(bz - z);
                        if dx.abs() > i64::from(radius) || dz.abs() > i64::from(radius) {
                            continue;
                        }
                        let d2 = dx * dx + dz * dz;
                        if best.is_none_or(|(b, _)| d2 < b) {
                            let by = (spec.y_min + spec.y_max) / 2;
                            best = Some((d2, [bx, by, bz]));
                        }
                    }
                }
                if best.is_some() {
                    break;
                }
            }
        }
        Ok(best.map(|(_, pos)| (pos[0], pos[1], pos[2])))
    }

    fn locate_poi(
        &self,
        dimension: Dimension,
        target: &str,
        x: i32,
        _y: i32,
        z: i32,
        radius: i32,
    ) -> anyhow::Result<Option<(i32, i32, i32)>> {
        let short = target
            .trim_start_matches('#')
            .strip_prefix("minecraft:")
            .unwrap_or(target);
        let structure_key = if short.contains("village")
            || short.contains("home")
            || short.contains("meeting")
            || short.contains("bell")
            || short.contains("job")
        {
            Some("#minecraft:village")
        } else if short.contains("portal") {
            Some("#minecraft:ruined_portal")
        } else {
            None
        };
        if let Some(key) = structure_key {
            return self.locate_structure(dimension, key, x, z, radius);
        }
        Ok(None)
    }

    fn structure_starts(
        &self,
        dimension: Dimension,
        chunk_x: i32,
        chunk_z: i32,
    ) -> anyhow::Result<Vec<StructureStart>> {
        let mut out = Vec::new();
        for spec in STRUCTURE_SPECS.iter().filter(|s| s.dimension == dimension) {
            let rx = chunk_x.div_euclid(spec.spacing);
            let rz = chunk_z.div_euclid(spec.spacing);
            let (scx, scz) = structure_chunk_in_region(
                self.seed,
                spec.salt,
                spec.spacing,
                spec.separation,
                rx,
                rz,
            );
            if (scx, scz) == (chunk_x, chunk_z) {
                out.push((spec.id.to_owned(), structure_bounds(spec, scx, scz)));
            }
        }
        Ok(out)
    }

    fn structure_pieces(
        &self,
        dimension: Dimension,
        id: &str,
        chunk_x: i32,
        chunk_z: i32,
    ) -> anyhow::Result<Vec<[i32; 6]>> {
        let starts = self.structure_starts(dimension, chunk_x, chunk_z)?;
        Ok(starts
            .into_iter()
            .filter(|(name, _)| name == id)
            .map(|(_, bounds)| bounds)
            .collect())
    }

    fn structure_at(
        &self,
        dimension: Dimension,
        id: &str,
        x: i32,
        y: i32,
        z: i32,
    ) -> anyhow::Result<bool> {
        let cx = x.div_euclid(16);
        let cz = z.div_euclid(16);
        for spec in STRUCTURE_SPECS
            .iter()
            .filter(|s| s.dimension == dimension && structure_matches(s, id))
        {
            let rx = cx.div_euclid(spec.spacing);
            let rz = cz.div_euclid(spec.spacing);
            let (scx, scz) = structure_chunk_in_region(
                self.seed,
                spec.salt,
                spec.spacing,
                spec.separation,
                rx,
                rz,
            );
            let [min_x, min_y, min_z, max_x, max_y, max_z] = structure_bounds(spec, scx, scz);
            if (min_x..=max_x).contains(&x)
                && (min_y..=max_y).contains(&y)
                && (min_z..=max_z).contains(&z)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn place(&self, dimension: Dimension, argument: &str) -> anyhow::Result<Placed> {
        let parts: Vec<&str> = argument.split(';').collect();
        anyhow::ensure!(
            parts.len() >= 5,
            "expected place argument '<kind>;<id>;<x>;<y>;<z>', got '{argument}'"
        );
        let kind = parts[0];
        let id = parts[1];
        let n = parts.len();
        let x: i32 = parts[n - 3].parse()?;
        let y: i32 = parts[n - 2].parse()?;
        let z: i32 = parts[n - 1].parse()?;
        match kind {
            "feature" => self.place_tree(dimension, id, [x, y, z], None),
            "structure" => {
                let s = self.seed ^ (x as u64).wrapping_mul(31) ^ (z as u64);
                let blocks = self
                    .jigsaw
                    .assemble_structure(id, s, [x, y, z], 24)
                    .unwrap_or_else(|| {
                        decorate::structure_template_blocks(id, [x, y, z], &self.deco)
                            .into_iter()
                            .map(|(p, st)| (p, st as u16))
                            .collect()
                    });
                Ok(Placed {
                    blocks,
                    entities: Vec::new(),
                })
            }
            "jigsaw" => {
                let (target, max_depth) = if parts.len() >= 7 {
                    (Some(parts[2]), parts[3].parse::<usize>().unwrap_or(3))
                } else {
                    (None, 3)
                };
                let target = target.filter(|t| *t != "minecraft:empty" && *t != "empty");
                let s = self.seed ^ (x as u64).wrapping_mul(31) ^ (z as u64);
                let blocks = self
                    .jigsaw
                    .assemble_jigsaw(id, target, max_depth, s, [x, y, z], 32)
                    .unwrap_or_else(|| {
                        decorate::structure_template_blocks(id, [x, y, z], &self.deco)
                            .into_iter()
                            .map(|(p, st)| (p, st as u16))
                            .collect()
                    });
                Ok(Placed {
                    blocks,
                    entities: Vec::new(),
                })
            }
            other => anyhow::bail!("unsupported place kind '{other}'"),
        }
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
        let leaves = ferrite_data::blocks::state_id(
            leaves_name,
            &[
                ("distance", "1"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
        )
        .or_else(|| ferrite_data::blocks::default_state(leaves_name))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_generator_decorates_ores_caves_and_structures_deterministically() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3/generated");
        if !base.exists() {
            return;
        }
        let registry = Arc::new(RuntimeRegistry::load(&base).unwrap());
        let generator = RustGenerator::with_vanilla_data(424242, Arc::clone(&registry), &base);

        // Multi-block structure templates and vanilla Jigsaw .nbt assembly produce multi-piece 3D geometry.
        let placed = generator
            .place(
                Dimension::Overworld,
                "structure;minecraft:village_plains;0;64;0",
            )
            .unwrap();
        let max_y = placed.blocks.iter().map(|([_, y, _], _)| *y).max().unwrap();
        let min_y = placed.blocks.iter().map(|([_, y, _], _)| *y).min().unwrap();
        assert!(
            max_y - min_y >= 4,
            "expected 3D structure height, got {min_y}..={max_y}"
        );
        assert!(
            placed.blocks.len() > 150,
            "expected multi-piece vanilla NBT village assembly, got {} blocks",
            placed.blocks.len()
        );
        let jigsaw_id = ferrite_data::blocks::default_state("minecraft:jigsaw").unwrap_or(u16::MAX);
        assert!(
            placed.blocks.iter().all(|(_, s)| *s != jigsaw_id),
            "jigsaw connector blocks must be substituted with final_state"
        );
        let jigsaw_pool = generator
            .place(
                Dimension::Overworld,
                "jigsaw;minecraft:village/plains/town_centers;minecraft:empty;3;0;64;0",
            )
            .unwrap();
        assert!(jigsaw_pool.blocks.len() > 150);
        let pyramid = generator
            .place(
                Dimension::Overworld,
                "structure;minecraft:desert_pyramid;0;64;0",
            )
            .unwrap();
        let monument = generator
            .place(Dimension::Overworld, "structure;minecraft:monument;0;48;0")
            .unwrap();
        assert_ne!(
            pyramid
                .blocks
                .iter()
                .map(|(_, s)| *s)
                .collect::<HashSet<_>>(),
            monument
                .blocks
                .iter()
                .map(|(_, s)| *s)
                .collect::<HashSet<_>>()
        );

        let g1 = generator.generate(Dimension::Overworld, 0, 0).unwrap();
        let g2 = generator.generate(Dimension::Overworld, 0, 0).unwrap();
        // Verify deterministic chunk generation across repeated calls.
        for y in -56..80 {
            assert_eq!(g1.chunk.block(4, y, 4), g2.chunk.block(4, y, 4));
        }

        // Verify ores appear across a small 4x4 chunk sample.
        let ore_ids: Vec<u32> = [
            "minecraft:coal_ore",
            "minecraft:iron_ore",
            "minecraft:copper_ore",
            "minecraft:gold_ore",
            "minecraft:redstone_ore",
            "minecraft:diamond_ore",
            "minecraft:deepslate_iron_ore",
            "minecraft:deepslate_diamond_ore",
        ]
        .into_iter()
        .filter_map(|name| ferrite_data::blocks::default_state(name).map(u32::from))
        .collect();

        let mut found_ore = false;
        'scan: for cz in 0..2 {
            for cx in 0..2 {
                let c = generator
                    .generate(Dimension::Overworld, cx, cz)
                    .unwrap()
                    .chunk;
                for y in -58..120 {
                    for z in 0..16u8 {
                        for x in 0..16u8 {
                            if let Some(b) = c.block(x, y, z)
                                && ore_ids.contains(&b)
                            {
                                found_ore = true;
                                break 'scan;
                            }
                        }
                    }
                }
            }
        }
        assert!(
            found_ore,
            "expected deterministic underground ores in decorated chunks"
        );

        // Outer End islands get chorus plants while main End island (0,0) stays unobstructed.
        let main_end = generator.generate(Dimension::End, 0, 0).unwrap().chunk;
        let chorus_id = ferrite_data::blocks::default_state("minecraft:chorus_plant")
            .map(u32::from)
            .unwrap_or(0);
        if chorus_id != 0 {
            for y in 40..100 {
                for z in 0..16u8 {
                    for x in 0..16u8 {
                        assert_ne!(main_end.block(x, y, z), Some(chorus_id));
                    }
                }
            }
        }
    }

    #[test]
    fn worldgen_dungeons_populate_spawner_and_loot_chest_block_entities_and_swamp_hut() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3/generated");
        if !base.exists() {
            return;
        }
        let registry = Arc::new(RuntimeRegistry::load(&base).unwrap());
        let generator = RustGenerator::with_vanilla_data(1337, Arc::clone(&registry), &base);

        // Swamp hut template places spruce planks, oak stilts, cauldron & crafting table.
        let hut = generator
            .place(Dimension::Overworld, "structure;minecraft:swamp_hut;0;66;0")
            .unwrap();
        assert!(hut.blocks.len() > 50);

        // Scan chunks until we hit a deterministic underground dungeon and verify its block entities.
        let spawner_type = registry.entity_id("minecraft:mob_spawner").unwrap();
        let chest_type = registry.entity_id("minecraft:chest").unwrap();
        let mut found_spawner = false;
        let mut found_loot_chest = false;
        'search: for cz in 0..12 {
            for cx in 0..12 {
                let chunk = generator
                    .generate(Dimension::Overworld, cx, cz)
                    .unwrap()
                    .chunk;
                for entity in chunk.block_entities.values() {
                    if entity.type_id == spawner_type {
                        assert!(
                            entity.data.get("SpawnData").is_some(),
                            "dungeon spawner must carry SpawnData NBT"
                        );
                        found_spawner = true;
                    }
                    if entity.type_id == chest_type
                        && matches!(
                            entity.data.get("LootTable"),
                            Some(Tag::String(t)) if t == "minecraft:chests/simple_dungeon"
                        )
                    {
                        found_loot_chest = true;
                    }
                }
                if found_spawner && found_loot_chest {
                    break 'search;
                }
            }
        }
        assert!(found_spawner, "expected a dungeon mob_spawner block entity");
        assert!(
            found_loot_chest,
            "expected a dungeon chest with simple_dungeon LootTable"
        );
    }
}
