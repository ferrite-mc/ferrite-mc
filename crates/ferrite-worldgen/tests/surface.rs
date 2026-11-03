use ferrite_world::{BlockStateRegistry, Chunk, HeightmapKind, RuntimeRegistry};
use ferrite_worldgen::{
    DensityFunction, MaterialRule, MaterialRuleNode, NoiseChunkGenerator, ResourceId,
    SurfaceGenerator, WorldgenData,
};
use std::path::PathBuf;

fn generated() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3/generated")
}

/// Compares every block, heightmap, section counter and sampled biome of two surface passes.
///
/// A `SurfaceGenerator` memoizes biome selections and positional random factories between
/// chunks. A freshly constructed generator is the reference for those caches: both must
/// produce the same chunk for the same seed and coordinates.
fn assert_same_chunk(
    reused: &Chunk,
    fresh: &Chunk,
    reused_surface: &SurfaceGenerator<'_>,
    fresh_surface: &SurfaceGenerator<'_>,
    cx: i32,
    cz: i32,
) {
    assert_eq!(reused.min_y(), fresh.min_y(), "chunk ({cx}, {cz}) min_y");
    assert_eq!(
        reused.height_blocks(),
        fresh.height_blocks(),
        "chunk ({cx}, {cz}) height"
    );
    let min_y = reused.min_y();
    for z in 0_u8..16 {
        for x in 0_u8..16 {
            for y in min_y..min_y + reused.height_blocks() as i32 {
                let reused_block = reused.block(x, y, z);
                let fresh_block = fresh.block(x, y, z);
                assert_eq!(
                    reused_block, fresh_block,
                    "chunk ({cx}, {cz}) block ({x}, {y}, {z})"
                );
            }
        }
    }
    for kind in HeightmapKind::ALL {
        assert_eq!(
            reused.heights(kind),
            fresh.heights(kind),
            "chunk ({cx}, {cz}) heightmap {kind:?}"
        );
    }
    assert_eq!(
        reused.block_entities, fresh.block_entities,
        "chunk ({cx}, {cz}) block entities"
    );
    assert_eq!(
        reused.sections().len(),
        fresh.sections().len(),
        "chunk ({cx}, {cz}) section count"
    );
    for (index, (reused_section, fresh_section)) in
        reused.sections().iter().zip(fresh.sections()).enumerate()
    {
        assert_eq!(
            reused_section.block_count(),
            fresh_section.block_count(),
            "chunk ({cx}, {cz}) section {index} block count"
        );
        assert_eq!(
            reused_section.fluid_count(),
            fresh_section.fluid_count(),
            "chunk ({cx}, {cz}) section {index} fluid count"
        );
        assert_eq!(
            reused_section.randomly_ticking_block_count(),
            fresh_section.randomly_ticking_block_count(),
            "chunk ({cx}, {cz}) section {index} random block count"
        );
        assert_eq!(
            reused_section.randomly_ticking_fluid_count(),
            fresh_section.randomly_ticking_fluid_count(),
            "chunk ({cx}, {cz}) section {index} random fluid count"
        );
    }
    for qy in 0..reused.height_blocks() / 4 {
        for qz in 0_i32..4 {
            for qx in 0_i32..4 {
                let x = cx * 16 + qx * 4 + 2;
                let y = min_y + qy as i32 * 4 + 2;
                let z = cz * 16 + qz * 4 + 2;
                assert_eq!(
                    reused_surface.biome_at(x, y, z),
                    fresh_surface.biome_at(x, y, z),
                    "chunk ({cx}, {cz}) biome at ({x}, {y}, {z})"
                );
            }
        }
    }
}

