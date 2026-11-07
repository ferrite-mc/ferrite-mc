//! Per-region deferred topology mutations.
use ferrite_region::{ChunkPos, RegionData, Regionizer, Scheduler, SectionPos};
use std::collections::{HashMap, HashSet};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc,
};
use std::thread;
use std::time::{Duration, Instant};

/// Region data tracks the sections it was seeded with, so merges are visible.
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

/// Blocks the region owning `chunk` inside a task until the returned sender fires.
fn block_region(scheduler: &Scheduler<Data, ()>, chunk: ChunkPos) -> mpsc::Sender<()> {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    scheduler.schedule_at_chunk(chunk, move |_| {
        entered_tx.send(()).unwrap();
        release_rx.recv().unwrap();
    });
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    release_tx
}

#[test]
fn mutations_near_ticking_region_do_not_delay_idle_region() {
    // A at x=0, B at x=100, C at x=104 (radius 1, one chunk per section).
    let mut topology = Regionizer::<Data>::new(0, 1);
    for x in [0, 100, 104] {
        topology.add_chunk(ChunkPos(x, 0));
    }
    let scheduler = Scheduler::new(topology, (), 3, |_| {}, |_| {});
    let release = block_region(&scheduler, ChunkPos(0, 0));

    // Sections added beside A receive transient owners while A ticks.
    scheduler.add_chunk(ChunkPos(1, 0));
    scheduler.add_chunk(ChunkPos(-1, 1));
    let added = scheduler.owner(ChunkPos(1, 0)).expect("transient owner");
    assert_eq!(
        scheduler.region_state(added),
        Some(ferrite_region::RegionState::Transient)
    );

    // Near B/C only: applied at once, or at most after B's/C's own in-flight
    // (no-op) tick, but never after A's tick, which stays blocked throughout.
    let eventually = |what: &str, cond: &dyn Fn() -> bool| {
        let start = Instant::now();
        while !cond() {
            assert!(
                start.elapsed() < Duration::from_millis(200),
                "{what} was delayed by the ticking region"
            );
            thread::yield_now();
        }
    };
    let b = scheduler.owner(ChunkPos(100, 0)).unwrap();
    scheduler.add_chunk(ChunkPos(101, 0));
    eventually("add near B", &|| {
        scheduler.owner(ChunkPos(101, 0)) == Some(b)
    });
    // Bridge B and C (both idle): merges without waiting for A.
    scheduler.add_chunk(ChunkPos(102, 0));
    scheduler.add_chunk(ChunkPos(103, 0));
    eventually("B-C merge", &|| scheduler.region_count() == 4);
    assert_eq!(
        scheduler.owner(ChunkPos(104, 0)),
        scheduler.owner(ChunkPos(100, 0))
    );
    // ... and splits without waiting for A.
    scheduler.remove_chunk(ChunkPos(102, 0));
    eventually("B-C split", &|| scheduler.region_count() == 5);

    assert_eq!(scheduler.owner(ChunkPos(1, 0)), Some(added));
    scheduler.check_invariants().unwrap();

    // B keeps ticking while A is stuck.
    let (tx, rx) = mpsc::channel();
    scheduler.schedule_at_chunk(ChunkPos(100, 0), move |_| tx.send(()).unwrap());
    rx.recv_timeout(Duration::from_millis(500))
        .expect("idle region was delayed by a ticking one");

    release.send(()).unwrap();
    let start = Instant::now();
    while scheduler.owner(ChunkPos(1, 0)) != scheduler.owner(ChunkPos(0, 0))
        || scheduler.owner(ChunkPos(-1, 1)) != scheduler.owner(ChunkPos(0, 0))
    {
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "A's mutations never applied"
        );
        thread::yield_now();
    }
    assert_eq!(
        scheduler.owner(ChunkPos(1, 0)),
        scheduler.owner(ChunkPos(0, 0))
    );
    assert_eq!(
        scheduler.owner(ChunkPos(-1, 1)),
        scheduler.owner(ChunkPos(0, 0))
    );
    scheduler.check_invariants().unwrap();
    assert!(scheduler.shutdown().pending_tasks.is_empty());
}

