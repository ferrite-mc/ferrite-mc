# Ferrite 26.3 — Comprehensive Benchmark Report (2026-11-25, `091c7c53`)

Measured on `Intel(R) Xeon(R) Platinum 8358 CPU @ 2.60GHz` (12 logical CPUs, 62 GiB RAM, Linux, OpenJDK `25.0.5.1`).
All servers were tested on **Minecraft Java 26.3 (Protocol 777)** with `online-mode = false`, `compression-threshold = 256`, `view-distance = 8` (for the 5-way comparison) or `view-distance = 10` (for the `bots` 50/200/400 E2E suite).

## 1. 5-Way Server Comparison (`seed=0` Wild World, `view-distance=8`, `2026-11-25`)

Targets: **Ferrite** (`fworld` + Rust worldgen), **Pumpkin 0.2.2+26.3-26.60** (Rust), **Folia 26.3** (build 28, `-Xms2G -Xmx4G`), **Paper 26.3** (build 262, `-Xms2G -Xmx4G`), **Vanilla 26.3** (`-Xms2G -Xmx4G`).

| Metric | Ferrite (Rust) | Pumpkin 0.2.2 (Rust) | Folia 26.3 (Java) | Paper 26.3 (Java) | Vanilla 26.3 (Java) |
|---|---:|---:|---:|---:|---:|
| **Cold Boot Ready** | **151.4 ms** | 154.9 ms | 13,809.6 ms | 13,563.9 ms | 9,802.3 ms |
| **Idle RSS** | 201.87 MB | **66.21 MB** | 2,161.52 MB | 2,225.00 MB | 1,738.96 MB |
| **SLP (500 reqs, c=32)** | 4,879.3/s (`p50 4.426 ms`) | **6,134.7/s** (`p50 3.452 ms`) | 3,148.4/s (`p50 7.913 ms`) | 2,986.8/s (`p50 8.873 ms`) | 3,350.0/s (`p50 7.368 ms`) |
| **20 Bots Play Enter (`p50` / `p95`)** | **12.256 / 16.999 ms** | 33.215 / 51.000 ms | 3,033.503 / 3,391.048 ms | 1,655.197 / 1,701.856 ms | 1,908.622 / 1,909.194 ms |
| **20 Bots Peak RSS** | 358.50 MB | **229.88 MB** | 2,466.56 MB | 2,502.74 MB | 1,929.09 MB |
| **50 Bots Play Enter (`p50` / `p99`)** | **22.193 / 33.125 ms** | 74.953 / 294.365 ms | 1,747.447 / 3,233.119 ms | 553.174 / 857.350 ms | 127.265 / 458.399 ms |
| **50 Bots Peak RSS** | 463.59 MB | **396.37 MB** | 2,487.44 MB | 2,548.08 MB | 2,010.25 MB |
| **100 Bots Play Enter (`p50` / `p99`)** | **48.556 / 72.490 ms** *(100/100)* | 114.266 / 201.896 ms *(100/100)* | 3,343.621 / 6,542.279 ms *(100/100)* | 570.340 / 2,671.071 ms *(95/100)* | 383.602 / 386.044 ms *(100/100)* |
| **100 Bots Peak RSS** | **514.05 MB** | 627.56 MB | 2,534.62 MB | 2,627.12 MB | 2,134.41 MB |

## 2. `ferrite-server` E2E Concurrency (`bots` example, `view=10`, `BOTS_LIGHTING_POOL=1`)

| Workload / Bots | Joined / Walked | Join `p50` / `p99` | Avg / `p99` MSPT | RSS | CPU | Throughput |
|---|---:|---:|---:|---:|---:|---:|
| **`superflat` 50 Bots** | 50 / 50 | **44.069 / 45.273 ms** | **0.601 / 0.914 ms** | **154.1 MiB** | 16.0% | 0.58 MiB/s |
| **`fixture` 50 Bots** | 50 / 50 | **44.015 / 45.474 ms** | **0.868 / 8.555 ms** | **262.4 MiB** | 16.0% | 1.15 MiB/s |
| **`superflat` 200 Bots** | 200 / 200 | **44.035 / 49.085 ms** | **1.339 / 2.033 ms** | **199.1 MiB** | 162.7% | 8.29 MiB/s |
| **`fixture` 200 Bots** | 200 / 200 | **44.054 / 53.205 ms** | **1.458 / 9.134 ms** | **362.4 MiB** | 157.4% | 10.32 MiB/s |
| **`superflat` 400 Bots** | 400 / 400 | **44.443 / 51.879 ms** | **2.816 / 9.483 ms** | **317.3 MiB** | 583.8% | 31.67 MiB/s |
| **`fixture` 400 Bots** | 400 / 400 | **44.329 / 63.402 ms** | **2.399 / 14.556 ms** | **504.1 MiB** | 571.9% | 34.99 MiB/s |

## 3. Pure-Rust Worldgen & Engine Microbenchmarks (`worldgen_bench` + `flush_bench`, `2026-11-25`)

- **`worldgen_bench` (`RustGenerator::generate`, 256 Overworld chunks × 5 reps)**:
  - `1T` **315.5/s** (`1.00x`, p50 `2.79 ms`) · `2T` **625.4/s** (`1.98x`, `99.1%` eff.) · `4T` **1,251.9/s** (`3.97x`, `99.2%` eff.) · `6T` **1,845.9/s** (`5.85x`, `97.5%` eff.) · `8T` **1,922.2/s** (`6.09x`) · `12T` **2,109.6/s** (`6.69x`, p50 `4.84 ms`, peak **2,136.7/s**)
  - Dimensions (128 chunks × 3 reps, `12T`): Overworld **1,899.6/s** (`0.526 ms/chunk`), Nether **11,766.3/s** (`0.085 ms/chunk`, peak **12,365.9/s**), The End **21,572.4/s** (`0.046 ms/chunk`, peak **21,605.9/s**)
- **`flush_bench` (64 dirty chunks / 48 worldgen chunks)**:
  - `.fworld FWRD v3 (LZ4)` full save: **3.21 ms** (**2.01x** vs serial Anvil Zlib `6.46 ms`), incremental (4/64 dirty): **0.92 ms**, cold load: **0.13 ms**, full decode: **0.91 ms** (`416,429 B`)
  - 48-chunk 12T throughput: Overworld **2,138.2/s** (`0.47 ms/chunk`), Nether **10,739.6/s** (`0.09 ms/chunk`), The End **20,662.7/s** (`0.05 ms/chunk`)
  - 200-player chunk fan-out (`55,406 B` raw): per-socket zlib `38.49 ms` → `SharedFrame` **0.21 ms** (**186.6x** speedup)
  - `1,000x` `Chunk::recompute_heightmaps`: **14.02 ms** (`14.0 us/chunk`) · `50,000x` `raycast_shapes`: **1.09 ms** (`21.9 ns/ray`)
