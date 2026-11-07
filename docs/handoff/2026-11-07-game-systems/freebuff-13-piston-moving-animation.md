# Freebuff task 13: moving piston blocks and client animation

Read `README.md` in this directory and `docs/pistons.md` before editing.

## Target and change

Complete the moving-piston lifecycle so clients see vanilla-style moving blocks and timing while
the server keeps the existing atomic block movement and entity-push behavior. Inspect the local
Minecraft 26.3 data/protocol definitions for packet and block-entity details; do not guess packet
IDs or copy Mojang code/assets.

## Ownership and constraints

Own `crates/ferrite-server/src/game/piston.rs`, piston-specific scheduling and packet emission in
`crates/ferrite-server/src/game/region.rs`, and protocol definitions only if the existing protocol
surface cannot represent the verified 26.3 behavior. Keep edits in the piston path; do not change
leads, vaults, fluids, dimensions, or unrelated region logic. Preserve atomic world updates, block
entity contents, collision pushes, and exactly-once block drops.

## Acceptance

- A moving piston block/entity has explicit source, destination, progress, and completion state;
  retracting and extending cases do not duplicate or lose moved blocks.
- Tests cover progression, completion, interrupted/reversed movement, and overlapping updates.
- `docs/pistons.md` reflects what is implemented and what still needs a real client.
- Run focused piston tests, `cargo fmt --all -- --check`, and scoped clippy. Do not claim the
  animation is visually verified without an actual Minecraft client.
- Commit the work, then run `cargo clean` in this worktree to remove its build cache. Send the
  final report with commit hash and exact checks, then stop. Do not delete shared/global caches.
