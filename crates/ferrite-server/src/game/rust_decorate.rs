//! Pure-Rust deterministic post-terrain chunk decoration: 3D cave carving,
//! vanilla-band ore veins, biome-aware trees/flora, and structure features.

use std::collections::BTreeMap;

use ferrite_world::{BlockStateRegistry, RuntimeRegistry};

use super::Dimension;

/// Pre-resolved block state IDs for fast O(1) decoration without hash lookups per block.
#[derive(Clone, Copy)]
pub(super) struct DecorationPalette {
    pub air: u32,
    pub stone: u32,
    pub deepslate: u32,
    pub granite: u32,
    pub diorite: u32,
    pub andesite: u32,
    pub tuff: u32,
    pub gravel: u32,
    pub clay: u32,
    pub water: u32,
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
    pub leaf_litter: u32,
    pub seagrass: u32,
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
    pub mossy_cobblestone: u32,
    pub spawner: u32,
    pub chest: u32,
    pub crimson_stem: u32,
    pub warped_stem: u32,
    pub nether_wart_block: u32,
    pub warped_wart_block: u32,
    pub shroomlight: u32,
    pub glowstone: u32,
    pub end_portal_frame: u32,
    pub rail: u32,
    pub cobweb: u32,
    pub mycelium: u32,
    pub mushroom_stem: u32,
    pub red_mushroom_block: u32,
    pub brown_mushroom_block: u32,
    pub sandstone_slab: u32,
    pub spruce_planks: u32,
    pub spruce_fence: u32,
    pub cauldron: u32,
    pub crafting_table: u32,
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
        // Generated tree leaves MUST have `distance=1, persistent=false` instead of the vanilla
        // default (`distance=7, persistent=false`), otherwise random ticks immediately decay and
        // shred every natural tree canopy as soon as chunks load.
        let leaf_id = |name: &str| -> u32 {
            ferrite_data::blocks::state_id(
                name,
                &[
                    ("distance", "1"),
                    ("persistent", "false"),
                    ("waterlogged", "false"),
                ],
            )
            .map(u32::from)
            .unwrap_or_else(|| id(name))
        };
        Self {
            air: id("minecraft:air"),
            stone: id("minecraft:stone"),
            deepslate: id("minecraft:deepslate"),
            granite: id("minecraft:granite"),
            diorite: id("minecraft:diorite"),
            andesite: id("minecraft:andesite"),
            tuff: id("minecraft:tuff"),
            gravel: id("minecraft:gravel"),
            clay: id("minecraft:clay"),
            water: id("minecraft:water"),
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
            leaf_litter: id("minecraft:leaf_litter"),
            seagrass: id("minecraft:seagrass"),
            cactus: id("minecraft:cactus"),
            dead_bush: id("minecraft:dead_bush"),
            crimson_fungus: id("minecraft:crimson_fungus"),
            warped_fungus: id("minecraft:warped_fungus"),
            oak_log: id("minecraft:oak_log"),
            oak_leaves: leaf_id("minecraft:oak_leaves"),
            birch_log: id("minecraft:birch_log"),
            birch_leaves: leaf_id("minecraft:birch_leaves"),
            spruce_log: id("minecraft:spruce_log"),
            spruce_leaves: leaf_id("minecraft:spruce_leaves"),
            dark_oak_log: id("minecraft:dark_oak_log"),
            dark_oak_leaves: leaf_id("minecraft:dark_oak_leaves"),
            jungle_log: id("minecraft:jungle_log"),
            jungle_leaves: leaf_id("minecraft:jungle_leaves"),
            acacia_log: id("minecraft:acacia_log"),
            acacia_leaves: leaf_id("minecraft:acacia_leaves"),
            cherry_log: id("minecraft:cherry_log"),
            cherry_leaves: leaf_id("minecraft:cherry_leaves"),
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
            mossy_cobblestone: id("minecraft:mossy_cobblestone"),
            spawner: id("minecraft:spawner"),
            chest: id("minecraft:chest"),
            crimson_stem: id("minecraft:crimson_stem"),
            warped_stem: id("minecraft:warped_stem"),
            nether_wart_block: id("minecraft:nether_wart_block"),
            warped_wart_block: id("minecraft:warped_wart_block"),
            shroomlight: id("minecraft:shroomlight"),
            glowstone: id("minecraft:glowstone"),
            end_portal_frame: id("minecraft:end_portal_frame"),
            rail: id("minecraft:rail"),
            cobweb: id("minecraft:cobweb"),
            mycelium: id("minecraft:mycelium"),
            mushroom_stem: id("minecraft:mushroom_stem"),
            red_mushroom_block: id("minecraft:red_mushroom_block"),
            brown_mushroom_block: id("minecraft:brown_mushroom_block"),
            sandstone_slab: id("minecraft:sandstone_slab"),
            spruce_planks: id("minecraft:spruce_planks"),
            spruce_fence: id("minecraft:spruce_fence"),
            cauldron: id("minecraft:cauldron"),
            crafting_table: id("minecraft:crafting_table"),
        }
    }
}

/// Block entity metadata emitted during deterministic chunk decoration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DecoratedBlockEntity {
    LootChest { loot_table: &'static str, seed: i64 },
    MobSpawner { entity_id: &'static str },
}

pub(super) type PlacedBlockEntities = Vec<((u8, i32, u8), DecoratedBlockEntity)>;

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

pub(super) struct DecorateSpec<'a> {
    pub seed: u64,
    pub dimension: Dimension,
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub min_y: i32,
    pub height_blocks: u32,
    pub pal: &'a DecorationPalette,
    pub surface_h: &'a [u16; 256],
    pub structure_starts: &'a [(String, [i32; 6])],
    pub assembled_structure: Option<&'a [([i32; 3], u32)]>,
    pub center_biome: &'a str,
}

#[inline(always)]
fn block_u16(blocks: &[u16], min_y: i32, lx: u8, wy: i32, lz: u8) -> Option<u32> {
    if lx >= 16 || lz >= 16 || wy < min_y {
        return None;
    }
    let y_idx = (wy - min_y) as usize;
    blocks
        .get(y_idx * 256 + usize::from(lz) * 16 + usize::from(lx))
        .copied()
        .map(u32::from)
}

#[inline(always)]
fn set_block_u16(
    blocks: &mut [u16],
    min_y: i32,
    max_written_y: &mut i32,
    lx: u8,
    wy: i32,
    lz: u8,
    state: u32,
) {
    if lx >= 16 || lz >= 16 || wy < min_y {
        return;
    }
    let y_idx = (wy - min_y) as usize;
    let idx = y_idx * 256 + usize::from(lz) * 16 + usize::from(lx);
    if let Some(slot) = blocks.get_mut(idx) {
        let s = state as u16;
        *slot = s;
        if s != 0 && wy > *max_written_y {
            *max_written_y = wy;
        }
    }
}

