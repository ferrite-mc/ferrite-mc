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
- `/execute` (`commands_exec.rs`), verified by tests against the live dispatcher:
  - Implemented: `as`, `at`, `positioned <pos>`, `positioned as`, `rotated`, `facing`, `align`,
    `anchored`, `in`, `run`; `if|unless` `block`, `blocks`, `entity`, `predicate`, `dimension`,
    `data block|entity|storage`, `score` (`<op>` and `matches <range>`, selector holders);
    `store result|success` into `block`, `entity` (players), `storage` (namespaced ids) and `score`.
  - Conditions fork like vanilla: a false test in front of `run` runs nothing and reports
    nothing; ending the chain at the test reports `Test failed` (or returns 1).
  - Not implemented (each fails with `command.unknown.argument`): `on <relation>`, `summon`,
    `positioned over <heightmap>`, `if|unless` `biome`, `items`, `slots`, `loaded`, `function`,
    `stopwatch`, and `store ... bossbar`. `execute store ... entity` only reaches players.
  - A `return` ran by one fork of `execute as @a run return ...` does not stop the other forks.
- `/function` and `/return`: `return <n>`, `return fail` and `return run <command>` end the
  innermost running function, also when reached through `execute ... run return`; outside a
  function they fail. A command that fails inside a function is reported to the source and the
  function carries on, as in vanilla. Syntax errors still abort the function at run time (vanilla
  refuses to load such a function). `function <id> with storage|block|entity ...` is not
  implemented (only the inline `{...}` compound is).
- `/damage`: `from <cause>` only matters for death messages, which are not modelled
  (`commands.rs`, line 665).

Roots the sections below call partial but that were **not** re-verified today: advancement,
recipe, compute, waypoint, loot, ride, place, fillbiome, attribute, dialog, spectate, swing, tick,
forceload, debug, perf, fetchprofile, rotate, locate. So: 4 roots confirmed partial, 19 more
documented as partial, the other 71 not known to be partial but not behaviour-tested here.

## Deltas since 2026-11-04

- Advancement criteria and rewards: see "Lane B: progress and world" at the end of this file
  (trigger table, rewards, `/recipe`, `/compute`, `/waypoint`, `/place template`, `/ride`).
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
  tabs and toasts work. Completion from play, the chat announcement and the recipe, experience,
  loot and function rewards work; **which triggers are evaluated is the table in "Lane B"
  below** (the earlier prose list here was stale).
- `/recipe`: give/take with the extracted recipe ids; recipes also unlock from play
  (`recipe_unlock.rs`, see "Lane B": `inventory_changed`, `recipe_unlocked`, `enter_block` and
  `tick`, counts and components ignored). Unlocked recipes are saved per player
  (`playerdata/<uuid>.recipes.json`) and sent as `recipe_book_add`/`recipe_book_remove`, plus
  `update_recipes` and `recipe_book_settings` on join (`recipe_packets.rs`; displays are encoded by
  `tools/extractor/RecipeExtractor.java`, which boots the real data-pack loader, and the framing
  is checked against the vanilla codec bytes). The client's `recipe_book_seen_recipe` clears the
  "new" highlight (see "Lane B"). `recipe_book_change_settings` is stored per player
  (`playerdata/<uuid>.recipebook.json`) and sent back on join.
- `/compute`: evaluates `context_int_provider` / `context_float_provider` (constant, reference,
  conditional, div, number_dispatcher, weighted_list, match_block, plus the `entity_properties`,
  `inverted`, `all_of`, `any_of` conditions). The `entity` source gives providers the target's
  type, position and dimension; see "Lane B" for what that does and does not cover.
- `/waypoint`: `list` lists online players; `modify` stores color/style per entity and the locator bar
  (`waypoints.rs`, packet 141, byte-checked against the vanilla codec) applies it. Every player is a
  transmitter for the others in the same dimension (azimuth beyond 332 blocks, position inside the
  view distance, chunk otherwise). Transmit range, receive range, hiding (head items, sneaking,
  invisibility), spectators and non-player transmitters: see "Lane B".
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
- `/ride <target> mount <vehicle>|dismount`: rides live in a server-side table (`ride.rs`); the
  client gets `set_passengers` (id 109) for everyone online, and every tick the rider is teleported
  to the vehicle's seat. Sneaking dismounts. Players cannot be ridden. Untested against a real
  client. What is and is not simulated is listed in "Lane B".
