# Ferrite spawn region at 20 TPS — 50/100/200/400 bots (2026-11-23)

Follow-up to the [full same-day comparison](server-comparison.md). This pass reruns Ferrite
and all four JVM servers; the earlier version of this report reused the JVM columns from a
previous run. Ferrite is the release binary at `5e76edd2`.

## Workload

- Linux 7.0 x86_64, Intel Xeon Platinum 8358, 12 logical CPUs, 62 GiB RAM.
- Ferrite `--headless`, fresh `seed=0` world, `generator = "rust"`, `storage = "fworld"`,
  `view-distance = 10`, `threads = 0`, `lighting-pool = true`.
- External `examples/bots` client: 50, 100, 200 or 400 bots, `BOTS_STILL=1`,
  `BOTS_VIEW=10`, `BOTS_STAGGER_MS=5`, `BOTS_SETTLE_SECS=10`, 30 s walk. One server at a time.
  The 50-bot point was added in a short second batch.
- The 400-bot sustained run walks for 90 s and starts its 20 s profile 60 s into the walk.
- TPS is Ferrite's own `Region 0` line, not the coordinator/global tick. Global TPS was
  `20.0` in every run.

## Results

| Run | Region 0 TPS (5 s / 1 m / 5 m) | Join p50 / p99 | CPU | Peak RSS | entities | track-entities | fluids/advance | random | finish |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 50 bots, 20 s profile | **20.0 / 20.0 / 20.0** | 49 / 78 ms | 919% | 769 MiB | 2.333 ms | 4.057 ms | 11.074 ms | 6.587 ms | 0.440 ms |
| 100 bots, 20 s profile | **20.0 / 20.0 / 20.0** | 48 / 76 ms | 925% | 818 MiB | 2.798 ms | 4.415 ms | 12.157 ms | 7.173 ms | 0.582 ms |
| 200 bots, 20 s profile | **20.0 / 20.0 / 20.0** | 55 / 93 ms | 944% | 899 MiB | 3.203 ms | 5.921 ms | 13.061 ms | 7.155 ms | 0.662 ms |
| 400 bots, 20 s profile | **20.0 / 20.0 / 20.0** | 72 / 207 ms | 943% | 981 MiB | 4.418 ms | 7.326 ms | 15.035 ms | 8.643 ms | 1.022 ms |
| 400 bots, 60 s + 20 s profile | **20.0 / 20.0 / 20.0** | 70 / 270 ms | 744% | 977 MiB | 2.307 ms | 5.021 ms | —* | 3.790 ms | 0.618 ms |

Per-system values are `ms/call` from the 20-second region profile. CPU is the server process
only, derived from `/proc/<pid>/stat` over the measured wall time. Peak RSS is the maximum of
one sample per second. All runs completed 50/50, 100/100, 200/200 or 400/400 joins; the bot harness
exited `0` in all five runs.

\* `region-fluids/advance` did not enter the long run's top-10 profile list.

## Before / after

| Workload | Before | After |
|---|---:|---:|
| 50 bots, spawn Region 0 | — | **20.0 TPS** |
| 100 bots, spawn Region 0 | — | **20.0 TPS** |
| 200 bots, spawn Region 0 | 16.6 TPS | **20.0 TPS** |
| 400 bots, spawn Region 0 | 12.3 TPS | **20.0 TPS** |
| 400 bots, 90 s sustained | 14.8 TPS | **20.0 TPS** |

Before values are the earlier `254c444d` runs recorded in
[server-comparison.md](server-comparison.md) and the previous uncommitted optimization
profile on the same host. They are single observations and the host was not quiet.

## Fresh head-to-head

The JVM columns below were measured in this pass, not reused from an earlier report.

### 200 bots

| Metric | Ferrite | Folia | Leaf | Paper | Purpur |
|---|---:|---:|---:|---:|---:|
| Spawn/server TPS | **20.0** | 18.7 | **20.0** | 14.6 | **20.0** |
| Join p50 / p99 | **55 / 93 ms** | 10,844 / 19,542 ms | 2,399 / 11,025 ms | 2,766 / 10,770 ms | 2,523 / 12,466 ms |
| Peak RSS | **899 MiB** | 1,704 MiB | 3,606 MiB | 2,013 MiB | 1,972 MiB |

### 400 bots

| Metric | Ferrite | Folia | Leaf | Paper | Purpur |
|---|---:|---:|---:|---:|---:|
| Spawn/server TPS | **20.0** | —† | **20.0** | 12.8 | **20.0** |
| Join p50 / p99 | **72 / 207 ms** | —† | 11,107 / 21,115 ms | 12,556 / 26,482 ms | 11,555 / 22,014 ms |
| Peak RSS | **981 MiB** | 1,878 MiB† | 4,468 MiB | 2,483 MiB | 3,034 MiB |

† Folia reached 275/400 players and then closed the connections. Its CPU and RSS cover that
partial run; no 400-bot TPS is claimed.

Ferrite holds the same `20.0` spawn-region TPS as Leaf and Purpur at every count, while
keeping 42x–199x lower median join latency and 1.97x–4.6x lower peak RSS. Paper reports
`19.3 / 17.6 / 14.6 / 12.8` TPS at 50 / 100 / 200 / 400; Folia reports `19.3 / 18.9 / 18.7`
before closing the 400-bot run at 275 players. At 50 bots Ferrite's join `p50` is `49 ms`
versus `2.0–3.8 s` on the JVM servers. Ferrite's CPU cost remains high (`919–944%`, `744%`
sustained) because world generation, lighting and sessions are parallelized; this is not a
CPU-saving claim.

## What changed

The dominant long-run regression was not the network or fluid scheduler: natural spawning
used a **per-player** cap. With 400 players sharing one region that allowed 28,000 monsters
and 4,000 animals, and the entity store kept growing until AI and tracking dominated the
region tick.

`ferrite-entity::spawn` now follows vanilla `SpawnState`: the category cap is scaled by the
number of spawnable chunks divided by `17 × 17`, not by the number of players viewing those
chunks. The region passes its loaded chunk count; the old per-player wrapper remains for
callers without a chunk inventory.

The earlier hot-path changes are also included in `5e76edd2`:

- viewer-batched tracker, fluid and random-tick block updates through `Outbound::Batch`;
- a fast path for stable source water surrounded by source water;
- synthetic spawn-cap tests, including the `289`-chunk scale.

## Reproduction

The benchmark used the repository's external bot harness with the options above. Every raw
`meta.txt`, probe `ticks.csv`, the exact runner/parser scripts and the run schedule are in
[`full-run/`](full-run/); the cross-server tables are in
[server-comparison.md](server-comparison.md). The world and bot counts were regenerated for
each run.

![Ferrite spawn region 20 TPS](spawn-region-20tps.svg)
