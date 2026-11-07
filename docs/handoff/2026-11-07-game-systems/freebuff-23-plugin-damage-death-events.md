# Freebuff task 23: end-to-end plugin damage and death events

Read the shared `README.md`, `docs/plugin-compat-plan.md`, `docs/plugin-stubs-status.md`, and the
latest `origin/main` before editing. Also inspect `crates/ferrite-server/tests/plugin_lifecycle.rs`;
the existing bot coverage identifies missing production event hooks but does not implement them.

## Goal

Complete the server-to-JVM gameplay event path for entity damage and death as one coherent feature:

1. Before applying damage, construct and fire Bukkit `EntityDamageEvent` on the correct server
   path. Honor cancellation and any plugin-modified damage exactly once; cancelled damage must not
   change health, trigger death, or emit a successful damage result.
2. When an entity actually dies, fire `EntityDeathEvent` once with the final drops and experience.
   Apply plugin edits to drops/experience to the actual server outcome and avoid duplicate drops on
   later ticks, save/load, or repeated damage.
3. Cover player and supported non-player entities, including lethal damage and cancelled damage.
   Preserve the existing native event order and do not fire death for prevented damage.

The known gaps are the `PlayerCommand::Damage -> Command::PluginDamage -> Player::hurt` route and
the missing Rust call sites for `Bootstrap.fireEntityDamage` / `Bootstrap.fireEntityDeath`. Trace
all relevant damage paths before changing them; source may have advanced since the report.

## Ownership and constraints

Own the narrow event bridge path in `crates/ferrite-server/src/game/region.rs`,
`crates/ferrite-server/src/game/mod.rs`, `crates/ferrite-server/src/plugins.rs`, matching Java
hooks in `java/ferrite-api-impl/src/main/java/dev/ferrite/server/Bootstrap.java`, and focused
tests in `crates/ferrite-server/tests/plugin_lifecycle.rs` plus Java tests if needed. Keep edits to
damage/death event dispatch and its direct result handling. Do not change unrelated plugin APIs,
NMS packets, combat formulas, or entity AI. Ask the coordinator before broadening scope.

## Acceptance

- Integration tests run through the embedded JVM and protocol bot, proving cancellation prevents
  damage, changed damage is applied once, lethal damage fires one death event, and edited drops/XP
  reach the client/server result. Include repeated-hit and player/non-player cases where supported.
- Focused tests, `cargo fmt --all -- --check`, scoped clippy, and relevant Gradle tests pass.
  Clearly distinguish protocol-bot evidence from real Minecraft client verification.
- Update `docs/plugin-stubs-status.md` with the verified event surface and remaining unsupported
  entity paths.
- Commit the task. Then run `cargo clean` and `./gradlew clean` in this worktree if those outputs
  were built. Do not remove global caches or another worktree's files. Report commit, exact checks,
  limitations, then stop. ASWM and Rust world generation are excluded.
