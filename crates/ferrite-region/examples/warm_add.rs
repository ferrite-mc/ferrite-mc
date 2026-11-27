//! Cost of adding chunks to sections that already hold holders — the common
//! chunk-(re)load path, where no region is created, merged or split.
//!
//! `cargo run -p ferrite-region --release --example warm_add`
//!
//! The scheduler is started with the default topology (merge radius 2, halo 2)
//! so each batched `add_chunks` call would otherwise scan a 5x5 affected-region
//! window per chunk before discovering that the section is already populated.
use ferrite_region::{ChunkPos, RegionData, RegionizerConfig, Scheduler, SectionPos};
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

fn main() {
    let side = 64i32;
    let chunks: Vec<ChunkPos> = (0..side)
        .flat_map(|x| (0..side).map(move |z| ChunkPos(x, z)))
        .collect();
    let mut topology = ferrite_region::Regionizer::<Data>::with_config(RegionizerConfig {
        section_shift: 4,
        merge_radius: 2,
        empty_section_radius: 2,
        min_section_recalc_count: 2,
        max_dead_section_percent: 0.3,
    });
    // Warm every section (and its halo) before the scheduler takes ownership.
    for &chunk in &chunks {
        topology.add_chunk(chunk);
    }
    let scheduler = Scheduler::new(topology, (), 1, |_: &mut Data| {}, |_| {});
    scheduler.add_chunks(chunks.iter().copied());
    let mut samples = Vec::with_capacity(21);
    for _ in 0..21 {
        let start = Instant::now();
        scheduler.add_chunks(chunks.iter().copied());
        samples.push(start.elapsed().as_secs_f64() * 1e6);
    }
    samples.sort_by(f64::total_cmp);
    println!(
        "warm batched add of {} chunks: median {:.0} us ({:.1} ns/chunk)",
        chunks.len(),
        samples[samples.len() / 2],
        samples[samples.len() / 2] * 1000.0 / chunks.len() as f64
    );
}
