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

## Earlier numbers

`crates/ferrite-server/LOAD_TEST.md` holds the 200-bot Windows figures from 2026-10-28; they were
taken on different hardware and are not comparable to the table above.
