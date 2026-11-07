# Freebuff task 20: Bukkit world entity queries

Read this directory's `README.md`, `docs/plugin-compat-plan.md`, and `docs/plugin-stubs-status.md`.
Audit the current implementation before coding; do not assume the backlog is current.

## Goal

Implement a complete Bukkit `World` entity-query slice for non-player entities: `getEntities()` and
the supported `getNearbyEntities(...)` overloads should return stable, live wrappers for entities
actually present in the requested world, respect bounds/filter semantics, and avoid duplicates or
stale entities after removal/unload. Support the entity types Ferrite actually models; fail
explicitly for unsupported conversions rather than inventing generic wrappers.

## Ownership

Own the `CraftWorld` entity-query methods, the minimal Java entity wrapper/cache needed by that
surface, focused Java tests, and the narrow HostBridge/Rust query path required to retrieve entity
IDs/types/locations. Do not edit `crates/ferrite-entity`, combat/AI, packet codecs, NMS classes, or
shared event routing. If the query needs a broader Rust entity-index redesign, ask the coordinator.

## Done when

- Tests cover players and multiple non-player types, world filtering, AABB edges, stable wrapper
  identity, removed entities, and unsupported entity kinds.
- `./gradlew test`, focused Rust tests, `cargo fmt --all -- --check`, and scoped clippy pass for
  changed Rust code. Document real-client limits honestly.
- Update `docs/plugin-stubs-status.md`, commit, then clean this worktree's Rust and Gradle outputs
  (`cargo clean`, `./gradlew clean`). Do not remove global or other-worktree caches. Report hash,
  checks, limits, and stop. ASWM and Rust worldgen are excluded.
