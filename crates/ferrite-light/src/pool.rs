//! Batched relighting on a fixed number of scoped worker threads.
//!
//! A [`LightPool`] runs many centre-chunk relights in one call. Each worker
//! owns one reusable [`LightEngine`] and computes a snapshot for a job's centre
//! chunk outside every lock: the block view is immutable for the duration of a
//! batch and the engine is private, so no light state is shared while light is
//! propagated.
//!
//! # Chunk lock tokens and lock order
//!
//! Publication is the only shared mutation. A job publishes its centre chunk
//! while holding the lock tokens of its whole 3x3 read neighbourhood, acquired
//! in ascending `(chunk_x, chunk_z)` order. Every job uses that one total
//! order, so a set of jobs cannot form a lock cycle. The pool's chunk table
//! mutex is only held to look up or create a cell; it is never held while a
//! chunk token is acquired, while light is computed, or while a snapshot is
//! published. There is no engine-wide mutex.
//!
//! Two jobs with overlapping neighbourhoods take their tokens in the same
//! order and therefore serialise at publication instead of deadlocking. Jobs
//! with disjoint neighbourhoods never contend. A job only writes its centre
//! cell, which is the only chunk whose light a 3x3 engine can compute exactly:
//! light in a chunk depends on the blocks of that chunk and its eight
//! neighbours, so extra engine output at the neighbourhood fringe is discarded.
//!
//! # Determinism
//!
//! A job's result is a pure function of the centre coordinate, the pool
//! configuration and the block view. It does not depend on which worker ran it
//! or on publication order, so results at 1, 2, 4 and 8 workers are
//! bit-identical and are returned in job order. Duplicate jobs for one centre
//! compute the same snapshot and publish the same bytes.
//!
//! # Incremental block changes
//!
//! The pool does not change [`LightEngine::block_changed`](crate::LightEngine::block_changed);
//! the serial incremental algorithm is untouched. After blocks change, a
//! caller that uses the pool resubmits every centre chunk within one chunk of
//! the change (see [`chunk_neighbourhood`]). Those are full relights, not
//! incremental repairs. The pooled results match a serial engine that applied
//! the same changes through `block_changed`; `tests/pool.rs` checks this after
//! every change over every block and both light kinds. Overlapping
//! resubmissions cannot corrupt each other because each publication holds the
//! neighbourhood's chunk tokens in the order above.
//!
//! # Panics
//!
//! Each job runs under [`std::panic::catch_unwind`]. A panicking job releases
//! its chunk tokens while unwinding (`parking_lot` mutexes do not poison) and
//! the pool returns [`LightPoolError::WorkerPanicked`] with the job index. No
//! snapshot is published for a job that panicked before publication. The pool
//! remains usable for later batches. This recovery requires unwinding builds;
//! the workspace release profile sets `panic = "abort"`, where a panic aborts
//! the process instead.
use crate::{LightBlockView, LightEngine, LightKind, NibbleArray};
use parking_lot::{Mutex, MutexGuard};
use std::{
    any::Any,
    collections::HashMap,
    fmt,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

/// Chunk coordinate as `(chunk_x, chunk_z)`, matching the engine's API.
pub type ChunkPos = (i32, i32);

/// The centre chunk and its eight neighbours, in an unspecified order.
///
/// Panics when the coordinate cannot be offset without overflowing. Every job
/// computes light for the centre of this set, so after a block change every
/// returned position is a decode candidate for resubmission.
pub fn chunk_neighbourhood(center_x: i32, center_z: i32) -> [ChunkPos; 9] {
    let mut positions = [(0, 0); 9];
    let mut next = 0;
    for dz in -1..=1 {
        for dx in -1..=1 {
            positions[next] = (
                center_x.checked_add(dx).expect("chunk x out of range"),
                center_z.checked_add(dz).expect("chunk z out of range"),
            );
            next += 1;
        }
    }
    positions
}

/// Pool tuning. `min_section..=max_section` is the buildable range, exactly as
/// passed to [`LightEngine::new`]; the pool also stores one light section above
/// and below it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightPoolConfig {
    /// Worker threads used per batch. `1` is the serial reference path.
    pub workers: usize,
    pub min_section: i32,
    pub max_section: i32,
    pub sky_enabled: bool,
}
impl LightPoolConfig {
    pub fn new(workers: usize, min_section: i32, max_section: i32, sky_enabled: bool) -> Self {
        Self {
            workers,
            min_section,
            max_section,
            sky_enabled,
        }
    }
}
impl Default for LightPoolConfig {
    fn default() -> Self {
        Self {
            workers: thread::available_parallelism().map_or(1, |n| n.get()),
            min_section: -4,
            max_section: 19,
            sky_enabled: true,
        }
    }
}

