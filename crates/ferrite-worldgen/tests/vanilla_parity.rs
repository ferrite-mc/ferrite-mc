use std::path::Path;
use std::sync::Arc;

use ferrite_nbt::{Tag, binary};
use ferrite_world::{
    BlockStateRegistry, HeightmapKind, RegionFile, RuntimeRegistry, chunk_from_nbt,
};
use ferrite_worldgen::biome::{EndBiomeSource, MultiNoiseBiomeSource, Parameter, ParameterPoint};
use ferrite_worldgen::chunk_gen::TerrainPalette;
use ferrite_worldgen::vanilla_loader::{
    load_end_generator, load_multi_noise_biomes, load_nether_generator, load_overworld_biomes,
    load_overworld_generator,
};

fn test_palette(reg: &RuntimeRegistry) -> TerrainPalette {
    let id = |name: &str| reg.block_id(name, &Default::default()).unwrap_or(0) as u16;
    TerrainPalette {
        air: id("minecraft:air"),
        stone: id("minecraft:stone"),
        deepslate: id("minecraft:deepslate"),
        bedrock: id("minecraft:bedrock"),
        water: id("minecraft:water"),
        grass_block: id("minecraft:grass_block"),
        dirt: id("minecraft:dirt"),
        sand: id("minecraft:sand"),
        netherrack: id("minecraft:netherrack"),
        lava: id("minecraft:lava"),
        end_stone: id("minecraft:end_stone"),
    }
}

fn overworld_biome_source(gen_root: &Path, reg: &RuntimeRegistry) -> MultiNoiseBiomeSource {
    let report = gen_root.join("reports/biome_parameters/minecraft/overworld.json");
    load_overworld_biomes(&report, |name| reg.biome_id(name).unwrap_or(0)).unwrap_or_else(|_| {
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
            1,
        )])
    })
}

#[test]
fn vanilla_overworld_json_loads_and_generates_in_parallel() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3/generated");
    if !base.exists() {
        eprintln!("skipping: vanilla/26.3/generated absent");
        return;
    }
    let reg = Arc::new(RuntimeRegistry::load(&base).unwrap());
    let worldgen_dir = base.join("data/minecraft/worldgen");
    let seed = (-540_760_265_i64) as u64;
    let gen_arc = Arc::new(
        load_overworld_generator(
            &worldgen_dir,
            seed,
            test_palette(&reg),
            overworld_biome_source(&base, &reg),
        )
        .expect("overworld JSON should compile cleanly"),
    );

    // Single-threaded baseline for a 2x2 grid.
    let mut seq = Vec::new();
    for z in 0..2 {
        for x in 0..2 {
            let mut c = gen_arc.generate_chunk(x, z, reg.as_ref());
            c.recompute_heightmaps(reg.as_ref());
            seq.push((x, z, c));
        }
    }

    // 8-thread parallel generation across 16 chunks, verifying exact determinism.
    let mut handles = Vec::new();
    for tid in 0..8 {
        let g = Arc::clone(&gen_arc);
        let r = Arc::clone(&reg);
        handles.push(std::thread::spawn(move || {
            let cx = tid % 4;
            let cz = tid / 4;
            let mut c = g.generate_chunk(cx, cz, r.as_ref());
            c.recompute_heightmaps(r.as_ref());
            (cx, cz, c)
        }));
    }

    for h in handles {
        let (cx, cz, par_chunk) = h.join().expect("worker thread panicked");
        assert_eq!(
            par_chunk.block(0, -64, 0),
            Some(test_palette(&reg).bedrock as u32)
        );
        if let Some((_, _, seq_chunk)) = seq.iter().find(|(x, z, _)| *x == cx && *z == cz) {
            for lz in 0..16u8 {
                for lx in 0..16u8 {
                    assert_eq!(
                        par_chunk.height(HeightmapKind::WorldSurface, lx, lz),
                        seq_chunk.height(HeightmapKind::WorldSurface, lx, lz),
                        "parallel mismatch at chunk ({cx},{cz}) col ({lx},{lz})"
                    );
                }
            }
        }
    }
}

