# Lighting worker pool benchmark — 3-worker point and contention (2026-11-21)

Component benchmark for the additive `ferrite-light` worker pool, run in the
`yxshadxw/lighting-bench` worktree on base `2ac1b115`. This is a companion to
`docs/perf/2026-11-21/lighting-pool.md`, which is unchanged; it adds the 3-worker point, a
same-centre contention scenario, per-worker efficiency, and per-measurement load recording.

## Method

```sh
source tools/env.sh
cargo +stable run -p ferrite-light --release --example pool_throughput
```

The example relights 12 full-height (`-4..=19`, sky enabled) chunk neighbourhoods per batch at
1, 2, 3, 4 and 8 workers in three scenarios:

- **disjoint centres** — a 4×3 grid with centres three chunks apart; the 3×3 read
  neighbourhoods do not overlap, so only the chunk table and publication machinery can contend.
- **adjacent centres** — a 4×3 grid with centres one chunk apart; every neighbourhood overlaps.
- **same-centre contention** — 12 identical jobs on centre `(2, 2)`; all jobs publish to the same
  nine chunk tokens, the maximum publication contention the pool can see.

Each configuration is warmed up once, then timed for 3 runs; **best** and **mean** wall time are
reported. `ms/job`, `speedup vs 1` and `efficiency` are derived from **best** wall time
(`efficiency = speedup / workers × 100%`). Efficiency is **88.5–98.0%** at 3 workers,
**87.4–97.3%** at 4, and **48.6–61.1%** at 8. `/proc/loadavg` is read as the full
1/5/15-minute triple before and after every configuration, and `nproc` is read at the start and
end of each invocation. The example runs in about 25 seconds, under the 5-minute budget.

## Machine and conditions

- Intel Xeon Platinum 8358 @ 2.60 GHz, 1 socket, 6 cores, 2 threads per core, **12 logical
  CPUs**; `nproc` = 12 and `available_parallelism` = 12 in both runs; NUMA node 0 = CPUs 0–11.
- 62 GiB RAM (19 GiB used, 43 GiB available during the runs), Linux 7.0.0-1016-oracle x86_64.
- The box is shared with other Orca agents. The 1-minute load average was **4.57–5.77** across
  both runs, i.e. the machine was already busy before the benchmark started. Treat absolute
  times as inflated and comparisons within a run as indicative only.

## Run A

`nproc` at start and end: 12. Load average 5.77/2.66/1.41 at start, 5.04/2.73/1.47 at end.
Each row's load column is the 1/5/15-minute triple before → after that configuration.

### Disjoint centres (12 jobs)

| workers | loadavg 1/5/15 before → after | best wall (ms) | mean wall (ms) | ms/job (best) | speedup vs 1 (best) | efficiency |
|---:|---|---:|---:|---:|---:|---:|
| 1 | 5.77/2.66/1.41 → 5.47/2.65/1.41 | 840.420 | 852.649 | 70.035 | 1.00x | 100.0% |
| 2 | 5.47/2.65/1.41 → 5.47/2.65/1.41 | 435.473 | 458.688 | 36.289 | 1.93x | 96.5% |
| 3 | 5.47/2.65/1.41 → 5.47/2.65/1.41 | 286.373 | 305.369 | 23.864 | 2.93x | 97.8% |
| 4 | 5.47/2.65/1.41 → 5.47/2.65/1.41 | 215.948 | 234.168 | 17.996 | 3.89x | 97.3% |
| 8 | 5.47/2.65/1.41 → 5.51/2.70/1.44 | 173.320 | 184.806 | 14.443 | 4.85x | 60.6% |

### Adjacent centres (12 jobs)

| workers | loadavg 1/5/15 before → after | best wall (ms) | mean wall (ms) | ms/job (best) | speedup vs 1 (best) | efficiency |
|---:|---|---:|---:|---:|---:|---:|
| 1 | 5.51/2.70/1.44 → 5.51/2.70/1.44 | 800.704 | 809.911 | 66.725 | 1.00x | 100.0% |
| 2 | 5.51/2.70/1.44 → 5.51/2.70/1.44 | 416.598 | 426.614 | 34.716 | 1.92x | 96.1% |
| 3 | 5.51/2.70/1.44 → 5.31/2.71/1.45 | 285.395 | 296.291 | 23.783 | 2.81x | 93.5% |
| 4 | 5.31/2.71/1.45 → 5.31/2.71/1.45 | 219.537 | 241.144 | 18.295 | 3.65x | 91.2% |
| 8 | 5.31/2.71/1.45 → 5.31/2.71/1.45 | 175.815 | 197.035 | 14.651 | 4.55x | 56.9% |

### Same-centre contention (12 identical jobs)

