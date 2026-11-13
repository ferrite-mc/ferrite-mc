# Vanilla command coverage

> **Stale snapshot.** The sections below were written on 2026-11-04 and only partly updated
> afterwards. This block lists what has changed since, checked against source; where the two
> disagree, this block wins. The root count and tree statement were re-counted on 2026-11-12;
> see "Recount 2026-11-12" below. The per-command "Partly done" prose has not been re-verified.

## Recount 2026-11-12

Method (no guessing): the live server tree, `dispatcher()` in
`crates/ferrite-server/src/game/commands.rs`, was walked in a throwaway test (not committed;
it used only the public `CommandDispatcher` API: `root_literals`, `node`, `children`,
`is_executable`, `redirect`) and diffed path by path against
`vanilla/26.3/generated/reports/commands.json`. Redirecting nodes were treated as leaves on both
sides. Roots register through `dispatcher()` and the `register` functions of
`crates/ferrite-server/src/game/commands_{world,extras,server,more,meta,data,exec}.rs` (`more`
also calls `commands_{progress,late,loot,item}.rs`) plus `functions.rs`. The server builds its
tree by hand; `crates/ferrite-command/src/vanilla.rs` (`load_vanilla_json`, whose stubs fail
with `Command not implemented`) is not used by the server.

| measure | count |
|---|---:|
| vanilla roots in `commands.json` | 94 |
| vanilla roots registered in Ferrite's tree | 94 |
| non-vanilla roots (`tps`, `mspt`, `profile`) | 3 |
| roots whose tree matches vanilla exactly (same paths, same executable nodes) | 72 |
| roots whose tree differs in shape | 22 |

Roots with a different tree shape: clone, data, datapack, effect, execute, fill, function,
gamerule, item, particle, place, playsound, return, scoreboard, setblock, stopsound, swing, team,
teleport, test, time, trigger. **A shape difference is not counted as a gap.** The diff compares
argument names and literal layout, so it also flags a different but possibly equivalent layout
(for example `clone` takes mask/mode as flags, `time of <clock>` and `execute` are laid out
differently, `trigger` adds `<value>`, `playsound` and `swing` have non-executable nodes vanilla
makes executable). Which of these change behaviour has not been checked. That is also why this
recount does not give a single "implemented" figure.

Partial behaviour confirmed in source today:

- `/jfr`: both forms always fail with vanilla's own failure keys (`commands_more.rs`, `jfr_tree`);
  Ferrite is not a JVM process, so there is nothing to record. Use `/perf`, `/tps`, `/mspt` and
  `/profile` for native diagnostics. This is deliberate, not a gap to fill.
- `/test`: every form answers `commands.test.no_tests` (`commands_more.rs`, `no_tests`,
  `test_tree`). Vanilla 26.3 ships one test instance (`always_pass`, type `function`, structure
  `minecraft:empty`). Running tests needs a test-instance runner (batching, a test environment,
  structure placement in a test area and the `block_based` test blocks), none of which exists.
- `/item`: `item[...]` components are applied (same parser as `/give`), and `modify` plus
  `<modifier>` run data-pack item modifiers from `data/<ns>/item_modifier/**/*.json` (see the
  `/item` entry below for the supported functions).
- `/attribute`: defaults come from the target's entity type (`attributes::limits_for`); an
  attribute the type does not carry (and every non-living type) fails `failed.no_attribute`.
- `/damage`: `from <cause>` only matters for death messages, which are not modelled
  (`commands.rs`, line 665).

Roots the sections below call partial but that were **not** re-verified today: advancement,
recipe, compute, waypoint, loot, ride, place, fillbiome, attribute, dialog, spectate, swing, tick,
forceload, debug, perf, fetchprofile, rotate, locate. So: 4 roots confirmed partial, 19 more
documented as partial, the other 71 not known to be partial but not behaviour-tested here.

## Deltas since 2026-11-04

- Advancement criteria (`game/recipe_unlock.rs`, `Criterion`): plain `player_killed_entity`,
  `consume_item`, `changed_dimension` (into one dimension), `placed_block` (one block),
  `location` (one biome), `recipe_crafted`, `bred_animals`, `tame_animal` and
  `item_used_on_block` are now evaluated. The "Not done" list in `/advancement` below (melee kills,
  eating/drinking, entering a dimension, placing a plain block) is out of date. Experience and
  recipe rewards and the chat announcement are given. Loot-table and function rewards are run
  as `loot give <player> loot <table>` and `execute as <player> at @s run function <fn>`
  (`game/mod.rs`, `recipe_unlock::reward_loot`/`reward_function`; parsing is unit-tested, the
  console wiring is not covered by an end-to-end test).
