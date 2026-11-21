//! Batched light-pool throughput and contention.
//!
//! `cargo run -p ferrite-light --release --example pool_throughput`
//!
//! Three synthetic sets of 12 full-height chunk neighbourhoods are relit at 1,
//! 2, 3, 4 and 8 workers: centres three chunks apart (disjoint 3x3
//! neighbourhoods, so only the chunk table and publication can contend),
//! centres one chunk apart (every neighbourhood overlaps, so publication
//! serialises), and 12 identical jobs on one centre (maximum publication
//! contention). The best and mean wall time of three runs are reported per
//! worker count, together with per-worker efficiency and the machine load
//! average before and after each measurement, because a shared build machine
//! is not a quiet benchmark host.
use ferrite_light::{LightBlockView, LightJob, LightPool, LightPoolConfig};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};

const MIN_SECTION: i32 = -4;
const MAX_SECTION: i32 = 19;
const REPEATS: usize = 3;
const WORKER_COUNTS: [usize; 5] = [1, 2, 3, 4, 8];

struct Terrain {
    stone: u16,
    water: u16,
    torch: u16,
}
impl Terrain {
    fn new() -> Self {
        Self {
            stone: ferrite_data::blocks::default_state("stone").unwrap(),
            water: ferrite_data::blocks::default_state("water").unwrap(),
            torch: ferrite_data::blocks::default_state("torch").unwrap(),
        }
    }
}

fn hash3(seed: u32, x: i32, y: i32, z: i32) -> u32 {
    let mut h = seed
        ^ (x as u32).wrapping_mul(0x9e37_79b9)
        ^ (y as u32).wrapping_mul(0x85eb_ca6b)
        ^ (z as u32).wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^ (h >> 16)
}

impl LightBlockView for Terrain {
    fn state(&self, x: i32, y: i32, z: i32) -> u16 {
        let ground = 60 + (hash3(1, x, 0, z) % 7) as i32;
        if y < ground {
            if y < ground - 3 && hash3(2, x, y, z).is_multiple_of(23) {
                0
            } else {
                self.stone
            }
        } else if y <= 62 && hash3(3, x, 0, z).is_multiple_of(5) {
            self.water
        } else if y == ground + 1 && hash3(4, x, 0, z).is_multiple_of(31) {
            self.torch
        } else {
            0
        }
    }
}

fn grid(columns: i32, rows: i32, spacing: i32) -> Vec<LightJob> {
    let mut jobs = Vec::new();
    for z in 0..rows {
        for x in 0..columns {
            jobs.push(LightJob::new(x * spacing, z * spacing));
        }
    }
    jobs
}

/// The full `/proc/loadavg` 1/5/15-minute triple, or `unavailable`.
fn load_average() -> String {
    std::fs::read_to_string("/proc/loadavg")
        .map(|line| {
            line.split_whitespace()
                .take(3)
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_else(|_| "unavailable".to_owned())
}

fn nproc() -> String {
    std::process::Command::new("nproc")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|stdout| stdout.trim().to_owned())
        .filter(|trimmed| !trimmed.is_empty())
        .unwrap_or_else(|| "unavailable".to_owned())
}

struct Measurement {
    workers: usize,
    best: Duration,
    mean: Duration,
    load_before: String,
    load_after: String,
}

fn measure(terrain: &Terrain, jobs: &[LightJob], workers: usize) -> Measurement {
    let load_before = load_average();
    let pool = LightPool::new(LightPoolConfig::new(
        workers,
        MIN_SECTION,
        MAX_SECTION,
        true,
    ));
    // Warm up caches and worker-engine allocation outside the timing.
    black_box(pool.relight(terrain, jobs).expect("warm-up relight"));
    let mut best = Duration::MAX;
    let mut total = Duration::ZERO;
    for _ in 0..REPEATS {
        let start = Instant::now();
        let snapshots = pool.relight(terrain, jobs).expect("timed relight");
        let elapsed = start.elapsed();
        black_box(&snapshots);
        best = best.min(elapsed);
        total += elapsed;
    }
    Measurement {
        workers,
        best,
        mean: total / REPEATS as u32,
        load_after: load_average(),
        load_before,
    }
}

fn print_scenario(name: &str, jobs: &[LightJob], readings: &[Measurement]) {
    let serial = readings[0].best.as_secs_f64();
    println!(
        "\n## {name} ({} jobs)\n\n\
         | workers | loadavg 1/5/15 before → after | best wall (ms) | mean wall (ms) | \
         ms/job (best) | speedup vs 1 (best) | efficiency |\n\
         |---:|---|---:|---:|---:|---:|---:|",
        jobs.len()
    );
    for reading in readings {
        let best = reading.best.as_secs_f64() * 1000.0;
        let speedup = serial / reading.best.as_secs_f64();
        println!(
            "| {} | {} → {} | {best:.3} | {:.3} | {:.3} | {speedup:.2}x | {:.1}% |",
            reading.workers,
            reading.load_before,
            reading.load_after,
            reading.mean.as_secs_f64() * 1000.0,
            best / jobs.len() as f64,
            speedup / reading.workers as f64 * 100.0,
        );
    }
}

fn main() {
    let terrain = Terrain::new();
    let parallelism = std::thread::available_parallelism().map_or(1, |n| n.get());
    println!(
        "Ferrite light pool throughput\n\
         nproc = {}; available_parallelism = {parallelism}; sections {MIN_SECTION}..={MAX_SECTION}; \
         12 jobs per scenario; {REPEATS} timed runs (best and mean reported)",
        nproc()
    );
    println!("load average at start: {}", load_average());

    let scenarios = [
        ("disjoint centres", grid(4, 3, 3)),
        ("adjacent centres", grid(4, 3, 1)),
        ("same-centre contention", vec![LightJob::new(2, 2); 12]),
    ];
    let mut all = Vec::new();
    for (name, jobs) in &scenarios {
        let readings: Vec<_> = WORKER_COUNTS
            .iter()
            .map(|&workers| measure(&terrain, jobs, workers))
            .collect();
        print_scenario(name, jobs, &readings);
        all.push((*name, readings));
    }

    println!(
        "\n## speedup by worker count (best wall, each scenario vs its own 1-worker run)\n\n\
         | workers | disjoint centres | adjacent centres | same-centre contention |\n\
         |---:|---:|---:|---:|"
    );
    for (index, &workers) in WORKER_COUNTS.iter().enumerate() {
        let speedups: Vec<_> = all
            .iter()
            .map(|(_, readings)| {
                readings[0].best.as_secs_f64() / readings[index].best.as_secs_f64()
            })
            .collect();
        println!(
            "| {workers} | {:.2}x | {:.2}x | {:.2}x |",
            speedups[0], speedups[1], speedups[2]
        );
    }

    println!("\nnproc at end: {}", nproc());
    println!("load average at end: {}", load_average());
}