/// One full relight job: the chunk whose light is computed and published.
///
/// The job's engine neighbourhood is [`chunk_neighbourhood`]`(center_x, center_z)`.
/// Duplicate jobs are kept: they are independent computations that publish
/// identical snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LightJob {
    pub center_x: i32,
    pub center_z: i32,
}
impl LightJob {
    pub const fn new(center_x: i32, center_z: i32) -> Self {
        Self { center_x, center_z }
    }
    pub const fn center(self) -> ChunkPos {
        (self.center_x, self.center_z)
    }
}

/// Light arrays for one chunk, captured after a completed relight.
///
/// Section arrays cover `min_section - 1 ..= max_section + 1`, including the
/// extra propagation sections at both ends. The engine compacts uniform arrays,
/// so `Zero` and `Full` sections carry no payload.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LightSnapshot {
    chunk_x: i32,
    chunk_z: i32,
    min_section: i32,
    max_section: i32,
    sections: Vec<SectionLight>,
}
#[derive(Clone, PartialEq, Eq, Debug)]
struct SectionLight {
    sky: NibbleArray,
    block: NibbleArray,
}
impl LightSnapshot {
    fn capture(engine: &LightEngine, job: LightJob, config: LightPoolConfig) -> Self {
        let mut sections =
            Vec::with_capacity((config.max_section - config.min_section + 3) as usize);
        for section_y in config.min_section - 1..=config.max_section + 1 {
            sections.push(SectionLight {
                sky: engine
                    .section(LightKind::Sky, job.center_x, section_y, job.center_z)
                    .expect("engine sky section missing")
                    .clone(),
                block: engine
                    .section(LightKind::Block, job.center_x, section_y, job.center_z)
                    .expect("engine block section missing")
                    .clone(),
            });
        }
        Self {
            chunk_x: job.center_x,
            chunk_z: job.center_z,
            min_section: config.min_section,
            max_section: config.max_section,
            sections,
        }
    }
    pub const fn center(&self) -> ChunkPos {
        (self.chunk_x, self.chunk_z)
    }
    pub const fn min_section(&self) -> i32 {
        self.min_section
    }
    pub const fn max_section(&self) -> i32 {
        self.max_section
    }
    /// Number of stored sections, including the two propagation sections.
    pub fn section_count(&self) -> usize {
        self.sections.len()
    }
    /// The stored array for `section_y`, or `None` outside the stored range.
    pub fn section(&self, kind: LightKind, section_y: i32) -> Option<&NibbleArray> {
        let index = i64::from(section_y) - i64::from(self.min_section) + 1;
        let index = usize::try_from(index).ok()?;
        let section = self.sections.get(index)?;
        Some(match kind {
            LightKind::Sky => &section.sky,
            LightKind::Block => &section.block,
        })
    }
    /// Light at world coordinates in this chunk, or `None` outside its columns
    /// or the stored section range.
    pub fn get(&self, kind: LightKind, x: i32, y: i32, z: i32) -> Option<u8> {
        let local_x = i64::from(x) - i64::from(self.chunk_x) * 16;
        let local_z = i64::from(z) - i64::from(self.chunk_z) * 16;
        if !(0..16).contains(&local_x) || !(0..16).contains(&local_z) {
            return None;
        }
        let section_y = y.div_euclid(16);
        let index = i64::from(section_y) - i64::from(self.min_section) + 1;
        let index = usize::try_from(index).ok()?;
        let section = self.sections.get(index)?;
        let offset = (y.rem_euclid(16) as usize) * 256 + (local_z as usize) * 16 + local_x as usize;
        match kind {
            LightKind::Sky => &section.sky,
            LightKind::Block => &section.block,
        }
        .get(offset)
    }
}

