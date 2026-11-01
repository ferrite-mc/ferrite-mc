# Backlog — remaining work by family

State of `origin/main` when audited: **2026-11-02, `f10a510`**. Checked items are verified by the
source or test named beside them; unchecked items remain open. Tick items off in the commit that
finishes them. `docs/HANDOFF.md` records the measured session checks.

## World generation

`ferrite-worldgen` has the 26.3 data model, the density-function evaluator, bit-exact noise and
the multi-noise biome source. The server generates vanilla overworld and nether chunks from a
bounded worker pool; Anvil chunks still come from disk.

- [x] W1c: finish the Java differential (`tests/java_differential.rs`,
  `tests/java/DensityRouterDump.java`) — max error ≤ 1e-9 over 2,000 points.
- [x] W2: noise chunk fill — `tests/noise_chunk.rs` asserts ≥ 99% solid-mask agreement on
  structure-free fixture chunks.
- [x] W3: surface rules — `tests/surface.rs` asserts ≥ 98% top-block agreement.
- [x] Carvers — `NoiseChunkGenerator::carve_chunk`; `src/carver.rs` matches the 26.3 Java
  reference vectors and harness.
- [ ] Trees and structures, biome blending.
- [ ] Feature types beyond ores and ground vegetation (geodes, lakes, monster rooms, ...).
- [x] Spawn placement from worldgen (`VanillaTerrain::spawn_height`, used by `main.rs`).
- [x] Wire `ferrite-worldgen` into the server's `ChunkSource` (`game/terrain.rs` worker pool;
  `AnvilChunkSource` calls `generate_dimension`).
- Brief and known API differences: `docs/handoff/worldgen-26-3.md`.

## Entities and mobs

`ferrite-entity` has the region-local store, the vanilla tracker, item entities (M1–M3), mob
goals, combat and projectiles. `ferrite-loot` supplies drops.

- [x] Mob AI goals, combat, despawn rules.
- [x] Natural spawning with light and mob-cap rules.
- [ ] Biome-specific spawning rules; metadata breadth per entity type; attribute packets;
  passengers/vehicles.
- [x] Arrows, TNT, experience orbs, falling blocks.
- [ ] Boats and minecarts.
- [x] Entity persistence (per-chunk sidecars) and chunk-unload hand-off
  (`game/entity_save.rs`, `game/persistence_tests.rs`).
- [x] Regional hand-off under sustained motion: `game/region.rs` `Signal::EntityTransfer`
  (`entities_follow_region_merges_splits_and_hand_offs`).

## Blocks, redstone and gameplay

- [x] Neighbour-update ordering and placement rules, gravity blocks, fire spread
  (`game/neighbors.rs`, `game/gravity.rs`, `game/fire.rs`).
- [ ] Crop growth.
- [x] Redstone: wire, lever, torch, repeater, comparator, observer, hopper, dropper/dispenser
  (`game/redstone.rs` tests).
- [ ] Pistons.
- [x] Fluid waterlogging and bucket edge cases (`ferrite-fluid` tests).
- [ ] Fluid/redstone interaction and flow into unloaded chunks.
- [x] Containers: furnace, anvil, enchanting, brewing, beacon, stonecutter, loom, smithing,
  grindstone (`game/furnace.rs`, `game/brewing.rs`, `game/beacon.rs`, `game/container.rs`).
- [ ] Cartography table and shulker boxes.
- [x] Villagers/trading, portals, nether dimension (`ferrite-entity/src/villager.rs`,
  `game/portal.rs`, `game/dimension.rs`).
- [ ] End dimension (`game/terrain.rs` returns "the end generator is not implemented yet").

## Commands

`ferrite-command` parses the generated 26.3 tree and serializes it to clients; the server
registers handlers for the implemented families.

- [x] Families: `execute`, `scoreboard`, `team`, `bossbar`, `title`, `tellraw`, `data`, `summon`,
  `effect`, `setblock`, `fill`, `clone`, `worldborder`, `difficulty` aliases, `help`, `trigger`
  (`game/commands*.rs`).
