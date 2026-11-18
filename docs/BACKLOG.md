# Backlog — remaining work

Single list of open work, checked against source on **2026-11-18** at the Plugin B integration head.
`README.md` holds the status table; this file holds the open work. The six feature lanes and the
Plugin B lanes are merged. Older coordinator and handoff notes are historical; use this file and
the current source instead of their "remaining" lists. The open runtime-world sequence is
captured below so it stays with the release branch.

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
- 2026-11-14: gateway landing radius 5; perched breath cloud 200 ticks at radius 5; sheep
  dyeing; `execute summon` hands its new entity to the chain; non-player slot predicates and
  `World#getFluidData`.
- 2026-11-18 Plugin B: typed glint/glider/rarity ItemMeta components and Bungee views; contract
  v3 loaded-chunk/entity/biome reads; real `HelpMap`, `CommandMap` and `Messenger` support, which
  lets WorldEdit 7.4.6 enable and disable headlessly.

## 1. Real 26.3 client validation (highest value; cannot be automated here)

Run `python tools/wirecheck/run.py` against a real client after any packet change, then check by
hand: dimension travel, arrival platform, void death, End boss bar/dragon/gateways, piston
animation, animal interaction, vault and trading screens, plugin output. Everything below is
server-side only.

## 2. Entity and mob parity

- Entity replacement when a player leaves and returns to chunks is pinned by regression tests
  (`two_viewers_one_leaving_first_then_reentry_keeps_one_entity` in `game/mod.rs`,
  `a_chunk_stays_held_until_the_last_viewer_releases_it` in `game/region.rs`). A real-client
  reproduction is still outstanding.
- Vanilla mob AI parity is incomplete. Cross-region player snapshots, remote attack routing and
  the knockback of a mob whose melee hit was blocked by a shield (`Signal::MobKnockback`) are in.
  Hostile goal selection keeps a valid target, endermen stay neutral until provoked, creepers
  hold their fuse out to seven blocks and skeletons strafe. Pathfinding and goal selection are
  still to be audited against vanilla for each affected mob; do not claim full parity.
- Dragon: the strafe phase, the gateway creation beam, the 2400-tick automatic beam and the
  perched subphases are implemented. The perched timings were checked against the 26.3 jar
  (scan 20×10, 25/100 ticks, roar 40, one cloud at tick 10 per 200-tick flame, four flames then
  takeoff). The gateway landing radius now matches vanilla's 5. The charge now uses the 3.0
  fly-speed slope clamp and the 10-tick recovery; left: collision exits and saving its target;
  the breath cloud uses flat 3 magic damage rather than the mob effect; the guarded-spike choice
  can differ on non-vanilla host terrain (`docs/end-dimension.md`, `docs/mob-ai-parity.md`).
- Projectiles: arrow, trident, snowball, egg, ender pearl, fireballs, llama spit, experience
  bottle, wind charge, splash and lingering potion, and the fishing bobber are in
  (`ferrite-entity/src/{arrow,bobber,potion}.rs`). A lingering cloud inserts a visible
  `area_effect_cloud` entity and applies its effects to players **and** mobs; the cloud follows
  region split/merge. Bobber casting, biting, hooking and reeling have region tests.
- Shoulder parrot is a real state: the parrot entity is removed when picked up, its colour is
  synched through player metadata, it survives reconnects in a sidecar save, and releasing it
  recreates the parrot entity. Client rendering is unverified.
- Structure entities: `Structure#getEntities()` returns the captured entities and non-empty
  `EntityTransformer` lists are applied at the transformed position (`CapturedStructure.java`).
  The default world, nether and end publish entity NBT under their own world ids
  (`plugins.rs::world_id`), so capture is no longer runtime-world-only.
- Metadata breadth per entity type. Default attributes are now sent when a viewer starts
  tracking a living entity; vehicle passengers and a skeleton's bow are sent on pairing start.
  Remaining per-type metadata, a generic equip API and custom equipment persistence are still
  missing (`docs/mob-ai-parity.md`). Vehicles beyond `/ride` are unchanged.

## 3. Plugin compatibility (Plugin B)

The old bridge (`crates/ferrite-plugin`, `crates/ferrite-server/src/plugins`, and the session
stubs) was removed. The current implementation is **Plugin B**: one embedded JVM per process, a
Rust host (`crates/ferrite-plugin-host`), server wiring
(`crates/ferrite-server/src/plugin_host`), and three Java modules — `java/ferrite-host` (loader,
lifecycle, RPC), `java/ferrite-api-impl` (Bukkit/Paper facade), and `java/ferrite-nms` (selected
26.3 NMS/CraftBukkit adapters). Contract v3 is negotiated in
[`plugin-b-host-contract.md`](plugin-b-host-contract.md) (v2 remains the minimum compatible
version); lane briefs and results are under
[`handoff/`](handoff/) (`plugin-b-lane-1-host.md` … `plugin-b-lane-5-entity.md`).

