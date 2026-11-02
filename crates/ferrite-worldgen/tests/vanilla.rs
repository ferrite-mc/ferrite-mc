//! Acceptance tests against the complete vanilla 26.3 data generator output.

use std::fs;
use std::path::{Path, PathBuf};

use ferrite_world::{BlockInfo, BlockStateRegistry, RuntimeRegistry};
use ferrite_worldgen::{
    DensityFunction, NoiseChunkGenerator, NoiseSettings, SurfaceGenerator, WorldgenData,
};

fn generated_root() -> PathBuf {
    if let Some(root) = std::env::var_os("FERRITE_VANILLA_ROOT") {
        return PathBuf::from(root);
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3/generated")
}

fn json_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if !root.is_dir() {
        return files;
    }
    collect(root, &mut files);
    files.sort();
    files
}

fn collect(root: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root).expect("read directory") {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            collect(&path, files);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
            files.push(path);
        }
    }
}

#[test]
fn loads_every_vanilla_worldgen_file_in_strict_mode() {
    let generated = generated_root();
    assert!(
        generated.is_dir(),
        "missing {}; run `python tools/fetch_vanilla.py` first",
        generated.display()
    );
    let data = WorldgenData::load_strict(&generated)
        .unwrap_or_else(|error| panic!("strict worldgen load failed: {error}"));

    let counts = data.counts();
    println!("worldgen registry counts:");
    println!("  noise parameters:  {}", counts.noises);
    println!("  density functions: {}", counts.density_functions);
    println!("  noise settings:    {}", counts.noise_settings);
    println!("  biomes:            {}", counts.biomes);
    println!("  material rules:    {}", counts.material_rules);
    println!("  material conditions: {}", counts.material_conditions);
    println!("  dimension types:   {}", counts.dimension_types);
    println!("  dimensions (standalone files): {}", counts.dimensions);
    println!(
        "  dimension definitions inside world presets: {}",
        counts.dimension_definitions
    );
    println!("  world presets:     {}", counts.world_presets);
    println!("  multi-noise presets: {}", counts.multi_noise_presets);
    println!("  biome parameter lists: {}", counts.biome_parameters);

    let data_root = generated.join("data").join("minecraft");
    let reports_root = generated.join("reports");
    let checked = [
        (
            "worldgen/noise",
            data.noises.len(),
            json_files(&data_root.join("worldgen").join("noise")).len(),
        ),
        (
            "worldgen/density_function",
            data.density_functions.len(),
            json_files(&data_root.join("worldgen").join("density_function")).len(),
        ),
        (
            "worldgen/noise_settings",
            data.noise_settings.len(),
            json_files(&data_root.join("worldgen").join("noise_settings")).len(),
        ),
        (
            "worldgen/biome",
            data.biomes.len(),
            json_files(&data_root.join("worldgen").join("biome")).len(),
        ),
        (
            "worldgen/material_condition",
            data.material_conditions.len(),
            json_files(&data_root.join("worldgen").join("material_condition")).len(),
        ),
        (
            "worldgen/material_rule",
            data.material_rules.len(),
            json_files(&data_root.join("worldgen").join("material_rule")).len(),
        ),
        (
            "dimension_type",
            data.dimension_types.len(),
            json_files(&data_root.join("dimension_type")).len(),
        ),
        (
            "dimension",
            data.dimensions.len(),
            json_files(&data_root.join("dimension")).len(),
        ),
        (
            "worldgen/world_preset",
            data.world_presets.len(),
            json_files(&data_root.join("worldgen").join("world_preset")).len(),
        ),
        (
            "worldgen/multi_noise_biome_source_parameter_list",
            data.multi_noise_presets.len(),
            json_files(
                &data_root
                    .join("worldgen")
                    .join("multi_noise_biome_source_parameter_list"),
            )
            .len(),
        ),
        (
            "reports/biome_parameters",
            data.biome_parameters.len(),
            json_files(&reports_root.join("biome_parameters")).len(),
        ),
    ];

    for (name, loaded, on_disk) in checked {
        assert_eq!(
            loaded, on_disk,
            "{name}: loaded {loaded} of {on_disk} files"
        );
        assert!(on_disk > 0 || name == "dimension", "{name} is empty");
    }

    assert!(data.noise("continentalness").is_some());
    assert!(
        data.density_function("minecraft:overworld/final_density")
            .is_some()
    );
    assert!(data.noise_settings("minecraft:overworld").is_some());
    assert!(data.material_rule("minecraft:overworld").is_some());
    assert!(data.material_condition("minecraft:under_floor").is_some());
    assert!(data.biome("plains").is_some());
    assert!(data.dimension_type("the_nether").is_some());
    assert!(data.world_preset("normal").is_some());
    assert!(data.multi_noise_preset("overworld").is_some());
    assert!(data.biome_parameters("minecraft:overworld").is_some());
    assert_eq!(counts.dimension_definitions, 21);

    let plains = data.biome("minecraft:plains").expect("plains");
    assert!(plains.spawner_ids().contains(&"minecraft:sheep"));
    let normal = data.world_preset("normal").expect("normal preset");
    assert_eq!(normal.dimensions.len(), 3);
}

