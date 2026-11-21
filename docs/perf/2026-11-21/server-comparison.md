# Ferrite vs Paper 26.3 vs Vanilla 26.3 — Apples-to-Apples Benchmark Report (2026-11-21)

Head-to-head sequential comparison run on the same host (`Linux 7.0 x86_64`, Intel Xeon Platinum 8358, 12 logical CPUs, 62 GiB RAM, Java 25.0.5.1) using identical external Rust bot loads (`crates/ferrite-server/examples/bots.rs` via `BOTS_ADDR=127.0.0.1:<port>`). Each server was started **strictly sequentially one at a time**, given `-Xmx4G` (JVM), and shut down cleanly before starting the next.

![Ferrite vs Paper 26.3 vs Vanilla 26.3](server-comparison.svg)

## 1. Apples-to-Apples Fresh Worldgen (`seed=0`, 200 Bots Completed Cleanly)

Every server started from an **empty world directory** (`level-seed=0`) and generated terrain + lighting on demand using all 12 logical CPUs (`threads = 0`, `--lighting-pool`, `BOTS_STILL=1` so all 200 bots completed the full 31+ s window cleanly with `0` disconnects):

| Metric | Ferrite 26.3 (`.fworld` Default) | Ferrite 26.3 (`anvil`) | Paper 26.3 *(No Plugins, -Xmx4G)* | Vanilla 26.3 *(-Xmx4G)* |
|---|---:|---:|---:|---:|
| **200-Bot Avg MSPT** *(budget ≤ 50 ms)* | **5.68 ms** *(p99 14.15 ms)* | **5.48 ms** *(p99 16.33 ms)* | 27.47 ms *(11.2–16.0 TPS)* | 18.33 ms *(p99 110.43 ms)* |
| **Chunk Stream (`view=10`, Fresh Gen)** | **112.6 chunks/s** *(2.6 s)* | **104.8 chunks/s** *(2.8 s)* | 64.0 chunks/s *(6.3 s)* | 58.1 chunks/s *(5.7 s)* |
| **200-Bot Join Latency `p99`** | **4,336 ms** | **4,450 ms** | 8,134 ms | 5,596 ms |
| **Cold Boot to Ready Listener** | **0.004 s** | **0.004 s** | 7.08 s | 6.48 s |
| **Idle Memory (`0 bots`, `VmRSS`)** | **40.5 MiB** | **40.2 MiB** | 1,387.0 MiB | 1,026.7 MiB |
| **200-Bot Mean / Peak `VmRSS`** | 1,373 / 1,905 MiB | 1,409 / 2,035 MiB | 1,636 / 1,687 MiB | **1,391 / 1,476 MiB** |
| **200-Bot Avg CPU (12 vCPUs)** | 812.0% | 804.4% | 297.7% | 356.9% |

- **Why Ferrite uses higher CPU during join + higher peak RSS**: During fresh `seed=0` join, Ferrite saturates parallel Rust worldgen + `LightPool` workers across cores so all 200 bots receive generated spawn chunks in `4.43–4.55 s` p99 (vs `8.13 s` Paper), keeping ticks at **5.4–5.8 ms** while Paper dips to **11.2 TPS**. Peak `VmRSS` also retains `mimalloc` arena pages and pre-encoded chunk packet buffers/network send queues (`send-queue-bytes = 16 MiB` in the benchmark config) rather than triggering JVM stop-the-world GCs.

## 2. Pre-Generated Disk Save (`fixture-world`, No Worldgen)

When loading identical pre-generated `vanilla/26.3/fixture-world/world` saves (`-Xmx2G` on JVM):

| Metric | Ferrite 26.3 (`anvil`) | Paper 26.3 *(No Plugins)* | Vanilla 26.3 |
|---|---:|---:|---:|
| **Chunk Stream (`view=10`)** | **690.8 chunks/s** *(289 in 0.4 s)* | 64.2 chunks/s *(401 in 6.2 s)* | 88.8 chunks/s *(329 in 3.7 s)* |
| **200-Bot Avg MSPT** | **2.21 ms** *(p99 6.14 ms, 120.5% CPU)* | 147.1 ms *(9.3 TPS, 538% CPU)* | 110.7 ms *(p99 835.9 ms, 389% CPU)* |

## 3. Ferrite `.fworld` (`FWRD` v3) Storage & Subsystem Scaling (2026-11-21)

![Ferrite Subsystem Scaling](ferrite-scaling.svg)

Component-level release benchmarks on the same 12-CPU Linux host (`flush_bench`, `docs/BACKLOG.md`, `docs/perf/2026-11-21/lighting-bench.md`):

- **`.fworld` (`FWRD` v3) RAM-Blob Storage (`ferrite-world`)**: Proprietary single-blob format with **per-chunk independent compression (`FworldColdStore`)**, multi-core parallel NBT serialization & encode/decode, `xxh32` integrity verification, pre-checked decompression size limits, and zero-recompression clean-chunk flushes. On 64 dirty Rust-worldgen chunks (`flush_bench`):
  - **`.fworld` (`FWRD` v3) LZ4 atomic save**: **`8.84 ms`** (`252,700` bytes — **7.20x** faster than single-region Anvil `63.69 ms`, down from `19.05 ms` in v2)
  - **`.fworld` (`FWRD` v3) LZ4 cold load**: **`0.88 ms`** (64 chunks loaded into RAM — **8.20x** faster than v2 `7.22 ms`)
  - **Anvil 4-region parallel `flush_all`**: **`23.37 ms`** (**2.72x** vs `63.69 ms` serial)
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
