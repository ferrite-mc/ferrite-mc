# World creation and unload contract (seed semantics + unload edge cases)

Branch `world-lifecycle-contract`, created from `rw-unload-persistence` @ ce68ae5 (the transactional unload work,
already committed there). Not pushed.

## Seed semantics
Paper's `new WorldCreator(name)` picks a random seed itself, so every creator carries one; `createWorld` used to throw
unless it was 0. Now any `long` is accepted.

- Java `FerriteServer#createWorld` sends `createRuntimeWorld(String,String,long)` (name, environment, seed) and reads a
  5-part reply `uuid|x|y|z|seed`. `FerriteWorld#getSeed` reports the effective seed (the host's live answer still wins
  for the default world).
- Rust: `RuntimeWorldDescriptor.seed`; `RuntimeWorldCatalog::open_persistent` takes a `RuntimeWorldRequest
  { dimension, spawn, seed }`. A new world writes the seed to `level.dat` at `Data.WorldGenSettings.seed` (long)
  next to the spawn (`anvil::write_spawn_and_seed` / `read_seed`).
- **An existing world keeps the seed stored on disk**; the requested seed only applies at creation (Paper does the same
  for an existing world). A `level.dat` with no seed (written before this change) reopens as seed 0, which is the only
  seed such a world could have been created with.
- Plumbing: `GameMessage::CreateRuntimeWorld` gained `seed`, `RuntimeWorldCreateResult` gained the effective seed
  (`ferrite-net`, `game/mod.rs`, `main.rs`, `plugins.rs`). `ferrite-net` is outside this lane's usual crates but the
  message cannot carry the seed without it.
- Still rejected, each with its own message: custom generator, `generateStructures(false)`, world type other than
  NORMAL (message names the type), environment other than NORMAL/NETHER/THE_END. Rejections happen before any host call.

**Not done / caveats**
- The seed is stored and reported but **does not drive terrain**: runtime worlds still use the shared overworld
  terrain generator. Different seeds give the same terrain.
- `Data.WorldGenSettings.seed` is the long-standing vanilla location, but I did not verify it against a real 26.3
  `level.dat` (Ferrite's own reader is the only consumer, and the file only models spawn + seed). If 26.x moved
  world-gen settings to a separate data file, this needs aligning before importing vanilla worlds.
- A create failure on the Rust side still reaches Java as `Ferrite: not implemented: Server#createRuntimeWorld(...)`
  because the host call returns no reply for `Err`; the real error text is lost. Pre-existing, left alone.

## Unload edge cases
| Case | Behaviour | Test (post-condition asserted) |
|---|---|---|
| `WorldUnloadEvent` cancelled | `unloadWorld` returns false before any host call | `ApiSurfaceTest.aCancelledWorldUnloadEventLeavesTheWorldIntact`: false, host saw no unload, world still registered and in `getWorlds()`; unloads fine once the veto is removed |
| Player inside | host refuses, nothing stopped | `runtime_persist::unload_with_a_player_inside_is_refused_and_changes_nothing`: catalog entry, scheduler, chunk hold and the cow all intact, nothing written to disk; unloads after the player leaves |
| `save=false` | no persistence, world fully removed | `runtime_persist::unload_without_save_skips_persistence_and_leaves_no_half_removed_world` (+ `ApiSurfaceTest.unloadingWithoutSavingTellsTheHostToSkipPersistence`): catalog, name index and scheduler all gone; a deliberately broken entity dir is untouched (proves nothing was written); reopening shows the unsaved block edit and cow are not on disk, same UUID; Java sent `save=false` and unregistered the world |
| Failed flush | world restored | `runtime_persist::failed_chunk_flush_restores_the_scheduler_and_catalog_entry` (region files cannot be created; entity write succeeds first) plus the existing `failed_flush_keeps_the_runtime_world_usable` (entity write fails): catalog entry + scheduler present, cow + hold owned, unsaved block edit still in memory, retry succeeds |

**Bug found and fixed while doing this:** when the Rust unload handler returned `Err` (player inside, failed flush) the
host call produced no reply, so Java's `HostBridge` threw `UnsupportedOperationException` instead of `unloadWorld`
returning `false`. `plugins::unload_reply` now answers `"false"` (and logs a warning). Covered by
`plugins::tests::a_refused_unload_answers_false_instead_of_no_reply` and
`ApiSurfaceTest.aHostRefusalSuchAsAPlayerInsideOrAFailedFlushKeepsTheWorldRegistered`.

## Tests added
Rust (`ferrite-server`): `runtime_world_seed_is_stored_in_level_dat_and_survives_reopen` (0, `i64::MAX`, negative; a
different requested seed on reopen does not rewrite), `a_level_dat_without_a_seed_reopens_with_seed_zero`, the three
`runtime_persist` unload tests above, `a_refused_unload_answers_false_instead_of_no_reply`.
Java (`ApiSurfaceTest`): unset seed succeeds and reaches the host, explicit seed round-trips through unload/reopen,
rejections with explicit messages and no host call, cancel, host refusal, `save=false`.

Java tests use `HostBridge.transport` with a fake host that mimics the Rust contract (stored seed wins on reopen); the
real Rust behaviour is covered by the Rust tests, so the two halves are tested separately, not end to end through JNI.
The cancel test inserts its handler into `FerritePluginManager.handlers` by reflection: the manager only takes handlers
from plugins it loaded, and loading one in `ApiSurfaceTest` would collide with `BootstrapTest` in the same JVM.
Mutation check: removing the `isCancelled()` check in `unloadWorld` makes exactly that test fail.

## Gates (raw)
Env: `RUSTUP_TOOLCHAIN=stable CARGO_TARGET_DIR=$HOME/ferrite/target-world-contract`, JDK 25.

```
cargo fmt --all -- --check                                  -> FMT_EXIT=0
cargo clippy --workspace --all-targets -- -D warnings       -> Finished `dev` profile ... CLIPPY_EXIT=0
cargo test -p ferrite-server --lib -- runtime_persist runtime_world a_level_dat a_refused_unload
  running 11 tests ... test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 721 filtered out  TARGETED_EXIT=0
gradle test (java/ferrite-api-impl, full)                   -> BUILD SUCCESSFUL, JAVA_FULL_EXIT=0
  204 tests, 0 skipped, 0 failures, 0 errors
```
```
cargo test --workspace   -> WORKSPACE_EXIT=101
  every suite ok except crates/ferrite-server/tests/plugin_lifecycle.rs: 1 passed; 3 failed
    (server_loads_plugins_at_startup_and_disables_them_on_stop, plugin_api_calls_reach_the_client_as_packets,
     typical_plugin_uses_config_commands_listener_and_repeating_task)
```
**These 3 failures are not from this change.** The identical suite on the baseline ce68ae5 (clean detached worktree,
separate target dir) fails the same 3 tests with the same panics (`plugin_lifecycle.rs:517`, `:444`, `:202`; 1 passed,
3 failed). I did not diagnose the cause (the tests time out waiting for plugin output); the integration lane owns them.

## Constraints respected
`crates/ferrite-server/tests/plugin_lifecycle.rs` untouched (it does not use world create/unload). No command-tree files
touched. Nothing pushed.