#[test]
fn resolves_every_overworld_router_density_function() {
    let data = WorldgenData::load(generated_root()).expect("lenient worldgen load");
    let settings = data
        .noise_settings("minecraft:overworld")
        .expect("overworld noise settings");
    let before = settings.noise_router.reference_count();
    assert_eq!(before, 8, "one named reference per router field");

    let resolved = settings
        .noise_router
        .resolve_references(&data)
        .expect("resolve overworld router");
    let nodes = resolved.node_count();
    println!(
        "resolved minecraft:overworld noise router: {nodes} AST nodes ({before} references inlined)"
    );
    assert_eq!(resolved.reference_count(), 0);
    assert!(
        !matches!(resolved.final_density, DensityFunction::Reference(_)),
        "final density must be inlined"
    );
    assert_eq!(nodes, 9_242);
}

#[test]
fn generates_nether_terrain_with_nether_biomes() {
    let root = generated_root();
    let data = WorldgenData::load(&root).expect("worldgen data");
    let registry = RuntimeRegistry::load(&root).expect("block registry");
    let noise =
        NoiseChunkGenerator::with_settings(&data, 0, "minecraft:nether").expect("nether generator");
    assert_eq!(noise.min_y(), 0);
    assert_eq!(noise.height(), 128);
    let surface = SurfaceGenerator::with_biome_preset(&data, &noise, 0, "minecraft:nether")
        .expect("nether surface pass");
    let mut chunk = noise
        .fill_noise_chunk(0, 0, &registry)
        .expect("nether fill");
    surface
        .apply(&mut chunk, 0, 0, &registry)
        .expect("nether surface");
    noise
        .carve_chunk(&mut chunk, 0, 0, &registry)
        .expect("nether carve");
    let min_y = chunk.min_y();
    for (section_index, section) in chunk.sections_mut().iter_mut().enumerate() {
        for qy in 0..4 {
            let y = min_y + section_index as i32 * 16 + qy * 4 + 2;
            for qz in 0..4 {
                for qx in 0..4 {
                    let name = surface.biome_at(qx * 4 + 2, y, qz * 4 + 2);
                    let id = registry.biome_id(&name.to_string()).expect("biome id");
                    section
                        .set_biome((qy * 16 + qz * 4 + qx) as usize, id)
                        .expect("set biome");
                }
            }
        }
    }

    let netherrack = registry
        .block_id("minecraft:netherrack", &Default::default())
        .expect("netherrack");
    let mut netherrack_blocks = 0;
    for y in 0..128 {
        for z in 0..16 {
            for x in 0..16 {
                if chunk.block(x, y, z) == Some(netherrack) {
                    netherrack_blocks += 1;
                }
            }
        }
    }
    assert!(
        netherrack_blocks > 1000,
        "nether column is not netherrack: {netherrack_blocks} blocks"
    );

    let mut biomes = Vec::new();
    for section in chunk.sections() {
        for index in 0..64 {
            if let Some(id) = section.biomes().get(index)
                && let Some(name) = registry.biome_name(id)
            {
                biomes.push(name.to_owned());
            }
        }
    }
    const NETHER: [&str; 5] = [
        "minecraft:nether_wastes",
        "minecraft:soul_sand_valley",
        "minecraft:crimson_forest",
        "minecraft:warped_forest",
        "minecraft:basalt_deltas",
    ];
    assert!(
        biomes.iter().all(|name| NETHER.contains(&name.as_str())),
        "overworld biome in a nether chunk: {:?}",
        biomes.iter().collect::<std::collections::BTreeSet<_>>()
    );
    // Lava below the nether sea level comes from the nether fluid, not water.
    let lava = registry
        .block_id("minecraft:lava", &Default::default())
        .expect("lava");
    assert!((0..32).any(|y| chunk.block(8, y, 8) == Some(lava)));
    assert!(!registry.flags(lava).motion_blocking);
}

#[test]
fn loads_from_the_data_minecraft_directory_too() {
    let data_root = generated_root().join("data").join("minecraft");
    let data = WorldgenData::load_strict(&data_root).expect("load data/minecraft directly");
    let counts = data.counts();
    assert_eq!(counts.noises, 64);
    assert_eq!(counts.density_functions, 55);
    assert_eq!(counts.noise_settings, 7);
    assert_eq!(counts.biomes, 67);
    assert_eq!(counts.dimension_types, 4);
    assert_eq!(counts.biome_parameters, 2);
    assert_eq!(counts.dimension_definitions, 21);
    assert!(data.world_preset("minecraft:normal").is_some());
}

#[test]
fn round_trips_real_worldgen_files() {
    let data_root = generated_root().join("data").join("minecraft");

    let density_path = data_root
        .join("worldgen")
        .join("density_function")
        .join("shift_x.json");
    let text = fs::read_to_string(&density_path).expect("read shift_x.json");
    let density: DensityFunction = serde_json::from_str(&text).expect("parse shift_x.json");
    let density_back: DensityFunction =
        serde_json::from_str(&serde_json::to_string(&density).expect("serialize density"))
            .expect("reparse density");
    assert_eq!(density_back, density);

    let settings_path = data_root
        .join("worldgen")
        .join("noise_settings")
        .join("overworld.json");
    let text = fs::read_to_string(&settings_path).expect("read overworld.json");
    let settings: NoiseSettings = serde_json::from_str(&text).expect("parse overworld.json");
    let settings_back: NoiseSettings =
        serde_json::from_str(&serde_json::to_string(&settings).expect("serialize settings"))
            .expect("reparse settings");
    assert_eq!(settings_back, settings);
}