- [x] Selector options: distance, volume (`dx`/`dy`/`dz`), scores, tags, sort, limit, entity type
  negation and type tags (`game/selectors.rs`).
- [x] Suggestions and `ParserRegistry` coverage per node; `/execute` store/if/unless
  (`ferrite-command/src/dispatcher.rs`, `game/commands_exec.rs`).
- [x] Command blocks and functions (`game/command_block.rs`, `game/functions.rs`,
  `crates/ferrite-server/tests/tick_function.rs`).
- [x] Permission tree: `requires` levels and redirects match `commands.json` for every node
  (`ferrite-command/tests/vanilla.rs`).

## Inventory and items

`ferrite-inventory` (menus, clicks, hashed slots) and `ferrite-item` (components, wire codec).

- [ ] Remaining menu layouts: cartography table, shulker boxes.
- [x] Drag distribution, double-click collection, shift-click between menus
  (`ferrite-inventory/src/menu.rs` `QUICK_CRAFT`, `PICKUP_ALL`, `quick_move_stack`).
- [x] Creative inventory actions and offhand (`infinite_materials`, `OFFHAND`).
- [ ] Equipment/armor packets (armor slots sync as inventory slots; no equipment packet path).
- [ ] Item components beyond the current set; item predicates (`item_model` is passed through).

## Plugins (embedded JVM)

`ferrite-plugin`, `ferrite-plugin-meta`, `ferrite-jvm`, `java/ferrite-api-impl`.

- [ ] Bukkit API breadth: `Server` now has 145 overrides in `FerriteServer` (was ~26/206);
  the remaining stubs still throw.
- [x] Events beyond player/chat: block break/place, inventory click/close, death, entity damage,
  chunk load/unload, world load, weather, level/exp (`ferrite-plugin/src/lib.rs`).
- [x] Bukkit scheduler and async tasks (`FerriteScheduler`, `FerriteAsyncScheduler`,
  `FerriteRegionScheduler`).
- [ ] Plugin resource loading; `/reload` and plugin disable order (`Server#reload` still throws).
- [ ] EssentialsX needs a CraftBukkit shim; compat results are on `wip/day4-oc2` history and
  `java/ferrite-api-impl` notes.
- [x] Event error policy is wired (`plugins.on-event-error = allow | deny`; `event_allows`).
- [ ] Per-event tests for every new event.

## Protocol and networking

`ferrite-protocol`, `ferrite-net`.

- [x] Multi-version translation layer (`ferrite-translate`; `tests/translation.rs`;
  `supported-versions = all | native`).
- [x] Compression and encryption off the reader/writer thread (per-connection writer task).
- [x] Read timeout / keepalive parity with vanilla (30 s; `DEFAULT_READ_TIMEOUT`, keepalive loop).
- [ ] Rate limits on chat and commands (server.properties keys exist; no enforcement found).
- [ ] `tools/wirecheck/run.py` against a real client after **any** packet change; put it in CI once
  a headless client can be started there (CI does not run it).
- [x] Chunk packet cache invalidation when block entities change (`game/anvil.rs`
  `update_block_entity` calls `invalidate`; cache stats).

## World storage and lighting

`ferrite-world`, `ferrite-light`.

- [x] Entity and block-entity persistence beyond chests (`game/entity_save.rs`,
  `game/persistence_tests.rs`).
- [ ] Region-file compaction (`compact()` exists only for palette containers).
- [x] Dimension directories (`game/dimension.rs`: `DIM-1`, `DIM1`).
- [ ] Lighting persistence for chunks modified in memory (light is network-only).
- [x] Light differential: `tools/make_fixture_world.py` uses the stable double flush and
  `f10a510` relights before comparing; HANDOFF Day 5 records 256 comparable chunks and a pass.

## Performance and operations

- [ ] Re-run the 200-bot load test (`crates/ferrite-server/examples/bots.rs`,
  `crates/ferrite-server/LOAD_TEST.md`) after the per-connection writer task and inventory merges;
  record MSPT/RSS. `LOAD_TEST.md` still records the 2026-10-28 run.
