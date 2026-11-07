# Freebuff task 22: plugin gameplay event integration matrix

Read this directory's `README.md` and inspect `crates/ferrite-server/tests/plugin_lifecycle.rs`,
`java/test-plugin`, and existing Bootstrap event hooks before editing. The inventory open/click/close
scenario is already covered by commit `5fd513b`; retain it and add distinct scenarios.

## Goal

Expand the real embedded-JVM integration coverage into a coherent gameplay event matrix for
player interaction, block interaction/break cancellation, entity damage/death, and world/entity
lifecycle events. Use the test plugin to register listeners, capture event order and cancellation,
and assert the resulting server/client-visible state through the protocol bot. This task owns
integration evidence; do not silently make production behavior appear covered if the fixture only
observes event delivery.

## Ownership

Own `crates/ferrite-server/tests/plugin_lifecycle.rs` and `java/test-plugin/**` fixtures only. Do
not edit production Rust/Java bridge code, `CatalogTest`, or NMS packet classes. If tests expose a
missing production path, record the exact case and ask the coordinator before expanding scope.
Protocol bot coverage is not real-client GUI verification. ASWM and Rust worldgen are excluded.

## Done when

- Integration tests prove listener registration, event order, cancellation effects, and behavior
  after plugin shutdown for the selected event families; tests remain isolated and clean up child
  processes/temp files even after failure.
- Run the focused `plugin_lifecycle` test, fmt and scoped clippy. Run Gradle fixture tests only if
  needed; report any Java-home prerequisite precisely.
- Commit, then run `cargo clean` and `./gradlew clean` in this worktree when applicable. Do not
  delete global or other-worktree caches. Report commit, exact checks, uncovered paths, and stop.
