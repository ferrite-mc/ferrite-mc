# Holder batching end to end (2026-11-19)

Base: `main` after the piglin merge, plus the holder-batching toggle and `SyncHolds` from
`yxshadxw/performance-closure`. Host: Linux, 12 logical CPUs, 62 GiB RAM; 1-minute load average
1.75 before the run. Raw output: `holders-e2e.txt`.

```sh
HOLDERS_RUNS=2 FORCELOAD_CYCLES=40 target/release/examples/holders_e2e
```

Workload: 40 cycles per run of a 256-chunk `/forceload add` followed by
`/forceload remove all`, each synchronized until the region scheduler has settled the holds.
Both modes run at the normal 20 TPS tick rate.

| run | mode | holder changes/s | holder signal time (ms) | add mean (ms) | remove mean (ms) |
|---:|---|---:|---:|---:|---:|
| 1 | batched | 5,077 | 52.5 | 50.358 | 50.491 |
| 1 | unbatched | 5,112 | 411.7 | 47.618 | 52.533 |
| 2 | batched | 5,079 | 49.2 | 50.366 | 50.439 |
| 2 | unbatched | 5,111 | 441.8 | 47.378 | 52.785 |

Summary: **5,078 vs 5,112 changes/s (0.99x)** and **50.9 vs 426.8 ms total holder-signal time
(8.39x lower)**. Batching collapses 30,840 per-chunk coordinator signals per run to 240 batched
signals, but the end-to-end rate stays dominated by the 50 ms tick cadence. This is a measured
latency reduction in the holder path, not a throughput speedup.

`batch_chunk_holders` defaults to `true`, which preserves the pre-existing `HoldBatch` behavior;
the toggle only exists for the A/B measurement and for the regression test. No production
default was changed by this lane.

Not measured or not claimed:

- The 800-bot workload was not re-run in this session.
- The player chunk-stream suites are experimental and opt-in (`HOLDERS_STREAM=1`); the harness
  can stall after joining, so they are not part of this result.
- `/tick rate 10000` variants are opt-in and not usable yet: at that rate the coordinator's
  event branch can starve behind the always-ready clock, so even the rate-change command may not
  complete. This remains a separate coordinator bug, not a holder-batching result.
- No real Minecraft 26.3 client was attached; no client-visible latency claim is made.
