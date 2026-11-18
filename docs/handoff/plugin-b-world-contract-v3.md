# Plugin B — Contract v3 Bulk World Reads

- **Branch**: `yxshadxw/plugin-b-world-contract-v3` (base `ee4d7cf`, `origin/main`)
- **Date**: 2026-11-17

## Implemented Surface

### Contract v3 Negotiation (`ferrite-plugin-host`, `ferrite-host`)

- Bumped `CONTRACT_VERSION = 3` with `MIN_CONTRACT_VERSION = 2` in Rust (`crates/ferrite-plugin-host/src/lib.rs`) and Java (`dev.ferrite.host.Bootstrap`, `HostRuntime`).
- `host.info` keeps `capabilities.v2_ops` unchanged (`V2_OPS`) and adds `capabilities.v3_ops` advertising only the v3 ops supported by the installed `HostStateProvider`.
- Added `Bootstrap.hostInfo()`, `Bootstrap.worldChunkStatus(String)`, `Bootstrap.worldLoadedChunks(String)`, and `Bootstrap.worldEntitiesSnapshot(String)`.

### Server Provider (`crates/ferrite-server/src/plugin_host/state.rs`)

- `world.chunk_status`: reads `AnvilChunkSource::chunk_resident`, `AnvilChunkSource::chunk_saved`, and coordinator player snapshots (`DEFAULT_CHUNK_VIEW_DISTANCE = 10`) without loading or generating any chunk.
- `world.loaded_chunks`: reads `AnvilChunkSource::resident_chunks()` sorted by `(x, z)`, bounded to `MAX_LOADED_CHUNKS_SNAPSHOT = 4096` (or smaller caller `limit`, rejecting `limit = 0` with `invalid_request`), returning `total` and `truncated`.
- `world.entities_snapshot`: merges coordinator `LivePlayerState` snapshots and region-published `EntityIndex` entries for the requested world, deduplicated and sorted by UUID, bounded to `MAX_ENTITIES_SNAPSHOT = 1024` (or smaller caller `limit`, rejecting `limit = 0`), returning `total` and `truncated`.
- Never reads another region's live state directly, takes no global world lock, and never blocks synchronously on a region.

### Java Bridge & `FerriteWorld`

- `FerriteWorld` negotiates `capabilities.v3_ops` from `Bootstrap.hostInfo()` and wires:
  - `getLoadedChunks()` -> `world.loaded_chunks`
  - `isChunkGenerated(int, int)` and `isChunkInUse(int, int)` -> `world.chunk_status`
  - `getEntities()`, `getLivingEntities()`, `getEntitiesByClass(...)`, `getEntitiesByClasses(...)`, `getNearbyEntities(...)` -> `world.entities_snapshot`
- `getEntity(UUID)` remains on `entity.state_get`. `loadChunk` remains refused on unloaded host chunks.

## Verification

- `cd java && JAVA_HOME=/usr/lib/jvm/java-25-openjdk-amd64 gradle --no-daemon --console=plain :ferrite-host:test :ferrite-api-impl:test :fixtures:typical-plugin:jar :fixtures:nms-plugin:jar` → **BUILD SUCCESSFUL**
- `source tools/env.sh && cargo test -p ferrite-plugin-host` → **18 unit + 1 jvm_smoke + 1 doctest passed** (1 ignored benchmark)
- `source tools/env.sh && cargo test -p ferrite-server plugin_host::state::tests` → **9 passed, 0 failed**
- `source tools/env.sh && cargo test -p ferrite-server --test plugin_b_server_acceptance` → **1 passed, 0 failed** (6.21s), including `world-reads` and `world-bulk:chunksLoaded=true:hasPlayer=true:hasPig=true:livingHasPig=true:nearbyHasPig=true`
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `git diff --check` → exit 0

## Limitations

- Bulk snapshots are bounded (`4096` chunks, `1024` entities per call); callers exceeding the cap receive `truncated: true`.
- `world.entities_snapshot` omits `passengers`/`vehicle` links, inventory/equipment, and NBT (use `entity.state_get` / `player.state_get` for full per-entity detail).
- `loadChunk` on unloaded host chunks and `getBiome`/`getComputedBiome` remain `UnsupportedOperationException`. No real Minecraft client rendering was verified.
