//! Adversarial concurrency tests for [`LightPool`].
//!
//! `tests/pool.rs` covers parity, same-centre contention, one small overlapping
//! pair, panic recovery and a sequential random-edit stress. This file attacks
//! what those leave open:
//!
//! 1. randomized, heavily overlapping job sets with duplicate centres and
//!    negative coordinates, compared block-by-block against a serial engine;
//! 2. concurrent readers (and `clear_store`) while batches publish, checking
//!    that every observed snapshot is one whole publication and not a mix of
//!    two;
//! 3. job sequences whose centres are in opposite coordinate order, plus a
//!    cyclic quad, which a centre-first lock order would deadlock;
//! 4. a block view that panics for exactly one chosen job: `WorkerPanicked`,
//!    no publication for that job, every token released, pool still usable;
//! 5. two batches running on the same pool at once, checking each batch keeps
//!    its own results.
//!
//! Every pool interaction runs under a 60 s watchdog, so a deadlock fails the
//! test instead of hanging the run. The panic test needs an unwinding build:
//! the `test` profile unwinds, while the release profile sets
//! `panic = "abort"`.

use ferrite_light::{
    LightBlockView, LightEngine, LightJob, LightKind, LightPool, LightPoolConfig, LightPoolError,
    LightSnapshot, NibbleArray,
};
use std::{
    collections::{HashMap, HashSet},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

/// Chunk coordinate as `(chunk_x, chunk_z)`, matching the pool's API.
type Centre = (i32, i32);

const MIN_SECTION: i32 = 0;
const MAX_SECTION: i32 = 2;
/// Sections stored per snapshot: the buildable range plus one at each end.
const STORED_SECTIONS: usize = (MAX_SECTION - MIN_SECTION + 3) as usize;
const WORKER_COUNTS: [usize; 4] = [1, 2, 4, 8];
const WATCHDOG: Duration = Duration::from_secs(60);
/// Overlapping and negative centres used by the reader and concurrent-batch
/// tests. Every centre is within two chunks of the origin, so their 3x3
/// neighbourhoods all touch.
const ATOMICITY_CENTRES: [Centre; 9] = [
    (0, 0),
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (-1, -1),
    (2, -1),
    (-2, 2),
];

// ---------------------------------------------------------------------------
// Deterministic terrain
// ---------------------------------------------------------------------------

struct TestWorld {
    blocks: HashMap<(i32, i32, i32), u16>,
    air: u16,
    panic: Option<PanicRegion>,
}

#[derive(Clone, Copy)]
struct PanicRegion {
    min_x: i32,
    max_x: i32,
    min_z: i32,
    max_z: i32,
}

impl TestWorld {
    fn set(&mut self, x: i32, y: i32, z: i32, block: &str) {
        self.blocks.insert((x, y, z), state(block));
    }

    fn with_panic_region(mut self, region: PanicRegion) -> Self {
        self.panic = Some(region);
        self
    }
}

impl LightBlockView for TestWorld {
    fn state(&self, x: i32, y: i32, z: i32) -> u16 {
        if let Some(region) = self.panic
            && (region.min_x..=region.max_x).contains(&x)
            && (region.min_z..=region.max_z).contains(&z)
        {
            panic!("block view exploded at {x},{y},{z}");
        }
        *self.blocks.get(&(x, y, z)).unwrap_or(&self.air)
    }
}

fn state(name: &str) -> u16 {
    ferrite_data::blocks::default_state(name).unwrap_or_else(|| panic!("missing block {name}"))
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

/// Deterministic terrain over `from_chunk..=to_chunk`: stone ground with caves,
/// occasional water, and torches, glass and leaves on the surface. Blocks
/// outside the generated range are air and every chunk reports as loaded.
fn terrain(seed: u32, from_chunk: i32, to_chunk: i32) -> TestWorld {
    let (air, stone, water, torch, glass, leaves) = (
        state("air"),
        state("stone"),
        state("water"),
        state("torch"),
        state("glass"),
        state("oak_leaves"),
    );
    let top = (MAX_SECTION + 1) * 16 - 1;
    let mut blocks = HashMap::new();
    for chunk_z in from_chunk..=to_chunk {
        for chunk_x in from_chunk..=to_chunk {
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
    TestWorld {
        blocks,
        air,
        panic: None,
    }
}

/// Terrain plus, in every given centre chunk, an emitting torch and a single
/// opaque block above it. The torch changes block light and the stone changes
/// sky light, so a snapshot mixing the two light kinds from two publications
/// can never equal either whole publication.
fn lit_world(seed: u32, centres: &[Centre]) -> TestWorld {
    let mut world = terrain(seed, -3, 3);
    for &(chunk_x, chunk_z) in centres {
        world.set(chunk_x * 16 + 8, 32, chunk_z * 16 + 8, "torch");
        world.set(chunk_x * 16 + 3, 40, chunk_z * 16 + 5, "stone");
    }
    world
}

// ---------------------------------------------------------------------------
// Reference results
// ---------------------------------------------------------------------------

fn serial_engines(world: &TestWorld, jobs: &[LightJob]) -> HashMap<Centre, LightEngine> {
    let mut engines = HashMap::new();
    for job in jobs {
        engines.entry(job.center()).or_insert_with(|| {
            let mut engine =
                LightEngine::new(job.center_x, job.center_z, MIN_SECTION, MAX_SECTION, true);
            engine.relight(world);
            engine
        });
    }
    engines
}

/// Canonical snapshot values for a world, produced by a pool batch. Calling
/// this is only sound because the job result is a pure function of the centre
/// and the block view, which test 1 checks block-by-block against a serial
/// engine.
fn canonical_snapshots(world: &TestWorld, centres: &[Centre]) -> HashMap<Centre, LightSnapshot> {
    let pool = LightPool::new(LightPoolConfig::new(4, MIN_SECTION, MAX_SECTION, true));
    let jobs: Vec<LightJob> = centres.iter().map(|&(x, z)| LightJob::new(x, z)).collect();
    let published = pool.relight(world, &jobs).expect("canonical batch failed");
    let mut snapshots = HashMap::with_capacity(centres.len());
    for (job, snapshot) in jobs.iter().zip(&published) {
        assert!(
            snapshots
                .insert(job.center(), LightSnapshot::clone(snapshot.as_ref()))
                .is_none(),
            "duplicate canonical centre {:?}",
            job.center()
        );
    }
    snapshots
}

// ---------------------------------------------------------------------------
// Assertions
// ---------------------------------------------------------------------------

/// Compares a snapshot against a serial engine block by block, over every
/// stored section (the extra propagation sections included) and through the
/// public world-coordinate accessor.
fn assert_matches_engine(expected: &LightEngine, snapshot: &LightSnapshot, context: &str) {
    let (chunk_x, chunk_z) = snapshot.center();
    assert_eq!(
        snapshot.min_section(),
        MIN_SECTION,
        "{context}: min_section"
    );
    assert_eq!(
        snapshot.max_section(),
        MAX_SECTION,
        "{context}: max_section"
    );
    assert_eq!(
        snapshot.section_count(),
        STORED_SECTIONS,
        "{context}: stored section count"
    );
    for section_y in MIN_SECTION - 1..=MAX_SECTION + 1 {
        for kind in [LightKind::Sky, LightKind::Block] {
            let want = expected
                .section(kind, chunk_x, section_y, chunk_z)
                .unwrap_or_else(|| {
                    panic!("{context}: serial engine lacks {kind:?} section {section_y}")
                });
            let got = snapshot.section(kind, section_y).unwrap_or_else(|| {
                panic!("{context}: snapshot lacks {kind:?} section {section_y}")
            });
            assert_eq!(
                got, want,
                "{context}: {kind:?} section {section_y} of chunk {chunk_x},{chunk_z}"
            );
        }
    }
    for local_z in 0..16 {
        for local_x in 0..16 {
            let x = chunk_x * 16 + local_x;
            let z = chunk_z * 16 + local_z;
            for y in MIN_SECTION * 16..=(MAX_SECTION + 1) * 16 - 1 {
                for kind in [LightKind::Sky, LightKind::Block] {
                    assert_eq!(
                        snapshot.get(kind, x, y, z),
                        expected.get(kind, x, y, z),
                        "{context}: {kind:?} light at {x},{y},{z} of chunk {chunk_x},{chunk_z}"
                    );
                }
            }
        }
    }
    assert_eq!(
        snapshot.get(LightKind::Block, chunk_x * 16 - 1, 0, chunk_z * 16),
        None,
        "{context}: a block west of the chunk must read as outside"
    );
    assert_eq!(
        snapshot.get(LightKind::Sky, chunk_x * 16, 0, chunk_z * 16 + 16),
        None,
        "{context}: a block south of the chunk must read as outside"
    );
}

/// A snapshot handed to a concurrent reader must be exactly one publication:
/// well-formed section arrays and a value identical to one whole candidate.
fn assert_atomic(
    snapshot: &LightSnapshot,
    from_a: &LightSnapshot,
    from_b: &LightSnapshot,
    context: &str,
) {
    let centre = snapshot.center();
    assert_eq!(
        from_a.center(),
        centre,
        "{context}: candidate A is another chunk"
    );
    assert_eq!(
        from_b.center(),
        centre,
        "{context}: candidate B is another chunk"
    );
    assert_eq!(
        snapshot.min_section(),
        MIN_SECTION,
        "{context}: min_section"
    );
    assert_eq!(
        snapshot.max_section(),
        MAX_SECTION,
        "{context}: max_section"
    );
    assert_eq!(
        snapshot.section_count(),
        STORED_SECTIONS,
        "{context}: mixed section count"
    );
    for section_y in MIN_SECTION - 1..=MAX_SECTION + 1 {
        for kind in [LightKind::Sky, LightKind::Block] {
            let section = snapshot.section(kind, section_y).unwrap_or_else(|| {
                panic!("{context}: {kind:?} section {section_y} of {centre:?} is missing")
            });
            assert!(
                !matches!(section, NibbleArray::Null),
                "{context}: {kind:?} section {section_y} of {centre:?} is unloaded inside a publication"
            );
        }
    }
    assert!(
        snapshot == from_a || snapshot == from_b,
        "{context}: snapshot for {centre:?} is a mix of two publications"
    );
}

// ---------------------------------------------------------------------------
// Harnesses
// ---------------------------------------------------------------------------

/// Runs `body` on its own thread and fails the test if it does not finish
/// within [`WATCHDOG`]. Assertion failures inside `body` are re-raised with
/// their original payload.
fn watch<T: Send + 'static>(what: &str, body: impl FnOnce() -> T + Send + 'static) -> T {
    let (send, receive) = mpsc::channel();
    thread::spawn(move || {
        let outcome = catch_unwind(AssertUnwindSafe(body));
        let _ = send.send(outcome);
    });
    match receive.recv_timeout(WATCHDOG) {
        Ok(Ok(value)) => value,
        Ok(Err(payload)) => std::panic::resume_unwind(payload),
        Err(_) => panic!("{what}: no completion within {WATCHDOG:?}, probable deadlock"),
    }
}

/// Sets an `AtomicBool` when dropped, including while unwinding, so reader
/// threads stop even when the publishing thread panics.
struct StopOnDrop<'a>(&'a AtomicBool);
impl Drop for StopOnDrop<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

// ---------------------------------------------------------------------------
// Randomized job sets
// ---------------------------------------------------------------------------

struct Lcg(u64);
impl Lcg {
    fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }

    fn range(&mut self, low: i32, high: i32) -> i32 {
        low + (self.next_u32() % (high - low + 1) as u32) as i32
    }
}

/// Fixed-seed job list over `[-span, span]` with repeated centres, so batches
/// contain both maximal overlap and duplicate publication targets.
fn random_jobs(seed: u64, count: usize, span: i32) -> Vec<LightJob> {
    let mut rng = Lcg(seed);
    let mut jobs = Vec::with_capacity(count);
    while jobs.len() < count {
        let job = LightJob::new(rng.range(-span, span), rng.range(-span, span));
        jobs.push(job);
        if jobs.len() < count && rng.next_u32().is_multiple_of(3) {
            let index = (rng.next_u32() as usize) % jobs.len();
            jobs.push(jobs[index]);
        }
    }
    jobs
}

// ---------------------------------------------------------------------------
// 1. Randomized overlapping jobs
// ---------------------------------------------------------------------------

#[test]
fn randomized_overlapping_jobs_match_serial_at_every_worker_count() {
    const SPAN: i32 = 3;
    let jobs = random_jobs(0x00c0_ffee_0000_0001, 20, SPAN);
    let unique: HashSet<Centre> = jobs.iter().map(|job| job.center()).collect();
    assert!(
        jobs.len() - unique.len() >= 3,
        "fixture needs duplicate centres: {} jobs over {} centres",
        jobs.len(),
        unique.len()
    );
    assert!(
        jobs.iter().any(|job| job.center_x < 0),
        "fixture needs a negative x centre"
    );
    assert!(
        jobs.iter().any(|job| job.center_z < 0),
        "fixture needs a negative z centre"
    );

    watch("randomized overlapping jobs", move || {
        let world = Arc::new(terrain(0x5eed_0001, -SPAN - 1, SPAN + 1));
        let expected = serial_engines(&world, &jobs);
        let mut baseline: Option<Vec<Arc<LightSnapshot>>> = None;
        for workers in WORKER_COUNTS {
            let pool = Arc::new(LightPool::new(LightPoolConfig::new(
                workers,
                MIN_SECTION,
                MAX_SECTION,
                true,
            )));
            let snapshots = pool
                .relight(world.as_ref(), &jobs)
                .unwrap_or_else(|error| panic!("{workers} workers: {error}"));
            assert_eq!(snapshots.len(), jobs.len());

            for (job, snapshot) in jobs.iter().zip(&snapshots) {
                assert_eq!(
                    snapshot.center(),
                    job.center(),
                    "{workers} workers: result is in the wrong job slot"
                );
                assert_matches_engine(
                    &expected[&job.center()],
                    snapshot,
                    &format!("{workers} workers, randomized jobs"),
                );
            }

            let mut by_centre: HashMap<Centre, &Arc<LightSnapshot>> = HashMap::new();
            for (job, snapshot) in jobs.iter().zip(&snapshots) {
                if let Some(previous) = by_centre.insert(job.center(), snapshot) {
                    assert_eq!(
                        previous,
                        snapshot,
                        "{workers} workers: duplicate jobs for {:?} disagree",
                        job.center()
                    );
                }
            }
            for (job, snapshot) in jobs.iter().zip(&snapshots) {
                assert_eq!(
                    pool.snapshot(job.center_x, job.center_z).as_deref(),
                    Some(snapshot.as_ref()),
                    "{workers} workers: store lookup for {:?} is not the job result",
                    job.center()
                );
            }

            if let Some(previous) = baseline.as_ref() {
                assert_eq!(
                    previous, &snapshots,
                    "{workers} workers produced different bytes than one worker"
                );
            } else {
                baseline = Some(snapshots);
            }
        }
    });
}

// ---------------------------------------------------------------------------
// 2. Concurrent readers and clear_store
// ---------------------------------------------------------------------------

#[test]
fn snapshots_are_whole_publications_under_concurrent_readers_and_clear_store() {
    watch("concurrent readers and clear_store", move || {
        let jobs: Vec<LightJob> = ATOMICITY_CENTRES
            .iter()
            .map(|&(x, z)| LightJob::new(x, z))
            .collect();
        let world_a = Arc::new(terrain(0x0000_00a1, -3, 3));
        let world_b = Arc::new(lit_world(0x0000_00b2, &ATOMICITY_CENTRES));
        let expected_a = canonical_snapshots(&world_a, &ATOMICITY_CENTRES);
        let expected_b = canonical_snapshots(&world_b, &ATOMICITY_CENTRES);

        // Ground the candidate values in a serial engine and prove the two
        // worlds differ in both light kinds, otherwise a torn snapshot mixing
        // sky from one publication with block from the other could match a
        // candidate.
        let serial_a = serial_engines(&world_a, &jobs);
        let serial_b = serial_engines(&world_b, &jobs);
        for &centre in &ATOMICITY_CENTRES {
            assert_matches_engine(
                &serial_a[&centre],
                &expected_a[&centre],
                "canonical world A",
            );
            assert_matches_engine(
                &serial_b[&centre],
                &expected_b[&centre],
                "canonical world B",
            );
            for kind in [LightKind::Sky, LightKind::Block] {
                assert_ne!(
                    expected_a[&centre].section(kind, 2),
                    expected_b[&centre].section(kind, 2),
                    "worlds must differ in {kind:?} for {centre:?}"
                );
            }
        }

        for workers in WORKER_COUNTS {
            let pool = Arc::new(LightPool::new(LightPoolConfig::new(
                workers,
                MIN_SECTION,
                MAX_SECTION,
                true,
            )));

            // Phase 1: readers only. Snapshots persist, so every reader must
            // observe at least one and it must be a whole publication.
            let done = AtomicBool::new(false);
            thread::scope(|scope| {
                let publisher_guard = StopOnDrop(&done);
                let mut readers = Vec::new();
                for _ in 0..3 {
                    let pool = Arc::clone(&pool);
                    let done = &done;
                    let expected_a = &expected_a;
                    let expected_b = &expected_b;
                    readers.push(scope.spawn(move || {
                        let mut seen = 0usize;
                        loop {
                            for &(x, z) in &ATOMICITY_CENTRES {
                                if let Some(snapshot) = pool.snapshot(x, z) {
                                    assert_atomic(
                                        &snapshot,
                                        &expected_a[&(x, z)],
                                        &expected_b[&(x, z)],
                                        "reader",
                                    );
                                    seen += 1;
                                }
                            }
                            if done.load(Ordering::Acquire) && seen > 0 {
                                break;
                            }
                            thread::yield_now();
                        }
                        seen
                    }));
                }
                for round in 0..6 {
                    let (world, expected) = if round % 2 == 0 {
                        (&world_a, &expected_a)
                    } else {
                        (&world_b, &expected_b)
                    };
                    let snapshots = pool
                        .relight(world.as_ref(), &jobs)
                        .unwrap_or_else(|error| panic!("round {round}: {error}"));
                    for (snapshot, &centre) in snapshots.iter().zip(ATOMICITY_CENTRES.iter()) {
                        assert_eq!(
                            snapshot.as_ref(),
                            &expected[&centre],
                            "round {round}: batch result is not its own world's publication"
                        );
                    }
                }
                drop(publisher_guard);
                for reader in readers {
                    let seen = reader.join().expect("reader thread panicked");
                    assert!(
                        seen > 0,
                        "{workers} workers: a reader never observed a published snapshot"
                    );
                }
            });

            // Phase 2: a thread clears the store while batches publish and
            // readers read. A publication may be dropped by the clear, so only
            // atomicity and the absence of panics are asserted here.
            let stop = AtomicBool::new(false);
            thread::scope(|scope| {
                let stop_guard = StopOnDrop(&stop);
                let mut readers = Vec::new();
                for _ in 0..2 {
                    let pool = Arc::clone(&pool);
                    let stop = &stop;
                    let expected_a = &expected_a;
                    let expected_b = &expected_b;
                    readers.push(scope.spawn(move || {
                        while !stop.load(Ordering::Acquire) {
                            for &(x, z) in &ATOMICITY_CENTRES {
                                if let Some(snapshot) = pool.snapshot(x, z) {
                                    assert_atomic(
                                        &snapshot,
                                        &expected_a[&(x, z)],
                                        &expected_b[&(x, z)],
                                        "reader during clear_store",
                                    );
                                }
                            }
                            thread::yield_now();
                        }
                    }));
                }
                let clearer_pool = Arc::clone(&pool);
                let clearer_stop = &stop;
                let clearer = scope.spawn(move || {
                    let mut clears = 0usize;
                    while !clearer_stop.load(Ordering::Acquire) {
                        clearer_pool.clear_store();
                        clears += 1;
                        thread::yield_now();
                    }
                    clears
                });
                for round in 0..4 {
                    let world = if round % 2 == 0 { &world_a } else { &world_b };
                    pool.relight(world.as_ref(), &jobs)
                        .unwrap_or_else(|error| panic!("batch during clear_store: {error}"));
                }
                drop(stop_guard);
                for reader in readers {
                    reader.join().expect("reader thread panicked");
                }
                let clears = clearer.join().expect("clear_store thread panicked");
                assert!(
                    clears > 0,
                    "{workers} workers: the clearer never ran, so phase 2 is vacuous"
                );
            });

            // Phase 3: a quiescent clear drops every publication and the next
            // batch republishes it.
            pool.clear_store();
            for &(x, z) in &ATOMICITY_CENTRES {
                assert!(
                    pool.snapshot(x, z).is_none(),
                    "{workers} workers: clear_store left a snapshot for {x},{z}"
                );
            }
            let snapshots = pool
                .relight(world_a.as_ref(), &jobs)
                .expect("relight after clear_store failed");
            for (snapshot, &centre) in snapshots.iter().zip(ATOMICITY_CENTRES.iter()) {
                assert_eq!(
                    snapshot.as_ref(),
                    &expected_a[&centre],
                    "{workers} workers: republished snapshot differs"
                );
                assert_eq!(
                    pool.snapshot(centre.0, centre.1).as_deref(),
                    Some(snapshot.as_ref()),
                    "{workers} workers: store lookup after clear_store"
                );
            }
        }
    });
}

// ---------------------------------------------------------------------------
// 3. Lock order
// ---------------------------------------------------------------------------

#[test]
fn opposite_order_and_cyclic_centres_complete_without_deadlock() {
    watch("opposite-order centres", move || {
        let world = Arc::new(terrain(0x0dd1_0003, -3, 3));
        let mut jobs = Vec::new();
        // Alternating neighbours in both coordinate orders. A centre-first
        // lock order takes (0,0) then (1,0) for one job and (1,0) then (0,0)
        // for the other, which is a two-lock cycle.
        for _ in 0..8 {
            jobs.push(LightJob::new(0, 0));
            jobs.push(LightJob::new(1, 0));
        }
        for _ in 0..8 {
            jobs.push(LightJob::new(1, 0));
            jobs.push(LightJob::new(0, 0));
        }
        // The same reversal on the z axis.
        for _ in 0..8 {
            jobs.push(LightJob::new(0, 0));
            jobs.push(LightJob::new(0, 1));
        }
        // A negative centre whose neighbourhood extends to a higher x.
        for _ in 0..6 {
            jobs.push(LightJob::new(-1, 0));
            jobs.push(LightJob::new(0, 0));
        }
        // A cyclic quad: centre-first in this order is a four-lock cycle.
        for _ in 0..5 {
            for centre in [(0, 0), (1, 0), (1, 1), (0, 1)] {
                jobs.push(LightJob::new(centre.0, centre.1));
            }
        }
        assert_eq!(jobs.len(), 80);
        let expected = serial_engines(&world, &jobs);

        for workers in WORKER_COUNTS {
            let pool = LightPool::new(LightPoolConfig::new(
                workers,
                MIN_SECTION,
                MAX_SECTION,
                true,
            ));
            let snapshots = pool
                .relight(world.as_ref(), &jobs)
                .unwrap_or_else(|error| panic!("{workers} workers: {error}"));
            for (job, snapshot) in jobs.iter().zip(&snapshots) {
                assert_matches_engine(
                    &expected[&job.center()],
                    snapshot,
                    &format!("{workers} workers, opposite-order jobs"),
                );
            }
        }
    });
}

// ---------------------------------------------------------------------------
// 4. Panic path
// ---------------------------------------------------------------------------

#[test]
fn panicking_job_publishes_nothing_and_leaves_the_pool_usable() {
    watch("panic path", move || {
        let healthy = Arc::new(terrain(0x9a11_0004, 0, 8));
        // The engine for centre (6, 0) reads chunks 5..=7, i.e. x 80..=127 and
        // z -16..=31. No other centre in the batch reads that range.
        let exploding = Arc::new(terrain(0x9a11_0004, 0, 8).with_panic_region(PanicRegion {
            min_x: 80,
            max_x: 127,
            min_z: -16,
            max_z: 31,
        }));
        let target = (6, 0);
        let jobs = vec![
            LightJob::new(target.0, target.1),
            LightJob::new(0, 0),
            LightJob::new(3, 0),
            LightJob::new(target.0, target.1),
        ];
        let expected = serial_engines(&healthy, &jobs);

        for workers in WORKER_COUNTS {
            let pool = Arc::new(LightPool::new(LightPoolConfig::new(
                workers,
                MIN_SECTION,
                MAX_SECTION,
                true,
            )));
            // Seed the store with a good publication for the centre that will
            // panic, so "publishes nothing" also means "does not clobber the
            // previous publication".
            let seeded = pool
                .relight(healthy.as_ref(), &[LightJob::new(target.0, target.1)])
                .expect("seed batch failed");
            assert_eq!(
                pool.snapshot(target.0, target.1).as_deref(),
                Some(seeded[0].as_ref())
            );

            let error = pool
                .relight(exploding.as_ref(), &jobs)
                .expect_err("a panicking block view must fail the batch");
            let (job, message) = match error {
                LightPoolError::WorkerPanicked { job, message } => (job, message),
            };
            assert_eq!(
                jobs[job].center(),
                target,
                "panic reported for the wrong job"
            );
            assert!(
                message.contains("block view exploded"),
                "panic payload was lost: {message}"
            );
            assert_eq!(
                pool.snapshot(target.0, target.1).as_deref(),
                Some(seeded[0].as_ref()),
                "{workers} workers: the panicking job changed the published snapshot"
            );
            // Jobs that did not panic may still have completed (they always
            // do with more than one worker); whatever they published must be
            // their own correct result, never a partial one.
            for centre in [(0, 0), (3, 0)] {
                if let Some(snapshot) = pool.snapshot(centre.0, centre.1) {
                    assert_matches_engine(
                        &expected[&centre],
                        &snapshot,
                        &format!("{workers} workers, job that did not panic"),
                    );
                }
            }

            // This batch must acquire all nine tokens of (6, 0) again. With one
            // worker a token left held by the panicking job would deadlock and
            // trip the watchdog.
            let recovered = pool
                .relight(healthy.as_ref(), &jobs)
                .expect("pool unusable after a worker panic");
            for (job, snapshot) in jobs.iter().zip(&recovered) {
                assert_matches_engine(
                    &expected[&job.center()],
                    snapshot,
                    &format!("{workers} workers after a panic"),
                );
            }
            let single = pool
                .relight(healthy.as_ref(), &[LightJob::new(target.0, target.1)])
                .expect("tokens of the panicking centre were not released");
            assert_matches_engine(&expected[&target], &single[0], "single job after a panic");
            assert_eq!(
                pool.snapshot(target.0, target.1).as_deref(),
                Some(single[0].as_ref())
            );
        }
    });
}

// ---------------------------------------------------------------------------
// 5. Concurrent batches on one pool
// ---------------------------------------------------------------------------

#[test]
fn concurrent_batches_on_one_pool_keep_their_own_results() {
    watch("concurrent batches", move || {
        let jobs: Vec<LightJob> = ATOMICITY_CENTRES
            .iter()
            .map(|&(x, z)| LightJob::new(x, z))
            .collect();
        let world_a = Arc::new(terrain(0x0000_0a11, -3, 3));
        let world_b = Arc::new(lit_world(0x0000_0b22, &ATOMICITY_CENTRES));
        let expected_a = canonical_snapshots(&world_a, &ATOMICITY_CENTRES);
        let expected_b = canonical_snapshots(&world_b, &ATOMICITY_CENTRES);
        for &centre in &ATOMICITY_CENTRES {
            assert_ne!(
                &expected_a[&centre], &expected_b[&centre],
                "the two worlds must relight {centre:?} differently"
            );
        }

        const ROUNDS: usize = 3;
        for workers in WORKER_COUNTS {
            let pool = Arc::new(LightPool::new(LightPoolConfig::new(
                workers,
                MIN_SECTION,
                MAX_SECTION,
                true,
            )));
            // `relight` takes `&self` and the pool is `Sync`, so two batches
            // can run at once. Each must return its own world's values.
            let (from_a, from_b) = thread::scope(|scope| {
                let a = scope.spawn(|| {
                    let mut batches = Vec::with_capacity(ROUNDS);
                    for _ in 0..ROUNDS {
                        batches.push(
                            pool.relight(world_a.as_ref(), &jobs)
                                .expect("batch A failed"),
                        );
                    }
                    batches
                });
                let b = scope.spawn(|| {
                    let mut batches = Vec::with_capacity(ROUNDS);
                    for _ in 0..ROUNDS {
                        batches.push(
                            pool.relight(world_b.as_ref(), &jobs)
                                .expect("batch B failed"),
                        );
                    }
                    batches
                });
                (
                    a.join().expect("batch A thread panicked"),
                    b.join().expect("batch B thread panicked"),
                )
            });
            for (batches, expected) in [(&from_a, &expected_a), (&from_b, &expected_b)] {
                for batch in batches {
                    for (snapshot, &centre) in batch.iter().zip(ATOMICITY_CENTRES.iter()) {
                        assert_eq!(
                            snapshot.as_ref(),
                            &expected[&centre],
                            "{workers} workers: a batch returned another batch's publication"
                        );
                    }
                }
            }
            for &centre in &ATOMICITY_CENTRES {
                let stored = pool
                    .snapshot(centre.0, centre.1)
                    .unwrap_or_else(|| panic!("{workers} workers: no publication for {centre:?}"));
                assert!(
                    stored.as_ref() == &expected_a[&centre]
                        || stored.as_ref() == &expected_b[&centre],
                    "{workers} workers: stored snapshot for {centre:?} is a mix of both batches"
                );
            }
        }
    });
}