#[test]
fn merge_of_ticking_and_idle_region_applies_right_after_the_tick() {
    let a = SectionPos(0, 0);
    let b = SectionPos(2, 0);
    let mut topology = Regionizer::<Data>::new(0, 1);
    topology.add_chunk(ChunkPos(0, 0));
    topology.add_chunk(ChunkPos(2, 0));
    let ia = topology.owner(ChunkPos(0, 0)).unwrap();
    let ib = topology.owner(ChunkPos(2, 0)).unwrap();
    topology.data_mut(ia).unwrap().0.insert(a);
    topology.data_mut(ib).unwrap().0.insert(b);

    // Tick log: (start time, sections in the ticking region's data).
    let log = Arc::new(Mutex::new(Vec::<(Instant, HashSet<SectionPos>)>::new()));
    let tick_log = log.clone();
    let scheduler = Scheduler::new(
        topology,
        (),
        3,
        move |data: &mut Data| {
            tick_log
                .lock()
                .unwrap()
                .push((Instant::now(), data.0.clone()));
        },
        |_| {},
    );
    let release = block_region(&scheduler, ChunkPos(0, 0));
    scheduler.add_chunk(ChunkPos(1, 0));
    let submitted = Instant::now();
    // B may be in the middle of its own (no-op) tick right now. Then the bridge is a third,
    // separate region until that tick ends and B merges into it, so 2 and 3 are both fine; what
    // must not happen is the merge into one region while A's tick is still blocked.
    assert!(
        (2..=3).contains(&scheduler.region_count()),
        "merge must wait for A's tick"
    );
    let bridge = scheduler.owner(ChunkPos(1, 0)).unwrap();
    assert_eq!(
        scheduler.region_state(bridge),
        Some(ferrite_region::RegionState::Transient)
    );
    // B is part of a transient region: it must not start new ticks.
    thread::sleep(Duration::from_millis(150));
    assert_eq!(scheduler.region_count(), 2);

    let released = Instant::now();
    release.send(()).unwrap();
    while scheduler.region_count() != 1 {
        assert!(
            released.elapsed() < Duration::from_secs(1),
            "merge never applied"
        );
        thread::yield_now();
    }
    let merged_at = released.elapsed();
    // Wait for a tick of the merged region.
    let start = Instant::now();
    loop {
        if log.lock().unwrap().iter().any(|(_, d)| d.len() == 2) {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(2));
        thread::yield_now();
    }
    let log = log.lock().unwrap();
    let b_only_after_submit = log
        .iter()
        .filter(|(t, d)| *t > submitted && *d == HashSet::from([b]))
        .count();
    let a_only_after_release = log
        .iter()
        .filter(|(t, d)| *t > released && *d == HashSet::from([a]))
        .count();
    // At most one B tick could already have been dispatched when the merge was
    // submitted; none may start afterwards.
    assert!(b_only_after_submit <= 1, "reserved region kept ticking");
    // Exactly the tick that finished A's blocked work ran unmerged.
    assert_eq!(
        a_only_after_release, 1,
        "merge was not applied at A's safe point"
    );
    assert!(
        merged_at < Duration::from_millis(50),
        "merge took {merged_at:?}, more than one tick period"
    );
    drop(log);
    assert_eq!(scheduler.shutdown().regions.len(), 1);
}

