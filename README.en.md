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
  <a href="#-benchmarks-ferrite-vs-paper-vs-vanilla">Benchmarks</a> ·
  <a href="#-core-features">Features</a> ·
  <a href="#-quick-start">Quick Start</a> ·
  <a href="#-implementation-status">Status</a> ·
  <a href="#-crates">Crates</a> ·
  <a href="docs/CONFIG.md">Configuration</a> ·
  <a href="README.md">한국어</a>
</p>

</div>

---

## ⚡ Benchmarks: Ferrite vs Paper vs Vanilla

> **Apples-to-apples comparison**: Executed **strictly sequentially one server at a time** on the same host (`Linux 7.0`, 12-vCPU Intel Xeon Platinum 8358, Java 25 `-Xms1G -Xmx4G`), starting every server from an **empty `seed=0` world** with live terrain/light generation and all **200 bots finishing cleanly (`BOTS_STILL=1`)**.

<p align="center">
  <img src="docs/perf/2026-11-21/server-comparison.svg" alt="Ferrite vs Paper 26.3 vs Vanilla 26.3 Benchmark" width="100%">
</p>

### 1) Identical Fresh Worldgen + 200 Bots (`seed=0`)

| Metric | Ferrite (`.fworld` Default) | Ferrite (`anvil`) | Paper 26.3 *(No Plugins)* | Vanilla 26.3 *(Mojang)* |
|---|---:|---:|---:|---:|
| **200-Bot Avg MSPT** *(budget ≤ 50 ms)* | **5.84 ms** *(p99 16.77 ms)* | **5.53 ms** *(p99 13.49 ms)* | 27.47 ms *(11.2–16.0 TPS)* | 18.33 ms *(p99 110.43 ms)* |
| **Chunk Streaming (`view=10`)** | **100.1 chunks/s** *(2.9 s)* | **100.9 chunks/s** *(2.9 s)* | 64.0 chunks/s *(6.3 s)* | 58.1 chunks/s *(5.7 s)* |
| **200-Bot Join Latency `p99`** | **4,443 ms** | **4,549 ms** | 8,134 ms | 5,596 ms |
| **Cold Boot to Listener Ready** | **0.004 s** | **0.004 s** | 7.08 s | 6.48 s |
| **Idle Memory (`0 bots`, `VmRSS`)** | **40.7 MiB** | **39.8 MiB** | 1,387.0 MiB | 1,026.7 MiB |
| **200-Bot Mean / Peak `VmRSS`** | 1,393 / 2,130 MiB | 1,449 / 2,334 MiB | 1,636 / 1,687 MiB | **1,391 / 1,476 MiB** |
| **200-Bot Avg CPU (12 vCPUs)** | 590.9% | 618.3% | 297.7% | 356.9% |

- **Parallel worldgen while holding 20.0 TPS**: During a 200-bot cold join onto a brand-new `seed=0` world, Ferrite spreads Rust worldgen and `LightPool` across CPU cores—getting all 200 players in **1.8x faster (`4,443–4,549 ms` p99 vs `8,134 ms` on Paper)** while holding **5.53–5.84 ms MSPT** (whereas Paper dipped to **11.2 TPS** and Vanilla spiked to **110.4 ms p99**).
- **Pre-generated disk world (`fixture-world`, no worldgen)**: Ferrite (Anvil) streams **`690.8 chunks/s`** and holds **200 bots at `2.21 ms` avg MSPT (`120.5%` CPU)** vs Paper (`64.2/s`, `147.1 ms`) and Vanilla (`88.8/s`, `110.7 ms`).
- **800-bot stress test (`examples/bots 800`, `--lighting-pool`)**: `800/800` bots joined and walked cleanly on `fixture` at **10.42 ms avg MSPT** (`p99 33.64 ms`, `1,620 MiB` RSS, `47.30 MiB/s`).

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

### 💾 `.fworld` RAM-Blob & Anvil Storage / High-Throughput Net (`ferrite-world`, `ferrite-net`)
- **Persistent storage backends (ephemeral in-memory world mode removed)**:
  - `--storage fworld` (default, `.fworld` / `FWRD` v2): keeps the entire compressed world resident in RAM for lock-free reads/writes and persists atomically with `xxh32` checksum integrity verification, pre-checked decompression size limits, and dirty-skip flushing (`19.05 ms` LZ4 save vs `68.59 ms` serial Anvil — **3.60x** faster, `7.22 ms` cold load for 64 chunks).
  - `--storage anvil`: standard Mojang Anvil (`mca`) with up to 8-region parallel flush (**3.18x** speedup). [Config Guide →](docs/CONFIG.md)
- **Network pipeline**: `libdeflate` level 1–12 wire compression with incompressible fallback, blocking-pool writer offload for batches ≥ 4 KiB (`5.2x` lower p99 flush latency at 128 connections), per-IP TCP connect token buckets, and chat/command rate limiting. [Offload Design →](docs/NET-WRITER-OFFLOAD.md)

### 🎮 Vanilla 26.3 Gameplay & Commands (`ferrite-entity`, `ferrite-command`)
Implements 41 mob types, survival/combat/breeding, villager trading, wandering traders, redstone/hoppers/pistons, brewing/beacons, Nether & End dragon fights/gateways, and all 94 vanilla command roots (`91/94` exact tree parity). Verified **2,908/2,908** against the official 26.3 codecs via `tools/wirecheck`. [Command Status →](docs/COMMANDS-STATUS.md)

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
