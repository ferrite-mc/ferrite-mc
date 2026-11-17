# Plugin B — Read-only World Slice (Chunks & Entities)

- **Branch**: `yxshadxw/plugin-b-world-reads` (base `f58a210`, `origin/main`)
- **Date**: 2026-11-17

## Implemented Surface

### Changed / Added Files

- `java/ferrite-api-impl/src/main/java/dev/ferrite/api/world/FerriteChunk.java`
- `java/ferrite-api-impl/src/main/java/dev/ferrite/api/world/FerriteWorld.java`
- `java/ferrite-api-impl/src/main/java/dev/ferrite/api/entity/FerriteEntitySnapshot.java`
- `java/ferrite-api-impl/src/test/java/dev/ferrite/api/WorldReadsTest.java`
- `java/fixtures/typical-plugin/src/main/java/dev/ferrite/fixtures/typical/TypicalListener.java`
- `crates/ferrite-server/tests/plugin_b_server_acceptance.rs`
- `docs/handoff/plugin-b-world-reads.md`

### World & Chunk Methods

- `FerriteWorld`:
  - `getChunkAt(int, int)`, `getChunkAt(int, int, boolean)`, `getChunkAt(Block)` (with wrong-world guard)
  - `isChunkLoaded(int, int)`, `isChunkLoaded(Chunk)` — queries `world.chunk_loaded` via `Bootstrap.worldChunkLoaded` when the embedded Rust host is attached, falling back to in-memory chunk tracking in pure-Java unit tests
  - `getLoadedChunks()` — returns loaded `FerriteChunk[]` in in-memory mode; throws `UnsupportedOperationException` (`World#getLoadedChunks`) when attached to the Rust host (no `world.loaded_chunks` op in contract v2)
  - `loadChunk(int, int, boolean)`, `loadChunk(Chunk)` — returns `true` when already resident on the Rust host, throws `UnsupportedOperationException` (`World#loadChunk`) if unloaded on the Rust host (never synchronously blocks another region); supports in-memory load/generate flags in unit tests
  - `isChunkGenerated(int, int)` — returns `true` when loaded on the Rust host, throws `UnsupportedOperationException` (`World#isChunkGenerated`) for unloaded chunks on the Rust host; tracks generated chunks in in-memory mode
  - `isChunkInUse(int, int)`, `getPlayersSeeingChunk(Chunk)`, `getPlayersSeeingChunk(int, int)` — checks loaded state + online players in this world within view distance
- `FerriteChunk` (`org.bukkit.Chunk`):
  - `getX()`, `getZ()`, `getWorld()`, `getBlock(int, int, int)` (0..15 bounds check), `isLoaded()`, `isGenerated()`, `load()`, `load(boolean)`, `isEntitiesLoaded()`, `getEntities()`, `getPlayersSeeingChunk()`, `getLoadLevel()`
  - Unmodelled/mutating methods (`unload`, `setForceLoaded`, `addPluginChunkTicket`, `getChunkSnapshot`, `getTileEntities`, `getStructures`, `getPersistentDataContainer`, etc.) throw `UnsupportedOperationException` loudly.

### World Entity Methods

- `FerriteWorld`:
  - `getEntities()` — returns unmodifiable `List.copyOf(...)` combining online players in the world and registered non-player entity snapshots (`registerEntity` / `unregisterEntity`)
  - `getEntity(UUID)` — checks online players in this world, registered world entities, and `entity.state_get` via `Bootstrap.entityStateGet` (constructing an immutable `FerriteEntitySnapshot` when present in this world; returns `null` for missing or wrong-world UUIDs)
  - `getLivingEntities()`, `getEntitiesByClass(Class<T>)`, `getEntitiesByClass(Class<T>...)`, `getEntitiesByClasses(Class<?>...)`
  - `getNearbyEntities(Location, double, double, double, Predicate)`, `getNearbyEntities(BoundingBox, Predicate)` (with wrong-world guard on `Location`)
- `FerriteEntitySnapshot`:
  - Clean-room immutable proxy over `EntityType.getEntityClass()` (or `LivingEntity` / `Entity`) with defensive copies of `Location` and `Vector`; unmodelled methods throw `UnsupportedOperationException`.

## Verification

- `cd java && JAVA_HOME=/usr/lib/jvm/java-25-openjdk-amd64 gradle --no-daemon --console=plain :ferrite-api-impl:test :fixtures:typical-plugin:jar :fixtures:nms-plugin:jar` → **BUILD SUCCESSFUL** (all `:ferrite-api-impl:test` tests including `WorldReadsTest` passed).
- `source tools/env.sh && cargo fmt --all -- --check` → exit 0.
- `source tools/env.sh && cargo clippy --workspace --all-targets -- -D warnings` → exit 0.
- `source tools/env.sh && cargo test -p ferrite-server --test plugin_b_server_acceptance` → **1 passed, 0 failed, 0 ignored** (5.83s), including `world-reads:loaded=true:inUse=true:seeing=true:entity=true:living=true:nearby=true`.
- `git diff --check` → exit 0.

## Limitations & Blockers

- **Biomes (`getBiome`, `getComputedBiome`)**: Kept throwing `UnsupportedOperationException` because contract v2 (`V2_OPS`) and `world.block_get` do not expose biome data.
- **Host chunk/entity listing ops**: Contract v2 (`docs/plugin-b-host-contract.md`, frozen and not edited) has no `world.loaded_chunks`, `world.chunk_generated`, `world.chunk_load`, or `world.entities_get` op. Accordingly, `getLoadedChunks()` and unloaded `isChunkGenerated()`/`loadChunk()` throw `UnsupportedOperationException` when attached to the Rust host, and `getEntities()`/`getLivingEntities()`/`getNearbyEntities()` on the Rust host only enumerate online players and facade-registered entities (while `getEntity(UUID)` also queries `entity.state_get`).
- **Entity surface**: `FerriteEntitySnapshot` exposes identity, location, velocity, world, type, and health snapshots; mutating or unmodelled entity methods throw `UnsupportedOperationException`. No real Minecraft client rendering was verified.