/// Runs `chunks` through one reused generator and one freshly constructed generator per
/// chunk, asserting that generator reuse cannot change the surface output.
fn assert_reuse_matches_fresh(
    data: &WorldgenData,
    registry: &RuntimeRegistry,
    seed: i64,
    chunks: &[(i32, i32)],
) {
    let noise = NoiseChunkGenerator::new(data, seed).expect("noise generator");
    let reused_surface = SurfaceGenerator::new(data, &noise, seed).expect("reused surface");
    for &(cx, cz) in chunks {
        let fresh_surface = SurfaceGenerator::new(data, &noise, seed).expect("fresh surface");

        let mut reused = noise
            .fill_noise_chunk(cx, cz, registry)
            .expect("reused noise fill");
        reused_surface
            .apply(&mut reused, cx, cz, registry)
            .expect("reused surface apply");

        let mut fresh = noise
            .fill_noise_chunk(cx, cz, registry)
            .expect("fresh noise fill");
        fresh_surface
            .apply(&mut fresh, cx, cz, registry)
            .expect("fresh surface apply");

        assert_same_chunk(&reused, &fresh, &reused_surface, &fresh_surface, cx, cz);
    }
}

/// Chunk locations that stress the generator's per-chunk caches: a repeated chunk, negative
/// coordinates and both sides of the 32-chunk region boundary.
const REUSE_CHUNKS: &[(i32, i32)] = &[
    (0, 0),
    (0, 0),
    (-1, -1),
    (-1, 0),
    (0, -1),
    (31, 31),
    (32, 32),
    (-32, -32),
    (-33, -33),
    (31, -32),
    (-32, 31),
];

#[test]
fn block_rule_changes_filled_stone() {
    let path = generated();
    let mut data = WorldgenData::load(&path).unwrap();
    let registry = RuntimeRegistry::load(&path).unwrap();
    let settings = data
        .noise_settings
        .get_mut(&ResourceId::parse("overworld").unwrap())
        .unwrap();
    settings.noise.min_y = 0;
    settings.noise.height = 16;
    settings.aquifers = None;
    settings.noise_router.final_density = DensityFunction::Constant(1.0);
    let rule = settings.material_rule.clone();
    data.material_rules.insert(
        rule,
        MaterialRule::Node(MaterialRuleNode::Block {
            result_state: ResourceId::parse("dirt").unwrap(),
        }),
    );
    let noise = NoiseChunkGenerator::new(&data, 42).unwrap();
    let surface = SurfaceGenerator::new(&data, &noise, 42).unwrap();
    let mut chunk = noise.fill_noise_chunk(0, 0, &registry).unwrap();
    surface.apply(&mut chunk, 0, 0, &registry).unwrap();
    let dirt = registry
        .block_id("minecraft:dirt", &Default::default())
        .unwrap();
    assert_eq!(chunk.block(0, 15, 0), Some(dirt));
    assert_eq!(chunk.block(15, 0, 15), Some(dirt));
}

#[test]
fn y_above_rule_only_changes_stone_at_or_above_anchor() {
    use ferrite_worldgen::{MaterialCondition, MaterialConditionNode, VerticalAnchor};
    let path = generated();
    let mut data = WorldgenData::load(&path).unwrap();
    let registry = RuntimeRegistry::load(&path).unwrap();
    let settings = data
        .noise_settings
        .get_mut(&ResourceId::parse("overworld").unwrap())
        .unwrap();
    settings.noise.min_y = 0;
    settings.noise.height = 16;
    settings.aquifers = None;
    settings.noise_router.final_density = DensityFunction::Constant(1.0);
    let root = settings.material_rule.clone();
    data.material_rules.insert(
        root,
        MaterialRule::Node(MaterialRuleNode::Condition {
            if_true: MaterialCondition::Node(MaterialConditionNode::YAbove {
                anchor: VerticalAnchor::Absolute { absolute: 8 },
                surface_depth_multiplier: 0,
                add_stone_depth: false,
            }),
            then_run: Box::new(MaterialRule::Node(MaterialRuleNode::Block {
                result_state: ResourceId::parse("dirt").unwrap(),
            })),
        }),
    );
    let noise = NoiseChunkGenerator::new(&data, 42).unwrap();
    let surface = SurfaceGenerator::new(&data, &noise, 42).unwrap();
    let mut chunk = noise.fill_noise_chunk(0, 0, &registry).unwrap();
    surface.apply(&mut chunk, 0, 0, &registry).unwrap();
    let dirt = registry
        .block_id("minecraft:dirt", &Default::default())
        .unwrap();
    let stone = registry
        .block_id("minecraft:stone", &Default::default())
        .unwrap();
    assert_eq!(chunk.block(0, 8, 0), Some(dirt));
    assert_eq!(chunk.block(0, 7, 0), Some(stone));
}

