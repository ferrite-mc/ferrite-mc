# Ferrite game-systems worker instructions

Baseline: `main` at `d7f6ccb` plus this instruction set. Treat source and tests as
authoritative; older handoff/backlog lists contain stale items. These tasks cover Rust
gameplay and world behavior. Rust world generation, Moonrise-style worldgen work, and the
plugin API are out of scope for this run.

## Shared constraints

- Read this file and your assigned task file before editing.
- Preserve protocol 777 / Minecraft Java 26.3 behavior and do not add Mojang code or assets.
- Keep edits within the task's owned files/functions. Ask the coordinator before crossing that
  boundary; shared integration points in `game/region.rs` must stay within the named function.
- Add regression tests for behavior changes. Do not weaken or delete existing tests.
- Run the task's focused tests, `cargo fmt --all -- --check`, and scoped clippy where practical.
- Report what was already correct, changes made, exact test commands/results, commit/hash (Freebuff
  workers), and any behavior that still needs a real-client check.
- Do not push to `main`. The coordinator reviews and integrates completed work.

## Freebuff session reuse rule

The five initial Freebuff worker sessions are the only new Freebuff sessions for this run. Once a
worker reports `worker_done` and the coordinator accepts the result, dispatch its next queued task
to that same Orca worker/session. Do not open a replacement session merely because its task ended.
If its process is positively known to have exited or its state is unverifiable, do not duplicate
the task or silently create a session; report the exact state to the coordinator for routing.

## Completion contract

Every task ends with a focused implementation result and regression evidence. A worker must not
claim client-visible visual behavior is verified without a real Minecraft client. Avoid edits to
the single shared integration test file unless your task explicitly owns it; prefer unit tests in
the module you own.
