use ferrite_region::{ChunkPos, RegionData, Regionizer, SectionPos};
use std::collections::HashSet;

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
#[test]
fn bridge_merges_and_removal_splits() {
    let mut r = Regionizer::<Data>::new(0, 1);
    r.add_chunk(ChunkPos(0, 0));
    r.add_chunk(ChunkPos(2, 0));
    assert_eq!(r.region_count(), 2);
    r.add_chunk(ChunkPos(1, 0));
    assert_eq!(r.region_count(), 1);
    r.remove_chunk(ChunkPos(1, 0));
    assert_eq!(r.region_count(), 2);
}

#[test]
fn queued_tasks_follow_chunk_ownership_and_shutdown_returns_data() {
    use ferrite_region::Scheduler;
    use std::sync::mpsc;
    use std::time::Duration;
    let scheduler = Scheduler::new(
        Regionizer::<Data>::new(0, 1),
        (),
        2,
        |_: &mut Data| {},
        |_: &mut ()| {},
    );
    let (tx, rx) = mpsc::channel();
    scheduler.schedule_at_chunk(ChunkPos(8, 0), move |_| {
        tx.send(8).unwrap();
    });
    assert!(rx.recv_timeout(Duration::from_millis(80)).is_err());
    scheduler.add_chunk(ChunkPos(8, 0));
    assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), 8);
    let result = scheduler.shutdown();
    assert_eq!(result.regions.len(), 1);
    assert!(result.pending_tasks.is_empty());
}

#[test]
fn batch_chunk_mutations_preserve_topology() {
    use ferrite_region::Scheduler;
    use std::time::{Duration, Instant};
    let scheduler = Scheduler::new(Regionizer::<Data>::new(0, 1), (), 1, |_| {}, |_| {});
    let chunks = [ChunkPos(0, 0), ChunkPos(8, 0), ChunkPos(16, 0)];
    scheduler.add_chunks(chunks);
    let deadline = Instant::now() + Duration::from_secs(2);
    while chunks.iter().any(|&chunk| scheduler.owner(chunk).is_none()) {
        assert!(
            Instant::now() < deadline,
            "batched additions were not applied"
        );
        std::thread::yield_now();
    }
    scheduler.remove_chunks(chunks);
    let deadline = Instant::now() + Duration::from_secs(2);
    while chunks.iter().any(|&chunk| scheduler.owner(chunk).is_some()) {
        assert!(
            Instant::now() < deadline,
            "batched removals were not applied"
        );
        std::thread::yield_now();
    }
    scheduler.shutdown();
}

#[test]
fn split_partitions_data_and_sections_use_floor_division() {
    let mut r = Regionizer::<Data>::new(1, 1);
    assert_eq!(r.section_of(ChunkPos(-1, -3)), SectionPos(-1, -2));
    for x in [0, 2, 4] {
        r.add_chunk(ChunkPos(x, 0));
    }
    let id = r.owner(ChunkPos(0, 0)).unwrap();
    r.data_mut(id)
        .unwrap()
        .0
        .extend([SectionPos(0, 0), SectionPos(2, 0)]);
    r.remove_chunk(ChunkPos(2, 0));
    let left = r.owner(ChunkPos(0, 0)).unwrap();
    let right = r.owner(ChunkPos(4, 0)).unwrap();
    assert_ne!(left, right);
    assert_eq!(
        r.data_mut(left).unwrap().0,
        HashSet::from([SectionPos(0, 0)])
    );
    assert_eq!(
        r.data_mut(right).unwrap().0,
        HashSet::from([SectionPos(2, 0)])
    );
}