Merged into `main` and covered by automated tests:

- Host: plugin discovery and `plugin.yml`/`paper-plugin.yml` validation, exactly one JVM,
  per-plugin classloader and data folder, `onLoad`/`onEnable`/`onDisable`, event delivery,
  sync/async scheduler, commands, clean shutdown (`crates/ferrite-plugin-host`,
  `crates/ferrite-server/src/plugin_host`).
- Facade: Paper API `26.3.build.227-stable` on Java 25 — Server/World/Player/inventory/item/
  event/command/scheduler/permission surfaces. Methods outside the supported surface throw
  `UnsupportedOperationException` naming the API; they are not silent no-ops.
- NMS: selected `CraftItemStack`, `CraftWorld#getHandle`, `CraftMagicNumbers`, `CraftPlayer`,
  `CraftEntity`, `ServerPlayer`, and `Entity` reads plus region-routed asynchronous writes.
- Acceptance: `crates/ferrite-server/tests/plugin_b_server_acceptance.rs` starts the real
  `ferrite-server` with `[plugins] enabled = true`, loads `typical-plugin` and `nms-plugin` into
  the same JVM, drives join/block/command/scheduler/quit, and requires clean shutdown. Evidence
  is Rust tests, JUnit, and a protocol bot only. **No real 26.3 client and no differential test
  against a vanilla server exist**, so "same as vanilla" is not verified.
- Live inventory: the server registers each joining player's `PlayerInventory` with the host,
  `FerriteHostInventoryBridge` pulls the initial snapshot and unwraps the RPC envelope, and
  `FerritePlayerInventory.applyHostSnapshot` applies it on the primary thread. The acceptance bot
  decodes `ContainerContent`/`ContainerSlot` and checks the fixture's named/damaged diamond sword,
  three emeralds and two gold ingots (`plugin-b-lane-4-items.md` follow-up).
- Live player/entity state: `WorldStateProvider` keeps join snapshots and updates location, game
  mode, health and food from the game loop; `HostStateReader` serves `player.state_get` and
  `entity.state_get` from those snapshots, and `ServerPlayer`/`Entity`/`CraftPlayer`/`CraftEntity`
  read through it. The NMS fixture exercises the path (`plugin-b-lane-5-entity.md`).
- Plugin-host benchmark: `crates/ferrite-plugin-host/tests/plugin_host_benchmark.rs` drives the real
  embedded JVM with both fixtures and records lifecycle, RSS, event/command throughput and
  scheduler ticks; the baseline and its limitations are in `docs/plugin-host-benchmark.md`.
- ItemMeta breadth: `item_name`, lore, custom model data, enchantments, unbreakable, damage,
  max damage, glint override, glider, rarity and Bungee name/lore component views round-trip
  through `FerriteItemNbt`; `ItemMetaBreadthTest` and `ItemStackTest` cover the JVM path.
- World reads: `FerriteWorld`/`FerriteChunk` expose chunk lookup, loaded/use checks, block and
  entity reads, class/nearby filters and immutable `FerriteEntitySnapshot`s. Contract v3 adds
  `world.chunk_status`, `world.loaded_chunks`, `world.entities_snapshot` and read-only
  `world.biome_get`; `World#getBiome`/`getComputedBiome` and `FerriteChunk#getBiome` read
  resident 3D biome cells without loading or generating chunks. Unloaded chunk loading and
  generation remains refused.
- Third-party scan: `tools/compat-scan` scans WorldEdit 7.4.6, ProtocolLib 5.4.0, VaultAPI 1.7.1,
  LuckPerms 5.5.71 and PlaceholderAPI 2.12.3. WorldEdit now reaches `onEnable` and disables
  cleanly headlessly through real `HelpMap`, `CommandMap` and `Messenger` surfaces; this is a
  boot result, not compatibility certification.

Open work:

- Full Bukkit/Paper surface. `java/ferrite-api-impl` contains generated `Unsupported*Base`
  classes; the methods they reject still need host ops, game-loop wiring, and tests.
- NMS is a selected surface, not full CraftBukkit. Menus and
  `ServerGamePacketListenerImpl`, packet listeners, the full 400+ method Bukkit entity surface,
  and multi-version support are explicitly refused.
- Events delivered after an action (`block_break`, `player_interact`) cannot undo it; the host
  callback boundary has no pre-action hook.
- A disabled plugin's classloader is closed but its classes are not guaranteed to unload while
  the process lives.
- WorldEdit boots headlessly and disables cleanly, but in-game commands, region edits and its
  26.3 NMS world adapters are not implemented; the dist jar still references missing
  Spigot/Paper/NMS symbols. ProtocolLib, LuckPerms and PlaceholderAPI were scanned but not
  booted; VaultAPI is a library jar. CMI and large NMS plugins are still untested.
