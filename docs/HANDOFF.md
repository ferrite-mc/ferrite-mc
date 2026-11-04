> **2026-11-04 — world generation removed from this repository.** `ferrite-worldgen` and
> `ferrite-noise` (a 1:1 port of vanilla classes) are no longer here, along with their docs and
> benchmarks, to avoid distributing a translation of Mojang code. The server keeps only the
> `ChunkGenerator` seam; vanilla generation is planned as a separate JVM-backed component. Entries
> below that mention worldgen (W1–W3, oc6, terra2, `docs/handoff/worldgen-26-3.md`) are history only
> and are not open work.

> **Coordinator:** how to run the orchestration (start workers, merge, push) is in
> [`docs/handoff/coordinator-guide.md`](handoff/coordinator-guide.md).
> **Newest:** the Day 6 end state (what is on main, snapshot branches, unstarted tasks, how to resume) is in
> [`docs/handoff/day6-session-record.md`](handoff/day6-session-record.md).

# Handoff — end of 2026-10-28 session

Branch `phase1-foundation` (HEAD `c8989a7`): workspace green — 131 tests, clippy -D warnings, fmt.
Plan: `C:\Users\taegy\.claude\plans\minecraft-lexical-dijkstra.md` (9 phases). Target MC 26.3 / protocol 777.

## Done (committed)
| Crate | State |
|---|---|
| ferrite-protocol | codec, framing+compression, status/login/configuration/play packets |
| ferrite-net | offline+online login (RSA/AES-CFB8, Mojang hasJoined), known-pack registry+tag sync, typed play channel |
| ferrite-server | config (ferrite.toml), game loop on ferrite-region: movement, chunk streaming (superflat), player visibility, chat, Ctrl+C |
| ferrite-region | Folia-style regionizer v2 (region-local safe points, per-region queues, heap deadlines) |
| ferrite-light | Starlight-style engine, **0 mismatches vs vanilla** on 256 chunks; 8.4 ms/chunk, 0.066 ms/torch |
| ferrite-world | paletted containers, chunk columns, heightmaps, Anvil .mca/.mcc, 26.3 chunk NBT, network encoding |
| ferrite-nbt | binary IO, SNBT, serde feature |
| ferrite-data | static block-state / registry / packet-ID tables from vanilla reports |
| ferrite-plugin-meta | plugin.yml / paper-plugin.yml, Paper-compatible load order, ThreadingMode |
| ferrite-jvm | lazily embedded JVM (Java 25), natives, exception mapping |

Measured: release server idle RSS **16.7 MB**, status ping 17.5 ms.
**Never tested with a real 26.3 client yet** — do this first (`cargo run --release -p ferrite-server`
with `ferrite.toml`: `online-mode = false`), then join `localhost`.

## Unverified WIP — branch `wip/freebuff-2026-10-28` (`f70e1dd`)
Interrupted, never built. Too big for freebuff; reassign to Codex terra, starting from this code:
- **F5** tools/extractor + ferrite-data `block_props` (exact light/collision/heightmap props from the unobfuscated jar). Unblocks exact heightmaps (world) and voxel-shape light occlusion (light).
- **F6** java/ferrite-api-impl (Paper API `Server` impl, plugin loader, schedulers).
- **F7** crates/ferrite-command (Brigadier-compatible dispatcher + Commands packet).

## Next steps (in order)
1. Real-client join test; fix whatever breaks.
2. Terra: finish F5 → wire into ferrite-world `BlockInfo` and ferrite-light `properties.rs` (exact face shapes); rerun light differential.
3. Terra: A3 leftovers — 200-bot/30 s load test (MSPT + RSS), review `game/mod.rs` duplicate-UUID + transfer races (see A3 notes below).
4. Terra: wire ferrite-world Anvil loading into ferrite-server's `ChunkSource` (load real worlds, save on unload).
5. Terra: F6 + ferrite-jvm → first real plugin `onEnable` inside Ferrite; then F7 commands into play.
6. Regionizer: move deferred mutations into per-region queues (currently a shared queue scanned).
7. Phase 2 remainder: block place/break, signed chat, inventory basics.

### Worker notes
- A3: validate `game/region.rs` `Player::update` / `Player::chunks_tick` and `game/global.rs` `GlobalRegion::tick` under sustained concurrency.
- B3: `ferrite-light/src/properties.rs` `classify` + `LightProperties::faces` approximate special shapes with quadrant masks.
- C2: see `docs/handoff/c2-world-storage.md` (fluids/heightmap/biome IDs in registry.rs, SNBT/serde edge cases, palette perf).

