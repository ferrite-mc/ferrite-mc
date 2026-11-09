# Backlog — remaining work

Single list of open work, checked against source on **2026-11-09**. `README.md` holds the status
table; this file holds the plan. The old session handoffs (`docs/HANDOFF.md`, `docs/handoff/**`,
`docs/sessions.md`) were deleted on 2026-11-09 — recover them with
`git log --all -- docs/handoff` if you need the archaeology.

## Contract for every task

Ferrite's Folia-style regionized concurrency is a correctness requirement, not a later optimization.

- Keep mutable world and entity state owned by its region thread. Never read or mutate another
  region's live state directly; use the region scheduler and the existing ownership-transfer paths
  (`Signal::EntityTransfer`, the per-player mailboxes, `RegionData::{merge, split}`).
- Cross-region interactions (entities, leads, mounts, projectiles, portals, block entities, plugin
  calls) need explicit handoff, cancellation, unload and reconnect behaviour, and tests for it.
- No global lock, shared mutable registry, or synchronous cross-region wait that can serialize
  ticking or deadlock. A global index needs a bounded, documented synchronization strategy and
  immutable or snapshot data where possible.
- Tests must cover same-region and cross-region cases, region split/merge, and persistence. Do not
  claim Folia compatibility from single-region tests.
- No Mojang code or assets. Real-client visual checks are excluded and must be reported as a
  limitation, not replaced with a claim.

## Landed since this file was last rewritten (do not re-implement)

- Redstone family including observers, hoppers, droppers/dispensers, command blocks, repeaters,
  comparators and torches — `game/{redstone,hopper,dispenser,command_block}.rs`.
- Pistons: resolver, entity pushes, two-tick overlay, region split/merge — `docs/pistons.md`.
- Nether and End dimensions, portals, the dragon fight, respawn stages, exit portal, gateways —
  `docs/end-dimension.md`.
- Containers: furnace/blast furnace/smoker, anvil, enchanting, grindstone, stonecutter, smithing,
  loom, brewing, beacon, chests/barrels, villager and trader menus —
  `game/{furnace,container,brewing,beacon}.rs`.
- Villagers and deterministic trades, wandering trader spawn/lifetime with the llama caravan,
  horse/llama riding and inventories — `game/{wandering_trader,ride,container}.rs`.
- Entity and block-entity persistence across restarts; the world is saved as Anvil under `world/`.
- All 94 vanilla command roots are registered, many only partly — `docs/COMMANDS-STATUS.md`.
- Signed chat, Velocity/Bungee forwarding, the per-connection writer task, the chunk packet cache.

## 1. Real 26.3 client validation (highest value; cannot be automated here)

Run `python tools/wirecheck/run.py` against a real client after any packet change, then check by
hand: dimension travel, arrival platform, void death, End boss bar/dragon/gateways, piston
animation, animal interaction, vault and trading screens, plugin output. Everything below is
server-side only.

## 2. Entity and mob parity

- Dragon: vanilla strafe phases and the gateway beam (`docs/end-dimension.md`).
- Projectiles beyond arrow, snowball, egg, ender pearl and fireball: potion, experience bottle,
  thrown trident, wind charge, fishing bobber.
- Shoulder parrot is bookkeeping only (`set/get/releaseShoulderEntity*`): no entity, no display.
- Structure entities: `Structure#getEntities()` is always empty and `EntityTransformer` lists are
  refused. This needs host-side non-player entity snapshots inside a structure region first.
- Metadata breadth per entity type, attribute packets, passengers/vehicles beyond `/ride`.

## 3. Plugin API breadth

- Bukkit `Server` still throws for the help map, recipes, maps, world creation, tags and entity
  serialization; `docs/plugin-api-stubs.md` is the generated inventory.
- `InventoryDragEvent` is not fired: `container.rs` skips `ClickInput::QuickCraft` ("which are not
  fired yet"). The drag mechanics themselves already exist in `ferrite-inventory`.
- `World#sendBlockUpdate` supports signs only, `unloadChunk` cannot force-unload, and an ender pearl
  crossing a region boundary does not move its thrower.
- NMS-typed CraftBukkit methods (`CraftItemStack.asNMSCopy`, `CraftWorld#getHandle`,
  `CraftMagicNumbers`) stay `NoSuchMethodError` by design.
- Block snapshots are opt-in, so `Block#getType` for arbitrary coordinates throws.

## 4. World interaction and gameplay

- Waterlogged edge cases, fluid/redstone interaction, flow into unloaded chunks.
- Neighbour-update ordering and placement rules; audit crop growth and gravity-block chains against
  vanilla before implementing.
- Enchanting/anvil edge cases, shulker boxes, cartography table.
- Maps: only the visible cursor decoration is sent; hidden cursors and styled captions are refused.

## 5. Protocol and networking

- Multi-version translation layer (the WIP branch `wip/day2-2026-10-28` has the start).
- Compression and encryption off the reader and writer threads.
- Keepalive/read-timeout parity (30 s), chat and command rate limits.
- Chunk packet cache invalidation when a block entity changes (`game/anvil.rs` cache stats).

## 6. Performance and operations

- Regionizer: deferred mutations still go through one shared queue.
- Coordinator: `Signal::Update` takes the `command_state` mutex every tick; split it by concern.
- Re-run the 800-bot measurement after gameplay merges and record MSPT/RSS.

## 7. Release and tooling hygiene

- `v0.1.3` is prepared in-tree (`f6a381d`); tagging and publishing the notes are still open.
- Keep `docs/CONFIG.md`, `docs/CONFIG-PAPER.md` and `docs/plugin-stubs-status.md` in step with code.
- The Windows console window (`mod windows_gui` in `crates/ferrite-server/src/main.rs`, built on
  `native-windows-gui`) has no documented or designed UI surface yet.