#[test]
fn compare_against_vanilla_fixture_chunks() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3");
    let region_path =
        base.join("fixture-world/world/dimensions/minecraft/overworld/region/r.0.0.mca");
    if !region_path.exists() {
        eprintln!("skipping: vanilla fixture-world absent");
        return;
    }

    let gen_root = base.join("generated");
    let reg = RuntimeRegistry::load(&gen_root).unwrap();
    let worldgen_dir = gen_root.join("data/minecraft/worldgen");
    let seed = (-540_760_265_i64) as u64;
    let generator = load_overworld_generator(
        &worldgen_dir,
        seed,
        test_palette(&reg),
        overworld_biome_source(&gen_root, &reg),
    )
    .expect("overworld JSON should compile cleanly");

    let region = RegionFile::open(&region_path, 0, 0).unwrap();
    let mut exact_biome = 0usize;
    let mut total_biome_cells = 0usize;
    let mut tested = 0usize;
    let mut total_cols = 0usize;
    let mut exact_surface = 0usize;
    let mut total_abs_err = 0i64;

    for cz in 0..4 {
        for cx in 0..4 {
            let Some(bytes) = region.read_chunk(cx, cz).unwrap() else {
                continue;
            };
            let (_, tag) = binary::read_named(&bytes).unwrap();
            let Tag::Compound(root) = &tag else {
                continue;
            };
            if root.get("Status") != Some(&Tag::String("minecraft:full".into())) {
                continue;
            }
            let mut vanilla_chunk = chunk_from_nbt(&tag, -64, 384, &reg).unwrap();
            vanilla_chunk.recompute_heightmaps(&reg);

            let mut rust_chunk = generator.generate_chunk(cx, cz, &reg);
            rust_chunk.recompute_heightmaps(&reg);

            tested += 1;
            for sy in 4..10 {
                for cell in 0..64 {
                    if vanilla_chunk.sections()[sy].biomes().get(cell)
                        == rust_chunk.sections()[sy].biomes().get(cell)
                    {
                        exact_biome += 1;
                    }
                    total_biome_cells += 1;
                }
            }
            for lz in 0..16u8 {
                for lx in 0..16u8 {
                    let vh = vanilla_chunk
                        .height(HeightmapKind::OceanFloor, lx, lz)
                        .unwrap_or(0) as i64;
                    let rh = rust_chunk
                        .height(HeightmapKind::OceanFloor, lx, lz)
                        .unwrap_or(0) as i64;
                    let diff = (vh - rh).abs();
                    total_abs_err += diff;
                    if diff == 0 {
                        exact_surface += 1;
                    }
                    total_cols += 1;
                }
            }
        }
    }

    assert!(tested > 0, "expected at least one full chunk in r.0.0.mca");
    let mae = total_abs_err as f64 / total_cols as f64;
    eprintln!(
        "vanilla fixture parity: chunks={tested}, cols={total_cols}, exact_surface={exact_surface} ({:.2}%), mean_abs_height_err={mae:.3} blocks, exact_biomes={exact_biome}/{total_biome_cells} ({:.2}%)",
        exact_surface as f64 * 100.0 / total_cols as f64,
        exact_biome as f64 * 100.0 / total_biome_cells as f64,
    );
    assert!(
        mae < 0.1,
        "expected mean_abs_height_err < 0.1 blocks against vanilla fixture, got {mae:.3}"
    );
    assert!(
        exact_surface * 100 >= total_cols * 95,
        "expected >= 95% exact surface columns, got {exact_surface}/{total_cols}"
    );
    assert!(
        exact_biome * 100 >= total_biome_cells * 90,
        "expected >= 90% exact biome cells, got {exact_biome}/{total_biome_cells}"
    );
}

#[test]
fn vanilla_nether_and_end_json_load_and_generate() {
    let gen_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3/generated");
    if !gen_root.join("reports/blocks.json").exists() {
        return;
    }
    let reg = RuntimeRegistry::load(&gen_root).expect("load registry");
    let pal = test_palette(&reg);
    let wg_dir = gen_root.join("data/minecraft/worldgen");

    let nether_biomes = load_multi_noise_biomes(
        &gen_root.join("reports/biome_parameters/minecraft/nether.json"),
        |name| reg.biome_id(name).unwrap_or(0),
    )
    .expect("load nether biomes");
    let nether = load_nether_generator(&wg_dir, 42, pal, nether_biomes).expect("load nether");
    let n_chunk = nether.generate_chunk(0, 0, &reg);
    assert_eq!(n_chunk.sections().len(), 16);
    assert_eq!(n_chunk.block(0, 0, 0), Some(pal.bedrock as u32));
    assert_eq!(n_chunk.block(0, 127, 0), Some(pal.bedrock as u32));

    let end_biomes = EndBiomeSource {
        the_end: reg.biome_id("minecraft:the_end").unwrap_or(0),
        highlands: reg.biome_id("minecraft:end_highlands").unwrap_or(0),
        midlands: reg.biome_id("minecraft:end_midlands").unwrap_or(0),
        islands: reg.biome_id("minecraft:small_end_islands").unwrap_or(0),
        barrens: reg.biome_id("minecraft:end_barrens").unwrap_or(0),
    };
    let end = load_end_generator(&wg_dir, 42, pal, end_biomes).expect("load end");
    let e_chunk = end.generate_chunk(0, 0, &reg);
    assert_eq!(e_chunk.sections().len(), 16);
    let mut end_stone_count = 0usize;
    for y in 0..128 {
        for z in 0..16 {
            for x in 0..16 {
                if e_chunk.block(x, y, z) == Some(pal.end_stone as u32) {
                    end_stone_count += 1;
                }
            }
        }
    }
    assert!(
        end_stone_count > 100,
        "expected central end island at (0,0), got {end_stone_count} end_stone blocks"
    );
}
