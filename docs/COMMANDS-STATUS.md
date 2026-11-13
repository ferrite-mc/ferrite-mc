# Vanilla command coverage

> **Partly stale.** The sections after "Snapshot of 2026-11-04" were written on 2026-11-04 and
> only partly updated afterwards. The blocks above that heading were re-verified against source
> and `vanilla/26.3/generated` on 2026-11-13 (lane C); where the two disagree, the upper blocks
> win. Anything marked "unverified" could not be checked against vanilla's source from here.

## Tree parity (measured 2026-11-13)

`crates/ferrite-server/src/game/commands_tree_parity.rs` walks the live tree (`dispatcher()`)
through the public `CommandDispatcher` API and diffs it path by path against
`vanilla/26.3/generated/reports/commands.json` (redirecting nodes are leaves on both sides). It
is a normal test: it fails on any difference that is not in its explicit allow-list and on any
allow-list entry that no longer matches (it skips when `vanilla/` is not extracted). Run it with
`FERRITE_TREE_DUMP=1 cargo test -p ferrite-server --lib tree_matches -- --nocapture` to print
every difference.

| measure | count |
|---|---:|
| vanilla roots in `commands.json` | 94 |
| vanilla roots registered | 94 |
| non-vanilla roots (`tps`, `mspt`, `profile`) | 3 |
| roots whose tree equals vanilla exactly (paths, executable nodes, parsers) | 80 |
| vanilla roots that still differ | 14 |
| individual differences / allow-list entries covering them | 207 / 26 |

Classes: **a** behaviour-equivalent layout, **b** behaviour gap (fixed, or listed as a blocker),
**c** cosmetic or an extra-lenient extension, **x** a real gap with a blocker written down below,
**o** owned by another lane. After this lane's work no class-b difference remains in a lane-C
root; what is left is:

| root | class | what differs |
|---|---|---|
| advancement, recipe, summon, execute `summon` | c | registry-key arguments parse as `resource_location`; validated when the command runs |
| locate | c | `resource_or_tag` for vanilla's `resource_or_tag_key` |
| schedule | c | `schedule clear function` also takes function tags |
| execute | a | `run` is a redirect to the root; `< <= = > >=` are one `operation` word; `store ... <type>` is one word instead of six literals; `align` takes a string for `swizzle` |
| execute | c | `if|unless function <name>` ends the chain and reports the result (vanilla needs `run`); `objective`/`loot_predicate` parse as word/`resource_location` |
| team | a | colours are literals; vanilla has one colour argument |
| gamerule | c | pre-26 camelCase names are accepted as aliases (unverified whether vanilla 26.3 rejects them) |
| return | a | `return run` carries the root redirect |
| compute, data | x | `data modify ... compute` and `/compute`'s provider parsers: the number providers are not parsed in the command tree (see Blockers) |
| mspt, tps, profile | c | non-vanilla roots |
| item (`target` vs `targets`), place (`template` executable, parsers), fillbiome (parser) | o | reported to the owning lane |

### Differences found and fixed by the audit

`playsound` (`<sound>` and `<sound> <source>` executable, vanilla range rules), `swing` (bare form),
`stopsound` (`<t> *` is not executable), `trigger` (`<objective>` alone adds 1; the non-vanilla
`<objective> <value>` form was removed; the objective lookup used a namespaced key and never
found plain objectives), `tp ... <rotation>` (no `rotation` keyword), `effect give` (default
duration, `infinite`, `hideParticles`, instant effects), `particle ... [force|normal [viewers]]`,
`team modify` option values (nested under the first literal and unreachable), `team join <team>`,
`datapack list|enable|disable|create` (placement, kinds), `function ... with storage|entity|block`,
`clone` (`filtered`, `force|move|normal`, `strict`, overlap check, block entities), `fill`
(`strict`, replace filter, block tags), `setblock ... strict`, `data get ... <scale>`,
`data modify ... from block|storage` (`from storage` used to sit under the `entity` literal and was
unreachable), `data modify ... string`, `scoreboard objectives modify displayautoupdate|numberformat`,
`scoreboard players display name|numberformat`, `test runfailed onlyRequiredTests`, and the argument
names of `data`/`execute store`/`execute if data`/`rotated`/`facing`.

