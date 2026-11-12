//! Does compressing and encrypting inline in a connection's tokio task hurt other tasks?
//!
//! `cargo bench -p ferrite-net --bench writer_stage -- [workers] [seconds]`
//!
//! The play writer (`write_loop`) frames and compresses each queued packet and CFB8-encrypts the
//! whole batch inside the connection task, on a tokio worker. This bench reproduces that stage
//! with the same public pieces (`FrameEncoder`, `Cfb8`) for N simulated connections, each pushing
//! a batch of chunk-sized packets every 50 ms, and measures how late a 1 ms probe timer fires
//! while the load runs. It compares:
//!
//! * `inline`: compress and encrypt in the connection task, as the writer does today;
//! * `offload`: the same work on `spawn_blocking`, the cipher state moved in and back.
//!
//! The probe stands in for everything else sharing the runtime: socket readers, keep-alives,
//! the server list. Output is a table; nothing here is a server-level throughput claim.

use ferrite_net::cfb8::Cfb8;
use ferrite_protocol::frame::FrameEncoder;
use std::sync::Arc;
use std::time::{Duration, Instant};

const PACKET_BYTES: usize = 64 * 1024;
const PACKETS_PER_BATCH: usize = 4;
const BATCH_INTERVAL: Duration = Duration::from_millis(50);

/// Deterministic chunk-like body: mostly structured bytes with some noise (same shape as the
/// `ferrite-protocol` framing bench).
fn body(size: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(size);
    let mut state = 0x9e37_79b9u32;
    for i in 0..size {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        out.push(if i % 7 == 0 {
            (state >> 24) as u8
        } else {
            (i % 251) as u8
        });
    }
    out
}

fn stage(encoder: &FrameEncoder, cipher: &mut Cfb8, bodies: &[Vec<u8>], batch: &mut Vec<u8>) {
    batch.clear();
    for body in bodies {
        encoder.encode(body, batch);
    }
    cipher.encrypt(batch);
}

struct Outcome {
    lateness_us: Vec<u64>,
    batches: u64,
}

fn run(workers: usize, connections: usize, seconds: u64, offload: bool) -> Outcome {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async move {
        let bodies = Arc::new(vec![body(PACKET_BYTES); PACKETS_PER_BATCH]);
        let deadline = Instant::now() + Duration::from_secs(seconds);
        let batches = Arc::new(std::sync::atomic::AtomicU64::new(0));

        let mut tasks = Vec::new();
        for id in 0..connections {
            let (bodies, batches) = (bodies.clone(), batches.clone());
            tasks.push(tokio::spawn(async move {
                // One encoder per connection, as in the server: the compressor state sits behind
                // a mutex, so sharing it would serialise every connection.
                let mut encoder = FrameEncoder::default();
                encoder.set_compression(Some(256), 6);
                let encoder = Arc::new(encoder);
                let key = [id as u8; 16];
                let mut cipher = Cfb8::new(&key);
                let mut batch = Vec::new();
                // Spread the connections over the interval like real, unsynchronised clients.
                tokio::time::sleep(BATCH_INTERVAL * id as u32 / connections as u32).await;
                while Instant::now() < deadline {
                    if offload {
                        let (e, b) = (encoder.clone(), bodies.clone());
                        (cipher, batch) = tokio::task::spawn_blocking(move || {
                            stage(&e, &mut cipher, &b, &mut batch);
                            (cipher, batch)
                        })
                        .await
                        .unwrap();
                    } else {
                        stage(&encoder, &mut cipher, &bodies, &mut batch);
                    }
                    std::hint::black_box(&batch);
                    batches.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    tokio::time::sleep(BATCH_INTERVAL).await;
                }
            }));
        }

        // Probes: a few 1 ms timers, as readers and keep-alive tasks would be.
        let mut probes = Vec::new();
        for _ in 0..8 {
            probes.push(tokio::spawn(async move {
                let mut late = Vec::new();
                while Instant::now() < deadline {
                    let start = Instant::now();
                    tokio::time::sleep(Duration::from_millis(1)).await;
                    let over = start.elapsed().saturating_sub(Duration::from_millis(1));
                    late.push(over.as_micros() as u64);
                }
                late
            }));
        }
        let mut lateness_us = Vec::new();
        for probe in probes {
            lateness_us.extend(probe.await.unwrap());
        }
        for task in tasks {
            task.await.unwrap();
        }
        lateness_us.sort_unstable();
        Outcome {
            lateness_us,
            batches: batches.load(std::sync::atomic::Ordering::Relaxed),
        }
    })
}

fn pct(sorted: &[u64], p: f64) -> u64 {
    sorted[((sorted.len() - 1) as f64 * p) as usize]
}

fn main() {
    // `cargo bench` appends `--bench`; take only numeric arguments.
    let numbers: Vec<u64> = std::env::args()
        .skip(1)
        .filter_map(|a| a.parse().ok())
        .collect();
    let cpus = std::thread::available_parallelism().map_or(4, |n| n.get());
    let workers = numbers.first().map_or(cpus, |&w| w as usize);
    let seconds = numbers.get(1).copied().unwrap_or(5);
    println!(
        "writer stage: {PACKETS_PER_BATCH} x {} KiB packets per connection every {} ms, \
         cfb8 backend {}, {workers} tokio workers, {cpus} cpus, {seconds} s per row",
        PACKET_BYTES / 1024,
        BATCH_INTERVAL.as_millis(),
        Cfb8::new(&[0; 16]).backend_name(),
    );
    println!(
        "{:>6} {:>8} {:>10} {:>10} {:>10} {:>10} {:>9}",
        "conns", "mode", "p50 us", "p99 us", "p99.9 us", "max us", "batches"
    );
    for connections in [8, 32, 64, 128] {
        for (name, offload) in [("inline", false), ("offload", true)] {
            let o = run(workers, connections, seconds, offload);
            println!(
                "{connections:>6} {name:>8} {:>10} {:>10} {:>10} {:>10} {:>9}",
                pct(&o.lateness_us, 0.5),
                pct(&o.lateness_us, 0.99),
                pct(&o.lateness_us, 0.999),
                o.lateness_us.last().unwrap(),
                o.batches,
            );
        }
    }
}
