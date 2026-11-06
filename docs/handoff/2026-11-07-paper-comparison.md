# Paper / Folia comparison (2026-11-07)

Setup: Windows 11, same machine, superflat/flat world, offline mode, view distance 8, 200 bots
(`crates/ferrite-server/examples/bots.rs` with `BOTS_ADDR`): join, 20 s settle, then each bot
steps up to 1 block every second for 40 s. Paper 26.3 build 244 and Folia 26.3 build 6 with
`-Xms4G -Xmx4G`, Ferrite release build. Driver: `C:\Users\taegy\bench\bench.py` (not in the repo;
it starts the server, runs the bots and samples the process tree's CPU and memory).

| | Ferrite | Paper | Folia |
|---|---|---|---|
| Start until port open | 0.5 s | 6 s | 6 s |
| Idle RSS | 128 MB | 2280 MB | 2264 MB |
| Join of 200 bots (excl. 20 s settle) | ~1.7 s | ~3.5 s | ~13 s |
| CPU while walking (% of one core) | 159 | 211* | 167* |
| Peak RSS | 643 MB | 4014 MB | 4000 MB |
| Bots kicked "Invalid move player packet" | 0 | 51 | 78 |

\* Paper and Folia kick a bot that sends two position packets within one (lagging) server tick;
the kicked bots stop loading them, so their CPU is understated.

Not measured: tick time (MSPT) of Paper/Folia, chunk generation, or a vanilla-generated world.
One run each on a noisy machine; treat differences under ~30% as noise.

## What the comparison found
Ferrite first used 334% CPU (187% of it system time): the connection writer woke for the first
queued packet and wrote it at once, so every tracked entity move cost one syscall per viewer.
The writer now waits 2 ms after the first packet and sends everything that arrived in one write
(`ferrite-net/src/lib.rs`), which cut system time from 187% to about 18% (total ~135-160%).
Costs 2 ms of latency per burst.

## Vanilla-terrain world (worldgen test)
Same bots and settings, but a generated world: Paper/Folia `level-type=minecraft:normal`, seed 0;
Ferrite `generator = "vanilla"` (`heap-mb = 2048`, 4 threads, seed 0), which runs the patched official
jar in a separate JVM. Ferrite needs about 60 s before its first join on a fresh directory (the host
patches and boots the jar); the other two are ready in 6 s.

| | Ferrite (vanilla host) | Paper | Folia |
|---|---|---|---|
| Idle RSS (Ferrite: server + host JVM) | 1285 MB | 2370 MB | 2342 MB |
| Join of 200 bots (excl. 20 s settle) | ~2.2 s | ~4.4 s | ~13.7 s |
| CPU while joining (core-seconds, whole process tree) | 100 | 72 | 78 |
| CPU while walking (% of one core) | 120 | 230* | 183* |
| Peak RSS | 2908 MB | 4166 MB | 4088 MB |
| Bots kicked "Invalid move" | 0 | 174 | 86 |

\* understated for the same reason as above (most Paper bots were kicked during the walk).

The bots only walk about 1 block per second around the spawn, so this measures the spawn-area
chunks (a few hundred, generated once) and not sustained generation. Ferrite spent about 40%
more CPU than Paper on generating them. A real worldgen throughput test needs bots flying
outwards (not written); the earlier measurement of ~32 chunks/s is the vanilla chunk system's
plateau (see the optimisation hand-off).
