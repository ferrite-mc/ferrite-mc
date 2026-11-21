//! Pure-Rust deterministic post-terrain chunk decoration: 3D cave carving,
//! vanilla-band ore veins, biome-aware trees/flora, and structure features.

use std::collections::BTreeMap;

use ferrite_world::{BlockStateRegistry, Chunk, HeightmapKind, RuntimeRegistry};

use super::Dimension;

/// Pre-resolved block state IDs for fast O(1) decoration without hash lookups per block.
#[derive(Clone, Copy)]
pub(super) struct DecorationPalette {
    pub air: u32,
    pub stone: u32,
    pub deepslate: u32,
    pub lava: u32,
    pub dirt: u32,
    pub grass_block: u32,
    pub sand: u32,
    pub red_sand: u32,
    pub snow_block: u32,
    pub podzol: u32,
    pub netherrack: u32,
    pub crimson_nylium: u32,
    pub warped_nylium: u32,
    pub coal_ore: u32,
    pub deepslate_coal_ore: u32,
    pub iron_ore: u32,
    pub deepslate_iron_ore: u32,
    pub copper_ore: u32,
    pub deepslate_copper_ore: u32,
    pub gold_ore: u32,
    pub deepslate_gold_ore: u32,
    pub redstone_ore: u32,
    pub deepslate_redstone_ore: u32,
    pub diamond_ore: u32,
    pub deepslate_diamond_ore: u32,
    pub lapis_ore: u32,
    pub deepslate_lapis_ore: u32,
    pub nether_quartz_ore: u32,
    pub nether_gold_ore: u32,
    pub ancient_debris: u32,
    pub short_grass: u32,
    pub fern: u32,
    pub dandelion: u32,
    pub poppy: u32,
    pub cactus: u32,
    pub dead_bush: u32,
    pub crimson_fungus: u32,
    pub warped_fungus: u32,
    pub oak_log: u32,
    pub oak_leaves: u32,
    pub birch_log: u32,
    pub birch_leaves: u32,
    pub spruce_log: u32,
    pub spruce_leaves: u32,
    pub dark_oak_log: u32,
    pub dark_oak_leaves: u32,
    pub jungle_log: u32,
    pub jungle_leaves: u32,
    pub acacia_log: u32,
    pub acacia_leaves: u32,
    pub cherry_log: u32,
    pub cherry_leaves: u32,
    pub stone_bricks: u32,
    pub cobblestone: u32,
    pub oak_planks: u32,
    pub sandstone: u32,
    pub obsidian: u32,
    pub crying_obsidian: u32,
    pub lantern: u32,
    pub end_stone: u32,
    pub chorus_plant: u32,
    pub chorus_flower: u32,
}

