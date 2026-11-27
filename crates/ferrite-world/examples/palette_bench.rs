//! Times `PalettedContainer::get`/`set` at every indirect width.
//!
//! The interesting widths are 5, 6 and 7 bits, where a 64-bit word holds 12, 10 or
//! 9 entries: the lane index is then not a power of two, so a runtime `per` costs
//! an integer division per block access.
//!
//! Run with `cargo run --release -p ferrite-world --example palette_bench`.

use ferrite_world::{PaletteKind, PalettedContainer};
use std::hint::black_box;
use std::time::Instant;

const ITERS: usize = 4_000_000;

fn fixture(bits: u8) -> PalettedContainer {
    let values = 1usize << bits;
    let mut states = [0u16; 4096];
    for (i, state) in states.iter_mut().enumerate() {
        *state = (i % values) as u16;
    }
    let container = PalettedContainer::from_states_u16(PaletteKind::Blocks, &states, 16).unwrap();
    assert_eq!(
        container.bits_per_entry(),
        bits,
        "fixture should pack at {bits} bits"
    );
    container
}

fn main() {
    println!("{:<6} {:>12} {:>12}", "bits", "get ns/op", "set ns/op");
    for bits in [4u8, 5, 6, 7, 8] {
        let mut container = fixture(bits);
        let values = 1u32 << bits;

        // Reads: walk every entry so each word and lane is touched.
        let start = Instant::now();
        let mut sum = 0u64;
        for i in 0..ITERS {
            let index = black_box(i & 4095);
            sum = sum.wrapping_add(u64::from(container.get(index).unwrap()));
        }
        let read = start.elapsed().as_secs_f64() * 1e9 / ITERS as f64;
        black_box(sum);

        // Writes: flip one entry between two palette values so the packed lane is
        // actually rewritten instead of short-circuiting on an unchanged value.
        let a = 0u32;
        let b = values - 1;
        let start = Instant::now();
        for i in 0..ITERS {
            let value = if i & 1 == 0 { a } else { b };
            container.set(black_box(2047), value).unwrap();
        }
        let write = start.elapsed().as_secs_f64() * 1e9 / ITERS as f64;
        black_box(container.get(2047));

        println!("{bits:<6} {read:>12.2} {write:>12.2}");
    }
}
