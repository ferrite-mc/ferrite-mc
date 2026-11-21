use ferrite_light::{
    LightBlockView, LightEngine, LightJob, LightKind, LightPool, LightPoolConfig, LightPoolError,
    LightSnapshot,
};
use std::{
    collections::HashMap,
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

struct World {
    blocks: HashMap<(i32, i32, i32), u16>,
    air: u16,
}
impl LightBlockView for World {
    fn state(&self, x: i32, y: i32, z: i32) -> u16 {
        *self.blocks.get(&(x, y, z)).unwrap_or(&self.air)
    }
}

fn state(name: &str) -> u16 {
    ferrite_data::blocks::default_state(name).unwrap()
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

/// Deterministic terrain over the given chunk range: stone ground with caves,
/// occasional water, and torches, glass and leaves on the surface. Blocks
/// outside the generated range are air and every chunk reports as loaded.
fn random_world(seed: u32, min_chunk: i32, max_chunk: i32, max_section: i32) -> World {
    let (air, stone, water, torch, glass, leaves) = (
        state("air"),
        state("stone"),
        state("water"),
        state("torch"),
        state("glass"),
        state("oak_leaves"),
    );
    let top = (max_section + 1) * 16 - 1;
    let mut blocks = HashMap::new();
    for chunk_z in min_chunk..=max_chunk {
        for chunk_x in min_chunk..=max_chunk {
            let ground = 10 + (hash3(seed, chunk_x, 0, chunk_z) % 20) as i32;
            for local_z in 0..16 {
                for local_x in 0..16 {
                    let x = chunk_x * 16 + local_x;
                    let z = chunk_z * 16 + local_z;
                    for y in 0..=ground.min(top) {
                        let h = hash3(seed, x, y, z);
                        let material = if y < ground - 3 && h.is_multiple_of(23) {
                            air
                        } else if h.is_multiple_of(29) {
                            water
                        } else {
                            stone
                        };
                        blocks.insert((x, y, z), material);
                    }
                    let surface = ground + 1;
                    if surface <= top {
                        let h = hash3(seed, x, 7, z);
                        if h.is_multiple_of(19) {
                            blocks.insert((x, surface, z), torch);
                        } else if h.is_multiple_of(13) {
                            blocks.insert((x, surface, z), glass);
                        } else if h.is_multiple_of(17) {
                            blocks.insert((x, surface, z), leaves);
                        }
                    }
                }
            }
        }
    }
    World { blocks, air }
}

fn serial_engine(world: &World, job: LightJob, min_section: i32, max_section: i32) -> LightEngine {
    let mut engine = LightEngine::new(job.center_x, job.center_z, min_section, max_section, true);
    engine.relight(world);
    engine
}

fn assert_snapshot_matches(
    expected: &LightEngine,
    snapshot: &LightSnapshot,
    kind: LightKind,
    context: &str,
) {
    let (chunk_x, chunk_z) = snapshot.center();
    for local_z in 0..16 {
        for local_x in 0..16 {
            let x = chunk_x * 16 + local_x;
            let z = chunk_z * 16 + local_z;
            for y in snapshot.min_section() * 16..=(snapshot.max_section() + 1) * 16 - 1 {
                assert_eq!(
                    snapshot.get(kind, x, y, z),
                    expected.get(kind, x, y, z),
                    "{context}: {kind:?} at {x},{y},{z} in chunk {chunk_x},{chunk_z}"
                );
            }
        }
    }
}

fn jobs(centres: &[(i32, i32)]) -> Vec<LightJob> {
    centres.iter().map(|&(x, z)| LightJob::new(x, z)).collect()
}

#[test]
fn disjoint_centres_parallel_relight_equals_serial() {
    let (min_section, max_section) = (0, 2);
    let world = random_world(7, -5, 5, max_section);
    let jobs = jobs(&[
        (0, 0),
        (3, 0),
        (0, 3),
        (3, 3),
        (-3, 0),
        (0, -3),
        (-3, -3),
        (3, -3),
    ]);
    let serial: Vec<_> = jobs
        .iter()
        .map(|&job| serial_engine(&world, job, min_section, max_section))
        .collect();
    for workers in [1, 2, 4, 8] {
        let pool = LightPool::new(LightPoolConfig::new(
            workers,
            min_section,
            max_section,
            true,
        ));
        let snapshots = pool
            .relight(&world, &jobs)
            .unwrap_or_else(|error| panic!("{workers} workers failed: {error}"));
        assert_eq!(snapshots.len(), jobs.len());
        for ((job, snapshot), expected) in jobs.iter().zip(&snapshots).zip(&serial) {
            for kind in [LightKind::Block, LightKind::Sky] {
                assert_snapshot_matches(expected, snapshot, kind, &format!("{workers} workers"));
            }
            assert_eq!(
                pool.snapshot(job.center_x, job.center_z).as_deref(),
                Some(snapshot.as_ref()),
                "published snapshot for {:?}",
                job.center()
            );
        }
    }
}

#[test]
fn same_centre_contention_completes_and_equals_serial() {
    let (min_section, max_section) = (0, 2);
    let world = random_world(11, -4, 4, max_section);
    let job = LightJob::new(2, 2);
    let jobs = vec![job; 32];
    let expected = serial_engine(&world, job, min_section, max_section);
    for workers in [1, 2, 4, 8] {
        let pool = LightPool::new(LightPoolConfig::new(
            workers,
            min_section,
            max_section,
            true,
        ));
        let snapshots = pool
            .relight(&world, &jobs)
            .unwrap_or_else(|error| panic!("{workers} workers failed: {error}"));
        for snapshot in &snapshots {
            assert_eq!(snapshot, &snapshots[0], "duplicate jobs disagree");
            for kind in [LightKind::Block, LightKind::Sky] {
                assert_snapshot_matches(&expected, snapshot, kind, &format!("{workers} workers"));
            }
        }
    }
}

/// Centres `(0, 0)` and `(1, 0)` have overlapping 3x3 neighbourhoods. A
/// centre-first lock order (`(0, 0)` then `(1, 0)` for one job, the reverse for
/// the other) would deadlock; the pool's ascending `(x, z)` order cannot.
#[test]
fn overlapping_neighbourhoods_complete_in_canonical_lock_order() {
    let (min_section, max_section) = (0, 2);
    let world = Arc::new(random_world(23, -4, 4, max_section));
    let jobs: Vec<_> = (0..32usize)
        .map(|i| {
            if i.is_multiple_of(2) {
                LightJob::new(0, 0)
            } else {
                LightJob::new(1, 0)
            }
        })
        .collect();
    let expected: Vec<_> = [LightJob::new(0, 0), LightJob::new(1, 0)]
        .iter()
        .map(|&job| serial_engine(&world, job, min_section, max_section))
        .collect();
    let pool = Arc::new(LightPool::new(LightPoolConfig::new(
        8,
        min_section,
        max_section,
        true,
    )));
    let (send, receive) = mpsc::channel();
    let worker_world = Arc::clone(&world);
    let worker_pool = Arc::clone(&pool);
    let worker_jobs = jobs.clone();
    let handle = thread::spawn(move || {
        let result = worker_pool.relight(worker_world.as_ref(), &worker_jobs);
        let _ = send.send(result);
    });
    let snapshots = receive
        .recv_timeout(Duration::from_secs(30))
        .expect("pool deadlocked acquiring overlapping neighbourhood tokens")
        .expect("pool failed");
    handle.join().expect("pool thread panicked");
    for (job, snapshot) in jobs.iter().zip(&snapshots) {
        let index = usize::from(job.center_x != 0);
        for kind in [LightKind::Block, LightKind::Sky] {
            assert_snapshot_matches(&expected[index], snapshot, kind, "overlapping jobs");
        }
    }
}

#[test]
fn pooled_relight_matches_serial_incremental_over_random_terrain_and_changes() {
    let (min_section, max_section) = (0, 2);
    let mut world = random_world(99, -4, 4, max_section);
    let centres: Vec<_> = (-1..=1)
        .flat_map(|z| (-1..=1).map(move |x| (x, z)))
        .collect();
    let jobs = jobs(&centres);
    let mut serial: Vec<_> = jobs
        .iter()
        .map(|&job| serial_engine(&world, job, min_section, max_section))
        .collect();
    let materials = [
        "air",
        "stone",
        "torch",
        "water",
        "glass",
        "oak_leaves",
        "oak_slab",
    ];
    let mut seed = 0x5eed_1234u32;
    let mut changes = 0;
    for round in 0..8 {
        for _ in 0..2 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let chunk_x = ((seed >> 8) % 3) as i32 - 1;
            let chunk_z = ((seed >> 16) % 3) as i32 - 1;
            let x = chunk_x * 16 + ((seed >> 4) % 16) as i32;
            let z = chunk_z * 16 + ((seed >> 12) % 16) as i32;
            let y = 1 + ((seed >> 20) % 46) as i32;
            let name = materials[(seed >> 28) as usize % materials.len()];
            world.blocks.insert((x, y, z), state(name));
            for engine in &mut serial {
                engine.block_changed(&world, x, y, z);
            }
            changes += 1;
        }
        for workers in [1, 2, 4, 8] {
            let pool = LightPool::new(LightPoolConfig::new(
                workers,
                min_section,
                max_section,
                true,
            ));
            let snapshots = pool
                .relight(&world, &jobs)
                .unwrap_or_else(|error| panic!("round {round}, {workers} workers failed: {error}"));
            for (engine, snapshot) in serial.iter().zip(&snapshots) {
                for kind in [LightKind::Block, LightKind::Sky] {
                    assert_snapshot_matches(
                        engine,
                        snapshot,
                        kind,
                        &format!("round {round}, {workers} workers"),
                    );
                }
            }
        }
    }
    assert_eq!(changes, 16);
}

struct PanicWorld;
impl LightBlockView for PanicWorld {
    fn state(&self, _x: i32, _y: i32, _z: i32) -> u16 {
        panic!("block view exploded")
    }
}

#[test]
fn worker_panic_is_reported_and_the_pool_stays_usable() {
    let (min_section, max_section) = (0, 1);
    let pool = LightPool::new(LightPoolConfig::new(4, min_section, max_section, true));
    let jobs = jobs(&[(0, 0), (1, 0)]);
    let Err(error) = pool.relight(&PanicWorld, &jobs) else {
        panic!("a panicking block view must fail the batch");
    };
    assert!(matches!(error, LightPoolError::WorkerPanicked { .. }));
    assert!(pool.snapshot(0, 0).is_none(), "panic published a snapshot");

    let world = random_world(31, -3, 3, max_section);
    let snapshots = pool
        .relight(&world, &jobs)
        .expect("pool must stay usable after a worker panic");
    for (job, snapshot) in jobs.iter().zip(&snapshots) {
        let expected = serial_engine(&world, *job, min_section, max_section);
        for kind in [LightKind::Block, LightKind::Sky] {
            assert_snapshot_matches(&expected, snapshot, kind, "after panic");
        }
        assert_eq!(
            pool.snapshot(job.center_x, job.center_z).as_deref(),
            Some(snapshot.as_ref())
        );
    }
}
