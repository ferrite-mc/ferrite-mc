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

> **2026-11-23 20 TPS optimization run**: Ferrite `70facc94` was measured on the same host (12-vCPU Xeon Platinum 8358, 62 GiB RAM, Linux) with an **empty `seed=0` world**, `view-distance=10`, **200/400 bots** at a 5 ms stagger, a 10 s settle and a 30 s walk (`BOTS_STILL=1`). It held **20.0 TPS in spawn `Region 0` at both 200 and 400 bots**, and **20.0 TPS over a 60 s sustained 400-bot run**. JVM comparison columns are the earlier same-day 26.3 runs (Paper 259, Folia 26, Purpur 2660, Leaf 28), not a fresh rerun in this pass.

<p align="center">
  <img src="docs/perf/2026-11-23/spawn-region-20tps.svg" alt="Ferrite 20 TPS spawn region at 200 and 400 bots (2026-11-23)" width="100%">
</p>

| Metric | Ferrite | Folia 26.3 | Leaf 26.3 | Paper 26.3 | Purpur 26.3 |
|---|---:|---:|---:|---:|---:|
| **200-bot join `p50` / `p99`** | **56 / 95 ms** | 11,802 / 24,850 ms | 2,345 / 8,673 ms | 2,568 / 13,037 ms | 4,714 / 10,866 ms |
| **200-bot server TPS** | **20.0** *(spawn region)* | 16.4 | **20.0** | 14.5 | **20.0** |
| **200-bot JVM 10 s tick `avg / p99`** | —* | 30.05 / 70.19 ms | 16.16 / 22.76 ms | 17.56 / 25.59 ms | 29.37 / 66.91 ms |
| **200-bot CPU / peak RSS** | 933% / **824 MiB** | 329% / 1,730 MiB | 366% / 3,666 MiB | 335% / 2,429 MiB | 390% / 1,860 MiB |
| **400-bot join `p50` / `p99`** | **74 / 240 ms** | *262/400, connection closed†* | 8,711 / 19,919 ms | 13,432 / 27,589 ms | 15,620 / 26,902 ms |
| **400-bot server TPS** | **20.0** *(spawn region)* | —† | **20.0** | 10.6 | **20.0** |
| **400-bot JVM 10 s tick `avg / p99`** | —* | —† | 41.62 / 55.36 ms | 45.19 / 62.50 ms | 44.40 / 59.75 ms |
| **400-bot CPU / peak RSS** | 864% / **913 MiB** | 630% / 1,883 MiB† | 459% / 4,547 MiB | 448% / 2,507 MiB | 456% / 2,558 MiB |

<sub>* Ferrite `/mspt` reads the coordinator/global tick (`0.02–0.04 ms`), so it is not put in the same row as JVM region tick time. The comparable Ferrite value is spawn `Region 0` TPS; global TPS stayed `20.0` at both 200 and 400 bots. † Folia closed connections at 262/400; its CPU/RSS cover that partial run. TPS is the server's own 1-minute figure and includes the join storm.</sub>

- **Spawn region now holds 20.0 TPS**: at 400 bots Ferrite's spawn region reported **20.0 TPS** in both the short and 60 s sustained runs. Leaf and Purpur also held `20.0`; Paper reported `10.6`. At 200 bots Ferrite reported `20.0`, versus Leaf/Purpur `20.0`, Folia `16.4` and Paper `14.5`.
- **400/400 completed, with a large join-latency win**: Ferrite finished all 400 joins at `p50 74 ms` (`p99 240 ms`), versus Leaf `8.7 s`, Paper `13.4 s` and Purpur `15.6 s` — roughly **118x / 182x / 211x** lower median join latency. At 200 bots Ferrite was `56 ms` vs Leaf `2.3 s`, Paper `2.6 s`, Purpur `4.7 s` and Folia `11.8 s`.
- **824–913 MiB peak RSS, one third to one fifth of the JVM servers**: at 400 bots Ferrite used `913 MiB`, versus Paper `2,507 MiB`, Purpur `2,558 MiB` and Leaf `4,547 MiB`. At 200 bots Ferrite was also lowest at `824 MiB`.
- **More CPU, not less**: Ferrite used `933%` at 200 bots and `864%` at 400, still far above the completed JVM runs' `448–459%` at 400. This is a throughput-for-latency trade from parallel worldgen, lighting and sessions, not a CPU saving.
- **The 20 TPS fix**: natural spawning now scales the vanilla cap by spawnable chunks divided by `289`, not by every player viewing those chunks. The previous per-player cap allowed 28,000 monsters with 400 bots in one region, which made AI and tracking grow without bound. Fluid and random-tick block updates are also viewer-batched.
- **Single observations; JVM reference is the same-day earlier run**: one run per point and no error bars. JVM columns are reused from the earlier same-host comparison, not rerun in this pass. Folia reached 262 players and then closed connections in the 400-bot run.

<sub>Conditions and per-system costs are in the [20 TPS report](docs/perf/2026-11-23/spawn-region-20tps.md); the JVM source figures are in the [2026-11-23 server comparison](docs/perf/2026-11-23/server-comparison.md). Older 50–400-bot, fixture-world and A/B records remain in the [2026-11-21 report](docs/perf/2026-11-21/server-comparison.md) and [performance baseline](docs/perf/README.md).</sub>


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
