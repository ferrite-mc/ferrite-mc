# Plugin B — Loaded-Chunk Biome Reads

- **Branch**: `yxshadxw/plugin-b-world-biomes` (base `3d7eb846`, `origin/main`)
- **Date**: 2026-11-18

## Implemented Surface

### Host Op (`world.biome_get`)

- **Request**: `{"world": "<dimension key>", "x": <i32>, "y": <i32>, "z": <i32>}` (`deny_unknown_fields`)
- **Response**: `{"world": "<dimension key>", "x": <i32>, "y": <i32>, "z": <i32>, "biome": "<namespaced biome id>"}`
- **Errors**: `invalid_request`, `state_unavailable`, `unknown_world`, `chunk_not_loaded`, `biome_unavailable`, `unsupported`, `shutdown`.
- **Negotiation**: Advertised via `host.info` `capabilities.v3_ops` (`V3_OPS` in `crates/ferrite-plugin-host/src/state.rs`, keeping `CONTRACT_VERSION = 3` and `MIN_CONTRACT_VERSION = 2` so v2/v3 compatibility is preserved).
- **Server Provider (`crates/ferrite-server/src/plugin_host/state.rs`)**:
  - Verifies `source.chunk_resident(x.div_euclid(16), z.div_euclid(16))` first (returning `chunk_not_loaded` without loading or generating any chunk).
  - Reads the 3D 4x4x4 biome palette cell via `AnvilChunkSource::biome_name(x, y, z)` (returning `biome_unavailable` if no biome cell exists at that Y).
- **Java Bridge & Facade**:
  - `Bootstrap.worldBiomeGet(String)` (`java/ferrite-host`)
  - `FerriteWorld#getBiome(int, int)` (queries `y = 64`), `FerriteWorld#getBiome(int, int, int)`, `FerriteWorld#getComputedBiome(int, int, int)`, `FerriteWorld.resolveBiome(String)` (maps namespaced id via Paper's `RegistryKey.BIOME`), and `FerriteChunk#getBiome(int, int, int)` (validates 0..15 chunk-local bounds).
  - Unloaded chunks fail explicitly (`IllegalStateException` / `chunk_not_loaded`) without loading/generating. `setBiome` remains refused (`UnsupportedOperationException`).

## Verification

- `cd java && JAVA_HOME=/usr/lib/jvm/java-25-openjdk-amd64 gradle --no-daemon --console=plain :ferrite-api-impl:test :ferrite-host:test :fixtures:typical-plugin:jar :fixtures:nms-plugin:jar` → **BUILD SUCCESSFUL**
- `source tools/env.sh && cargo test -p ferrite-plugin-host` → **18 unit + 1 jvm_smoke + 1 doctest passed** (1 ignored benchmark)
- `source tools/env.sh && cargo test -p ferrite-server plugin_host::state::tests` → **10 passed, 0 failed**
- `source tools/env.sh && cargo test -p ferrite-server --test plugin_b_server_acceptance` → **1 passed, 0 failed** (6.15s), including `world-reads:...:biome=minecraft:plains`
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `git diff --check` → exit 0

## Limitations

- `AnvilChunkSource::biome_name` indexes chunk sections using Overworld's `y.div_euclid(16) + 4` offset; out-of-range Y on non-flat sources returns `biome_unavailable`, and flat sources report `minecraft:plains` for resident chunks.
- `World#setBiome`, `Chunk#contains(Biome)`, `World#getBiomeProvider`, and `World#locateNearestBiome` remain `UnsupportedOperationException`. No real Minecraft client rendering was verified.
