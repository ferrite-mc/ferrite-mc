# Coordinator plan and queue

Single source of truth for lane order. Worker rules live in
[worker-contract.md](worker-contract.md).

## Operating rules

- **Three concurrent lanes.** The host has 12 cores; each lane builds with `CARGO_BUILD_JOBS=4`
  and its own `CARGO_TARGET_DIR`. A fourth lane starves the others.
- **Reuse the running session.** New work is dispatched to an existing, settled worker terminal
  with `orca orchestration dispatch --task <task_id> --to <handle> --inject`. Do not call
  `worker-start` for a lane that already has a terminal on this host.
- **One merge gate per lane.** A lane merges into `main` only after its focused test, the full
  workspace test, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo fmt --all --
  --check` all pass, and the coordinator has read the report.
- **Two failures, then re-scope.** After two failed attempts on a lane, split it or do it
  directly; do not retry blind a third time.
- **No pushes** unless the coordinator asks. No `cargo clean` outside the lane's own target.

## Running now

`main` is `b59dc3c`, pushed to `origin/main`. Every lane below starts from that commit, in a
worktree under `~/orca/workspaces/ferrite-mc/`, with its own `CARGO_TARGET_DIR` and
`CARGO_BUILD_JOBS=4`.

| lane | scope | branch / worktree | terminal | model |
|---|---|---|---|---|
| P5 gap 7 end-to-end | headless plugin jar plus a scripted client: create from `onEnable`, lookup, block read/write, player travel, unload, reopen, chunk and entity persistence | `p5-gap7` | `term_34881043` | `sonnet` |
| L1 light fixture deadlock | `compare_fixture_light_when_available` futex hang: reproduce, capture stacks, root-cause, fix or bound the wait | `light-deadlock` | `term_a2ca58b2` | default |
| L2 playerless-region damage events | mob damage in a region with no players must fire the plugin damage event and be cancellable | `mob-damage-events` | `term_3e95abd3` | default |

### Landed

- **P2** `f670dbf` — heightmap variants, block/light/biome reads that load unloaded runtime chunks, chest/furnace/sign block entities; merged `2c9684d`.
- **P3** `c359496` — `World#ferriteEntities` and `ferriteSpawnSnapshot`, `EntityTransformer`, `CapturedStructure#getEntities`/`getEntityCount`, `StructureEntity`; merged `b59dc3c`.
- **P4** `29a5a01` — `DamageKind::Explosion`, armour bypass only for explosions, cancellable plugin damage event; merged `2c9684d`.
- **Merge gate** — merged tree `b59dc3c`: `fmt --check` 0, `clippy --workspace --all-targets -D warnings` 0, workspace tests 0 across 92 suites.
- **G1** — `main` `9fd4f8d` → `301524f`: plugin command-tree fix, transactional unload, seed/`level.dat`, P1 host coverage. Workspace suite `exit 0`, 1606 passed in 92 suites, one test skipped (`ferrite_light::compare_fixture_light_when_available`, see Blocked).
- **G3** — player travel merged as `293179f`, test seed fix `301524f`. `player_travels_into_a_runtime_world_and_back_keeping_holds_and_entities` passes.
- **RW1/RW3/A/B** — all merged by `9fd4f8d`; their terminals are idle and are being reused for P2/P3/P4.

## Gates

- **G1** — `main` contains the plugin command-tree fix and the transactional unload work;
  `plugin_lifecycle` 4/4, `runtime_persist` 2/2, workspace test and clippy exit 0.
- **G2** — the world contract branch passes its own tests and clippy, and merges onto the G1 main.
- **G3** — player travel merges onto the G2 main without touching plugin tests.

G1, G3 and P1 landed (`0a9d405`, `0df0cc4`, `c139c80`, `9fd4f8d`), so P5 is clear and running.
P6 needs P5. L1 and L2 need nothing and are running. Push is unblocked: `origin/main` is current.

## Ordered queue

| # | lane | scope | starts when | terminal |
|---|---|---|---|---|
| P1 | plugin host coverage, tier 1 | the highest-use `HostBridge` methods with no Rust answer (ban/kick/chat/broadcast/breakBlock/canBreatheUnderwater/clearRecipes family), one family per module, each with a host-answer test | first freed slot | that slot |
| P2 | world data queries | heightmap variants for `getHighestBlockYAt`, block/light/biome reads on unloaded chunks, block-entity placement beyond command blocks | G1 | **running**, `term_a2ca58b2` |
| P3 | entity and structure snapshots | host op returning non-player entity type/pos/NBT for a chunk range, then `EntityFactory#createEntitySnapshot`, `Structure#getEntities`, `EntityTransformer` | G3 | **running**, `term_9ff86e3e` |
| P4 | gameplay gaps | `DamageType::Explosion` wired into explosion damage, plus the block-entity placement left from P2 if not taken there | G1 | **running**, `term_3e95abd3` |
| P5 | gap 7 end-to-end | real plugin/client fixture: create from `onEnable`, lookup, block read/write, player travel, unload, reopen, chunk and entity persistence | G1, G2, G3, P1, P3 | freed slot |
| P6 | docs and release | `docs/BACKLOG.md` section 8 rewritten to what actually shipped, `docs/release-notes-v0.1.6.md` known limitations, `docs/plugin-api-stubs.md` refreshed | P5 | coordinator |

