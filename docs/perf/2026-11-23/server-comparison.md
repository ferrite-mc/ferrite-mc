# Ferrite vs Folia · Leaf · Paper · Purpur 26.3 — Fresh World 200/400 Bots (2026-11-23)

Sequential head-to-head run on the same host (`Linux 7.0 x86_64`, Intel Xeon Platinum 8358,
12 logical CPUs, 62 GiB RAM, Java 25.0.5.1). One server ran at a time and was shut down before
the next started. Ferrite is the release binary at `254c444d`; the JVM servers are 26.3 builds
Paper 259, Folia 26, Purpur 2660 and Leaf 28.

## Workload

- Fresh empty world, `level-seed=0`, `view-distance=10`, `simulation-distance=10`.
- JVM: `-Xms1G -Xmx4G`, no plugins except the measurement plugin.
- Ferrite: `ferrite-server --headless`, `generator = "rust"`, `storage = "fworld"`,
  `threads = 0`, `lighting-pool = true`, 16 MiB burst queue, no connection burst limit.
- External `examples/bots` client: 200 or 400 bots, `BOTS_STILL=1`, `BOTS_VIEW=10`,
  `BOTS_STAGGER_MS=5`, `BOTS_SETTLE_SECS=10`, `BOTS_WALK_SECS=30`.
- One run per server and bot count. The host was not quiet: the 1-minute load average was
  `5.19` after the 400-bot JVM batch and `14.28` after the Ferrite batch. Results are single
  observations and carry no error bars.
- The JVM plugin sampled `Bukkit.getTickTimes()` and `Bukkit.getTPS()[0]` once per second and
  wrote `epoch_ms,mean_ms,p99_ms,max_ms,tps1,players`. Ferrite was sampled with the console
  `/tps` and `/mspt` commands at 20 s and 30 s after the bot run began.

## Results

### 200 bots

| Metric | Ferrite | Folia 26.3 | Leaf 26.3 | Paper 26.3 | Purpur 26.3 |
|---|---:|---:|---:|---:|---:|
| **Join `p50` / `p99`** | **59 / 112 ms** | 11,802 / 24,850 ms | 2,345 / 8,673 ms | 2,568 / 13,037 ms | 4,714 / 10,866 ms |
| **Server TPS (1 min)** | 16.6 (spawn region) | 16.4 | 20.0 | 14.5 | 20.0 |
| **JVM mean tick, last 10 s** | — | 30.05 ms | 16.16 ms | 17.56 ms | 29.37 ms |
| **JVM p99 tick, last 10 s** | — | 70.19 ms | 22.76 ms | 25.59 ms | 66.91 ms |
| **CPU** | 977% | 329% | 366% | 335% | 390% |
| **Peak RSS** | **886 MiB** | 1,730 MiB | 3,666 MiB | 2,429 MiB | 1,860 MiB |

### 400 bots

| Metric | Ferrite | Folia 26.3 | Leaf 26.3 | Paper 26.3 | Purpur 26.3 |
|---|---:|---:|---:|---:|---:|
| **Join `p50` / `p99`** | **72 / 198 ms** | 262/400, connection closed† | 8,711 / 19,919 ms | 13,432 / 27,589 ms | 15,620 / 26,902 ms |
| **Server TPS (1 min)** | 12.3 (spawn region) | —† | 20.0 | 10.6 | 20.0 |
| **JVM mean tick, last 10 s** | — | —† | 41.62 ms | 45.19 ms | 44.40 ms |
| **JVM p99 tick, last 10 s** | — | —† | 55.36 ms | 62.50 ms | 59.75 ms |
| **CPU** | 995% | 630% at 262 players† | 459% | 448% | 456% |
| **Peak RSS** | **961 MiB** | 1,883 MiB at 262 players† | 4,547 MiB | 2,507 MiB | 2,558 MiB |

† Folia did not finish the 400-bot run. The bot harness reached 262 online players and then
received `server closed the connection`; its CPU and RSS cover that partial run and are not a
400-bot result.

## Reading the numbers

- **Joins are Ferrite's clear win.** All 200/400 bots completed in `59–72 ms` at the median,
  versus `2.3–15.6 s` on the JVM servers at 400 bots. At 400 bots the Ferrite `p50` is about
  121x lower than Leaf, 187x lower than Paper and 217x lower than Purpur.
- **Memory is much lower.** Ferrite peaked at `886 MiB` with 200 bots and `961 MiB` with 400,
  versus `1.7–3.7 GiB` for the JVM servers at 200 and `2.5–4.5 GiB` at 400.
- **Ferrite uses more CPU in this workload.** `977–995%` of one 12-vCPU host, because world
  generation, lighting and 400 sessions run in parallel. The JVM servers used `329–459%` for
  the completed runs. This is a throughput-for-latency trade, not a like-for-like CPU saving.
- **Tick-time claims are deliberately limited.** Ferrite's `/mspt` reads the coordinator/global
  tick (`0.02–0.04 ms` in these runs), while the JVM plugin measures region/server tick time.
  Those are different quantities, so the table does not put them in the same row. The
  comparable Ferrite figure is the spawn region's `/tps`: `16.6` at 200 bots and `12.3` at 400,
  while global TPS stayed `20.0`. Leaf and Purpur held `20.0` region/server TPS at 400, so
  **this run does not claim a Ferrite tick-time win at 400 bots**; it shows a large join-latency
  and RSS advantage alongside a busy spawn region.
- **TPS is the server's own 1-minute figure.** For the JVM servers it is `getTPS()[0]`, and for
  Ferrite it is the `/tps` line for `Region 0`. It still includes the join storm, which is why
  Paper reads `10.6` at 400 even though its last-10-second mean tick was below 50 ms late in
  the run. The raw last-10-second means are listed separately rather than converted into a
  second TPS number.

## Reproduction notes

The JVM servers were the official 26.3 jars with `online-mode=false`, `white-list=false`,
`allow-nether=false`, `allow-end=false`, `network-compression-threshold=256` and
`sync-chunk-writes=false`. Folia and Leaf needed a first start to create their loader/config
files; the measured run was the second start. The measurement plugin is a single
`JavaPlugin` that schedules a repeating global-region task and writes the CSV above.

Ferrite's world directory was regenerated for each bot count. The JVM world directory was also
fresh for each run. The bots are the repository's external harness, not in-process clients, so
server CPU and RSS do not include the bot processes.

![Ferrite vs Folia, Leaf, Paper, Purpur — 2026-11-23](server-comparison.svg)
