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

## ⚡ Benchmarks: Ferrite vs Pumpkin · Folia · Paper · Vanilla (2026-11-25)

> **2026-11-25 latest comparison run (`091c7c53`)**: Freshly measured on `Intel(R) Xeon(R) Platinum 8358 @ 2.60GHz` (12 vCPU, 62 GiB RAM, Linux, OpenJDK `25.0.5.1`) under identical **Minecraft Java 26.3 (Protocol 777)** settings (`seed=0` wild world, `view-distance=8`, `compression=256`, `online-mode=false`) across **Ferrite**, **Pumpkin 0.2.2+26.3-26.60**, **Folia 26.3**, **Paper 26.3**, and **Vanilla 26.3**.

### 5-Way Server Comparison (`seed=0` Wild World, `view=8`, `2026-11-25`)

| Metric | Ferrite (Rust) | Pumpkin 0.2.2 (Rust) | Folia 26.3 (Java) | Paper 26.3 (Java) | Vanilla 26.3 (Java) |
|---|---:|---:|---:|---:|---:|
| **Cold Boot Ready** | **`151.4 ms`** | `154.9 ms` | `13,809.6 ms` | `13,563.9 ms` | `9,802.3 ms` |
| **Idle `VmRSS`** | `201.87 MB` | **`66.21 MB`** | `2,161.52 MB` | `2,225.00 MB` | `1,738.96 MB` |
| **SLP (500 reqs, c=32)** | `4,879.3/s` *(p50 4.43 ms)* | **`6,134.7/s`** *(p50 3.45 ms)* | `3,148.4/s` *(p50 7.91 ms)* | `2,986.8/s` *(p50 8.87 ms)* | `3,350.0/s` *(p50 7.37 ms)* |
| **20-Bot Play Enter (`p50` / `p95`)** | **`12.26 / 17.00 ms`** | `33.22 / 51.00 ms` | `3,033.5 / 3,391.0 ms` | `1,655.2 / 1,701.9 ms` | `1,908.6 / 1,909.2 ms` |
| **50-Bot Play Enter (`p50` / `p99`)** | **`22.19 / 33.13 ms`** | `74.95 / 294.37 ms` | `1,747.4 / 3,233.1 ms` | `553.2 / 857.4 ms` | `127.3 / 458.4 ms` |
| **100-Bot Play Enter (`p50` / `p99`)** | **`48.56 / 72.49 ms`** *(100/100)* | `114.27 / 201.90 ms` *(100/100)* | `3,343.6 / 6,542.3 ms` *(100/100)* | `570.3 / 2,671.1 ms` *(95/100)* | `383.6 / 386.0 ms` *(100/100)* |
| **100-Bot Peak `VmRSS`** | **`514.05 MB`** | `627.56 MB` | `2,534.62 MB` | `2,627.12 MB` | `2,134.41 MB` |

### `ferrite-server` E2E Concurrency (`bots`, `view=10`, `--lighting-pool`, `2026-11-25`)

| Workload / Bots | Joined / Walked | Join `p50` / `p99` | Avg / `p99` MSPT | RSS | CPU |
|---|---:|---:|---:|---:|---:|
| **`superflat` 50 Bots** | 50 / 50 | **44.07 / 45.27 ms** | **0.601 / 0.914 ms** | **154.1 MiB** | 16.0% |
| **`fixture` 50 Bots** | 50 / 50 | **44.02 / 45.47 ms** | **0.868 / 8.555 ms** | **262.4 MiB** | 16.0% |
| **`superflat` 200 Bots** | 200 / 200 | **44.04 / 49.09 ms** | **1.339 / 2.033 ms** | **199.1 MiB** | 162.7% |
| **`fixture` 200 Bots** | 200 / 200 | **44.05 / 53.21 ms** | **1.458 / 9.134 ms** | **362.4 MiB** | 157.4% |
| **`superflat` 400 Bots** | 400 / 400 | **44.44 / 51.88 ms** | **2.816 / 9.483 ms** | **317.3 MiB** | 583.8% |
| **`fixture` 400 Bots** | 400 / 400 | **44.33 / 63.40 ms** | **2.399 / 14.556 ms** | **504.1 MiB** | 571.9% |

- **Pure-Rust Worldgen Scaling (`worldgen_bench`, `2026-11-25`)**: Overworld `1T 315.5/s (p50 2.79 ms) → 4T 1,251.9/s (99.2% eff.) → 6T 1,845.9/s (97.5% eff.) → 12T 2,109.6/s (6.69x, peak 2,136.7/s)`, Nether **`11,766.3/s` (peak `12,365.9/s`, `0.085 ms/chunk`)**, End **`21,572.4/s` (`0.046 ms/chunk`, peak `21,605.9/s`)**
- **Engine Hotpaths (`flush_bench`, `2026-11-25`)**: `.fworld` v3 LZ4 64-chunk save **`3.21 ms`** (**2.01x** vs serial Anvil Zlib `6.46 ms`), 200-player chunk fan-out **`0.21 ms`** (**186.6x**), `1,000x` heightmap recompute **`14.02 ms`** (`14.0 us/chunk`), `50,000x` raycast **`1.09 ms`** (`21.9 ns/ray`)
- <sub>Full report: [2026-11-25 Benchmark Report](docs/perf/2026-11-25/README.md) · Historical: [2026-11-24 Report](docs/perf/2026-11-24/README.md) · [2026-11-23 Report](docs/perf/2026-11-23/server-comparison.md)</sub>

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
- **Network pipeline**: `libdeflate` level 1–12 wire compression with incompressible fallback, **shared pre-compressed broadcast & chunk packet fan-out (`Outbound::SharedFrame`)**, **thread-local packet serialization scratch**, **lightweight `GlobalRegion` `TickPlayer` & `AddEntity` spawn cache**, blocking-pool writer offload for batches ≥ 4 KiB (`5.2x` lower p99 flush latency at 128 connections), per-IP TCP connect token buckets, chat/command rate limiting, 30 s play keepalive and configuration read timeouts, and a pooled Mojang session client (5 s connect / 10 s total timeout). [Offload Design →](docs/NET-WRITER-OFFLOAD.md)

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
