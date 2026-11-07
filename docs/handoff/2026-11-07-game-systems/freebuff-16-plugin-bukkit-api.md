# Freebuff task 16: one complete Bukkit API behavior slice

Read this directory's `README.md`, `docs/plugin-compat-plan.md`, and
`docs/plugin-stubs-status.md` before editing. Check current code and tests first; do not rely on
the handoff's old status as proof.

## Target

Choose one adjacent, high-value Bukkit/Paper API path that is currently a stub or Java-only
fallback, then make it produce a real server/client effect end to end. Candidate areas include
inventory open/update, entity spawn/drop, player state metadata, or a packet-backed world/player
operation. Establish the actual call path before coding, and implement only one coherent slice.
Do not duplicate already-working methods.

## Ownership and constraints

Own only `java/ferrite-api-impl/src/main/java/org/bukkit/**` plus focused tests under that module.
Do not edit `net/minecraft/**`, shared `HostBridge`/JNI/Rust boundaries, or unrelated API families.
If a real behavior requires a shared bridge/Rust change, stop and ask the coordinator before
crossing this boundary. Keep unsupported methods explicit. Do not claim broad Paper/Folia,
EssentialsX, or NMS compatibility. ASWM and Rust world generation are out of scope.

## Acceptance

- Add Java tests for dispatch/contract behavior and any Rust regression test only if the
  coordinator approves a shared-boundary change.
- Run `./gradlew test` in `java/ferrite-api-impl`; document what remains unverified in a real
  Minecraft client.
- Update `docs/plugin-stubs-status.md` for the verified API surface.
- Commit, then run `./gradlew clean` and `cargo clean` in this worktree if Rust artifacts were
  produced. Do not delete global caches or files in another worktree. Report exact checks, commit,
  limits, and stop.
