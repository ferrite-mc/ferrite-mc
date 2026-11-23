# Performance baseline (2026-11-11, base `6aca3fc`)

Raw outputs are committed next to this file. Nothing here claims a speedup: the only
server-side change on this branch that touches the measured paths is a statistics counter and a
gateway age write that only runs in the End, so these numbers are the baseline for later work.

Machine: Linux 7.0 x86_64, Intel Xeon Platinum 8358 (12 logical CPUs), 62 GiB RAM.
Server and bots share one process (`examples/bots.rs`), so CPU and RSS include the 800 bot
clients. Region tick times are sampled around region tick callbacks including the join phase
unless noted; the join phase is printed separately.

## 800-bot load (superflat and fixture world)

```sh
export RUSTUP_TOOLCHAIN=stable CARGO_TARGET_DIR=$HOME/ferrite/target-rw3 CARGO_BUILD_JOBS=4
cargo build --release -p ferrite-server --example bots
ulimit -n 65535
BOTS_STAGGER_MS=5 BOTS_FORCELOAD=20 BOTS_VIEW=2 \
  $CARGO_TARGET_DIR/release/examples/bots 800 > docs/perf/bots-800-runN.txt
```

`BOTS_STAGGER_MS` spreads the 800 connects (all at once they overflow the accept backlog and
reset). `BOTS_VIEW` is the client view distance; the server view distance is 2. `BOTS_FORCELOAD=n`
runs `n` cycles of `/forceload add` of 256 chunks plus `/forceload remove all` through the
console during the 30 s walk. Without `BOTS_FORCELOAD` it is the plain walk.

| World | Run | avg region tick | p99 region tick | RSS | CPU | cache hit rate |
|---|---|---:|---:|---:|---:|---:|
| superflat | 1 | 63.4 ms | 146.1 ms | 1248 MiB | 798% | 99.8% |
| superflat | 2 | 75.8 ms | 192.3 ms | 1252 MiB | 880% | 99.7% |
| fixture | 1 | 9.0 ms | 31.6 ms | 1750 MiB | 886% | 99.8% |
| fixture | 2 | 9.1 ms | 41.9 ms | 1932 MiB | 864% | 99.9% |

Raw: `bots-800-run1.txt`, `bots-800-run2.txt` (join phase, per-coordinator-arm timings and the
profiler tree are in the files). Run-to-run spread on the superflat world is about 20%, so treat
differences under that as noise.

What the profiler shows on the superflat world (run 1): `region-tick/track-entities` is 21.9 s of
the 29.3 s spent in region ticks and `global-tick/entity-tracking` is another 30.5 s over the 30 s
walk. 800 bots standing in one area track each other, so entity tracking grows with the square of
the player count; chunk streaming (`players/chunk-stream`, 0.5 s) is small by comparison. The
fixture world is about 7x cheaper per tick; why was not investigated (the bots use the same walk
and spawn in both). Entity tracking is the first place to look for the 50 ms budget at 800
players; it is not changed here.

## Re-run on branch `w6-protocol` (2026-11-12, head `3652b74`)

Same command and machine as the baseline above, built with
`cargo build --release -p ferrite-server --example bots`. This branch adds the chat/command rate
limits (on by default in `ServerConfig`; the bots send no chat), the username-validation switch,
and the `[rate-limits]` config. Raw: `bots-800-w6-run1.txt`, `bots-800-w6-run2.txt`.

**The machine was not quiet.** Other agents were building and testing on the same 12 CPUs. The
load average (1/5/15 min) was 14.8 / 15.5 / 9.8 at the start of run 1 and 39.6 / 24.4 / 13.5 at its
end; run 2 started at 37.4 / 24.2 / 13.5 and ended at 30.4 / 25.2 / 14.7. Run 1 reached only 380%
CPU on the superflat world (baseline: 798-880%), so it was starved. Treat both runs as
contended, and run 1 as unusable for comparison.

| World | Run | avg MSPT | p99 MSPT | RSS | CPU | cache hit rate |
|---|---|---:|---:|---:|---:|---:|
| superflat | 1 | 94.4 ms | 250.7 ms | 1580 MiB | 381% | 99.8% |
| superflat | 2 | 69.0 ms | 177.9 ms | 1356 MiB | 690% | 99.9% |
| fixture | 1 | 9.1 ms | 74.9 ms | 2272 MiB | 681% | 99.8% |
| fixture | 2 | 9.2 ms | 40.8 ms | 1998 MiB | 878% | 99.7% |

