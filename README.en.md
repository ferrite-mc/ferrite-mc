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
| **50-Bot Avg MSPT** *(p99 / TPS)* | **0.35 ms** *(0.40 / 20.0)* | **0.35 ms** *(0.39 / 20.0)* | 12.56 ms *(19.5)* | 6.80 ms *(20.0)* | 5.87 ms *(18.5)* | 5.00 ms *(20.0)* |
| **100-Bot Avg MSPT** *(p99 / TPS)* | **0.42 ms** *(0.57 / 20.0)* | **0.45 ms** *(0.60 / 20.0)* | 19.35 ms *(18.9)* | 14.73 ms *(19.6)* | 11.63 ms *(16.8)* | 15.97 ms *(19.3)* |
| **200-Bot Avg MSPT** *(p99 / TPS)* | **1.20 ms** *(1.57 / 20.0)* | **1.25 ms** *(1.93 / 20.0)* | 24.29 ms *(19.1)* | 30.70 ms *(17.5)* | 28.63 ms *(13.9)* | 23.67 ms *(18.3)* |
| **200-Bot Join Latency `p50` / `p99`** | **44.5 / 52.8 ms** | **44.5 / 52.4 ms** | 9,500 / 15,926 ms | 5,131 / 8,797 ms | 4,596 / 8,573 ms | 4,292 / 7,754 ms |
| **400-Bot Avg MSPT** *(budget ≤ 50 ms)* | **4.92 ms** *(7.37 / 20.0)* | **4.43 ms** *(6.57 / 20.0)* | 31.58 ms† *(298/400 drop)* | 82.67 ms *(13.6 TPS)* | 80.23 ms *(9.8 TPS)* | 92.80 ms *(13.4 TPS)* |
| **400-Bot Join Latency `p50` / `p99`** | **44.9 / 59.0 ms** | **45.0 / 59.8 ms** | *dropped during storm†* | 7,796 / 12,972 ms | 7,205 / 12,742 ms | 8,369 / 12,847 ms |
| **Chunk Streaming (`view=6 / 10 / 16`)** | **153.5 / 154.7 / 138.4/s** | **153.6 / 157.5 / 136.2/s** | 62.6 / 63.9 / 70.7/s | 52.8 / 66.7 / 71.1/s | 56.1 / 67.0 / 69.1/s | 57.8 / 67.8 / 72.2/s |
| **Cold Boot to Listener Ready** | **0.004 s** | **0.004–0.005 s** | 6.98–7.13 s | 7.28–7.63 s | 6.98–7.18 s | 7.24–7.48 s |
| **Idle / 200-Bot Peak `VmRSS`** | **192.9 / 891.2 MiB** | **195.3 / 849.1 MiB** | 1,383.8 / 1,592.2 MiB | 1,298.0 / 1,640.8 MiB | 1,395.9 / 1,682.5 MiB | 1,392.6 / 1,664.8 MiB |

<sub>† Under the 5 ms-stagger 400-bot join storm, Folia 26.3 reached `31.58 ms` avg MSPT (`18.98 TPS`, `464.9%` CPU) at 298 online players before closing client connections; Ferrite, Leaf, Paper, and Purpur completed 400/400.</sub>

- **4.4–4.9 ms MSPT & sub-45 ms join at 400 concurrent players**: Across 20 hotpath optimizations—including coordinator dirty/revision gating, lockless TLS frame coding, 16-shard `ChunkCache` + dedicated `CHUNK_GEN_POOL`, persistent Condvar `TrackingWorkers`, zero-alloc packet/chunk serialization, 4-way DM `EvalCache` + branchless `GRADS` LUT, and bit-packed `u64` lighting queues—Ferrite sustains **1.20 ms avg MSPT (`54.9%` CPU, `891.2 MiB` peak RSS) at 200 bots** and **4.43–4.92 ms avg MSPT (`109.5%–120.7%` CPU, `44.9–45.0 ms` join p50, `948–994 MiB` peak RSS) at 400 bots** — **16.3x–20.9x** faster tick times than Paper (`80.23 ms`), Leaf (`82.67 ms`), and Purpur (`92.80 ms`).
- **Pre-generated disk world (`fixture-world`, no worldgen)**: Ferrite (`anvil`) streams **`962.4 chunks/s`** (`441` chunks in `0.5 s`) and holds **200 bots at `1.19 ms` avg MSPT (`56.0%` CPU, `974.2 MiB` peak)** vs Folia (`65.3/s`, `25.54 ms`), Leaf (`64.3/s`, `101.57 ms`), Purpur (`64.9/s`, `113.00 ms`), and Paper (`64.9/s`, `113.20 ms`).
- **Standalone binary fresh wild (`seed=0`, `view=10`, 200 bots)**: Standalone `ferrite-server` holds **`3.55 ms` 1m avg MSPT** (`3.50 ms` 10s avg, `54.8 ms` join p50, `1,309.3 MiB` peak RSS) vs Paper `84.6 ms` (`9.3 TPS`), Leaf `53.2 ms` (`13.1 TPS`), Purpur `52.8 ms` (`13.1 TPS`), and Folia `21.87 ms` (`20.0 TPS`).
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

### 🎮 Vanilla 26.3 Gameplay & Hotpath Optimizations (`ferrite-entity`, `ferrite-command`, `ferrite-net`)
Implements 62 vanilla mob types (including Piglin Hoglin hunting & dance, Witch raider healing, Goat `#snaps_goat_horn` horn snaps, Fox sneaking exemption & sweet-berry harvesting, Killer Bunny & carrot raid, Mooshroom lightning toggle, Hard spider spawn potions), dispenser/piston-settled golem builds, survival/combat/breeding, villager & wandering-trader trading, redstone/hoppers/pistons, brewing/beacons, Nether & End dragon fights/gateways, 26.3 serverbound play packets, and all 94 vanilla command roots (`91/94` exact tree parity, including `strict` setblock/fill, built-in `vanilla` datapack, `WorldId::Runtime` command context, and `/test create|pos|run*`). Optimized via coordinator batched activity tracking, 16-shard `settings`, `FxMap`/`FxSet` in net/region/entity/redstone, 2x2 `ChunkReadCache`, lockless `SharedFrame`/writer queue buffer reuse, `chunk_gen` TLS corner pooling, `CubicSpline` small-slice linear search, and `chunk_nbt` BTreeMap reuse. Verified against Mojang 26.3 codecs via `tools/wirecheck` (`ok=543 bad=0` live, `ok=144 bad=0` across all 62 mobs). [Command Status →](docs/COMMANDS-STATUS.md)

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
| ✅ **Working (bot & test verified)** | Login, encryption, signed chat, Velocity/Bungee forwarding, region multithreading, `.fworld`/Anvil IO, parallel lighting, pure-Rust worldgen, inventory/crafting/containers/villager trading, 62 mob types/AI/combat, redstone/hoppers/pistons/dispensers/command blocks/brewing/beacons, Nether/End/dragon fight/gateways, 26.3 play packets & all 94 command roots ([`docs/COMMANDS-STATUS.md`](docs/COMMANDS-STATUS.md)) |
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
