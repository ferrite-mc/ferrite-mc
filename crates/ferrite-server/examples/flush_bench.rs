//! Release measurement of parallel Anvil `flush_all`, `.fworld` (`FWRD` v3) compression modes,
//! incremental cold-store flushes, and pure-Rust multi-core worldgen scaling across dimensions.
//!
//! `cargo run -p ferrite-server --release --example flush_bench`

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use ferrite_server::game::{
    ChunkGenerator, Dimension, RustGenerator, new_anvil_source_with_compression, new_fworld_source,
};
use ferrite_world::{BlobCompression, Fworld, FworldColdStore, RegionCompression, RuntimeRegistry};

#[tokio::main]
async fn main() {
    let vanilla = std::path::Path::new("vanilla/26.3/generated");
    let reg_arc = Arc::new(RuntimeRegistry::load(vanilla).unwrap());
    let generator = Arc::new(RustGenerator::with_vanilla_data(
        12345,
        Arc::clone(&reg_arc),
        vanilla,
    ));

    // Spread 64 chunks across 4 region files ((-1,-1), (-1,0), (0,-1), (0,0)).
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

    // 1) Anvil flush_all matrix (1 region serial vs 4 regions parallel, Zlib vs Lz4)
    let mut anvil_results = Vec::new();
    for (label, regions_4, comp) in [
        (
            "Anvil 1-region serial (Zlib)",
            false,
            RegionCompression::Zlib,
        ),
        ("Anvil 1-region serial (LZ4)", false, RegionCompression::Lz4),
        (
            "Anvil 4-region parallel (Zlib)",
            true,
            RegionCompression::Zlib,
        ),
        (
            "Anvil 4-region parallel (LZ4)",
            true,
            RegionCompression::Lz4,
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let src = new_anvil_source_with_compression(
            dir.path().to_path_buf(),
            RuntimeRegistry::load(vanilla).unwrap(),
            Some(Arc::clone(&generator) as _),
            comp,
        );
        if regions_4 {
            for &(cx, cz) in &coords {
                while src.get(cx, cz, 0).is_none() {
                    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                }
                src.set_block(cx * 16, 80, cz * 16, 1).unwrap();
            }
        } else {
            for dz in 0..8 {
                for dx in 0..8 {
                    while src.get(dx, dz, 0).is_none() {
                        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                    }
                    src.set_block(dx * 16, 80, dz * 16, 1).unwrap();
                }
            }
        }
        let t0 = Instant::now();
        src.flush_all().await.unwrap();
        let ms = t0.elapsed().as_secs_f64() * 1e3;
        anvil_results.push((label, ms));
    }
    let serial_zlib_ms = anvil_results[0].1;

    println!("=== 1. Storage & Compression Matrix (64 dirty Rust-worldgen chunks) ===");
    for &(label, ms) in &anvil_results {
        println!(
            "- {label:<34}: {ms:>6.2} ms ({:.2}x vs serial Zlib)",
            serial_zlib_ms / ms.max(0.001)
        );
    }

    // 2) .fworld (FWRD v3) across compression modes + incremental flush (4/64 dirty chunks)
    for (mode_name, comp) in [
        ("LZ4", BlobCompression::Lz4),
        ("Zlib", BlobCompression::Zlib),
        ("Gzip", BlobCompression::Gzip),
        ("None", BlobCompression::None),
    ] {
        let fworld_dir = tempfile::tempdir().unwrap();
        let fworld_path = fworld_dir.path().join("bench.fworld");
        let fworld = new_fworld_source(
            fworld_path.clone(),
            RuntimeRegistry::load(vanilla).unwrap(),
            Some(Arc::clone(&generator) as _),
            comp,
            [0, 64, 0],
        )
        .unwrap();
        for &(cx, cz) in &coords {
            while fworld.get(cx, cz, 0).is_none() {
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
            fworld.set_block(cx * 16, 80, cz * 16, 1).unwrap();
        }
        let t_save = Instant::now();
        fworld.flush_all().await.unwrap();
        let save_ms = t_save.elapsed().as_secs_f64() * 1e3;
        let blob_bytes = std::fs::metadata(&fworld_path).unwrap().len();

        // Incremental flush: mutate only 4 chunks out of 64 (60 chunks stay pre-compressed in FworldColdStore)
        for &(cx, cz) in &coords[..4] {
            fworld.set_block(cx * 16 + 1, 80, cz * 16 + 1, 1).unwrap();
        }
        let t_incr = Instant::now();
        fworld.flush_all().await.unwrap();
        let incr_ms = t_incr.elapsed().as_secs_f64() * 1e3;

        let t_cold = Instant::now();
        let cold = FworldColdStore::read_file(&fworld_path, comp).unwrap();
        let cold_ms = t_cold.elapsed().as_secs_f64() * 1e3;
        assert_eq!(cold.chunks.len(), 64);

        let t_full = Instant::now();
        let decoded = Fworld::read_file(&fworld_path).unwrap();
        let full_ms = t_full.elapsed().as_secs_f64() * 1e3;
        assert_eq!(decoded.chunks.len(), 64);

        println!(
            "- .fworld FWRD v3 ({mode_name:<4}) full save    : {save_ms:>6.2} ms ({:.2}x vs serial Anvil) | incr (4/64 dirty): {incr_ms:>5.2} ms | cold load: {cold_ms:>4.2} ms | full decode: {full_ms:>4.2} ms | size: {blob_bytes} B",
            serial_zlib_ms / save_ms.max(0.001)
        );
    }

    // 3) Pure-Rust Worldgen Multi-Core Thread Scaling (48 Overworld chunks)
    println!("\n=== 2. Pure-Rust Worldgen Multi-Core Scaling (48 Overworld chunks) ===");
    let wg_coords: Vec<(i32, i32)> = (0..48).map(|i| (i % 8, i / 8)).collect();
    let mut base_chunks_per_s = 0.0;
    for threads in [1usize, 2, 4, 8, 12] {
        let next = AtomicUsize::new(0);
        let t0 = Instant::now();
        std::thread::scope(|s| {
            for _ in 0..threads {
                let gen_ref = &generator;
                let coords_ref = &wg_coords;
                let next_ref = &next;
                s.spawn(move || {
                    loop {
                        let idx = next_ref.fetch_add(1, Ordering::Relaxed);
                        if idx >= coords_ref.len() {
                            break;
                        }
                        let (cx, cz) = coords_ref[idx];
                        std::hint::black_box(
                            gen_ref.generate(Dimension::Overworld, cx, cz).unwrap(),
                        );
                    }
                });
            }
        });
        let elapsed = t0.elapsed().as_secs_f64();
        let cps = wg_coords.len() as f64 / elapsed;
        if threads == 1 {
            base_chunks_per_s = cps;
        }
        println!(
            "- {threads:>2} thread(s): {cps:>7.1} chunks/s ({:>6.2} ms wall, {:.2}x speedup)",
            elapsed * 1e3,
            cps / base_chunks_per_s.max(0.001)
        );
    }

    // 4) Pure-Rust Worldgen Dimension Throughput (12 threads, 48 chunks each)
    println!("\n=== 3. Pure-Rust Worldgen Dimension Throughput (12 threads, 48 chunks) ===");
    for (dim_name, dim) in [
        ("Overworld (-64..320, 384h)", Dimension::Overworld),
        ("Nether    (  0..256, 128g)", Dimension::Nether),
        ("The End   (  0..256, 128g)", Dimension::End),
    ] {
        let next = AtomicUsize::new(0);
        let t0 = Instant::now();
        std::thread::scope(|s| {
            for _ in 0..12 {
                let gen_ref = &generator;
                let coords_ref = &wg_coords;
                let next_ref = &next;
                s.spawn(move || {
                    loop {
                        let idx = next_ref.fetch_add(1, Ordering::Relaxed);
                        if idx >= coords_ref.len() {
                            break;
                        }
                        let (cx, cz) = coords_ref[idx];
                        std::hint::black_box(gen_ref.generate(dim, cx, cz).unwrap());
                    }
                });
            }
        });
        let elapsed = t0.elapsed().as_secs_f64();
        let cps = wg_coords.len() as f64 / elapsed;
        println!(
            "- {dim_name}: {cps:>7.1} chunks/s ({:>6.2} ms total, {:.2} ms/chunk effective)",
            elapsed * 1e3,
            (elapsed * 1e3) / wg_coords.len() as f64
        );
    }

    // 5) Core Engine Microbenchmarks (SharedFrame fan-out, DDA raycast_shapes, recompute_heightmaps)
    println!("\n=== 4. Core Engine Hotpath Microbenchmarks ===");
    let sample_chunk = generator.generate(Dimension::Overworld, 0, 0).unwrap();
    let sample_pkt: Arc<[u8]> =
        ferrite_world::encode_chunk_packet(0, 0, &sample_chunk.chunk, &sample_chunk.light)
            .unwrap()
            .into();
    let enc = ferrite_protocol::FrameEncoder::new_compressed(Some(256), 6, true);

    // 200-recipient chunk fan-out: per-socket compression vs SharedFrame
    let t_per_sock = Instant::now();
    let mut sink = Vec::with_capacity(sample_pkt.len() + 64);
    for _ in 0..200 {
        sink.clear();
        enc.encode(&sample_pkt, &mut sink);
        std::hint::black_box(&sink);
    }
    let per_sock_ms = t_per_sock.elapsed().as_secs_f64() * 1e3;

    let t_shared = Instant::now();
    let sf = ferrite_net::game::shared_frame_arc(&enc, Arc::clone(&sample_pkt));
    for _ in 0..200 {
        sink.clear();
        sink.extend_from_slice(&sf.framed);
        std::hint::black_box(&sink);
    }
    let shared_ms = t_shared.elapsed().as_secs_f64() * 1e3;
    println!(
        "- 200-player chunk fan-out ({} B raw): per-socket zlib {per_sock_ms:.2} ms -> SharedFrame {shared_ms:.2} ms ({:.1}x speedup)",
        sample_pkt.len(),
        per_sock_ms / shared_ms.max(0.0001)
    );

    // 1,000 full-chunk heightmap recomputations (6 heightmaps x 256 columns)
    let mut hm_chunk = sample_chunk.chunk.clone();
    let t_hm = Instant::now();
    for _ in 0..1_000 {
        hm_chunk.recompute_heightmaps(reg_arc.as_ref());
        std::hint::black_box(&hm_chunk);
    }
    let hm_ms = t_hm.elapsed().as_secs_f64() * 1e3;
    println!(
        "- 1,000x Chunk::recompute_heightmaps (6 kinds): {hm_ms:.2} ms total ({:.1} us/chunk)",
        (hm_ms * 1e3) / 1_000.0
    );

    // 50,000x Lazy DDA raycast_shapes across a 6-block ray
    let full_box = [ferrite_math::AABB::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0)];
    let origin = ferrite_math::Vec3::new(0.5, 70.5, 0.5);
    let dir = ferrite_math::Vec3::new(0.57735, -0.57735, 0.57735);
    let t_ray = Instant::now();
    for i in 0..50_000 {
        let hit = ferrite_math::raycast_shapes(origin, dir, 6.0, |p| -> &[ferrite_math::AABB] {
            if (p.x + p.y + p.z + (i & 3)) == 205 {
                &full_box
            } else {
                &[]
            }
        });
        std::hint::black_box(hit);
    }
    let ray_ms = t_ray.elapsed().as_secs_f64() * 1e3;
    println!(
        "- 50,000x Lazy DDA raycast_shapes (6-block range): {ray_ms:.2} ms total ({:.1} ns/ray)",
        (ray_ms * 1e6) / 50_000.0
    );
}
