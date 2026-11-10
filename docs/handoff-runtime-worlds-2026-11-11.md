# Runtime worlds handoff — 2026-11-11

## Current state

Runtime-world work is in progress on branch `release/v0.1.6` in the `release-prep-v0.1.5` worktree. The worktree started from commit `08ee9e3` and had no upstream branch configured. The user asked to commit and push this checkpoint, then continue the implementation in a later session.

The current implementation adds persistent world descriptors, a separate scheduler per `WorldId`, Rust world create/unload messages, and a Bukkit `Server#createWorld`/`unloadWorld` route through the plugin host. A runtime world uses the existing worldgen host and inherits its terrain settings. Java `WorldCreator` requests are currently limited to `NORMAL` type, seed `0`, structures enabled, and no custom generator. Paper's `WorldCreator(String)` initializes its seed randomly, so an unconfigured creator is rejected before reaching the host; this makes the common `createWorld(new WorldCreator(name))` call unavailable until seed handling is implemented or the API contract is revised.

## Changes in this checkpoint

- `RuntimeWorldCatalog` validates portable names/resource keys, creates a persistent world directory, preserves the Bukkit UUID in `uid.dat`, and preserves spawn in `level.dat` across unload/reopen.
- `AnvilChunkSource::flush_quiesced` waits for in-flight loads before flushing. The load task decrements the pending count even after a panic.
- `WorldId::{Vanilla, Runtime}` now keys the scheduler map. Players and global player records carry a world identity; player transfers, entity transfers, and summons route by that identity.
- The tick/scheduler construction was factored so a runtime world can receive its own independently ticking scheduler.
- `GameMessage` now has create, unload, and plugin-hook attachment variants. `World` exposes async create/unload entry points.
- Runtime source registration updates the dynamic `SourceWorldView`. Bukkit receives `WorldLoadEvent` after creation and a cancellable `WorldUnloadEvent` before requesting unload.
- Main startup now starts the game loop before enabling plugins, then attaches plugin hooks. This allows a plugin's `onEnable` to make a synchronous runtime-world request against a live game loop.
- The provided screenshot showed Clippy rejecting an unnecessary `unwrap()` in the transient entity test. The test now keeps the concrete `Arc` and passes an `Option<Arc<_>>` to the function, removing that unnecessary unwrap. Tests/Clippy have not been rerun after that edit.

## Verification performed

- `cargo fmt --all` passed.
- `cargo check -p ferrite-server` passed after the current Rust and Java-facing API changes.
- `java/ferrite-api-impl/gradlew.bat compileJava --console=plain` passed after correcting the `HostBridge.call` overloads.
- `git diff --check` passed.
- No tests have been run, per the user's instruction to defer tests until the implementation is complete.

## Known gaps to finish

1. **Player travel into runtime worlds is not wired.** `PlayerCommand::Teleport` still accepts only the default world; player-held chunk signals, tracking observers, transient entity keys, and entity sidecar paths still use `Dimension`. A player cannot yet safely enter a runtime world through Bukkit teleport or vanilla routing.
2. **Shutdown persistence is incomplete.** The shutdown path still gathers all regions together and flushes only the primary source. Runtime entity regions and runtime chunk sources must be saved/flushed separately before server shutdown.
3. **Unload failure recovery is incomplete.** The runtime scheduler is removed before entity/chunk flushing; if persistence fails, the world remains catalogued but its scheduler is gone. Make unload transactional or restore the scheduler on save failure.
4. **World unload lifecycle edge cases need validation.** The Java layer fires the cancellable event and removes the Java world only after the Rust response, but verify event cancellation, player-in-world refusal, `save=false`, and failed flush behavior.
5. **Creator settings and seed semantics are constrained.** `WorldCreator` currently rejects custom generators, nonzero seeds, structure-disabled worlds, and non-`NORMAL` world types. Since Paper's default creator seed is random rather than zero, basic seed handling must be resolved before normal plugin world creation works.
6. **Real plugin/client validation is outstanding.** Exercise create/reopen/unload, plugin `onEnable` creation, world lookup, block reads/writes, player travel, and chunk/entity persistence using the real client/plugin fixture.

## Important working constraints

- Do not start subagents; the user explicitly asked to stop all workers.
- Do not run tests until the runtime-world implementation is complete. `cargo check`, formatting, and compilation are acceptable during implementation.
- Preserve the shared `plan1104` and sibling worktrees. The `plan1104\target` folder contains worktrees and runtime world data; never run an unrestricted `cargo clean` or delete the target root.
- `cargo clean -p` was used to reclaim package build outputs while preserving source worktrees and `target\debug\world`. Do not remove world data or unrelated worktrees as part of cache cleanup.

## Next implementation sequence

1. Finish world-aware player commands and change-world handling; migrate holds/releases, tracking, transient entities, and save paths to `WorldId`.
2. Make runtime unload transactional and persist each world's entities and chunks both on unload and shutdown.
3. Add regression tests for runtime UUID/spawn persistence, same-dimension isolation, cancellation, unload failure recovery, and player travel.
4. Run final workspace tests and lint checks, then update `docs/BACKLOG.md` and release notes with the actual supported API surface.