| workers | loadavg 1/5/15 before → after | best wall (ms) | mean wall (ms) | ms/job (best) | speedup vs 1 (best) | efficiency |
|---:|---|---:|---:|---:|---:|---:|
| 1 | 5.31/2.71/1.45 → 5.05/2.70/1.45 | 809.543 | 825.865 | 67.462 | 1.00x | 100.0% |
| 2 | 5.05/2.70/1.45 → 5.05/2.70/1.45 | 412.247 | 419.978 | 34.354 | 1.96x | 98.2% |
| 3 | 5.05/2.70/1.45 → 5.05/2.70/1.45 | 275.389 | 283.307 | 22.949 | 2.94x | 98.0% |
| 4 | 5.05/2.70/1.45 → 5.04/2.73/1.47 | 231.478 | 234.451 | 19.290 | 3.50x | 87.4% |
| 8 | 5.04/2.73/1.47 → 5.04/2.73/1.47 | 198.015 | 206.772 | 16.501 | 4.09x | 51.1% |

## Run B

Immediately after Run A. `nproc` at start and end: 12. Load average 5.04/2.73/1.47 at start,
5.06/2.88/1.54 at end.

### Disjoint centres (12 jobs)

| workers | loadavg 1/5/15 before → after | best wall (ms) | mean wall (ms) | ms/job (best) | speedup vs 1 (best) | efficiency |
|---:|---|---:|---:|---:|---:|---:|
| 1 | 5.04/2.73/1.47 → 5.04/2.73/1.47 | 822.165 | 835.346 | 68.514 | 1.00x | 100.0% |
| 2 | 5.04/2.73/1.47 → 4.88/2.74/1.48 | 416.572 | 424.638 | 34.714 | 1.97x | 98.7% |
| 3 | 4.88/2.74/1.48 → 4.88/2.74/1.48 | 281.833 | 293.635 | 23.486 | 2.92x | 97.2% |
| 4 | 4.88/2.74/1.48 → 4.88/2.74/1.48 | 218.851 | 226.230 | 18.238 | 3.76x | 93.9% |
| 8 | 4.88/2.74/1.48 → 4.88/2.74/1.48 | 168.220 | 194.462 | 14.018 | 4.89x | 61.1% |

### Adjacent centres (12 jobs)

| workers | loadavg 1/5/15 before → after | best wall (ms) | mean wall (ms) | ms/job (best) | speedup vs 1 (best) | efficiency |
|---:|---|---:|---:|---:|---:|---:|
| 1 | 4.88/2.74/1.48 → 4.57/2.71/1.47 | 779.703 | 809.413 | 64.975 | 1.00x | 100.0% |
| 2 | 4.57/2.71/1.47 → 4.57/2.71/1.47 | 413.081 | 426.390 | 34.423 | 1.89x | 94.4% |
| 3 | 4.57/2.71/1.47 → 5.24/2.88/1.54 | 293.766 | 298.079 | 24.481 | 2.65x | 88.5% |
| 4 | 5.24/2.88/1.54 → 5.24/2.88/1.54 | 216.283 | 231.268 | 18.024 | 3.61x | 90.1% |
| 8 | 5.24/2.88/1.54 → 5.24/2.88/1.54 | 199.864 | 212.765 | 16.655 | 3.90x | 48.8% |

### Same-centre contention (12 identical jobs)

| workers | loadavg 1/5/15 before → after | best wall (ms) | mean wall (ms) | ms/job (best) | speedup vs 1 (best) | efficiency |
|---:|---|---:|---:|---:|---:|---:|
| 1 | 5.24/2.88/1.54 → 5.06/2.88/1.54 | 806.109 | 809.072 | 67.176 | 1.00x | 100.0% |
| 2 | 5.06/2.88/1.54 → 5.06/2.88/1.54 | 408.866 | 413.213 | 34.072 | 1.97x | 98.6% |
| 3 | 5.06/2.88/1.54 → 5.06/2.88/1.54 | 292.404 | 296.483 | 24.367 | 2.76x | 91.9% |
| 4 | 5.06/2.88/1.54 → 5.06/2.88/1.54 | 221.529 | 231.896 | 18.461 | 3.64x | 91.0% |
| 8 | 5.06/2.88/1.54 → 5.06/2.88/1.54 | 207.233 | 221.754 | 17.269 | 3.89x | 48.6% |

## The 3-worker point

| run | scenario | best wall (ms) | mean wall (ms) | ms/job (best) | speedup vs 1 | efficiency |
|---|---|---:|---:|---:|---:|---:|
| A | disjoint | 286.373 | 305.369 | 23.864 | 2.93x | 97.8% |
| A | adjacent | 285.395 | 296.291 | 23.783 | 2.81x | 93.5% |
| A | same-centre | 275.389 | 283.307 | 22.949 | 2.94x | 98.0% |
| B | disjoint | 281.833 | 293.635 | 23.486 | 2.92x | 97.2% |
| B | adjacent | 293.766 | 298.079 | 24.481 | 2.65x | 88.5% |
| B | same-centre | 292.404 | 296.483 | 24.367 | 2.76x | 91.9% |