#[test]
fn pending_merge_chain_across_three_regions_applies_in_order() {
    // All three regions tick while the bridges are added. Removing the first
    // bridge remains ordered after its addition, even with both merges deferred.
    let mut topology = Regionizer::<Data>::new(0, 1);
    for x in [0, 2, 4] {
        topology.add_chunk(ChunkPos(x, 0));
    }
    let scheduler = Scheduler::new(topology, (), 3, |_| {}, |_| {});
    let release = block_region(&scheduler, ChunkPos(0, 0));
    let release_b = block_region(&scheduler, ChunkPos(2, 0));
    let release_c = block_region(&scheduler, ChunkPos(4, 0));
    scheduler.add_chunk(ChunkPos(1, 0));
    scheduler.add_chunk(ChunkPos(3, 0));
    scheduler.remove_chunk(ChunkPos(1, 0));
    assert_eq!(scheduler.region_count(), 5);
    scheduler.check_invariants().unwrap();
    release.send(()).unwrap();
    release_b.send(()).unwrap();
    release_c.send(()).unwrap();
    let start = Instant::now();
    while scheduler.owner(ChunkPos(1, 0)).is_some()
        || scheduler.owner(ChunkPos(2, 0)) != scheduler.owner(ChunkPos(4, 0))
        || scheduler.region_count() != 2
    {
        assert!(start.elapsed() < Duration::from_secs(2));
        thread::yield_now();
    }
    scheduler.check_invariants().unwrap();
    assert_eq!(scheduler.owner(ChunkPos(1, 0)), None);
    assert_eq!(scheduler.region_count(), 2);
    assert_eq!(
        scheduler.owner(ChunkPos(2, 0)),
        scheduler.owner(ChunkPos(4, 0))
    );
    scheduler.shutdown();
}