- [ ] Profiler: `/tps` `/mspt` `/profile` exist; `ferrite-profiler` has a sampler, but it is not
  wired into the server command and there is no `/spark`-style view.
- [x] Regionizer: deferred topology mutations replaced by region state transitions (commit
  `8eb0594`; `crates/ferrite-region/tests/mutations.rs`).
- [ ] MSPT under real worldgen (the server generates vanilla terrain now, but no MSPT measurement
  is recorded; the README reports chunks/s only).
- [ ] **Optional, not started:** OpenCL GPU acceleration for chunk rendering — mesh building,
  light propagation or chunk-packet encoding on the GPU, with a CPU fallback and a per-driver
  benchmark. Evaluate only after the worldgen throughput work lands; it is the last roadmap item,
  not a prerequisite.

## Quality and release

- [x] CI (`.github/workflows/ci.yml`): Linux overrides the MSVC toolchain via `RUSTUP_TOOLCHAIN`;
  the workflow generates vanilla data and the fixture, rejects tracked `vanilla/` files, and runs
  fmt/clippy/test on Linux and Windows.
- [x] Integration tests: player data is per world (`WorldSettings::playerdata:
  Option<PathBuf>`; `player_data_is_scoped_to_one_world`).
- [ ] Release: tag `v0.1.0`, publish the first release notes; review `LICENSE`, `NOTICE.md` and
  every ported-code header. The repository name is chosen (`ferrite-mc`); no git tag exists yet.
- [x] Docs: `docs/CONFIG.md` and `docs/CONFIG-PAPER.md` exist and document the current config
  (`CONFIG.md` includes `supported-versions`; `CONFIG-PAPER.md` models all six files).

### Plugin bridge status (2026-10-31; `Server` count refreshed 2026-11-02)

- Fixed: the game loop drives `PluginHost::tick` from the 50 ms clock, so Bukkit
  `runTask`/`runTaskLater`/`runTaskTimer` and Paper's global scheduler tasks execute on the
  live server; the typical fixture's repeating task reaches tick 5 and cancels itself.
- Fixed: the main world is published before plugins load and every joining player is registered
  with location, mode and operator state, so `Player#getLocation`, `World#getSpawnLocation`,
  `World#getPlayers` and join-time chat work against real values.
- Fixed: `plugin.yml` permissions are registered on load and removed on disable, so
  `default: true` reaches players and command permission gates pass.
- Fixed: `Native.playerInventory` is wired end to end. `main` forwards every `PlayerCommand`
  through a channel; the game loop routes inventory writes, gamemode, operator and chat to the
  owning region/player, and `PlayerInventory#setItem` now replaces the slot, sends
  `SetPlayerInventory` and resyncs the menu. Teleports reuse the `/tp` mailbox path. Health and
  food route through the owning region; titles, display names, sounds and kicks have no host
  path yet.
- Fixed: Paper's region scheduler is live. `RegionScheduler#run`, `runDelayed` and
  `runAtFixedRate` tasks come back from `PluginHost::take_region_tasks` and the game loop runs
  each on the region that owns its chunk (a task for a world the server does not own is dropped
  with a debug log). Bukkit's sync scheduler stays on the global tick, and
  `runTaskAsynchronously`, `runTaskLaterAsynchronously` and `runTaskTimerAsynchronously` run on
  Paper's async scheduler.
- Fixed: the Bukkit server is created before plugins enable and the main world is published
  first, so `onEnable` can already call `getWorlds()`; every accepted movement now refreshes the
  player snapshot (`updatePlayerLocation`), so `Player#getLocation` follows the live player and
  `registerPlayer` updates the existing `FerritePlayer` instead of replacing it.
- Open: block snapshots are still opt-in via `setBlockSnapshot`, so `Block#getType` for arbitrary
  coordinates throws; the remaining `Server` stubs throw (`FerriteServer` overrides 145 of the
  206 throwing `ServerStubs` methods). CraftBukkit shims cannot declare NMS-typed methods
  (`CraftItemStack.asNMSCopy`, `CraftWorld#getHandle`, `CraftMagicNumbers`); those stay
  NoSuchMethodError by design.