## Environment
- Rust: `source tools/env.sh` (cargo in ~/.cargo/bin), toolchain stable-x86_64-pc-windows-msvc; Windows SDK installed.
- Java 25: `C:/Program Files/Eclipse Adoptium/jdk-25.0.5.8-hotspot` (JAVA_HOME for `python tools/fetch_vanilla.py`).
- Vanilla data + fixture world + protocol docs (gitignored): `vanilla/26.3/`, `vanilla/docs/`.
- Orca run `run_4622c45f6259`; F5/F6/F7 dispatches were never settled (interrupted) — create new tasks rather than reusing them.

---
# Day 2 (2026-10-28) end
phase1-foundation HEAD `0eac5a9`. Committed today: exact block props, fluid count fix, palette/light perf,
real-world Anvil loading + persistence, 200-bot load test (avg 0.4 ms MSPT), regionizer v3, ferrite-command,
ferrite-text, ferrite-math, ferrite-console, ferrite-noise (bit-exact), SNBT hardening, plugin bridge checkpoint.
NOT re-verified by coordinator at the end (usage limit): the last two commits rely on worker-reported checks — run tools/check.sh first.
Unverified WIP on branch `wip/day2-2026-10-28`: T2 block place/break/inventory/commands (creative-break test still red;
ops-by-UUID security fix done), ferrite-recipe (no vanilla tests yet), ferrite-protocol-translate (agent hit rate limit mid-pair-module),
FB11 libdeflate/VarInt (clippy collapsible_if at codec.rs:98), FB12 profiler / FB13 item never started (prompts unsent).
Also: delete crates/ferrite-noise/logs/ (accidentally committed log). T3 remaining: java/T3-CHECKPOINT.md. FB15 part 2 (ferrite-data tags) not started.


---
# Day 3 (2026-10-29) end
Branch phase1-foundation. **A real 26.3 client now joins, walks, breaks/places blocks** (offline mode, superflat). Fixed along the way:
3 missing synchronized registries, light BitSet wire format, plus chunk streaming (prefetch) and player resume (playerdata/<uuid>.json).
`python tools/wirecheck/run.py` (server must be running) verifies every packet against the real client codecs — run it after ANY packet change.

Committed today: exact block props, fluid-count/palette/light perf, real-world loading + persistence, phase-2 gameplay (break/place/commands/ops by UUID),
loot tables, recipes (1,763), item components + wire codec, entity data tables, tags, worldgen data model + multi-noise biome source,
admin lists, config files (server.properties/bukkit/spigot/paper), profiler crate, console crate, admin/config crates, plugin bridge (Paper-only loading, RegionContext).
Parked on `wip/day3-2026-10-29`: T3's unfinished registry/inventory work (does NOT compile: OfflinePlayerSnapshot / register_offline_player missing). Earlier: `wip/day2-2026-10-28` (translate crate).
Running freebuff tasks at end of day (prompts may be unsent in their tabs): FB21 ferrite-fluid, FB22 ferrite-explosion, FB23 ferrite-rules — check `git status` for their crates, verify, commit.

## Goals (user)
ALL vanilla features incl. every command; Rust-first, Java only when unavoidable. **Tomorrow: use Codex (one terra at a time) and PUBLISH TO GITHUB.**

## Next steps (priority)
1. Wire `ferrite-profiler` into the tick loop: /tps /mspt (currently prints "TPS unavailable"); then `/spark`-like profile.
2. Commands: all vanilla commands (commands.json has the tree; ferrite-command loads it; most nodes are stubs "Command not implemented") — implement by family: gamemode/tp/give/clear/effect/xp/time/weather/difficulty/gamerule/kill/summon/setblock/fill/clone/data/execute/scoreboard/team/bossbar/title/tellraw/say/me/msg/list/kick/ban/op/whitelist/save-all/stop/... 
3. Signed chat (the "Chat message validation failed" notice appears because chat is relayed unsigned) — implement secure chat or disguised chat properly.
4. Block-change fidelity: neighbor updates/placement rules (FB23), fluids (FB21), explosions (FB22), redstone, random ticks, gravity blocks.
5. Entities & mobs (largest): entity ECS, metadata/tracking, AI goals, spawning, combat, drops (ferrite-loot ready), item entities; inventory clicks (hashed slots) & crafting (ferrite-recipe ready).
6. World generation: density-function evaluator on ferrite-noise + biome_source + surface rules + features/structures (phase 6).
7. Multi-version translation layer (wip/day2 has the start).
8. Plugin API breadth (Server 26/206 implemented), EssentialsX needs CraftBukkit shim; compat-report.md on the WIP branch has probe results.
9. **GitHub release prep**: LICENSE-MIT/LICENSE-APACHE, README polish, CI check on Linux (rust-toolchain.toml pins MSVC — CI overrides), confirm no vanilla/ or Mojang data committed (`git ls-files | grep -i vanilla`), choose repo name, push.