#[test]
fn mutations_wait_for_inflight_tick_and_tasks_follow_merge() {
    use ferrite_region::Scheduler;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    use std::time::Duration;

    /// Unblocks a tick that is waiting on `release_tx` even when an assertion
    /// panics. The guard is declared after the scheduler, so it is dropped
    /// first; otherwise `Scheduler::drop` joins a worker that can never stop.
    struct ReleaseOnDrop(Option<mpsc::Sender<()>>);
    impl Drop for ReleaseOnDrop {
        fn drop(&mut self) {
            if let Some(tx) = self.0.take() {
                let _ = tx.send(());
            }
        }
    }

    let mut topology = Regionizer::<Data>::new(0, 1);
    topology.add_chunk(ChunkPos(0, 0));
    topology.add_chunk(ChunkPos(2, 0));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release = Arc::new(std::sync::Mutex::new(release_rx));
    let first = Arc::new(AtomicBool::new(true));
    // One worker: the second region cannot tick at the same time, so adding the
    // bridge always has exactly one idle destination and the test cannot
    // observe a transient third region depending on thread scheduling.
    let scheduler = Scheduler::new(
        topology,
        (),
        1,
        move |_| {
            if first.swap(false, Ordering::SeqCst) {
                entered_tx.send(()).unwrap();
                release.lock().unwrap().recv().unwrap();
            }
        },
        |_| {},
    );
    let mut release_tx = ReleaseOnDrop(Some(release_tx));
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    scheduler.add_chunk(ChunkPos(1, 0));
    assert_eq!(scheduler.region_count(), 2);
    let (tx, rx) = mpsc::channel();
    scheduler.schedule_at_chunk(ChunkPos(2, 0), move |_| {
        tx.send(()).unwrap();
    });
    release_tx.0.take().unwrap().send(()).unwrap();
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(scheduler.region_count(), 1);
    assert_eq!(scheduler.shutdown().regions.len(), 1);
}

#[test]
fn holder_counts_and_global_queue_are_preserved() {
    use ferrite_region::Scheduler;
    use std::sync::mpsc;
    use std::time::Duration;
    let mut r = Regionizer::<Data>::default();
    r.add_chunk(ChunkPos(0, 0));
    r.add_chunk(ChunkPos(0, 0));
    assert!(r.remove_chunk(ChunkPos(0, 0)));
    assert!(r.owner(ChunkPos(0, 0)).is_some());
    let scheduler = Scheduler::new(r, 0usize, 1, |_| {}, |_| {});
    let (tx, rx) = mpsc::channel();
    scheduler.schedule_global(move |value| {
        *value += 1;
        tx.send(()).unwrap();
    });
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(scheduler.shutdown().global, 1);
}

#[test]
#[ignore = "long-running concurrent stress test"]
fn stress_mutations_and_queued_tasks() {
    use ferrite_region::Scheduler;
    use std::sync::{Arc, mpsc};
    use std::time::Duration;
    let mut topology = Regionizer::<Data>::new(0, 1);
    for i in 0..64 {
        topology.add_chunk(ChunkPos(i * 10, 0));
        topology.add_chunk(ChunkPos(i * 10 + 2, 0));
    }
    let scheduler = Arc::new(Scheduler::new(topology, (), 4, |_| {}, |_| {}));
    let (tx, rx) = mpsc::channel();
    let workers: Vec<_> = (0..4)
        .map(|worker| {
            let scheduler = scheduler.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut seed = worker as u32 + 1;
                for sequence in 0..1000 {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    let i = worker * 16 + ((seed >> 16) % 16) as i32;
                    let extra = ChunkPos(i * 10 + 1, 0);
                    scheduler.add_chunk(extra);
                    let tx = tx.clone();
                    scheduler.schedule_at_chunk(extra, move |_| {
                        tx.send((worker, sequence)).unwrap();
                    });
                    scheduler.remove_chunk(extra);
                }
            })
        })
        .collect();
    drop(tx);
    for worker in workers {
        worker.join().unwrap();
    }
    // Re-acquire every temporary section, so tasks parked by a removal can run.
    for i in 0..64 {
        scheduler.add_chunk(ChunkPos(i * 10 + 1, 0));
    }
    let mut received = HashSet::new();
    for _ in 0..4000 {
        assert!(received.insert(rx.recv_timeout(Duration::from_secs(20)).unwrap()));
    }
    for i in 0..64 {
        assert_eq!(
            scheduler.owner(ChunkPos(i * 10, 0)),
            scheduler.owner(ChunkPos(i * 10 + 1, 0))
        );
        assert_eq!(
            scheduler.owner(ChunkPos(i * 10, 0)),
            scheduler.owner(ChunkPos(i * 10 + 2, 0))
        );
    }
    let owners: HashSet<_> = (0..64)
        .map(|i| scheduler.owner(ChunkPos(i * 10, 0)).unwrap())
        .collect();
    assert_eq!(owners.len(), 64);
    assert_eq!(scheduler.region_count(), 64);
    assert!(
        Arc::try_unwrap(scheduler)
            .ok()
            .unwrap()
            .shutdown()
            .pending_tasks
            .is_empty()
    );
}