- `/waypoint`: spectators do not transmit on the locator bar.
- `/attribute`: `max_health` feeds the player's survival state.
- `/debug function` traces to `debug/commands-<time>.txt`; `/dialog show` accepts an inline
  dialog compound; `/recipe` stores the recipe book open/filter settings per player.
- `/place template` places block entity data (chests, signs); `/place feature|jigsaw|structure`
  carry block entities from the host.
- `/forceload` persists per dimension and holds chunks again at startup; adds and removes reach
  the scheduler as one batch (`Signal::HoldBatch`).
- `/summon` and plugin player location route by the player's world id, so they work in runtime
  worlds created through `Server#createWorld` (`docs/BACKLOG.md` section 8).

## Snapshot of 2026-11-04 (stale)

Goal: every vanilla command works like vanilla. The list of root commands comes from the
dispatcher compared with `vanilla/26.3/generated/reports/commands.json` (94 roots).

## Done on 2026-11-04 (all in `commands_more.rs` unless noted)

version, random, setidletimeout, teammsg/tm, transfer, stopwatch, publish, unpublish, tick,
rotate, forceload, damage, debug, perf, jfr, test, fetchprofile, spreadplayers, swing, attribute, dialog, posteffect, spectate, fillbiome, advancement, recipe, compute, waypoint, loot, item, ride, place (the last four partly, see below).

## Not done: none missing from the command tree

All 94 vanilla roots are registered. Several only cover part of their behavior; see the next section.

## Partly done (works, but not like vanilla yet)

- `/advancement`: grant/revoke with the vanilla tree (`only/from/until/through/everything`,
  criteria). Progress is saved per player (`playerdata/<uuid>.advancements.json`) and sent as
  `update_advancements` (`advancement_packets.rs`; entries are encoded with the server's own codec by
  `tools/extractor/AdvancementExtractor.java`, framing checked against the vanilla packet bytes), so
  tabs and toasts work. Completion by holding items, the chat announcement, recipe rewards and
  experience rewards work. Not done: melee kills of a plain entity type (e.g. Monster Hunter) , eating/drinking a plain item (e.g. A Balanced Diet) , entering a dimension (e.g. The End) and placing a plain block (match_block only) complete advancements, but sweep, arrow, fire and explosion kills and every other trigger (travel, location, consume, ...) do not;
  loot-table and function rewards are given (see the note above).
- `/recipe`: give/take with the extracted recipe ids; recipes also unlock from play (`recipe_unlock.rs`: every
  second the held item names are scanned and matched against the `recipes/**` advancements'
  `inventory_changed` items/tags and `recipe_unlocked`, ignoring counts and components). Unlocked recipes are saved per player
  (`playerdata/<uuid>.recipes.json`) and sent as `recipe_book_add`/`recipe_book_remove`, plus
  `update_recipes` and `recipe_book_settings` on join (`recipe_packets.rs`; displays are encoded by
  `tools/extractor/RecipeExtractor.java`, which boots the real data-pack loader, and the framing
  is checked against the vanilla codec bytes). Not done: the `enter_block`/`tick` recipe triggers, the client's `seen_recipe` packet is ignored (the
  "new" highlight is not tracked). `recipe_book_change_settings` is stored per player
  (`playerdata/<uuid>.recipebook.json`) and sent back on join.
- `/compute`: evaluates `context_int_provider` / `context_float_provider` (constant, reference,
  conditional, div, number_dispatcher, weighted_list, match_block). `entity` context carries no
  entity data yet.
- `/waypoint`: `list` lists online players; `modify` stores color/style per entity and the locator bar
  (`waypoints.rs`, packet 141, byte-checked against the vanilla codec) applies it. Every player is a
  transmitter for the others in the same dimension (azimuth beyond 332 blocks, position inside the
  view distance, chunk otherwise). Spectators (via /gamemode or join) do not transmit. Not yet: invisible/`waypoint_transmit_range` filtering and spectators set by plugins,
  non-player transmitters.
- `/loot give|insert|replace|spawn <target> loot|mine|fish|kill ...` roll the extracted tables
  (`mine` reads the block at the position; `mainhand`/`offhand` count as no tool; `kill` rolls
  `entities/<type>` of the first selected entity with an empty context). `insert` merges into
  chest/barrel/hopper/dropper/dispenser/furnace contents and reports "full"; `replace` takes
  `<slot> [count]` consecutive slots on a block container or a player; `spawn` drops item entities.
  Stacks over the max size are split.