Baseline for reference (`6aca3fc`, quieter machine): superflat 63.4 / 75.8 ms avg and 146.1 /
192.3 ms p99, RSS 1248 / 1252 MiB; fixture 9.0 / 9.1 ms avg and 31.6 / 41.9 ms p99, RSS 1750 /
1932 MiB.

What can and cannot be said:

- Average MSPT is within the baseline's own run-to-run spread (about 20% on superflat) for
  superflat run 2 (69.0 vs 63.4-75.8 ms) and for both fixture runs (9.1-9.2 vs 9.0-9.1 ms). Nothing
  here shows a change from this branch.
- Superflat run 1 (94.4 ms) and the higher p99 and forceload latencies (for example `/forceload add`
  mean 147 ms in run 1 and 75 ms in run 2, against 49-50 ms at baseline) coincide with the heavy
  load, and no run was made on a quiet machine, so they are not attributed to the code.
- RSS is 5-30% above baseline in these runs (superflat 1356-1580 vs 1248-1252 MiB, fixture
  1998-2272 vs 1750-1932 MiB). The baseline's own fixture RSS varied by 10% between its two runs,
  and no run isolated the cause, so no claim is made about whether this branch changed memory use.
- A quiet-machine re-run was still needed here; it is recorded in the next section.


## Re-run on branch `w6-protocol`, quiet machine (2026-11-12, head `9d1fd2d` + `c0357b2`)

Command (both runs; release build of `examples/bots` at the head named above, which adds the
coordinator `biased;` select and the headless-piston cascade):

```sh
ulimit -n 65535
BOTS_STAGGER_MS=5 BOTS_FORCELOAD=20 BOTS_VIEW=2 target/release/examples/bots 800
```

Raw: `bots-800-w6-idle-run1.txt`, `bots-800-w6-idle-run2.txt`, each with the load average before and
after. Run 1 started at load 1.56 / 4.91 / 6.21, i.e. with the box otherwise idle (the earlier
re-run above was at 15-39). Run 2 started at 15.1 / 8.3 / 7.3: that figure is mostly the decaying
average of run 1, which itself drives ~10 CPUs, so it is not evidence that other agents were
active, but it was not checked per process either.

| World | Run | avg MSPT | p99 MSPT | RSS | CPU | cache hit rate |
|---|---|---:|---:|---:|---:|---:|
| superflat | 1 | 46.2 ms | 102.7 ms | 1261 MiB | 1025% | 99.8% |
| superflat | 2 | 51.9 ms | 141.0 ms | 1221 MiB | 1023% | 99.7% |
| fixture | 1 | 8.6 ms | 33.7 ms | 1900 MiB | 982% | 99.8% |
| fixture | 2 | 7.5 ms | 30.1 ms | 1710 MiB | 989% | 99.7% |

Baseline (`6aca3fc`): superflat 63.4 / 75.8 ms avg, 146.1 / 192.3 ms p99, RSS 1248 / 1252 MiB;
fixture 9.0 / 9.1 ms avg, 31.6 / 41.9 ms p99, RSS 1750 / 1932 MiB.

What can and cannot be said:

- With the machine quiet, the rate-limit work did not show a cost: RSS is 1221-1261 MiB on
  superflat (baseline 1248-1252) and 1710-1900 MiB on fixture (baseline 1750-1932), inside the
  baseline's own spread.
- Average and p99 MSPT are *lower* than the baseline on superflat (46-52 vs 63-76 ms avg). The
  baseline machine state is not known to be comparable, and the two runs here differ by 12% from
  each other, so this is not claimed as a speedup from the branch; it only shows there is no
  regression at 800 bots.
- Fixture p99 (30-34 ms) matches the baseline's 32-42 ms. The bots send no chat, so these runs
  do not exercise the rate limiter itself; its cost is covered by the unit tests only.


## W7: 800 bots on `0c2ef7f` (main + harness) vs `0f9737c` (with the chunk-lookup fix)

Date 2026-11-12. Host: 12 vCPU Xeon Platinum 8358, 62 GiB RAM, Linux, **shared with the other
W7 workers**: the 1-minute load average before the four runs was 3.7, 14.6, 16.7 and 20.8, so
this is not a quiet machine and the runs are interleaved (base, head, base, head) to spread
the drift. Two runs per variant is a small sample.

