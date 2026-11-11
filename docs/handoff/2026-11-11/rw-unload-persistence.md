# Runtime-world unload transaction and per-world shutdown persistence

Baseline b6a7b3e, branch `rw-unload-persistence`.

## Changes
- New `crates/ferrite-server/src/game/runtime_persist.rs`:
  - `unload_runtime_world`: stops the scheduler, drains entities, writes entity sidecars, then `flush_quiesced`.
    On any failure the scheduler is rebuilt (`insert_runtime_scheduler`), chunk holds, entities and queued
    tasks are put back, the world stays catalogued, and the error says "unload aborted; the world was restored"
    (or that the scheduler could not be restored). The catalog entry and plugin view are only dropped after success.
  - `persist_shutdown`: every stopped world is saved separately. Vanilla dimensions use `dimension.storage_root(world)`
    and `dimensions.source(dim)`; runtime worlds use their own source path and chunk source. Sources are flushed once each
    (deduped by pointer). All worlds are attempted and all errors are joined, so one failure cannot hide another.
- `mod.rs`: Unload and Shutdown arms delegate to the module; the runtime TickContext builder is now a shared closure.
  The primary `source` is still flushed if it is not the overworld's source.
- `entity_save.rs`: `drain_regions` / `save_chunks` (attempt all chunks, return first error) split out of `save_regions`.
- `region.rs`: `Region::held_chunk_counts`.

Behaviour note: shutdown previously wrote all dimensions' entities into the world root; they are now written under each
dimension's storage root, matching how they are loaded.

## Known limits
- Unload failure restores entities, holds and queued tasks; transient per-region state (fluid/random ticks, etc.)
  restarts empty. Chunk data lives in the chunk source and is untouched.

## Acceptance (raw output; `RUSTUP_TOOLCHAIN=stable CARGO_TARGET_DIR=$HOME/ferrite/target-rw3`)
`cargo test -p ferrite-server --lib runtime_persist -- --nocapture`
```
running 2 tests
test game::runtime_persist::tests::failed_flush_keeps_the_runtime_world_usable ... ok
test game::runtime_persist::tests::shutdown_then_reopen_preserves_runtime_chunks_and_entities ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 724 filtered out; finished in 8.31s

EXIT=0
```
`cargo clippy -p ferrite-server --all-targets -- -D warnings` -> exit 0. `cargo fmt -p ferrite-server` applied.

- Failure test forces the failure by putting a regular file at `<world>/entities`; asserts the error, catalog entry,
  scheduler presence, entity + chunk hold still owned, then a retry after removing the fault succeeds and writes the sidecar.
- Shutdown test edits a block and a cow in a runtime world, runs `persist_shutdown`, reopens the world by UUID and checks both.
- Full `cargo test --workspace` was not run (only the ferrite-server targets above).
