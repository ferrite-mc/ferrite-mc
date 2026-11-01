//! Server-side worldgen throughput benchmark.
//!
//! Usage: cargo run --release -p ferrite-server --example genbench [seed] [count] [startX] [startZ]
//!
//! Exercises the same entry point the chunk source uses (`VanillaTerrain::generate`) and
//! times the generated column, the wire encoding of `level_chunk_with_light`, and the
//! whole pipeline. `FERRITE_WORLDGEN_THREADS` selects both the worker pool size and the
//! number of concurrent requesters, so the benchmark measures the pool the server runs.

use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Instant;

/// Pipeline stage names, in the order [`ferrite_server::game::StageTimings`] lists them.
const STAGE_NAMES: [&str; 6] = ["noise", "surface", "carve", "biomes", "features", "light"];
const STAGES: usize = STAGE_NAMES.len();

fn stage_values(timings: ferrite_server::game::StageTimings) -> [f64; STAGES] {
    [
        timings.noise_ms,
        timings.surface_ms,
        timings.carve_ms,
        timings.biomes_ms,
        timings.features_ms,
        timings.light_ms,
    ]
}

fn worldgen_threads() -> usize {
    std::env::var("FERRITE_WORLDGEN_THREADS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|threads| *threads > 0)
        .unwrap_or_else(|| {
            std::thread::available_parallelism().map_or(1, |threads| threads.get().min(8))
        })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seed: i64 = args.get(1).map_or(0, |s| s.parse().unwrap());
    let count: usize = args.get(2).map_or(50, |s| s.parse().unwrap());
    let start_x: i32 = args.get(3).map_or(0, |s| s.parse().unwrap());
    let start_z: i32 = args.get(4).map_or(0, |s| s.parse().unwrap());
    let requesters = worldgen_threads();

    let terrain = Arc::new(ferrite_server::game::VanillaTerrain::spawn(seed).unwrap());
    let started = Instant::now();
    let (generate_total, encode_total, stages, hashes) = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(requesters);
        for requester in 0..requesters {
            let terrain = Arc::clone(&terrain);
            handles.push(scope.spawn(move || {
                let mut generate = 0.0_f64;
                let mut encode = 0.0_f64;
                let mut stages = [0.0_f64; STAGES];
                let mut hashes = Vec::new();
                for index in (requester..count).step_by(requesters) {
                    let cx = start_x + (index % 10) as i32;
                    let cz = start_z + (index / 10) as i32;
                    let mark = Instant::now();
                    let generated = terrain.generate(cx, cz).unwrap();
                    generate += mark.elapsed().as_secs_f64() * 1000.0;
                    for (total, value) in stages.iter_mut().zip(stage_values(generated.timings)) {
                        *total += value;
                    }
                    let mark = Instant::now();
                    let packet = ferrite_world::encode_chunk_packet(
                        cx,
                        cz,
                        &generated.chunk,
                        &generated.light,
                    )
                    .unwrap();
                    encode += mark.elapsed().as_secs_f64() * 1000.0;
                    std::hint::black_box(packet.len());
                    hashes.push((index, Sha256::digest(&packet)));
                }
                (generate, encode, stages, hashes)
            }));
        }
        let mut totals = (0.0_f64, 0.0_f64, [0.0_f64; STAGES], Vec::new());
        for handle in handles {
            let (generate, encode, stages, hashes) = handle.join().unwrap();
            totals.0 += generate;
            totals.1 += encode;
            for (total, value) in totals.2.iter_mut().zip(stages) {
                *total += value;
            }
            totals.3.extend(hashes);
        }
        totals
    });
    let wall = started.elapsed().as_secs_f64();
    let n = count.max(1) as f64;
    println!(
        "server worldgen benchmark: seed={seed} chunks={count} from ({start_x},{start_z}) threads={requesters}"
    );
    println!("generate+light: {:8.2} ms/chunk (cpu)", generate_total / n);
    println!("wire encode:    {:8.2} ms/chunk (cpu)", encode_total / n);
    for (name, total) in STAGE_NAMES.iter().zip(stages) {
        println!("  {name:10}    {:8.2} ms/chunk", total / n);
    }
    println!("pipeline wall:  {:8.2} ms/chunk", wall * 1000.0 / n);
    println!("chunks/second:  {:8.2}", count as f64 / wall);
    let mut hashes = hashes;
    hashes.sort_unstable_by_key(|(index, _)| *index);
    let mut digest = Sha256::new();
    for (_, hash) in hashes {
        digest.update(hash);
    }
    println!("packet SHA-256: {:x}", digest.finalize());
}