Three workers measured **2.65–2.94x** across the six scenario/run points, at **88.5–98.0%**
per-worker efficiency; best wall was **275–294 ms** for 12 full-height jobs. The two lower
points (2.65x adjacent and 2.76x same-centre in Run B) each coincided with the 1-minute load
rising to 5.24, so they are not distinguishable from host noise at this sample size.

## Contention comparison

Best-wall speedup for each scenario, each relative to its own 1-worker run:

| run | workers | disjoint | adjacent | same-centre | same-centre vs disjoint |
|---|---:|---:|---:|---:|---:|
| A | 1 | 1.00x | 1.00x | 1.00x | — |
| A | 2 | 1.93x | 1.92x | 1.96x | +0.03x |
| A | 3 | 2.93x | 2.81x | 2.94x | +0.01x |
| A | 4 | 3.89x | 3.65x | 3.50x | −0.39x |
| A | 8 | 4.85x | 4.55x | 4.09x | −0.76x |
| B | 1 | 1.00x | 1.00x | 1.00x | — |
| B | 2 | 1.97x | 1.89x | 1.97x | 0.00x |
| B | 3 | 2.92x | 2.65x | 2.76x | −0.16x |
| B | 4 | 3.76x | 3.61x | 3.64x | −0.12x |
| B | 8 | 4.89x | 3.90x | 3.89x | −1.00x |

Adjacent centres (all 3×3 neighbourhoods overlapping) cost nothing measurable for most
worker counts, which is the expected result: propagation is private per engine and only the
short publication step takes the nine ordered chunk tokens. Same-centre contention is visible
only when workers outnumber physical cores and jobs are finishing close together: at 8 workers
it loses **0.76x (Run A)** and **1.00x (Run B)** versus disjoint, i.e. 16–20% of the measured
speedup. At 2 and 3 workers the three scenarios are within run-to-run noise. In other words,
lock contention is not what limits this pool at 1–4 workers; job granularity, memory traffic
and host load are.

## Interpretation

- **Scaling shape.** 1.89–1.97x at 2 workers, 2.65–2.94x at 3, 3.50–3.89x at 4, and
  3.89–4.89x at 8. Efficiency is high (roughly 88–99%) through 4 workers and collapses at 8
  (49–61%). Efficiency is high (roughly 87–99%) through 4 workers.
- **SMT and physical cores.** The box has 6 physical cores and 12 logical CPUs. Eight workers
  already oversubscribe the six cores through SMT, so two workers share each core's execution
  resources; the 8-worker column is not an 8-core measurement and 8x is not claimed.
- **Job granularity ceiling.** A batch has 12 jobs. With dynamic dispatch, the slowest worker
  runs `ceil(12 / workers)` jobs, so the no-overhead ceiling is 3x at 3 workers, 4x at 4,
  and **6x at 8**, not 8x. Measured 8-worker speedup is 81% (Run A) / 82% (Run B) of that 6x
  ceiling on disjoint centres, and 68% / 65% on same-centre jobs. The 3-worker point divides 12
  evenly, which is part of why its efficiency is the highest reported.
- **Start-up cost is inside the timing.** Each `relight` call spawns `min(workers, jobs)`
  scoped threads and every worker allocates a fresh full-height `LightEngine` (about 15 MB of
  block/property state at `-4..=19`) before pulling its first job. At 1 worker that is one
  allocation, at 8 workers it is eight plus the thread start-ups; with a ~65–70 ms job this is
  a modest but real fixed cost that grows with worker count and contributes to the 8-worker
  drop-off together with SMT.
- **Same-centre publication.** Duplicate jobs are valid and deterministic, and the measured
  cost is small at low worker counts. Publication ordering (nine ordered chunk tokens) prevents
  deadlock; serialising all publications on the same nine tokens is what shows up at 8 workers.
- **Shared, loaded host.** The 1-minute load stayed between 4.57 and 5.77 on 12 logical CPUs,
  higher than the 3.18–4.56 of the earlier `lighting-pool.md` runs, and 1-worker times are
  correspondingly slower (780–840 ms versus 757–779 ms there). Differences of ~10% between
  runs (for example adjacent at 3 workers: 285 ms in A, 294 ms in B) are larger than any
  plausible pool change and should be read as host noise. Mean wall time was 1–12% above best.
- **No server claim.** This is a synthetic component benchmark on one shared machine with a
  hand-built block view. It says nothing about server tick time, chunk I/O, client traffic or
  memory under load, and the pool is still not wired into `ferrite-server`.

## Limitations

- Shared host with load 4.6–5.8; absolute wall times are inflated and run-to-run spread is
  roughly 10%. Only two invocations of three timed runs per point were taken.
- One synthetic terrain, 12 jobs per scenario, sky light enabled, full overworld height. Results
  for small section ranges would be dominated by per-batch thread and engine start-up.
- The release profile sets `panic = "abort"`, so panic recovery is not exercised here (and is
  not a throughput claim).
- No vanilla fixture differential was run; `tests/vanilla.rs` still skips without
  `vanilla/26.3/fixture-world`.
- No server integration, tick measurement, RSS measurement or player-scale run exists.