- `/place template <id> <pos> [rotation] [mirror] [integrity] [seed] [strict]` places the block
  palette of a structure template with vanilla's mirror-then-rotate transform about the
  origin, rotating/mirroring `facing`, `axis`, `rotation` and `north/east/south/west`
  properties (stair shapes are not adjusted). Templates are read from
  `vanilla/26.3/generated/data/minecraft/structure/**.nbt`, which `tools/fetch_vanilla.py`
  extracts from the user's own jar. Block entity data is placed (chests, signs), template
  entities are spawned and `strict` is documented (see "Lane B"); jigsaw blocks become their
  `final_state`.
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

## Lane B: progress and world

Verified against the code and the 26.3 jar (`javap` on the unobfuscated server jar) on branch
`lane/commands-b`. Where this section and an older bullet disagree, this section wins. The "not
re-verified" list at the top of this file is not part of Lane B.

### Advancement triggers

The table is printed by the test `coverage_table_for_the_extracted_advancements`
(`recipe_unlock.rs`). "Evaluated" is the number of criteria the server can complete; "other" is
the number it parses but never completes. A criterion that is "other" never fires.

| Trigger | Evaluated / other | Notes |
|---|---|---|
| bred_animals | 24 / 3 | only the child kind is known (`Bred`, ferrite-entity `breed.rs`), so parent/partner (frog, sniffer, turtle) is blocked |
| changed_dimension | 4 / 0 | `to` only |
| consume_item | 41 / 0 | |
| effects_changed | 2 / 1 | `all_effects`, `all_potions`; the axolotl `source` criterion is not covered. The server `Effect` enum lacks some effects (bad_omen, conduit_power, ...) |
| enter_block | 1 / 0 | |
| entity_killed_player | 1 / 0 | player killed by a mob or arrow; explosions excluded |
| inventory_changed | 28 / 0 | item names only; counts and components are ignored |
| item_used_on_block | 10 / 0 | |
| location | 61 / 6 | single-biome only; structures (stronghold, end_city, bastion, fortress, trial_chambers), `stepping_on` and equipment not done |
| nether_travel | 1 / 0 | fires on Nether to Overworld, with `distance` (x/y/z/horizontal/absolute) |
| placed_block | 12 / 3 | not done: pale_oak_log and 2 chiseled_bookshelf offset checks |
| player_killed_entity | 83 / 6 | plain entity type only; killing_blow/projectile, distance, equipment, dimension not done; tags are rejected by `kill_type` |
| recipe_crafted | 26 / 1 | decorated pot ingredients not done |
| slept_in_bed | 1 / 0 | |
| tame_animal | 1 / 20 | blocked: ferrite-entity has no cat/wolf variants (only horse/llama); `Criterion::Tames` matches unconditioned only |
| tick | 1 / 0 | |

Not implemented (0 evaluated, count of criteria): allay_drop_item_on_block 1, avoid_vibration 1,
bee_nest_destroyed 1, brewed_potion 1, channeled_lightning 1, construct_beacon 2,
crafter_recipe_crafted 1, cured_zombie_villager 1, enchanted_item 1, entity_hurt_player 1,
fall_after_explosion 1, fall_from_height 1, filled_bucket 6, fishing_rod_hooked 4,
hero_of_the_village 1, item_durability_changed 1, kill_mob_near_sculk_catalyst 1,
killed_by_arrow 3, levitation 1, lightning_strike 1, player_generates_container_loot 10,
player_hurt_entity 3, player_interacted_with_entity 8, player_sheared_equipment 1,
ride_entity_in_lava 1, shot_crossbow 1, slide_down_block 1, spear_mobs 1, started_riding 1,
summoned_entity 3, target_hit 1, thrown_item_picked_up_by_entity 2,
thrown_item_picked_up_by_player 1, used_totem 1, using_item 3, villager_trade 2.
Reason: no event hook exists for these in the world loop. For the hit and kill triggers the damage
model (`survival.rs` `DamageKind`, 13 kinds, no attacker identity) and `recipe_unlock::KILLS`
(only killer and kind) do not carry the data the predicates need. Most of the others were not
investigated beyond the missing hook.

Recipes (`data/minecraft/recipe/**`): inventory_changed 1886, recipe_unlocked 1739, enter_block 11
and tick 1 are all evaluated. `minecraft:impossible` (1) never completes, as in vanilla.

