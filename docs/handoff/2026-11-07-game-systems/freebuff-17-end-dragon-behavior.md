# Freebuff task 17: End dragon behavior and fight-state audit

Read this directory's `README.md`, `docs/end-dimension.md`, and existing End implementation before
editing. The respawn sequence has just been changed by another task; inspect current `origin/main`
and avoid undoing or duplicating it.

## Target

Audit dragon behavior against Minecraft 26.3 and complete one coherent missing slice in the
dragon's phase/attack behavior or fight-state persistence. Existing code may already implement
circle, dive, fireball, breath, perch, fight-file persistence, and credits; prove each status from
source and tests first. Prioritize a missing behavior with clear server and client-observable
acceptance, such as phase transitions and target selection. Do not redo respawn crystals or
protocol beam metadata from task 05.

## Ownership and constraints

Own `crates/ferrite-server/src/game/dragon.rs` if present, dragon-specific logic in
`crates/ferrite-server/src/game/region.rs`, and dragon fight persistence tests/state only when
required. Do not edit `end_fight.rs` respawn sequencing or `ferrite-protocol` without coordinator
approval. If the same dragon behavior is already complete, add only a focused missing regression
test and report the remaining gaps; do not invent scope. ASWM and Rust world generation are
excluded.

## Acceptance

- Verify the behavior gap against current code and local 26.3 data; state the before/after behavior.
- Add focused deterministic tests for phase/attack transitions, target edge cases, and persistence
  if the selected slice affects saved state.
- Run focused End tests, `cargo fmt --all -- --check`, and scoped clippy. Do not claim real-client
  visuals or full vanilla parity unless tested with a real client.
- Commit, then run `cargo clean` in this worktree. Do not delete shared caches or other worktrees.
  Report the commit, exact checks, and remaining unverified behaviors, then stop.
