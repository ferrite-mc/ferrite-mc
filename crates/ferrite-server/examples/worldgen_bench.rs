//! Detailed pure-Rust worldgen throughput measurement.
//!
//! Measures the full [`RustGenerator::generate`] path (terrain, structure starts/jigsaw and
//! decoration; no lighting, serialization or networking) for every thread count from 1 to
//! the number of logical CPUs, and for all three dimensions at 1/4/12 threads.
//!
//! `cargo run -p ferrite-server --release --example worldgen_bench -- <csv-path>`
//!
//! Writes one CSV row per trial, plus a human-readable summary to stdout.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use ferrite_server::game::{ChunkGenerator, Dimension, RustGenerator};
use ferrite_world::RuntimeRegistry;

const SCALING_CHUNKS: usize = 256;
const SCALING_REPS: usize = 5;
const DIM_CHUNKS: usize = 128;
const DIM_REPS: usize = 3;
const DIM_THREADS: [usize; 3] = [1, 4, 12];
const MAX_THREADS: usize = 12;

struct Trial {
    section: &'static str,
    dimension: &'static str,
    threads: usize,
    rep: usize,
    chunks: usize,
    wall_ms: f64,
    cps: f64,
    min_ms: f64,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    max_ms: f64,
}

impl Trial {
    fn csv_row(&self) -> String {
        format!(
            "{},{},{},{},{},{:.3},{:.3},{:.4},{:.4},{:.4},{:.4},{:.4}",
            self.section,
            self.dimension,
            self.threads,
            self.rep,
            self.chunks,
            self.wall_ms,
            self.cps,
            self.min_ms,
            self.p50_ms,
            self.p95_ms,
            self.p99_ms,
            self.max_ms
        )
    }
}

fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = (((sorted.len() - 1) as f64) * q).round() as usize;
    sorted[idx]
}

fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    percentile(&sorted, 0.5)
}

/// A square block of chunk coordinates starting at `offset`.
fn chunk_grid(count: usize, side: i32, offset: i32) -> Vec<(i32, i32)> {
    (0..count)
        .map(|i| (i as i32 % side + offset, i as i32 / side + offset))
        .collect()
}

/// Runs `coords` across `threads` workers and returns the wall time and per-chunk timings.
fn run_trial(
    generator: &RustGenerator,
    dimension: Dimension,
    coords: &[(i32, i32)],
    threads: usize,
) -> (f64, Vec<f64>) {
    let next = AtomicUsize::new(0);
    let started = Instant::now();
    let mut timings = Vec::with_capacity(coords.len());

    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(threads);
        for _ in 0..threads {
            let next_ref = &next;
            let coords_ref = coords;
            let generator_ref = generator;
            handles.push(scope.spawn(move || {
                let mut local = Vec::with_capacity(coords_ref.len() / threads + 1);
                loop {
                    let idx = next_ref.fetch_add(1, Ordering::Relaxed);
                    let Some(&(cx, cz)) = coords_ref.get(idx) else {
                        break;
                    };
                    let chunk_start = Instant::now();
                    let generated = generator_ref.generate(dimension, cx, cz).unwrap();
                    local.push(chunk_start.elapsed().as_secs_f64() * 1e3);
                    std::hint::black_box(generated);
                }
                local
            }));
        }
        for handle in handles {
            timings.extend(handle.join().unwrap());
        }
    });

    (started.elapsed().as_secs_f64() * 1e3, timings)
}

fn measure(
    generator: &RustGenerator,
    section: &'static str,
    dimension: Dimension,
    dimension_name: &'static str,
    coords: &[(i32, i32)],
    threads: usize,
    rep: usize,
) -> Trial {
    let (wall_ms, mut timings) = run_trial(generator, dimension, coords, threads);
    timings.sort_by(f64::total_cmp);
    let cps = coords.len() as f64 / (wall_ms / 1000.0).max(0.000_001);
    Trial {
        section,
        dimension: dimension_name,
        threads,
        rep,
        chunks: coords.len(),
        wall_ms,
        cps,
        min_ms: timings.first().copied().unwrap_or(0.0),
        p50_ms: percentile(&timings, 0.50),
        p95_ms: percentile(&timings, 0.95),
        p99_ms: percentile(&timings, 0.99),
        max_ms: timings.last().copied().unwrap_or(0.0),
    }
}

