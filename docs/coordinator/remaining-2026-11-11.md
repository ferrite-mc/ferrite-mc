# Remaining work audit — 2026-11-11

> **Historical snapshot (2026-11-11).** Superseded by [`docs/BACKLOG.md`](../BACKLOG.md)
> (2026-11-14) and the current source. Do not use this file as the current work queue; it is kept
> for provenance. Some items below are already closed by the merged lanes.

Base: `main` = `origin/main` = `6aca3fc`; written on `w5-residual` (L4). Sources read: the coordinator
`task-queue.md` (it lives only on `origin/integrate/worker-20261109`, not on `main`), `docs/BACKLOG.md`,
`docs/OPTIMIZATIONS.md`, `docs/plugin-api-stubs.md`, `docs/plugin-stubs-status.md`, `docs/pistons.md`,
`docs/end-dimension.md`, `docs/handoff-runtime-worlds-2026-11-11.md`, `README.md`, and the lane briefs
`w5-world-plugin`, `w5-ai-gameplay`, `w5-perf-docs`, `w5-residual` in `~/ferrite/briefs/`.
Code scan: `TODO|FIXME|unimplemented|not implemented|unsupported|todo!|panic!("…not…")` over `crates/` and `java/`.

Owners: `L1` = `w5-world-plugin`, `L2` = `w5-ai-gameplay`, `L3` = `w5-perf-docs`, `L4` = this lane,
`manual` = needs a human or hardware this host lacks, `closed` = source or a test proves it done.
An item is `closed` only with a cited source line. L1/L2/L3 status is "assigned", not "done": none of those
lanes had landed on `main` when this was written.

## Open items