- `/item replace|fill|override` work on block containers and player inventories (`container.N`,
  `hotbar.N`, `inventory.N`, `armor.*`, `weapon.mainhand|offhand|*`, `*` wildcards) with `with`
  or `from block|entity`. Several source stacks map pairwise onto the slots; `fill` only touches
  empty slots; `override` clears slots left without a stack (these fill/override semantics are
  inferred, not checked against vanilla). Item components in the item argument are applied.
  `modify` and `from ... <modifier>` run data-pack item modifiers (`game/item_modifier.rs`):
  `set_count`, `limit_count`, `set_damage`, `set_components`, `set_name`, `set_lore`
  (`replace_all`), `set_enchantments`, `sequence` and constant `random_chance` conditions; an
  inline JSON modifier or `ns:path` works. Other functions or conditions make the modifier fail
  to load with a message naming them, and an unknown id gives `argument.item_modifier.unknown`.
  Only slots the modifier actually changed are written and counted.
  Non-player entities have no inventory here, so only players are written. Feedback texts are
  from memory.
- `/ride <target> mount <vehicle>|dismount`: rides live in a server-side table; the client gets
  `set_passengers` (id 109) for everyone online, and every tick the rider is teleported to the
  vehicle's position + 0.6 blocks. Sneaking dismounts. Not simulated: vehicle steering, per-type
  seat offsets, riding across dimensions, riders surviving a vehicle's region hand-off smoothly.
  Players cannot be ridden. Untested against a real client.
- `/place template <id> <pos> [rotation] [mirror] [integrity] [seed] [strict]` places the block
  palette of a structure template with vanilla's mirror-then-rotate transform about the
  origin, rotating/mirroring `facing`, `axis`, `rotation` and `north/east/south/west`
  properties (stair shapes are not adjusted). Templates are read from
  `vanilla/26.3/generated/data/minecraft/structure/**.nbt`, which `tools/fetch_vanilla.py`
  extracts from the user's own jar. Block entity data of `/place template` is placed (chests, signs); template entities
  are not placed, `strict` is accepted and ignored, jigsaw blocks become their `final_state`.
  `/place feature|jigsaw|structure` run the jar's own `/place` in the worldgen host (op 8) on a
  force-loaded area and apply the changed blocks. Block entities (chest contents and loot
  tables, signs) come back from the host as NBT and are stored under their `id`; they need
  `generator = "vanilla"`. The host integration test only runs with your own
  `FERRITE_TEST_ACCEPT_EULA=true`, so it is not part of the default test run.

- `/fillbiome` rewrites the 4x4x4 biome cells of loaded chunks (also fixed `biome_name` reading
  one section too high) and resends the column to tracking players. A `#tag` filter is read
  from the extracted vanilla tag files; it is not saved to disk until the chunk is flushed, and
  the client keeps old biome colours until it re-receives the chunk. Feedback text is from memory.

- `/attribute` stores base values and modifiers per entity and computes vanilla's formula, but
  nothing in the simulation reads them yet (only generic defaults plus the player overrides for
  attack damage, movement speed and — via the survival state — max health; per-mob defaults and `failed.entity` are missing).
- `/dialog show` takes registry ids or an inline `{type:...,title:...}` compound (checked for the fields the vanilla codec requires, so a bad one is rejected instead of kicking the client; buttons' actions are not validated). `/spectate` sends `set_camera` and the
  spectator's body follows the target every tick (sneaking ends it). Feedback texts for the new commands
  were written from memory of en_us (argument order not verified against a lang file).

- `/swing` sends the arm-swing animation to viewers; the animation type and duration are
  validated but not used, and non-living entities are not rejected (`failed.notliving`).
- `/tick sprint <time>|stop` raises the clock to the maximum rate, restores the rate and the
  frozen state afterwards and broadcasts the report; `/tick query` has no percentile line.
  Freeze stops game time, fluid ticks, random ticks and all entity simulation except players.
- `/forceload` marks are saved per dimension in `data/minecraft/chunk_tickets.dat` (forced
  tickets only) and held again at startup.
- `/debug function` runs the function line by line and writes `debug/commands-<time>.txt` (nested function calls are run, not traced separately). `/test` accepts all syntax but never runs a test (no game
  test framework); `/jfr` always fails (Ferrite is not a JVM process); `/perf` and `/debug`
  report time and ticks but write no report file.
- `/fetchprofile` only knows players who are online (no Mojang API lookup).
- `/rotate` assumes the target is in the command's dimension.
- `/damage` maps the damage type to the nearest survival kind; mobs take the plain amount and play the hurt animation,
  `at <pos>` and `by <entity>` give mobs knockback from that point and make hostile mobs turn on a player attacker; `from` (the indirect cause) and players are not affected by the source.
- `/locate biome|structure|poi` ask the vanilla host (ops 5, 6 and 9, need
  `generator = "vanilla"`); `poi` only finds points of interest of chunks that are stored.
- Plugin `World#generateTree` runs the matching vanilla feature through the host and keeps block
  states; it reports `false` when nothing was placed and is unavailable without the host.