## Blocked

- **G1 skip** — `crates/ferrite-light/tests/vanilla.rs:155 compare_fixture_light_when_available`
  hangs in a futex wait with no child process or socket; the whole-suite run needs
  `--skip compare_fixture_light_when_available` until it is fixed. Not yet reproduced on the
  pre-change baseline. Lane L1 owns the fix; until it lands, every gate run keeps the skip and
  reports it.

- **D1 github-ops** — resolved. `gh auth login --web` plus `gh auth setup-git` on the remote; `origin/main` tracks local `main` and is pushed through `b59dc3c`.
- **Fable quota** — the first P5 attempt ran on `--model fable` and hit the provider usage limit (`limit resets 11:20am`) after a read-only survey, with no code written. That session was closed and P5 restarted on `sonnet`, seeded with its findings: `Block#setType` dropped on runtime worlds, `spawnEntity` refused there, and `/summon` inside a runtime world landing in the overworld.
- **DeepSeek lanes** — `opencode` reports `agent_unconfigured` and `opencode2` rejects `--model`
  on launch. Claude lanes only until that launcher is configured.

## Notes

- RW1 and RW3 completed on 2026-11-11 (plugin lifecycle fix; transactional unload plus per-world
  shutdown persistence). Their work is uncommitted and is committed by lane A.
- Gap list and evidence live in `docs/handoff-runtime-worlds-2026-11-11.md`; the plugin inventory
  is in `docs/plugin-api-stubs.md` and `docs/plugin-stubs-status.md`. Both are staler than the
  source; trust the source when they disagree.

## Landed 2026-11-11 (wave 2) — main `7a0ae81`, pushed

| Lane | Commit | Evidence |
|---|---|---|
| L1 light fixture | `f0779bf` | `[profile.dev.package.ferrite-light] opt-level = 2`; first debug run after the override: 32.03s (release 24.6s). Full workspace suite now runs **without** `--skip`, 92 suites ok |
| L2 playerless mob damage | `3d31805` | `SharedPluginHooks` on `TickContext`, published at startup and on `SetPluginHooks`; `game::` 620 passed, clippy 0, fmt 0 |
| P5 gap 7 end-to-end | `493e585` | `runtime_world_gap7_create_edit_enter_leave_unload_reopen` ok in 6.69s; full suite 92 ok; clippy 0, fmt 0 |

- fable hit the provider usage limit mid-survey; P5 was rerun on `sonnet` and landed. L1 and L2 workers also stopped at their limits; the coordinator closed both out.
- L2's failing test was a test bug: the region never loaded chunk (0,0). Fixed by awaiting `source.get(0,0,0)` as the older blast test does.
- P5 left open and documented in its report: `/summon` inside a runtime world still lands in the overworld; `createWorld`/`unloadWorld` called from a plugin command deadlock the game loop; `Player#getWorld` still reports the default world; `isChunkLoaded`/`isChunkGenerated`/`loadChunk` unimplemented on runtime worlds; entities reappear only after a player re-enters; block-state properties are dropped on the `setType` path.

## Dispatched 2026-11-11 (wave 3) — runtime-world gaps, all on sonnet

Base: `main` `7a0ae81`. Worktrees under `/home/ubuntu/orca/workspaces/ferrite-mc/<lane>`, each with `vanilla` symlinked.

| Lane | Branch | Terminal | Target |
|---|---|---|---|
| R1 | `rw-player-world` | `term_a452282d` | `/summon` and `Player#getWorld` resolve the player's runtime world |
| R2 | `rw-chunk-api` | `term_dffde10b` | `isChunkLoaded` / `isChunkGenerated` / `loadChunk` for runtime worlds |
| R3 | `rw-plugin-cmd-thread` | `term_ec631ec3` | `createWorld` / `unloadWorld` from a plugin command no longer deadlock the loop |

All three confirmed `input_accepted -> turn_started`. Each owner commits on its own branch, no push, no merge.

**Next session:** collect the three reports, run `fmt --check`, `clippy --workspace --all-targets -- -D warnings` and the relevant tests per branch, merge into `main`, push, and delete the worktrees. Still open after that: block-state properties dropped on the `setType` path, and entities only reappearing after a player re-enters a reopened world.

## Landed 2026-11-11 (wave 3) — main `265c336`, pushed

| Lane | Commit | Evidence |
|---|---|---|
| R1 `rw-player-world` | `ecd58c3` | `/summon` resolves through the player's `WorldId` (`summon_for_player`), plugin location uses `plugin_world_id(player_world(..))`. `cargo test -p ferrite-server --lib` 759 passed; new tests `summon_from_a_runtime_world_player_lands_in_that_world`, `plugin_world_id_follows_the_players_world` |
| R3 `rw-plugin-cmd-thread` | `9322e48` | `Action::Plugin` arms dispatch through `spawn_blocking`; new `tests/plugin_command_world.rs` fails on old code after 97.6s, passes in 4.9s fixed. `plugin_lifecycle` 5/5, lib 757 passed |

