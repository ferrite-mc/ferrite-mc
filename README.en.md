<div align="center">

# Ferrite

**A Pure-Rust Minecraft Java 26.3 Server with Folia-Style Regionized Multithreading**  
<sub>Replacing the single-threaded tick bottleneck with a parallel Rust core targeting vanilla 26.3.</sub>

<p>
  <img src="https://img.shields.io/badge/Rust-Edition%202024-f97316?style=flat-square&logo=rust" alt="Rust Edition 2024">
  <img src="https://img.shields.io/badge/Minecraft-26.3%20(Protocol%20777)-22c55e?style=flat-square" alt="Minecraft 26.3">
  <img src="https://img.shields.io/badge/Worldgen-Pure%20Rust-ea580c?style=flat-square" alt="Pure Rust Worldgen">
  <img src="https://img.shields.io/badge/Storage-.fworld%20RAM--Blob-f59e0b?style=flat-square" alt=".fworld Storage">
  <img src="https://img.shields.io/badge/License-GPL--3.0--or--later-3b82f6?style=flat-square" alt="GPL-3.0-or-later">
</p>

<p>
  <a href="#-benchmarks-ferrite-vs-folia--paper--purpur--leaf">Benchmarks</a> ·
  <a href="#-core-features">Features</a> ·
  <a href="#-quick-start">Quick Start</a> ·
  <a href="#-implementation-status">Status</a> ·
  <a href="#-crates">Crates</a> ·
  <a href="docs/CONFIG.md">Configuration</a> ·
  <a href="README.md">한국어</a>
</p>

</div>

---

## ⚡ Benchmarks: Ferrite vs Folia · Paper · Purpur · Leaf

> **Apples-to-apples comparison**: Executed **strictly sequentially one server at a time** on the same host (`Linux 7.0`, 12-vCPU Intel Xeon Platinum 8358, Java 25 `-Xms1G -Xmx4G`), comparing **Ferrite** against **4 Minecraft 26.3 server implementations** (Folia, Leaf, Paper, Purpur) across both an **empty `seed=0` world** (`50–400` bots, `BOTS_STILL=1`) and a **pre-generated `fixture-world`**.

<p align="center">
  <img src="docs/perf/2026-11-21/server-comparison.svg" alt="Ferrite vs Folia, Leaf, Paper, Purpur 26.3 Benchmark" width="100%">
</p>

### 1) Identical Fresh Worldgen Scaling (`50–400` Bots & View Distance `6–16`, `seed=0`)

