# Light pool (ScalableLux-style) — design and throughput (2026-11-21)

Additive worker pool in `crates/ferrite-light`. The default `LightEngine` path is unchanged and
single-threaded; nothing is wired into `ferrite-server`. Base: `origin/main` at `2a594816`.

## API

```rust
use ferrite_light::{LightJob, LightPool, LightPoolConfig};

let pool = LightPool::new(LightPoolConfig::new(4, -4, 19, true)); // workers, min/max section, sky
let jobs = [LightJob::new(0, 0), LightJob::new(3, 0)];
let snapshots = pool.relight(&world, &jobs)?;   // one Arc<LightSnapshot> per job, in job order
let published = pool.snapshot(0, 0);             // last snapshot published for that centre
```

Public additions: `LightPool`, `LightPoolConfig`, `LightJob`, `LightSnapshot`, `LightPoolError`,
`chunk_neighbourhood`, `LightEngine::recenter`, and `PartialEq`/`Eq`/manual `Debug` on
`NibbleArray` (needed to compare and report snapshots). `parking_lot` is added to the crate from
the workspace dependency table.

`LightSnapshot` holds the centre chunk's sky and block arrays for
`min_section - 1 ..= max_section + 1`; `get(kind, x, y, z)` reads world coordinates.

## Design

- **One private engine per worker.** A scoped worker thread owns one `LightEngine` and calls
  `recenter` + `relight` per job. Engine state is never shared, so propagation runs outside all
  locks. `recenter` discards section, block, queue and pending-edge state; it is covered by
  `recentered_reused_engine_matches_a_fresh_engine`.
- **Centre-only publication.** A 3x3 engine computes exact light for its centre chunk only:
  light in chunk `C` depends on blocks in `C` and its eight neighbours, and the engine's fringe
  chunks are missing one outer neighbour each. The job therefore publishes only its centre cell;
  the other eight engine outputs are discarded.
- **Per-chunk lock tokens.** `ChunkTable` maps `(chunk_x, chunk_z)` to `ChunkCell`; each cell's
  `parking_lot::Mutex<Option<Arc<LightSnapshot>>>` is both the lock token and the published
  snapshot. The table's own mutex is held only to look up or create a cell and is released before
  any token is acquired, so it cannot participate in a lock cycle. There is no engine-wide lock.
- **Lock order.** Publication acquires the nine tokens of the job's 3x3 read neighbourhood in
  ascending `(chunk_x, chunk_z)` order (numeric, so `-1 < 0 < 1`), writes the centre cell, and
  releases. Every job uses that one total order, so overlapping jobs serialise at publication
  instead of forming a cycle. `neighbourhood_tokens_sort_in_one_total_order` pins the order.
- **Determinism.** A job is a pure function of its centre, the pool config and the immutable
  block view. Work stealing and publication order do not affect values; results are stored by
  job index. Duplicate jobs for one centre compute and publish identical bytes. The block view
  must not change during a batch.
- **Incremental changes.** `LightEngine::block_changed` is untouched. The pool does full
  relights: after a edit, a caller resubmits every centre returned by
  `chunk_neighbourhood(block_chunk_x, block_chunk_z)`. Overlapping resubmissions take the same
  ordered tokens, so they cannot corrupt a chunk or deadlock. The stress test below checks the
  pooled results against a serial engine that applied the same edits through `block_changed`.
- **Panic handling.** Each job runs under `catch_unwind`. `parking_lot` does not poison, and
  guards drop during unwinding, so no token stays held; the batch returns
  `LightPoolError::WorkerPanicked { job, message }` and the pool remains usable. A separate unit
  test panics while holding all nine tokens and verifies every token is acquirable afterwards.
  This requires an unwinding build. The workspace release profile sets `panic = "abort"`, where
  a panicking job aborts the process instead.
- **Dispatch.** Each batch spawns `min(workers, jobs.len())` scoped threads (no persistent pool,
  so the block view can be borrowed without `'static` or `unsafe`) and pulls job indices from an
  atomic counter. Thread start-up is included in the benchmark.

## Tests

```sh
cargo +stable test -p ferrite-light
```

Result: 3 library unit tests, 14 `tests/light.rs`, 5 `tests/pool.rs`, 0 doc-tests — all pass.

- `disjoint_centres_parallel_relight_equals_serial` — eight centres three chunks apart, 1/2/4/8
  workers against a serial engine, every block in the centre chunk and both light kinds, plus
  the published-store lookup.
- `same_centre_contention_completes_and_equals_serial` — 32 identical jobs on `(2, 2)` at
  1/2/4/8 workers; all snapshots equal each other and the serial engine.
- `overlapping_neighbourhoods_complete_in_canonical_lock_order` — 32 jobs alternating between
  centres `(0, 0)` and `(1, 0)`, whose 3x3 sets overlap; a watchdog fails the test after 30 s.
  A centre-first lock order (`(0,0)` then `(1,0)` in one job, the reverse in the other) is the
  naive order this scenario would deadlock.
