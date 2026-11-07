# Freebuff task 21: NMS player inventory packet lifecycle

Read the shared `README.md`, `docs/plugin-compat-plan.md`, and `docs/plugin-stubs-status.md`; then
inspect existing NMS packet shims. `ClientboundSetHealthPacket` is already implemented in commit
`cd2e457`; do not redo it.

## Goal

Complete one coherent outgoing NMS inventory/menu packet family for the menus Ferrite already
supports: open screen, synchronize full contents/slot changes, and close screen. Verify exact
26.3 packet fields and ordering from the local server jar with `javap`. Route each packet to the
existing Bukkit inventory view and client-facing behavior; unsupported menu types must fail
clearly. Keep container IDs and inventory ownership consistent across open, updates, and close.

## Ownership

Own `java/ferrite-api-impl/src/main/java/net/minecraft/network/protocol/game/**`, the narrow
`ServerGamePacketListenerImpl.send` path, and focused NMS Java tests. Do not edit Bukkit API
implementations, shared HostBridge/JNI/Rust code, `plugin_lifecycle.rs`, or other packet families.
Ask the coordinator before crossing those boundaries.

## Done when

- Tests cover open/update/close order, slot/content propagation, invalid/unsupported menu IDs, and
  explicit failure for unhandled packets.
- Run focused NMS tests and `./gradlew test`; report actual GUI rendering as unverified unless a
  real Minecraft client was used.
- Commit, then run `./gradlew clean` and `cargo clean` in this worktree if Rust artifacts exist.
  Do not delete global or other-worktree caches. Report commit and checks, then stop. ASWM and Rust
  worldgen are excluded.
