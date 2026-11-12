# Handoff 2026-11-12: what was verified, what was not, and what to do locally

Branch `claude/trusting-franklin-754wok`, based on `main` at `eedec60` (CI run 309 green).
This file says exactly what was checked in the cloud container and what still needs a human, a
real client, Windows, or a local machine. Nothing below is claimed beyond what is listed under
"Verified".

## Changed in this branch

| Commit | Change |
|---|---|
| `eb89cbe` | Docs and comments reconciled with source: `/ride` module comment, `potion.rs` header, `docs/BACKLOG.md` (entity replacement tests, shield recoil, projectile list, `Structure#getEntities`, chunk cache invalidation, read timeout). |
| `da4c06f` | Lingering potions: `arrow::Kind::LingeringPotion`, `potion::PotionCloud` (vanilla `AreaEffectCloud` numbers), region wiring with split/merge, tests. `AnvilChunkSource::mark_modified` is now `#[cfg(test)]` (gameplay edits use `set_block`). |

## Verified (Linux container, Rust 1.97 stable, Temurin 25.0.5)

- `cargo fmt --all --check`: clean.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `ferrite-entity` lib tests: 31 passed (7 new: cloud timing, radius, reapplication, instant
  effects, thrown kind).
- `ferrite-server` new region tests: 3 passed (bottle with and without effects, split/merge,
  expiry).
- `game::integration`: 80 of 80 passed once `vanilla/26.3/fixture-world` existed. Without it,
  11 of them fail at `integration.rs:3957` with `NotFound`. That is a missing local fixture, not a
  code defect; CI builds it with `tools/make_fixture_world.py`.
- Java (`./gradlew build shadedJar :test-plugin-typical:jar`, Gradle 9.8.1 wrapper): 226 tests,
  0 failures.
- `FERRITE_JAVA_PREBUILT=1 cargo test --workspace --no-fail-fast` with the vanilla data, fixture
  world and Java jars in place: 1682 passed, 0 failed, including doc-tests. A first run without
  the fixture world had 11 failures (see above); the full suite was re-run after it existed.

## Not verified, and why

| Item | Why it is unverified | What to do |
|---|---|---|
| Real 26.3 client behaviour of anything here | No client in the container. `docs/manual-checklist.md` is still entirely unticked. | Run `python tools/wirecheck/run.py`, then the manual checklist; write the commit you ran at the top. |
| Lingering potion cloud | Server-side only. No area-effect-cloud entity, particle or metadata is sent, so a player sees nothing and only the effects arrive. Mobs are not affected (same as splash potions). No test drives a real player through `tick_potion_clouds` end to end; the unit tests cover the cloud maths and the region bookkeeping. | Add the entity spawn and metadata (radius, colour, particle) once a client can be checked; add a player-level region test. |
| Windows CI | Only Linux was run. The Windows-only stall in `four_crystals_respawn_the_dragon_through_the_full_sequence` was never reproduced; on Linux the whole `game::integration` group (80 tests) takes about 48 s. Commit `da059a0` raised CI's `timeout-minutes` (job 30 to 45, test step 25 to 40), which gives a stall more room rather than explaining it. | Run the suite on Windows; if it stalls, bound the test (it depends on `/tick rate 10000` and 5 ms sleeps) and find the Windows-only wait. |
| Everything outside `crates/ferrite-entity` and the new region tests | Covered only by the existing suite. | Review the diff. |
| `docs/perf` numbers | Not re-measured; the 800-bot baseline is from `6aca3fc`. | `docs/perf/README.md` has the command. |
| Windows ZIP, Tauri GUI, Dependabot #10 | Cannot be done here. | Unchanged from `docs/coordinator/remaining-2026-11-11.md` #1, #33. |

## Not done, and why

| Item | Reason |
|---|---|
| Fishing bobber | A new entity with hook, bite and reel mechanics and client metadata; nothing exists in the code to extend. Needs a real client to check the animation. |
| Cartography table menu | The server has no map item or map data model (only the plugin `MapData` packet). Zoom out needs a new map at scale + 1, so a lock/clone-only menu would be misleading. Build the map model first. |
| `EntityTransformer` lists (`CapturedStructure.java`, `FerriteTemplate.java`) | Needs host-side entity placement inside a structure transform; not designed. Still refused with an explicit `UnsupportedOperationException`. |
| Paper lifecycle event handlers (`FerriteInternalBridge.java:39`) | Scope decision for the owner: is the Paper lifecycle API in scope? Still refused explicitly. |
| Plugin world environments other than NORMAL/NETHER/THE_END (`main.rs:583`) | Decision: keep refusing; the error names the environment. |
| Shoulder parrot entity and display | Needs an entity plus rendering; client-visible. |
| Dragon perched subphases, purple/2400-tick gateway beams | Client-visible behaviour; `docs/end-dimension.md` lists them. |
| Vanilla goal parity for each mob | Open-ended audit; do not claim parity. |
| `ferrite-net`: compression/encryption off reader/writer threads, chat/command rate limits | Needs a plan; no lane owns `ferrite-net`. |
| Multi-version translation layer, waterlogging and fluid/redstone audit | Needs a decision / vanilla audit first. |
| `docs/COMMANDS-STATUS.md` re-count of stubbed roots | Not re-counted. |

## Local setup that works on Linux

`rust-toolchain.toml` pins `stable-x86_64-pc-windows-msvc`, so override it, and use Java 25
(`tools/fetch_vanilla.py` and the Gradle toolchain both need it; the system Gradle 8.x cannot run
on it, use the wrapper).

```sh
export PATH=$HOME/.cargo/bin:$PATH RUSTUP_TOOLCHAIN=stable
export JAVA_HOME=/path/to/jdk-25 PATH=$JAVA_HOME/bin:$PATH
python3 tools/fetch_vanilla.py            # vanilla/26.3/generated (gitignored)
python3 tools/make_fixture_world.py       # vanilla/26.3/fixture-world, about 3 minutes
(cd java/ferrite-api-impl && ./gradlew --console=plain build shadedJar :test-plugin-typical:jar)
FERRITE_JAVA_PREBUILT=1 cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

## Local verification and improvement work to pick up

1. Run the checklist above on Windows and on a real client; tick `docs/manual-checklist.md`.
2. Install the ECC skills locally (the `ecc` skill/plugin was not found in the container's
   plugin or skill catalogue, so it was not used) and run its review and verification skills on
   this branch's diff, especially `PotionCloud` and the region wiring.
3. Run `/code-review` and a mutation-testing pass on `ferrite-entity/src/potion.rs`; the cloud
   boundary conditions (`WAIT`, `MIN_RADIUS`, the 5-tick check) are where an off-by-one hides.
4. Add the lingering cloud entity packets and a player-level region test (see "Not verified").
5. Add a project skill (for example `.claude/skills/ferrite-checks`) that encodes the setup block
   above, so a fresh session does not rediscover the toolchain override, Java 25, the fixture
   world and the Gradle wrapper.
6. `crates/ferrite-server/src/game/region.rs` is over 12,000 lines; `tick_potion_clouds` and the
   other cloud/projectile handling are candidates to move into their own module.
7. Re-run `docs/coordinator/remaining-2026-11-11.md` against source after merging; it is a
   snapshot, and several of its rows were already closed when this branch started.
