# Claude lanes — remaining work split (2026-11-14)

Prepared from `main` = `d71b286`, after `lane/{commands,entities,world,plugin}-g` were merged.
[`BACKLOG.md`](BACKLOG.md) stays the single list of open work; this file only assigns ownership,
order and acceptance for two Claude lanes. It adds no new work.

## Lanes

| | Lane A — world and entity | Lane B — commands, protocol and plugin API |
|---|---|---|
| Branch | `lane/claude-world-2026-11-14` | `lane/claude-api-2026-11-14` |
| Worktree | `/home/ubuntu/orca/workspaces/ferrite-mc/claude-world` | `/home/ubuntu/orca/workspaces/ferrite-mc/claude-api` |
| BACKLOG source | §2, §4 | §3, §5 (command/protocol items only) |
| Primary files | `ferrite-entity/**`, `ferrite-fluid/**`, `game/{ai,dragon,end_fight,mob,path,random_tick,fluid,gravity,neighbors,region}.rs` | `ferrite-command/**`, `ferrite-protocol/**`, `game/commands_*.rs`, `game/entity_index.rs`, `plugins/**`, `java/**` |
| Lane docs | `docs/mob-ai-parity.md`, `docs/end-dimension.md`, `docs/world-interaction-audit.md` | `docs/COMMANDS-STATUS.md`, `docs/plugin-api-stubs.md`, `docs/plugin-stubs-status.md` |
| Must not edit | `ferrite-protocol`, `ferrite-command`, `plugins/`, `java/`, `game/commands_*.rs` | `ferrite-entity`, `ferrite-fluid`, `game/{ai,dragon,end_fight,random_tick,fluid,gravity,neighbors}.rs` |

`game/region.rs` belongs to lane A. `game/mod.rs` is shared: keep edits append-only and list them
in the lane report. If lane B needs a change inside lane A's files, it stops and reports the
required interface instead of editing.

## Rules for both lanes

1. Branch from `origin/main` (`d71b286`); never commit to `main`.
2. Linux: use `cargo +stable` (the toolchain file pins Windows MSVC). Keep
   `cargo +stable fmt --all` and `cargo +stable clippy --workspace --all-targets -- -D warnings`
   clean.
3. Keep mutable world and entity state on its region thread: no direct cross-region reads or
   writes, no global lock, no synchronous cross-region wait. Cross-region paths need handoff,
   cancellation, unload and reconnect behaviour plus tests.
4. No Mojang code or assets. Read IDs and rules from `vanilla/26.3/generated/` and the local jar
   summary already recorded in the docs; do not invent values.
5. One task per commit, with its test. A bug fix carries a regression test.
6. No real-client claim. Anything only checked with unit tests, protocol bots or the JVM host is
   reported as such.
7. Do **not** edit `BACKLOG.md`, `README.md`, `README.en.md`, `docs/README.md`, or the other
   lane's docs. Put status in the owned docs and in the lane report; the coordinator updates the
   shared pages after merge.
8. Do not start work that is listed as out of scope at the bottom of this file.

## Lane A — world and entity simulation

Target: close the server-side deltas in [`mob-ai-parity.md`](mob-ai-parity.md),
[`end-dimension.md`](end-dimension.md) §Remaining gaps and
[`world-interaction-audit.md`](world-interaction-audit.md) §Still open.

### A1. Dragon leftovers (P0)

Files: `crates/ferrite-entity/src/dragon.rs`, `crates/ferrite-server/src/game/end_fight.rs`,
`game/region.rs`.

- When the perched scan gives up, vanilla charges a player within 150 blocks instead of always
  taking off; the attack turns toward the player.
- Place the breath cloud at the head aim point rather than under the perch.
- Keep the existing subphase timers and tests; add one test per transition and for the aim point.
- Do not change the gateway radius here: vanilla uses radius 5 versus Ferrite's 16, and that
  change belongs with a test that lands a player on solid ground at 1024 blocks.

Acceptance: new unit tests for the give-up charge, the attack turn and the cloud origin; existing
dragon tests still pass; `end-dimension.md` updated with what is and is not modelled.

### A2. Mob AI next slice (P0)

Files: `ferrite-entity/src/{ai,mob,path,breed}.rs`, `game/{ai,region}.rs`.

Work in this order, one mob per commit; add a row per behaviour to `mob-ai-parity.md`:

1. Sheep: wool colour and sheared state, `EatBlockGoal` (grass to dirt), shearing drops and
   persistence. This is the largest passive gap in the audit.
2. Enderman: stare-to-anger, water damage, carried-block metadata.
3. Creeper: charged explosion power and the hiss event.
4. Zombie: door breaking, daylight burning, reinforcements/village goals where the region model
   allows it.
5. Skeleton: `RestrictSunGoal`/`FleeSunGoal` and the melee fallback.
6. Spider: wall climbing, leap attack and jockey.

Acceptance: each behaviour has a same-region test and, where it crosses a region, a cross-region
test; the parity table distinguishes implemented, approximated and absent. Do not rewrite the
A* pathfinder into vanilla navigation in this lane; record its limits instead.

### A3. Chunk-load wake (P0)

Files: `game/region.rs`, `game/fluid.rs`, `game/random_tick.rs`.

`Region::wake_loaded_chunks` exists and is unit-tested but is disabled in `Region::tick`: a full
border scan reads about 60k positions through `AnvilChunkSource::block` and costs about one second
of region time at join, which broke `survival_eating_uses_food_component_after_use_duration`.
Re-enable it with palette-direct reads or a per-position blocked-fluid queue.

Acceptance: the pass runs from the tick loop; the join-time integration test above passes at its
baseline duration; a benchmark records the join cost before and after; the three existing wake
tests still pass. Vines and bamboo `leaves` are implemented only if the vanilla rule is verified
from available data; otherwise they stay documented as open.

