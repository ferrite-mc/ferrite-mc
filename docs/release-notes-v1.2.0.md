# Ferrite 1.2.0

> Major **Minecraft Java 26.3 (protocol 777)** parity expansion and fresh wild worldgen / streaming performance release.

## Highlights

- **Vanilla 26.3 Protocol & Entity Parity (41 → 62 Mobs)**:
  - Added 21 new Vanilla 26.3 mobs (`Hoglin`, `Zoglin`, `PiglinBrute`, `Vindicator`, `Pillager`, `Evoker`, `Ravager`, `Guardian`, `ElderGuardian`, `Phantom`, `Wither`, `Warden`, `Breeze`, `Creaking`, `Shulker`, `Strider`, `Allay`, `Axolotl`, `Camel`, `Sniffer`, `Armadillo`) with `server-26.3.jar`-verified `SynchedEntityData` indices (`Direction(12)`, `SnifferState(35)`, `ArmadilloState(36)`) and attributes (`ok=144 bad=0` Wirecheck).
  - Wired 10 Play serverbound packets (`BundleItemSelected`, `ChangeGameMode`, `ClientTickEnd`, `ClientInfoPlay` live view-distance clamp, `EditBook`, `PickItemFromBlock`, `PickItemFromEntity`, `PlayerAbilities`, `Punch`, `SetJigsawBlock`).
  - Expanded mob AI & world parity: goat wall-ram horn snap, fox sneaking exemption & sweet-berry harvest, Killer Bunny attacks & carrot raiding, Piglin hoglin hunt & dance celebration, Witch raider healing, Hard-difficulty spider spawn effects, Mooshroom lightning toggle, dispenser pumpkin placement & piston-settled golems.
  - Added strict `/setblock` & `/fill`, `/test` GameTest subcommands (`create`, `pos`, `clear*`, `reset*`, `run*`, `stop`, `locate`), runtime world command routing, and built-in `"vanilla"` datapack protection.
- **Engine, Worldgen & Streaming Optimizations**:
  - Switched `flate2` compression backend to `zlib-rs` (default `level=4`), converted `LightSection::Data` to `Arc<[u8; 2048]>` with shared static `full_sky()`, and replaced `CHUNK_GEN_POOL` mutexes with lock-free `crossbeam::channel` + `FworldStore::has_chunks` lockless empty-world skip.
  - Added Overworld surface-biome R-Tree skip above sea level, incremental linear trilinear stepping, precomputed per-chunk 3D cave lattice grid (`rust_decorate.rs`), batch biome packing (`set_biomes_u32`), `FxSet` chunk tracking maps, 4-chunk `prefetch_batch` with batched stats, immediate join prefetch, and tick-free `ChunkBatchReceived` dispatch.
  - **Fresh Wild Overworld (`seed=0`, `view=10`)**: `187.6 chunks/s` cold stream (`194.9/s` at `view=16`), `0.47 ms` (50 bots) / `1.65 ms` (100 bots) / `4.94 ms` (200 bots) / `15.19 ms` (400 bots) steady-state MSPT, and `762.5 chunks/s` 12-thread Overworld terrain generation (`flush_bench`).
