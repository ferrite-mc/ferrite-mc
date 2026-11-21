# Ferrite vs Paper 26.3 vs Vanilla 26.3 — Apples-to-Apples Benchmark Report (2026-11-21)

Head-to-head sequential comparison run on the same host (`Linux 7.0 x86_64`, Intel Xeon Platinum 8358, 12 logical CPUs, 62 GiB RAM, Java 25.0.5.1) using identical external Rust bot loads (`crates/ferrite-server/examples/bots.rs` via `BOTS_ADDR=127.0.0.1:<port>`). Each server was started **strictly sequentially one at a time**, given `-Xmx4G` (JVM), and shut down cleanly before starting the next.

![Ferrite vs Paper 26.3 vs Vanilla 26.3](server-comparison.svg)

## 1. Concurrency Scaling (`50–400` Bots) & View-Distance Matrix (`6–16`, `seed=0`)

Every server started from an **empty world directory** (`level-seed=0`) and generated terrain + lighting on demand using all 12 logical CPUs (`threads = 0`, `--lighting-pool`, `BOTS_STILL=1`, `-Xmx4G` on JVM):

![Concurrency and View Distance Scaling](concurrency-and-view-scaling.svg)

| Bots / View | Ferrite 26.3 (`.fworld` Default) | Ferrite 26.3 (`anvil`) | Paper 26.3 *(No Plugins, -Xmx4G)* | Vanilla 26.3 *(-Xmx4G)* |
|---|---:|---:|---:|---:|
| **50 Bots Avg MSPT** *(p99)* | **0.78 ms** *(3.32 ms)* | **0.74 ms** *(1.02 ms)* | 6.53 ms | 9.37 ms *(116.5 ms)* |
| **100 Bots Avg MSPT** *(p99)* | **3.61 ms** *(12.17 ms)* | **3.18 ms** *(11.31 ms)* | 9.13 ms | 15.40 ms *(161.6 ms)* |
| **200 Bots Avg MSPT** *(p99)* | **5.46 ms** *(13.91 ms)* | **5.37 ms** *(15.47 ms)* | 31.10 ms | 16.97 ms *(287.2 ms)* |
| **400 Bots Avg MSPT** *(p99)* | **14.52 ms** *(28.84 ms)* | **16.13 ms** *(35.39 ms)* | 97.03 ms *(10.3 TPS)* | 38.20 ms *(617.4 ms)* |
| **400 Bots Join `p50` / `p99`** | **3,891 / 4,617 ms** | **3,919 / 4,535 ms** | 8,554 / 14,647 ms | 4,782 / 6,640 ms |
| **400 Bots Mean / Peak `VmRSS`** | 1,761 / 2,646 MiB | 1,810 / 2,759 MiB | 1,982 / 2,178 MiB | **1,574 / 1,772 MiB** |
| **Chunk Stream (`view=6`)** | **93.8/s** *(169 in 1.8 s)* | **108.9/s** *(169 in 1.6 s)* | 58.0/s *(213 in 3.7 s)* | 44.2/s *(213 in 4.8 s)* |
| **Chunk Stream (`view=10`)** | **102.8/s** *(441 in 4.3 s)* | **102.9/s** *(441 in 4.3 s)* | 68.5/s *(473 in 6.9 s)* | 59.3/s *(473 in 8.0 s)* |
| **Chunk Stream (`view=16`)** | **93.9/s** *(1089 in 11.6 s)* | **85.9/s** *(1089 in 12.7 s)* | 70.2/s *(1057 in 15.1 s)* | 61.6/s *(1057 in 17.1 s)* |
| **Cold Boot / Idle RSS** | **0.004 s** / **40.3 MiB** | **0.004 s** / **40.3 MiB** | 7.18 s / 1,388.9 MiB | 6.53 s / 1,043.6 MiB |

- **Why Ferrite scales cleanly to 400 bots on a fresh `seed=0` world**: Ferrite spreads parallel Rust worldgen, `LightPool`, and pre-compressed `SharedFrame` chunk fan-out across cores (`872%` CPU at 400 bots), keeping ticks at **14.52 ms avg MSPT (20.0 TPS)** while Paper saturates its main thread at **97.03 ms (10.3 TPS)** and Vanilla suffers **617.4 ms p99** stalls. Peak `VmRSS` retains `mimalloc` arena pages and `send-queue-bytes = 16 MiB` burst buffers instead of JVM stop-the-world GCs.

## 2. Pre-Generated Disk Save (`fixture-world`, No Worldgen)

When loading identical pre-generated `vanilla/26.3/fixture-world/world` saves (`-Xmx2G` on JVM):

| Metric | Ferrite 26.3 (`anvil`) | Paper 26.3 *(No Plugins)* | Vanilla 26.3 |
|---|---:|---:|---:|
| **Chunk Stream (`view=10`)** | **690.8 chunks/s** *(289 in 0.4 s)* | 64.2 chunks/s *(401 in 6.2 s)* | 88.8 chunks/s *(329 in 3.7 s)* |
| **200-Bot Avg MSPT** | **2.21 ms** *(p99 6.14 ms, 120.5% CPU)* | 147.1 ms *(9.3 TPS, 538% CPU)* | 110.7 ms *(p99 835.9 ms, 389% CPU)* |