| # | Item | Evidence | Owner | Next action |
|---|---|---|---|---|
| 1 | Dependabot #10 (`glib` 0.18.5, GHSA-wrw7-89jp-8q8g, fixed in 0.20) | `Cargo.lock:1761`; only reachable through `gtk 0.18.2` <- `tauri 2.12.5` (`crates/ferrite-server/Cargo.toml:46`, `cfg(windows)`) | L4 / manual | No safe upgrade: see "Dependabot #10". Re-check when a stable tauri release moves to `gtk 0.19`+. Do not dismiss the alert. |
| 2 | Windows CI timeout on `6aca3fc` | run 43040195519: `test (windows-latest)` failed, `cargo test` hit the 25-minute step limit (`.github/workflows/ci.yml:34` job limit 30). No assertion failed; the lib stage stalled on `game::integration::four_crystals_respawn_the_dragon_through_the_full_sequence` (`crates/ferrite-server/src/game/integration.rs`), last log line 14:44:59Z. Ubuntu passed that test (about 3 min under load; 16.3 s alone on this host) | L3 | End integration test, so L3-adjacent: not reproducible on Linux, left unedited. L3 should bound the test (it relies on `/tick rate 10000` and 5 ms sleeps) or find the Windows-only stall. Run 43034005859 (`abdb2b6`) also failed: Ubuntu timed out in `a_new_end_gateway_sends_the_block_entity_age_for_its_beam`; Windows failed `plugin_command_world.rs:137` before `fa7e422` serialized commands. |
| 3 | Real 26.3 client join and visual checks | `docs/BACKLOG.md:40-45`, `README.md:9`, `docs/end-dimension.md` (client-only section), `docs/pistons.md` (last paragraph) | manual | Run `python tools/wirecheck/run.py`, then L3's manual checklist, against a real client. Nothing here claims it. |
| 4 | Retired worktrees and `target-*` caches | `docs/coordinator/cleanup-2026-11-11.md` | manual (coordinator) | Run the listed commands after L1-L3 finish. 15 stale caches, 12 merged-branch worktrees. |
| 5 | Math/English homework | outside the repo | manual | Not tracked in this repo. |
| 6 | Runtime world: player `WorldId` not persisted, join snapshot reports default world | `crates/ferrite-server/src/game/mod.rs:1765` (`register_player`) | L1 | Brief item 3: persist the `WorldId` and restore on join. |
| 7 | `SummonCrystal` still routes by `Dimension` | `crates/ferrite-server/src/game/commands.rs:709`, `game/mod.rs:1869`, `:1995` | L1 | Brief `w5-world-plugin` (`rw-world-push` cherry-pick `70b4ba4`). |
| 8 | `Block#setType` property loss on the runtime path; entities visible only after a player re-enters a reopened world | `rw-world-content` is 1 commit ahead of `main` (unmerged) | L1 | Land W2 as briefed. |
| 9 | `Player#breakBlock`, `Player#chat`, 1-arg `broadcastSlotBreak`, `broadcastHurtAnimation`, `playPickupItemAnimation` | brief `w5-world-plugin` item 4; `java/ferrite-api-impl/src/main/java/dev/ferrite/server/PlayerStubs.java` | L1 | Implement with tests. |
| 10 | `Structure#getEntities` / non-empty `EntityTransformer` | `java/ferrite-api-impl/src/main/java/dev/ferrite/server/FerriteTemplate.java:118,176`, `CapturedStructure.java:505,539` | L1 | Implement or document the exact blocker (brief item 4). `docs/BACKLOG.md:65` is stale: P3 added `getEntities`. |
| 11 | `World#sendBlockUpdate` signs only | `PlayerMisc.java:368-371` | L1 | Extend per tile-entity type or keep refusing. |
| 12 | `Player#launchProjectile` Arrow only; `playEffect` data types; `EquipmentSlot` set | `PlayerMisc.java:231,599`, `FerritePlayerInventory.java:68` | L1 | Extend when L2's projectiles land. |
| 13 | Cartography table menu | `crates/ferrite-server/src/plugins/inventory.rs:382` | L1 | Menu plus open-inventory support, or keep the explicit refusal. |
| 14 | Lifecycle event handlers | `java/ferrite-api-impl/src/main/java/dev/ferrite/server/FerriteInternalBridge.java:39` | L1 | Not briefed; decide whether Paper lifecycle API is in scope. |
| 15 | Plugin world environment other than the supported set | `crates/ferrite-server/src/main.rs:583` | L1 | Keep refusing or extend with `createWorld`. |
| 16 | Entity replacement on chunk reload (overlapping viewers, re-entry while a save is pending) | `docs/BACKLOG.md:49-52` | L2 | Brief item 1; regression test with two viewers. |
| 17 | Cross-region mob AI: nearby player in another region invisible to targeting; blocked melee does not knock the attacker back | `docs/BACKLOG.md:53-60` | L2 | Brief item 2. |
| 18 | Vanilla goal parity for zombie, skeleton, spider, creeper, enderman, chicken, cow, pig, sheep | `docs/BACKLOG.md:53-56` | L2 | Brief item 3. Do not claim full parity. |
| 19 | Projectiles: potion, experience bottle, thrown trident, wind charge, fishing bobber | `crates/ferrite-entity/src/potion.rs:6` ("splash and lingering potions are not implemented yet") | L2 | Brief item 4. |
| 20 | Dragon perched subphases; gateway purple/2400-tick beam; outer-island search radius; guarded-spike choice | `docs/end-dimension.md` "Remaining gaps" | L3 (docs/tests) / manual | L3 adds the restart and `Age` replay tests; subphases need a real client. `docs/end-dimension.md` and `docs/BACKLOG.md:61` disagree on strafe: strafe is implemented. |
| 21 | Piston restart during the two-tick overlay; real-client animation | `docs/pistons.md` (last two paragraphs) | L3 (test) / manual | L3 adds the restart test. Animation is client-only. |
| 22 | Chunk packet cache not invalidated when a block entity changes | `docs/BACKLOG.md:94`, `crates/ferrite-server/src/game/anvil.rs` | L3 | Brief item 2. |
| 23 | 800-bot baseline, MSPT/RSS, batched holder throughput | `docs/BACKLOG.md:98-103`, `docs/OPTIMIZATIONS.md` | L3 | Brief item 1. |
| 24 | Docs out of step with source | `docs/BACKLOG.md:118-133` (runtime worlds "not included"), `docs/release-notes-v0.1.6.md`, `docs/plugin-api-stubs.md`, `docs/plugin-stubs-status.md`, `docs/COMMANDS-STATUS.md` | L3 | Brief item 3. |
| 25 | Smoke/join checklist and headless join smoke test | `docs/BACKLOG.md:40` | L3 | Brief item 5. |
| 26 | `/ride` module comment says riding is unsupported, but `/ride` is registered | `crates/ferrite-server/src/game/commands_late.rs:6` vs `:438` and `docs/COMMANDS-STATUS.md:55` | L4 | Comment-only fix; left for after L2 settles because riding code is L2-adjacent. |
| 27 | `AnvilChunkSource::mark_modified` is called only from tests | `crates/ferrite-server/src/game/anvil.rs:1105` (TODO), callers `game/mod.rs:5029`, `anvil.rs:1787` | L1 | `anvil.rs` is L1-owned. Decide: wire it, or delete the dead API. |
| 28 | `Command not implemented` stubs among the 94 command roots | `crates/ferrite-command/src/vanilla.rs:111-122`, `docs/COMMANDS-STATUS.md` | L3 (docs) | Delta list belongs in `COMMANDS-STATUS.md`. |
| 29 | Protocol decoder refusals: block entities, item components, inline chat types, packed signature cache, number formats | `crates/ferrite-protocol/src/packets/play.rs:104,1127,1462,1508,2495,2539` | L4 / not scheduled | Server-bound decode paths for inputs a vanilla client does not send; no action unless wirecheck shows a client sending them. |
| 30 | Multi-version translation layer | `docs/BACKLOG.md:91`, branch `origin/wip/day2-2026-10-28` | not scheduled | Out of scope for W5; needs a decision. |
| 31 | Compression and encryption off the reader/writer threads; chat/command rate limits | `docs/BACKLOG.md:92-93` | not scheduled | Needs a plan; touches `ferrite-net`, which no lane owns. |
| 32 | Default `plugins/` directory not created on startup | `crates/ferrite-server/src/plugins.rs:1140` (`has_plugin_jars` only reads); nothing created it | closed (L4) | Fixed on `w5-residual`: `ensure_plugins_dir` in `crates/ferrite-server/src/main.rs`, test `a_missing_plugins_directory_is_created_and_an_existing_one_is_kept`. Fresh-install, custom-path and headless runs were not exercised end to end. |
| 33 | Windows ZIP release verification | `docs/BACKLOG.md:109-110` | manual | Check the v0.1.6 ZIP on Windows and attach it to the release. |
| 34 | Shoulder parrot is bookkeeping only | `PlayerMisc.java:214-215`, `PlayerExtras.java:345` | L2 | Needs an entity plus display; assigned with entity work. |
| 35 | Metadata breadth, attribute packets, passengers/vehicles beyond `/ride` | `docs/BACKLOG.md:67` | L2 | Part of entity parity. |
| 36 | Waterlogging, fluid/redstone interaction, flow into unloaded chunks; maps hidden cursors/captions | `docs/BACKLOG.md:83-87` | not scheduled | Audit against vanilla before implementing. |
| 37 | Plugin sees the old world between a dimension change and the next movement packet | task-queue "Still open after wave 3" | L1 | `Signal::ChangeDimension` push is in the brief (`70b4ba4`). |
| 38 | Gap-7 Java steps still use the async scheduler; overlapping plugin commands | task-queue "Still open after wave 3"; `fa7e422` serialized commands | closed (partial) | `fa7e422` serializes commands and releases the bridge lock. Gap-7 sync-scheduler step: verify in `crates/ferrite-server/tests/plugin_lifecycle.rs` after L1 lands. |

