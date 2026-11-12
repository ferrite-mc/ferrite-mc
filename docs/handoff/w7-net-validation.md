# W7 lane 4 handoff: networking measurement and integration reliability

Branch `w7-net-validation` (brief: `w7-net-validation-brief.md`). Based on `origin/main` 236f0e2
(PR #6, ancestry checked), then merged with `origin/main` 3ec1678 (PR #8, maps). Not a release
handoff: the release steps in the brief wait for lanes 1-3 (see "Not done").

## CI state when checked (2026-11-12)

| Run | What | Result |
|---|---|---|
| 43233919648 | main 236f0e2 | success, ubuntu and windows |
| 43298683540 | main 3ec1678 (maps merged) | success, ubuntu and windows |
| 43227184374 | main bb0a47e | failure at `cargo clippy` on both OSes: the broken w6 merge that PR #6 repaired, not a test |
| 43241959878 | PR #7 `w7-api` 81f125a | windows failed at `cargo clippy` (rustc 1.100.0, `needless_range_loop` x4 in `ferrite-inventory/src/menu.rs`), ubuntu passed. Windows CI was already on 1.100.0 while ubuntu was on 1.99.1. The four loops are fixed on main by PR #8, so the PR only needs main merged in |
| 43299884934 | PR #9 `w7-ai-vehicles` 09746bd | both jobs failed in `runtime_world_gap7_create_edit_enter_leave_unload_reopen`: `plugin_lifecycle.rs:333` `write_all` BrokenPipe (linux) and `:353` os error 10053 (windows), i.e. the server dropped the bot's connection. Not reproduced on main here (15.5 s pass). The branch changes only ride/targeting/ai-tick code, so this is lane 2's to read: check the server's stdout/stderr in the test's assertion output and whether the branch's `targeting::publish`/`ai::tick_full` path can panic or close a connection with plugin-spawned cows. The branch has moved since (e7efd1a) |

No log in the runs above shows `four_crystals_respawn_the_dragon...` or
`a_new_end_gateway_sends_the_block_entity_age_for_its_beam` failing; they are intermittent
under load, and I reproduced them locally (below).

## Diagnosis and fixes

### The End fight tests were slow because every block read scanned the whole chunk cache

Reproduction: the unchanged `four_crystals_respawn_the_dragon_through_the_full_sequence` test
binary of main, pinned with `taskset -c 0-3` plus two busy-loop processes on the same CPUs.
Before the fix, 8 runs: 47, 122, 181, 233, 253, 288 s passed and **two failed** (312 s and
313 s: `the respawned dragon gets a boss bar`, i.e. the 300 s bound; one earlier w6-branch run
also hit `no packet 9 within 300s` and `no packet 73 within 120s`, the latter fixed in main by
the coordinator `biased;` select). The new `Stages` timeline (below) showed the respawn
sequence *began* within 0.5 s every time and then needed 600 ticks of the one region that
owns the fight; the profiler showed `region-tick/entities` using almost all tick time and
inside it the mob AI tick. Cause: `AnvilChunkSource::block`, `sky_light`, `with_loaded_chunk`
and the block-entity edit paths found a chunk by iterating all cache entries under the cache
mutex, O(cached chunks) per block read, for every region, every tick. Raising the tick rate
did not help: `/tick rate 10000` makes every region of every dimension spin and they starve
the one region that matters (66-300+ s).

Fix (commit `5f9ef43`): `Cache::ready_column` probes the `(x, z, biome-hint)` key first and
scans only on a miss. Regression test `block_reads_do_not_scan_the_whole_cache`: 20000 reads,
1 cached chunk 3.5 ms vs 1500 cached chunks 621 ms before; 17 ms for both after (the test
fails before, passes after; second test `block_reads_find_a_chunk_cached_under_another_biome_variant`).

Test change (`0f9737c`): the dragon test no longer raises `/tick rate`. After the fix it takes
35.0 s and 34.7 s (600 ticks at 20 TPS) under the same 4 CPUs plus two spinners, and passed in
all three pinned full-suite runs below; the production assertion and the 300 s bound are
unchanged. The gateway test needs 2400 ticks and keeps `/tick rate 10000`: 55-61 s over 4 runs
before, 37 s over 2 runs after.

### Not root-caused: `slow_reader_is_disconnected_without_stalling_ticks`

Failed once in three pinned full-suite runs after the fix (`integration.rs` panic arm "slow
reader got no timed-out disconnect: Some(Kind(UnexpectedEof))", i.e. a clean close without the
disconnect frame). It passed in 55+ isolated runs, and 2 of 25 runs on 2 CPUs with 8 spinners
failed with a different message (tick stall 396/438 ms, scheduler starvation). I tested the
obvious explanation, the server's 30 s write timeout expiring while the bot is not reading, by
setting 1-4 s timeouts (8 runs each): they produced only the accepted resets, no EOF. So
the cause is unproven. Commit `2b61dbe` adds the bot's time since join, frames read and last
packet ids to that failure message, so the next occurrence can be attributed. Assertions are
unchanged; I did not add EOF to the accepted outcomes without evidence that it is a legitimate
server close.

### Not touched: `runtime_world_gap7...` and the plugin tests

gap7 passes locally (15.5 s) on this branch. The earlier `unloadWorld` refusal race was the
coordinator select and is fixed in main (`biased;`, regression test
`unload_right_after_the_client_sees_the_departure_is_not_refused`). I could not run the other
Java plugin tests: the home volume is 98% full and the Gradle build needs it. They run in CI.

### Windows

Nothing here was run on Windows. The CI windows jobs for main were green (runs above). The
`Stages` timeline prints on any panic of the two End tests and with `FERRITE_TEST_STAGES=1`,
including the profiler report, so a Windows failure reports which stage stalled and whether
the region was ticking.

## 800-bot measurement

Raw files `docs/perf/w7/bots-800-{base,head}-run{1,2}.txt`, commands, host, load before/after
and tables are in `docs/perf/README.md` ("W7: 800 bots"). Summary: all four runs 800/800 joined
and walked; superflat avg MSPT 73.4/63.7 (base) vs 66.8/60.1 (head), p99 152/153 vs 137/123 ms;
fixture 8.5/8.8 vs 9.2/8.0 ms; RSS 1.2/1.8 GiB in both; 44-48 MiB/s received. The host was shared
(load 3.7 to 20.8 before the runs), two runs per variant: no regression and a possible small
superflat gain, **not** a demonstrated speedup. The harness (`examples/bots.rs`) now also
reports join time, MSPT p50/p90, throughput, thread count, keepalive RTT, runtime timer
lateness per phase and a `/list` round-trip probe (`BOTS_PROBES=0` turns the probes off).

## Writer offload: not justified

`docs/NET-WRITER-OFFLOAD.md` has the update. Timer lateness is at the timer floor during the
join storm (p99 under 7 ms) and large only in the CPU-saturated walk, so the inline
compression/encryption is not what delays socket tasks here. `ferrite-net`'s writer and
reader are unchanged. The only `ferrite-net` change is `settings::ping_snapshot()` for the harness.

## Wire validation

`python tools/wirecheck/run.py --port 25598 --seconds 4` against the built server (superflat,
offline mode, Java 25): **352 packets OK, 0 bad** (`docs/perf/w7/wirecheck.txt`). The server
was started in a scratch directory with an `eula.txt` written by hand, the way the repo's
`smoke_join` test fixture does. Automated headless tests that pass here: `smoke_join` (shipped
binary: start, bot join, console stop), `console`, `ferrite-net` unit tests.

**Unverified: every item in `docs/manual-checklist.md`.** This host is headless (no Minecraft
26.3 client, no GPU; Xvfb is installed but there is no client to run and no Mojang client
jar may be committed or fetched into the repo). No box was ticked and no bot output stands in
for one. Client procedure for the tested commit: build `ferrite-server` at the final merge
SHA, run `python tools/fetch_vanilla.py`, start with `generator = "superflat"`, set `eula=true`
yourself, join with a 26.3 client (protocol 777), then walk `docs/manual-checklist.md` from
section 1, writing the SHA at the top of that file.

## Clippy 1.100

Workspace `cargo clippy --workspace --all-targets -- -D warnings` is clean on rustc 1.100.0 for
this branch merged with main 3ec1678. CI's ubuntu job still used 1.99.1 when checked, Windows
1.100.0; future PRs from older bases will hit the `menu.rs` lints until they merge main.

## Verification run on the merged tree

- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --all-targets -- -D warnings` (1.100.0): clean.
- `cargo test -p ferrite-server --lib --test smoke_join --test console`: 869 + 1 + 1 passed.
- `cargo test -p ferrite-protocol -p ferrite-net -p ferrite-config -p ferrite-inventory -p ferrite-item`: all passed.
- Pinned full lib suite, 4 CPUs + 2 spinners, three runs of the fix before the main merge:
  836 passed, 836 passed, 835 passed + the slow-reader failure above.

## Not done (blocked or pending)

- **Lanes 1-3 merges** (w7-api PR #7, w7-ai-vehicles PR #9, world-parity; the maps PR #8 is
  merged) and their expanded handoffs (`w7-api-expanded.md`, `w7-ai-vehicles.md`,
  `w7-world-parity.md`): not all merged at the time of writing, so the final integrated
  profile/optimization pass, the comparable 800-bot re-run on the combined main, the release
  workflow review (including the unpinned third-party actions and broad `contents: write` an
  automated review flagged in `release.yml`), the v0.2.1 tag/publish and the asset
  verification have **not** been started. Nothing was tagged or published.
- Root cause of the slow-reader EOF (instrumented, see above).
- Windows reproduction of any flake, and all graphical-client checks.