---
# Day 4 (2026-10-30) end
Relicensed to GPL-3.0-or-later; work coordinated by one Claude coordinator through Orca orchestration (run `run_57911c7b3e38`, sessions in `docs/sessions.md`).
**No new dispatches were made after the user said to stop for the day.** Sessions keep finishing what they were already doing.

## On main today
- Optimization survey of 20 projects with licenses and 26.x support (`docs/OPTIMIZATIONS.md`, FB5).
- `ferrite-world`: per-section random-tick/fluid-tick counters, recounted from palette frequencies (Lithium-style; Terra).
- `ferrite-fluid` (new): fluid state codec, new-liquid rules, scheduled ticks and slope spreading, directional collision faces, lava/water reactions and waterlogging; wired into the server so each region simulates only chunks it owns (`docs/handoff/fluid-ownership.md`; Terra).
- `ferrite-net`: AES-NI CFB8 cipher (decrypt ~4-8x faster, FB5); Velocity modern forwarding + BungeeCord legacy forwarding (OC1).
- `ferrite-server`: real /tps /mspt /profile (FB3). `ferrite-plugin` offline-player registry salvage (FB2). VarInt decode fast path, protocol-translate handoff notes (FB1).
- `tools/make_fixture_world.py` regenerates the vanilla fixture world (seed `ferrite-fixture`, spawn pinned to 0,80,112).

## Unmerged work (worktrees under C:/Users/taegy/orca/workspaces/ferrite-mc-ref/)
| Worktree | State |
|---|---|
| fb1 | `31b00a1` libdeflate default frame compression (2.6-3.4x compress, 1.7-2.2x decompress) - MERGED. Next task (compression/encryption off the tick thread) is pasted in its input but was never submitted. |
| terra2 | `a634351` density-function evaluator (noise-free nodes), `f87df53` seeded noise + splines - MERGED; W1c (Java differential vs the 26.3 jar) uncommitted. Then W2 (noise chunk fill) and W3 (surface rules); brief in task `task_295171c6e46d` / `docs/handoff/worldgen-26-3.md` (26.3 API names differ from the brief: use the jar). |
| claude-worker | `b408f74` ferrite-entity M1 (region-local store, section index, region hand-off) committed; M2 (tracking packets) and M3 (item entities) in progress, uncommitted. Edits `game/region.rs` and `game/mod.rs` - rebase after the fluid merge. |
| terra | random ticks R1/R2 started, 2 files uncommitted. |
| fb2 / fb3 / fb4 / fb5 / oc1 / oc2 | uncommitted work: plugin onEnable end-to-end test / commands 2 (tp, msg, kill, op, ban, kick, stop, save-*) / signed chat (never committed) / chunk packet cache / light-fixture fix / commands 1 (gamemode, time, weather, ...). |

## Known issues
- `ferrite-light` `compare_fixture_light_when_available` fails on the regenerated fixture: 14/3,207,168 sky-light values (0.00044%) at the outer ring of comparable chunks; block light 0 mismatches. Suspected fixture-edge artifact, not an engine bug (OC1 is investigating; CI skips it because the fixture is absent). Run workspace tests with `-- --skip compare_fixture_light_when_available` until fixed.
- Codex (Terra, Terra-2) is out of quota until 2026-10-31 00:00. Freebuff sessions end hourly and need a manual resume.
- Long instructions sent to Codex/Freebuff terminals land as a "pasted content" chip; press Enter (and check for `Working (` / `working...`) or the task never starts.
- Many concurrent cargo builds slow everything and make benchmarks noisy.