| Metric | Ferrite (`.fworld`) | Ferrite (`anvil`) | Folia 26.3 | Leaf 26.3 | Paper 26.3 | Purpur 26.3 |
|---|---:|---:|---:|---:|---:|---:|
| **50-Bot Avg MSPT** *(p99 / TPS)* | **0.35 ms** *(0.38 / 20.0)* | **0.35 ms** *(0.41 / 20.0)* | 12.61 ms *(19.1)* | 5.57 ms *(20.0)* | 5.43 ms *(18.3)* | 8.17 ms *(20.1)* |
| **100-Bot Avg MSPT** *(p99 / TPS)* | **0.63 ms** *(0.79 / 20.0)* | **0.63 ms** *(0.90 / 20.0)* | 18.52 ms *(19.2)* | 12.57 ms *(19.9)* | 10.93 ms *(16.4)* | 11.93 ms *(19.6)* |
| **200-Bot Avg MSPT** *(p99 / TPS)* | **1.54 ms** *(1.84 / 20.0)* | **1.56 ms** *(1.90 / 20.0)* | 24.55 ms *(18.8)* | 26.00 ms *(18.4)* | 24.70 ms *(13.6)* | 28.37 ms *(17.8)* |
| **200-Bot Join Latency `p50` / `p99`** | **44.7 / 58.1 ms** | **44.9 / 57.9 ms** | 9,270 / 15,279 ms | 5,271 / 8,784 ms | 4,646 / 7,652 ms | 4,909 / 8,388 ms |
| **400-Bot Avg MSPT** *(budget ≤ 50 ms)* | **5.13 ms** *(6.95 / 20.0)* | **5.23 ms** *(9.10 / 20.0)* | 33.98 ms† *(291/400 drop)* | 69.87 ms *(16.1 TPS)* | 77.00 ms *(10.2 TPS)* | 84.00 ms *(13.7 TPS)* |
| **400-Bot Join Latency `p50` / `p99`** | **44.9 / 60.4 ms** | **44.9 / 57.9 ms** | *dropped during storm†* | 7,143 / 11,678 ms | 7,718 / 12,778 ms | 8,429 / 13,420 ms |
| **Chunk Streaming (`view=6 / 10 / 16`)** | **120.5 / 133.6 / 126.4/s** | **125.1 / 137.8 / 131.4/s** | 60.7 / 61.7 / 71.9/s | 54.4 / 63.7 / 69.3/s | 57.6 / 69.1 / 72.0/s | 61.4 / 68.0 / 72.1/s |
| **Cold Boot to Listener Ready** | **0.004 s** | **0.004 s** | 6.88–6.98 s | 7.28–7.59 s | 7.08–7.14 s | 7.23–7.43 s |
| **Idle / 200-Bot Peak `VmRSS`** | **193.3 / 1,150.3 MiB** | **195.0 / 1,138.1 MiB** | 1,391.5 / 1,592.6 MiB | 1,304.5 / 1,773.3 MiB | 1,395.0 / 1,623.5 MiB | 1,363.7 / 1,613.0 MiB |

<sub>† Under the 5 ms-stagger 400-bot join storm, Folia 26.3 reached `33.98 ms` avg MSPT (`18.75 TPS`, `506%` CPU) at 291 online players before closing client connections; Ferrite, Paper, Purpur, and Leaf completed 400/400.</sub>

- **5.1 ms MSPT & sub-45 ms join at 400 concurrent players**: With coordinator dirty/revision gating, cipherless `SharedFrame` inline network batching, non-HoL chunk streaming, flat `u16` worldgen section packing, and empty/uniform-palette light skipping, Ferrite (`.fworld`) sustains **1.54 ms avg MSPT (`68.7%` CPU) at 200 bots** and **5.13 ms avg MSPT (`129.3%` CPU, `44.9 ms` join p50, `1,351.6 MiB` peak RSS) at 400 bots** — **13.6x–18.4x** faster tick times than Folia (`24.55 ms`, `9.27 s` join), Leaf (`69.87 ms`), Paper (`77.00 ms`), and Purpur (`84.00 ms`).
- **Pre-generated disk world (`fixture-world`, no worldgen)**: Ferrite (`anvil`) streams **`968.3 chunks/s`** (`441` chunks in `0.5 s`) and holds **200 bots at `1.67 ms` avg MSPT (`57.8%` CPU, `1,004.8 MiB` peak)** vs Folia (`65.7/s`, `26.84 ms`), Paper (`63.6/s`, `105.93 ms`), Purpur (`65.4/s`, `109.73 ms`), and Leaf (`66.2/s`, `113.87 ms`).
- **800-bot stress test (`examples/bots 800`, `--lighting-pool`)**: `800/800` bots joined and walked cleanly on `fixture` at **10.42 ms avg MSPT** (`p99 33.64 ms`, `1,620 MiB` RSS, `47.30 MiB/s`).

<p align="center">
  <img src="docs/perf/2026-11-21/concurrency-and-view-scaling.svg" alt="Concurrency and View Distance Scaling" width="100%">
</p>
<p align="center">
  <img src="docs/perf/2026-11-21/ferrite-scaling.svg" alt="Ferrite Subsystem Scaling" width="100%">
</p>

<sub>Full methodology, raw measurements, and discussion of CPU parallelism & `mimalloc` / burst-queue peak RSS are documented in the [full benchmark report](docs/perf/2026-11-21/server-comparison.md) and [perf baseline](docs/perf/README.md).</sub>

