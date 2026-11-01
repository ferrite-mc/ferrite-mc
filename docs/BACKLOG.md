# Backlog — remaining work by family

State of `main` when this list was written: **2026-10-31, `3a43753`** — `cargo test --workspace`
reports 782 passed / 0 failed / 2 ignored (97 test binaries); see `docs/HANDOFF.md` for the measured
Day-5 checks. Everything below is *not* done; each family names the crate or document to start
from. Tick items off in the commit that finishes them.

## World generation

`ferrite-worldgen` has the 26.3 data model, the density-function evaluator, bit-exact noise and
the multi-noise biome source. The server still only serves superflat or Anvil chunks.

- W1c: finish the Java differential (`tests/java_differential.rs`, `tests/java/DensityRouterDump.java`)
  to 1e-9 max error against the 26.3 jar.
- W2: noise chunk fill — ≥99% solid-mask agreement on the fixture chunks.
- W3: surface rules — ≥98% top-block agreement.
- Then: carvers, features/structures, biome blending, spawn placement from worldgen.
- Wire `ferrite-worldgen` into the server's `ChunkSource` so a world without region files generates.
- Brief and known API differences: `docs/handoff/worldgen-26-3.md`.

## Entities and mobs

`ferrite-entity` has the region-local store, the vanilla tracker and item entities (M1–M3).
`ferrite-loot` supplies drops.

- Mob AI goals, spawning (light/biome rules), combat, despawn rules.
- Metadata breadth per entity type; attribute packets; passengers/vehicles.
- Projectiles, TNT, experience orbs, falling blocks, boats/minecarts.
- Entity persistence (`playerdata`-style save for entities) and chunk-unload hand-off under load.
- Regional hand-off under sustained motion: `game/region.rs` `Signal::EntityTransfer`.

## Blocks, redstone and gameplay

- Neighbour-update ordering and placement rules (`ferrite-server` `game/interaction.rs`,
  `game/region.rs`), gravity blocks, fire spread, crop growth.
- Redstone: wire, torch, repeater, comparator, piston, observer, hopper, dropper/dispenser.
- Fluids exist (`ferrite-fluid`); still missing waterlogged edge cases, fluid/redstone interaction,
  and flow into unloaded chunks.
- Containers: furnace smelting, anvil, enchanting, brewing, beacon, stonecutter, loom,
  cartography, smithing, grindstone, shulker boxes.
- Villagers/trading, portals, nether and end dimensions (`dimension` is fixed to overworld today).

## Commands

`ferrite-command` parses the generated 26.3 tree and serializes it to clients; many nodes are
still stubs that answer "Command not implemented".

- Families: `execute`, `scoreboard`, `team`, `bossbar`, `title`, `tellraw`, `data`, `summon`,
  `effect`, `setblock`, `fill`, `clone`, `worldborder`, `difficulty` aliases, `help`, `trigger`.
- Selector options: distance, volume, scores, tags, sort, limit flattening, entity type negation.
- Suggestions and `ParserRegistry` coverage per node; `/execute` store/if/unless.
- Command blocks and functions (`function`, `schedule`).
- Permission tree: `requires` levels and redirects must match `commands.json` for every node.

## Inventory and items

`ferrite-inventory` (menus, clicks, hashed slots) and `ferrite-item` (components, wire codec).

- Remaining menu layouts and click validation (see containers above).
- Drag distribution, double-click collection, shift-click between arbitrary menus.
- Creative inventory actions, equipment/armor packets, offhand.
- Item components beyond the current set; predicates and `item_model`.

## Plugins (embedded JVM)

`ferrite-plugin`, `ferrite-plugin-meta`, `ferrite-jvm`, `java/ferrite-api-impl`.

- Bukkit API breadth: `Server` is ~26/206 methods; add blocks, worlds, inventories, scoreboards.
- Events beyond player/chat: block break/place (exists), inventory, entity, world, scheduler.
- Bukkit scheduler and async tasks; plugin resource loading; `/reload` and plugin disable order.
- EssentialsX needs a CraftBukkit shim; compat results are on `wip/day4-oc2` history and
  `java/ferrite-api-impl` notes.
- Event error policy is wired (`plugin-event-errors`); add per-event tests for new events.

## Protocol and networking

`ferrite-protocol`, `ferrite-net`.

- Multi-version translation layer (started on `wip/day2-2026-10-28`; notes in
  `docs/handoff/protocol-translate.md`).
- Compression and encryption off the reader/writer thread (libdeflate is in; batching exists).
- Read timeout / keepalive parity with vanilla (30 s), rate limits on chat and commands.
- `tools/wirecheck/run.py` against a real client after **any** packet change; put it in CI once a
  headless client can be started there.
- Chunk packet cache invalidation when block entities change (`game/anvil.rs` cache stats).

## World storage and lighting

`ferrite-world`, `ferrite-light`.

- Entity and block-entity persistence beyond chests; region-file compaction; dimension directories.
- Lighting persistence for chunks modified in memory.
- Light differential: the fixture must be regenerated with `python tools/make_fixture_world.py`
  (stable double flush, 120 s+) before `compare_fixture_light_when_available` compares 256 chunks;
  the checked-in fixture used on 2026-10-30 only has 196 comparable chunks.

## Performance and operations

- Re-run the 200-bot load test (`crates/ferrite-server/examples/bots.rs`, `LOAD_TEST.md`) after the
  per-connection writer task and inventory merges; record MSPT/RSS.
- Profiler: `/tps` `/mspt` `/profile` exist; add a `/spark`-style sampling view and per-system
  attribution outside regions.
- Regionizer: move deferred mutations into per-region queues (a shared queue is still scanned).
- MSPT under real worldgen (blocked on W2/W3).
- **Optional, not started:** OpenCL GPU acceleration for chunk rendering — mesh building, light
  propagation or chunk-packet encoding on the GPU, with a CPU fallback and a per-driver benchmark.
  Evaluate only after the worldgen throughput work lands; it is the last roadmap item, not a
  prerequisite.

## Quality and release

- CI (`.github/workflows/ci.yml`): Linux overrides the MSVC toolchain via `RUSTUP_TOOLCHAIN`;
  fixture-dependent tests skip when `vanilla/26.3/fixture-world` is absent. Re-check after toolchain
  or test-harness changes.
- Integration tests: player data is per world now (Day 5). New tests must not share process state;
  keep `WorldSettings::playerdata` at `None` unless the test is about persistence.
- Release: choose the repository name, tag `v0.1.0`, publish the first release notes; review
  `LICENSE`, `NOTICE.md` and every ported-code header.
- Docs: keep `docs/CONFIG.md` and `docs/CONFIG-PAPER.md` in step with `ferrite-config`.

### Plugin bridge status (2026-10-31)

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
  `SetPlayerInventory` and resyncs the menu. Teleports reuse the `/tp` mailbox path. Health,
  food, titles, display names, sounds and kicks are still logged as unrouted.
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
  coordinates throws; and the remaining `Server` stubs throw (help map, recipes, boss bars, maps,
  world creation, tags, entity serialization). CraftBukkit shims cannot declare NMS-typed methods
  (`CraftItemStack.asNMSCopy`, `CraftWorld#getHandle`, `CraftMagicNumbers`); those stay
  NoSuchMethodError by design.