#[test]
fn tasks_run_on_the_region_that_owns_the_target() {
    use ferrite_region::Scheduler;
    use std::sync::mpsc;
    use std::time::Duration;
    let mut r = Regionizer::<Data>::new(0, 1);
    r.add_chunk(ChunkPos(0, 0));
    r.add_chunk(ChunkPos(10, 0));
    let left = r.owner(ChunkPos(0, 0)).unwrap();
    let right = r.owner(ChunkPos(10, 0)).unwrap();
    r.data_mut(left).unwrap().0.insert(SectionPos(0, 0));
    r.data_mut(right).unwrap().0.insert(SectionPos(10, 0));
    let scheduler = Scheduler::new(r, (), 2, |_| {}, |_| {});
    let (tx, rx) = mpsc::channel();
    scheduler.schedule_at_chunk(ChunkPos(10, 0), move |data| {
        tx.send(data.0.clone()).unwrap();
    });
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        HashSet::from([SectionPos(10, 0)])
    );
    scheduler.schedule_at_chunk(ChunkPos(100, 0), |_| {});
    let result = scheduler.shutdown();
    assert_eq!(result.regions.len(), 2);
    assert_eq!(result.pending_tasks.len(), 1);
}

#[test]
fn ticking_region_does_not_block_distant_regions_or_chunk_add() {
    use ferrite_region::Scheduler;
    use std::sync::{Arc, mpsc};
    use std::time::Duration;
    let mut topology = Regionizer::<Data>::new(0, 1);
    topology.add_chunk(ChunkPos(0, 0));
    topology.add_chunk(ChunkPos(100, 0));
    let scheduler = Arc::new(Scheduler::new(topology, (), 3, |_| {}, |_| {}));
    let (entered, wait_entered) = mpsc::channel();
    let (release, wait_release) = mpsc::channel();
    scheduler.schedule_at_chunk(ChunkPos(0, 0), move |_| {
        entered.send(()).unwrap();
        wait_release.recv().unwrap();
    });
    wait_entered.recv_timeout(Duration::from_secs(2)).unwrap();
    let (added, wait_added) = mpsc::channel();
    let cloned = scheduler.clone();
    std::thread::spawn(move || {
        cloned.add_chunk(ChunkPos(0, 0));
        cloned.add_chunk(ChunkPos(200, 0));
        cloned.remove_chunk(ChunkPos(200, 0));
        added.send(()).unwrap();
    });
    wait_added.recv_timeout(Duration::from_secs(1)).unwrap();
    let (tx, rx) = mpsc::channel();
    scheduler.schedule_at_chunk(ChunkPos(100, 0), move |_| {
        tx.send(()).unwrap();
    });
    let result = rx.recv_timeout(Duration::from_millis(500));
    release.send(()).unwrap();
    assert!(
        result.is_ok(),
        "distant region was blocked by unrelated tick and mutation"
    );
    Arc::try_unwrap(scheduler).ok().unwrap().shutdown();
}

