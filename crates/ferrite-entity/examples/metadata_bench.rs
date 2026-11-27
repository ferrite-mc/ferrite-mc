//! Times the metadata sync the entity tracker runs every tick. A named mob calls
//! `set_custom_name` twice per tick even when the name never changes.
//!
//! Run with `cargo run --release -p ferrite-entity --example metadata_bench`.

use ferrite_entity::Metadata;
use std::hint::black_box;
use std::time::Instant;

fn main() {
    let mut meta = Metadata::for_player();
    meta.set_custom_name(Some("Tracker"));

    const CALLS: usize = 2_000_000;
    let start = Instant::now();
    for _ in 0..CALLS {
        meta.set_custom_name(Some("Tracker"));
        black_box(&meta);
    }
    let ns = start.elapsed().as_secs_f64() * 1e9 / CALLS as f64;
    println!("unchanged custom name: {ns:.2} ns/call");
}