Tests: `placed_block_criteria_match_blocks_tags_and_states`,
`placing_a_block_with_a_state_completes_the_vanilla_criterion`,
`event_criteria_parse_and_match`, `events_complete_the_vanilla_advancements`,
`effects_changed_needs_every_listed_effect_with_its_ranges`.

Rewards: experience, recipes, loot and function rewards. Loot and function rewards are turned into
console commands (`loot give <p> loot <table>`, `execute as <p> at @s run function <fn>`); the
mapping is tested (`loot_and_function_rewards_become_console_commands`) but running them through
the console has no end-to-end test. No vanilla 26.3 advancement uses them; they only matter for
data packs. `effects_changed` is sampled on the 20-tick inventory scan.

### /recipe

`enter_block` and `tick` recipe triggers were already implemented. The seen_recipe packet
(serverbound play id 48, VarInt display id) is decoded in `ferrite-net` (`ServerboundPlay::RecipeSeen`,
test `seen_recipe_decodes_the_display_id`). New recipes are sent with notify and highlight flags
(1 and 2); the join resend carries highlights; removal clears them. Seen state is stored in
`<uuid>.recipehighlight.json` in the persist dir (our own format; compatibility with vanilla is
not verified). Tests: `highlighted_recipes_carry_flag_two_and_toast_flag_one`,
`seen_recipes_lose_their_highlight_and_it_survives_a_reconnect`.

### /compute

The context has `target_entity` and `this_entity` (kind, position, dimension). Supported
predicates: `entity_properties` (`minecraft:entity_type` including tags, `minecraft:location`
position and dimension), `inverted`, `all_of`, `any_of`. Unknown predicate entries never match.
The entity source resolves a selector; the default source uses the executing entity (matches
vanilla `LootContextSources`). Entity health is not available in the entity index. Feedback
messages use `commands.compute.success/failed`; vanilla uses
`command.compute.result.{unnamed,named}.{exact,rounded,invalid}`. There is no lang file here, so the
texts are unverified.

### /waypoint

Transmit range: a transmitter is shown when `distance < range`; spectator receivers ignore the
range. Sneaking (crouch modifier) and the invisibility effect shrink the range as in vanilla
(`waypoint_transmit_range_crouch`, `waypoint_transmit_range_hide`); the check is sampled on the
20-tick scan, vanilla reacts immediately. Non-player transmitters: any entity with a stored
`waypoint_transmit_range` attribute above 0 appears on the locator bar (tests
`a_mob_with_a_transmit_range_shows_up_on_the_locator_bar`,
`only_entities_with_a_raised_transmit_range_transmit`). `/waypoint modify` and `list` still
resolve players only, so a mob keeps the default style and colour. Spectators set by plugins:
unverified.

### /place

`/place template` spawns the template's entities with the vanilla Vec3 rotation, mirror and yaw
transform (checked in bytecode); the UUID is dropped. Not done: `Facing`/`Tile` fix-up of hanging
entities, vanilla's all-chunks-loaded check, and the vanilla success message key. `strict` maps to
`setKnownShape`, which only gates edge shape updates; this server never runs shape updates, so
`strict` matches vanilla and the non-strict edge fix-ups are missing. Tests: three in
`structure_template.rs`, `commands_world::template_entities_spawn_at_the_origin_plus_their_offset`,
`commands::place_template_emits_a_world_action_for_extracted_templates`. `/fillbiome` was not touched
or re-verified in this pass.

### /ride

Seat offsets follow the vanilla attachment points (`seats_follow_the_vanilla_attachment_points`);
seat scale, baby size and horse rearing offsets are not applied. Region hand-off keeps the rider for
a short grace (`a_vehicle_in_transit_between_regions_keeps_its_rider_for_a_short_grace`,
`a_teleported_vehicle_carries_its_rider_without_orphaning_it`). A cross-dimension move ends the
ride (`changing_dimension_ends_the_ride_instead_of_moving_the_rider_across_worlds`). Steering works
only for a saddled, tame, steerable horse with a player rider, however it was mounted
(`a_horse_mounted_with_ride_is_steered_by_its_player_rider`); boats, minecarts, pigs and striders are
not steered (the play-input handler only handles sneak). Whether a hand-off shows a visible jump
cannot be verified without a client.