- `pooled_relight_matches_serial_incremental_over_random_terrain_and_changes` — random stone
  ground with caves, water, torches, glass, leaves and slabs over a 3x3 centre grid; eight
  rounds of two random edits. A serial engine applies all 16 edits with `block_changed`; the
  pool relights all nine centres at 1/2/4/8 workers after each round and is compared over every
  block and both light kinds.
- `worker_panic_is_reported_and_the_pool_stays_usable` — a block view that panics produces
  `WorkerPanicked`, publishes nothing, and the same pool then matches serial on a normal world.
- Unit: token order, token release after a panic, and the recenter reuse above.

**Vanilla differential:** the fixture is absent in this worktree, so
`tests/vanilla.rs::compare_fixture_light_when_available` skipped and did **not** verify vanilla
parity. It printed:

```text
SKIP vanilla light differential: .../vanilla/26.3/fixture-world/world/dimensions/minecraft/overworld/region/r.0.0.mca absent
```

The generated `vanilla/26.3/generated` data was fetched to compile `ferrite-data`; only the
fixture world is missing. The pool was not exercised against the fixture.

## Benchmark

Machine: Intel Xeon Platinum 8358, **6 cores / 12 logical CPUs** (2 threads per core), 62 GiB RAM,
Linux x86_64. The box is shared with other Orca agents; load average is the 1-minute figure from
`/proc/loadavg` before and after each run. Numbers below are wall time for 12 full-height
(`-4..=19`) chunk neighbourhoods per batch, best of 3 timed runs; the mean was within 2–8% of the
best. "Adjacent" uses centres one chunk apart (every neighbourhood overlaps); "disjoint" uses
centres three chunks apart.

```sh
cargo +stable run -p ferrite-light --release --example pool_throughput
```

Run A (load 3.18 → 4.56):

| scenario | workers | best wall (ms) | ms/job | speedup vs 1 |
|---|---:|---:|---:|---:|
| disjoint | 1 | 771.692 | 64.308 | 1.00x |
| disjoint | 2 | 398.495 | 33.208 | 1.94x |
| disjoint | 4 | 210.402 | 17.534 | 3.67x |
| disjoint | 8 | 157.001 | 13.083 | 4.92x |
| adjacent | 1 | 779.368 | 64.947 | 1.00x |
| adjacent | 2 | 401.229 | 33.436 | 1.94x |
| adjacent | 4 | 202.611 | 16.884 | 3.85x |
| adjacent | 8 | 150.371 | 12.531 | 5.18x |

Run B (load 3.78 → 3.41):

| scenario | workers | best wall (ms) | ms/job | speedup vs 1 |
|---|---:|---:|---:|---:|
| disjoint | 1 | 759.156 | 63.263 | 1.00x |
| disjoint | 2 | 392.706 | 32.726 | 1.93x |
| disjoint | 4 | 203.512 | 16.959 | 3.73x |
| disjoint | 8 | 139.984 | 11.665 | 5.42x |
| adjacent | 1 | 756.911 | 63.076 | 1.00x |
| adjacent | 2 | 389.501 | 32.458 | 1.94x |
| adjacent | 4 | 204.501 | 17.042 | 3.70x |
| adjacent | 8 | 143.405 | 11.950 | 5.28x |

Reading: the pool measured **1.9x at 2 workers, 3.7–3.9x at 4, and 4.9–5.4x at 8** on this box.
The 8-worker point oversubscribes six physical cores via SMT and is also where engine
initialisation and memory traffic matter; it is not 8x and is not claimed to be. Adjacent and
disjoint centres scale the same within run-to-run spread, which is expected because propagation
is outside the locks and only publication serialises. This is a component benchmark on a shared
machine, not a server throughput result.

## Limitations and open work

- The pool is not wired into `ferrite-server`; no server tick, real client or 800-bot run exists
  for it. The 800-bot MSPT/RSS measurement remains open.
- The vanilla fixture differential was skipped, not passed. Run it on a machine with
  `vanilla/26.3/fixture-world` before enabling any worker in the server.
- One engine per worker costs about 15 MB at the full overworld range (`-4..=19`, 8-byte
  `LightProperties` over 48×48×384 blocks), so an 8-worker batch holds roughly 120 MB of engine
  state transiently in addition to snapshots.
- `LightPool` retains one snapshot per published chunk until `clear_store`; there is no eviction.
- Batches are executed by scoped threads created per call, with no persistent worker pool,
  work-stealing queue or backpressure. At 64 ms per full-height job the start-up cost is small;
  for tiny section ranges it would not be.
- Only the centre chunk of each job is published. Parallelising the fringe, or using a wider
  neighbourhood so all nine chunks are exact, is not implemented.
- Panic recovery needs an unwinding build; the release profile's `panic = "abort"` aborts.
- The benchmark host was shared and its load varied; use the two runs as an indication, not a
  quiet-machine baseline.

## Files changed

- `crates/ferrite-light/src/pool.rs` (new)
- `crates/ferrite-light/src/lib.rs`
- `crates/ferrite-light/tests/pool.rs` (new)
- `crates/ferrite-light/tests/light.rs`
- `crates/ferrite-light/examples/pool_throughput.rs` (new)
- `crates/ferrite-light/Cargo.toml`, `Cargo.lock`
- `docs/perf/2026-11-21/lighting-pool.md` (this file)