- `ItemMeta` still refuses localized name, `damage_resistant`/fire resistance,
  `use_remainder`, `repairable` and other component lists. Add them only when
  `ferrite-item::Component` can preserve the data.
- Bulk world reads are covered by the v3 loaded-chunk, entity-snapshot and biome ops. Unloaded
  `loadChunk`/generation remains refused; the API returns `chunk_not_loaded` instead of
  generating a chunk.
- The plugin-host benchmark uses the two clean-room fixtures and an in-memory state provider.
  WorldEdit now boots headlessly, but there is still no real client, in-game edit or before/after
  comparison, so the benchmark remains a baseline rather than a speedup or compatibility claim.

## 4. World interaction and gameplay

- Waterlogging, fluid/redstone interaction and fluid-wash drops are implemented and covered by
  the `world-interaction-audit.md` tests. `Region::wake_loaded_chunks` implements the border pass
  and `Region::tick` runs it; the join benchmark is in the audit. Vines now follow their 26.3
  random tick, including the `spread_vines` rule and face-aware slab/stairs support, and bamboo
  `leaves` updates are in. Remaining audit blockers are sapling tree growth (worldgen feature set)
  and real-client checks.
- Neighbour-update ordering and placement rules match the audited six-face order; sugar cane, cactus,
  bamboo, kelp, melon/pumpkin stems, pitcher crop and sapling stage growth now have random-tick
  handlers. Gravity-block chains were audited earlier.
- Enchanting edge cases. Anvil enchantment combining and shulker boxes are in. The cartography table is implemented (`game/map_menu.rs` over `game/map.rs` `MapStore`; zoom out, lock, clone, restart persistence are unit-tested).
- **A3 breadth closure (2026-11-17)**: `FerriteItemNbt` + `FerriteHostInventoryBridge` `nbt_base64` round-trip (`InventoryNbtBridgeTest`, Rust `player_inventory_nbt_base64_round_trips_and_rejects_corrupt_nbt`), `FerritePlayer` & `FerriteBlock` runtime accessors + explicit refusals (`ApiBreadthClosureTest`). Re-counted `Unsupported*Base.java` stubs: Player 638, World 208, Server 206, ItemMeta 115, Block 70, PlayerInventory 57, Scheduler 35, PluginManager 34, Console 28, PDC 14. Not verified with a real Minecraft client.
- Maps: styled captions are sent as component NBT. Hidden cursors are omitted because the 26.3
  decoration record has no visibility bit (`FerriteMapView`, `PlayerExtrasTest`).
- `Structure#getEntities` and non-empty `EntityTransformer` lists are applied at the transformed
  position (`CapturedStructure.placeEntities`, `StructureEntitiesTest`).

## 5. Protocol and networking

- Protocol negotiation and bounded version boundary (`ferrite-protocol` `ProtocolVersion`,
  `SUPPORTED_PROTOCOLS`, `NegotiationOutcome`, `negotiate_protocol`, wired into `ferrite-net`
  `handle_connection`): accepts `26.3` (`777`) and returns explicit `OutdatedClient` /
  `OutdatedServer` disconnect reasons on mismatch. Older-protocol packet translation (started in
  `wip/day2-2026-10-28`) remains deferred because only `vanilla/26.3` data exists in this
  workspace.
- Outbound compression and CFB8 encryption offload is implemented in `ferrite-net` (`write_loop` /
  `flush_bodies`, batches `>= 4096` raw bytes via `spawn_blocking`, smaller batches inline; reader
  remains inline). Measurements and caveats are in `docs/NET-WRITER-OFFLOAD.md` (bench:
  `ferrite-net/benches/writer_stage.rs`).
- Chat and command rate limits are in (`ferrite-net/src/rate_limit.rs`, `ServerConfig::rate_limits`,
  vanilla spam-threshold defaults), configured by the `[rate-limits]` table of `ferrite.toml`.
  The 30 s read timeout is in (`ferrite-net/src/lib.rs`, `DEFAULT_READ_TIMEOUT`).

## 6. Performance and operations

- Player chunk stream and `/forceload` holder changes are batched into one scheduler topology
  lock/wake per packet or command batch. Regionizer tests cover batched add and remove; the
  end-to-end throughput gain is still unmeasured.
- `Signal::Update` is sent when player state changes. Do not skip coordinator updates based on the
  online snapshot: command teleports can make that copy stale. Profile before changing ownership.
- Re-run the 800-bot measurement after gameplay merges and record MSPT/RSS.
- The plugin-host baseline is measured in `docs/plugin-host-benchmark.md` (JVM/host start,
  `onEnable`/`onDisable`, event and command throughput, scheduler ticks, RSS). It is fixture-only;
  do not turn it into a speedup or third-party compatibility claim.

