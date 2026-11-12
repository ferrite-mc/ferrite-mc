# W7-1: Bukkit/Paper API end-to-end completion (brief, saved verbatim in substance)

Base: origin/main 236f0e2abae1a9ecc2da11008d2d224aa73a0611 (PR #6 merged). Branch: w7-api.

Rules: do not spawn more agents, merge to main, push main, tag, publish releases, or change repository policy.
Shared remote host: cap build jobs, no simultaneous heavy benchmarks, never kill others' processes.
Old BACKLOG/handoffs have stale gaps: fishing bobbers, cloud entity rendering, dragon perched/gateway beams
and shoulder gameplay already have implementations; inspect code rather than reimplementing them.

## Deliverables
1. Player#launchProjectile(Trident): remove the genuine host capability gap. Trace PlayerMisc.java launchProjectile
   through launchThrown/host calls to region-owned projectile creation; implement a real thrown trident with identity,
   velocity, owner, correct metadata and supported existing lifecycle/pickup behavior. Reuse the existing trident engine.
   Never turn it into an arrow, silently drop velocity, or return a fake entity. Follow vanilla 26.3 behavior from local
   extracted data; record limitations honestly.
2. Player#sendBlockUpdate: PlayerMisc.java accepts Sign only. Extend through real tile snapshots and block-entity packet
   encoding for block-entity families Ferrite models (prioritize chest/container, furnace, banner/skull, shulker).
   Inspect existing codecs first. Preserve location/world validation and targeted recipient behavior. Unsupported
   unmodeled families must fail explicitly; do not invent NBT. Per-player visual update, not a world mutation.
3. Paper lifecycle handlers: FerriteInternalBridge.createPluginLifecycleEventManager always throws. Implement
   registration and firing for lifecycle event types actually supported by this host, with enable/disable/reload
   ownership, ordering and cleanup. Inspect bundled Paper API contracts first; do not register callbacks that can never
   fire. Report exact remaining types and dependency.
4. Shoulder plugin bridge: game/region.rs set_shoulder/change_shoulder exist with an annotation saying plugin host wiring
   is missing. Trace Java get/set/releaseShoulderEntity, host operations and region scheduling. Connect the real path,
   preserving UUID/variant, clearing/releasing old shoulders, persistence and metadata, and the API return contract.
   Verify whether W6 wired any part; repair only actual gaps. Never call into another region's live state directly.

## Ownership
Own Java PlayerMisc/PlayerExtras and lifecycle bridge implementation/tests, relevant plugin player/entity/block host
routing, and lane-specific new Rust helper modules/tests. Shared game/mod.rs, game/region.rs, main.rs, protocol play.rs
may receive only narrow wiring/codec hunks. Do not reformat them nor touch map/menu code, ItemMeta/PDC/loot preservation,
networking, release workflows or global backlog. Keep public host op numbers stable; document any new ID.

## Quality
Test-first through the API/host boundary; cover invalid inputs, explicit unsupported cases, cancellation/lifecycle
cleanup, same-region and cross-region routing, unload/disconnect, shoulder persistence/split/merge. Every new codec gets a
round-trip test. No skipped/deleted/weakened tests, blanket lint allowances, swallowed exceptions, success-shaped no-ops.
Never commit Mojang code/assets. Linux: RUSTUP_TOOLCHAIN=stable, Java 25, Gradle wrapper; generate fixture-world with
tools/make_fixture_world.py when missing. Run cargo fmt --all --check and scoped clippy --all-targets -D warnings.
Finish with atomic commits, push only w7-api, and docs/handoff/w7-api.md (base/head SHA, behavior, tests, limitations,
shared-file wiring list, integration instructions).