#[test]
fn floor_stone_depth_changes_only_the_top_layer() {
    use ferrite_worldgen::{MaterialCondition, MaterialConditionNode, SurfaceType};
    let path = generated();
    let mut data = WorldgenData::load(&path).unwrap();
    let registry = RuntimeRegistry::load(&path).unwrap();
    let settings = data
        .noise_settings
        .get_mut(&ResourceId::parse("overworld").unwrap())
        .unwrap();
    settings.noise.min_y = 0;
    settings.noise.height = 16;
    settings.aquifers = None;
    settings.noise_router.final_density = DensityFunction::Constant(1.0);
    let root = settings.material_rule.clone();
    data.material_rules.insert(
        root,
        MaterialRule::Node(MaterialRuleNode::Condition {
            if_true: MaterialCondition::Node(MaterialConditionNode::StoneDepth {
                offset: 0,
                surface_type: SurfaceType::Floor,
                secondary_depth_range: 0,
                add_surface_depth: false,
            }),
            then_run: Box::new(MaterialRule::Node(MaterialRuleNode::Block {
                result_state: ResourceId::parse("dirt").unwrap(),
            })),
        }),
    );
    let noise = NoiseChunkGenerator::new(&data, 42).unwrap();
    let surface = SurfaceGenerator::new(&data, &noise, 42).unwrap();
    let mut chunk = noise.fill_noise_chunk(0, 0, &registry).unwrap();
    surface.apply(&mut chunk, 0, 0, &registry).unwrap();
    let dirt = registry
        .block_id("minecraft:dirt", &Default::default())
        .unwrap();
    let stone = registry
        .block_id("minecraft:stone", &Default::default())
        .unwrap();
    assert_eq!(chunk.block(0, 15, 0), Some(dirt));
    assert_eq!(chunk.block(0, 14, 0), Some(stone));
}

/// The overworld rule tree starts with `minecraft:bedrock_floor`; the vertical gradient must
/// keep bedrock at the floor and leave stone at and above `above_bottom: 5`.
#[test]
fn bedrock_floor_rule_is_applied_at_the_floor() {
    let path = generated();
    let mut data = WorldgenData::load(&path).unwrap();
    let registry = RuntimeRegistry::load(&path).unwrap();
    let settings = data
        .noise_settings
        .get_mut(&ResourceId::parse("overworld").unwrap())
        .unwrap();
    settings.noise.min_y = 0;
    settings.noise.height = 16;
    settings.aquifers = None;
    settings.noise_router.final_density = DensityFunction::Constant(1.0);
    settings.material_rule = ResourceId::parse("bedrock_floor").unwrap();
    let noise = NoiseChunkGenerator::new(&data, 42).unwrap();
    let surface = SurfaceGenerator::new(&data, &noise, 42).unwrap();
    let mut chunk = noise.fill_noise_chunk(0, 0, &registry).unwrap();
    surface.apply(&mut chunk, 0, 0, &registry).unwrap();
    let bedrock = registry
        .block_id("minecraft:bedrock", &Default::default())
        .unwrap();
    let stone = registry
        .block_id("minecraft:stone", &Default::default())
        .unwrap();
    assert_eq!(chunk.block(0, 0, 0), Some(bedrock));
    for y in 5..16 {
        assert_eq!(chunk.block(0, y, 0), Some(stone), "y={y}");
    }
}

