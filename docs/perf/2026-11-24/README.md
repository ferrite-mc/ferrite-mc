# Ferrite vs Folia · Leaf · Paper · Purpur 26.3 — Chunk Gen, Worldgen & Concurrency Report (2026-11-24)

Head-to-head sequential comparison run on the same host (`Linux 7.0 x86_64`, Intel Xeon Platinum 8358, 12 logical CPUs, 62 GiB RAM, Java 25.0.5.1 `-Xms1G -Xmx4G`) using the updated release binary (including per-chunk viewer visibility bitsets in `ferrite-entity`, per-chunk batched viewer caching + batch-deferred `compare_set_block_deferred`/`reencode_dirty_chunks` in `RegionFluidWorld`/`AnvilChunkSource`, lock-free parallel `encode_column_packet` in `relight_batch`, write-generation direct `Arc<Chunk>` sampling in `RandomTicks::tick`, and snapshot `seen_by` elision in `EntityIndex`).

![Ferrite vs Folia, Leaf, Paper, Purpur 26.3 Chunk Gen & Worldgen Comparison](detailed-comparison.svg)

## 1. Fresh Worldgen Chunk Streaming (`seed=0`, `view=6 / 10 / 16`)

Cold empty world (`level-seed=0`, `threads = 0`, `--lighting-pool`, matching `BOTS_VIEW` per run):

| Metric | Ferrite (`.fworld`) | Ferrite (`anvil`) | Folia 26.3 | Leaf 26.3 | Paper 26.3 | Purpur 26.3 |
|---|---:|---:|---:|---:|---:|---:|
| **Cold Boot (`view=6 / 10 / 16`)** | **0.004 s** | **0.004 s** | 9.41–9.45 s | 10.10–10.32 s | 9.76–9.77 s | 9.99–10.18 s |
| **Idle RSS (`0 bots`)** | **16.8–18.1 MiB** | **16.9–17.6 MiB** | 1,392.5–1,413.0 MiB | 1,325.1–1,366.1 MiB | 1,423.8–1,443.4 MiB | 1,416.1–1,453.8 MiB |
| **Chunk Stream (`view=6`)** | **170.3/s** *(169 in 1.00s)* | **162.6/s** *(169 in 1.00s)* | 56.6/s *(213 in 3.80s)* | 52.9/s *(213 in 4.00s)* | 55.9/s *(213 in 3.80s)* | 58.0/s *(213 in 3.70s)* |
| **Chunk Stream (`view=10`)** | **180.8/s** *(441 in 2.40s)* | **192.6/s** *(441 in 2.30s)* | 65.2/s *(473 in 7.30s)* | 63.1/s *(473 in 7.50s)* | 63.6/s *(473 in 7.40s)* | 64.7/s *(473 in 7.30s)* |
| **Chunk Stream (`view=16`)** | **183.6/s** *(1,089 in 5.90s)* | **191.5/s** *(1,089 in 5.70s)* | 70.3/s *(1,057 in 15.00s)* | 70.3/s *(1,057 in 15.00s)* | 70.3/s *(1,057 in 15.00s)* | 70.3/s *(1,057 in 15.00s)* |

## 2. Pre-Generated Disk Save (`fixture-world`, `view=10`)

| Metric | Ferrite (`anvil`) | Folia 26.3 | Leaf 26.3 | Paper 26.3 | Purpur 26.3 |
|---|---:|---:|---:|---:|---:|
| **Cold Boot** | **0.004 s** | 38.43–38.51 s | 10.92–10.95 s | 40.35–40.53 s | 40.94–41.04 s |
| **Chunk Stream (`view=10`)** | **1,082.5/s** *(441 in 0.40s)* | 65.6/s *(473 in 7.20s)* | 66.1/s *(473 in 7.20s)* | 64.3/s *(473 in 7.40s)* | 65.3/s *(473 in 7.20s)* |
| **200-Bot Join `p50` / `p99`** | **57.7 / 146.4 ms** | 8,050.3 / 15,234.6 ms | 8,053.3 / 9,466.6 ms | 13,756.3 / 13,984.7 ms | 10,068.4 / 10,556.6 ms |
| **200-Bot Peak `VmRSS`** | 2,023.3 MiB | **1,667.1 MiB** | 2,675.4 MiB | 3,709.6 MiB | 1,916.8 MiB |
| **200-Bot Avg CPU** | 885.9% | 285.8% | 531.4% | 573.8% | 485.1% |

## 3. Fresh Worldgen Concurrency (`50–400` Bots, `seed=0`, `view=10`)

