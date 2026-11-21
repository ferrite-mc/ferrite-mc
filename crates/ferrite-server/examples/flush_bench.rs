//! Release measurement of parallel Anvil `flush_all` and `.fworld` blob save/load.
//!
//! `cargo run -p ferrite-server --release --example flush_bench`

use std::sync::Arc;
use std::time::Instant;

use ferrite_server::game::{RustGenerator, new_anvil_source_with_compression, new_fworld_source};
use ferrite_world::{BlobCompression, Fworld, RegionCompression, RuntimeRegistry};

#[tokio::main]
async fn main() {
    let vanilla = std::path::Path::new("vanilla/26.3/generated");
    let registry = RuntimeRegistry::load(vanilla).unwrap();
    let generator = Arc::new(RustGenerator::with_vanilla_data(
        12345,
        Arc::new(RuntimeRegistry::load(vanilla).unwrap()),
        vanilla,
    ));

    // Spread 64 chunks across 4 region files ((-1,-1), (-1,0), (0,-1), (0,0)):
    // 16 chunks near (-4..0, -4..0) and (0..4, 0..4) across all 4 quadrants.
    let mut coords = Vec::new();
    for rz in [-1i32, 0] {
        for rx in [-1i32, 0] {
            for dz in 0..4 {
                for dx in 0..4 {
                    coords.push((rx * 32 + dx, rz * 32 + dz));
                }
            }
        }
    }

    // 1) Parallel Anvil flush_all across 4 regions (zlib)
    let anvil_dir = tempfile::tempdir().unwrap();
    let anvil = new_anvil_source_with_compression(
        anvil_dir.path().to_path_buf(),
        RuntimeRegistry::load(vanilla).unwrap(),
        Some(Arc::clone(&generator) as _),
        RegionCompression::Zlib,
    );
    for &(cx, cz) in &coords {
        while anvil.get(cx, cz, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        anvil.set_block(cx * 16, 80, cz * 16, 1).unwrap();
    }
    let t_anvil = Instant::now();
    anvil.flush_all().await.unwrap();
    let anvil_ms = t_anvil.elapsed().as_secs_f64() * 1e3;

    // 2) Single-region Anvil flush_all baseline (all 64 chunks inside r.0.0.mca -> 1 region writer)
    let single_dir = tempfile::tempdir().unwrap();
    let single = new_anvil_source_with_compression(
        single_dir.path().to_path_buf(),
        RuntimeRegistry::load(vanilla).unwrap(),
        Some(Arc::clone(&generator) as _),
        RegionCompression::Zlib,
    );
    for dz in 0..8 {
        for dx in 0..8 {
            while single.get(dx, dz, 0).is_none() {
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
            single.set_block(dx * 16, 80, dz * 16, 1).unwrap();
        }
    }
    let t_single = Instant::now();
    single.flush_all().await.unwrap();
    let single_ms = t_single.elapsed().as_secs_f64() * 1e3;

    // 3) .fworld (`FWRD` v2) atomic LZ4 save & cold decode
    let fworld_dir = tempfile::tempdir().unwrap();
    let fworld_path = fworld_dir.path().join("bench.fworld");
    let fworld = new_fworld_source(
        fworld_path.clone(),
        registry,
        Some(generator),
        BlobCompression::Lz4,
        [0, 64, 0],
    )
    .unwrap();
    for &(cx, cz) in &coords {
        while fworld.get(cx, cz, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        fworld.set_block(cx * 16, 80, cz * 16, 1).unwrap();
    }
    let t_fworld_save = Instant::now();
    fworld.flush_all().await.unwrap();
    let fworld_save_ms = t_fworld_save.elapsed().as_secs_f64() * 1e3;
    let blob_bytes = std::fs::metadata(&fworld_path).unwrap().len();
    let t_fworld_load = Instant::now();
    let decoded = Fworld::read_file(&fworld_path).unwrap();
    let fworld_load_ms = t_fworld_load.elapsed().as_secs_f64() * 1e3;
    assert_eq!(decoded.chunks.len(), 64);

    println!(
        "flush_bench (64 dirty Rust-worldgen chunks):\n\
         - Anvil flush_all (1 region file / serial): {single_ms:.2} ms\n\
         - Anvil flush_all (4 region files / parallel): {anvil_ms:.2} ms ({:.2}x)\n\
         - .fworld LZ4 atomic save: {fworld_save_ms:.2} ms ({blob_bytes} bytes)\n\
         - .fworld LZ4 cold load:   {fworld_load_ms:.2} ms",
        single_ms / anvil_ms.max(0.001)
    );
}