#[test]
fn bridge_task_queued_during_tick_survives_deferred_merge() {
    use ferrite_region::Scheduler;
    use std::sync::mpsc;
    use std::time::Duration;
    let mut topology = Regionizer::<Data>::new(0, 1);
    topology.add_chunk(ChunkPos(0, 0));
    topology.add_chunk(ChunkPos(2, 0));
    let scheduler = Scheduler::new(topology, (), 3, |_| {}, |_| {});
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    scheduler.schedule_at_chunk(ChunkPos(0, 0), move |_| {
        entered_tx.send(()).unwrap();
        release_rx.recv().unwrap();
    });
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let (other_entered_tx, other_entered_rx) = mpsc::channel();
    let (other_release_tx, other_release_rx) = mpsc::channel();
    scheduler.schedule_at_chunk(ChunkPos(2, 0), move |_| {
        other_entered_tx.send(()).unwrap();
        other_release_rx.recv().unwrap();
    });
    other_entered_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    scheduler.add_chunk(ChunkPos(1, 0));
    let (tx, rx) = mpsc::channel();
    scheduler.schedule_at_chunk(ChunkPos(1, 0), move |_| {
        tx.send(()).unwrap();
    });
    // Both neighbours are ticking, so the bridge is temporarily a third region.
    assert_eq!(scheduler.region_count(), 3);
    release_tx.send(()).unwrap();
    other_release_tx.send(()).unwrap();
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(scheduler.region_count(), 1);
    assert!(rx.try_recv().is_err());
    assert!(scheduler.shutdown().pending_tasks.is_empty());
}
#[test]
fn configured_empty_sections_reserve_the_merge_radius() {
    use ferrite_region::RegionizerConfig;
    let mut r = Regionizer::<Data>::with_config(RegionizerConfig {
        section_shift: 0,
        merge_radius: 1,
        empty_section_radius: 1,
        min_section_recalc_count: 2,
        max_dead_section_percent: 0.5,
    });
    r.add_chunk(ChunkPos(0, 0));
    let owner = r.owner(ChunkPos(0, 0)).unwrap();
    for x in -1..=1 {
        for z in -1..=1 {
            assert_eq!(r.owner(ChunkPos(x, z)), Some(owner));
        }
    }
    r.check_invariants().unwrap();
    assert!(r.remove_chunk(ChunkPos(0, 0)));
    r.recalculate();
    assert_eq!(r.region_count(), 0);
}
#[test]
fn recalculation_retains_empty_sections_near_remaining_chunks() {
    use ferrite_region::RegionizerConfig;
    let mut r = Regionizer::<Data>::with_config(RegionizerConfig {
        section_shift: 0,
        merge_radius: 1,
        empty_section_radius: 1,
        min_section_recalc_count: 1000,
        max_dead_section_percent: 1.0,
    });
    r.add_chunk(ChunkPos(0, 0));
    r.add_chunk(ChunkPos(2, 0));
    r.remove_chunk(ChunkPos(0, 0));
    assert!(
        r.owner(ChunkPos(-1, 0)).is_some(),
        "dead sections stay until recalculation"
    );
    r.recalculate();
    assert_eq!(r.owner(ChunkPos(-1, 0)), None);
    assert!(
        r.owner(ChunkPos(1, 0)).is_some(),
        "empty section beside live chunk stays"
    );
    r.check_invariants().unwrap();
}
#[test]
fn tasks_parked_in_new_empty_sections_follow_owner() {
    use ferrite_region::{RegionizerConfig, Scheduler};
    use std::{sync::mpsc, time::Duration};
    let mut topology = Regionizer::<Data>::with_config(RegionizerConfig {
        section_shift: 0,
        merge_radius: 1,
        empty_section_radius: 1,
        min_section_recalc_count: 2,
        max_dead_section_percent: 0.5,
    });
    topology.add_chunk(ChunkPos(0, 0));
    let scheduler = Scheduler::new(topology, (), 2, |_| {}, |_| {});
    let (tx, rx) = mpsc::channel();
    scheduler.schedule_at_chunk(ChunkPos(3, 0), move |_| tx.send(()).unwrap());
    scheduler.add_chunk(ChunkPos(2, 0));
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(scheduler.shutdown().pending_tasks.is_empty());
}
#[test]
fn merged_region_handle_becomes_dead() {
    use ferrite_region::RegionState;
    let mut r = Regionizer::<Data>::new(0, 1);
    r.add_chunk(ChunkPos(0, 0));
    r.add_chunk(ChunkPos(2, 0));
    let first = r.owner(ChunkPos(0, 0)).unwrap();
    let second = r.owner(ChunkPos(2, 0)).unwrap();
    r.add_chunk(ChunkPos(1, 0));
    assert_eq!(r.region_state(first), Some(RegionState::Ready));
    assert_eq!(r.region_state(second), Some(RegionState::Dead));
}
#[test]
fn region_stats_count_live_empty_and_dead_sections() {
    use ferrite_region::RegionizerConfig;
    let mut r = Regionizer::<Data>::with_config(RegionizerConfig {
        section_shift: 0,
        merge_radius: 1,
        empty_section_radius: 1,
        min_section_recalc_count: 100,
        max_dead_section_percent: 1.0,
    });
    r.add_chunk(ChunkPos(0, 0));
    r.add_chunk(ChunkPos(2, 0));
    let id = r.owner(ChunkPos(0, 0)).unwrap();
    r.remove_chunk(ChunkPos(0, 0));
    let stats = r.region_stats(id).unwrap();
    assert_eq!(stats.nonempty_sections, 1);
    assert!(stats.empty_sections > 0);
    assert!(stats.dead_sections > 0);
}
#[test]
fn default_regionizer_reserves_empty_sections_within_merge_radius() {
    let mut topology = Regionizer::<Data>::default();
    topology.add_chunk(ChunkPos(0, 0));
    let owner = topology.owner(ChunkPos(0, 0)).unwrap();
    assert_eq!(topology.owner(ChunkPos(32, 0)), Some(owner));
    topology.check_invariants().unwrap();
}