/// A pool failure. A panic in one job does not stop the other jobs from
/// finishing; their results are discarded because the batch is reported as
/// failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LightPoolError {
    WorkerPanicked { job: usize, message: String },
}
impl fmt::Display for LightPoolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WorkerPanicked { job, message } => {
                write!(f, "light worker panicked on job {job}: {message}")
            }
        }
    }
}
impl std::error::Error for LightPoolError {}

fn panic_message(payload: Box<dyn Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}

struct ChunkCell {
    light: Mutex<Option<Arc<LightSnapshot>>>,
}

/// The only pool-wide mutex guards this coordinate-to-cell map. It is released
/// before any cell token is acquired, so it cannot participate in a lock cycle.
#[derive(Default)]
struct ChunkTable {
    cells: Mutex<HashMap<ChunkPos, Arc<ChunkCell>>>,
}
impl ChunkTable {
    fn cell(&self, pos: ChunkPos) -> Arc<ChunkCell> {
        let mut cells = self.cells.lock();
        Arc::clone(cells.entry(pos).or_insert_with(|| {
            Arc::new(ChunkCell {
                light: Mutex::new(None),
            })
        }))
    }
    fn neighbourhood(&self, center: ChunkPos) -> Vec<(ChunkPos, Arc<ChunkCell>)> {
        chunk_neighbourhood(center.0, center.1)
            .into_iter()
            .map(|pos| (pos, self.cell(pos)))
            .collect()
    }
    /// Acquires the centre's nine chunk tokens in `(x, z)` order and writes the
    /// centre cell. Only the centre is written; the other tokens document and
    /// guard the job's 3x3 read neighbourhood.
    fn publish(&self, center: ChunkPos, snapshot: Arc<LightSnapshot>) {
        let mut cells = self.neighbourhood(center);
        cells.sort_unstable_by_key(|(pos, _)| *pos);
        let mut guards: Vec<MutexGuard<'_, Option<Arc<LightSnapshot>>>> =
            cells.iter().map(|(_, cell)| cell.light.lock()).collect();
        let index = cells
            .iter()
            .position(|(pos, _)| *pos == center)
            .expect("the centre is part of its neighbourhood");
        *guards[index] = Some(snapshot);
    }
    fn get(&self, pos: ChunkPos) -> Option<Arc<LightSnapshot>> {
        let cell = self.cells.lock().get(&pos).cloned()?;
        cell.light.lock().clone()
    }
    fn clear(&self) {
        self.cells.lock().clear();
    }
}