## Lane C command status

- `/execute` (`commands_exec.rs`, `commands_exec_more.rs`), each subcommand has a test against
  the live dispatcher:
  - Modifiers: `as`, `at`, `positioned <pos>`, `positioned as` (now takes the entity's full
    position; it used to keep the old y), `positioned over <heightmap>`, `rotated`, `rotated as`,
    `facing <pos>`, `facing entity` (a fork), `align`, `anchored`, `in`, `on vehicle|passengers|
    controller`, `run`. Selectors reach every entity the regions publish, with all selector
    options; `as|at|if entity` over an empty selection quietly runs nothing, like vanilla's
    optional entity argument.
  - Conditions (`if`/`unless`): `block` (wildcard properties, `#tags`, nbt subset), `blocks`
    (`masked` skips air in the *source*), `biome`, `loaded`, `function`, `stopwatch`, `entity`,
    `dimension`, `score`, `data block|entity|storage`, `items|slots block` (container blocks),
    `predicate`.
  - `store result|success` into `block`, `entity` (players only), `storage`, `score`, `bossbar`
    (`value|max`).
  - A fork swallows the errors of its branches like vanilla, so the relations and nodes below are
    plain redirects: their failure reaches the caller.
  - **Not supported** (each fails with a message, never silently): `summon` (spawning is
    scheduled onto the owning region, so the new entity is not visible to the rest of the
    chain); `on attacker|leasher|origin|owner|target` (the `CommandEntity` snapshot,
    `game/entity_index.rs`, carries no relation data; `vehicle|passengers|controller` come from
    the ride table); `if items|slots entity` (entity inventories live in regions and cannot be
    read while a command runs); item predicates with components; `store ... entity` for
    non-player entities; `if predicate` is always false (the command layer has no loot context
    to evaluate datapack predicates in; `commands_late.rs` only knows `match_block`).
  - A `return` run by one fork of `execute as @a run return ...` does not stop the other forks.
- `/time` (`commands_time.rs`): `set <time>|<timemarker>`, `add`, `query time|gametime|
  <timeline> [repetition]`, `pause`, `resume`, `rate`, `of <clock>` for the `overworld` and
  `the_end` clocks (data from `data/minecraft/world_clock` and `timeline`; the Nether has no
  default clock). Unverified: that a marker moves to its next occurrence (`already_at_time_marker`
  when already there), and the `set_time` packet's pause/rate fields (not sent).
- `/datapack` (`functions.rs`, `commands_server.rs`): `list [available|enabled]`, `enable <name>
  [first|last|before|after]`, `disable`, `create`. The order is stored in
  `datapacks/.ferrite-order` (last pack wins) and honoured at load. Unverified: ordering
  semantics and the invalid-name rule. Not modelled: built-in packs (`vanilla`, feature packs).
- `/function` and `/return`: `function <id> with storage|block|entity <source> [path]` works for
  players (entity data) and block entities. `return <n>`, `return fail`, `return run <command>`
  end the innermost function; outside a function they fail with a **non-vanilla** message
  (`ferrite.function.return.not_in_function`; the lang file has no key, and whether vanilla
  accepts `/return` at the top level is unverified).
- `/clone`: `[strict] [filtered <filter>|masked|replace] [force|move|normal]`, overlap check
  ("The source and destination areas cannot overlap" unless `force`), `move` clears the source,
  block entities are copied. **Blocker:** `from <dim>`/`to <dim>` parse, but the command runs on
  one `WorldBlocks`, so a cross-dimension clone fails (`commands_world.rs`, the clone executor).
  Unverified: whether vanilla counts only changed blocks.
- `/fill` and `/setblock`: `strict` is accepted and behaves like `replace`; its real semantics
  are unverified.
- `/data`: entity data covers players only (a non-player entity is a blocker: the snapshot does
  not carry NBT). `data modify ... compute` is not implemented (blocker: the number providers
  are not parsed in the command tree, see `/compute`). `data get` numeric rules follow
  vanilla's `getData`/`getNumeric` as remembered; the `%.2f` scale rendering is unverified.