### A4. Pairing data and vehicles (P1)

- Equipment and passengers in `ServerEntity.sendPairingData`, plus the remaining per-type metadata
  listed in `mob-ai-parity.md`.
- Steering for boats, minecarts, pigs and striders beyond the current saddled-horse path.

Acceptance: tracking-start tests assert equipment/passengers reach the client; steering tests per
vehicle kind; no claim about client rendering.

Out of lane A: tree growth (worldgen feature set), Rust worldgen, real-client checks.

## Lane B — commands, protocol and plugin API

Target: the blockers in [`COMMANDS-STATUS.md`](COMMANDS-STATUS.md) §Blockers and the explicit
limits in `README.md` §한계와 남은 것 (plugin part) and [`plugin-api-stubs.md`](plugin-api-stubs.md).

### B1. Scoreboard number formats on the wire (P0)

Files: `crates/ferrite-protocol/src/packets/play.rs`, `game/commands_meta.rs`.

`SetObjective`/`SetScore` currently write `false` for the number-format field and two tests assert
that refusal (`objective_refuses_number_format`, `score_refuses_number_format`). Encode the vanilla
26.3 format (blank, styled, fixed) and replace the refusal tests with round-trip and packet tests.
Check the field order against the local server jar before changing the codec.

Acceptance: protocol round-trip tests for each format kind, a command-level test showing
`/scoreboard ... numberformat` state reaches the encoded packet, and no refusal test left behind.

### B2. Cross-dimension clone (P0)

Files: `game/commands_world.rs`, `game/commands_exec*.rs` if the parser needs it.

`/clone ... from <dim> to <dim>` parses but runs on a single `WorldBlocks`. Resolve both source and
destination worlds through the world catalog, keep the overlap check per world pair, and fail with
vanilla's message when either dimension is unknown. Do not add a global lock; use the existing
world-loop ownership paths.

Acceptance: a test that clones between two loaded worlds (default and a runtime world), one that
rejects an unknown dimension, and a same-world regression test. If the fix requires lane A's
`region.rs`, stop and report the interface.

### B3. `data modify ... compute` providers (P0)

Files: `ferrite-command/**`, `game/commands_exec_more.rs`, `game/commands_more.rs`.

The command tree has no `context_int_provider` / `context_float_provider` parsers, so
`data modify ... compute` and the `/compute` tree remain blocked (`COMMANDS-STATUS.md` §Blockers).
Add the tree parsers and evaluators for the providers the existing `/compute` code already
supports (constant, reference, conditional, div, number dispatcher, weighted list, match_block,
`entity_properties`, `inverted`, `all_of`, `any_of`).

Acceptance: dispatcher tree test for the provider arguments, evaluation tests for each provider
form, `/data modify ... compute` end-to-end test on a block entity, and the allow-list entry in
`commands_tree_parity.rs` removed or narrowed.

### B4. Plugin limits, highest value first (P0)

Files: `java/ferrite-api-impl/**`, `crates/ferrite-server/src/plugins/**`, plus the matching
`docs/plugin-*.md` page. Pick tasks in this order and finish each before starting the next:

1. Runtime recipes for cooking, smithing and stonecutting (`addRecipe`); the client recipe book is
   a separate follow-up, so say so instead of implying it appears.
2. Firework handle methods, colour/shape display and the attached-firework boost approximation.
3. `Player#getConnection` and `postEffects` state limits: implement what the host can answer and
   turn the rest into precise, tested refusals.
4. `World` POI occupancy/space, `Structure#getPieces`, `World#getWorldPath` for runtime worlds, and
   the `locateNearestBiome` interval arguments — implement or document as refused with evidence.

Acceptance: JUnit for the Java surface, a host test where the call crosses the bridge, the
`typical` plugin still passes, and `plugin-stubs-status.md` gains a dated section. Real-client
rendering is not claimed.

### B5. `execute` gaps that need richer entity data (P1)

Only after B1–B3: `execute on attacker|leasher|origin|owner|target`, `if items|slots entity`,
`store ... entity` for non-players, and `execute summon`. These need relation, inventory or NBT
data in `CommandEntity` (`game/entity_index.rs`). Implement the snapshot fields first, then the
predicates, one feature per commit. If a feature needs region state at command time, stop and
report instead of reading another region's live state.

Acceptance: one dispatcher test and one behaviour test per feature; blockers table updated.

Out of lane B: `/test` runner, NMS-typed CraftBukkit methods (intentional), multi-version
translation, compression/encryption offload, real-client checks.

## Not in either lane

| BACKLOG item | Owner |
|---|---|
| §1 real 26.3 client validation | manual, `docs/manual-checklist.md` |
| §5 multi-version translation, writer offload | later; not a two-lane item |
| §6 800-bot measurement | coordinator |
| §7 release hygiene, Windows ZIP, docs truth-up | coordinator |
| §8 runtime worlds with a real client | manual |
| §9 Orca FB review | historical, closed |

## Merge and definition of done

- Push the lane branch when its P0 list is green; do not merge it yourself.
- The first green lane may be merged by the coordinator; the second rebases on the new `main` and
  reruns the full workspace checks. Conflicts are resolved by the coordinator, not by rewriting
  the other lane's work.
- Done for a lane: every P0 task is either implemented with tests or blocked with evidence in the
  report; owned docs are current; `cargo +stable test --workspace`, clippy and fmt pass; Java tests
  pass for lane B; no real-client claim is made. P1 items may remain and are listed in the report.

Lane report format: branch, commit list, per-task status, exact test commands and results, files
outside the lane's column that were touched, blockers, and what remains.