| Workload / Server | Joined | Join `p50` / `p99` | Region 0 TPS *(5s / 1m)* | Peak `VmRSS` | Avg CPU |
|---|---:|---:|---:|---:|---:|
| **50 Bots — Ferrite (`.fworld`)** | 50 / 50 | **64.9 / 195.6 ms** | **20.0 / 20.0** | **1,075.9 MiB** | 948.5% |
| **100 Bots — Ferrite (`.fworld`)** | 100 / 100 | **59.0 / 226.3 ms** | **20.0 / 20.0** | **1,132.1 MiB** | 933.5% |
| **200 Bots — Ferrite (`.fworld`)** | 200 / 200 | **58.3 / 191.6 ms** | **20.0 / 20.0** | 1,353.6 MiB | 936.6% |
| **200 Bots — Ferrite (`anvil`)** | 200 / 200 | **58.8 / 206.6 ms** | **20.0 / 20.0** | **1,288.6 MiB** | 928.8% |
| 200 Bots — Leaf 26.3 | 200 / 200 | 2,237.1 / 8,401.3 ms | — | 3,748.1 MiB | 395.3% |
| 200 Bots — Paper 26.3 | 200 / 200 | 2,313.5 / 21,440.7 ms | — | 4,263.9 MiB | 494.5% |
| 200 Bots — Purpur 26.3 | 200 / 200 | 2,751.5 / 13,716.5 ms | — | 1,865.6 MiB | 368.3% |
| 200 Bots — Folia 26.3 | 200 / 200 | 10,994.0 / 19,287.0 ms | — | 1,679.9 MiB | 333.8% |
| **400 Bots — Ferrite (`.fworld`)** | **400 / 400** | **69.7 / 180.9 ms** | **20.0 / 20.0** | 1,614.4 MiB | 911.9% |
| **400 Bots — Ferrite (`anvil`)** | **400 / 400** | **75.6 / 193.8 ms** | **20.0 / 20.0** | **1,500.3 MiB** | 932.3% |
| 400 Bots — Leaf 26.3 | 400 / 400 | 9,036.1 / 22,288.0 ms | — | 3,567.8 MiB | 458.4% |
| 400 Bots — Purpur 26.3 | 400 / 400 | 11,128.8 / 23,522.8 ms | — | 2,977.4 MiB | 472.1% |
| 400 Bots — Paper 26.3 | *0 / 400†* | *—†* | — | 4,236.5 MiB† | 569.5%† |
| 400 Bots — Folia 26.3 | *0 / 400†* | *—†* | — | 1,874.5 MiB† | 401.0%† |

† Under the 5 ms-staggered 400-bot `BOTS_VIEW=10` storm on fresh worldgen, Paper 26.3 and Folia 26.3 did not complete the 400-bot join within the 180 s window (`0/400` recorded).

## 4. Pure-Rust Worldgen & Engine Microbenchmarks (`worldgen_bench` + `flush_bench`)

- **`worldgen_bench` (256 Overworld chunks × 5 reps, `RustGenerator::generate`)**:
  - `1T` **`125.8/s`** (`1.00x`, p50 `7.66 ms`) · `2T` **`248.8/s`** (`1.98x`, `98.9%` eff.) · `4T` **`495.8/s`** (`3.94x`, `98.5%` eff.) · `6T` **`734.8/s`** (`5.84x`, `97.3%` eff.) · `8T` **`749.5/s`** (`5.96x`) · `12T` **`780.9/s`** (`6.21x`, `1.28 ms/chunk`)
  - Dimensions (128 chunks × 3 reps, `12T`): Overworld **`780.2/s`** (`1.282 ms/chunk`), Nether **`4,727.7/s`** (`0.212 ms/chunk`), The End **`9,833.3/s`** (`0.102 ms/chunk`)
- **`flush_bench` (64 dirty chunks / 48 worldgen chunks)**:
  - `.fworld FWRD v3 (LZ4)` full save: **`3.91 ms`** (**8.52x** vs serial Anvil Zlib `33.35 ms`), incremental (4/64 dirty): **`0.88 ms`**, cold load: **`0.09 ms`**
  - 48-chunk 12T throughput: Overworld **`861.2/s`**, Nether **`4,739.1/s`**, The End **`10,100.2/s`**
  - 200-player chunk fan-out (`18,485 B` raw): `22.29 ms` → `SharedFrame` **`0.11 ms`** (**198.8x** speedup)
  - `1,000x` `Chunk::recompute_heightmaps`: **`52.18 ms`** (`52.2 us/chunk`) · `50,000x` `raycast_shapes`: **`1.09 ms`** (`21.7 ns/ray`)
