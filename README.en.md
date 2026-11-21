<div align="center">

# Ferrite

**A Pure-Rust Minecraft Java 26.3 Server with Folia-Style Regionized Multithreading**  
<sub>Replacing the single-threaded tick bottleneck with a parallel Rust core targeting vanilla 26.3.</sub>

<p>
  <img src="https://img.shields.io/badge/Rust-Edition%202024-f97316?style=flat-square&logo=rust" alt="Rust Edition 2024">
  <img src="https://img.shields.io/badge/Minecraft-26.3%20(Protocol%20777)-22c55e?style=flat-square" alt="Minecraft 26.3">
  <img src="https://img.shields.io/badge/Worldgen-Pure%20Rust-ea580c?style=flat-square" alt="Pure Rust Worldgen">
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

> Measured head-to-head on the same host (`Linux 7.0`, 12-vCPU Intel Xeon Platinum 8358, Java 25 `-Xms1G -Xmx2G`) against the identical `fixture-world` save and Rust protocol bot load (`crates/ferrite-server/examples/bots.rs`).

<p align="center">
  <img src="docs/perf/2026-11-21/server-comparison.svg" alt="Ferrite vs Paper 26.3 vs Vanilla 26.3 Benchmark" width="100%">
</p>

| Metric | Ferrite 26.3 (Anvil) | Ferrite 26.3 (Memory) | Paper 26.3 (No Plugins) | Vanilla 26.3 (Mojang) |
|---|---:|---:|---:|---:|
| **200-Bot Avg MSPT** *(budget 50 ms)* | **2.21 ms** *(p99 6.14 ms)* | **5.50 ms** *(p99 16.64 ms)* | 147.1 ms *(9.3 TPS)* | 110.7 ms *(p99 835.9 ms)* |
| **Chunk Streaming (`view=10`)** | **690.8 chunks/s** *(0.4 s)* | 88.8 chunks/s *(live Rust gen)* | 64.2 chunks/s *(6.2 s)* | 88.8 chunks/s *(3.7 s)* |
| **Cold Boot to Listener Ready** | **0.004 s** | **0.004 s** | 37.24 s | 6.53 s |
| **Idle Memory (`0 bots`, `VmRSS`)** | **40.6 MiB** | **40.0 MiB** | 1,408.6 MiB | 1,030.7 MiB |
| **200-Bot Mean / Peak `VmRSS`** | 1,105 / 2,009 MiB | 1,419 / 2,282 MiB | 1,587 / 1,659 MiB | **1,304 / 1,429 MiB** |
| **200-Bot Avg CPU (12 vCPUs)** | **120.5%** | 600.8% *(live worldgen)* | 538.1% | 388.5% |

- **66x lower tick time at 200 players**: Ferrite (Anvil) holds **2.21 ms avg MSPT** (4.4% of the 50 ms 20 TPS budget), while Paper 26.3 drops to **147.1 ms (9.3 TPS)** and Vanilla 26.3 hits **110.7 ms (p99 835.9 ms)** under the same 200-bot load.
- **10.7x faster chunk streaming**: **690.8 chunks/s** (`view=10`) vs Paper (`64.2/s`) and Vanilla (`88.8/s`). Even `--storage memory` generating brand-new Rust terrain on the fly matches Vanilla disk-backed streaming at **88.8 chunks/s**.
- **800-bot stress test (`examples/bots 800`, `--lighting-pool`)**: `800/800` bots joined and walked cleanly on `fixture` at **10.42 ms avg MSPT** (p99 33.64 ms, `1,620 MiB` RSS, `47.30 MiB/s`).

<p align="center">
  <img src="docs/perf/2026-11-21/ferrite-scaling.svg" alt="Ferrite Subsystem Scaling" width="100%">
</p>

<sub>Full methodology, raw measurements, and notes (Paper and Vanilla disconnected bots on the first `MovePosition` packet with `Invalid move player packet received`; their MSPT/RSS/CPU figures were recorded during the 200-bot join/settle window prior to disconnect) are documented in the [full benchmark report](docs/perf/2026-11-21/server-comparison.md) and [perf baseline](docs/perf/README.md).</sub>

---

## ✨ Core Features

### 🔀 Folia-Style Regionized Multithreading (`ferrite-region`)
Splits active world areas into independent regions ticked in parallel, coordinating region splits/merges and cross-region entity, redstone, and portal handoffs via explicit ownership transfer. Batched chunk-holder signaling cuts 4,096-chunk holder-add latency from `13.00 ms` to `2.64 ms`.

### 🌍 Pure-Rust Parallel Worldgen (`ferrite-worldgen`)
Generates Overworld, Nether, and End terrain, biomes, caves, and structures (including Jigsaw/decorators) natively across CPU cores without any JVM child process (the legacy `ferrite-jvm` host has been removed).

### 💡 ScalableLux-Style Lighting Worker Pool (`ferrite-light`)
Enabled via `--lighting-pool`, `LightPool` uses per-chunk lock tokens in one global `(chunk_x, chunk_z)` order and worker-local light engines for lock-free propagation—scaling **5.23x on 8 workers** across 12 full-height (`-4..=19`) jobs with **0 mismatches** across 201,326,592 fixture block/kind checks. [Lighting Benchmark →](docs/perf/2026-11-21/lighting-bench.md)

