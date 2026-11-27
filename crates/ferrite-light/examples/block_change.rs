//! `cargo run -p ferrite-light --release --example block_change`
//!
//! Median wall time of one incremental `LightEngine::block_changed` call in a
//! fully loaded 3x3 neighborhood with a 384-block world, measured for a change
//! near the surface and one near the build limit. A block change only affects
//! sky light at and below its own layer, so the lower probe shows how much of
//! the column rescan is redundant work.
use ferrite_light::{LightBlockView, LightEngine};
use std::{hint::black_box, time::Instant};

struct Terrain {
    stone: u16,
    dirt: u16,
    placed: bool,
    /// At [`Terrain::LOW`], emit dirt instead of stone. Both are full opaque cubes, so
    /// their light properties are identical and the swap must not relight anything.
    swap_full_block: bool,
}

impl Terrain {
    const LOW: (i32, i32, i32) = (8, 70, 8);
    const HIGH: (i32, i32, i32) = (9, 300, 9);
}

impl LightBlockView for Terrain {
    fn state(&self, x: i32, y: i32, z: i32) -> u16 {
        if (x, y, z) == Self::LOW {
            if !self.placed {
                return 0;
            }
            return if self.swap_full_block {
                self.dirt
            } else {
                self.stone
            };
        }
        if (x, y, z) == Self::HIGH {
            return if self.placed { self.stone } else { 0 };
        }
        let ground = 60 + (x.wrapping_mul(13) ^ z.wrapping_mul(7)).rem_euclid(7);
        if y < ground { self.stone } else { 0 }
    }
}

fn probe(
    engine: &mut LightEngine,
    world: &mut Terrain,
    pos: (i32, i32, i32),
    rounds: usize,
) -> f64 {
    let mut samples = Vec::with_capacity(rounds);
    for _ in 0..rounds {
        world.placed = !world.placed;
        let start = Instant::now();
        engine.block_changed(world, pos.0, pos.1, pos.2);
        samples.push(start.elapsed().as_secs_f64() * 1e6);
        black_box(engine.get(ferrite_light::LightKind::Sky, pos.0, pos.1 - 1, pos.2));
    }
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}

fn probe_same_properties(
    engine: &mut LightEngine,
    world: &mut Terrain,
    pos: (i32, i32, i32),
    rounds: usize,
) -> f64 {
    world.placed = true;
    let mut samples = Vec::with_capacity(rounds);
    for _ in 0..50 {
        world.swap_full_block = !world.swap_full_block;
        engine.block_changed(world, pos.0, pos.1, pos.2);
    }
    for _ in 0..rounds {
        world.swap_full_block = !world.swap_full_block;
        let start = Instant::now();
        engine.block_changed(world, pos.0, pos.1, pos.2);
        samples.push(start.elapsed().as_secs_f64() * 1e6);
        black_box(engine.get(ferrite_light::LightKind::Sky, pos.0, pos.1 - 1, pos.2));
    }
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}

fn main() {
    let stone = ferrite_data::blocks::default_state("stone").unwrap();
    let dirt = ferrite_data::blocks::default_state("dirt").unwrap();
    let mut world = Terrain {
        stone,
        dirt,
        placed: false,
        swap_full_block: false,
    };
    let mut engine = LightEngine::new(0, 0, -4, 19, true);
    engine.relight(&world);
    // Warm up both columns through the same code path used for the samples.
    for _ in 0..50 {
        world.placed = !world.placed;
        engine.block_changed(&world, Terrain::LOW.0, Terrain::LOW.1, Terrain::LOW.2);
        world.placed = !world.placed;
        engine.block_changed(&world, Terrain::HIGH.0, Terrain::HIGH.1, Terrain::HIGH.2);
    }
    let low = probe(&mut engine, &mut world, Terrain::LOW, 400);
    world.placed = false;
    engine.block_changed(&world, Terrain::LOW.0, Terrain::LOW.1, Terrain::LOW.2);
    let high = probe(&mut engine, &mut world, Terrain::HIGH, 400);
    let same = probe_same_properties(&mut engine, &mut world, Terrain::LOW, 400);
    println!("| probe | y | median block_changed |");
    println!("|---|---|---|");
    println!("| surface column | {} | {low:.1} us |", Terrain::LOW.1);
    println!("| near build limit | {} | {high:.1} us |", Terrain::HIGH.1);
    println!(
        "| same-properties swap | {} | {same:.1} us |",
        Terrain::LOW.1
    );
}
