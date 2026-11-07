# Ferrite parallel game-systems instructions

Refresh from current `origin/main` before work. Treat source and tests as authoritative; older handoffs may be stale. The current parallel wave has six independent tasks, each described in its assigned `freebuff-*.md` file.

## Shared rules

- Keep changes inside the ownership listed by your task. Ask the coordinator before crossing a boundary or editing shared files outside your named functions.
- Preserve Minecraft Java 26.3 / protocol 777 behavior. Never commit Mojang code or assets.
- Add regression tests for behavior changes. Do not weaken existing tests.
- Run focused tests, `cargo fmt --all -- --check` and scoped clippy for Rust changes, or Gradle tests for Java changes. Report exact commands and results.
- State clearly what needs a real Minecraft client and do not claim visual verification without one.
- Commit your task. After verification and commit, run `cargo clean` in your own worktree; for Java work also run `./gradlew clean` if Gradle outputs were created. Do not delete shared/global caches or another worktree's files. Send a concise report with commit and checks, then stop.
- Do not push to `main`; coordinator reviews and integrates completed work.

## Explicit exclusions

ASWM / Advanced Slime World Manager and Rust world generation are not assigned in this wave. Rust world generation remains last on the roadmap. Do not start either task or include it as incidental scope.

## Current integration baseline

`origin/main` includes cross-region lead reconnect fixes and CI lint corrections through `da2ea4a`. Re-check `origin/main` when you start because the coordinator may integrate more commits during the wave.

## Coordinator audit — 2026-11-08

The completed wave is integrated on `yxshadxw/plan1104`: End staged respawn, wandering-trader
lifecycle, structure transformers, equine pack/death behavior, cat/parrot taming, riptide viewer
metadata, and the earlier map/projectile/plugin-event slices. Full Rust workspace tests, workspace
clippy with warnings denied, formatting check, and the Java API Gradle suite passed after integration.
No real Minecraft client was available, so visual packet behavior remains unverified. Llama caravan
parity, multi-world plugin block-entity access, and item PDC/flags remain future task specs (`26`,
`28`, `29`). ASWM and Rust world generation remain excluded/deferred.