/// Applies deterministic caves, ores, biome vegetation/trees, and structure features directly
/// onto the flat `blocks` buffer (`blocks[y_idx * 256 + lz * 16 + lx]`), returning the highest
/// world `Y` modified with a non-air block (or `min_y - 1` if none) together with any
/// generated block entities (dungeon spawners, structure/dungeon loot chests).
pub(super) fn decorate_chunk_u16(
    spec: DecorateSpec<'_>,
    blocks: &mut [u16],
) -> (i32, PlacedBlockEntities) {
    let DecorateSpec {
        seed,
        dimension,
        chunk_x,
        chunk_z,
        min_y,
        height_blocks,
        pal,
        surface_h,
        structure_starts,
        assembled_structure,
        center_biome,
    } = spec;
    let mut max_written_y = min_y - 1;
    let mut block_entities = Vec::new();
    if dimension == Dimension::End {
        decorate_outer_end(
            seed,
            chunk_x,
            chunk_z,
            min_y,
            height_blocks,
            surface_h,
            blocks,
            &mut max_written_y,
            pal,
        );
        return (max_written_y, block_entities);
    }

    let max_y = min_y + height_blocks as i32 - 1;
    let base_x = chunk_x * 16;
    let base_z = chunk_z * 16;

    let mut surface_ys = [min_y; 256];
    let mut carve_tops = [min_y; 256];
    let mut global_carve_top = min_y;
    for lz in 0..16usize {
        for lx in 0..16usize {
            let col = lz * 16 + lx;
            let surf_h = surface_h[col];
            let surf_y = if surf_h > 0 {
                min_y + i32::from(surf_h) - 1
            } else {
                min_y
            };
            surface_ys[col] = surf_y;
            let ct = match dimension {
                Dimension::Overworld => (surf_y - 5).min(max_y - 1),
                Dimension::Nether => 116.min(max_y - 2),
                Dimension::End => min_y,
            };
            carve_tops[col] = ct;
            if ct > global_carve_top {
                global_carve_top = ct;
            }
        }
    }

    // 1. Deep lava pools in vanilla noise-carved caves (y <= -55). Vanilla `final_density`
    // already carves cheese/spaghetti/noodle caves; avoid synthetic grid ribbons on top of them.
    if dimension == Dimension::Overworld && pal.lava != 0 {
        let lava_u16 = pal.lava as u16;
        let air_u16 = pal.air as u16;
        let lava_hi = (-55).min(max_y - 1);
        for wy in (min_y + 1)..=lava_hi {
            let row = ((wy - min_y) as usize) * 256;
            for cell in &mut blocks[row..row + 256] {
                if *cell == air_u16 {
                    *cell = lava_u16;
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
                attempts: 30,
                size: 10,
            },
            OreBand {
                stone_ore: pal.iron_ore,
                deep_ore: pal.deepslate_iron_ore,
                min_y: -56,
                max_y: 120,
                attempts: 24,
                size: 7,
            },
            OreBand {
                stone_ore: pal.copper_ore,
                deep_ore: pal.deepslate_copper_ore,
                min_y: -16,
                max_y: 96,
                attempts: 18,
                size: 7,
            },
            OreBand {
                stone_ore: pal.gold_ore,
                deep_ore: pal.deepslate_gold_ore,
                min_y: -58,
                max_y: 32,
                attempts: 6,
                size: 6,
            },
            OreBand {
                stone_ore: pal.redstone_ore,
                deep_ore: pal.deepslate_redstone_ore,
                min_y: -59,
                max_y: 15,
                attempts: 9,
                size: 6,
            },
            OreBand {
                stone_ore: pal.diamond_ore,
                deep_ore: 0,
                min_y: 0,
                max_y: 14,
                attempts: 1,
                size: 3,
            },
            OreBand {
                stone_ore: 0,
                deep_ore: pal.deepslate_diamond_ore,
                min_y: -60,
                max_y: -4,
                attempts: 11,
                size: 7,
            },
            OreBand {
                stone_ore: pal.lapis_ore,
                deep_ore: pal.deepslate_lapis_ore,
                min_y: -56,
                max_y: 48,
                attempts: 6,
                size: 5,
            },
        ];
        // Underground stone variants (granite, diorite, andesite, tuff) and gravel blobs.
        for (block_id, target_stone, lo_y, hi_y, attempts, radius) in [
            (pal.andesite, pal.stone, 0i32, 60i32, 3u8, 4i32),
            (pal.granite, pal.stone, 0, 60, 3, 4),
            (pal.diorite, pal.stone, 0, 60, 3, 4),
            (pal.tuff, pal.deepslate, -60, -2, 4, 4),
            (pal.gravel, pal.stone, -40, 60, 4, 3),
        ] {
            if block_id == 0 || target_stone == 0 {
                continue;
            }
            let lo = lo_y.max(min_y + 1);
            let hi = hi_y.min(max_y - 1);
            if lo > hi {
                continue;
            }
            let span = (hi - lo + 1) as u32;
            for _ in 0..attempts {
                let ox = (next_u32(&mut rng) % 16) as i32;
                let oz = (next_u32(&mut rng) % 16) as i32;
                let oy = lo + (next_u32(&mut rng) % span) as i32;
                place_rock_blob(
                    blocks,
                    min_y,
                    max_y,
                    ox,
                    oy,
                    oz,
                    radius,
                    target_stone as u16,
                    block_id as u16,
                );
            }
        }
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
                    blocks,
                    min_y,
                    max_y,
                    &mut rng,
                    ox,
                    oy,
                    oz,
                    band.size,
                    pal,
                    band.stone_ore,
                    band.deep_ore,
                );
            }
        }

        // 3. Overworld surface vegetation and trees (kept cleanly inside 0..16 chunk bounds).
        place_overworld_vegetation(
            &mut rng,
            blocks,
            min_y,
            &mut max_written_y,
            &surface_ys,
            center_biome,
            pal,
        );

        // 3b. Biome surface features: desert wells, mushroom-island huge mushrooms, surface ponds.
        place_overworld_biome_features(
            &mut rng,
            blocks,
            min_y,
            max_y,
            &mut max_written_y,
            &surface_ys,
            center_biome,
            pal,
        );

        // 3c. Occasional underground monster room (dungeon) with spawner + 2 loot chests.
        if next_u32(&mut rng).is_multiple_of(24) {
            place_underground_dungeon(
                &mut rng,
                blocks,
                min_y,
                &mut max_written_y,
                pal,
                &mut block_entities,
            );
        }
    } else if dimension == Dimension::Nether {
        let netherrack_u16 = pal.netherrack as u16;
        for (ore, lo, hi, attempts, size) in [
            (pal.nether_quartz_ore, 10, 114, 14, 6),
            (pal.nether_gold_ore, 10, 114, 8, 5),
            (pal.ancient_debris, 8, 24, 2, 2),
        ] {
            if ore == 0 {
                continue;
            }
            let ore_u16 = ore as u16;
            let span = (hi - lo + 1) as u32;
            for _ in 0..attempts {
                let ox = (next_u32(&mut rng) % 16) as i32;
                let oz = (next_u32(&mut rng) % 16) as i32;
                let oy = lo + (next_u32(&mut rng) % span) as i32;
                for step in 0..size {
                    let dx = (ox + ((next_u32(&mut rng) % 3) as i32 - 1) * i32::from(step > 0))
                        .clamp(0, 15) as usize;
                    let dz = (oz + ((next_u32(&mut rng) % 3) as i32 - 1) * i32::from(step > 0))
                        .clamp(0, 15) as usize;
                    let dy = (oy + (next_u32(&mut rng) % 3) as i32 - 1).clamp(min_y + 1, max_y - 1);
                    let idx = ((dy - min_y) as usize) * 256 + dz * 16 + dx;
                    if blocks[idx] == netherrack_u16 {
                        blocks[idx] = ore_u16;
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
                    if block_u16(blocks, min_y, lx, wy + 1, lz) != Some(pal.air) {
                        continue;
                    }
                    match block_u16(blocks, min_y, lx, wy, lz) {
                        Some(b) if b == pal.crimson_nylium && pal.crimson_fungus != 0 => {
                            set_block_u16(
                                blocks,
                                min_y,
                                &mut max_written_y,
                                lx,
                                wy + 1,
                                lz,
                                pal.crimson_fungus,
                            );
                            break;
                        }
                        Some(b) if b == pal.warped_nylium && pal.warped_fungus != 0 => {
                            set_block_u16(
                                blocks,
                                min_y,
                                &mut max_written_y,
                                lx,
                                wy + 1,
                                lz,
                                pal.warped_fungus,
                            );
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }

        // Nether huge fungi on crimson/warped nylium and ceiling glowstone clusters.
        place_nether_features(&mut rng, blocks, min_y, max_y, &mut max_written_y, pal);
    }

    // 4. Deterministic structure surface landmarks when a structure starts in this chunk.
    if let Some((structure_id, _)) = structure_starts.first() {
        let fallback;
        let landmark_blocks = if let Some(preassembled) = assembled_structure {
            preassembled
        } else {
            let surf = surface_ys[8 * 16 + 8].max(min_y + 2).min(max_y - 8);
            let anchor_y = if dimension == Dimension::Nether {
                let mut found = 48;
                for wy in (32..96).rev() {
                    if block_u16(blocks, min_y, 8, wy, 8) != Some(pal.air)
                        && block_u16(blocks, min_y, 8, wy + 1, 8) == Some(pal.air)
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
        let loot_table = structure_loot_table(structure_id);
        for &([wx, wy, wz], state) in landmark_blocks {
            if let Ok(lx) = u8::try_from(wx - base_x)
                && let Ok(lz) = u8::try_from(wz - base_z)
                && lx < 16
                && lz < 16
                && wy > min_y
                && wy < max_y
            {
                set_block_u16(blocks, min_y, &mut max_written_y, lx, wy, lz, state);
                if pal.chest != 0
                    && state == pal.chest
                    && let Some(table) = loot_table
                {
                    let loot_seed = (next_u32(&mut rng) as i64) << 32 | (next_u32(&mut rng) as i64);
                    block_entities.push((
                        (lx, wy, lz),
                        DecoratedBlockEntity::LootChest {
                            loot_table: table,
                            seed: loot_seed,
                        },
                    ));
                }
            }
        }
    }

    (max_written_y, block_entities)
}

fn structure_loot_table(structure_id: &str) -> Option<&'static str> {
    let short = structure_id
        .strip_prefix("minecraft:")
        .unwrap_or(structure_id);
    if short.contains("shipwreck") {
        Some("minecraft:chests/shipwreck_treasure")
    } else if short.contains("desert") {
        Some("minecraft:chests/desert_pyramid")
    } else if short.contains("stronghold") {
        Some("minecraft:chests/stronghold_corridor")
    } else if short.contains("mineshaft") {
        Some("minecraft:chests/abandoned_mineshaft")
    } else if short.contains("igloo") {
        Some("minecraft:chests/igloo_chest")
    } else if short.contains("village") {
        Some("minecraft:chests/village/village_plains_house")
    } else if short.contains("bastion") {
        Some("minecraft:chests/bastion_treasure")
    } else if short.contains("fortress") {
        Some("minecraft:chests/nether_bridge")
    } else if short.contains("end_city") {
        Some("minecraft:chests/end_city_treasure")
    } else if short.contains("outpost") {
        Some("minecraft:chests/pillager_outpost")
    } else if short.contains("portal") {
        Some("minecraft:chests/ruined_portal")
    } else {
        None
    }
}

#[allow(clippy::too_many_arguments)]
fn place_ore_blob(
    blocks: &mut [u16],
    min_y: i32,
    max_y: i32,
    rng: &mut u64,
    ox: i32,
    oy: i32,
    oz: i32,
    size: u8,
    pal: &DecorationPalette,
    stone_ore: u32,
    deep_ore: u32,
) {
    if stone_ore == 0 && deep_ore == 0 {
        return;
    }
    let stone_u16 = pal.stone as u16;
    let deep_u16 = pal.deepslate as u16;
    let stone_ore_u16 = stone_ore as u16;
    let deep_ore_u16 = deep_ore as u16;
    let mut x = ox;
    let mut y = oy;
    let mut z = oz;
    for _ in 0..size {
        let lx = x.clamp(0, 15) as usize;
        let lz = z.clamp(0, 15) as usize;
        let wy = y.clamp(min_y + 1, max_y - 1);
        let idx = ((wy - min_y) as usize) * 256 + lz * 16 + lx;
        let cur = blocks[idx];
        if cur == stone_u16 && stone_ore_u16 != 0 {
            blocks[idx] = stone_ore_u16;
        } else if cur == deep_u16 && deep_ore_u16 != 0 {
            blocks[idx] = deep_ore_u16;
        }
        x += (next_u32(rng) % 3) as i32 - 1;
        y += (next_u32(rng) % 3) as i32 - 1;
        z += (next_u32(rng) % 3) as i32 - 1;
    }
}

#[allow(clippy::too_many_arguments)]
fn place_rock_blob(
    blocks: &mut [u16],
    min_y: i32,
    max_y: i32,
    ox: i32,
    oy: i32,
    oz: i32,
    radius: i32,
    target: u16,
    replace: u16,
) {
    let r2 = radius * radius;
    let x0 = (ox - radius).max(0);
    let x1 = (ox + radius).min(15);
    let z0 = (oz - radius).max(0);
    let z1 = (oz + radius).min(15);
    let y0 = (oy - radius).max(min_y + 1);
    let y1 = (oy + radius).min(max_y - 1);
    for wy in y0..=y1 {
        let dy = wy - oy;
        let row = ((wy - min_y) as usize) * 256;
        for lz in z0..=z1 {
            let dz = lz - oz;
            for lx in x0..=x1 {
                let dx = lx - ox;
                if dx * dx + dy * dy + dz * dz <= r2 {
                    let idx = row + (lz as usize) * 16 + (lx as usize);
                    if blocks[idx] == target {
                        blocks[idx] = replace;
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TreeShape {
    Oak,
    Birch,
    Spruce,
    DarkOak,
    Acacia,
    Jungle,
    Cherry,
}

#[inline]
#[allow(clippy::too_many_arguments)]
fn put_leaf_u16(
    blocks: &mut [u16],
    min_y: i32,
    max_written_y: &mut i32,
    x: i32,
    y: i32,
    z: i32,
    leaves: u32,
    air: u32,
) {
    if let (Ok(tx), Ok(tz)) = (u8::try_from(x), u8::try_from(z))
        && tx < 16
        && tz < 16
        && block_u16(blocks, min_y, tx, y, tz) == Some(air)
    {
        set_block_u16(blocks, min_y, max_written_y, tx, y, tz, leaves);
    }
}

#[allow(clippy::too_many_arguments)]
fn place_vanilla_tree(
    rng: &mut u64,
    blocks: &mut [u16],
    min_y: i32,
    max_written_y: &mut i32,
    lx: u8,
    surf_y: i32,
    lz: u8,
    shape: TreeShape,
    log: u32,
    leaves: u32,
    pal: &DecorationPalette,
) {
    let cx = i32::from(lx);
    let cz = i32::from(lz);

    match shape {
        TreeShape::Spruce => {
            let trunk_h = 6 + (next_u32(rng) % 4) as i32;
            set_block_u16(blocks, min_y, max_written_y, lx, surf_y, lz, pal.dirt);
            for dy in 1..=trunk_h {
                set_block_u16(blocks, min_y, max_written_y, lx, surf_y + dy, lz, log);
            }
            let top = surf_y + trunk_h + 1;
            put_leaf_u16(blocks, min_y, max_written_y, cx, top, cz, leaves, pal.air);
            for (dy, rad) in [
                (0, 0i32),
                (-1, 1),
                (-2, 0),
                (-3, 1),
                (-4, 2),
                (-5, 1),
                (-6, 2),
            ] {
                let y = top + dy;
                if y <= surf_y + 1 {
                    break;
                }
                for dx in -rad..=rad {
                    for dz in -rad..=rad {
                        if rad >= 2 && dx.abs() == rad && dz.abs() == rad {
                            continue;
                        }
                        if !(dx == 0 && dz == 0 && y <= surf_y + trunk_h) {
                            put_leaf_u16(
                                blocks,
                                min_y,
                                max_written_y,
                                cx + dx,
                                y,
                                cz + dz,
                                leaves,
                                pal.air,
                            );
                        }
                    }
                }
            }
        }
        TreeShape::DarkOak => {
            if lx >= 13 || lz >= 13 {
                return;
            }
            let trunk_h = 5 + (next_u32(rng) % 3) as i32;
            for dx in 0..=1u8 {
                for dz in 0..=1u8 {
                    set_block_u16(
                        blocks,
                        min_y,
                        max_written_y,
                        lx + dx,
                        surf_y,
                        lz + dz,
                        pal.dirt,
                    );
                    for dy in 1..=trunk_h {
                        set_block_u16(
                            blocks,
                            min_y,
                            max_written_y,
                            lx + dx,
                            surf_y + dy,
                            lz + dz,
                            log,
                        );
                    }
                }
            }
            for dy in (trunk_h - 1)..=(trunk_h + 1) {
                let rad = if dy == trunk_h + 1 { 1 } else { 2 };
                for dx in -rad..=(rad + 1) {
                    for dz in -rad..=(rad + 1) {
                        let edge_x = dx == -rad || dx == rad + 1;
                        let edge_z = dz == -rad || dz == rad + 1;
                        if edge_x && edge_z && !next_u32(rng).is_multiple_of(3) {
                            continue;
                        }
                        put_leaf_u16(
                            blocks,
                            min_y,
                            max_written_y,
                            cx + dx,
                            surf_y + dy,
                            cz + dz,
                            leaves,
                            pal.air,
                        );
                    }
                }
            }
        }
        TreeShape::Acacia => {
            let trunk_h = 4 + (next_u32(rng) % 3) as i32;
            let lean_x = if next_u32(rng).is_multiple_of(2) {
                1
            } else {
                -1
            };
            let lean_z = if next_u32(rng).is_multiple_of(2) {
                0
            } else {
                1
            };
            set_block_u16(blocks, min_y, max_written_y, lx, surf_y, lz, pal.dirt);
            for dy in 1..=(trunk_h - 2) {
                set_block_u16(blocks, min_y, max_written_y, lx, surf_y + dy, lz, log);
            }
            let top_x = (cx + lean_x).clamp(2, 13);
            let top_z = (cz + lean_z).clamp(2, 13);
            for dy in (trunk_h - 1)..=trunk_h {
                set_block_u16(
                    blocks,
                    min_y,
                    max_written_y,
                    top_x as u8,
                    surf_y + dy,
                    top_z as u8,
                    log,
                );
            }
            for dx in -2i32..=2 {
                for dz in -2i32..=2 {
                    if dx.abs() + dz.abs() <= 3 {
                        put_leaf_u16(
                            blocks,
                            min_y,
                            max_written_y,
                            top_x + dx,
                            surf_y + trunk_h,
                            top_z + dz,
                            leaves,
                            pal.air,
                        );
                    }
                    if dx.abs() <= 1 && dz.abs() <= 1 {
                        put_leaf_u16(
                            blocks,
                            min_y,
                            max_written_y,
                            top_x + dx,
                            surf_y + trunk_h + 1,
                            top_z + dz,
                            leaves,
                            pal.air,
                        );
                    }
                }
            }
        }
        TreeShape::Oak | TreeShape::Birch | TreeShape::Jungle | TreeShape::Cherry => {
            let trunk_h = match shape {
                TreeShape::Birch => 5 + (next_u32(rng) % 3) as i32,
                TreeShape::Jungle => 7 + (next_u32(rng) % 4) as i32,
                TreeShape::Cherry => 5 + (next_u32(rng) % 3) as i32,
                _ => 5 + (next_u32(rng) % 3) as i32,
            };
            set_block_u16(blocks, min_y, max_written_y, lx, surf_y, lz, pal.dirt);
            for dy in 1..=trunk_h {
                set_block_u16(blocks, min_y, max_written_y, lx, surf_y + dy, lz, log);
            }
            let top = surf_y + trunk_h;
            for dy in -2..=1 {
                let rad: i32 = if dy <= -1 { 2 } else { 1 };
                for dx in -rad..=rad {
                    for dz in -rad..=rad {
                        if rad == 2
                            && dx.abs() == 2
                            && dz.abs() == 2
                            && (dy == -1 || !next_u32(rng).is_multiple_of(3))
                        {
                            continue;
                        }
                        if dy == 1 && dx.abs() == 1 && dz.abs() == 1 {
                            continue;
                        }
                        put_leaf_u16(
                            blocks,
                            min_y,
                            max_written_y,
                            cx + dx,
                            top + dy,
                            cz + dz,
                            leaves,
                            pal.air,
                        );
                    }
                }
            }
        }
    }
}

fn place_overworld_vegetation(
    rng: &mut u64,
    blocks: &mut [u16],
    min_y: i32,
    max_written_y: &mut i32,
    surface_ys: &[i32; 256],
    biome_name: &str,
    pal: &DecorationPalette,
) {
    let short = biome_name.strip_prefix("minecraft:").unwrap_or(biome_name);
    let (tree_mod, max_trees, shape, log, leaves, desert_like) =
        if short.contains("desert") || short.contains("badlands") {
            (0u32, 0u8, TreeShape::Oak, 0, 0, true)
        } else if short.contains("birch") {
            (
                16,
                4,
                TreeShape::Birch,
                pal.birch_log,
                pal.birch_leaves,
                false,
            )
        } else if short.contains("taiga") || short.contains("grove") || short.contains("snowy") {
            (
                18,
                4,
                TreeShape::Spruce,
                pal.spruce_log,
                pal.spruce_leaves,
                false,
            )
        } else if short.contains("dark_forest") {
            (
                14,
                5,
                TreeShape::DarkOak,
                pal.dark_oak_log,
                pal.dark_oak_leaves,
                false,
            )
        } else if short.contains("jungle") {
            (
                12,
                6,
                TreeShape::Jungle,
                pal.jungle_log,
                pal.jungle_leaves,
                false,
            )
        } else if short.contains("savanna") {
            (
                32,
                2,
                TreeShape::Acacia,
                pal.acacia_log,
                pal.acacia_leaves,
                false,
            )
        } else if short.contains("cherry") {
            (
                18,
                4,
                TreeShape::Cherry,
                pal.cherry_log,
                pal.cherry_leaves,
                false,
            )
        } else if short.contains("forest") || short.contains("swamp") {
            (9, 8, TreeShape::Oak, pal.oak_log, pal.oak_leaves, false)
        } else if short.contains("plains") || short.contains("meadow") {
            (85, 1, TreeShape::Oak, pal.oak_log, pal.oak_leaves, false)
        } else {
            (0, 0, TreeShape::Oak, 0, 0, false)
        };

    let mut placed_trees = 0u8;
    for lz in 2..14u8 {
        for lx in 2..14u8 {
            let surf_y = surface_ys[(lz as usize) * 16 + (lx as usize)];
            if surf_y < 62 {
                continue;
            }
            let Some(ground) = block_u16(blocks, min_y, lx, surf_y, lz) else {
                continue;
            };
            if block_u16(blocks, min_y, lx, surf_y + 1, lz) != Some(pal.air) {
                continue;
            }
            if desert_like && (ground == pal.sand || ground == pal.red_sand) {
                let roll = next_u32(rng) % 48;
                if roll == 0 && pal.cactus != 0 {
                    let h = 1 + (next_u32(rng) % 3) as i32;
                    for dy in 1..=h {
                        set_block_u16(
                            blocks,
                            min_y,
                            max_written_y,
                            lx,
                            surf_y + dy,
                            lz,
                            pal.cactus,
                        );
                    }
                } else if roll == 1 && pal.dead_bush != 0 {
                    set_block_u16(
                        blocks,
                        min_y,
                        max_written_y,
                        lx,
                        surf_y + 1,
                        lz,
                        pal.dead_bush,
                    );
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
            // Trees only grow on grassy/podzol ground (not exposed steep dirt cliffs) and need
            // vertical clearance so canopies do not get sliced off into hillsides.
            let tree_soil = ground == pal.grass_block || ground == pal.podzol;
            let clear_trunk = tree_soil
                && (1..=6).all(|dy| block_u16(blocks, min_y, lx, surf_y + dy, lz) == Some(pal.air))
                && [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)]
                    .iter()
                    .all(|&(dx, dz)| {
                        block_u16(
                            blocks,
                            min_y,
                            (i32::from(lx) + dx) as u8,
                            surf_y + 4,
                            (i32::from(lz) + dz) as u8,
                        ) == Some(pal.air)
                    });

            if placed_trees < max_trees
                && tree_mod > 0
                && log != 0
                && leaves != 0
                && clear_trunk
                && next_u32(rng).is_multiple_of(tree_mod)
            {
                // Avoid trunks spawning directly adjacent to another trunk.
                let crowded = [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)]
                    .iter()
                    .any(|&(dx, dz)| {
                        block_u16(
                            blocks,
                            min_y,
                            (i32::from(lx) + dx) as u8,
                            surf_y + 1,
                            (i32::from(lz) + dz) as u8,
                        ) == Some(log)
                    });
                if !crowded {
                    let (tree_shape, tree_log, tree_leaves) = if short.contains("forest")
                        && !short.contains("dark")
                        && !short.contains("birch")
                        && pal.birch_log != 0
                        && pal.birch_leaves != 0
                        && next_u32(rng).is_multiple_of(5)
                    {
                        (TreeShape::Birch, pal.birch_log, pal.birch_leaves)
                    } else {
                        (shape, log, leaves)
                    };
                    place_vanilla_tree(
                        rng,
                        blocks,
                        min_y,
                        max_written_y,
                        lx,
                        surf_y,
                        lz,
                        tree_shape,
                        tree_log,
                        tree_leaves,
                        pal,
                    );
                    placed_trees += 1;
                    continue;
                }
            }

            let flora_roll = next_u32(rng) % 512;
            let flora = match flora_roll {
                0..=30 => pal.short_grass,
                31..=34 if short.contains("taiga") || short.contains("jungle") => pal.fern,
                35 if short.contains("forest") && pal.leaf_litter != 0 => pal.leaf_litter,
                36..=55 if short.contains("forest") && pal.leaf_litter != 0 => pal.leaf_litter,
                56 => pal.dandelion,
                57 => pal.poppy,
                _ => 0,
            };
            if flora != 0 {
                set_block_u16(blocks, min_y, max_written_y, lx, surf_y + 1, lz, flora);
            }
        }
    }

    // Underwater river/lakebed gravel/clay disks and seagrass.
    if pal.water != 0 {
        for lz in 1..15u8 {
            for lx in 1..15u8 {
                let top_y = surface_ys[(lz as usize) * 16 + (lx as usize)];
                if top_y < 46 {
                    continue;
                }
                if block_u16(blocks, min_y, lx, top_y, lz) != Some(pal.water) {
                    continue;
                }
                // `WorldSurface` stops at the water surface; scan down through water to find the bed.
                let mut bed_y = None;
                for wy in (44..top_y).rev() {
                    let b = block_u16(blocks, min_y, lx, wy, lz);
                    if b != Some(pal.water) && b != Some(pal.air) {
                        bed_y = Some(wy);
                        break;
                    }
                }
                let Some(surf_y) = bed_y else {
                    continue;
                };
                if block_u16(blocks, min_y, lx, surf_y + 1, lz) != Some(pal.water) {
                    continue;
                }
                let roll = next_u32(rng) % 48;
                if roll == 0 && pal.clay != 0 {
                    set_block_u16(blocks, min_y, max_written_y, lx, surf_y, lz, pal.clay);
                } else if roll <= 4 && pal.gravel != 0 {
                    set_block_u16(blocks, min_y, max_written_y, lx, surf_y, lz, pal.gravel);
                } else if roll <= 10 && pal.seagrass != 0 {
                    set_block_u16(
                        blocks,
                        min_y,
                        max_written_y,
                        lx,
                        surf_y + 1,
                        lz,
                        pal.seagrass,
                    );
                }
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

    if short.contains("stronghold") {
        let wall = pal.stone_bricks.max(1);
        let frame = pal.end_portal_frame.max(wall);
        let lava = pal.lava.max(1);
        for dx in -4..=4 {
            for dz in -4..=4 {
                blocks.push(([x + dx, y - 1, z + dz], wall));
                let is_edge = dx.abs() == 4 || dz.abs() == 4;
                for dy in 0..=4 {
                    if is_edge || dy == 4 {
                        blocks.push(([x + dx, y + dy, z + dz], wall));
                    } else {
                        blocks.push(([x + dx, y + dy, z + dz], pal.air));
                    }
                }
            }
        }
        // 3x3 lava pool + 12 End Portal frames around the rim.
        for dx in -1..=1 {
            for dz in -1..=1 {
                blocks.push(([x + dx, y, z + dz], lava));
            }
        }
        for d in -1..=1 {
            blocks.push(([x + d, y + 1, z - 2], frame));
            blocks.push(([x + d, y + 1, z + 2], frame));
            blocks.push(([x - 2, y + 1, z + d], frame));
            blocks.push(([x + 2, y + 1, z + d], frame));
        }
        return blocks;
    }

    if short.contains("mineshaft") {
        let planks = pal.oak_planks.max(1);
        let log = pal.oak_log.max(planks);
        let rail = pal.rail;
        let web = pal.cobweb;
        for dz in -4..=4 {
            for dx in -1..=1 {
                blocks.push(([x + dx, y - 1, z + dz], planks));
                for dy in 0..=2 {
                    blocks.push(([x + dx, y + dy, z + dz], pal.air));
                }
            }
            if rail != 0 {
                blocks.push(([x, y, z + dz], rail));
            }
            if dz % 4 == 0 {
                for dy in 0..=2 {
                    blocks.push(([x - 1, y + dy, z + dz], log));
                    blocks.push(([x + 1, y + dy, z + dz], log));
                }
                for dx in -1..=1 {
                    blocks.push(([x + dx, y + 2, z + dz], planks));
                }
            } else if dz.abs() == 2 && web != 0 {
                blocks.push(([x + 1, y + 1, z + dz], web));
            }
        }
        return blocks;
    }

    if short.contains("swamp_hut") || short.contains("witch_hut") {
        let planks = pal.spruce_planks.max(pal.oak_planks.max(1));
        let stilt = pal.oak_log.max(planks);
        let fence = pal.spruce_fence;
        // 4 corner stilts down to foundation.
        for &(sx, sz) in &[(-2, -2), (2, -2), (-2, 2), (2, 2)] {
            for dy in -3..=0 {
                blocks.push(([x + sx, y + dy, z + sz], stilt));
            }
        }
        // 5x5 spruce floor + hollow cabin walls + overhanging roof.
        for dx in -2..=2 {
            for dz in -2..=2 {
                blocks.push(([x + dx, y, z + dz], planks));
                let is_wall = dx.abs() == 2 || dz.abs() == 2;
                let is_door = dx == 0 && dz == 2;
                for dy in 1..=3 {
                    if dy == 3 || (is_wall && !is_door) {
                        blocks.push(([x + dx, y + dy, z + dz], planks));
                    } else {
                        blocks.push(([x + dx, y + dy, z + dz], pal.air));
                    }
                }
            }
        }
        if fence != 0 {
            blocks.push(([x - 1, y + 1, z + 3], fence));
            blocks.push(([x + 1, y + 1, z + 3], fence));
        }
        if pal.cauldron != 0 {
            blocks.push(([x - 1, y + 1, z - 1], pal.cauldron));
        }
        if pal.crafting_table != 0 {
            blocks.push(([x + 1, y + 1, z - 1], pal.crafting_table));
        }
        return blocks;
    }

    if short.contains("shipwreck") {
        let hull = pal.oak_planks.max(1);
        let mast = pal.oak_log.max(hull);
        for dz in -4..=4i32 {
            let half_w: i32 = if dz.abs() >= 3 { 1 } else { 2 };
            for dx in -half_w..=half_w {
                blocks.push(([x + dx, y, z + dz], hull));
                if dx.abs() == half_w || dz.abs() == 4 {
                    blocks.push(([x + dx, y + 1, z + dz], hull));
                } else {
                    blocks.push(([x + dx, y + 1, z + dz], pal.air));
                }
            }
        }
        for dy in 1..=6 {
            blocks.push(([x, y + dy, z], mast));
        }
        if pal.chest != 0 {
            blocks.push(([x, y + 1, z - 2], pal.chest));
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

#[allow(clippy::too_many_arguments)]
fn place_overworld_biome_features(
    rng: &mut u64,
    blocks: &mut [u16],
    min_y: i32,
    max_y: i32,
    max_written_y: &mut i32,
    surface_ys: &[i32; 256],
    biome_name: &str,
    pal: &DecorationPalette,
) {
    let short = biome_name.strip_prefix("minecraft:").unwrap_or(biome_name);

    // 1. Mushroom Fields: huge red & brown mushrooms on mycelium.
    if short.contains("mushroom")
        && pal.mycelium != 0
        && pal.mushroom_stem != 0
        && (pal.red_mushroom_block != 0 || pal.brown_mushroom_block != 0)
    {
        for _ in 0..3 {
            let lx = 3 + (next_u32(rng) % 10) as u8;
            let lz = 3 + (next_u32(rng) % 10) as u8;
            let surf_y = surface_ys[usize::from(lz) * 16 + usize::from(lx)];
            if surf_y < 62 || surf_y + 8 >= max_y {
                continue;
            }
            if block_u16(blocks, min_y, lx, surf_y, lz) != Some(pal.mycelium)
                || block_u16(blocks, min_y, lx, surf_y + 1, lz) != Some(pal.air)
            {
                continue;
            }
            let stem_h = 4 + (next_u32(rng) % 3) as i32;
            for dy in 1..=stem_h {
                set_block_u16(
                    blocks,
                    min_y,
                    max_written_y,
                    lx,
                    surf_y + dy,
                    lz,
                    pal.mushroom_stem,
                );
            }
            let is_red = next_u32(rng).is_multiple_of(2) && pal.red_mushroom_block != 0;
            let cap = if is_red {
                pal.red_mushroom_block
            } else {
                pal.brown_mushroom_block.max(pal.red_mushroom_block)
            };
            if is_red {
                // Red mushroom dome cap.
                for dy in (stem_h - 1)..=(stem_h + 1) {
                    let r: i32 = if dy == stem_h + 1 { 1 } else { 2 };
                    for dx in -r..=r {
                        for dz in -r..=r {
                            if dx.abs() == r && dz.abs() == r && r == 2 {
                                continue;
                            }
                            if dy < stem_h + 1 && dx.abs() < r && dz.abs() < r {
                                continue;
                            }
                            let wx = (i32::from(lx) + dx) as u8;
                            let wz = (i32::from(lz) + dz) as u8;
                            set_block_u16(blocks, min_y, max_written_y, wx, surf_y + dy, wz, cap);
                        }
                    }
                }
            } else {
                // Brown mushroom flat 5x5 cap.
                for dx in -2i32..=2 {
                    for dz in -2i32..=2 {
                        if dx.abs() == 2 && dz.abs() == 2 {
                            continue;
                        }
                        let wx = (i32::from(lx) + dx) as u8;
                        let wz = (i32::from(lz) + dz) as u8;
                        set_block_u16(blocks, min_y, max_written_y, wx, surf_y + stem_h, wz, cap);
                    }
                }
            }
        }
        return;
    }

    // 2. Desert Well (rare 1 in 64 chunks on flat sand).
    if short.contains("desert")
        && pal.sandstone != 0
        && pal.water != 0
        && next_u32(rng).is_multiple_of(64)
    {
        let surf_y = surface_ys[8 * 16 + 8];
        if surf_y >= 63
            && surf_y + 6 < max_y
            && block_u16(blocks, min_y, 8, surf_y, 8) == Some(pal.sand)
        {
            let slab = pal.sandstone_slab.max(pal.sandstone);
            for dx in -2i32..=2 {
                for dz in -2i32..=2 {
                    let wx = (8 + dx) as u8;
                    let wz = (8 + dz) as u8;
                    set_block_u16(
                        blocks,
                        min_y,
                        max_written_y,
                        wx,
                        surf_y - 1,
                        wz,
                        pal.sandstone,
                    );
                    let is_cross_water = (dx == 0 && dz.abs() <= 1) || (dz == 0 && dx.abs() <= 1);
                    let is_corner = dx.abs() == 2 && dz.abs() == 2;
                    if is_cross_water {
                        set_block_u16(blocks, min_y, max_written_y, wx, surf_y, wz, pal.water);
                    } else if is_corner {
                        set_block_u16(blocks, min_y, max_written_y, wx, surf_y, wz, pal.sandstone);
                        for dy in 1..=3 {
                            set_block_u16(
                                blocks,
                                min_y,
                                max_written_y,
                                wx,
                                surf_y + dy,
                                wz,
                                pal.sandstone,
                            );
                        }
                    } else {
                        set_block_u16(blocks, min_y, max_written_y, wx, surf_y, wz, pal.sandstone);
                    }
                }
            }
            for dx in -1i32..=1 {
                for dz in -1i32..=1 {
                    let wx = (8 + dx) as u8;
                    let wz = (8 + dz) as u8;
                    set_block_u16(blocks, min_y, max_written_y, wx, surf_y + 3, wz, slab);
                }
            }
            set_block_u16(
                blocks,
                min_y,
                max_written_y,
                8,
                surf_y + 4,
                8,
                pal.sandstone,
            );
            return;
        }
    }

    // 3. Occasional natural surface water or lava pond (3x3 basin safely inside 5..=10).
    let pond_fluid = if (short.contains("plains")
        || short.contains("forest")
        || short.contains("swamp"))
        && pal.water != 0
        && next_u32(rng).is_multiple_of(28)
    {
        Some((pal.water, pal.dirt))
    } else if (short.contains("plains") || short.contains("desert") || short.contains("badlands"))
        && pal.lava != 0
        && next_u32(rng).is_multiple_of(72)
    {
        Some((pal.lava, pal.stone))
    } else {
        None
    };

    if let Some((fluid, rim)) = pond_fluid {
        let cx = 6 + (next_u32(rng) % 4) as i32;
        let cz = 6 + (next_u32(rng) % 4) as i32;
        let surf_y = surface_ys[(cz as usize) * 16 + (cx as usize)];
        if surf_y >= 63
            && surf_y + 3 < max_y
            && block_u16(blocks, min_y, cx as u8, surf_y + 1, cz as u8) == Some(pal.air)
        {
            for dx in -2i32..=2 {
                for dz in -2i32..=2 {
                    let wx = (cx + dx) as u8;
                    let wz = (cz + dz) as u8;
                    let inside = dx.abs() <= 1 && dz.abs() <= 1;
                    if inside {
                        set_block_u16(blocks, min_y, max_written_y, wx, surf_y - 1, wz, rim);
                        set_block_u16(blocks, min_y, max_written_y, wx, surf_y, wz, fluid);
                        set_block_u16(blocks, min_y, max_written_y, wx, surf_y + 1, wz, pal.air);
                    } else if dx.abs() + dz.abs() <= 3 && rim != 0 {
                        set_block_u16(blocks, min_y, max_written_y, wx, surf_y, wz, rim);
                    }
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn place_underground_dungeon(
    rng: &mut u64,
    blocks: &mut [u16],
    min_y: i32,
    max_written_y: &mut i32,
    pal: &DecorationPalette,
    block_entities: &mut Vec<((u8, i32, u8), DecoratedBlockEntity)>,
) {
    if pal.cobblestone == 0 || pal.spawner == 0 {
        return;
    }
    let cy = -24 + (next_u32(rng) % 56) as i32;
    let cx = 5 + (next_u32(rng) % 6) as i32;
    let cz = 5 + (next_u32(rng) % 6) as i32;
    for dx in -3..=3 {
        for dz in -3..=3 {
            let lx = (cx + dx) as u8;
            let lz = (cz + dz) as u8;
            let is_wall = dx.abs() == 3 || dz.abs() == 3;
            let floor_mat = if pal.mossy_cobblestone != 0 && next_u32(rng).is_multiple_of(3) {
                pal.mossy_cobblestone
            } else {
                pal.cobblestone
            };
            set_block_u16(blocks, min_y, max_written_y, lx, cy, lz, floor_mat);
            for dy in 1..=4 {
                let mat = if is_wall || dy == 4 {
                    pal.cobblestone
                } else {
                    pal.air
                };
                set_block_u16(blocks, min_y, max_written_y, lx, cy + dy, lz, mat);
            }
        }
    }
    set_block_u16(
        blocks,
        min_y,
        max_written_y,
        cx as u8,
        cy + 1,
        cz as u8,
        pal.spawner,
    );
    // Vanilla dungeon spawner mob distribution: 50% zombie, 25% skeleton, 25% spider.
    let entity_id = match next_u32(rng) % 4 {
        0 => "minecraft:skeleton",
        1 => "minecraft:spider",
        _ => "minecraft:zombie",
    };
    block_entities.push((
        (cx as u8, cy + 1, cz as u8),
        DecoratedBlockEntity::MobSpawner { entity_id },
    ));
    if pal.chest != 0 {
        set_block_u16(
            blocks,
            min_y,
            max_written_y,
            (cx - 2) as u8,
            cy + 1,
            cz as u8,
            pal.chest,
        );
        let seed_a = (next_u32(rng) as i64) << 32 | (next_u32(rng) as i64);
        block_entities.push((
            ((cx - 2) as u8, cy + 1, cz as u8),
            DecoratedBlockEntity::LootChest {
                loot_table: "minecraft:chests/simple_dungeon",
                seed: seed_a,
            },
        ));
        set_block_u16(
            blocks,
            min_y,
            max_written_y,
            cx as u8,
            cy + 1,
            (cz - 2) as u8,
            pal.chest,
        );
        let seed_b = (next_u32(rng) as i64) << 32 | (next_u32(rng) as i64);
        block_entities.push((
            (cx as u8, cy + 1, (cz - 2) as u8),
            DecoratedBlockEntity::LootChest {
                loot_table: "minecraft:chests/simple_dungeon",
                seed: seed_b,
            },
        ));
    }
}

#[allow(clippy::too_many_arguments)]
fn place_nether_features(
    rng: &mut u64,
    blocks: &mut [u16],
    min_y: i32,
    max_y: i32,
    max_written_y: &mut i32,
    pal: &DecorationPalette,
) {
    // Huge crimson/warped fungi on nylium floors (kept safely within 3..13 chunk bounds).
    for _ in 0..2 {
        let lx = 3 + (next_u32(rng) % 10) as u8;
        let lz = 3 + (next_u32(rng) % 10) as u8;
        for wy in (24..100).rev() {
            if block_u16(blocks, min_y, lx, wy + 1, lz) != Some(pal.air) {
                continue;
            }
            let (stem, cap) = match block_u16(blocks, min_y, lx, wy, lz) {
                Some(b) if b == pal.crimson_nylium && pal.crimson_stem != 0 => {
                    (pal.crimson_stem, pal.nether_wart_block)
                }
                Some(b) if b == pal.warped_nylium && pal.warped_stem != 0 => {
                    (pal.warped_stem, pal.warped_wart_block)
                }
                _ => continue,
            };
            if cap == 0 || wy + 8 >= max_y {
                break;
            }
            let h = 5 + (next_u32(rng) % 3) as i32;
            for dy in 1..=h {
                set_block_u16(blocks, min_y, max_written_y, lx, wy + dy, lz, stem);
            }
            for dy in (h - 2)..=h {
                let r: i32 = if dy == h { 1 } else { 2 };
                for dx in -r..=r {
                    for dz in -r..=r {
                        if dx == 0 && dz == 0 && dy < h {
                            continue;
                        }
                        if dx.abs() == r && dz.abs() == r && next_u32(rng).is_multiple_of(2) {
                            continue;
                        }
                        let wx = (lx as i32 + dx) as u8;
                        let wz = (lz as i32 + dz) as u8;
                        let mat = if pal.shroomlight != 0 && next_u32(rng).is_multiple_of(9) {
                            pal.shroomlight
                        } else {
                            cap
                        };
                        set_block_u16(blocks, min_y, max_written_y, wx, wy + dy, wz, mat);
                    }
                }
            }
            break;
        }
    }

    // Ceiling glowstone stalactites hanging from netherrack.
    if pal.glowstone != 0 && next_u32(rng).is_multiple_of(3) {
        let gx = 2 + (next_u32(rng) % 12) as u8;
        let gz = 2 + (next_u32(rng) % 12) as u8;
        for wy in (60..118).rev() {
            if block_u16(blocks, min_y, gx, wy, gz) == Some(pal.netherrack)
                && block_u16(blocks, min_y, gx, wy - 1, gz) == Some(pal.air)
            {
                for dy in 1..=3 {
                    set_block_u16(blocks, min_y, max_written_y, gx, wy - dy, gz, pal.glowstone);
                }
                for (ox, oz) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                    set_block_u16(
                        blocks,
                        min_y,
                        max_written_y,
                        (gx as i32 + ox) as u8,
                        wy - 2,
                        (gz as i32 + oz) as u8,
                        pal.glowstone,
                    );
                }
                break;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn decorate_outer_end(
    seed: u64,
    chunk_x: i32,
    chunk_z: i32,
    min_y: i32,
    height_blocks: u32,
    surface_h: &[u16; 256],
    blocks: &mut [u16],
    max_written_y: &mut i32,
    pal: &DecorationPalette,
) {
    // Keep the main dragon island (r < 64 chunks) unobstructed; decorate outer End islands only.
    let dist_sq = i64::from(chunk_x) * i64::from(chunk_x) + i64::from(chunk_z) * i64::from(chunk_z);
    if dist_sq < 4096 || pal.end_stone == 0 || pal.chorus_plant == 0 || pal.chorus_flower == 0 {
        return;
    }

    let max_y = min_y + height_blocks as i32 - 1;
    let mut rng = chunk_seed(seed, chunk_x, chunk_z, 0x454E_4443);

    for lz in 2..14u8 {
        for lx in 2..14u8 {
            if !next_u32(&mut rng).is_multiple_of(55) {
                continue;
            }
            let h = surface_h[usize::from(lz) * 16 + usize::from(lx)];
            if h == 0 {
                continue;
            }
            let surf_y = min_y + i32::from(h) - 1;
            if surf_y + 7 >= max_y
                || block_u16(blocks, min_y, lx, surf_y, lz) != Some(pal.end_stone)
            {
                continue;
            }
            let height = 3 + (next_u32(&mut rng) % 4) as i32;
            for dy in 1..=height {
                set_block_u16(
                    blocks,
                    min_y,
                    max_written_y,
                    lx,
                    surf_y + dy,
                    lz,
                    pal.chorus_plant,
                );
            }
            set_block_u16(
                blocks,
                min_y,
                max_written_y,
                lx,
                surf_y + height + 1,
                lz,
                pal.chorus_flower,
            );
        }
    }
}
