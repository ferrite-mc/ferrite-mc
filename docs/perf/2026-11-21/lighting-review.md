# Lighting pool concurrency review (2026-11-21)

Independent adversarial review of the per-chunk-locked lighting worker pool added in `514dc4fa`
and documented in [`lighting-pool.md`](lighting-pool.md). Reviewed at branch
`yxshadxw/lighting-review`, HEAD `2ac1b115`, against `crates/ferrite-light/src/pool.rs`,
`crates/ferrite-light/tests/pool.rs` and the design doc. This review is additive: only
`crates/ferrite-light/tests/pool_stress.rs` and this file were written; `src/`, the existing
tests, `examples/`, `docs/BACKLOG.md` and the design doc are unchanged.

## Verdict

| question | verdict |
|---|---|
| Deadlock from overlapping publications | **No defect found.** Acquisition is one total order over chunk positions; the chunk-table mutex is never held while a token is taken. |
| Snapshot atomicity | **Atomic per published centre.** A reader sees one whole `LightSnapshot` or none, never a mix. Not atomic across chunks of a batch. |
| Wrong light under parallelism | **No mismatch observed.** Every job at 1/2/4/8 workers matched a serial engine block-by-block, including duplicate and negative centres. |
| Panic handling | **Correct in unwinding builds.** `WorkerPanicked` with the job index and payload; the panicking job publishes nothing and leaves the pool usable. |
| Defects found | **None.** Behavioural caveats are listed under open questions. |

## Threat model

Assets: the per-chunk light values in the published store; liveness of the per-chunk lock tokens;
usability of the pool after a failure; the light values each batch returns.

Faults injected by the new tests:

- adversarial job sets: heavy 3x3 overlap, duplicate centres, negative coordinates;
- job sequences whose centre order is reversed between jobs, and a cyclic quad of centres;
- readers calling `snapshot` while batches publish, with and without a concurrent `clear_store`;
- a block view that panics for exactly one chosen job;
- two batches running on the same pool concurrently;
- every scenario at 1, 2, 4 and 8 workers.

Failures of interest: ABBA deadlock or livelock; a torn snapshot (sky from one publication and
block from another); cross-job or cross-batch contamination; token leak or poisoning; light that
differs from a serial engine; an escaped panic that aborts the process; a hung batch.

Out of scope: changing the block view during a batch (the API forbids it), vanilla parity (the
fixture is absent here), and model checking of the mutex protocol (see Limitations).

## Tests added

All new tests live in `crates/ferrite-light/tests/pool_stress.rs`. Each pool interaction runs
inside a 60 s watchdog thread, so a deadlock fails the test with a "probable deadlock" message
instead of hanging the suite; assertion failures are re-raised with their original payload.

1. `randomized_overlapping_jobs_match_serial_at_every_worker_count` — a fixed-seed LCG produces
   20 jobs over a 7x7 grid of centres (span +-3 chunks) with at least three duplicates and both
   negative axes (the fixture checks these properties). Terrain is stone with caves, water,
   torches, glass and leaves over 9x9 chunks. At each worker count: every snapshot is compared
   block-by-block against a serial `LightEngine` over all five stored sections (including the two
   propagation sections) and through the world-coordinate accessor; duplicate jobs must return
   equal snapshots; `snapshot()` in the store must return the job's result; and all worker counts
   must be bit-identical to one worker.
2. `snapshots_are_whole_publications_under_concurrent_readers_and_clear_store` — two worlds whose
   publications provably differ in both light kinds at every observed centre: the second adds an
   emitting torch at y=32 and an opaque stone block at y=40 inside each centre chunk, and the test
   asserts the sky and block arrays of section 2 differ per centre (so a snapshot mixing sky from
   one publication with block from another matches neither). The candidate values are themselves
   validated block-by-block against serial engines. Phase 1: three reader threads spin on
   `snapshot()` for nine overlapping centres while six batches alternate the two worlds; every
   observed snapshot must have the right bounds, five sections, no `Null` section, and equal one
   whole candidate. Phase 2: two readers plus a `clear_store` thread race four publishing rounds;
   only atomicity and the absence of panics are asserted, because `clear_store` is documented as
   quiescent-pool-only. Phase 3: a quiescent clear drops every publication and the next batch
   republishes it.