```sh
ulimit -n 65535
BOTS_STAGGER_MS=5 BOTS_FORCELOAD=20 BOTS_VIEW=2 target/release/examples/bots 800
```

Both binaries use the same harness (`examples/bots.rs` as of `0c2ef7f`, which adds the figures
below); `base` is main (`236f0e2`) plus only that harness, `head` adds the
`AnvilChunkSource` lookup fix. Raw output with load before/after: `docs/perf/w7/bots-800-*-run*.txt`.
Workload: 800 bots join 5 ms apart, walk randomly for 30 s (view distance 2, one 4 Hz
position update each), while 20 `/forceload` cycles run; superflat world, then the fixture
world. All four runs: 800/800 joined, 800/800 walked to the deadline, 77-85 process threads,
about 35 s elapsed, 43.7-47.6 MiB/s received in total (about 56-61 KiB/s per bot).

| | superflat avg / p50 / p99 MSPT | RSS | fixture avg / p50 / p99 MSPT | RSS |
|---|---|---:|---|---:|
| base run 1 | 73.4 / 68.5 / 152.1 ms | 1208 MiB | 8.5 / 5.9 / 41.1 ms | 1846 MiB |
| head run 1 | 66.8 / 64.3 / 136.6 ms | 1238 MiB | 9.2 / 7.3 / 32.8 ms | 1801 MiB |
| base run 2 | 63.7 / 59.5 / 153.1 ms | 1239 MiB | 8.8 / 6.0 / 34.5 ms | 1791 MiB |
| head run 2 | 60.1 / 59.2 / 122.6 ms | 1260 MiB | 8.0 / 5.9 / 29.1 ms | 1854 MiB |

Reading: head is lower on superflat in both pairs (average -9% and -6%, p99 -10% and -20%) and
mixed on fixture (+8% and -9% average). The spread between the two base runs is itself 13%, the
load differed, and there are two runs, so **this shows no regression and a possible small
improvement on superflat, not a measured speedup**. The fix's effect is clear where block
reads dominate (below), not in this walk, where the 800 bots stay in a few chunks.

