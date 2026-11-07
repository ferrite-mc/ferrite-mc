# Freebuff task 14: narrow CraftBukkit NMS bridge slice

Read this directory's `README.md`, `docs/plugin-compat-plan.md`, and
`docs/plugin-stubs-status.md` before editing. Compare current source and tests with these docs;
they may be stale.

## Target

Implement one narrow, end-to-end NMS bridge interaction that materially improves compatibility
for a real plugin call path. Inspect existing NMS classes and `CatalogTest` first, choose one
missing item/player/world/server handle or packet-send path, and implement only the smallest
complete slice. Verify exact Minecraft 26.3 names and signatures with local `javap`; do not copy
Mojang code or assets. Keep all unimplemented calls explicit with
`UnsupportedOperationException("Ferrite: not implemented: …")`.

## Ownership and constraints

Own only `java/ferrite-api-impl/src/main/java/net/minecraft/**` and focused tests for those NMS
classes. Do not edit Bukkit `org/bukkit/**`, shared `HostBridge`/JNI/Rust boundaries, or
`Craft*` API implementation classes. If the chosen path requires those files, stop and ask the
coordinator before expanding scope. Do not claim EssentialsX, CMI, or general NMS compatibility.
ASWM and Rust world generation are out of scope.

## Acceptance

- Add focused tests proving the implemented NMS call reaches the intended bridge behavior and
  unsupported calls still fail visibly.
- Run `./gradlew test` in `java/ferrite-api-impl`; report exact results and any environment block.
- Update `docs/plugin-stubs-status.md` only if needed and only with verified scope.
- Commit the change, then run `./gradlew clean` and `cargo clean` in this worktree if Rust build
  output was created. Do not remove global caches or other worktrees' files. Report commit,
  checks, and remaining limits, then stop.
