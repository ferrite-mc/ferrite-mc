# Runtime worlds handoff — 2026-11-11

## Current state

Runtime-world work is in progress in `C:\Users\taegy\orca\workspaces\ferrite-mc-ref\plan1104\target\release-prep-v0.1.5`, on branch `release/v0.1.6`. At this handoff, `main` and `release/v0.1.6` both point to `6b711ca` and the worktree is clean. Runtime-world changes were pushed directly to both branches at the user's request. The initial implementation commit is `dceb03b`; follow-up commits address CI failures described below.

The current implementation adds persistent world descriptors, a separate scheduler per `WorldId`, Rust world create/unload messages, and a Bukkit `Server#createWorld`/`unloadWorld` route through the plugin host. A runtime world uses the existing worldgen host and inherits its terrain settings. Java `WorldCreator` requests are currently limited to `NORMAL` type, seed `0`, structures enabled, and no custom generator. Paper's `WorldCreator(String)` initializes its seed randomly, so an unconfigured creator is rejected before reaching the host; the common `createWorld(new WorldCreator(name))` call remains unsupported until seed handling is implemented or the API contract is revised.

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

- Latest GitHub Actions run: [42919492436](https://github.com/ferrite-mc/ferrite-mc/actions/runs/42919492436). On both Linux and Windows, Java fixture build, `cargo fmt`, and `cargo clippy` passed. The full `cargo test --workspace` step failed on both OSes.
- Rust server unit tests passed: 724/724 on Linux and 724/724 on Windows. Three `crates/ferrite-server/tests/plugin_lifecycle.rs` tests failed on both OSes:
  - `server_loads_plugins_at_startup_and_disables_them_on_stop`: expected chat `pong`, got `Unknown or incomplete command`; events only showed `join Bot`.
  - `typical_plugin_uses_config_commands_listener_and_repeating_task`: expected `ip ...` in the plugin event file; events showed only `async task` and tab completion.
  - `plugin_api_calls_reach_the_client_as_packets`: expected `verify chunk-sent ...`; events showed `join Bot` and `chunk-load 0 0`.
- The targeted Java test `ApiSurfaceTest.serverSurfaceImplementsTheBoundedCallsAndDocumentsTheRest` passed locally after specifying supported seed `0` in its no-host case.
- `cargo fmt --all -- --check` passed after the Rust Clippy fixes. No local Rust workspace tests were run; the CI test job is the evidence above.

## Known gaps to finish

1. **Player travel into runtime worlds is not wired.** `PlayerCommand::Teleport` still accepts only the default world; player-held chunk signals, tracking observers, transient entity keys, and entity sidecar paths still use `Dimension`. A player cannot yet safely enter a runtime world through Bukkit teleport or vanilla routing.
2. **Shutdown persistence is incomplete.** The shutdown path still gathers all regions together and flushes only the primary source. Runtime entity regions and runtime chunk sources must be saved/flushed separately before server shutdown.
3. **Unload failure recovery is incomplete.** The runtime scheduler is removed before entity/chunk flushing; if persistence fails, the world remains catalogued but its scheduler is gone. Make unload transactional or restore the scheduler on save failure.
4. **World unload lifecycle edge cases need validation.** The Java layer fires the cancellable event and removes the Java world only after the Rust response, but verify event cancellation, player-in-world refusal, `save=false`, and failed flush behavior.
5. **Creator settings and seed semantics are constrained.** `WorldCreator` currently rejects custom generators, nonzero seeds, structure-disabled worlds, and non-`NORMAL` world types. Since Paper's default creator seed is random rather than zero, basic seed handling must be resolved before normal plugin world creation works.
6. **Plugin lifecycle CI is failing.** Resolve the three `plugin_lifecycle` failures above before treating the branch as CI-clean. The test logs suggest plugin commands/events are not reaching the expected path; trace command dispatch and bridge callbacks from plugin startup through the game loop.
7. **Real plugin/client validation is outstanding.** Exercise create/reopen/unload, plugin `onEnable` creation, world lookup, block reads/writes, player travel, and chunk/entity persistence using the real client/plugin fixture.

## Important working constraints

- Do not start subagents; the user explicitly asked to stop all workers.
- The user asked to defer tests until final validation. The final CI run has now exposed the plugin lifecycle failures above; avoid repeated full test runs until those failures have a concrete fix.
- Preserve the shared `plan1104` and sibling worktrees. The `plan1104\target` folder contains worktrees and runtime world data; never run an unrestricted `cargo clean` or delete the target root.
- `cargo clean -p` was used to reclaim package build outputs while preserving source worktrees and `target\debug\world`. Do not remove world data or unrelated worktrees as part of cache cleanup.

## Next implementation sequence

1. Diagnose and fix the three `plugin_lifecycle` CI failures; rerun the focused integration test before the next full CI run.
2. Finish world-aware player commands and change-world handling; migrate holds/releases, tracking, transient entities, and save paths to `WorldId`.
3. Make runtime unload transactional and persist each world's entities and chunks both on unload and shutdown.
4. Implement seed semantics for `WorldCreator` (including its random default), then validate create/reopen/unload and real plugin/client behavior.
5. Run final workspace tests and lint checks, then update `docs/BACKLOG.md` and release notes with the actual supported API surface.