Merge gate on the merged tree (`265c336`): `fmt --check` 0, `clippy --workspace --all-targets -D warnings` 0,
`cargo test --workspace` 0, **93 suites ok**, `plugin_command_world` 1 passed (8.65s),
`plugin_lifecycle` 5 passed (15.76s), `runtime_world_gap7_...` ok. `origin/main` == local `main`; `rw-player-world`,
`rw-chunk-api` and `rw-plugin-cmd-thread` are pushed as remote branches.

### Still open after wave 3

- **R2 `rw-chunk-api`** landed as `0f8ca6c` (merge `abdb2b6`): defaulted `WorldView` methods, `chunk_resident` /
  `chunk_saved` on `AnvilChunkSource`, and one test per method; `ferrite-server` lib 760 passed.
- `Block#setType` drops block-state properties (runtime world path).
- Entities reappear only after a player re-enters a reopened world.
- Join-time `register_player` still reports the default world; a player rejoining inside a runtime world would be reported wrong.
- The plugin still sees the old world between a world/dimension change and the next movement packet (no push from `Signal::ChangeDimension`).
- `SummonCrystal` still routes by dimension.
- Plugin commands are no longer serialized on the loop; overlap safety on the blocking pool is unchecked.
- The gap 7 Java steps in `plugin_lifecycle.rs` still use the async scheduler.
- Dependabot moderate 1 (`/security/dependabot/10`).
- Retired lane worktrees (`p5-gap7`, `light-deadlock`, `mob-damage-events`, `rw-player-world`, `rw-plugin-cmd-thread`) and `$HOME/ferrite/target-*` caches still on disk.

## Dispatched 2026-11-11 (wave 4) — remaining code gaps, all on sonnet

Base: `main` `abdb2b6`. Three concurrent lanes, `CARGO_BUILD_JOBS=4`, one target directory each.
Briefs live on the host at `~/ferrite/briefs/<lane>.md`.

| Lane | Branch / worktree | Terminal | Target | Scope |
|---|---|---|---|---|
| W1 | `rw-world-push` / `rw-player-world` | `term_a452282d` | `target-rw1` | join-time plugin snapshot uses the live world; `Signal::ChangeDimension` pushes the new location at once; player `/summoncrystal` routes by the player's `WorldId` |
| W2 | `rw-world-content` / `rw-chunk-api` | `term_dffde10b` | `target-rw2` | `BlockData` properties on the runtime `setType` path; entities visible after unload/reopen without a re-entry; Java `World#loadChunk` wired to the host method; `chunk_saved` disk test and `loaded` hold |
| W3 | `rw-bridge-safety` / `rw-plugin-cmd-thread` | `term_ec631ec3` | `target-rw3` | overlapping plugin commands on the blocking pool (prove safe or serialize); gap-7 Java steps on the sync scheduler |

All three confirmed `input_accepted -> turn_started`. Each owner commits on its branch; no push,
no merge. Not assigned to a lane (coordinator-side): Dependabot moderate 1 and the retired
worktree/target-cache cleanup.

## Dispatched 2026-11-11 (wave 5) — remaining work in three lanes, all on sonnet

Base: `main` `6aca3fc` (`origin/main` current). The W2 `rw-world-content` work from wave 4 is
still uncommitted on `rw-chunk-api`; lane L1 lands it first. Briefs live on the host at
`~/ferrite/briefs/w5-*.md`.

| Lane | Branch / worktree | Terminal | Target | Scope |
|---|---|---|---|---|
| L1 | `rw-world-content` -> `w5-world-plugin` / `rw-chunk-api` | `term_dffde10b` | `target-rw2` | land W2; cherry-pick W1 `70b4ba4`; persist and restore the runtime `WorldId` on rejoin; `Player#breakBlock`, `Player#chat`, 1-arg `broadcastSlotBreak`, hurt/pickup animations, structure/non-player snapshot where feasible; plugin docs |
| L2 | `w5-ai-gameplay` / `rw-player-world` | `term_a452282d` | `target-rw1` | entity replacement across unload/reload with overlapping viewers; cross-region targeting/despawn/knockback; vanilla behaviour slice for 5 hostiles + 4 passives; potion/xp/trident/wind-charge/fishing projectiles; regression tests |
| L3 | `w5-perf-docs` / `rw-plugin-cmd-thread` | `term_ec631ec3` | `target-rw3` | repeatable 800-bot/MSPT/RSS baseline (or largest harness); chunk packet cache invalidation on block-entity change; consolidate BACKLOG/release-notes/plugin docs/README/COMMANDS-STATUS; piston restart and gateway `Age` regression tests; headless join smoke + manual 26.3 checklist |

All three confirmed `input_accepted -> turn_started`. Workers commit and push their own
branches; no merge into `main`. Coordinator still owns the merge gate, CI, Orca visibility,
the real 26.3 client join, Dependabot #10, and retired-worktree/cache cleanup.

Not assigned to a lane: the real-client 23:38 server join (needs a 26.3 client and is a manual
verification, not a remote code task); math and English practice (outside this repository, tracked
separately).