impl DecorationPalette {
    pub(super) fn from_registry(registry: &RuntimeRegistry) -> Self {
        let empty = BTreeMap::new();
        let id = |name: &str| -> u32 {
            ferrite_data::blocks::default_state(name)
                .map(u32::from)
                .or_else(|| registry.block_id(name, &empty))
                .unwrap_or(0)
        };
        Self {
            air: id("minecraft:air"),
            stone: id("minecraft:stone"),
            deepslate: id("minecraft:deepslate"),
            lava: id("minecraft:lava"),
            dirt: id("minecraft:dirt"),
            grass_block: id("minecraft:grass_block"),
            sand: id("minecraft:sand"),
            red_sand: id("minecraft:red_sand"),
            snow_block: id("minecraft:snow_block"),
            podzol: id("minecraft:podzol"),
            netherrack: id("minecraft:netherrack"),
            crimson_nylium: id("minecraft:crimson_nylium"),
            warped_nylium: id("minecraft:warped_nylium"),
            coal_ore: id("minecraft:coal_ore"),
            deepslate_coal_ore: id("minecraft:deepslate_coal_ore"),
            iron_ore: id("minecraft:iron_ore"),
            deepslate_iron_ore: id("minecraft:deepslate_iron_ore"),
            copper_ore: id("minecraft:copper_ore"),
            deepslate_copper_ore: id("minecraft:deepslate_copper_ore"),
            gold_ore: id("minecraft:gold_ore"),
            deepslate_gold_ore: id("minecraft:deepslate_gold_ore"),
            redstone_ore: id("minecraft:redstone_ore"),
            deepslate_redstone_ore: id("minecraft:deepslate_redstone_ore"),
            diamond_ore: id("minecraft:diamond_ore"),
            deepslate_diamond_ore: id("minecraft:deepslate_diamond_ore"),
            lapis_ore: id("minecraft:lapis_ore"),
            deepslate_lapis_ore: id("minecraft:deepslate_lapis_ore"),
            nether_quartz_ore: id("minecraft:nether_quartz_ore"),
            nether_gold_ore: id("minecraft:nether_gold_ore"),
            ancient_debris: id("minecraft:ancient_debris"),
            short_grass: id("minecraft:short_grass"),
            fern: id("minecraft:fern"),
            dandelion: id("minecraft:dandelion"),
            poppy: id("minecraft:poppy"),
            cactus: id("minecraft:cactus"),
            dead_bush: id("minecraft:dead_bush"),
            crimson_fungus: id("minecraft:crimson_fungus"),
            warped_fungus: id("minecraft:warped_fungus"),
            oak_log: id("minecraft:oak_log"),
            oak_leaves: id("minecraft:oak_leaves"),
            birch_log: id("minecraft:birch_log"),
            birch_leaves: id("minecraft:birch_leaves"),
            spruce_log: id("minecraft:spruce_log"),
            spruce_leaves: id("minecraft:spruce_leaves"),
            dark_oak_log: id("minecraft:dark_oak_log"),
            dark_oak_leaves: id("minecraft:dark_oak_leaves"),
            jungle_log: id("minecraft:jungle_log"),
            jungle_leaves: id("minecraft:jungle_leaves"),
            acacia_log: id("minecraft:acacia_log"),
            acacia_leaves: id("minecraft:acacia_leaves"),
            cherry_log: id("minecraft:cherry_log"),
            cherry_leaves: id("minecraft:cherry_leaves"),
            stone_bricks: id("minecraft:stone_bricks"),
            cobblestone: id("minecraft:cobblestone"),
            oak_planks: id("minecraft:oak_planks"),
            sandstone: id("minecraft:sandstone"),
            obsidian: id("minecraft:obsidian"),
            crying_obsidian: id("minecraft:crying_obsidian"),
            lantern: id("minecraft:lantern"),
            end_stone: id("minecraft:end_stone"),
            chorus_plant: id("minecraft:chorus_plant"),
            chorus_flower: id("minecraft:chorus_flower"),
        }
    }
}

#[inline]
fn mix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

#[inline]
fn chunk_seed(world_seed: u64, cx: i32, cz: i32, salt: u64) -> u64 {
    let coords = ((cx as i64 as u64) << 32) ^ (cz as i64 as u64 & 0xFFFF_FFFF);
    mix64(world_seed ^ coords ^ salt)
}

#[inline]
fn next_u32(state: &mut u64) -> u32 {
    *state = mix64(*state);
    (*state >> 32) as u32
}

#[inline]
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

#[inline]
fn smooth_lattice3(seed: u64, x: f64, y: f64, z: f64) -> f64 {
    let xi = x.floor() as i32;
    let yi = y.floor() as i32;
    let zi = z.floor() as i32;
    let fx = x - f64::from(xi);
    let fy = y - f64::from(yi);
    let fz = z - f64::from(zi);
    let ux = fx * fx * (3.0 - 2.0 * fx);
    let uy = fy * fy * (3.0 - 2.0 * fy);
    let uz = fz * fz * (3.0 - 2.0 * fz);

    let sample = |gx: i32, gy: i32, gz: i32| -> f64 {
        let h = mix64(
            seed ^ (gx as i64 as u64).wrapping_mul(0x9E37_79B1)
                ^ (gy as i64 as u64).wrapping_mul(0x85EB_CA77)
                ^ (gz as i64 as u64).wrapping_mul(0xC2B2_AE3D),
        );
        ((h >> 11) as f64 / ((1u64 << 53) as f64)) * 2.0 - 1.0
    };

    let c000 = sample(xi, yi, zi);
    let c100 = sample(xi + 1, yi, zi);
    let c010 = sample(xi, yi + 1, zi);
    let c110 = sample(xi + 1, yi + 1, zi);
    let c001 = sample(xi, yi, zi + 1);
    let c101 = sample(xi + 1, yi, zi + 1);
    let c011 = sample(xi, yi + 1, zi + 1);
    let c111 = sample(xi + 1, yi + 1, zi + 1);

    let x00 = lerp(c000, c100, ux);
    let x10 = lerp(c010, c110, ux);
    let x01 = lerp(c001, c101, ux);
    let x11 = lerp(c011, c111, ux);
    let y0 = lerp(x00, x10, uy);
    let y1 = lerp(x01, x11, uy);
    lerp(y0, y1, uz)
}

