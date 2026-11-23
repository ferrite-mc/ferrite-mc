# Detailed chunk, fixture, forceload and 800-bot run (2026-11-23)

This pass adds the criteria that were present in the 2026-11-21 comparison but missing from
the 50/100/200/400-bot fresh-world table:

- cold chunk streaming on a fresh `seed=0` world at `view=6/10/16`;
- chunk streaming from the pre-generated `vanilla/26.3/fixture-world` save;
- 200 bots on that fixture world, with 20 `/forceload` cycles;
- Ferrite at 800 bots on both the fixture and a fresh world;
- Ferrite `flush_bench` (worldgen, storage and hotpath microbenchmarks).

All five servers are measured one at a time on the same host. The runner is
`run_detailed.py`; `analyze.py` writes the summary and SVGs, and `collect.py` copies the
selected artifacts into `raw/` without the per-run copied worlds.

```sh
python3 docs/perf/2026-11-23/detailed-run/run_detailed.py --all
python3 docs/perf/2026-11-23/detailed-run/analyze.py
python3 docs/perf/2026-11-23/detailed-run/collect.py
```

`--pilot` runs Ferrite and Paper on the fixture world only; `--leaf-only` reruns Leaf's
fixture points after the migration-confirm path was added.

## Conditions

- Host: 12-vCPU Xeon Platinum 8358, 62 GiB RAM, Linux; Java 25.0.5.1.
- One server at a time on `127.0.0.1:25567`; each run gets a fresh world directory.
- JVM: `-Xms1G -Xmx4G`, no gameplay plugins except `BenchProbe.jar` for TPS/tick samples.
- Ferrite: release binary, `threads = 0`, `lighting-pool = true`, 16 MiB send queue, no
  connection burst limit. Fresh runs use `.fworld`; fixture runs use Anvil because the save
  is an Anvil world.
- Fixture runs copy `vanilla/26.3/fixture-world/world` (chunks `-3..=18`), with
  `session.lock`, logs and player data excluded. Leaf asks for a world-folder migration
  confirmation; the runner sends `confirm` automatically.
- Chunk stream: one external `examples/bots` client with `BOTS_CHUNKS=1`, `BOTS_VIEW=V`;
  the server view/simulation distance is `V`. The harness counts `ChunkData` packets until
  none arrives for 8 s. Two fresh-world runs and two fixture runs per server.
- 200/800-bot load: `BOTS_STILL=1`, `BOTS_STAGGER_MS=5`, `BOTS_SETTLE_SECS=10`,
  `BOTS_WALK_SECS=40`, `BOTS_VIEW=10`. One run per point.
- Forceload: 20 cycles of `forceload add -48 -48 207 207` (256 chunks) and
  `forceload remove all`, while the 200 bots walk. Each command is followed by a `/seed`
  marker; the time is command dispatch to that marker on the server console.
- Ferrite TPS is read from `/tps` 32 s into the 40 s walk, while every bot is still online.
  JVM TPS is the last probe CSV row with `players == bots`, averaged over the last ten
  one-second samples for tick mean/p99.

## What is and is not compared

- The chunk count differs between implementations because the server sends slightly
  different windows (`169/441/1089` for Ferrite, `213/473/1057` for the JVM forks at
  `view=6/10/16`). The rate is the actual received count divided by the time to the last
  chunk, so it is not normalized to a theoretical window.
- `/forceload` timing is **command RTT**, not the time until all 256 chunks finish loading.
  Ferrite's console command and the JVM command both return after dispatching the ticket
  change. The fixture area includes spawn chunks that 200 bots already keep loaded, so this
  is a command-path measurement under load, not a cold-load throughput result.
- Fixture-world boot includes Leaf/Paper world migration on every copied world; the boot
  column is therefore not a clean engine startup comparison.
- The 800-bot points are Ferrite-only. Running 800 bots on the 4 GiB JVM heap with the same
  view would exceed the comparison's memory budget.
- The host is shared. `schedule.log` records the 1-minute load at every start and finish;
  load-point runs are single observations, and the two chunk runs are reported as min–max.
- The forceload marker uses `/seed` because Ferrite does not echo console `/say` broadcasts
  back to the console; `/seed` is available on all five servers and produces a console
  reply.

## Files

- `run_detailed.py` — server/bot orchestration, per-run `meta.json`, console capture.
- `analyze.py` — `summary.json`, `summary.md`, `chunk-stream.svg`, `fixture-load.svg`,
  `ferrite-800.svg`.
- `collect.py` — copies `meta.json`/`meta.txt`, `bots.log`, JVM `ticks.csv`, compressed
  `console.log.gz`, `schedule.log`, the summaries and `flush_bench.txt` into `raw/`.
- `raw/<server>-<world>[-<bots>]/` — one directory per run; no copied world directories.
- `../chunk-and-fixture.md` — the human-readable report.