/// Batched relight executor with per-chunk lock tokens.
///
/// See the [module documentation](self) for the lock order, determinism and
/// incremental-update contract.
pub struct LightPool {
    config: LightPoolConfig,
    chunks: ChunkTable,
}
impl LightPool {
    /// Panics on `workers == 0` or a section range the engine rejects.
    pub fn new(config: LightPoolConfig) -> Self {
        assert!(config.workers > 0, "light pool needs at least one worker");
        assert!(
            config.min_section <= config.max_section,
            "min_section must not exceed max_section"
        );
        let sections = i64::from(config.max_section) - i64::from(config.min_section) + 1;
        assert!(
            sections * 16 <= 65_536,
            "section range exceeds the engine height limit"
        );
        assert!(
            (i64::from(config.min_section) - 1) * 16 >= i64::from(i32::MIN)
                && (i64::from(config.max_section) + 1) * 16 <= i64::from(i32::MAX),
            "section range exceeds the engine coordinate range"
        );
        Self {
            config,
            chunks: ChunkTable::default(),
        }
    }
    pub fn config(&self) -> LightPoolConfig {
        self.config
    }
    pub fn workers(&self) -> usize {
        self.config.workers
    }
    /// The last snapshot published for `center`, if any job has completed one.
    pub fn snapshot(&self, center_x: i32, center_z: i32) -> Option<Arc<LightSnapshot>> {
        self.chunks.get((center_x, center_z))
    }
    /// Drops every published snapshot. Intended for quiescent pools; a batch
    /// running concurrently may publish into cells that are no longer indexed.
    pub fn clear_store(&self) {
        self.chunks.clear();
    }
    /// Relights every job's centre chunk. Results are returned in job order.
    ///
    /// The block view must not change during the call. Jobs may be adjacent or
    /// duplicated: light propagation is private to each worker, and publication
    /// locks the 3x3 neighbourhood in ascending `(x, z)` order.
    pub fn relight(
        &self,
        world: &(impl LightBlockView + Sync),
        jobs: &[LightJob],
    ) -> Result<Vec<Arc<LightSnapshot>>, LightPoolError> {
        if jobs.is_empty() {
            return Ok(Vec::new());
        }
        let workers = self.config.workers.min(jobs.len());
        let next = AtomicUsize::new(0);
        let results: Mutex<Vec<Option<Arc<LightSnapshot>>>> = Mutex::new(vec![None; jobs.len()]);
        let failure: Mutex<Option<LightPoolError>> = Mutex::new(None);
        thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(|| {
                    // One engine per worker: `recenter` discards all prior state,
                    // so jobs cannot influence one another.
                    let mut engine = LightEngine::new(
                        0,
                        0,
                        self.config.min_section,
                        self.config.max_section,
                        self.config.sky_enabled,
                    );
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        if index >= jobs.len() {
                            break;
                        }
                        let job = jobs[index];
                        match catch_unwind(AssertUnwindSafe(|| {
                            self.relight_one(world, &mut engine, job)
                        })) {
                            Ok(snapshot) => results.lock()[index] = Some(snapshot),
                            Err(payload) => {
                                let mut failure = failure.lock();
                                if failure.is_none() {
                                    *failure = Some(LightPoolError::WorkerPanicked {
                                        job: index,
                                        message: panic_message(payload),
                                    });
                                }
                                break;
                            }
                        }
                    }
                });
            }
        });
        if let Some(error) = failure.into_inner() {
            return Err(error);
        }
        Ok(results
            .into_inner()
            .into_iter()
            .map(|slot| slot.expect("every job completed without a result"))
            .collect())
    }
    fn relight_one<W: LightBlockView>(
        &self,
        world: &W,
        engine: &mut LightEngine,
        job: LightJob,
    ) -> Arc<LightSnapshot> {
        engine.recenter(job.center_x, job.center_z);
        engine.relight(world);
        let snapshot = Arc::new(LightSnapshot::capture(engine, job, self.config));
        self.chunks.publish(job.center(), Arc::clone(&snapshot));
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbourhood_tokens_sort_in_one_total_order() {
        let table = ChunkTable::default();
        let mut cells = table.neighbourhood((5, -2));
        cells.sort_unstable_by_key(|(pos, _)| *pos);
        let order: Vec<_> = cells.iter().map(|(pos, _)| *pos).collect();
        assert_eq!(
            order,
            vec![
                (4, -3),
                (4, -2),
                (4, -1),
                (5, -3),
                (5, -2),
                (5, -1),
                (6, -3),
                (6, -2),
                (6, -1),
            ]
        );
    }

    #[test]
    fn panic_while_holding_chunk_tokens_releases_every_token() {
        let table = Arc::new(ChunkTable::default());
        let worker_table = Arc::clone(&table);
        let result = thread::spawn(move || {
            let mut cells = worker_table.neighbourhood((7, 3));
            cells.sort_unstable_by_key(|(pos, _)| *pos);
            let _guards: Vec<_> = cells.iter().map(|(_, cell)| cell.light.lock()).collect();
            panic!("simulated worker panic");
        })
        .join();
        assert!(result.is_err());
        for (pos, cell) in table.neighbourhood((7, 3)) {
            assert!(
                cell.light.try_lock().is_some(),
                "chunk {pos:?} still locked"
            );
        }
    }
}
