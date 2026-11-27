//! Times `Chunk::set_block` heightmap maintenance for the two patterns that dominate
//! block updates: a state swap whose heightmap membership is unchanged, and removing a
//! floating block whose column below is empty for hundreds of blocks.
//!
//! Run with `cargo run --release -p ferrite-world --example heightmap_bench`.

use ferrite_data::block_props;
use ferrite_world::{BlockFlags, BlockInfo, Chunk};
use std::hint::black_box;
use std::time::Instant;

struct Vanilla;

impl BlockInfo for Vanilla {
    fn flags(&self, state: u32) -> BlockFlags {
        let p = block_props::get(state as u16);
        BlockFlags {
            air: p.is_air,
            motion_blocking: p.blocks_motion,
            fluid: p.fluid.is_some(),
            leaves: false,
        }
    }
}

fn main() {
    let info = Vanilla;
    let air = u32::from(ferrite_data::blocks::default_state("air").unwrap());
    let stone = u32::from(ferrite_data::blocks::default_state("stone").unwrap());
    let dirt = u32::from(ferrite_data::blocks::default_state("dirt").unwrap());

    // A full stone column: replacing its top block with another full block changes no
    // heightmap membership, so the entire update can be skipped.
    let mut column = Chunk::new(-64, 384, air, 0, 15, 6).unwrap();
    for y in -64..320 {
        column.set_block(0, y, 0, stone, &info).unwrap();
    }
    const SWAPS: usize = 200_000;
    let start = Instant::now();
    for i in 0..SWAPS {
        column
            .set_block(0, 319, 0, if i % 2 == 0 { dirt } else { stone }, &info)
            .unwrap();
    }
    let swap_ns = start.elapsed().as_secs_f64() * 1e9 / SWAPS as f64;
    black_box(&column);

    // One floating block in an otherwise empty column: removing it makes four
    // heightmaps rescan the whole column below.
    let mut floating = Chunk::new(-64, 384, air, 0, 15, 6).unwrap();
    const FLOATS: usize = 2_000;
    floating.set_block(0, 300, 0, stone, &info).unwrap();
    let start = Instant::now();
    for _ in 0..FLOATS {
        floating.set_block(0, 300, 0, air, &info).unwrap();
        floating.set_block(0, 300, 0, stone, &info).unwrap();
    }
    let float_ns = start.elapsed().as_secs_f64() * 1e9 / FLOATS as f64;
    black_box(&floating);

    println!("| probe | time |");
    println!("|---|---:|");
    println!("| full-block swap, same heightmaps | {swap_ns:.1} ns/set |");
    println!("| floating block remove+restore | {float_ns:.0} ns/pair |");
}