fn is_carved_cave(seed: u64, wx: i32, wy: i32, wz: i32) -> bool {
    let fx = f64::from(wx) / 14.0;
    let fy = f64::from(wy) / 9.0;
    let fz = f64::from(wz) / 14.0;
    let n1 = smooth_lattice3(seed ^ 0x4341_5645_0001, fx, fy, fz);
    if n1.abs() >= 0.11 {
        return false;
    }
    let n2 = smooth_lattice3(
        seed ^ 0x4341_5645_0002,
        fx * 1.45 + 11.3,
        fy * 1.45,
        fz * 1.45 - 7.1,
    );
    n2.abs() < 0.13
}

pub(super) struct DecorateSpec<'a> {
    pub seed: u64,
    pub dimension: Dimension,
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub pal: &'a DecorationPalette,
    pub registry: &'a RuntimeRegistry,
    pub structure_starts: &'a [(String, [i32; 6])],
    pub assembled_structure: Option<&'a [([i32; 3], u32)]>,
}

/// Applies deterministic caves, ores, biome vegetation/trees, and structure features in-place.
pub(super) fn decorate_chunk<F>(spec: DecorateSpec<'_>, chunk: &mut Chunk, mut biome_name_at: F)
where
    F: FnMut(i32, i32, i32) -> String,
{
    let DecorateSpec {
        seed,
        dimension,
        chunk_x,
        chunk_z,
        pal,
        registry,
        structure_starts,
        assembled_structure,
    } = spec;
    if dimension == Dimension::End {
        decorate_outer_end(seed, chunk_x, chunk_z, chunk, pal, registry);
        return;
    }

    let min_y = chunk.min_y();
    let max_y = min_y + chunk.height_blocks() as i32 - 1;
    let base_x = chunk_x * 16;
    let base_z = chunk_z * 16;

    // 1. Subsurface 3D cave carving (keeps >= 5 blocks beneath surface intact to preserve surface/spawn).
    let mut surface_ys = [min_y; 256];
    for lz in 0..16usize {
        for lx in 0..16usize {
            let surf_y = chunk
                .height(HeightmapKind::WorldSurface, lx as u8, lz as u8)
                .filter(|&h| h > 0)
                .map(|h| min_y + i32::from(h) - 1)
                .unwrap_or(min_y);
            surface_ys[lz * 16 + lx] = surf_y;

            let carve_top = match dimension {
                Dimension::Overworld => (surf_y - 5).min(max_y - 1),
                Dimension::Nether => 116.min(max_y - 2),
                Dimension::End => continue,
            };
            let carve_bottom = min_y + 2;
            if carve_top <= carve_bottom {
                continue;
            }
            let wx = base_x + lx as i32;
            let wz = base_z + lz as i32;
            for wy in carve_bottom..=carve_top {
                let Some(cur) = chunk.block(lx as u8, wy, lz as u8) else {
                    continue;
                };
                let replaceable = match dimension {
                    Dimension::Overworld => cur == pal.stone || cur == pal.deepslate,
                    Dimension::Nether => cur == pal.netherrack,
                    Dimension::End => false,
                };
                if replaceable && is_carved_cave(seed, wx, wy, wz) {
                    let fill = if dimension == Dimension::Overworld && wy <= -55 && pal.lava != 0 {
                        pal.lava
                    } else {
                        pal.air
                    };
                    let _ = chunk.set_block_raw(lx as u8, wy, lz as u8, fill, registry);
                }
            }
        }
    }

    // 2. Vanilla-band ore clusters (deterministic per chunk).
    let mut rng = chunk_seed(seed, chunk_x, chunk_z, 0x4F52_4553);
    if dimension == Dimension::Overworld {
        struct OreBand {
            stone_ore: u32,
            deep_ore: u32,
            min_y: i32,
            max_y: i32,
            attempts: u8,
            size: u8,
        }
        let bands = [
            OreBand {
                stone_ore: pal.coal_ore,
                deep_ore: pal.deepslate_coal_ore,
                min_y: 0,
                max_y: 160,
                attempts: 14,
                size: 7,
            },
            OreBand {
                stone_ore: pal.iron_ore,
                deep_ore: pal.deepslate_iron_ore,
                min_y: -56,
                max_y: 120,
                attempts: 14,
                size: 6,
            },
            OreBand {
                stone_ore: pal.copper_ore,
                deep_ore: pal.deepslate_copper_ore,
                min_y: -16,
                max_y: 96,
                attempts: 10,
                size: 6,
            },
            OreBand {
                stone_ore: pal.gold_ore,
                deep_ore: pal.deepslate_gold_ore,
                min_y: -58,
                max_y: 32,
                attempts: 5,
                size: 5,
            },
            OreBand {
                stone_ore: pal.redstone_ore,
                deep_ore: pal.deepslate_redstone_ore,
                min_y: -59,
                max_y: 15,
                attempts: 6,
                size: 5,
            },
            OreBand {
                stone_ore: pal.diamond_ore,
                deep_ore: pal.deepslate_diamond_ore,
                min_y: -59,
                max_y: 14,
                attempts: 5,
                size: 4,
            },
            OreBand {
                stone_ore: pal.lapis_ore,
                deep_ore: pal.deepslate_lapis_ore,
                min_y: -56,
                max_y: 48,
                attempts: 3,
                size: 4,
            },
        ];
        for band in bands {
            let lo = band.min_y.max(min_y + 1);
            let hi = band.max_y.min(max_y - 1);
            if lo > hi {
                continue;
            }
            let span = (hi - lo + 1) as u32;
            for _ in 0..band.attempts {
                let ox = (next_u32(&mut rng) % 16) as i32;
                let oz = (next_u32(&mut rng) % 16) as i32;
                let oy = lo + (next_u32(&mut rng) % span) as i32;
                place_ore_blob(
                    chunk,
                    &mut rng,
                    ox,
                    oy,
                    oz,
                    band.size,
                    pal,
                    band.stone_ore,
                    band.deep_ore,
                    registry,
                );
            }
        }

        // 3. Overworld surface vegetation and trees (kept cleanly inside 0..16 chunk bounds).
        let center_biome = biome_name_at(base_x + 8, 64, base_z + 8);
        place_overworld_vegetation(&mut rng, chunk, &surface_ys, &center_biome, pal, registry);
    } else if dimension == Dimension::Nether {
        for (ore, lo, hi, attempts, size) in [
            (pal.nether_quartz_ore, 10, 114, 14, 6),
            (pal.nether_gold_ore, 10, 114, 8, 5),
            (pal.ancient_debris, 8, 24, 2, 2),
        ] {
            if ore == 0 {
                continue;
            }
            let span = (hi - lo + 1) as u32;
            for _ in 0..attempts {
                let ox = (next_u32(&mut rng) % 16) as i32;
                let oz = (next_u32(&mut rng) % 16) as i32;
                let oy = lo + (next_u32(&mut rng) % span) as i32;
                for step in 0..size {
                    let dx = (ox + ((next_u32(&mut rng) % 3) as i32 - 1) * i32::from(step > 0))
                        .clamp(0, 15) as u8;
                    let dz = (oz + ((next_u32(&mut rng) % 3) as i32 - 1) * i32::from(step > 0))
                        .clamp(0, 15) as u8;
                    let dy = (oy + (next_u32(&mut rng) % 3) as i32 - 1).clamp(min_y + 1, max_y - 1);
                    if chunk.block(dx, dy, dz) == Some(pal.netherrack) {
                        let _ = chunk.set_block_raw(dx, dy, dz, ore, registry);
                    }
                }
            }
        }

        // Nether fungus scatter on nylium surfaces.
        for lz in 1..15u8 {
            for lx in 1..15u8 {
                if !next_u32(&mut rng).is_multiple_of(14) {
                    continue;
                }
                for wy in (16..110).rev() {
                    if chunk.block(lx, wy + 1, lz) != Some(pal.air) {
                        continue;
                    }
                    match chunk.block(lx, wy, lz) {
                        Some(b) if b == pal.crimson_nylium && pal.crimson_fungus != 0 => {
                            let _ =
                                chunk.set_block_raw(lx, wy + 1, lz, pal.crimson_fungus, registry);
                            break;
                        }
                        Some(b) if b == pal.warped_nylium && pal.warped_fungus != 0 => {
                            let _ =
                                chunk.set_block_raw(lx, wy + 1, lz, pal.warped_fungus, registry);
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    // 4. Deterministic structure surface landmarks when a structure starts in this chunk.
    if let Some((structure_id, _)) = structure_starts.first() {
        let fallback;
        let blocks = if let Some(preassembled) = assembled_structure {
            preassembled
        } else {
            let surf = surface_ys[8 * 16 + 8].max(min_y + 2).min(max_y - 8);
            let anchor_y = if dimension == Dimension::Nether {
                let mut found = 48;
                for wy in (32..96).rev() {
                    if chunk.block(8, wy, 8) != Some(pal.air)
                        && chunk.block(8, wy + 1, 8) == Some(pal.air)
                    {
                        found = wy + 1;
                        break;
                    }
                }
                found
            } else {
                surf + 1
            };
            fallback =
                structure_template_blocks(structure_id, [base_x + 8, anchor_y, base_z + 8], pal);
            &fallback
        };
        for &([wx, wy, wz], state) in blocks {
            if let Ok(lx) = u8::try_from(wx - base_x)
                && let Ok(lz) = u8::try_from(wz - base_z)
                && lx < 16
                && lz < 16
                && wy > min_y
                && wy < max_y
            {
                let _ = chunk.set_block_raw(lx, wy, lz, state, registry);
            }
        }
    }

    chunk.recompute_heightmaps(registry);
}

#[allow(clippy::too_many_arguments)]
fn place_ore_blob(
    chunk: &mut Chunk,
    rng: &mut u64,
    ox: i32,
    oy: i32,
    oz: i32,
    size: u8,
    pal: &DecorationPalette,
    stone_ore: u32,
    deep_ore: u32,
    registry: &RuntimeRegistry,
) {
    if stone_ore == 0 && deep_ore == 0 {
        return;
    }
    let min_y = chunk.min_y();
    let max_y = min_y + chunk.height_blocks() as i32 - 1;
    let mut x = ox;
    let mut y = oy;
    let mut z = oz;
    for _ in 0..size {
        let lx = x.clamp(0, 15) as u8;
        let lz = z.clamp(0, 15) as u8;
        let wy = y.clamp(min_y + 1, max_y - 1);
        match chunk.block(lx, wy, lz) {
            Some(b) if b == pal.stone && stone_ore != 0 => {
                let _ = chunk.set_block_raw(lx, wy, lz, stone_ore, registry);
            }
            Some(b) if b == pal.deepslate && deep_ore != 0 => {
                let _ = chunk.set_block_raw(lx, wy, lz, deep_ore, registry);
            }
            _ => {}
        }
        x += (next_u32(rng) % 3) as i32 - 1;
        y += (next_u32(rng) % 3) as i32 - 1;
        z += (next_u32(rng) % 3) as i32 - 1;
    }
}

fn place_overworld_vegetation(
    rng: &mut u64,
    chunk: &mut Chunk,
    surface_ys: &[i32; 256],
    biome_name: &str,
    pal: &DecorationPalette,
    registry: &RuntimeRegistry,
) {
    let short = biome_name.strip_prefix("minecraft:").unwrap_or(biome_name);
    let (tree_mod, log, leaves, two_by_two, desert_like) =
        if short.contains("desert") || short.contains("badlands") {
            (0u32, 0, 0, false, true)
        } else if short.contains("birch") {
            (18, pal.birch_log, pal.birch_leaves, false, false)
        } else if short.contains("taiga") || short.contains("grove") || short.contains("snowy") {
            (22, pal.spruce_log, pal.spruce_leaves, false, false)
        } else if short.contains("dark_forest") {
            (24, pal.dark_oak_log, pal.dark_oak_leaves, true, false)
        } else if short.contains("jungle") {
            (16, pal.jungle_log, pal.jungle_leaves, false, false)
        } else if short.contains("savanna") {
            (36, pal.acacia_log, pal.acacia_leaves, false, false)
        } else if short.contains("cherry") {
            (24, pal.cherry_log, pal.cherry_leaves, false, false)
        } else if short.contains("forest") || short.contains("swamp") {
            (18, pal.oak_log, pal.oak_leaves, false, false)
        } else if short.contains("plains") || short.contains("meadow") {
            (90, pal.oak_log, pal.oak_leaves, false, false)
        } else {
            (0, 0, 0, false, false)
        };

    let mut placed_tree = false;
    for lz in 2..14u8 {
        for lx in 2..14u8 {
            let surf_y = surface_ys[(lz as usize) * 16 + (lx as usize)];
            if surf_y < 63 {
                continue;
            }
            let Some(ground) = chunk.block(lx, surf_y, lz) else {
                continue;
            };
            if chunk.block(lx, surf_y + 1, lz) != Some(pal.air) {
                continue;
            }
            if desert_like && (ground == pal.sand || ground == pal.red_sand) {
                let roll = next_u32(rng) % 48;
                if roll == 0 && pal.cactus != 0 {
                    let h = 1 + (next_u32(rng) % 3) as i32;
                    for dy in 1..=h {
                        let _ = chunk.set_block_raw(lx, surf_y + dy, lz, pal.cactus, registry);
                    }
                } else if roll == 1 && pal.dead_bush != 0 {
                    let _ = chunk.set_block_raw(lx, surf_y + 1, lz, pal.dead_bush, registry);
                }
                continue;
            }

            let valid_soil = ground == pal.grass_block
                || ground == pal.dirt
                || ground == pal.podzol
                || ground == pal.snow_block;
            if !valid_soil {
                continue;
            }

            if !placed_tree
                && tree_mod > 0
                && log != 0
                && leaves != 0
                && next_u32(rng).is_multiple_of(tree_mod)
                && chunk.block(lx, surf_y + 5, lz) == Some(pal.air)
            {
                let _ = chunk.set_block_raw(lx, surf_y, lz, pal.dirt, registry);
                let trunk_h = if two_by_two { 6 } else { 5 };
                for dy in 1..=trunk_h {
                    let _ = chunk.set_block_raw(lx, surf_y + dy, lz, log, registry);
                }
                for dx in -1i32..=1 {
                    for dz in -1i32..=1 {
                        let tx = (i32::from(lx) + dx) as u8;
                        let tz = (i32::from(lz) + dz) as u8;
                        for dy in (trunk_h - 2)..=trunk_h {
                            if (dx, dz) == (0, 0) && dy < trunk_h {
                                continue;
                            }
                            if chunk.block(tx, surf_y + dy, tz) == Some(pal.air) {
                                let _ = chunk.set_block_raw(tx, surf_y + dy, tz, leaves, registry);
                            }
                        }
                    }
                }
                placed_tree = true;
                continue;
            }

            let flora_roll = next_u32(rng) % 18;
            let flora = match flora_roll {
                0..=2 => pal.short_grass,
                3 if short.contains("taiga") => pal.fern,
                4 => pal.dandelion,
                5 => pal.poppy,
                _ => 0,
            };
            if flora != 0 {
                let _ = chunk.set_block_raw(lx, surf_y + 1, lz, flora, registry);
            }
        }
    }
}

/// Builds distinct multi-block structure templates for `/place structure` and chunk generation.
pub(super) fn structure_template_blocks(
    id: &str,
    pos: [i32; 3],
    pal: &DecorationPalette,
) -> Vec<([i32; 3], u32)> {
    let short = id.strip_prefix("minecraft:").unwrap_or(id);
    let block_or = |name: &str, fallback: u32| -> u32 {
        ferrite_data::blocks::default_state(name)
            .map(u32::from)
            .unwrap_or(fallback)
    };
    let [x, y, z] = pos;
    let mut blocks = Vec::new();

    if short.contains("portal") {
        let wall = pal.obsidian.max(1);
        let trim = if pal.crying_obsidian != 0 {
            pal.crying_obsidian
        } else {
            wall
        };
        let floor = pal.netherrack.max(1);
        for dx in -2..=2 {
            for dz in -1..=1 {
                blocks.push(([x + dx, y - 1, z + dz], floor));
            }
            blocks.push(([x + dx, y, z], if dx == 0 { trim } else { wall }));
        }
        for dy in 1..=3 {
            blocks.push(([x - 2, y + dy, z], wall));
            blocks.push(([x + 2, y + dy, z], if dy == 2 { trim } else { wall }));
        }
        for dx in -1..=1 {
            blocks.push(([x + dx, y + 4, z], wall));
        }
        return blocks;
    }

    if short.contains("desert") && pal.sandstone != 0 {
        let sandstone = pal.sandstone;
        let accent = block_or("minecraft:terracotta", sandstone);
        for level in 0..=3i32 {
            let half = 3 - level;
            for dx in -half..=half {
                for dz in -half..=half {
                    let is_edge = dx.abs() == half || dz.abs() == half;
                    let is_door = level <= 1 && dx == 0 && dz == half;
                    if level == 0 || is_edge {
                        let mat = if (level == 1 && dx == 0 && dz == 0)
                            || (level == 2 && (dx == 0 || dz == 0))
                        {
                            accent
                        } else {
                            sandstone
                        };
                        if !is_door {
                            blocks.push(([x + dx, y + level, z + dz], mat));
                        } else {
                            blocks.push(([x + dx, y + level, z + dz], pal.air));
                        }
                    } else {
                        blocks.push(([x + dx, y + level, z + dz], pal.air));
                    }
                }
            }
        }
        if pal.lantern != 0 {
            blocks.push(([x, y + 2, z], pal.lantern));
        }
        return blocks;
    }

    if short.contains("igloo") && pal.snow_block != 0 {
        let snow = pal.snow_block;
        let ice = block_or("minecraft:ice", snow);
        for dx in -2..=2 {
            for dz in -2..=2 {
                blocks.push(([x + dx, y - 1, z + dz], snow));
                let is_edge = dx.abs() == 2 || dz.abs() == 2;
                let is_corner = dx.abs() == 2 && dz.abs() == 2;
                let is_door = dx == 0 && dz == 2;
                for dy in 0..=2 {
                    if dy == 2 && !is_corner {
                        blocks.push(([x + dx, y + dy, z + dz], snow));
                    } else if is_edge && !is_corner && !(is_door && dy < 2) {
                        let mat = if dy == 1 && dz == 0 && dx.abs() == 2 {
                            ice
                        } else {
                            snow
                        };
                        blocks.push(([x + dx, y + dy, z + dz], mat));
                    } else if dy < 2 {
                        blocks.push(([x + dx, y + dy, z + dz], pal.air));
                    }
                }
            }
        }
        if pal.lantern != 0 {
            blocks.push(([x, y + 1, z], pal.lantern));
        }
        return blocks;
    }

    let (wall, trim, roof, max_y_rel) =
        if short.contains("monument") || short.contains("ocean_ruin") {
            let prism = block_or("minecraft:prismarine_bricks", pal.stone_bricks.max(1));
            let lantern = block_or("minecraft:sea_lantern", prism);
            (prism, lantern, prism, 4)
        } else if short.contains("fortress") || short.contains("bastion") {
            let nether_bricks = block_or("minecraft:nether_bricks", pal.stone_bricks.max(1));
            let blackstone = block_or("minecraft:blackstone", nether_bricks);
            (nether_bricks, blackstone, nether_bricks, 5)
        } else if short.contains("outpost") {
            let dark_planks = block_or("minecraft:dark_oak_planks", pal.oak_planks.max(1));
            (
                pal.cobblestone.max(1),
                pal.dark_oak_log.max(pal.oak_log.max(1)),
                dark_planks,
                6,
            )
        } else if short.contains("village") {
            (
                pal.oak_planks.max(1),
                pal.cobblestone.max(1),
                pal.oak_log.max(pal.oak_planks.max(1)),
                4,
            )
        } else {
            (
                pal.stone_bricks.max(1),
                pal.cobblestone.max(1),
                pal.oak_planks.max(1),
                5,
            )
        };

    for dx in -2..=2 {
        for dz in -2..=2 {
            blocks.push(([x + dx, y - 1, z + dz], trim));
            let is_edge = dx.abs() == 2 || dz.abs() == 2;
            let is_corner = dx.abs() == 2 && dz.abs() == 2;
            let is_door = dx == 0 && dz == 2;
            for dy in 0..max_y_rel {
                if is_edge && !(is_door && dy < 2) {
                    let b = if is_corner || dy == 0 { trim } else { wall };
                    blocks.push(([x + dx, y + dy, z + dz], b));
                } else if dy < max_y_rel - 1 {
                    blocks.push(([x + dx, y + dy, z + dz], pal.air));
                }
            }
            if is_corner || !is_edge {
                blocks.push(([x + dx, y + max_y_rel - 1, z + dz], roof));
            } else {
                blocks.push(([x + dx, y + max_y_rel - 1, z + dz], wall));
            }
            if is_corner || (dx.abs() == 2 && dz == 0) || (dz.abs() == 2 && dx == 0) {
                blocks.push(([x + dx, y + max_y_rel, z + dz], wall));
            }
        }
    }
    if pal.lantern != 0 {
        blocks.push(([x, y + max_y_rel - 2, z], pal.lantern));
    }
    blocks
}

fn decorate_outer_end(
    seed: u64,
    chunk_x: i32,
    chunk_z: i32,
    chunk: &mut Chunk,
    pal: &DecorationPalette,
    registry: &RuntimeRegistry,
) {
    // Keep the main dragon island (r < 64 chunks) unobstructed; decorate outer End islands only.
    let dist_sq = i64::from(chunk_x) * i64::from(chunk_x) + i64::from(chunk_z) * i64::from(chunk_z);
    if dist_sq < 4096 || pal.end_stone == 0 || pal.chorus_plant == 0 || pal.chorus_flower == 0 {
        return;
    }

    let min_y = chunk.min_y();
    let max_y = min_y + chunk.height_blocks() as i32 - 1;
    let mut rng = chunk_seed(seed, chunk_x, chunk_z, 0x454E_4443);
    let mut changed = false;

    for lz in 2..14u8 {
        for lx in 2..14u8 {
            if !next_u32(&mut rng).is_multiple_of(55) {
                continue;
            }
            let Some(h) = chunk.height(HeightmapKind::WorldSurface, lx, lz) else {
                continue;
            };
            if h == 0 {
                continue;
            }
            let surf_y = min_y + i32::from(h) - 1;
            if surf_y + 7 >= max_y || chunk.block(lx, surf_y, lz) != Some(pal.end_stone) {
                continue;
            }
            let height = 3 + (next_u32(&mut rng) % 4) as i32;
            for dy in 1..=height {
                let _ = chunk.set_block_raw(lx, surf_y + dy, lz, pal.chorus_plant, registry);
            }
            let _ = chunk.set_block_raw(lx, surf_y + height + 1, lz, pal.chorus_flower, registry);
            changed = true;
        }
    }

    if changed {
        chunk.recompute_heightmaps(registry);
    }
}
