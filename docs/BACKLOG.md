# Backlog — remaining work

Single list of open work, checked against source on **2026-11-14** at `main` = `433b907`.
`README.md` holds the status table; this file holds the open work. The six feature lanes plus the
command/plugin lanes are merged. Older coordinator and handoff notes are historical; use this
file and the current source instead of their "remaining" lists. The open runtime-world sequence
is captured below so it stays with the release branch.

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

- Entity replacement when a player leaves and returns to chunks is pinned by regression tests
  (`two_viewers_one_leaving_first_then_reentry_keeps_one_entity` in `game/mod.rs`,
  `a_chunk_stays_held_until_the_last_viewer_releases_it` in `game/region.rs`). A real-client
  reproduction is still outstanding.
- Vanilla mob AI parity is incomplete. Cross-region player snapshots, remote attack routing and
  the knockback of a mob whose melee hit was blocked by a shield (`Signal::MobKnockback`) are in.
  Hostile goal selection keeps a valid target, endermen stay neutral until provoked, creepers
  hold their fuse out to seven blocks and skeletons strafe. Pathfinding and goal selection are
  still to be audited against vanilla for each affected mob; do not claim full parity.
- Dragon: strafing, the perched scanning/attacking/flaming subphases, the purple teleport beam,
  and the automatic 2400-tick gateway beam are implemented (`docs/end-dimension.md`). Remaining:
  verify the subphase timings against the 26.3 jar, refuse gateway teleports while the beam
  cooldown runs, and widen the outer-island search radius; real-client visuals remain unverified.
- Projectiles: arrow, trident, snowball, egg, ender pearl, fireballs, llama spit, experience
  bottle, wind charge, splash and lingering potion are in (`ferrite-entity/src/arrow.rs`), and the
  fishing bobber is implemented (`ferrite-entity/src/bobber.rs`). A lingering cloud still applies
  its effects to players (`potion::PotionCloud`) without sending an area-effect-cloud entity to
  clients, so it is invisible, and mobs are not affected.
- Shoulder parrot is bookkeeping only (`set/get/releaseShoulderEntity*`): no entity, no display.
- Structure entities: `Structure#getEntities()` returns the captured entities and non-empty
  `EntityTransformer` lists are applied at the transformed position (`CapturedStructure.java`).
  The default world, nether and end publish entity NBT under their own world ids
  (`plugins.rs::world_id`), so capture is no longer runtime-world-only.
- Metadata breadth per entity type, attribute packets, passengers/vehicles beyond `/ride`.

## 3. Plugin API breadth

- `Server#createWorld`/`unloadWorld` are implemented (runtime worlds, section 8). Creator options
  Ferrite cannot honour still throw `UnsupportedOperationException`.
- `Player#sendBlockUpdate` supports signs, chests, barrels, shulker boxes, furnaces, banner
  patterns and skull profiles (`plugins/tile_update.rs` `FAMILIES`). Note-block sounds, skull
  texture properties and other unmodelled tile states still throw `UnsupportedOperationException`.
  `World#unloadChunk` does not force-unload a live chunk: it succeeds after unload and returns
  false while a player or `/forceload` holds it.
- Ender pearls now route cross-region landings through the world loop to the owner's region. A
  landing after the player changes dimension is ignored.
- NMS-typed CraftBukkit methods (`CraftItemStack.asNMSCopy`, `CraftWorld#getHandle`,
  `CraftMagicNumbers`) stay `NoSuchMethodError` by design.
- Block snapshots are opt-in, so `Block#getType` for arbitrary coordinates throws.

### Plugin lane status (verified against code 2026-11-13; not verified with a real client)

