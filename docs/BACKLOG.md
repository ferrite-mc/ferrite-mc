# Backlog — remaining work

Single list of open work, checked against source on **2026-11-18** at the vanilla-core pivot.
`README.md` holds the status table; this file holds the open work. Older coordinator and handoff
notes are historical; use this file and the current source instead of their "remaining" lists.
The open runtime-world sequence is captured below so it stays with the release branch.

## Contract for every task

Ferrite's Folia-style regionized concurrency is a correctness requirement, not a later optimization.

- Keep mutable world and entity state owned by its region thread. Never read or mutate another
  region's live state directly; use the region scheduler and the existing ownership-transfer paths
  (`Signal::EntityTransfer`, the per-player mailboxes, `RegionData::{merge, split}`).
- Cross-region interactions (entities, leads, mounts, projectiles, portals, block entities) need
  explicit handoff, cancellation, unload and reconnect behaviour, and tests for it.
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

## 1. Real 26.3 client validation (highest value; cannot be automated here)

Run `python tools/wirecheck/run.py` against a real client after any packet change, then check by
hand: dimension travel, arrival platform, void death, End boss bar/dragon/gateways, piston
animation, animal interaction, and vault and trading screens. Everything below is server-side
only.

## 2. Entity and mob parity

- Entity replacement when a player leaves and returns to chunks is pinned by regression tests
  (`two_viewers_one_leaving_first_then_reentry_keeps_one_entity` in `game/mod.rs`,
  `a_chunk_stays_held_until_the_last_viewer_releases_it` in `game/region.rs`). A real-client
  reproduction is still outstanding.
- `MobKind` covers 39 entity types. Beyond the 2026-11-19 breadth slice (shared family AI,
  daylight burning/helmets, biome spawn tables, zombie/husk drowning conversion), hostile ranged
  special AI is now implemented for **Blaze** (`BlazeAttackGoal` 60-tick charge / 3-fireball
  volley / 100-tick cooldown, <2-block melee, hover & slow-fall, `SmallFireball` player/mob
  damage+fire and `mob_griefing` block ignition), **Witch** (`RangedAttackGoal` potion selection
  for Slowness/Poison/Weakness/Harming, 32-tick self-drinking without synthetic inventory, 85%
  magic resistance & own-potion immunity, cross-region `PlayerSnapshot` and `EntityData::thrown_potion`
  handoff), **Piglin** (`CrossbowAttack` 25-tick charge + 20..39-tick delay, `BackUpIfTooClose`,
  `#piglin_safe_armor` gold-armor neutrality including across regions, 600-tick anger + pack
  alerts, melee fallback), and **Stray/Bogged** tipped arrows (Slowness I 600 ticks / Poison I 100
  ticks). **Blocker**: Piglin bartering (`AdmireItem`, 8-slot piglin inventory, offhand hold, and
  `minecraft:gameplay/piglin_bartering` item throws) remains open until mob inventory/trade
  admiring exists; golem construction, snow trails/snowballs, passive animal specials, and real
  26.3 client visual/audio verification also remain open (`docs/mob-ai-parity.md`).
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
- Metadata breadth per entity type. Default attributes are now sent when a viewer starts
  tracking a living entity; vehicle passengers and a skeleton's bow are sent on pairing start.
  Generic six-slot equipment reads/writes, tracker sync and NBT persistence are implemented;
  remaining per-type metadata, BODY/SADDLE and drop chances are still missing
  (`docs/mob-ai-parity.md`). Vehicles beyond `/ride` are unchanged.

## 3. Scope: vanilla only

Bukkit/Paper/Spigot plugin compatibility is not supported and is not planned. The plugin host,
Java API facade, and NMS bridge were removed on 2026-11-18. Do not reintroduce them without an
explicit project decision. `ferrite-jvm` remains only for the vanilla worldgen host.

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

## 7. Release and tooling hygiene

- Create the expected default directories on startup. Check fresh and existing installations,
  configured custom paths, headless mode and Windows UI startup.
- `v0.3` is the latest tag; `main` is many commits past it. Verify the Windows ZIP/NSIS output
  and release notes before publishing the next tag, and attach the artifact to the GitHub release.
- Keep `docs/CONFIG.md`, `docs/windows-console.md`, `README.md` and `docs/COMMANDS-STATUS.md` in
  step with code; `docs/README.md` marks the historical handoff and coordinator notes.
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

Runtime worlds are implemented internally: a persistent descriptor, a dedicated chunk source and
scheduler keyed by `WorldId::Runtime`, and transactional unload with player/transfer guards and
save/restore (`game/runtime_persist.rs` tests). They are used by commands and server tooling.

Open: exercising create, reopen, unload, block access, travel and persistence with a real client.
`docs/handoff-runtime-worlds-2026-11-11.md` is a point-in-time note from before travel, shutdown
persistence and unload recovery landed; its "Known gaps" list is superseded by this section.

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
