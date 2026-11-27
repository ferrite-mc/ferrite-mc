//! Terrain + surface throughput for [`NoiseChunkGenerator::generate_chunk_into_u16`].
//!
//! Times only terrain, caves, biomes and the surface pass (no structures, decoration,
//! lighting or section packing), and prints an FNV-1a hash of the generated block ids so a
//! change can be checked for identical output.
//!
//! `cargo run -p ferrite-worldgen --release --example terrain_bench`

use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

use ferrite_worldgen::chunk_gen::{NoiseChunkGenerator, TerrainPalette};
use ferrite_worldgen::vanilla_loader::{load_multi_noise_biomes, load_overworld_generator};

fn palette() -> TerrainPalette {
    let state = |name: &str| ferrite_data::blocks::default_state(name).unwrap_or(0);
    TerrainPalette {
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
    }
}

fn generator(base: &Path) -> NoiseChunkGenerator {
    let worldgen = base.join("data/minecraft/worldgen");
    let biomes = load_multi_noise_biomes(
        &base.join("reports/biome_parameters/minecraft/overworld.json"),
        |name| {
            let short = name.strip_prefix("minecraft:").unwrap_or(name);
            ferrite_data::registries::id("worldgen/biome", short).unwrap_or(0) as u32
        },
    )
    .expect("biome parameters");
    load_overworld_generator(&worldgen, 12345, palette(), biomes).expect("overworld JSON")
}

fn hash(blocks: &[u16]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in blocks {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

fn main() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3/generated");
    let generator = generator(&base);
    let mut blocks = Vec::new();
    let coords: Vec<(i32, i32)> = (0..64).map(|i| (i % 8, i / 8)).collect();

    // Warm the lazily loaded tables and the thread-local evaluation cache.
    for &(x, z) in coords.iter().take(8) {
        let _ = black_box(generator.generate_chunk_into_u16(x, z, &mut blocks));
    }

    let mut combined = 0xcbf2_9ce4_8422_2325u64;
    let mut best = f64::INFINITY;
    for _ in 0..5 {
        let start = Instant::now();
        for &(x, z) in &coords {
            let (chunk, sections) = generator.generate_chunk_into_u16(x, z, &mut blocks);
            combined = combined.rotate_left(7) ^ hash(&blocks[..sections * 4096]);
            black_box(&chunk);
        }
        let per_chunk = start.elapsed().as_secs_f64() * 1e6 / coords.len() as f64;
        best = best.min(per_chunk);
    }
    println!(
        "terrain+surface: {best:.1} us/chunk (best of 5, {} chunks), hash {combined:016x}",
        coords.len()
    );
}
