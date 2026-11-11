# Freebuff task 36: Structure entities and non-player entity snapshots

Ignore prior worker task instructions and follow only this file. Read
`docs/plugin-api-stubs.md` ("구조물 엔티티 선행 작업 3가지"), `docs/plugin-stubs-status.md`, and
the current structure/entity code before editing. This is one vertical slice, not a mock.

## Goal

Make Bukkit structure entities real end to end:

1. The host can read the UUID, type, position/rotation, validity, and full NBT of non-player
   entities in a bounded world box.
2. `CapturedStructure#fill` captures those entities, `StructureNbt` reads/writes the template
   `entities` list, and `Structure#getEntities`/`getEntityCount` return useful handles.
3. `Structure#place(..., EntityTransformer)` calls the transformer on the owning region and spawns
   the transformed entities, honoring a rejected callback.
4. A structure spanning multiple regions preserves each entity's region ownership and never
   blocks a region thread.

This unblocks the structure entity gap and the wider `World#getEntities*` limitation. Do not
claim the whole Bukkit entity API is done; only the verified slice.

## Current boundary

- `CapturedStructure#getEntities()` is empty and `getEntityCount()` is zero.
- `CapturedStructure#place` throws for a non-empty `EntityTransformer` collection.
- `StructureNbt` knows the template `entities` key exists but does not load or save it.
- `World#getEntities*` and plugin entity queries return online players only; there is no host
  read-only non-player entity snapshot op.
- Existing block capture goes through `WorldView::block_entities` and `plugins/blockdata.rs`; reuse
  its bounded-query and explicit-failure conventions rather than inventing another ad-hoc format.

## Ownership

Rust:

- `crates/ferrite-plugin/src/lib.rs`: add the smallest read-only snapshot/spawn trait operations
  needed by the Java bridge.
- `crates/ferrite-server/src/plugins/snapshot.rs`: serialize/validate entity snapshots.
- `crates/ferrite-server/src/plugins/worldgen.rs` and `plugins.rs`: host routing and world lookup.
- `crates/ferrite-server/src/game/region.rs`: only the region-owned read/spawn hooks and the
  existing cross-region handoff path. Do not rewrite entity AI or the scheduler.
- `crates/ferrite-server/src/game/entity_save.rs`: reuse entity NBT helpers where possible.

Java:

- `CapturedStructure.java`, `StructureNbt.java`, `FerriteStructureManager.java`,
  `FerriteTemplate.java`, and focused tests.
- Add a read-only structure entity handle if `Structure#getEntities()` cannot return a live
  `Entity` handle; it must still implement the Bukkit `Entity` contract it advertises.

Do not implement runtime world creation, general mob ticking, NMS shims, or Rust worldgen.
No Mojang code/assets. Keep packet changes out of scope unless an existing packet path needs a
single tested field.

## Required design

### 1. Host entity snapshot read

Add a bounded query equivalent to "entities whose bounding positions are inside this box":

- Input: world UUID, min/max block coordinates, maximum chunk/entity limits.
- Output per entity: UUID, entity-type key, position, yaw/pitch, velocity/health when exposed,
  full SNBT, and a valid/removed flag. Player entities are excluded unless an existing API
  explicitly asks for them.
- Only the region owning the entity may read it. A structure crossing regions must collect one
  snapshot per region through the existing mailbox/`Signal`/`RegionData` style handoff; do not
  add a global entity lock and do not read another region's data from its tick thread.
- Return results in a deterministic order (UUID or position) so template saves are reproducible.
- An unloaded/unknown region, unreadable NBT, or exceeded bound must produce an explicit error;
  never silently return an empty list for a failed query.
- Removed entities must be reported as invalid or omitted consistently; document the choice and
  test it.

### 2. Host entity spawn from NBT

Add a spawn operation that takes a world UUID, transformed position/rotation, SNBT, and the
structure's spawn reason or a clearly documented equivalent. It must:

- run on the region that owns the target chunk;
- reject unloaded/unknown worlds and malformed SNBT explicitly;
- preserve UUID/Pos/Motion/Rotation and the rest of the NBT;
- return the spawned entity UUID or a clear failure;
- be idempotent only in the sense required by the structure transaction; a failed `place` must not
  leave a partially spawned entity set without a reported failure.

### 3. Structure capture, save, and read

- `StructureNbt` reads and writes `entities` entries with `pos`, `blockPos`, and `nbt` in the
  current 26.3 template format. Preserve unknown fields on round-trip.
- `CapturedStructure#fill` captures entities once, relative to the structure origin, and stores
  their NBT separately from blocks. Block-only structures remain valid.
- `Structure#getEntities()` returns `List<Entity>`; use a read-only handle backed by the captured
  snapshot when the entity is not live. Reads must work after the source entity is removed.
  Mutating a snapshot-only handle must throw a precise `UnsupportedOperationException`.
- `getEntityCount()` and save/load round-trip must agree with the captured list.

### 4. Placement and `EntityTransformer`

Use the actual Paper signature:
`boolean transform(LimitedRegion region, int x, int y, int z, Entity entity, boolean allowed)`.

- Apply rotation, mirror, integrity, and the structure offset to entity positions and NBT before
  invoking the transformer at the transformed block position.
- Run the callback on the region that owns the target position, with a `LimitedRegion` scoped to
  the structure area. A `false` return skips that entity.
- A callback exception aborts the entity transaction and is reported; do not swallow it.
- If a structure spans regions, batch by owning region and preserve original ordering for tests.
- For a `RegionAccessor` placement, use the same transformed-position rules and fail explicitly
  when entities cannot be spawned into that accessor.

## Acceptance

- Rust tests: bounded query in one region and across two; unloaded/unknown world; malformed NBT;
  removed entity; deterministic order; spawn on the correct region; rejected/mutated transformer;
  entity NBT and UUID round-trip; no blocking region calls (existing scheduler test style).
- Java tests: `fill` captures at least two entity types; `getEntities` survives source removal;
  template save/load keeps `entities`; rotation/mirror changes position and orientation; a
  structure spanning two regions returns both entities; `EntityTransformer` mutation and `false`
  are both covered; failed placement is reported.
- Add a focused fixture using local `vanilla/26.3/generated` data. Do not invent entity NBT fields
  the local data cannot support.
- Run `cargo test -p ferrite-plugin -p ferrite-server`, scoped `cargo clippy -D warnings`,
  `cargo fmt --all -- --check`, and `java/ferrite-api-impl` `./gradlew test`.
- Report exact checks, any real-client-only behavior, and unrelated pre-existing changes. Commit
  only this slice's files if the coordinator's current instruction says to commit; otherwise stop
  with the worktree ready for review.