#[test]
fn fixture_top_blocks_match_vanilla_surface() {
    use ferrite_nbt::{
        Tag,
        binary::{self, Compression},
    };
    use ferrite_world::{BlockInfo, RegionFile, chunk_from_nbt};
    let base = generated().parent().unwrap().to_owned();
    let seed_path = base.join("fixture-world/world/data/minecraft/world_gen_settings.dat");
    let (_, tag) =
        binary::read_compressed(&std::fs::read(seed_path).unwrap(), Compression::Gzip).unwrap();
    let Tag::Compound(root) = tag else {
        panic!("worldgen root")
    };
    let Some(Tag::Compound(settings)) = root.get("data") else {
        panic!("worldgen data")
    };
    let Some(Tag::Long(seed)) = settings.get("seed") else {
        panic!("seed")
    };
    let data = WorldgenData::load(generated()).unwrap();
    let registry = RuntimeRegistry::load(generated()).unwrap();
    let noise = NoiseChunkGenerator::new(&data, *seed).unwrap();
    let surface = SurfaceGenerator::new(&data, &noise, *seed).unwrap();
    let region = RegionFile::open(
        base.join("fixture-world/world/dimensions/minecraft/overworld/region/r.0.0.mca"),
        0,
        0,
    )
    .unwrap();
    let mut total = 0;
    let mut matched = 0;
    let mut clear_total = 0;
    let mut clear_matched = 0;
    let mut terrain_total = 0;
    let mut terrain_matched = 0;
    let mut mismatches =
        std::collections::BTreeMap::<(String, String, String, String), usize>::new();
    for (cx, cz) in [0, 1, 8, 15]
        .into_iter()
        .flat_map(|z| [0, 1, 8, 15].into_iter().map(move |x| (x, z)))
    {
        let bytes = region.read_chunk(cx, cz).unwrap().unwrap();
        let (_, tag) = binary::read_named(&bytes).unwrap();
        let Tag::Compound(root) = &tag else {
            panic!("chunk")
        };
        let Some(Tag::Compound(structures)) = root.get("structures") else {
            panic!("structures")
        };
        let Some(Tag::Compound(refs)) = structures.get("References") else {
            panic!("references")
        };
        let clear = refs.is_empty();
        let expected = chunk_from_nbt(&tag, -64, 384, &registry).unwrap();
        let mut actual = noise.fill_noise_chunk(cx, cz, &registry).unwrap();
        surface.apply(&mut actual, cx, cz, &registry).unwrap();
        noise.carve_chunk(&mut actual, cx, cz, &registry).unwrap();
        let top = |chunk: &ferrite_world::Chunk, x: u8, z: u8| -> (String, i32) {
            for y in (-64..320).rev() {
                let id = chunk.block(x, y, z).unwrap();
                let flags = registry.flags(id);
                if !flags.motion_blocking || flags.leaves || flags.fluid {
                    continue;
                }
                let name = &registry.block_state(id).unwrap().name;
                if name.ends_with("_log")
                    || name.ends_with("_wood")
                    || name.ends_with("_stem")
                    || name.ends_with("_hyphae")
                    || name.contains("mushroom")
                    || name.contains("azalea")
                    || name == "minecraft:cactus"
                    || name == "minecraft:bamboo"
                {
                    continue;
                }
                return (name.clone(), y);
            }
            ("minecraft:air".to_owned(), -64)
        };
        let before = matched;
        for z in 0..16 {
            for x in 0..16 {
                let (a, _) = top(&actual, x, z);
                let (b, y) = top(&expected, x, z);
                if a != b {
                    let sec = ((y + 64) / 16) as usize;
                    let index = (((y + 64) % 16) / 4 * 16 + i32::from(z / 4) * 4 + i32::from(x / 4))
                        as usize;
                    let id = expected.sections()[sec].biomes().get(index).unwrap();
                    let old_biome = registry.biome_name(id).unwrap_or("unknown").to_owned();
                    let new_biome = surface
                        .biome_at(cx * 16 + i32::from(x), y, cz * 16 + i32::from(z))
                        .to_string();
                    *mismatches
                        .entry((a.clone(), b.clone(), new_biome, old_biome))
                        .or_default() += 1;
                }
                matched += usize::from(a == b);
                total += 1;
                if clear {
                    clear_matched += usize::from(a == b);
                    clear_total += 1;
                    // Frozen-ocean ice/blue-ice features and recognizable structure
                    // materials can cross into a chunk without a structure reference.
                    // Determine exclusions solely from the fixture, before comparing.
                    let feature = matches!(
                        b.as_str(),
                        "minecraft:ice"
                            | "minecraft:blue_ice"
                            | "minecraft:netherrack"
                            | "minecraft:magma_block"
                            | "minecraft:obsidian"
                            | "minecraft:crying_obsidian"
                            | "minecraft:stone_bricks"
                            | "minecraft:mossy_stone_bricks"
                            | "minecraft:chiseled_stone_bricks"
                            | "minecraft:mossy_cobblestone"
                            | "minecraft:pumpkin"
                    );
                    if !feature {
                        terrain_total += 1;
                        terrain_matched += usize::from(a == b);
                    }
                }
            }
        }
        eprintln!(
            "surface chunk {cx},{cz}: {}/256; structure refs {}",
            matched - before,
            refs.len()
        );
    }
    let rate = matched as f64 / total as f64;
    let clear_rate = clear_matched as f64 / clear_total as f64;
    println!(
        "surface top blocks: all {matched}/{total} = {:.5}%; no structure references {clear_matched}/{clear_total} = {:.5}%",
        rate * 100.0,
        clear_rate * 100.0
    );
    eprintln!("surface mismatches: {mismatches:?}");
    let terrain_rate = terrain_matched as f64 / terrain_total as f64;
    println!(
        "surface top blocks excluding referenced-structure chunks and recognizable feature blocks: {terrain_matched}/{terrain_total} = {:.5}%",
        terrain_rate * 100.0
    );
    assert!(
        terrain_total > 0 && terrain_rate >= 0.98,
        "surface match below 98%: {terrain_rate}"
    );
}