Network-side figures from the same runs (all four, same harness; the probes run on the
server's own tokio runtime together with the 800 in-process bot tasks, so they include the
bots' CPU use and are for comparing runs, not absolute client-visible latency):

| figure | idle | join storm (about 4 s) | walk (30 s) |
|---|---|---|---|
| 1 ms timer lateness p50 / p99 (4 probe tasks) | | 1.14-1.18 / 4.2-6.8 ms | 1.40-1.53 / 94-115 ms (p90 21-31 ms) |
| `/list` round trip, probe bot, p50 | 3.2-4.0 ms | 3.0-3.9 ms (n=4) | 31-127 ms (n=24-26) |
| keepalive RTT, server-measured, smoothed per player | | | p50 28-34 ms, p99 103-182 ms, max 231-632 ms (n=801 each) |
| bot join time (connect to first position) | | p50 27-37 ms, p99 74-101 ms, max 103-136 ms | |

The join storm is where compression and encryption of chunk data peaks, and timer lateness
there is near the timer floor (p99 under 7 ms). The walk phase has little chunk traffic but far
higher lateness, so the delay follows total CPU saturation (the process used 900-1000% of 1200%)
from region ticks and the bots, not the inline writer work. This does not support the writer
offload in `docs/NET-WRITER-OFFLOAD.md`; see the update there.

### Block reads were O(cached chunks)

`AnvilChunkSource::block` (and `sky_light`, `with_loaded_chunk`, the block-entity edit paths)
found a chunk by iterating every cache entry under the cache lock. Regression test
`game::anvil::tests::block_reads_do_not_scan_the_whole_cache` (debug build, 20000 reads, best of
7): 1 cached chunk 3.5 ms, 1500 cached chunks **621 ms** before the fix; 17 ms for both after.
Effect on the End fight tests (4 CPUs plus 2 busy-loop processes): the dragon respawn went from
46-312 s (2 of 8 runs over the 300 s bound) to 35 s, details in `docs/handoff/w7-net-validation.md`.

## `/forceload` through the console, end to end

Measured inside the same runs: time from `World::console_command("forceload add x 0 x+255 255")`
(256 chunks, the command maximum) to the command reply, while 800 bots walk. The command is
answered by the world loop; the scheduler applies the holder batch on its own threads, so this
is the latency a command sender sees, not the time until every region holds the chunks.

| World | Run | add 256 chunks mean / p99 / max | remove all mean / p99 / max |
|---|---|---:|---:|
| superflat | 1 | 49.3 / 107.7 / 358.6 ms | 15.4 / 62.4 / 103.9 ms |
| superflat | 2 | 50.1 / 73.3 / 493.0 ms | 20.9 / 78.2 / 105.3 ms |
| fixture | 1 | 47.8 / 180.2 / 249.1 ms | 30.1 / 60.5 / 266.0 ms |
| fixture | 2 | 98.1 / 361.6 / 422.9 ms | 35.4 / 128.1 / 132.6 ms |

Under this load the command waits on the same loop that is running the 60+ ms ticks above, so
these figures follow the tick time more than the holder code.

## Batched holder changes in isolation

```sh
cargo run -p ferrite-region --release --example holders
```

One scheduler, 4 threads, median of 15 runs; `add_chunk` per chunk versus one `add_chunks`
call (`region-holders.txt`):

| chunks | one by one: add | one by one: remove | batched: add | batched: remove |
|---:|---:|---:|---:|---:|
| 16 | 29 us | 5 us | 18 us | 4 us |
| 256 | 415 us | 77 us | 294 us | 54 us |
| 1024 | 1753 us | 309 us | 1325 us | 221 us |
| 4096 | 12869 us | 1288 us | 7266 us | 1186 us |

Batching measures 1.4x to 1.8x faster for adds and 1.0x to 1.4x for removes at these sizes. A
256-chunk `/forceload` costs about 0.3 ms in the scheduler, which is negligible next to the tick
time above.

## Holder batching end to end (2026-11-19)

`docs/perf/2026-11-19/holder-batching.md` records two 40-cycle `/forceload` runs after the
piglin merge: batching measured 5,078 vs 5,112 changes/s (0.99x) and 50.9 vs 426.8 ms of
holder-signal time (8.39x lower). It is a measured holder-path latency reduction, not an
end-to-end throughput speedup. The 800-bot rerun and the experimental player-stream harness
remain open; no 800-bot claim is made here.

## Hotpath A/B on 2026-11-23 (HEAD `0c35f68c` vs working tree)

Same host (12-vCPU Xeon Platinum 8358, 62 GiB RAM), release build of `examples/bots`, two
interleaved runs per binary (HEAD, optimized, HEAD, optimized). The host 1-minute load average
ranged 7.39–20.47, so absolute times are noisy; only the paired difference is interpreted.
Workload:

```sh
ulimit -n 65535
# 800 bots (both worlds, 30 s walk)
BOTS_STAGGER_MS=5 BOTS_VIEW=2 target/release/examples/bots 800
# 200 bots
BOTS_STAGGER_MS=2 BOTS_PROBES=0 target/release/examples/bots 200
```

This is a different workload from the `BOTS_FORCELOAD=20` baseline above, and its absolute
numbers are not comparable to the `BOTS_LIGHTING_POOL=1` 800-bot fixture run in
`docs/perf/2026-11-21/server-comparison.md`. The 200-bot runs used the harness default
`view=8`; the 800-bot runs used `view=2`. Every run reported 800/800 (or 200/200) bots joined and
walked to the deadline.

| Bots | World | Metric | HEAD | optimized | Change |
|---:|---|---|---:|---:|---:|
| 200 | superflat | avg MSPT | 6.422 ms | **4.985 ms** | -22.4% |
| 200 | superflat | p99 MSPT | 13.349 ms | **10.181 ms** | -23.7% |
| 200 | fixture | avg MSPT | 2.204 ms | 2.145 ms | -2.7% |
| 200 | fixture | p99 MSPT | 12.972 ms | 12.901 ms | -0.6% |
| 800 | superflat | avg MSPT | 25.536 ms | **19.825 ms** | -22.4% |
| 800 | superflat | p99 MSPT | 49.464 ms | **37.892 ms** | -23.4% |
| 800 | fixture | avg MSPT | 6.936 ms | 6.824 ms | -1.6% |
| 800 | fixture | p99 MSPT | 34.238 ms | **31.178 ms** | -8.9% |

At 200 bots on superflat, mean process CPU fell from 204.2% to 179.2% and mean RSS from 246.5 to
222.7 MiB. At 800 bots on superflat, CPU fell from 915.0% to 829.2% and RSS from 655.4 to 651.3
MiB. Most of the 800-bot superflat gain is entity tracking: across the paired runs,
`global-tick/entity-tracking` went from 16.09 s to 7.20 s (-55.3%) and `region-tick/finish` from
4.22 s to 1.32 s (-68.8%).

The pass removes the double hash lookup when refreshing the per-observer seen map and reuses one
movement encode per target (`game/global.rs`), reuses mob scratch storage and skips leash work
when no leash exists (`game/region.rs`), precomputes waypoint locator sources and uses epoch
generations instead of keep-sets (`game/waypoints.rs`), and returns early for empty advancement
progress (`game/advancement_packets.rs`). The fixture world is already dominated by other work,
which is why its average changes only -1.6% at 800 bots while p99 improves -8.9%.

## Full 200/800 A/B on 2026-11-23 (HEAD `6db4e5ae` vs working tree)

This is a separate pass from the `0c35f68c` A/B above. `base` is the release binary built at
HEAD `6db4e5ae`; `optimized` is the working tree that adds incremental global entity tracking,
batched target publication, block-entity position scans without NBT copies, in-place entity-index
updates, and an inventory-menu revision fast path. Two interleaved rounds per binary with the
variant order reversed in round 2. The 800-bot base round 2 was rerun after the harness command
hit its timeout, so that pair is not in the original order.

```sh
ulimit -n 65535
# 200 bots: harness default view=8
BOTS_STAGGER_MS=2 BOTS_PROBES=0 target/release/examples/bots 200
# 800 bots: view=2, no /forceload, 30 s walk
BOTS_STAGGER_MS=5 BOTS_VIEW=2 target/release/examples/bots 800
```

All runs report 200/200 or 800/800 joined and walked to the deadline. The host was not quiet:
`uptime` showed a 1-minute load average of 12.1-16.1 around the last three runs; earlier load
points were not logged. Compare the interleaved pairs rather than absolute values. Raw logs:
`docs/perf/2026-11-23/bots-{200,800}-{base,opt}-r{1,2}.txt`.

| Bots | World | Metric | HEAD run 1 / 2 | optimized run 1 / 2 | Mean change |
|---:|---|---|---:|---:|---:|
| 200 | superflat | avg MSPT | 5.950 / 5.172 ms | 4.383 / 4.703 ms | **-18.3%** |
| 200 | superflat | p99 MSPT | 11.990 / 11.531 ms | 9.380 / 9.850 ms | -18.2% |
| 200 | fixture | avg MSPT | 2.135 / 2.128 ms | 1.807 / 1.836 ms | **-14.5%** |
| 200 | fixture | p99 MSPT | 12.952 / 12.131 ms | 12.766 / 13.440 ms | +4.5% (noise) |
| 800 | superflat | avg MSPT | 19.007 / 19.560 ms | 16.429 / 17.565 ms | **-11.9%** |
| 800 | superflat | p99 MSPT | 39.259 / 39.231 ms | 30.034 / 38.349 ms | -12.9% |
| 800 | fixture | avg MSPT | 6.816 / 6.646 ms | 4.663 / 4.490 ms | **-32.0%** |
| 800 | fixture | p99 MSPT | 31.378 / 29.696 ms | 25.825 / 23.926 ms | -18.5% |

The 800-bot superflat p99 spread in the optimized runs is 8.3 ms, so its average is the stronger
signal. The 200-bot fixture p99 moved in the other direction (+4.5%) and is not claimed as an
improvement; its two optimized runs differ by 0.7 ms. Mean process CPU and RSS across the pair:

| Bots | World | CPU HEAD -> optimized | RSS HEAD -> optimized |
|---:|---|---:|---:|
| 200 | superflat | 187.2% -> 180.8% | 224.7 -> 219.0 MiB |
| 200 | fixture | 159.7% -> 159.9% | 391.1 -> 384.2 MiB |
| 800 | superflat | 844.2% -> 803.4% | 646.7 -> 630.0 MiB |
| 800 | fixture | 849.8% -> 824.9% | 875.9 -> 846.5 MiB |

Profiler scopes from run 1 (summed over all regions; the optimized 800-bot run had 782 region
calls versus 768 for HEAD, so these are not normalized per tick):

| Scope | HEAD 800 superflat | optimized 800 superflat | Change |
|---|---:|---:|---:|
| `global-tick/entity-tracking` | 7.393 s | 4.083 s | -44.8% |
| `region-tick` (total) | 12.166 s | 10.791 s | -11.3% |
| `region-tick/track-entities` | 4.440 s | 4.733 s | +6.6% |
| `region-tick/entities` | 4.296 s | 3.828 s | -10.9% |
| `region-tick/finish` | 1.282 s | 0.931 s | -27.4% |
| `region-tick/containers` | 0.909 s | 0.082 s | -91.0% |

On the 800-bot fixture world the same run-1 scopes moved from 7.917 to 4.194 s for global
tracking (-47.0%), 4.222 to 2.859 s for the whole region tick (-32.3%), and 1.219 to 0.084 s for
containers (-93.1%). `track-entities` did not improve at 800 superflat (+6.6%, within the run
spread); the total gain comes from global tracking, finish, containers, and entity work.

The inventory-menu path now compares a monotonic slot revision and only diffs the 46 menu slots
when a slot or the cursor changed. This is visible in the container scope above and does not
alter any packet on an idle tick. Global tracking stores the last pose of each observer and
target, does a full scan only when the player count changes, and otherwise rechecks only moved
observers/targets (a same-size replacement is treated as a moved target). Target state is published in batches, block-entity predicates read positions
without cloning NBT, and the entity index updates existing entries in place and removes stale
UUIDs. A first attempt that returned early before `spawn::natural_spawn` was rejected because it
suppressed natural mob spawning; that early return is not in the measured tree.

## Network-hardening pass on 2026-11-23 (HEAD `093218de` vs working tree)

This pass adds a play keepalive timeout (30 s, vanilla's value), a configuration read timeout,
accept-loop resilience for transient `EMFILE`-class failures, a pooled Mojang session client
with 5 s connect / 10 s total timeouts, an `Arc<str>` favicon so the per-connection config clone
does not copy it, and a single-pass writer batch size calculation. It touches no tick-path code,
so the bot run is a regression check, not a speedup claim.

```sh
ulimit -n 65535
# 200 bots: harness default view=8, probes off
BOTS_STAGGER_MS=2 BOTS_PROBES=0 target/release/examples/bots 200
# 800 bots: view=2, 30 s walk, no /forceload
BOTS_STAGGER_MS=5 BOTS_VIEW=2 target/release/examples/bots 800
```

Two interleaved rounds per binary (base, hardened, hardened, base). Every run reported 200/200
or 800/800 joined and walked to the deadline. The host was not quiet: the 1-minute load at run
start was 3.52/3.72/3.85/4.87 for the 200-bot batch and 9.37/23.54/29.24/20.47 for the 800-bot
batch, and 800-bot process CPU ranged 528.7%-823.7%. Raw logs:
`docs/perf/2026-11-23/bots-{200,800}-{base,opt}-harden-r{1,2}.txt`.

| Bots | World | Metric | base mean | hardened mean | Change |
|---:|---|---|---:|---:|---:|
| 800 | superflat | avg MSPT | 18.039 ms | 19.854 ms | +10.1% |
| 800 | superflat | p99 MSPT | 45.764 ms | 44.023 ms | -3.8% |
| 800 | fixture | avg MSPT | 4.671 ms | 4.692 ms | +0.5% |
| 800 | fixture | p99 MSPT | 27.495 ms | 28.269 ms | +2.8% |
| 200 | superflat | avg MSPT | 5.836 ms | 5.185 ms | -11.2% |
| 200 | superflat | p99 MSPT | 15.845 ms | 13.800 ms | -12.9% |
| 200 | fixture | avg MSPT | 2.003 ms | 2.282 ms | +13.9% |
| 200 | fixture | p99 MSPT | 14.504 ms | 21.889 ms | +50.9% |

Join p50 stayed at 44-48 ms for both binaries in both worlds. The changes move in opposite
directions between worlds and the host load drifted by an order of magnitude, so **no tick-time
change is claimed**; this run shows no regression signal in joins, keepalive RTTs or completed
walks. The security changes are covered by the new `ferrite-net` unit/integration tests. Full
method, per-run values and discussion: `docs/perf/2026-11-23/network-hardening.md`.

## Earlier numbers

`crates/ferrite-server/LOAD_TEST.md` holds the 200-bot Windows figures from 2026-10-28; they were
taken on different hardware and are not comparable to the table above.