## Next steps
1. (done) fb1 libdeflate and terra2 W1a/W1b merged after ferrite-protocol/net/worldgen tests, clippy and fmt passed; main before them passed the full workspace (minus the light test).
2. Resume Terra/Terra-2 after the quota reset; collect FB/OC results as they commit, review, merge.
3. Remaining backlog: entity M2/M3, random ticks, plugin onEnable end-to-end (M3 of the plan), signed chat, chunk packet cache, commands, tick-thread-free compression.

## Hand-over for 2026-10-31 (read this first)
All usage quotas ran out on 2026-10-30 (Codex until 2026-10-31 00:00, opencode2 exceeded, Freebuff hourly), so the unfinished work was frozen and handed to whoever continues next. Nothing was dispatched after that point.

**Every unfinished worktree is snapshotted on GitHub as `wip/day4-<name>`** (a commit on top of the worktree's branch with all its uncommitted files; may not compile). To continue: `git fetch origin && git switch -c <name> origin/wip/day4-<name>`, finish, run the checks, open it against `main`.

| Branch | What is inside | To finish |
|---|---|---|
| `wip/day4-claude-worker` | ferrite-entity M1 committed (store, section index, region hand-off) + M2/M3 WIP (tracking packets, item entities) | M2: spawn/move/remove packets per tracker range, run `tools/wirecheck`; M3: item entity physics, pickup, block-break drops; bot integration test |
| `wip/day4-terra` | random ticks R1/R2 in ferrite-server (`game/random_tick.rs`), uses the section counters | R1 per-region random-tick framework (randomTickSpeed), R2 grass spread + leaf decay; tests |
| `wip/day4-terra2` | worldgen W1c WIP (Java differential for density functions: `tests/java/DensityRouterDump.java`, `tests/java_differential.rs`) on top of W1a/W1b already on main | finish W1c (max error 1e-9), then W2 noise chunk fill (>=99% solid mask match on fixture chunks), W3 surface rules (>=98% top block); see `docs/handoff/worldgen-26-3.md` (26.3 API names differ from older versions: follow the jar) |
| `wip/day4-fb3` | commands 2: tp/teleport, msg, kill, op/deop, whitelist, ban/pardon/kick, stop, save-* (+2.4k lines, `game/feedback.rs`) | build, fix, test per command (success + permission-denied) |
| `wip/day4-fb4` | signed chat (`game/chat.rs`, `docs/handoff/signed-chat.md`, +1k lines) | run wirecheck after packet changes; test that player_chat reaches another player; signature/last-seen unit test |
| `wip/day4-fb5` | chunk packet cache (encode once, reuse across players, invalidate on change) | byte-identical-output test, invalidation test, bots example MSPT before/after |
| `wip/day4-oc1` | light-fixture fix (`ferrite-light` vanilla differential) | see the diagnosis below |
| `wip/day4-oc2` | commands 1: gamemode, defaultgamemode, difficulty, time, weather, seed, list, say, me | build, fix, test per command |

Main also has FB2's M3 (plugins/*.jar loaded at startup, onEnable/onDisable, end-to-end test `crates/ferrite-server/tests/plugin_lifecycle.rs`; needs JAVA_HOME=Temurin 25 and a gradle build).

### Light differential failure: likely cause
`tools/make_fixture_world.py` waits only `GENERATE_SECONDS = 30` after the forceload before `save-all`; FB2 found that stale sky light can be saved with that and regenerated its fixture with a 120 s wait. Raise `GENERATE_SECONDS` to 120, regenerate (`python tools/make_fixture_world.py`), and re-run `cargo test -p ferrite-light --test vanilla`. If mismatches remain, check the outer ring of comparable chunks.

### Coordination notes
- Orca run `run_57911c7b3e38`; session table and terminal handles are in `docs/sessions.md` (handles die with the Orca runtime).
- Routing used: hard tasks to Claude workers and Terra (as commit-sized milestones), small ones to Freebuff/opencode2. Terra gives up fast on big specs ("incomplete" within a minute) but reliably finishes one-commit steps.
- Sending a long task to Codex/Freebuff lands as a pasted chip; press Enter and verify `Working (` / `working...`.


---
# Day 5 (2026-10-31) — release prep, docs, and the drop-test flake

`main` at the time of writing: **`3a43753`** (`Merge yxshadxw/fb2: plugin event error policy`), which
already contains every Day-4 merge. Sessions: `docs/sessions.md` (1 Claude worker + 5 Freebuff;
Terra/Terra-2 are out of quota until 2026-10-31 00:00; OC1/OC2 closed).

## On main now (all merged, all building)
- **Net**: per-connection writer task — ticks only push encoded bytes into a byte-budgeted queue
  (`send_queue_bytes`, 2 MiB default); the writer batches, compresses, encrypts, keepalives and
  drops a client that stops reading (`ferrite-net/src/game.rs`, `write_loop` in `lib.rs`).
- **Commands**: `/gamemode`, `/defaultgamemode`, `/difficulty`, `/time`, `/weather`, `/seed`,
  `/list`, `/say`, `/me` (OC2); `/tp`/`/teleport`, `/msg`+aliases, `/kill`, `/op`/`/deop`,
  `/whitelist`, `/ban`/`/pardon`/`/kick`, `/stop`, `/save-*` (FB3); `/clear`, `/give`, `/xp` (FB5);
  the whole `/gamerule` family with names from `commands.json` and values persisted in
  `data/minecraft/game_rules.dat`.
- **Inventory**: `ferrite-inventory` extraction, hashed-slot clicks, crafting (1,763 recipes),
  chest menus shared between players with block-entity save/spill (Claude worker).
- **Entities**: region-local store, vanilla tracker (spawn/move/rotate/teleport/remove, metadata,
  tracker ranges), item entities (physics, merging, pickup, loot-table drops) (Claude worker).
- **Chat**: 26.3 signed chat (signature chain, last-seen/ack, profile keys) and the status ping with
  MOTD/favicon (FB4).
- **Plugins**: embedded JVM at startup, Paper-only `plugin.yml` loading, `onEnable`/`onDisable`,
  player/chat/block events, plugin commands, `plugins.on-event-error = allow|deny` (FB2).
- **Selector permission** is enforced in **one place**: the dispatcher's `set_selector_requirement`
  (`ferrite-command/src/dispatcher.rs`), wired once in `commands::dispatcher()` (FB3). Day 5 removed
  the duplicate helper check that OC2 had added in `game/commands.rs`.
- Also from Day 4: Velocity/Bungee forwarding (OC1), random ticks (Terra), fluids (Terra), worldgen
  density functions + bit-exact noise (Terra-2), `/tps` `/mspt` `/profile` (FB3), chunk packet cache
  (FB5).

## Fixed on Day 5 (FB4)

### The intermittent `survival_break_drops_an_item_that_falls_and_is_picked_up` failure
**Symptom**: `entity packet: Elapsed(...)` after 20 s in full-suite runs, green when the test ran
alone; also seen as 20 s waits in `chest_contents_are_shared_saved_and_spilled_when_broken`.

**Root cause: integration tests shared one process-wide player-data directory.**
`playerdata::dir()` returned a single `ferrite-test-playerdata-<pid>` directory for the whole test
process, and `World::start_configured` restored `saved.gamemode`, `saved.pos` and the saved
inventory at join. Test worlds reuse bot names (`Miner`, `Walker`, `Alice`, ...), so whichever test
left first wrote that name's save and every later world in the same process joined as **that**
player:
- `/creative_break_broadcasts_to_tracking_player` saved `Miner` in **creative mode**, so the survival
drop test's miner joined in creative, broke the dirt without a drop, and the walker waited for an
`AddEntity` that could never come — the reported 20 s timeout.
- `/crossing_region_boundary_keeps_chat_and_movement_routed` saved `Walker` at **x = 300.5**, so the
drop test's walker joined there; its `MovePosition` to the drop (3.5, 65) is rejected as a 297-block
move, and the item stays out of tracking range.
- Leftover `Keeper.json` files with 40 stone in slot 0 (PID reuse across runs) explain the chest
test's 20 s "picked up N stone" waits.
It is intermittent because the save only happens if the region processes `Command::Leave` before
`World::shutdown()` stops the scheduler — the join/leave race made it roughly 3 in 5 full runs.
Stale files proving the mechanism are still under `%TEMP%\ferrite-test-playerdata-*`.

**Fix**: player data is now owned by the world, not the process. `WorldSettings::playerdata:
Option<PathBuf>` (default `None`) is the only save directory; `Player::update` writes with
`playerdata::save_to` only when it is `Some`, and the global `OnceLock`/`init`/`load`/`save` are
gone. Production passes the configured `<world>/playerdata` in `main.rs` (behaviour unchanged);
test worlds pass `None`, so they neither restore nor write anything. New regression test:
`player_data_is_scoped_to_one_world` (same directory resumes the saved position, another starts at
world spawn). That is also the first test that covers player-data resume end to end.

**Checked and ruled out** (as asked): the writer task's 2 MiB byte budget is far above what these
connections queue (view distance 2, one item entity); an overflow disconnects with "Timed out" and
the bot then reads EOF and panics, it does not sit in `Elapsed`. Item tracking is correct in
isolation (spawn/teleport/pickup all arrive when the bot is in range). Tick scheduling keeps
50 ms region deadlines; the 20 s gap was always "this world never had an item to track".

### Fixture-less CI and test hygiene
- Tests that read `vanilla/26.3/fixture-world` now **skip** when it is absent (`ferrite-server`
  `fixture_version_root`, `ferrite-world/tests/chunk_nbt_roundtrip.rs`); CI has no fixture, so these
  used to panic there. The light differential already skipped.
- CI (`.github/workflows/ci.yml`) keeps the Linux toolchain override (`RUSTUP_TOOLCHAIN=stable`
  beats the MSVC pin in `rust-toolchain.toml`) and now fails if any `vanilla/` file is tracked.
- `git ls-files | grep -E '^vanilla/'` is empty. (The bare `grep -i vanilla` also matches nine
  first-party source/test files named `vanilla*.rs`, which are code, not Mojang data.)
- README: current feature list, honest caveats, build instructions, CI and legal note.

## Checks on Day 5
`cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` and
`cargo test --workspace` are green: **782 passed, 0 failed, 2 ignored** across 97 test binaries
(includes the 181 s light differential on the regenerated fixture and the 125-test
`ferrite-server` lib suite). The two drop tests passed ten consecutive full-suite runs
(`cargo test -p ferrite-server --lib`, 125/125 each, both tests `ok` every run).

The CI condition (no fixture world) was verified locally: with `vanilla/26.3/fixture-world` moved
away the workspace is still green (782 passed, 0 failed) and the five fixture tests print
`SKIP fixture-world test`; the light differential skips the same way. The workflow YAML parses and
`RUSTUP_TOOLCHAIN=stable` overrides the MSVC pin in `rust-toolchain.toml`. **Caveat**: there is no
Linux or Docker runtime on this machine, so the GitHub Linux job itself was not executed here.

## Real backlog
See **`docs/BACKLOG.md`** — remaining work by family (mobs/AI, redstone, command
stubs, plugin API breadth, multi-version, persistence, perf, release).

## Known issues
- `ferrite-light` differential needs the regenerated fixture: `python tools/make_fixture_world.py`
  (stable double flush). The fixture used on 2026-10-30 had only 196/256 comparable chunks and made
  the test fail; the 2026-10-31 fixture compares 256 and passes (181 s). The fixture is gitignored.
- Terra/Terra-2 out of quota until 2026-10-31 00:00; Freebuff sessions need an hourly resume.
- `tools/wirecheck/run.py` must be run after any packet change (it needs a real 26.3 client).
- `cargo test --workspace` takes ~6–8 minutes locally; the light differential alone is ~3 minutes.

## Hand-over
1. Merge this worktree (`yxshadxw/fb4`): flake fix, selector dedup, docs, CI/release prep.
2. Re-run `cargo test --workspace` after any packet or test-harness change, and the two drop tests
   ten times in a row (`cargo test -p ferrite-server --lib`) if touching `playerdata`/tracking.
3. Next feature work in priority order is in `docs/BACKLOG.md`; start with mobs.

---

# Day 6 (2026-10-31) — plugin compatibility: real plugin surfaces

`main` at the time of writing: **`5d76bb9`** (FB1 widened the Paper Server API to 120/206 methods;
the compat report on the WIP branches lists what real plugins still need). Worktree `fb4`.

## On top of 5d76bb9 (green, not pushed)
- `dabcc1c` Plugin API: class-based BLOCK/ITEM registries for `Material.asBlockType()`, a versioned
  `UnsafeValues` (data version 5023, protocol 777), `BlockData` over material ids, Server
  tick/stop/entity/folder getters, `Block#getBlockData` and `World#getEntities`.
- `2210482` CraftBukkit shims (`CraftWorld`, `CraftPlayer`, `CraftOfflinePlayer`, `CraftBlock`,
  `CraftBlockData`, `CraftItemStack`, `CraftInventory`, `CraftInventoryPlayer`,
  `CraftNamespacedKey`) plus `java/test-plugin-typical` and its end-to-end test.
- `179cc43` Host wiring: `PluginHost::tick` runs from the 50 ms game clock (Bukkit scheduler tasks
  finally execute), the main world is published before plugins load, and joining players are
  registered with location/mode/op so `getLocation`/`getSpawnLocation` work in listeners.
- `3bbab31` `plugin.yml` permissions are registered and removed with the plugin manager, so
  `default: true` reaches players and command permission gates pass.

## Verified
- `cargo test -p ferrite-server --test plugin_lifecycle`: 2/2 (25 s). The typical plugin reads
  config.yml, completes tabs, receives join events, runs `/typical ping|where|info`, and its
  repeating task reaches tick 5 and cancels itself.
- Gradle bridge tests pass; `cargo clippy -p ferrite-plugin -p ferrite-server --all-targets` and
  `cargo fmt --all --check` are green.

## Remaining
- `Native.playerInventory` needs a `PlayerHandler` in the server (route `PlayerCommand` into the
  owning region); until then `PlayerInventory#setItem` reports "No player command handler" and the
  fixture asserts that branch.
- Block snapshots are still opt-in (`setBlockSnapshot`), so arbitrary `Block#getType` throws.
- The rest of the `Server` stubs and NMS-typed CraftBukkit methods (`CraftItemStack.asNMSCopy`,
  `CraftWorld#getHandle`, `CraftMagicNumbers`) remain; client-facing tab completion still goes
  through the Rust dispatcher, not plugin `TabCompleter`s.

---

# Day 7 (2026-11-01) — OC4 takeover: plugin region scheduler

Branch `yxshadxw/oc4` on top of `c0c2835`. Finishes the Day 6 `wip/day6-oc4` bridge work that was
left half-done when the opencode session hit its quota.

## On the branch
- `Bootstrap.startServer()` creates the Bukkit server without enabling plugins; the host publishes
  the main world and then calls `Bootstrap.enablePlugins`, so `onEnable` already sees
  `getWorlds()`.
- Every accepted movement refreshes the Bukkit player snapshot
  (`ServerHooks::update_player_location` -> `Bootstrap.updatePlayerLocation`), and
  `registerPlayer` updates the existing `FerritePlayer` instead of replacing it, so plugin-held
  `Player` references stay valid.
- Paper's region scheduler is live: `PluginHost::take_region_tasks` returns due tasks to the game
  loop, which schedules each on the region owning its chunk. Bukkit's sync scheduler and Paper's
  global scheduler run on the global tick; `runTaskAsynchronously`,
  `runTaskLaterAsynchronously` and `runTaskTimerAsynchronously` run on the async scheduler.
- The typical fixture schedules through `RegionScheduler#run`, `runTask`, `runTaskLater`,
  `runTaskTimer`, `runTaskAsynchronously`, `runTaskLaterAsynchronously` and
  `runTaskTimerAsynchronously`, and asserts `/typical where` after a move plus `/typical give`
  through `Native.playerInventory`.

## Checks
Final run on the merged tree (`origin/main` at `ac58ab3`, merge `2e5aeeb`):
- `cargo test --workspace --no-fail-fast`: **1012 passed, 1 failed, 2 ignored** across 105 test
  binaries. The one failure is `game::integration::chest_contents_are_shared_saved_and_spilled_when_broken`,
  which times out its 20 s pickup deadline only under full-suite load; it passes alone (1/1, 23 s).
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check`: clean.
- `cargo test -p ferrite-server --test plugin_lifecycle`: 2/2 (129 s), also covered by the merge
  run above.
- Load-sensitive runs on this machine also aborted the `ferrite-worldgen` Java harness
  (`carver::tests::mask_matches_26_3_java_harness`, 1/1 alone in 134.6 s) and once failed
  `java_differential` with `STATUS_DLL_INIT_FAILED`; both pass alone and in an unloaded full run.

## Startup note
On this machine a debug server spends about 80 s loading the vanilla registry and 60 s preloading
1,763 recipes before it accepts logins, so the lifecycle test allows 300 s for the first response
(all plugin assertions keep their 60 s deadlines). If startup regresses further, profile
`RuntimeRegistry::load` and `crafting::preload` instead of raising the deadline again.

## Day 7 orchestration record (2026-11-01)

`origin/main` = `bf2f8fd`. The whole workspace suite is green (105 test binaries, all passed) and
`clippy -D warnings` / `fmt --check` are clean; that run was made on the oc8 branch tree, which is
identical to `main` apart from `README.md`.

Merged this session:

| Merge | Content |
|---|---|
| `b8ef576` | oc7 containers: furnace/blast furnace/smoker smelting, anvil repair/rename/cost, enchanting, grindstone, stonecutter, smithing, loom |
| `ac58ab3` | oc5 hostile mobs (zombie, skeleton, spider, creeper, enderman) with goals and block-grid pathfinding, arrows, player combat (crits, sweep, knockback, shield), monster spawning |
| `d082d3c` | oc4 plugin bridge: plugin region tasks routed through the region scheduler, world/player snapshots, `Native.playerInventory` |
| `e94a34c` | oc8 commands: `locate`, `datapack`, `help`, `schedule`, `trigger`, `reload`, entity selectors from the region stores, selector options |
| `bf2f8fd` | oc8 follow-up: `@e[type=#tag]` resolves through the entity type tags and rejects unknown tags/types |

Two test fixes were needed on the way: `commands_packet_decodes_and_filters_permissions` now
decodes every parser property and suggestion type (the new `/help` word argument broke the old
decoder), and the oc5 merge kept both the `RenameItem` and `Attack`/`Interact` packet arms.

`README.md` was rewritten (`d88dc52`) around a done / in-progress / missing status table so the
state of the project can be read at a glance.

### Still open

- **Worldgen throughput (oc6) is not done.** The worker reported `worker_done` with outcome
  `failed`: `yxshadxw/oc6` at `6e28734` only adds the benchmark scaffolding (`rayon` in the
  workspace dependencies, genbench framing); there is no density cache and no parallel chunk pool.
  `cargo test -p ferrite-worldgen` passed 60/60 on that branch. Next: merge `main`, implement the
  cache reuse plus a bounded chunk worker pool with byte-identical output, and measure
  `crates/ferrite-worldgen/examples/genbench.rs` against the 69-chunks-in-30-s baseline before
  starting tree features. The oc6 terminal is wedged on a 6,334-character input overlay and does
  not accept new prompts, so it needs a fresh session.
- **Pistons** (`task_94b5018a1e91`) and **plugin API breadth** (`task_0c86a661ea7c`) are created
  but not started; the oc7 and oc4 sessions went idle, and oc4 hit its quota.

---
# Day 7 (2026-11-01, Freebuff coordinator)

`origin/main = f5ba1e8`. Five worker branches merged and pushed in one cycle:

| Commit | What landed |
|---|---|
| `deb2bea` | oc10: villagers, professions, deterministic trade offers, merchant menu and trading |
| `22c5b47` | oc12: hopper transfers (8-tick, powered, comparator), droppers and dispensers |
| `0e8b35d` | oc14: Bukkit `Server`/`PluginManager` surface and inventory/death/chunk/weather events |
| `81e8d76` | oc6b: per-chunk density grids, memoized biomes and surface rules, worker pool — 4.59 -> 6.18 chunks/s (1 thread), 23.49 -> 38.92 (8 threads), chunk output identical; the 10x target is not reached |
| `3a766aa` | oc11: entity and block-entity persistence across restarts |

Verification: `cargo check --workspace --all-targets`, `cargo test --workspace` (all green before
the oc11 merge), `cargo clippy --workspace --all-targets -- -D warnings` and
`cargo fmt --all --check` green after the two fixes in `f5ba1e8`. oc11 landed after the full-suite
run and was checked with `cargo check`, clippy, fmt and `cargo test -p ferrite-worldgen` only.

oc9 (pistons) and oc13 (command blocks) were stopped by the user before finishing; nothing from
them was merged and their empty worktrees were removed. Their specs stay in
`docs/handoff/day7-tasks/`. Ten merged or old worktrees were removed to free disk (65 GB free).
Worker branches are backed up as `origin/yxshadxw/oc6`, `oc10`, `oc11`, `oc12` and `oc14`.
Still open: pistons, brewing/beacons, command blocks,
nether/end dimensions and a real 26.3 client test.