/// Reusing one `SurfaceGenerator` across chunks must be indistinguishable from constructing a
/// new generator for every chunk, including when the same chunk is visited twice and when the
/// chunk coordinates are negative or on a region boundary. The height is reduced to 16 so the
/// test can cover many locations; the full-height test below covers the real column.
#[test]
fn reused_surface_generator_matches_fresh_across_repeated_negative_and_boundary_chunks() {
    let path = generated();
    let mut data = WorldgenData::load(&path).expect("worldgen data");
    let settings = data
        .noise_settings
        .get_mut(&ResourceId::parse("overworld").unwrap())
        .expect("overworld noise settings");
    settings.noise.min_y = 0;
    settings.noise.height = 16;
    settings.aquifers = None;
    settings.noise_router.final_density = DensityFunction::Constant(1.0);
    let registry = RuntimeRegistry::load(&path).expect("block registry");
    assert_reuse_matches_fresh(&data, &registry, 42, REUSE_CHUNKS);
}

/// The same reuse comparison against the real 384-block overworld column, at a negative chunk
/// and at a region boundary. This exercises the biome cache's full vertical extent and the
/// heightmap lookups the surface pass performs at every column.
#[test]
fn reused_surface_generator_matches_fresh_on_full_height_negative_and_region_boundary_chunks() {
    let path = generated();
    let data = WorldgenData::load(&path).expect("worldgen data");
    let registry = RuntimeRegistry::load(&path).expect("block registry");
    assert_reuse_matches_fresh(&data, &registry, 42, &[(0, 0), (-1, -1), (31, -32)]);
}