## 7. Release and tooling hygiene

- Create the expected default directories, including `plugins/`, on startup. Check fresh and
  existing installations, configured custom paths, headless mode and Windows UI startup.
- `v0.3` is the latest tag; `main` is many commits past it. Verify the Windows ZIP/NSIS output
  and release notes before publishing the next tag, and attach the artifact to the GitHub release.
- Keep `docs/CONFIG.md`, `docs/CONFIG-PAPER.md`, `docs/windows-console.md`,
  `docs/plugin-stubs-status.md`, `README.md` and `docs/COMMANDS-STATUS.md` in step with code;
  `docs/README.md` marks the historical handoff and coordinator notes.
- The Windows GUI is a Tauri 2 + WebView2 shell in `crates/ferrite-server`; its layout,
  lifecycle, and known limitations are documented in `docs/windows-console.md`.
- The portable Windows ZIP currently contains the executable and start notes; the NSIS
  bundle described by `tauri.conf.json` is not part of the ZIP.

- Dependabot alert 10 (`glib 0.18.5`, GHSA-wrw7-89jp-8q8g) is open. `glib` is a Linux GTK
  transitive dependency of Tauri; Ferrite compiles Tauri only under `cfg(windows)`, where
  GTK/glib is not built, and `cargo tree -i glib@0.18.5` prints nothing on Linux. The patched
  `glib 0.20` requires a newer GTK stack than the locked `gtk 0.18.2`; revisit when Tauri
  updates it and do not force the version.

## 8. Runtime worlds

Implemented: `Server#createWorld` and `Server#unloadWorld` (`FerriteServer.java`, host calls
`createRuntimeWorld(String,String,long)` and `unloadRuntimeWorld(String,boolean)`). A runtime world
has a persistent descriptor, its own chunk source and its own scheduler keyed by `WorldId::Runtime`;
player travel, entity transfers and summons route by world id. Unload is transactional: a player
inside the world refuses it, a failed flush restores the scheduler and catalog entry, and
`save=false` skips persistence (`game/runtime_persist.rs` tests). `WorldLoadEvent` and a
cancellable `WorldUnloadEvent` are fired, and the default world cannot be unloaded.

Still unsupported (each throws `UnsupportedOperationException`): `WorldCreator#generator`,
`generateStructures(false)`, a `WorldType` other than `NORMAL`, and environments other than
`NORMAL`, `NETHER` and `THE_END`.

Open: exercising create, reopen, unload, block access, travel and persistence with a real client
and plugin (`docs/manual-checklist.md`). `docs/handoff-runtime-worlds-2026-11-11.md` is a
point-in-time note from before travel, shutdown persistence and unload recovery landed; its
"Known gaps" list is superseded by this section.

## 9. Orca FB integration review

Reviewed the five FB worktrees through Orca on 2026-11-10. The release branch already contains the
production behavior from their completed work. Duplicate or older patches were not applied:

- `fb-cross-region-leads`: moving-piston overlays are present. Its local SmallFireball changes
  duplicate the projectile support and tests already in the release.
- `fb-end-respawn`: completed commits are already in the release base; its current local changes
  are handoff notes only.
- `fb-trader`: horse bucking and gear, cat/parrot taming and persistence, trader spawning, and the
  cross-dimension block-entity bridge are present. The branch's trader-spawn patch uses a separate
  per-region timer and removes traders after they drink, so that stale patch is not suitable.
  Its local portal preflight is superseded by the release's atomic block update.
- `fb-vault` and `fb-villager-trading`: completed code is already in the release base; local
  changes are handoff notes only.
- `freebuff2-plugin-api`: its API methods and inventory-click path are already implemented.
  Event-order and death/respawn regression assertions were integrated in `fecc56a`; the earlier
  inventory-flow test is already covered in the release source.

Orca reported no live terminals for these worktrees. The full-workspace test result and the final
release checks below are the evidence for this release; they do not certify real-client visuals.

## 10. Vanilla command completeness (A4)

- Tree parity (`commands_tree_parity.rs`): 81/94 vanilla roots match exactly; 13 roots have 19
  allow-list entries (`a`/`c` only). Fixed `/item` (`target`), `/place template` (executable without
  `<pos>`), `/place feature` (opaque parser), and `/fillbiome` (`biome` `RegistryParser::Resource`).
- Non-player entity NBT: `/data get|merge|modify|remove entity`, `data modify ... from entity`,
  and `execute store ... entity` read region-published `EntityIndex` save-NBT snapshots and route
  writes to the owning region via `Action::EntityNbt` / `entity_save::apply_entity_nbt`.
- Component-aware item predicates: `execute if|unless items|slots` supports `[component=value,...]`
  filters reusing `parse_item_components` and `ferrite_item::Component` NBT.
- Remaining limitation: no real Minecraft client was attached during these checks.
