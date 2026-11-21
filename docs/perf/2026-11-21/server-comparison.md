# Ferrite vs Paper 26.3 vs Vanilla 26.3 — Benchmark Report (2026-11-21)

Head-to-head comparison run on the same host (`Linux 7.0 x86_64`, Intel Xeon Platinum 8358, 12 logical CPUs, 62 GiB RAM, Java 25.0.5.1) using the identical `vanilla/26.3/fixture-world/world` save (for Anvil-backed servers) and identical external Rust bot load (`crates/ferrite-server/examples/bots.rs` via `BOTS_ADDR=127.0.0.1:<port>`).

![Ferrite vs Paper 26.3 vs Vanilla 26.3](server-comparison.svg)

## Methodology & Fairness

- **Server configurations**:
  - **Ferrite 26.3 (Anvil)**: `target/release/ferrite-server --headless --storage anvil --lighting-pool`, `world` copied from `vanilla/26.3/fixture-world/world`.
  - **Ferrite 26.3 (Memory)**: `target/release/ferrite-server --headless --storage memory --lighting-pool`, generating fresh chunks on the fly with `ferrite-worldgen` (`generator = "rust"`).
  - **Paper 26.3**: `/home/ubuntu/mc-paper/paper.jar` (`-Xms1G -Xmx2G --nogui`), **zero plugins installed**, `world` copied from `vanilla/26.3/fixture-world/world`.
  - **Vanilla 26.3**: `vanilla/26.3/server.jar` (`-Xms1G -Xmx2G --nogui`), `world` copied from `vanilla/26.3/fixture-world/world`.
- **Shared properties**: `online-mode=false`, `enforce-secure-profile=false`, `compression-threshold=256`, `max-players=1000`, `spawn-protection=0`, `sync-chunk-writes=false`, bound to `127.0.0.1`.
- **Workloads**:
  1. **Chunk streaming (`BOTS_CHUNKS=1`, `view-distance=10`, 1 bot)**: measures chunks/sec delivered from spawn until 8 s idle.
  2. **200 concurrent bots (`view-distance=2`, `BOTS_STAGGER_MS=5`, `BOTS_SETTLE_SECS=6`, `BOTS_WALK_SECS=20`)**: measures server MSPT (`/mspt` on Ferrite & Paper, `/tick query` on Vanilla), process `VmRSS` (`/proc/<pid>/status`), and average CPU% across all 12 vCPUs.
     *(Note: Paper and Vanilla disconnected bots on the first `MovePosition` packet with `Invalid move player packet received` after the 6 s join/settle window; MSPT, RSS, and CPU figures for Paper and Vanilla were sampled with all 200 bots joined before that kick.)*

## Results Summary

| Metric | Ferrite 26.3 (Anvil) | Ferrite 26.3 (Memory) | Paper 26.3 (No Plugins) | Vanilla 26.3 (Mojang) |
|---|---:|---:|---:|---:|
| **200-Bot avg MSPT** *(budget 50 ms)* | **2.21 ms** *(p99 6.14 ms)* | **5.50 ms** *(p99 16.64 ms)* | 147.1 ms *(9.3 TPS)* | 110.7 ms *(p99 835.9 ms)* |
| **Chunk Stream (`view=10`)** | **690.8 chunks/s** *(289 in 0.4 s)* | 88.8 chunks/s *(live Rust gen)* | 64.2 chunks/s *(401 in 6.2 s)* | 88.8 chunks/s *(329 in 3.7 s)* |
| **Cold Boot to Ready Listener** | **0.004 s** | **0.004 s** | 37.24 s | 6.53 s |
| **Idle Memory (`0 bots`, `VmRSS`)** | **40.6 MiB** | **40.0 MiB** | 1,408.6 MiB | 1,030.7 MiB |
| **200-Bot Mean / Peak `VmRSS`** | 1,105 / 2,009 MiB | 1,419 / 2,282 MiB | 1,587 / 1,659 MiB | **1,304 / 1,429 MiB** |
| **200-Bot Avg CPU (12 vCPUs)** | **120.5%** | 600.8% *(live worldgen)* | 538.1% | 388.5% |

## Subsystem Scaling (2026-11-21)

![Ferrite Subsystem Scaling](ferrite-scaling.svg)

Component-level release benchmarks measured on the same 12-CPU Linux host (`docs/BACKLOG.md` & `docs/perf/2026-11-21/lighting-bench.md`):

- **ScalableLux `LightPool` (`ferrite-light`)**: 12 full-height (`-4..=19`) disjoint chunk relight jobs scaled `1.00x / 1.96x / 2.92x / 3.78x / 5.23x` across `1 / 2 / 3 / 4 / 8` workers (0 mismatches across 201,326,592 fixture block/kind comparisons).
- **Outbound Net Writer Offload (`ferrite-net`)**: 128 connections × `4 × 64 KiB / 50 ms` p99 batch flush latency dropped from `184.70 ms` (inline) to `35.47 ms` (`5.21x` lower).
- **Parallel Storage IO (`ferrite-world`)**: 64 dirty Rust-worldgen chunks flushed in `20.89 ms` across 4 parallel Anvil regions (`3.01x` vs `62.88 ms` serial) and `20.21 ms` with ASWM LZ4 atomic save (`5.79 ms` cold load).
- **Batched Region Holders (`ferrite-region`)**: 4,096-chunk holder add `13.00 ms -> 2.64 ms` (`4.93x`), remove `7.80 ms -> 0.56 ms` (`14.0x`).
- **800-Bot Stress Test (`examples/bots 800`, `BOTS_LIGHTING_POOL=1`)**: `800/800` bots joined and walked cleanly on `fixture` at `10.42 ms` avg MSPT (`33.64 ms` p99, `1,620 MiB` RSS, `47.30 MiB/s`).
