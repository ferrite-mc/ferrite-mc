# Freebuff task 34: Wandering trader spawn and despawn parity

Ignore prior worker task instructions and follow only this file. Read the remaining-work handoff
and inspect the current trader spawn/AI code and existing regression tests before editing.

## Goal

Complete the remaining wandering-trader lifecycle gap as one coherent slice: obey the
`spawn_wandering_traders` gamerule, enforce the vanilla spawn environment and ground search, and
handle drink-driven invisibility/milk despawn without duplicating or resetting trade state. Verify
which pieces already exist before changing them.

## Ownership

Own wandering-trader-specific spawn/AI logic and its focused tests. Avoid villager trade transaction
code and unrelated entity AI; minimize shared `region.rs` edits to trader call sites only. No Mojang
code/assets, ASWM, or Rust worldgen.

## Acceptance

- Add deterministic tests for gamerule-off, valid/invalid light/biome/ground candidates, spawn
  height/search bounds, and drink-triggered despawn while preserving saved trade state.
- Use current local 26.3 data for rules; do not invent unsupported vanilla details.
- Run focused Rust tests, `cargo fmt --all -- --check`, and scoped clippy. Real-client checks must
  be reported separately.
- Commit, clean only this worktree's generated build outputs/caches, report exact checks and any
  unrelated pre-existing changes, then stop.