## 3. Ferrite `.fworld` (`FWRD` v3) Storage & Subsystem Scaling (2026-11-21)

![Ferrite Subsystem Scaling](ferrite-scaling.svg)

Component-level release benchmarks on the same 12-CPU Linux host (`flush_bench`, `docs/BACKLOG.md`, `docs/perf/2026-11-21/lighting-bench.md`):

- **`.fworld` (`FWRD` v3) Storage & Compression Matrix (64 Rust-worldgen chunks, `flush_bench`)**:
  - **`.fworld` (`FWRD` v3) LZ4**: **`4.44 ms`** full save (**9.41x** vs serial Anvil `41.77 ms`), **`0.84 ms`** incremental save (4/64 dirty), **`0.10 ms`** cold load (`252,700` bytes).
  - **`.fworld` (`FWRD` v3) Zlib / Gzip / None**: **`4.63 ms`** (`245,418` B) / **`4.65 ms`** (`246,186` B) / **`14.78 ms`** (`8,317,632` B).
  - **Anvil 4-region parallel `flush_all`**: **`16.73 ms`** (Zlib, `2.50x`) / **`5.18 ms`** (LZ4, `8.06x` vs serial Zlib `41.77 ms`).
- **Pure-Rust Worldgen Multi-Core Scaling (`48` chunks, `flush_bench`)**:
  - Overworld (`1 / 2 / 4 / 8 / 12` threads): `78.8 / 150.3 / 294.1 / 421.2 / 471.7 chunks/s` (**5.99x** on 12 threads).
  - 12-thread dimension throughput: Overworld **`455.3/s`**, Nether **`1,693.4/s`**, The End **`3,971.3/s`**.
- **Core Hotpath Microbenchmarks (`flush_bench`)**:
  - **`SharedFrame` 200-player chunk fan-out (`14,356` B raw)**: per-socket zlib `19.52 ms` -> **`0.08 ms` (`239.7x` speedup)**.
  - **`1,000x` `Chunk::recompute_heightmaps` (6 kinds)**: `53.33 ms` (**`53.3 us/chunk`**).
  - **`50,000x` Lazy DDA `raycast_shapes`**: `1.30 ms` (**`26.0 ns/ray`**).
- **Additional Landed Engine Optimizations (2026-11-21)**:
  - **Shared Pre-Compressed Broadcast & Chunk Fan-Out (`Outbound::SharedFrame`)**: `ferrite-net`, `GlobalRegion`, and `AnvilChunkSource::get_frame` compress movement, chat, and `level_chunk_with_light` packets once and share the compressed wire slice across all recipients matching server compression config.
  - **`AnvilChunkSource` `RwLock` + `O(1)` Coordinate Index (`anvil.rs`)**: concurrent read-lock fast path for `block()`, `sky_light()`, `brightness()`, `with_loaded_chunk()`, and cache-hit packet lookups, backed by `by_coord` for guaranteed `O(1)` chunk lookup across biome variants.
  - **Flat-Vector `PalettedContainer` Counts & Fast Heightmap Skip (`ferrite-world`)**: replaced per-section `BTreeMap<u32, usize>` frequency trees with contiguous sorted `Vec<(u32, u16)>` and added empty-section skipping + 6-kind bitmask early exit in `Chunk::recompute_heightmaps`.
  - **Explosion & Interaction Lazy DDA Raycast (`explosion.rs`, `region.rs`, `ferrite-math::raycast_shapes`)**: eliminated per-ray 3D bounding-box `HashMap` allocations in `Explosion.getSeenPercent` and `Player::hit`, and cached voxel states along `0.3`-step blast rays.
  - **Zero-Alloc `no_collision` & Reusable Entity Tracker Scratch (`ferrite-entity`)**: early-exit zero-alloc block collision check, hoisted `HashSet`/`Vec` scratch buffers out of `tracker::track`, and fused `ai::tick_full` mob scans.
  - **Pre-Bound Worldgen Noise Pointers & 2D Climate Column Separation (`ferrite-worldgen`)**: `DensityFunction::bind_noises` pre-resolves `Arc<NormalNoise>` pointers on AST nodes (eliminating runtime `HashMap<String, _>` lookups) and `ClimateSampler::sample_2d_channels_cached` evaluates horizontal climate channels once per `(bx, bz)` column.
- **ScalableLux `LightPool` (`ferrite-light`)**: 12 full-height (`-4..=19`) disjoint chunk relight jobs scaled `1.00x / 1.96x / 2.92x / 3.78x / 5.23x` across `1 / 2 / 3 / 4 / 8` workers (`201,326,592` block checks, 0 mismatches).
- **Outbound Net Writer Offload (`ferrite-net`)**: 128 connections × `4 × 64 KiB / 50 ms` p99 batch flush dropped from `184.70 ms` (inline) to `35.47 ms` (`5.21x` lower).
- **Batched Region Holders (`ferrite-region`)**: 4,096-chunk holder add `13.00 ms -> 2.64 ms` (`4.93x`), remove `7.80 ms -> 0.56 ms` (`14.0x`).
- **800-Bot Stress Test (`examples/bots 800`, `BOTS_LIGHTING_POOL=1`)**: `800/800` bots joined and walked cleanly on `fixture` at **`10.42 ms` avg MSPT** (`33.64 ms` p99, `1,620 MiB` RSS, `47.30 MiB/s`).