3. `opposite_order_and_cyclic_centres_complete_without_deadlock` — 80 jobs alternating
   (0,0)/(1,0) in both orders, the same reversal on the z axis, a negative (-1,0)/(0,0) pair, and
   the cyclic quad (0,0),(1,0),(1,1),(0,1). If publication acquired a job's own centre first and
   then the rest of the neighbourhood in a fixed order, the first two jobs (with two or more
   workers) and the quad would form a lock cycle. Results are also checked against serial at every
   worker count.
4. `panicking_job_publishes_nothing_and_leaves_the_pool_usable` — one block view panics only
   inside centre (6,0)'s 3x3 read set (x 80..=127, z -16..=31), which no other job in the batch
   reads. Per worker count: a good publication for (6,0) is seeded first; the failing batch must
   return `WorkerPanicked` naming a job at (6,0) with the original payload string; the seeded
   snapshot must be unchanged, so "publishes nothing" covers both "no new state" and "does not
   clobber the old value"; jobs that did not panic may have published (they always do with more
   than one worker) but only their own correct snapshot. Then a healthy batch and a single-job
   batch for (6,0) must complete. With one worker, any token the panicking job failed to release
   would deadlock that follow-up batch and trip the watchdog. Direct `try_lock` on the tokens is
   not reachable from an integration test; the crate-internal
   `panic_while_holding_chunk_tokens_releases_every_token` covers that half.
5. `concurrent_batches_on_one_pool_keep_their_own_results` — `relight` takes `&self` and the pool
   is `Sync`, so two threads run three batches each on the same pool with different worlds and the
   same overlapping centres, at every worker count. Every returned snapshot must equal its own
   world's canonical value (no cross-batch contamination), and the store must end holding one
   whole publication of either batch, never a mix.

## Commands and results

Environment: Linux x86_64, cargo 1.100.0, `stable-x86_64-unknown-linux-gnu` (per
`rust-toolchain.toml`), shared host. Rust data from `vanilla/26.3/generated` copied from the
sibling worktree; `vanilla/` remains gitignored.

```sh
source tools/env.sh
cargo +stable fmt --all                                              # exit 0, no other file changed
cargo +stable clippy -p ferrite-light --all-targets -- -D warnings   # exit 0
cargo +stable test -p ferrite-light                                  # exit 0
```

| target | tests | passed | failed | time |
|---|---:|---:|---:|---:|
| library unit tests | 3 | 3 | 0 | 0.00 s |
| `tests/light.rs` | 14 | 14 | 0 | 0.41 s |
| `tests/pool.rs` (pre-existing) | 5 | 5 | 0 | 3.01 s |
| `tests/pool_stress.rs` (new) | 5 | 5 | 0 | 5.68 s |
| `tests/vanilla.rs` | 1 | 1 (skipped inside) | 0 | 0.00 s |
| doc-tests | 0 | 0 | 0 | 0.00 s |

Total 28 passed, 0 failed. The new suite was then run three more times for stability: 5 passed /
0 failed each, 5.62 s, 5.68 s, 5.55 s. No flake and no watchdog expiry in four runs.

`tests/vanilla.rs::compare_fixture_light_when_available` returns early because
`vanilla/26.3/fixture-world` is absent in this worktree; it reports the skip and performs no
comparison, so its "ok" is not vanilla-parity evidence.

Clippy's only output is a pre-existing workspace-manifest warning (`workspace.package.rust-version`
is unused), unrelated to this crate.

## Defects

None found. Nothing in `src/` was changed; had a defect been found, the failing test would have
been left in place and escalated rather than fixed or weakened here.

## Open questions and caveats (not defects)

1. **Batch atomicity is not provided.** Each job publishes only its own centre, so during a 3x3
   resubmission after an edit a reader can see one chunk from the new batch and a neighbour from
   the old one. Per-snapshot atomicity (below) is unaffected. Consumers that need a consistent
   3x3 must re-read after the batch returns.
2. **A failed batch is not a rollback.** Jobs that completed before another job panicked have
   already published. `Err(WorkerPanicked)` means "this batch is incomplete", not "the store is
   unchanged"; test 4 asserts those early publications are individually correct and that the
   panicking job's cell keeps its previous value. A caller that treats the error as a no-op would
   leave a partially updated neighbourhood.
