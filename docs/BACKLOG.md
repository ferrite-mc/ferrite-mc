# Backlog — remaining work

Single list of open work, checked against source on **2026-11-10**. `README.md` holds the status
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
- Plugin Server boundary (help map, recipes, maps, tags), `InventoryDragEvent`, entity snapshots
  (`EntityFactory`), and `World#getEntities` over the published entity registry.
- Dragon strafe phases and the End gateway creation beam; shoulder-parrot metadata packets.
- Special crafting recipes: fireworks (rocket, star and fade), leather/wolf-armor dyeing,
  decorated pots, shield decoration, written-book copying with generation tracking and tipped
  arrows, backed by typed firework, pot-decoration, banner-pattern and book-content components —
  `game/crafting.rs`, `ferrite-item/src/{component,wire}.rs`.
- Data-driven anvil enchanting: `anvil_cost`, exclusive-set conflicts and `supported_items` from
  `enchantment/*.json` — `ferrite-inventory/src/menu.rs`, `game/crafting.rs`.

## 1. Real 26.3 client validation (highest value; cannot be automated here)

Run `python tools/wirecheck/run.py` against a real client after any packet change, then check by
hand: dimension travel, arrival platform, void death, End boss bar/dragon/gateways, piston
animation, animal interaction, vault and trading screens, plugin output. Everything below is
server-side only.

## 2. Entity and mob parity

- Projectiles beyond arrow, snowball, egg, ender pearl and fireball: fishing bobber. Splash and
  lingering potions, experience bottles, thrown tridents and wind charges are handled.
- Structure entities: capture from the published entity registry, template `entities` save/load,
  `getEntities`/`getEntityCount` and `EntityTransformer` placement are implemented in the working
  tree (`CapturedStructure`, `StructureNbt`, `CapturedStructureTest`); live-server verification
  across region boundaries remains.
- Metadata breadth per entity type, attribute packets, passengers/vehicles beyond `/ride`.

## 3. Plugin API breadth

- `Server#createWorld`/`unloadWorld` still refuse runtime worlds;
  `docs/plugin-api-stubs.md` is the generated inventory.
- Help map, recipes, maps, tags, `EntityFactory#createEntitySnapshot`, `InventoryDragEvent`,
  `World#getEntities` and the `Server`/`Player` result packets are implemented.
- `Player#sendBlockUpdate` supports signs only. `World#unloadChunk` does not evict a live chunk:
  it reports success only after the chunk is unloaded, and returns false while a player or
  `/forceload` holds it. Ender pearl landings now hand off to the thrower's owning region; a
  landing from a different dimension is ignored.
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

- Compression and encryption off the reader and writer threads.
- Keepalive/read-timeout parity (30 s), chat and command rate limits.
- Chunk packet cache invalidation when a block entity changes (`game/anvil.rs` cache stats).

## 6. Performance and operations

- Player chunk additions now arrive in one `HoldBatch` and update scheduler topology through
  `add_chunks`, sharing one scheduler state lock and wake per packet batch. Removal signals are
  batched too, while holder accounting remains per chunk. The topology regression test passes;
  an end-to-end throughput measurement is still open.
- `Signal::Update` is emitted for changed player state, not for stationary ticks. Skipping updates
  against the coordinator's `online` snapshot is unsafe around command teleports; profile before
  attempting a state-ownership redesign.
- Re-run the 800-bot measurement after gameplay merges and record MSPT/RSS.

## 7. Release and tooling hygiene

- The local `v0.1.5` tag already points to `0132fd9` (Tauri Windows console); GitHub has no
  `v0.1.5` Release. Do not move the existing tag. Release notes and the portable ZIP still need
  final verification before publication.
- Keep `docs/CONFIG.md`, `docs/CONFIG-PAPER.md`, `docs/windows-console.md` and
  `docs/plugin-stubs-status.md` in step with code.
- The Windows GUI is a Tauri 2 + WebView2 shell in `crates/ferrite-server`; its layout,
  lifecycle, and known limitations are documented in `docs/windows-console.md`.
- The portable Windows ZIP currently contains the executable and start notes; the NSIS
  bundle described by `tauri.conf.json` is not part of the ZIP.
