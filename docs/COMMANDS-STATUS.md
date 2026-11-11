# Vanilla command coverage (2026-11-10)

Goal: every vanilla command works like vanilla. The list of root commands comes from the
dispatcher compared with `vanilla/26.3/generated/reports/commands.json` (94 roots).

## Done by 2026-11-10 (all in `commands_more.rs` unless noted)

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
  experience rewards work. Loot-table and function rewards run through the console command path.
  Melee kills, consuming items, entering a dimension or a matching block, placing a block,
  crafting, breeding, taming, using an item on a block and entering a biome are matched from play
  events; sweep, arrow, fire and explosion kills and the remaining trigger details are not.
- `/recipe`: give/take with the extracted recipe ids; recipes also unlock from play (`recipe_unlock.rs`: every
  second the held item names are scanned and matched against the `recipes/**` advancements'
  `inventory_changed` items/tags and `recipe_unlocked`, ignoring counts and components). Unlocked recipes are saved per player
  (`playerdata/<uuid>.recipes.json`) and sent as `recipe_book_add`/`recipe_book_remove`, plus
  `update_recipes` and `recipe_book_settings` on join (`recipe_packets.rs`; displays are encoded by
  `tools/extractor/RecipeExtractor.java`, which boots the real data-pack loader, and the framing
  is checked against the vanilla codec bytes). `enter_block` and unconditioned `tick` recipe
  triggers run on the same scan: the block each player stands in is recorded once per change and
  the `tick` criteria fire every scan. The client's `seen_recipe` packet is still ignored (the
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
  (`mine` reads the block at the position; `mainhand`/`offhand` use the player's scanned held
  stack with its enchantments; `kill` rolls `entities/<type>` of the first selected entity).
  `insert` merges into
  chest/barrel/hopper/dropper/dispenser/furnace contents and reports "full"; `replace` takes
  `<slot> [count]` consecutive slots on a block container or a player; `spawn` drops item entities.
  Stacks over the max size are split.
- `/item replace|fill|override` work on block containers and player inventories (`container.N`,
  `hotbar.N`, `inventory.N`, `armor.*`, `weapon.mainhand|offhand|*`, `*` wildcards) with `with`
  or `from block|entity`. Several source stacks map pairwise onto the slots; `fill` only touches
  empty slots; `override` clears slots left without a stack (these fill/override semantics are
  inferred, not checked against vanilla). No item modifiers ship in the vanilla data, so `modify`
  and any `<modifier>` fail with `argument.item_modifier.unknown`.
  Non-player entities have no inventory here, so only players are written. Item components in the
  `item[...]` argument are preserved in the written stacks. Feedback texts are from memory.
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

- `/attribute` stores base values and modifiers per entity, computes vanilla's formula and reports
  an empty selection as `argument.entity.notfound.entity`. Nothing in the simulation reads the
  stored values yet beyond the player overrides for attack damage, movement speed and — via the
  survival state — max health; per-mob defaults are still missing.
- `/dialog show` takes registry ids or an inline `{type:...,title:...}` compound (checked for the fields the vanilla codec requires, so a bad one is rejected instead of kicking the client; buttons' actions are not validated). `/spectate` sends `set_camera` and the
  spectator's body follows the target every tick (sneaking ends it). Feedback texts for the new commands
  were written from memory of en_us (argument order not verified against a lang file).

- `/swing` sends the arm-swing animation to viewers and rejects non-living entities
  (`failed.notliving`); the animation type and duration are validated but not used.
- `/tick sprint <time>|stop` raises the clock to the maximum rate, restores the rate and the
  frozen state afterwards and broadcasts the report; `/tick query` reports P50/P95/P99 over the
  last 100 region tick samples.
  Freeze stops game time, fluid ticks, random ticks and all entity simulation except players.
- `/forceload` marks are saved per dimension in `data/minecraft/chunk_tickets.dat` (forced
  tickets only) and held again at startup.
- `/debug function` runs the function line by line and writes `debug/commands-<time>.txt` (nested function calls are run, not traced separately). `/test` accepts all syntax but never runs a test (no game
  test framework); `/jfr` always fails (Ferrite is not a JVM process); `/perf` and `/debug`
  report time and ticks but write no report file.
- `/fetchprofile` only knows players who are online (no Mojang API lookup).
- `/rotate` keeps each target in its own dimension.
- `/damage` maps the damage type to the nearest survival kind; mobs take the plain amount and play the hurt animation,
  `at <pos>` and `by <entity>` give mobs knockback from that point and make hostile mobs turn on a player attacker; `from` (the indirect cause) and players are not affected by the source.
- `/locate biome|structure|poi` ask the vanilla host (ops 5, 6 and 9, need
  `generator = "vanilla"`); `poi` only finds points of interest of chunks that are stored.
- Plugin `World#generateTree` runs the matching vanilla feature through the host and keeps block
  states; it reports `false` when nothing was placed and is unavailable without the host.

## Merge state

Nothing was pushed to the remote on 2026-11-04 after the worldgen/README push.