3. **`clear_store` during a batch can orphan publications.** The store's table is cleared while a
   publishing job already holds `Arc<ChunkCell>`s from the old generation; the write lands in a
   cell that is no longer indexed. This is documented in `pool.rs` and test 2 phase 2 exercises it
   for no-panic and atomicity only. If the store is later used for eviction, it needs a generation
   counter or a quiesce-before-clear protocol.
4. **Invalid job coordinates surface as worker panics.** `chunk_neighbourhood` uses `checked_add`
   and `LightEngine::new`/`recenter` panic for centres at the `i32` edges. Inside a worker that
   becomes `WorkerPanicked { job, message: "chunk x out of range" }`, which reads like a worker
   fault rather than a bad job. A validating variant (`InvalidJob`) would be clearer; that is an
   API change, not a concurrency fix.
5. **Panic recovery needs an unwinding build.** The workspace release profile sets
   `panic = "abort"`, so the panic test cannot run in release and a panicking job aborts a release
   server. Unchanged by this review.
6. **Reader latency couples to publication.** A job holds all nine neighbourhood tokens while it
   writes, so `snapshot` for chunk C blocks while any job centred at C, C+-1 publishes. Values read
   are unaffected; only latency. This is the intended cost of guarding the 3x3 read set.
7. **No fairness or backpressure.** Job indices come from `fetch_add`; a hot centre serialises its
   workers on nine tokens, and two concurrent batches run `2 * workers` scoped threads with
   nothing bounding that. The benchmark notes the 8-worker point already oversubscribes the
   machine's six physical cores.
8. **Thread start-up per batch.** Already noted in the design doc; it matters for small section
   ranges, not for full-height jobs.

## Verdict detail: lock ordering

Sound. The evidence:

- The only place that holds more than one cell token is `ChunkTable::publish`
  (`src/pool.rs:285`). It takes exactly the nine distinct cells of one 3x3, sorts them by
  `(chunk_x, chunk_z)` (numeric, so negatives order below zero), collects all nine guards, and
  writes only the sorted centre. Every job therefore acquires any shared pair of tokens in the
  same relative order; a total order over positions makes a cycle impossible.
- No other path takes two cell tokens. `ChunkTable::get` (`src/pool.rs:296`) locks the chunk
  table, clones the cell `Arc`, and the table guard is dropped at the end of that `let` statement,
  before the cell is locked. `LightPool::snapshot` (`:343`) uses it, and `clear_store` (`:348`)
  takes only the table mutex. The table mutex can therefore never be part of a cycle, and there is
  no engine-wide lock.
- `clear_store` can swap table generations between two of `publish`'s nine lookups, because
  `neighbourhood` (`:276`) looks each cell up separately. One publication may then mix cells from
  two generations, but the sort key is the position, not the cell identity, so the order argument
  still holds; test 2 phase 2 exercises exactly that race.
- Empirically, the reversed and cyclic job lists (test 3) and the two-batch race (test 5)
  completed within the watchdog at 1/2/4/8 workers in four consecutive runs. A centre-first
  publication order would deadlock on test 3 with two or more workers, so the test has teeth.

## Verdict detail: snapshot atomicity

Atomic per published centre. `LightSnapshot` is immutable after construction. Publication stores
one `Option<Arc<LightSnapshot>>` into the centre cell while holding that cell's token; a reader
clones that same `Option<Arc<_>>` under the same token. A reader therefore observes the previous
publication, the new one, or none, and never a partially written snapshot or a mix of two
publications' arrays. Test 2 makes tearing detectable by making the two candidate publications
differ in both sky and block light in the observed section (asserted per centre), and readers only
ever observed exact candidates; test 5 shows the same for two batches racing on the same centres.

Not provided: atomicity across chunks. Cross-chunk reads during a batch are eventually consistent,
as noted in open question 1.

## Limitations of this review

- No source changes, so no fix was attempted and none was needed.
- Interleaving coverage is probabilistic. Job sets are deterministic (fixed seed), but thread
  scheduling is not; four runs at four worker counts is strong evidence, not a proof. A model of
  `ChunkTable` in loom, or Miri/TSan, would be the next step if stronger evidence is required.
  Neither was run here: the release profile aborts on panic, and no sanitizer/model-checking lane
  exists in this worktree.
- Only the `test` profile (unwinding) was exercised.
- Timings are from a shared host and are not performance claims.