## Open-item count

38 rows: 36 open, 1 partly closed (#38), 1 closed on this branch (#32). Primary owner: L1 12 (#6-#15, #27, #37),
L2 6 (#16-#19, #34, #35), L3 8 (#2, #20-#25, #28), L4 3 (#1, #26, #29), manual 4 (#3, #4, #5, #33),
no owner 3 (#30, #31, #36), closed 2 (#32, #38). #1, #20 and #21 also carry a manual or blocked part.

Items **not covered by any lane** (L1-L3): #26, #29, #30, #31, #36. L4 can take #26 once L1/L2 land;
#29-#31 and #36 need a coordinator decision. Related docs: `cleanup-2026-11-11.md`, `orca-4lane.md`.

## Closed, with proof

| Item | Evidence |
|---|---|
| Keepalive/read timeout 30 s | `crates/ferrite-net/src/lib.rs:26` (`DEFAULT_READ_TIMEOUT`), `:139`. `docs/BACKLOG.md:93` is stale on that half. |
| Runtime-world create/unload, transactional unload, plugin command thread | `git log` `fa7e422`, `0f8ca6c`; `crates/ferrite-server/tests/plugin_command_world.rs`, `plugin_lifecycle.rs` |
| `isChunkLoaded` / `isChunkGenerated` / `loadChunk` on runtime worlds | `0f8ca6c` |
| Light-fixture hang | task-queue wave 2: `[profile.dev.package.ferrite-light]`; `crates/ferrite-light/tests/vanilla.rs:155` still has the test |

## Dependabot #10

- Package `glib` 0.18.5, GHSA-wrw7-89jp-8q8g / RUSTSEC-2024-0429, medium. Vulnerable `>= 0.15.0, < 0.20.0`, patched 0.20.0.
- `cargo tree -i glib@0.18.5 --target all`: `glib` <- `atk`, `cairo-rs`, `gdk`, `gdk-pixbuf`, `gio`, `gtk 0.18.2`, `pango`, ... <-
  `tao 0.37.2`, `wry 0.57.2`, `webkit2gtk 2.0.2`, `muda 0.20.0`, `tauri-runtime(-wry)` <- `tauri 2.12.5` <- `ferrite-server`.
- `cargo tree -i glib@0.18.5 --target x86_64-pc-windows-msvc` and `--target x86_64-unknown-linux-gnu` both print nothing: `ferrite-server`
  depends on `tauri` only under `cfg(windows)` and the GTK stack is Linux-only inside tauri, so no supported build compiles `glib`.
  It stays in `Cargo.lock` because the lockfile resolves every target.
- Blocker: `cargo update --dry-run -p glib` and `-p tauri -p wry -p tao` lock 0 packages. `gtk 0.18.2` requires `glib ^0.18`. The
  next `gtk` (0.19.0) requires `glib ^0.22`, but stable `tauri` 2.x does not use it; the newest tauri on crates.io is `3.0.0-alpha.6`.
  Not fixed: moving to a tauri alpha is not a safe update.
- Minimal affected path: lockfile only. No `ferrite-server` code path calls `glib::VariantStrIter`.