---

## ✨ Core Features

### 🔀 Folia-Style Regionized Multithreading (`ferrite-region`)
Splits active world areas into independent regions ticked in parallel, coordinating region splits/merges and cross-region entity, redstone, and portal handoffs via explicit ownership transfer. Batched chunk-holder signaling cuts 4,096-chunk holder-add latency from `13.00 ms` to `2.64 ms`.

### 🌍 Pure-Rust Parallel Worldgen (`ferrite-worldgen`)
Generates Overworld, Nether, and End terrain, biomes, caves, and structures (including Jigsaw/decorators) natively across CPU cores without any JVM child process (the legacy `ferrite-jvm` host has been removed).

### 💡 ScalableLux-Style Lighting Worker Pool (`ferrite-light`)
Enabled via `--lighting-pool`, `LightPool` uses per-chunk lock tokens in one global `(chunk_x, chunk_z)` order and worker-local light engines for lock-free propagation—scaling **5.23x on 8 workers** across 12 full-height (`-4..=19`) jobs with **0 mismatches** across 201,326,592 fixture block/kind checks. [Lighting Benchmark →](docs/perf/2026-11-21/lighting-bench.md)

### 💾 `.fworld` (`FWRD` v3) RAM-Blob & Anvil Storage / High-Throughput Net (`ferrite-world`, `ferrite-net`)
- **Persistent storage backends (ephemeral in-memory world mode removed)**:
  - `--storage fworld` (default, `.fworld` / `FWRD` v3): keeps per-chunk compressed records resident in RAM (`FworldColdStore`) with streaming file decoding, skips re-compressing unchanged chunks on flush, and runs multi-core parallel NBT serialization + encode/decode with `xxh32` checksum verification and pre-checked size bounds (**`3.85 ms`** LZ4 save vs `41.18 ms` serial Anvil — **10.71x** faster, **`0.84 ms`** incremental 4/64-chunk save, **`0.08 ms`** cold load).
  - `--storage anvil`: standard Mojang Anvil (`mca`) with up to 8-region parallel flush (**2.62x–7.67x** with LZ4), Copy-on-Write `Arc<Chunk>` chunk snapshots, and `RwLock` + `O(1)` coordinate-indexed chunk caching. [Config Guide →](docs/CONFIG.md)
- **Network pipeline**: `libdeflate` level 1–12 wire compression with incompressible fallback, **shared pre-compressed broadcast & chunk packet fan-out (`Outbound::SharedFrame`)**, **thread-local packet serialization scratch**, **lightweight `GlobalRegion` `TickPlayer` & `AddEntity` spawn cache**, blocking-pool writer offload for batches ≥ 4 KiB (`5.2x` lower p99 flush latency at 128 connections), per-IP TCP connect token buckets, and chat/command rate limiting. [Offload Design →](docs/NET-WRITER-OFFLOAD.md)

### 🎮 Vanilla 26.3 Gameplay & Combat Optimizations (`ferrite-entity`, `ferrite-command`)
Implements 41 mob types, survival/combat/breeding, villager trading, wandering traders, redstone/hoppers/pistons, brewing/beacons, Nether & End dragon fights/gateways, and all 94 vanilla command roots (`91/94` exact tree parity). Includes lazy DDA explosion & interaction raycasting (`raycast_shapes`), zero-alloc `no_collision`, hoisted entity-tracker & AI scratch buffers, per-tick `ChunkReadCache` & single-cache random-tick sampling, flat-vector `PalettedContainer` counts with empty-section heightmap skipping, pre-bound worldgen noise pointers (`bind_noises`), thread-local `EvalCache` pooling, 4x4 surface biome caches, and 2D climate column caching. Verified **2,908/2,908** against the official 26.3 codecs via `tools/wirecheck`. [Command Status →](docs/COMMANDS-STATUS.md)

---

## 🚀 Quick Start

