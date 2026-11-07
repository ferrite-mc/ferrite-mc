# Freebuff task 18: embedded plugin lifecycle integration coverage

Read this directory's `README.md` and inspect `crates/ferrite-server/tests/plugin_lifecycle.rs`
before editing. Re-check current test coverage; do not recreate scenarios already covered.

## Target

Strengthen one end-to-end plugin integration path through server startup, JVM bridge/plugin
loading, a real plugin API action reaching the game/client, and orderly shutdown. Add a focused
scenario that currently lacks reliable integration coverage (for example, inventory or entity
state change, event delivery, or unsupported-call reporting). Keep the test self-contained and
ensure child server processes and temporary files are cleaned up on assertion failure.

## Ownership and constraints

Own only `crates/ferrite-server/tests/plugin_lifecycle.rs` and its test fixture files under
`java/test-plugin` if needed. Do not edit production Rust or Java bridge code, shared protocol
code, or `CatalogTest`; if coverage reveals a production bug, report it with evidence and ask the
coordinator before changing production files. Do not claim an actual Minecraft client was tested:
the protocol bot is not a game client. ASWM and Rust world generation are excluded.

## Acceptance

- Add at least one integration assertion that fails against the current missing behavior and passes
  through the actual embedded JVM/server lifecycle.
- Run the focused integration test and report exact prerequisites/results (JDK, Gradle, runtime).
- Commit, then run `cargo clean` and `./gradlew clean` in this worktree if those builds created
  outputs. Do not delete global caches or other worktrees' files. Report commit and limits, then
  stop.
