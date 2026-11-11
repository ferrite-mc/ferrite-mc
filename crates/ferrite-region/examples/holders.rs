//! Chunk holder change benchmark, run with
//! `cargo run -p ferrite-region --release --example holders`.
//!
//! Compares changing the holders of an n x n chunk square one chunk at a time
//! (`add_chunk` per chunk: one scheduler lock and one wake each) against the batched
//! `add_chunks`/`remove_chunks` the streamer and `/forceload` use (one lock, one wake). The
//! regions tick a trivial callback so the scheduler is live, as in the server. Each row is the
//! median of 15 runs, in microseconds.
use ferrite_region::{ChunkPos, RegionData, Regionizer, Scheduler, SectionPos};
use std::{collections::HashSet, time::Instant};

#[derive(Default)]
struct Data(HashSet<SectionPos>);
impl RegionData for Data {
    fn merge(&mut self, other: Self) {
        self.0.extend(other.0);
    }
    fn split(&mut self, sections: &HashSet<SectionPos>) -> Self {
        let moved = self.0.intersection(sections).copied().collect();
        self.0.retain(|s| !sections.contains(s));
        Self(moved)
    }
}

fn square(side: i32) -> Vec<ChunkPos> {
    (0..side)
        .flat_map(|x| (0..side).map(move |z| ChunkPos(x, z)))
        .collect()
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn main() {
    println!("| chunks | one by one: add | one by one: remove | batched: add | batched: remove |");
    println!("|---:|---:|---:|---:|---:|");
    for side in [4, 16, 32, 64] {
        let chunks = square(side);
        let (mut a1, mut r1, mut a2, mut r2) = (vec![], vec![], vec![], vec![]);
        for _ in 0..15 {
            let scheduler = Scheduler::new(
                Regionizer::<Data>::new(0, 1),
                (),
                4,
                |_: &mut Data| {},
                |_| {},
            );
            let t = Instant::now();
            for &c in &chunks {
                scheduler.add_chunk(c);
            }
            a1.push(t.elapsed().as_secs_f64() * 1e6);
            let t = Instant::now();
            for &c in &chunks {
                scheduler.remove_chunk(c);
            }
            r1.push(t.elapsed().as_secs_f64() * 1e6);
            let t = Instant::now();
            scheduler.add_chunks(chunks.iter().copied());
            a2.push(t.elapsed().as_secs_f64() * 1e6);
            let t = Instant::now();
            scheduler.remove_chunks(chunks.iter().copied());
            r2.push(t.elapsed().as_secs_f64() * 1e6);
            drop(scheduler);
        }
        println!(
            "| {} | {:.0} us | {:.0} us | {:.0} us | {:.0} us |",
            chunks.len(),
            median(a1),
            median(r1),
            median(a2),
            median(r2)
        );
    }
}