Requires **Rust stable** (`rust-toolchain.toml` pins Linux GNU; override with `RUSTUP_TOOLCHAIN=stable` on Windows/macOS), **Python 3**, and **Java 25** (only for the one-time `vanilla/` data extraction). The Windows GUI also uses Microsoft Edge WebView2 Runtime.

```sh
source tools/env.sh                        # Git Bash (PowerShell: . .\tools\env.ps1)
python tools/fetch_vanilla.py              # extract vanilla 26.3 data locally (first run only)
cargo run --release -p ferrite-server -- --headless --storage fworld --lighting-pool
```

On first run, Ferrite writes `eula.txt` and exits. Read the [Minecraft EULA](https://aka.ms/MinecraftEULA), set `eula=true` yourself if you agree, and start again. Launching `ferrite-server.exe` without `--headless` on Windows opens the Tauri 2 GUI; Linux runs headless on stdin/stdout. [Console & GUI Docs →](docs/windows-console.md)

### Verification

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

---

## 📊 Implementation Status

| Area | Details |
|---|---|
| ✅ **Working (bot & test verified)** | Login, encryption, signed chat, Velocity/Bungee forwarding, region multithreading, `.fworld`/Anvil IO, parallel lighting, pure-Rust worldgen, inventory/crafting/containers/villager trading, 41 mob types/AI/combat, redstone/hoppers/command blocks/brewing/beacons, Nether/End/dragon fight/gateways, 94 command roots ([`docs/COMMANDS-STATUS.md`](docs/COMMANDS-STATUS.md)) |
| 🟡 **Partial** | Piston animation & dragon charge/fireball-breath approximations ([`docs/pistons.md`](docs/pistons.md), [`docs/end-dimension.md`](docs/end-dimension.md)), remaining mob goal-by-goal parity ([`docs/mob-ai-parity.md`](docs/mob-ai-parity.md)) |
| ⛔ **Not yet** | **Real Minecraft 26.3 client visual/UI verification** ([`docs/manual-checklist.md`](docs/manual-checklist.md)) — automated verification uses protocol bots and codec wirechecks. |

All remaining work is tracked exclusively in [`docs/BACKLOG.md`](docs/BACKLOG.md). *(Vanilla-only scope: Bukkit/Paper/Spigot plugin API compatibility is not supported.)*

---

## 📦 Crates

| Crate | Responsibility |
|---|---|
| `ferrite-protocol`, `ferrite-net` | Packet codecs (26.3/777), framing, encryption, compression/offload, rate limits |
| `ferrite-region` | Folia-style regionizer and region schedulers |
| `ferrite-world`, `ferrite-light`, `ferrite-worldgen`, `ferrite-nbt` | Chunks, `.fworld`/Anvil IO, ScalableLux lighting pool, pure-Rust worldgen, NBT |
| `ferrite-entity`, `ferrite-fluid`, `ferrite-inventory`, `ferrite-item`, `ferrite-recipe`, `ferrite-loot` | Entities/mobs, fluids, menus, items, recipes, loot |
| `ferrite-command`, `ferrite-admin`, `ferrite-profiler` | Command dispatcher, game rules/permissions, `/tps`, `/mspt`, `/profile` |
| `ferrite-data`, `ferrite-text`, `ferrite-math`, `ferrite-config`, `ferrite-console` | Vanilla data tables, text components, math, config, console |
| `ferrite-server` | Binary: CLI/Tauri GUI, config, world loop wiring |

---

## ⚖️ Legal Notice & License

- This repository **does not distribute Mojang code or assets.** Vanilla data is extracted locally from the official server jar into `vanilla/` (gitignored). Minecraft is a trademark of Mojang Studios; Ferrite is unofficial and not affiliated with Mojang or Microsoft.
- **License**: **GPL-3.0-or-later** ([`LICENSE`](LICENSE)). Ferrite adopted GPLv3 to port/reference GPL/LGPL optimizations such as Paper, Folia, and Lithium; attributions are listed in [`NOTICE.md`](NOTICE.md).