### 💾 Multi-Backend Storage & High-Throughput Networking (`ferrite-world`, `ferrite-net`)
- **Three storage modes**: diskless in-memory (`--storage memory`, default), single-blob LZ4 atomic save (`--storage aswm`, `20.21 ms` save / `5.79 ms` cold load for 64 chunks), and 8-region parallel Anvil (`--storage anvil`, `3.01x` flush speedup). [Config Guide →](docs/CONFIG.md)
- **Network pipeline**: `libdeflate` level 1–12 wire compression with incompressible fallback, blocking-pool writer offload for batches ≥ 4 KiB (`5.2x` lower p99 flush latency at 128 connections), per-IP TCP connect token buckets, and chat/command rate limiting. [Offload Design →](docs/NET-WRITER-OFFLOAD.md)

### 🎮 Vanilla 26.3 Gameplay & Commands (`ferrite-entity`, `ferrite-command`)
Implements 41 mob types, survival/combat/breeding, villager trading, wandering traders, redstone/hoppers/pistons, brewing/beacons, Nether & End dragon fights/gateways, and all 94 vanilla command roots (`91/94` exact tree parity). Verified **2,908/2,908** against the official 26.3 codecs via `tools/wirecheck`. [Command Status →](docs/COMMANDS-STATUS.md)

---

## 🚀 Quick Start

Requires **Rust stable** (`rust-toolchain.toml` pins Linux GNU; override with `RUSTUP_TOOLCHAIN=stable` on Windows/macOS), **Python 3**, and **Java 25** (only for the one-time `vanilla/` data extraction). The Windows GUI also uses Microsoft Edge WebView2 Runtime.

```sh
source tools/env.sh                        # Git Bash (PowerShell: . .\tools\env.ps1)
python tools/fetch_vanilla.py              # extract vanilla 26.3 data locally (first run only)
cargo run --release -p ferrite-server -- --headless --storage memory --lighting-pool
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
| ✅ **Working (bot & test verified)** | Login, encryption, signed chat, Velocity/Bungee forwarding, region multithreading, Memory/ASWM/Anvil IO, parallel lighting, pure-Rust worldgen, inventory/crafting/containers/villager trading, 41 mob types/AI/combat, redstone/hoppers/command blocks/brewing/beacons, Nether/End/dragon fight/gateways, 94 command roots ([`docs/COMMANDS-STATUS.md`](docs/COMMANDS-STATUS.md)) |
| 🟡 **Partial** | Piston animation & dragon charge/fireball-breath approximations ([`docs/pistons.md`](docs/pistons.md), [`docs/end-dimension.md`](docs/end-dimension.md)), remaining mob goal-by-goal parity ([`docs/mob-ai-parity.md`](docs/mob-ai-parity.md)) |
| ⛔ **Not yet** | **Real Minecraft 26.3 client visual/UI verification** ([`docs/manual-checklist.md`](docs/manual-checklist.md)) — automated verification uses protocol bots and codec wirechecks. |

All remaining work is tracked exclusively in [`docs/BACKLOG.md`](docs/BACKLOG.md). *(Vanilla-only scope: Bukkit/Paper/Spigot plugin API compatibility is not supported.)*

---

## 📦 Crates

| Crate | Responsibility |
|---|---|
| `ferrite-protocol`, `ferrite-net` | Packet codecs (26.3/777), framing, encryption, compression/offload, rate limits |
| `ferrite-region` | Folia-style regionizer and region schedulers |
| `ferrite-world`, `ferrite-light`, `ferrite-worldgen`, `ferrite-nbt` | Chunks, Anvil/ASWM IO, ScalableLux lighting pool, pure-Rust worldgen, NBT |
| `ferrite-entity`, `ferrite-fluid`, `ferrite-inventory`, `ferrite-item`, `ferrite-recipe`, `ferrite-loot` | Entities/mobs, fluids, menus, items, recipes, loot |
| `ferrite-command`, `ferrite-admin`, `ferrite-profiler` | Command dispatcher, game rules/permissions, `/tps`, `/mspt`, `/profile` |
| `ferrite-data`, `ferrite-text`, `ferrite-math`, `ferrite-config`, `ferrite-console` | Vanilla data tables, text components, math, config, console |
| `ferrite-server` | Binary: CLI/Tauri GUI, config, world loop wiring |

---

## ⚖️ Legal Notice & License

- This repository **does not distribute Mojang code or assets.** Vanilla data is extracted locally from the official server jar into `vanilla/` (gitignored). Minecraft is a trademark of Mojang Studios; Ferrite is unofficial and not affiliated with Mojang or Microsoft.
- **License**: **GPL-3.0-or-later** ([`LICENSE`](LICENSE)). Ferrite adopted GPLv3 to port/reference GPL/LGPL optimizations such as Paper, Folia, and Lithium; attributions are listed in [`NOTICE.md`](NOTICE.md).