fn main() {
    let vanilla = Path::new("vanilla/26.3/generated");
    let registry = Arc::new(RuntimeRegistry::load(vanilla).unwrap());
    let generator = RustGenerator::with_vanilla_data(12345, Arc::clone(&registry), vanilla);

    let csv_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "worldgen_scaling.csv".to_owned());
    let mut csv = BufWriter::new(File::create(&csv_path).unwrap());
    writeln!(
        csv,
        "section,dimension,threads,rep,chunks,wall_ms,cps,min_ms,p50_ms,p95_ms,p99_ms,max_ms"
    )
    .unwrap();

    println!("Ferrite pure-Rust worldgen — detailed measurement");
    println!(
        "logical CPUs: {}, seed: 12345, data: {}",
        std::thread::available_parallelism().map_or(0, |n| n.get()),
        vanilla.display()
    );
    println!(
        "work: full RustGenerator::generate (terrain + structures + decoration), no lighting/serialization"
    );

    // Warm lazy tables and the per-thread evaluation cache away from the measured area.
    for i in 0..8 {
        std::hint::black_box(
            generator
                .generate(Dimension::Overworld, 1000 + i, 1000)
                .unwrap(),
        );
    }

    let scaling_coords = chunk_grid(SCALING_CHUNKS, 16, 0);
    println!(
        "\n=== Overworld thread scaling ({} chunks x {} reps) ===",
        SCALING_CHUNKS, SCALING_REPS
    );
    println!(
        "{:>7} {:>12} {:>17} {:>9} {:>9} {:>9} {:>9}",
        "threads", "median cps", "min–max", "speedup", "effic.", "p50 ms", "p99 ms"
    );

    let mut base_cps = 0.0;
    for threads in 1..=MAX_THREADS {
        let mut cps_runs = Vec::with_capacity(SCALING_REPS);
        let mut p50_runs = Vec::with_capacity(SCALING_REPS);
        let mut p99_runs = Vec::with_capacity(SCALING_REPS);
        for rep in 0..SCALING_REPS {
            let trial = measure(
                &generator,
                "scaling",
                Dimension::Overworld,
                "Overworld",
                &scaling_coords,
                threads,
                rep,
            );
            writeln!(csv, "{}", trial.csv_row()).unwrap();
            cps_runs.push(trial.cps);
            p50_runs.push(trial.p50_ms);
            p99_runs.push(trial.p99_ms);
        }
        let cps = median(&cps_runs);
        if threads == 1 {
            base_cps = cps;
        }
        let speedup = cps / base_cps.max(0.000_001);
        println!(
            "{threads:>7} {cps:>12.1} {:>8.1}–{:<8.1} {speedup:>8.2}x {:>8.1}% {:>9.2} {:>9.2}",
            cps_runs.iter().copied().fold(f64::INFINITY, f64::min),
            cps_runs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            speedup / threads as f64 * 100.0,
            median(&p50_runs),
            median(&p99_runs)
        );
    }

    let dim_coords = chunk_grid(DIM_CHUNKS, 16, 0);
    println!(
        "\n=== Dimension throughput ({} chunks x {} reps) ===",
        DIM_CHUNKS, DIM_REPS
    );
    println!(
        "{:>10} {:>7} {:>12} {:>17} {:>12} {:>10}",
        "dimension", "threads", "median cps", "min–max", "ms/chunk", "p99 ms"
    );
    for (dim_name, dimension) in [
        ("Overworld", Dimension::Overworld),
        ("Nether", Dimension::Nether),
        ("End", Dimension::End),
    ] {
        for threads in DIM_THREADS {
            let mut cps_runs = Vec::with_capacity(DIM_REPS);
            let mut p99_runs = Vec::with_capacity(DIM_REPS);
            for rep in 0..DIM_REPS {
                let trial = measure(
                    &generator,
                    "dimension",
                    dimension,
                    dim_name,
                    &dim_coords,
                    threads,
                    rep,
                );
                writeln!(csv, "{}", trial.csv_row()).unwrap();
                cps_runs.push(trial.cps);
                p99_runs.push(trial.p99_ms);
            }
            let cps = median(&cps_runs);
            println!(
                "{dim_name:>10} {threads:>7} {cps:>12.1} {:>8.1}–{:<8.1} {:>12.3} {:>10.2}",
                cps_runs.iter().copied().fold(f64::INFINITY, f64::min),
                cps_runs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                1000.0 / cps.max(0.000_001),
                median(&p99_runs)
            );
        }
    }

    csv.flush().unwrap();
    println!("\nCSV: {csv_path}");
}
