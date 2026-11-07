# Freebuff task 33: Missing Bukkit projectile behavior

Ignore prior worker task instructions and follow only this file. Read `docs/plugin-stubs-status.md`
and inspect existing projectile spawn and entity packet paths before editing.

## Goal

Choose one coherent missing projectile slice from the currently unsupported Bukkit projectile API
(prioritized: snowball, egg, or small fireball), then implement its server-authoritative spawn,
ownership, collision/effect, and client spawn/update path. First prove the chosen path is still
missing; avoid duplicating already working projectile behavior.

## Ownership

Own the projectile-specific entity/model code and its dedicated plugin spawn handler/tests. If a
shared `region.rs` or protocol change is required, keep it minimal and document the exact interface
before expanding scope. Do not change unrelated damage events, entity AI, or inventory paths. No
Mojang code/assets, ASWM, or Rust worldgen.

## Acceptance

- Cover valid/invalid launch parameters, owner attribution, collision/effect behavior, and save/load
  or lifecycle behavior where applicable with deterministic tests.
- Verify packet IDs and layouts against the local 26.3 packet report; add round-trip tests for any
  codec changed.
- Run focused Rust and Gradle checks as applicable, `cargo fmt --all -- --check`, and scoped clippy.
  Do not claim real-client behavior without a client run.
- Commit, clean only this worktree's generated build outputs/caches, report exact checks and any
  unrelated pre-existing changes, then stop.
