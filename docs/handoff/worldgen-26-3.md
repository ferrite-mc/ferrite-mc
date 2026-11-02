# World generation on Minecraft 26.3

The 26.3 server jar and generated data use updated names and structures:

- `NoiseRouter.chunkSurfaceLevel()` replaces the older brief's `initial_density_without_jaggedness` differential target. It references `minecraft:overworld/chunk_surface_level`, which interpolates the preliminary surface-level function.
- `NoiseBasedChunkGenerator.buildTerrain` is the 26.3 entry point named in place of the older `doFill` API. The overworld final-density data explicitly contains a 4-by-8 `interpolated` node; interpolation belongs to the density graph.
- Noise settings reference `material_rule`, and 26.3 uses the separate `worldgen/material_rule` and `worldgen/material_condition` registries instead of an inline `surface_rule`.
- `DensitySampler.sampleValue` returns a Java `float`; a faithful evaluator needs to preserve vanilla's intermediate float arithmetic.

These observations were verified against `vanilla/26.3/lib/server-26.3.jar` with `javap` and the generated overworld JSON. Terrain generation is now implemented: `NoiseChunkGenerator` (noise fill, aquifers, carving), `SurfaceGenerator` (material rules) and `FeatureGenerator` (placed features for ores and ground vegetation) are wired into the server's `VanillaTerrain` worker pool (`crates/ferrite-server/src/game/terrain.rs`), with validation in `tests/noise_chunk.rs`, `tests/surface.rs`, `tests/features.rs` and the Java harnesses under `tests/java/`. Trees and structures remain open (`docs/BACKLOG.md`).
- The retired `weird_scaled_sampler` and its rarity mappers are absent from the 26.3 density-function classes and generated overworld data; 26.3 uses its current density nodes instead.

## Noise and carving validation

The fixture seed is stored in `world/data/minecraft/world_gen_settings.dat` in 26.3, rather than `level.dat`. The generated density fill, aquifer, and carving mask match 186,043 of 186,368 blocks below y=40 (99.82561%) in the seven systematically sampled chunks without structure references. Across all 16 sampled chunks the match is 418,991 of 425,984 blocks (98.35839%); trial chambers, mineshafts, and other structures alter saved terrain after this generator's scope. The test reports both values and applies the 99% threshold only to chunks with no structure references.

Carving masks match Java reference vectors for all 16 sampled chunks, and aquifer substance values match 2,000 sampled Java vectors. Surface rules run as a separate stage and are validated by `tests/surface.rs` (≥98% top-block agreement); structure generation is not implemented yet (`docs/BACKLOG.md`).