- `/scoreboard`: `objectives modify displayautoupdate|numberformat` and `players display
  name|numberformat` store their state (`MoreState`) and send vanilla's feedback. **Blocker:**
  none of it reaches clients: `ferrite-protocol`'s `SetObjective`/`SetScore` (`packets/play.rs`,
  near line 2594) writes `false` for the number-format field, so the wire encoding needs a
  protocol-crate change (out of lane C's scope; reported to the coordinator).
- `/jfr start|stop` (`commands_more.rs`, `jfr_tree`): Ferrite is not a JVM, so no recording can
  exist. `start` fails with `commands.jfr.start.failed`; `stop` fails with
  `commands.jfr.dump.failed` and says why. There is no success path. Faked: nothing. Not
  modelled: vanilla's already-running vs not-running distinction (no lang key shows one, so it
  is unverified rather than guessed).
- `/test`: `run|runmultiple|verify <tests>` resolve the selector (`*` globs, bare ids mean
  `minecraft:`) against `data/minecraft/test_instance` (vanilla ships only `always_pass`:
  type `function`, function `minecraft:always_pass`, structure `minecraft:empty`, environment
  `minecraft:default`). An unknown id fails with `commands.test.error.non_existant_test`, a
  wildcard that matches nothing with `error.no_test_instances` (the split is inferred from the
  lang keys). An instance that exists fails with an explicit "not implemented": running one
  needs structure placement, a test-instance block entity and a per-tick runner, and
  `minecraft:always_pass` is not even present as a function in the generated data (there is no
  `function/` directory). `locate` reports started/finished with 0 structures (this server never
  places any). Every other form, including `runfailed [onlyRequiredTests]`, reports
  `commands.test.no_tests`. Datapack-provided instances are not read.
- `/swing` (bare form executes, rejects non-living entities with `failed.notliving`, animation
  and hand are validated), `/tick` (`query` includes the percentile line), `/attribute`
  (defaults come from the entity type via `attributes::limits_for`; an attribute the type does not
  carry fails `failed.no_attribute`; the simulation reads only the player overrides), `/perf` and
  `/debug` (already-running/not-running state, `debug function` writes
  `debug/commands-<time>.txt`; `/perf` writes no report), `/forceload`, `/fetchprofile` (online
  players only), `/rotate` (assumes the command's dimension), `/locate`, `/dialog`, `/spectate`:
  checked in source for the claims in the old prose below; none of them was compared
  with vanilla behaviour beyond that, so this lane does not claim parity for them.
- Feedback templates: `game/feedback.rs` `render()` prints the raw key for an unknown key, so a
  test (`every_rendered_key_has_the_vanilla_template`) now checks every key passed to `render` has
  a template equal to `lang_en_us.json`; unit tests load the vanilla lang file, as the server does at
  start-up.

## Blockers (evidence)

| blocker | evidence |
|---|---|
| cross-dimension `clone` | single-world `WorldBlocks` in `commands_world.rs` |
| `execute summon`, `on attacker|leasher|origin|owner|target`, `if items|slots entity`, `store ... entity` (non-player) | `Action::Summon` is async with no uuid; `CommandEntity` (`entity_index.rs`) has no relation/NBT/inventory |
| `if predicate` | hard-wired to false in `commands_exec.rs`; only `match_block` exists in `commands_late.rs` |
| scoreboard number formats on the wire | `ferrite-protocol/src/packets/play.rs` writes `false` for the format field |
| `data modify ... compute` | no `context_int_provider` / `context_float_provider` parser in `ferrite-command` |
| `/test` runner | no structure placement / test-instance block / tick runner; `always_pass`'s function is absent from `vanilla/26.3/generated/data/minecraft` |
| `/jfr` | no JVM |

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

- `/swing` sends the arm-swing animation to viewers (see "Lane C command status").
- `/tick sprint <time>|stop` raises the clock to the maximum rate, restores the rate and the
  frozen state afterwards and broadcasts the report.
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
