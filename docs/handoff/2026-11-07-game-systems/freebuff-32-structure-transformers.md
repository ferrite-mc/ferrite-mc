# Freebuff task 32: Structure block and entity transformers

Ignore prior worker task instructions and follow only this file. Read `docs/plugin-stubs-status.md`
and inspect `CapturedStructure` plus existing structure tests before editing.

## Goal

Implement Bukkit `BlockTransformer` behavior for structure placement with correct transformed block
state and preserved block-entity NBT. Cover supported rotations/mirroring, including stairs, rails,
hinges, and directional properties. Inspect whether `EntityTransformer` can be implemented with
current entity enumeration; if not, leave it explicitly unsupported and document the precise gap.

## Ownership

Own Java structure capture/place and its tests, plus the smallest direct bridge needed for block
state updates. Do not implement general entity enumeration or unrelated world APIs. Avoid changing
shared protocol codecs. No Mojang code/assets, ASWM, or Rust worldgen.

## Acceptance

- Add deterministic golden tests for each supported directional property under rotation and mirror,
  and verify block-entity NBT stays at the transformed position.
- Unsupported transformer paths must fail explicitly rather than silently ignore callbacks.
- Run focused Gradle tests and relevant Rust tests, `cargo fmt --all -- --check`, and scoped clippy.
  State real-client limitations honestly.
- Commit, clean only this worktree's generated build outputs/caches, report exact checks and any
  unrelated pre-existing changes, then stop.