| # | Item | Status | Evidence |
|---|---|---|---|
| 1 | Runtime world: persist/restore player world, join snapshot, crystal routing, immediate world after dimension change | done | `container.rs` writes `PlayerData::in_world`; join inserts `player_worlds` before `plugin_location`; `Signal::ChangeDimension` calls `update_player_location` right after `player_worlds.insert` (that handler is inside the coordinator loop and has no isolated test; only the helpers are tested); tests `playerdata` in_world round trip, `plugin_location_reports_the_players_current_world`, `crystal_from_a_runtime_world_player_lands_in_that_world` |
| 2 | `Player#breakBlock`, `chat`, 1-arg `broadcastSlotBreak`, `broadcastHurtAnimation`, `playPickupItemAnimation` | done | Java `PlayerMisc`/`PlayerSight` + stubs; host `plugins/playerops.rs`, `broadcast.rs`, `network.rs`; JUnit `PlayerActionsTest`, host tests in those modules |
| 3a | `Player#sendBlockUpdate` beyond signs | done for containers, furnaces, banners (`patterns`) and skulls (profile name/uuid) | `tile_update.rs` + `PlayerTileUpdateTest`; skull textures and note-block sound are refused with `UnsupportedOperationException` (properties encoding not verified against the jar codec); not verified with a real client |
| 3c | `ChunkSnapshot#contains(Biome)`, `ChunkData#getBiome` | snapshot done, `ChunkData#getBiome` refuses | snapshot reads `Native.biomeAt` (checked by `typical_plugin_...`); the `createChunkData` buffer has no chunk coordinates, so it no longer invents PLAINS |
| 3d | `locateNearestBiome`/`locateNearestStructure`/`hasStructureAt` | done, real-jar tested | `vanilla_host.rs::plugin_world_queries_are_answered_by_the_real_jar`; unverified: other dimensions, `findUnexplored`, structure tags |
| 3e | Compatibility proof | `typical patterns` | PDC, inventory GUI, scoreboard, title, action bar, sound, config, permission, health all run through the real JVM host (`plugin_lifecycle::typical_plugin_...`); `Chunk#getChunkSnapshot(...)` still throws "not implemented" (use `World#getEmptyChunkSnapshot`) |
| 3b | `Player#playEffect` data types | done for confirmed call sites | `FerriteEffectData.java` (Number, Boolean for 4 events, Color, Axis, BlockData, BlockFace for `WHITE_SMOKE_SHOOT`); `JukeboxSong` and other Booleans/BlockFaces refused: registry ids of data-driven registries are not in the generated reports |
| 4 | Cartography table and map store | done | `game/map_menu.rs`, `game/map.rs` `MapStore::allocate`, tests listed in `plugin-stubs-status.md` |
| 5 | Paper lifecycle manager | done for `LifecycleEvents.COMMANDS` | `FerriteLifecycleManager`, `FerriteLifecycleTest`; bootstrap-only events refused (host does not run `PluginBootstrap`) |
| 6 | World environments | NORMAL/NETHER/THE_END only; others refused | `FerriteServer.createWorld` throws; host double-checks in `main.rs`; `ApiSurfaceTest` covers the refusal |
| 7 | `AnvilChunkSource::mark_modified` | done | the method is `#[cfg(test)]`; real edits go through `set_block`, and the gap-7 test shows a plugin-set block surviving unload/reopen |
| 8 | Gap-7 steps and overlapping commands | done | gap-7 steps run on the command thread, no async scheduler; `plugin_command_world::overlapping_plugin_commands_are_serialized_and_leave_world_state_intact` |

## 4. World interaction and gameplay

- Waterlogged edge cases, fluid/redstone interaction, flow into unloaded chunks.
- Neighbour-update ordering and placement rules; audit crop growth and gravity-block chains against
  vanilla before implementing.
- Enchanting edge cases. Anvil enchantment combining and shulker boxes are in. The cartography table is implemented (`game/map_menu.rs` over `game/map.rs` `MapStore`; zoom out, lock, clone, restart persistence are unit-tested).
- Maps: only the visible cursor decoration is sent; hidden cursors and styled captions are refused.

## 5. Protocol and networking

- Multi-version translation layer (the WIP branch `wip/day2-2026-10-28` has the start).
- Compression and encryption off the reader and writer threads: measured and deferred; numbers,
  caveats and the plan are in `docs/NET-WRITER-OFFLOAD.md` (bench: `ferrite-net/benches/writer_stage.rs`).
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
