# Vanilla command coverage (2026-11-04)

Goal: every vanilla command works like vanilla. The list of root commands comes from the
dispatcher compared with `vanilla/26.3/generated/reports/commands.json` (94 roots).

## Done on 2026-11-04 (all in `commands_more.rs` unless noted)

version, random, setidletimeout, teammsg/tm, transfer, stopwatch, publish, unpublish, tick,
rotate, forceload, damage, debug, perf, jfr, test, fetchprofile, spreadplayers, swing, attribute, dialog, posteffect, spectate, fillbiome, advancement, recipe, compute, waypoint, loot, item, ride, place (the last four partly, see below).

## Not done: none missing from the command tree

All 94 vanilla roots are registered. Several only cover part of their behavior; see the next section.

## Partly done (works, but not like vanilla yet)

- `/advancement`, `/recipe`: grant/revoke/give/take with the vanilla tree (`only/from/until/
  through/everything`, criteria) and recipe/advancement ids from the extracted data. State is in
  memory per player (not saved) and no `update_advancements` / `recipe_book_*` packet is sent,
  so there is no toast or recipe-book entry, and triggers never complete advancements.
- `/compute`: evaluates `context_int_provider` / `context_float_provider` (constant, reference,
  conditional, div, number_dispatcher, weighted_list, match_block). `entity` context carries no
  entity data yet.
- `/waypoint`: `list` lists online players; `modify` stores color/style per entity but sends no
  `waypoint` packet.
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
  inferred, not checked against vanilla). Item components are ignored. No item modifiers ship in
  the vanilla data, so `modify` and any `<modifier>` fail with `argument.item_modifier.unknown`.
  Non-player entities have no inventory here, so only players are written. Feedback texts are
  from memory.
- `/ride`: syntax only; `mount` fails ("not supported", no passenger model), `dismount` always
  reports "not riding".
- `/place template <id> <pos> [rotation] [mirror] [integrity] [seed] [strict]` places the block
  palette of a structure template with vanilla's mirror-then-rotate transform about the
  origin, rotating/mirroring `facing`, `axis`, `rotation` and `north/east/south/west`
  properties (stair shapes are not adjusted). Templates are read from
  `vanilla/26.3/generated/data/minecraft/structure/**.nbt`, which `tools/fetch_vanilla.py`
  extracts from the user's own jar. Block entity data and template entities are not placed
  (chests are empty), `strict` is accepted and ignored, jigsaw blocks become their `final_state`.
  `/place feature|jigsaw|structure` still fail with "not supported": they need chunk state in the
  worldgen host.

- `/fillbiome` rewrites the 4x4x4 biome cells of loaded chunks (also fixed `biome_name` reading
  one section too high) and resends the column to tracking players. A `#tag` filter is read
  from the extracted vanilla tag files; it is not saved to disk until the chunk is flushed, and
  the client keeps old biome colours until it re-receives the chunk. Feedback text is from memory.

- `/attribute` stores base values and modifiers per entity and computes vanilla's formula, but
  nothing in the simulation reads them yet (only generic defaults plus the player overrides for
  attack damage and movement speed; per-mob defaults and `failed.entity` are missing).
- `/dialog show` takes registry ids only (no inline dialog). `/spectate` sends `set_camera` but
  the server does not move the spectator with the target. Feedback texts for the new commands
  were written from memory of en_us (argument order not verified against a lang file).

- `/swing` sends the arm-swing animation to viewers; the animation type and duration are
  validated but not used, and non-living entities are not rejected (`failed.notliving`).
- `/tick sprint` is missing; `/tick query` has no percentile line. Freeze stops game time,
  fluid ticks and random ticks, not mob or entity movement.
- `/forceload` marks are not saved across restarts.
- `/debug function` always fails; `/test` accepts all syntax but never runs a test (no game
  test framework); `/jfr` always fails (Ferrite is not a JVM process); `/perf` and `/debug`
  report time and ticks but write no report file.
- `/fetchprofile` only knows players who are online (no Mojang API lookup).
- `/rotate` assumes the target is in the command's dimension.
- `/damage` maps the damage type to the nearest survival kind; mobs take the plain amount
  without a hurt animation, and `at`/`by`/`from` are parsed but not used.
- `/locate biome|structure` ask the vanilla host (ops 5 and 6, needs `generator = "vanilla"`).
  `/locate poi` still returns not found: the host has no POI request yet.

## Merge state

Nothing was pushed to the remote on 2026-11-04 after the worldgen/README push.
