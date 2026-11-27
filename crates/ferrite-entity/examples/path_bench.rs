//! Times A* pathfinding on a flat floor with a wall, the shape mob navigation
//! actually asks for.
//!
//! Run with `cargo run --release -p ferrite-entity --example path_bench`.

use ferrite_data::blocks;
use ferrite_entity::path::find_path;
use ferrite_entity::physics::BlockGetter;
use std::hint::black_box;
use std::time::Instant;

/// A flat floor at `y = 64` with a wall at `x = 8` spanning `z = -12..=12`.
struct World {
    stone: u16,
    air: u16,
}

impl BlockGetter for World {
    fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
        if x == 8 && (64..67).contains(&y) && (-12..=12).contains(&z) {
            return Some(self.stone);
        }
        Some(if y < 64 { self.stone } else { self.air })
    }
}

const PATHS: usize = 400;

fn main() {
    let world = World {
        stone: blocks::default_state("stone").expect("stone state"),
        air: blocks::default_state("air").expect("air state"),
    };
    // Warm the thread-local scratch tables so the timed loop measures search work.
    for _ in 0..8 {
        black_box(find_path(&world, [0, 64, 0], [12, 64, 0], 32));
    }

    let mut total_len = 0usize;
    let start = Instant::now();
    for _ in 0..PATHS {
        let path = find_path(&world, [0, 64, 0], [12, 64, 0], 32);
        total_len += path.len();
        black_box(path);
    }
    let elapsed = start.elapsed();
    let per_path = elapsed.as_secs_f64() * 1e6 / PATHS as f64;
    println!(
        "{PATHS} paths in {:.2} ms ({per_path:.1} us/path, {} waypoints total)",
        elapsed.as_secs_f64() * 1e3,
        total_len / PATHS
    );
}
