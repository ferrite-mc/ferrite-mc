# Freebuff task 37: Runtime world lifecycle and `Server#createWorld`

Ignore prior worker task instructions and follow only this file. Read
`docs/plugin-api-stubs.md` (the `Server#createWorld` boundary), `docs/plugin-stubs-status.md`, and
the current world/region code before editing. This is an architecture slice; do not fake a world
view that has no chunk source, storage, scheduler, or host mapping.

## Goal

Implement a real runtime world lifecycle:

- `Server#createWorld(WorldCreator)` creates and registers a new world, or returns the existing
  world for the same loaded key/name.
- `Server#unloadWorld(...)` saves and removes an eligible world, returns `false` for a world held
  by players, tickets, portals, or other live references, and never tears down the default world.
- A created world has its own chunk source/storage, spawn, dimension type, region ownership, and
  plugin host mapping.
- `WorldLoadEvent` and `WorldUnloadEvent` fire at the correct point, once per transition.
- Existing fixed overworld/nether/end worlds keep their current UUIDs, names, and behavior.

This is large because the current server models exactly three dimensions. If the smallest correct
implementation needs a new world key abstraction, introduce it rather than adding a fourth
hard-coded enum branch.

## Current boundary

- Java `FerriteServer#createWorld` returns an already registered world and otherwise throws
  `UnsupportedOperationException`; `unloadWorld` always returns `false`.
- Rust `Dimension` is a three-value enum and `DimensionRegistry` opens all three sources up front.
- Plugin world IDs/names are derived from the enum (`overworld`/`the_nether`/`the_end`), and
  `SourceWorldView` / host validation reject every other UUID.
- Region and portal/transfer paths store `Dimension` directly. A runtime world cannot be represented
  without either a dynamic world registry or a deliberate, documented restriction.

## Ownership

Rust:

- `crates/ferrite-server/src/game/dimension.rs`: world registry/descriptor and source creation.
- `crates/ferrite-server/src/game/anvil.rs`: per-world directory/layout support.
- `crates/ferrite-server/src/plugins.rs`: dynamic `WorldSnapshot`, world lookup, plugin routing.
- `crates/ferrite-server/src/game/region.rs` and `crates/ferrite-region`: only the regionizer
  add/remove and transfer hooks needed for a dynamic world. Keep any shared change small and
  documented; ask before a scheduler rewrite.
- `crates/ferrite-server/src/game/commands_world.rs` / server bootstrap: world list output,
  startup registration, and save/unload entry points as needed.

Java:

- `FerriteServer.java`, `Bootstrap.java`, `FerriteWorld.java`, and focused tests.
- `WorldCreator` fields must be read explicitly; do not ignore environment, seed, type, generator,
  generator settings, structures, or keep-spawn-loaded silently.

Do not implement structure entities, general entity AI, or NMS shims. No Mojang code/assets.
Do not change packet IDs/layouts unless a world registry field genuinely requires it; any codec
change needs a round-trip test.

## Required design

### 1. World descriptor and identity

Introduce a runtime descriptor with at least:

- stable UUID, Bukkit name, namespaced key;
- dimension kind/type (overworld, nether, end) and its wire dimension-type id;
- world directory and region/entity storage roots;
- seed, world type, generator settings, generate-structures, keep-spawn-loaded;
- spawn position and spawn yaw/pitch;
- lifecycle state (`Creating`, `Ready`, `Unloading`, `Unloaded`, `Failed`);
- owning chunk source, terrain generator, and regionizer registration.

Map the Bukkit `WorldCreator.environment()` to exactly one supported dimension type. Preserve the
three vanilla worlds' existing UUIDs and names. New worlds must not reuse a UUID or key.

### 2. Create and register

`Server#createWorld` must:

1. Return the loaded world for the same key/name without side effects.
2. Validate the creator before allocating anything: supported environment/type/generator,
   non-empty name, valid seed/settings, and no conflicting key/name.
3. Create the world directory/storage and `level.dat` (or memory storage only for an explicitly
   supported test/flat mode).
4. Open the chunk source and terrain generator, choose spawn, and register the descriptor.
5. Register the world with the regionizer and plugin host view before any player can transfer to
   it. No player/entity tick may observe a half-created world.
6. Fire `WorldLoadEvent` exactly once after the world is usable, on the server thread/global tick.
7. On any failure, roll back sources, storage handles, regionizer registration, plugin snapshot,
   and events; rethrow a clear exception. Never leave a directory or registry entry behind.

If a feature cannot honestly be supported (for example a custom Java `ChunkGenerator` that has no
host data path), reject it before creating files with an explicit message naming the missing
feature. Do not silently substitute the flat generator or the overworld.

### 3. Unload and reload

`Server#unloadWorld` must:

- return `false` for unknown worlds and for the default world;
- return `false` while any player, active portal transfer, plugin chunk ticket, or other live
  reference holds the world; document the exact retention rule;
- optionally save the world, then unregister the plugin view, remove the regionizer world, stop
  chunk sources, and fire `WorldUnloadEvent` once;
- reject new creation/teleport work against the unloading world;
- leave the directory intact when `save=false`, and make a later create with the same name reload
  saved data when `save=true`;
- be safe to call twice: the second call returns `false` without a second event or panic.

A restart/reload test must prove the world list, UUIDs, spawn, and saved blocks survive a fresh
`Bootstrap` over the same temporary root.

### 4. Folia / ownership contract

- World creation and unload are server-lifecycle operations, not region-tick operations.
- No new lock held while a region ticks; use the existing mailbox/`Signal` handoff to stop accepting
  work in a world before removing it.
- Removing a regionizer world must follow the documented topology-mutation protocol and must not
  leave orphan sections/tasks.
- A transfer/portal into a newly created world must either see the fully registered world or fail;
  no fallback to the overworld.
- Any blocking filesystem/save work must be outside the region tick; report where it runs.

## Acceptance

- Rust: create/load/save/unload/reload with a temp world root; duplicate name/key returns the same
  descriptor; invalid creator leaves no directory; UUID/name uniqueness; unload blocked by a
  player/ticket; unload removes sources and regionizer sections; double unload; event/order test;
  plugin world lookup resolves the new UUID and rejects stale/unknown UUIDs; startup reload.
- Java: `createWorld` forwards and returns a usable `World`; duplicate creator returns the same
  instance; unsupported generator/type fails before creating files; `unloadWorld` false cases and
  successful remove; exactly one `WorldLoadEvent`/`WorldUnloadEvent` per transition; a plugin can
  read the new world's name/key/spawn after `WorldLoadEvent`; existing worlds keep their identity.
- Use only local 26.3 data and temporary directories. No real client is available; say so.
- Run `cargo test -p ferrite-plugin -p ferrite-server` (including plugin lifecycle), scoped
  `cargo clippy -D warnings`, `cargo fmt --all -- --check`, and full
  `java/ferrite-api-impl` `./gradlew test`.
- Report the exact registry/regionizer interface changed, any deferred generator feature, and all
  unrelated pre-existing changes. Commit only this slice if the coordinator asks; otherwise stop
  with a reviewable worktree.