/// Concurrent adds/removes/tasks from 4 threads while regions tick (some slowly),
/// checking invariants continuously and task exactly-once delivery at the end.
fn stress(ops_per_thread: usize, area: i32) {
    // One chunk per section, so the sparse random population forms many regions.
    let topology = Regionizer::<Data>::new(0, 1);
    let ticks = Arc::new(AtomicU64::new(0));
    let tick_counter = ticks.clone();
    let scheduler = Arc::new(Scheduler::new(
        topology,
        (),
        4,
        move |_: &mut Data| {
            let n = tick_counter.fetch_add(1, Ordering::Relaxed);
            let work = if n.is_multiple_of(37) {
                Duration::from_millis(3)
            } else {
                Duration::from_micros(50)
            };
            let start = Instant::now();
            while start.elapsed() < work {
                std::hint::spin_loop();
            }
        },
        |_| {},
    ));
    let (tx, rx) = mpsc::channel::<(usize, usize)>();
    let ran = Arc::new(Mutex::new(HashMap::<(usize, usize), usize>::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let checker = {
        let scheduler = scheduler.clone();
        let stop = stop.clone();
        thread::spawn(move || {
            let mut checks = 0;
            let mut max_age = Duration::ZERO;
            while !stop.load(Ordering::Relaxed) {
                scheduler.check_invariants().unwrap();
                if let Some(age) = scheduler.oldest_pending_mutation() {
                    max_age = max_age.max(age);
                    assert!(age < Duration::from_secs(5), "mutation starved for {age:?}");
                }
                checks += 1;
                thread::sleep(Duration::from_millis(2));
            }
            (checks, max_age)
        })
    };
    let ops_start = Instant::now();
    let workers: Vec<_> = (0..4)
        .map(|worker| {
            let scheduler = scheduler.clone();
            let tx = tx.clone();
            let ran = ran.clone();
            thread::spawn(move || {
                let mut held: Vec<ChunkPos> = Vec::new();
                let mut seed = worker as u64 * 7919 + 17;
                let mut next = move || {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    seed
                };
                let mut tasks = 0;
                let mut deferred = 0;
                for _ in 0..ops_per_thread {
                    let r = next();
                    let chunk = ChunkPos((r % area as u64) as i32, ((r >> 8) % area as u64) as i32);
                    match (r >> 20) % 4 {
                        0 | 1 => {
                            scheduler.add_chunk(chunk);
                            if scheduler.owner(chunk).is_none() {
                                deferred += 1;
                            }
                            held.push(chunk);
                        }
                        2 if !held.is_empty() => {
                            let i = (r >> 24) as usize % held.len();
                            scheduler.remove_chunk(held.swap_remove(i));
                        }
                        _ => {
                            let tx = tx.clone();
                            let ran = ran.clone();
                            let key = (worker, tasks);
                            tasks += 1;
                            scheduler.schedule_at_chunk(chunk, move |_| {
                                *ran.lock().unwrap().entry(key).or_default() += 1;
                                tx.send(key).unwrap();
                            });
                        }
                    }
                }
                (held, tasks, deferred)
            })
        })
        .collect();
    drop(tx);
    let mut total_tasks = 0;
    let mut deferred_adds = 0;
    let mut held_all = Vec::new();
    for w in workers {
        let (held, tasks, deferred) = w.join().unwrap();
        total_tasks += tasks;
        deferred_adds += deferred;
        held_all.extend(held);
    }
    let ops_time = ops_start.elapsed();
    // Hold every section in the area so tasks parked on unheld sections run.
    for x in 0..area {
        for z in 0..area {
            scheduler.add_chunk(ChunkPos(x, z));
        }
    }
    let mut received = HashSet::new();
    for _ in 0..total_tasks {
        let key = rx.recv_timeout(Duration::from_secs(20)).expect("task lost");
        assert!(received.insert(key), "task {key:?} ran twice");
    }
    let start = Instant::now();
    while scheduler.pending_mutations() != 0 {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "mutations starved"
        );
        thread::sleep(Duration::from_millis(1));
    }
    stop.store(true, Ordering::Relaxed);
    let (checks, max_age) = checker.join().unwrap();
    scheduler.check_invariants().unwrap();
    // The whole area is held and contiguous: exactly one region.
    assert_eq!(scheduler.region_count(), 1);
    let owner = scheduler.owner(ChunkPos(0, 0)).unwrap();
    for c in &held_all {
        assert_eq!(scheduler.owner(*c), Some(owner));
    }
    // Release everything: no region may remain.
    let teardown = Instant::now();
    for x in 0..area {
        for z in 0..area {
            scheduler.remove_chunk(ChunkPos(x, z));
        }
    }
    for c in held_all {
        scheduler.remove_chunk(c);
    }
    let scheduler = Arc::try_unwrap(scheduler).ok().unwrap();
    scheduler.check_invariants().unwrap();
    let result = scheduler.shutdown();
    let teardown = teardown.elapsed();
    assert!(result.pending_tasks.is_empty());
    assert!(ran.lock().unwrap().values().all(|&n| n == 1));
    assert_eq!(ran.lock().unwrap().len(), total_tasks);
    eprintln!(
        "stress: {total_tasks} tasks, {} ticks, {checks} invariant checks, >= {deferred_adds} deferred adds, oldest pending mutation {max_age:?}, ops phase {ops_time:?}, teardown {teardown:?}",
        ticks.load(Ordering::Relaxed)
    );
}

#[test]
fn concurrent_stress_small() {
    stress(2_000, 24);
}

#[test]
#[ignore = "long-running concurrent stress test"]
fn concurrent_stress_large() {
    stress(100_000, 64);
}
#[test]
fn added_section_beside_ticking_region_is_transient_until_merge() {
    use ferrite_region::RegionState;
    let mut topology = Regionizer::<Data>::new(0, 1);
    topology.add_chunk(ChunkPos(0, 0));
    let scheduler = Scheduler::new(topology, (), 2, |_| {}, |_| {});
    let release = block_region(&scheduler, ChunkPos(0, 0));
    let original = scheduler.owner(ChunkPos(0, 0)).unwrap();
    scheduler.add_chunk(ChunkPos(1, 0));
    let added = scheduler
        .owner(ChunkPos(1, 0))
        .expect("section is owned immediately");
    assert_ne!(original, added);
    assert_eq!(scheduler.region_state(added), Some(RegionState::Transient));
    let (tx, rx) = mpsc::channel();
    scheduler.schedule_at_chunk(ChunkPos(1, 0), move |_| {
        tx.send(()).unwrap();
    });
    assert!(rx.recv_timeout(Duration::from_millis(30)).is_err());
    release.send(()).unwrap();
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(
        scheduler.owner(ChunkPos(0, 0)),
        scheduler.owner(ChunkPos(1, 0))
    );
    assert!(scheduler.shutdown().pending_tasks.is_empty());
}
#[test]
fn transient_bridge_waits_for_both_ticking_regions() {
    use ferrite_region::RegionState;
    let mut topology = Regionizer::<Data>::new(0, 1);
    topology.add_chunk(ChunkPos(0, 0));
    topology.add_chunk(ChunkPos(2, 0));
    let scheduler = Scheduler::new(topology, (), 3, |_| {}, |_| {});
    let left = block_region(&scheduler, ChunkPos(0, 0));
    let right = block_region(&scheduler, ChunkPos(2, 0));
    scheduler.add_chunk(ChunkPos(1, 0));
    let bridge = scheduler
        .owner(ChunkPos(1, 0))
        .expect("bridge is owned immediately");
    assert_eq!(scheduler.region_state(bridge), Some(RegionState::Transient));
    let (tx, rx) = mpsc::channel();
    scheduler.schedule_at_chunk(ChunkPos(1, 0), move |_| tx.send(()).unwrap());
    left.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while scheduler.owner(ChunkPos(0, 0)) != scheduler.owner(ChunkPos(1, 0)) {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    assert_eq!(
        scheduler.region_state(scheduler.owner(ChunkPos(1, 0)).unwrap()),
        Some(RegionState::Transient)
    );
    assert!(rx.recv_timeout(Duration::from_millis(30)).is_err());
    right.send(()).unwrap();
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(
        scheduler.owner(ChunkPos(0, 0)),
        scheduler.owner(ChunkPos(2, 0))
    );
    assert!(scheduler.shutdown().pending_tasks.is_empty());
}
#[test]
fn bridge_absorbs_idle_neighbor_while_other_region_ticks() {
    use ferrite_region::RegionState;
    let mut topology = Regionizer::<Data>::new(0, 1);
    topology.add_chunk(ChunkPos(0, 0));
    topology.add_chunk(ChunkPos(2, 0));
    let scheduler = Scheduler::new(topology, (), 2, |_| {}, |_| {});
    let release = block_region(&scheduler, ChunkPos(0, 0));
    // Occupy the other worker as well, so the neighboring region cannot start
    // a tick between checking its state and adding the bridge.
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_global_tx, release_global_rx) = mpsc::channel();
    scheduler.schedule_global(move |_| {
        entered_tx.send(()).unwrap();
        release_global_rx.recv().unwrap();
    });
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let neighbor = scheduler.owner(ChunkPos(2, 0)).unwrap();
    assert_eq!(scheduler.region_state(neighbor), Some(RegionState::Ready));
    scheduler.add_chunk(ChunkPos(1, 0));
    let bridge = scheduler
        .owner(ChunkPos(1, 0))
        .expect("bridge is owned immediately");
    assert_eq!(scheduler.region_state(bridge), Some(RegionState::Transient));
    assert_eq!(scheduler.owner(ChunkPos(2, 0)), Some(bridge));
    release_global_tx.send(()).unwrap();
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while scheduler.owner(ChunkPos(0, 0)) != scheduler.owner(ChunkPos(2, 0)) {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    assert!(scheduler.shutdown().pending_tasks.is_empty());
}
#[test]
fn removal_of_ticking_chunk_updates_holder_count_immediately() {
    let mut topology = Regionizer::<Data>::new(0, 1);
    topology.add_chunk(ChunkPos(0, 0));
    let scheduler = Scheduler::new(topology, (), 2, |_| {}, |_| {});
    let release = block_region(&scheduler, ChunkPos(0, 0));
    scheduler.remove_chunk(ChunkPos(0, 0));
    assert_eq!(scheduler.holder_count(ChunkPos(0, 0)), 0);
    scheduler.add_chunk(ChunkPos(0, 0));
    assert_eq!(scheduler.holder_count(ChunkPos(0, 0)), 1);
    release.send(()).unwrap();
    assert!(scheduler.shutdown().pending_tasks.is_empty());
}
#[test]
fn dead_section_is_retired_after_its_tick() {
    let mut topology = Regionizer::<Data>::new(0, 1);
    topology.add_chunk(ChunkPos(0, 0));
    let scheduler = Scheduler::new(topology, (), 2, |_| {}, |_| {});
    let release = block_region(&scheduler, ChunkPos(0, 0));
    scheduler.remove_chunk(ChunkPos(0, 0));
    assert_eq!(scheduler.holder_count(ChunkPos(0, 0)), 0);
    scheduler.check_invariants().unwrap();
    release.send(()).unwrap();
    let until = Instant::now() + Duration::from_secs(2);
    while scheduler.region_count() != 0 {
        assert!(Instant::now() < until);
        thread::yield_now();
    }
    assert!(scheduler.shutdown().pending_tasks.is_empty());
}
#[test]
fn ticking_region_retains_existing_empty_sections_and_new_halo_is_transient() {
    use ferrite_region::{RegionState, RegionizerConfig};
    let mut topology = Regionizer::<Data>::with_config(RegionizerConfig {
        section_shift: 0,
        merge_radius: 1,
        empty_section_radius: 1,
        min_section_recalc_count: 2,
        max_dead_section_percent: 0.5,
    });
    topology.add_chunk(ChunkPos(0, 0));
    let scheduler = Scheduler::new(topology, (), 2, |_| {}, |_| {});
    let release = block_region(&scheduler, ChunkPos(0, 0));
    let original = scheduler.owner(ChunkPos(0, 0));
    scheduler.add_chunk(ChunkPos(2, 0));
    let added = scheduler.owner(ChunkPos(2, 0)).unwrap();
    assert_eq!(scheduler.region_state(added), Some(RegionState::Transient));
    assert_eq!(scheduler.owner(ChunkPos(1, 0)), original);
    assert_eq!(scheduler.owner(ChunkPos(3, 0)), Some(added));
    release.send(()).unwrap();
    let until = Instant::now() + Duration::from_secs(2);
    while scheduler.owner(ChunkPos(2, 0)) != scheduler.owner(ChunkPos(0, 0)) {
        assert!(Instant::now() < until);
        thread::yield_now();
    }
    scheduler.shutdown();
}
#[test]
fn concurrent_halo_mutations_preserve_region_invariants() {
    use ferrite_region::RegionizerConfig;
    let topology = Regionizer::<Data>::with_config(RegionizerConfig {
        section_shift: 0,
        merge_radius: 1,
        empty_section_radius: 1,
        min_section_recalc_count: 2,
        max_dead_section_percent: 0.5,
    });
    let scheduler = Arc::new(Scheduler::new(topology, (), 4, |_| {}, |_| {}));
    let mut workers = Vec::new();
    for worker in 0..4 {
        let scheduler = scheduler.clone();
        workers.push(thread::spawn(move || {
            for i in 0..300 {
                let chunk = ChunkPos((i % 15) + worker * 20, (i / 15) % 15);
                scheduler.add_chunk(chunk);
                if i % 2 == 0 {
                    scheduler.remove_chunk(chunk);
                }
                scheduler.check_invariants().unwrap();
            }
        }));
    }
    for worker in workers {
        worker.join().unwrap();
    }
    scheduler.check_invariants().unwrap();
    Arc::try_unwrap(scheduler).ok().unwrap().shutdown();
}
