//! Times the per-entity tracker tick: type lookup, metadata sync, pairing and packet
//! encoding for a dense group of mobs seen by a handful of players.
//!
//! Run with `cargo run --release -p ferrite-entity --example tracker_bench`.

use ferrite_entity::{
    EntityStore, Rng,
    mob::{MobKind, new_mob},
    tracker::{Viewer, track},
};
use ferrite_math::Vec3;
use std::collections::HashSet;
use std::hint::black_box;
use std::time::Instant;

const MOBS: usize = 400;
const VIEWERS: usize = 8;
const TICKS: usize = 300;

fn main() {
    let mut store = EntityStore::default();
    let mut rng = Rng::new(0x5eed);
    let mut seed = 0x1234_5678u32;
    for _ in 0..MOBS {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let x = 8.0 + f64::from((seed >> 8) % 320) / 8.0;
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let z = 8.0 + f64::from((seed >> 8) % 320) / 8.0;
        store.insert(new_mob(MobKind::Zombie, Vec3::new(x, 64.0, z), &mut rng));
    }
    let loaded: HashSet<(i32, i32)> = (-4..=4)
        .flat_map(|x| (-4..=4).map(move |z| (x, z)))
        .collect();
    let mut keys = vec![];
    for i in 0..VIEWERS {
        keys.push((
            (ferrite_entity::random_uuid(), i as u64),
            Vec3::new(8.0 + i as f64 * 4.0, 64.0, 8.0),
        ));
    }
    let viewers: Vec<Viewer<'_>> = keys
        .iter()
        .map(|(key, pos)| Viewer {
            key: *key,
            pos: *pos,
            view_distance: 10,
            chunks: &loaded,
        })
        .collect();

    let mut bytes = 0usize;
    for _ in 0..20 {
        track(&mut store, &viewers, &mut |_, pkt: &[u8]| {
            bytes += pkt.len()
        });
    }
    let start = Instant::now();
    for _ in 0..TICKS {
        track(&mut store, &viewers, &mut |_, pkt: &[u8]| {
            bytes += pkt.len()
        });
    }
    let elapsed = start.elapsed();
    black_box(&store);
    let per_entity_tick = elapsed.as_secs_f64() * 1e9 / (MOBS * TICKS) as f64;
    println!(
        "{MOBS} mobs x {VIEWERS} viewers x {TICKS} ticks: {:.2} ms ({per_entity_tick:.1} ns/entity-tick, {bytes} bytes)",
        elapsed.as_secs_f64() * 1e3
    );
}
