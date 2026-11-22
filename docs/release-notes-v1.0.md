# Ferrite 1.0

> Targets **Minecraft Java 26.3 (protocol 777)** with Folia-style regionized multithreading in pure Rust.

## Highlights

- **Pure-Rust parallel worldgen (`ferrite-worldgen`)**: Overworld, Nether, and End terrain, biomes, caves, and Jigsaw structures without a JVM host process — pre-bound noise pointers (`bind_noises`), 2D climate column separation, thread-local `EvalCache` pooling, and 4×4 surface biome lookup caches (`523.0 chunks/s`, `6.83x` on 12 threads in `flush_bench`).
- **`.fworld` (`FWRD` v3) default storage & parallel Anvil (`ferrite-world`, `ferrite-server`)**: Per-chunk compressed cold store with streaming `BufReader` load (`0.08 ms`), multi-core parallel NBT + LZ4 flush (`3.85 ms` vs `41.18 ms` serial Anvil, `10.71x`), `xxh32` integrity checks, and `Arc<Chunk>` Copy-on-Write across chunk caches, relighting, and flush snapshots.
- **ScalableLux-style lighting pool (`ferrite-light`)**: Optional `--lighting-pool` worker pool with lock-free per-worker engines and deterministic chunk ordering (`5.23x` speedup on 8 workers; 0 mismatches across 201,326,592 fixture block checks).
- **Network & tick-loop optimizations**: `Outbound::SharedFrame` pre-compressed chunk/broadcast fan-out (`190.0x` on 200-player chunks), thread-local packet serialization scratch, `GlobalRegion` `TickPlayer` + `AddEntity` spawn cache, `ChunkReadCache` in fluid/redstone/physics/random-tick paths, lazy DDA `raycast_shapes`, zero-alloc `no_collision`, and flat `PalettedContainer` counts.
- **Vanilla 26.3 gameplay completeness**: 41 mob types, villagers and wandering traders, redstone/hoppers/pistons, brewing/beacons, Nether/End portals and dragon fights, 94 vanilla command roots (`91/94` exact tree parity), and `2,908/2,908` `tools/wirecheck` codec parity.
- **Fast join & login pipeline (`ferrite-net`, `ferrite-protocol`, `ferrite-world`, `ferrite-server`)**: Buffered typed JSON `RuntimeRegistry` load (`std::fs::read` + `serde::Deserialize`) shared across generator and storage, cached pre-compressed `VanillaData` configuration bundle (`RegistryData` + `UpdateTags` + `FinishConfiguration` in one write), `PlayerInfoBatch` (`id = 71`) batching in `GlobalRegion::join`, debounced background `usercache.json` flushes, and permission-cached pre-compressed `Commands` frames — cutting 200-bot join latency from ~4.1 s to **47.7 ms p50 (`72.2 ms p99`)**.
- **Verified benchmarks (12-vCPU Xeon, fresh `seed=0`, `--lighting-pool`, `BOTS_STILL=1`)**:
  - **50 / 100 / 200 / 400-Bot Avg MSPT**: **0.58 / 2.94 / 4.68 / 14.03 ms** (20.0 TPS at 400 bots vs Paper 26.3 `97.03 ms` / `10.3 TPS`).
  - **200 / 400-Bot Join Latency `p50` (`p99`)**: **47.7 ms (`72.2 ms`)** / **50.0 ms (`98.3 ms`)** (vs Paper `8,554 / 14,647 ms` at 400 bots).
  - **Chunk streaming (`view=6 / 10 / 16`)**: **109.0 / 108.5 / 93.0 chunks/s**.
  - **Cold boot / Ready RSS**: **0.004 s / 202.4 MiB** (400-bot peak RSS **1,785.6 MiB**).
- **Version metadata alignment**: Sets workspace crate version in `Cargo.toml` to `1.0.0` (so `ferrite-server` and the Windows Tauri GUI report `1.0.0`) and updates `.github/workflows/release.yml` to replace package sections cleanly on re-publish.

## Known limitations

- Vanilla-only scope: Bukkit/Paper/Spigot plugins are unsupported.
- Real Minecraft 26.3 client visual/UI verification remains open ([`docs/manual-checklist.md`](manual-checklist.md)); automated verification uses protocol bots and `wirecheck`.
- Portable archives do not bundle Mojang data; generate `vanilla/26.3/generated/` locally using Java 25 and `tools/fetch_vanilla.py`.

## Release packages

The portable packages contain the Ferrite server binary, `LICENSE`, `NOTICE.md`, and `START-HERE.txt`. Vanilla-generated data is not bundled; follow `START-HERE.txt` to prepare it locally.
