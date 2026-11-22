# Ferrite 1.1

> Targets **Minecraft Java 26.3 (protocol 777)** with Folia-style regionized multithreading in pure Rust.

## Highlights

- **20 engine & hotpath optimizations across protocol, worldgen, lighting, region scheduling, and networking**:
  - **Protocol & framing (`ferrite-protocol`)**: Lockless thread-local `FrameEncoder` and cursor-based lazy-compaction `FrameDecoder`, removing heap allocation churn and encoder mutex contention on hot packet loops.
  - **Chunk & NBT serialization (`ferrite-world`, `ferrite-nbt`)**: Direct `encode_chunk_packet_into` streaming without intermediate section `Vec`/NBT compound clones, `as_chunks_mut::<8>()` palette word packing, `write_network_compound_into_skipping`, and `Chunk::compute_heightmaps_from_u16`.
  - **Worldgen & lighting (`ferrite-worldgen`, `ferrite-light`)**: 4-way direct-mapped `EvalCache`, branchless `GRADS` Perlin gradient LUT + `y_scale == 0.0` fast path, and bit-packed `(x, z, y, mask)` `u64` lighting BFS queue avoiding per-voxel `/` and `%`.
  - **16-shard `ChunkCache` & dedicated `CHUNK_GEN_POOL` (`ferrite-server/src/game/anvil.rs`)**: Sharded `RwLock` cache with direct `(x, z)` lookup (dropping `by_coord` and duplicate `packet: Arc<[u8]>`), backed by a dedicated CPU chunk-generation worker pool.
  - **Region, tracker & coordinator overhead (`ferrite-region`, `ferrite-server`)**: Persistent Condvar `TrackingWorkers` pool + zero-alloc `TickInput` bucket recycling, consolidated `ctx.state` / `command_state` locks, hoisted tracker locks, reusable `Region` & `State::scratch` vectors, empty-tick fast paths, `OnceLock` `ChunkBatchStart` frame, `FxMap`/`FxSet` internal maps (`game/fx.rs`), and zero-alloc `waypoints` formatting.
- **Vanilla 26.3 gameplay & protocol parity fixes**:
  - **Piglin Overworld zombification (`ferrite-entity`, `ferrite-server`)**: Converts Piglins after 300 ticks outside `piglin_safe` dimensions into `ZombifiedPiglin` with 200-tick Nausea I, preserving equipment/health/pose and persisting `IsImmuneToZombification` & `TimeInOverworld` (with `IMMUNE_TO_ZOMBIFICATION` metadata index 16).
  - **Hard-difficulty zombie door breaking**: Gates `can_break_doors` strictly on Hard difficulty at spawn/summon (`mob::new_mob_with_difficulty`, `Region::summon_with_difficulty`).
  - **Live world/player state synchronization**: Broadcasts `ChangeDifficulty` (`id=10`), `PlayerAbilities` on join and `/gamemode`, `SetTime` (`id=115`) world-clock state (`overworld`, `the_end`) on join, `/time`, `/weather`, and every 20 ticks, plus `MetaValue::OptionalText` (`CUSTOM_NAME` index 2).
  - **Verified wire parity**: Live release `tools/wirecheck/run.py` passed **`SUMMARY ok=1923 bad=0`** against vanilla 26.3 codecs (`PacketCheck.java` updated with `Registries.WORLD_CLOCK`).
- **Measured performance improvements (12-vCPU Xeon, Java 25 `-Xmx4G`, `BOTS_STILL=1`)**:
  - **Fresh `seed=0` scaling (`.fworld` / `anvil`)**:
    - `100` bots: **`0.42–0.45 ms`** avg MSPT, **`820.6–824.4 MiB`** peak `VmRSS` (`.fworld` down from `0.63 ms` / `1,158.4 MiB`).
    - `200` bots: **`1.20–1.25 ms`** avg MSPT (**`49.3%–54.9%`** CPU), **`44.5 / 52.4–52.8 ms`** join `p50 / p99`, **`849.1–891.2 MiB`** peak `VmRSS` (vs Folia `24.29 ms`, Purpur `23.67 ms`, Paper `28.63 ms`, Leaf `30.70 ms`).
    - `400` bots: **`4.43–4.92 ms`** avg MSPT (`20.0 TPS`), **`44.9–45.0 ms`** join `p50`, **`948.0–993.6 MiB`** peak `VmRSS` — **16.3x–20.9x** faster tick times than Paper (`80.23 ms`), Leaf (`82.67 ms`), and Purpur (`92.80 ms`) while Folia dropped connections at 298/400.
    - Cold chunk stream (`view=6 / 10 / 16`): **`153.5 / 154.7 / 138.4 chunks/s`** (`.fworld`), **`153.6 / 157.5 / 136.2 chunks/s`** (`anvil`) vs `52.8–72.2/s` on JVM forks.
  - **Pre-generated `fixture-world` (`ferrite-anvil`)**: **`962.4 chunks/s`** (`view=10`) and **`1.19 ms`** avg MSPT (`p99 1.25 ms`, `56.0%` CPU, `974.2 MiB` peak) at 200 bots vs Folia `25.54 ms`, Leaf `101.57 ms`, Purpur `113.00 ms`, Paper `113.20 ms`.
  - **`flush_bench` release microbenchmarks**: Overworld worldgen `1T 109.2/s` → `12T 644.9/s` (`5.90x`), Nether `2,932.3/s`, End `9,077.2/s`; `.fworld` v3 LZ4 64-chunk full save `4.08 ms` (incr 4/64 `0.95 ms`); `SharedFrame` 200-player fan-out `0.12 ms` (`187.7x`).

## Known limitations

- Vanilla-only scope: Bukkit/Paper/Spigot plugins are unsupported.
- Real Minecraft 26.3 client visual/UI verification remains open ([`docs/manual-checklist.md`](manual-checklist.md)); automated verification uses protocol bots and `wirecheck`.
- Portable archives do not bundle Mojang data; generate `vanilla/26.3/generated/` locally using Java 25 and `tools/fetch_vanilla.py`.

## Release packages

The portable packages contain the Ferrite server binary, `LICENSE`, `NOTICE.md`, and `START-HERE.txt`. Vanilla-generated data is not bundled; follow `START-HERE.txt` to prepare it locally.
