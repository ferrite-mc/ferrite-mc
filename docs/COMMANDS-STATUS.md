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
- `/item`: see the `/item` entry under "Partly done" (rewritten 2026-11-13 against the 26.3 server jar).
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
- `/damage`: `from <cause>` is the causing entity now; see the `/damage` entry under "Partly done".

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
- `/loot give|insert|replace|spawn <target> loot|mine|fish|kill ...` (lane A, checked on 2026-11-13
  against the 26.3 server jar's `LootCommand` and `CommandResponseTracker` bytecode; tests are in
  `game/commands_loot.rs`):
  - `mine <pos> [mainhand|offhand|<item>]`: the block state at the position is the context; no
    tool argument is an empty hand (vanilla passes `ItemStack.EMPTY`). `mainhand`/`offhand` read
    the source player's held stack (refreshed by the inventory scan, so up to one scan old) with
    its enchantments, and fail with `commands.drop.no_held_items` when the source is not an
    entity (console). `<item>` keeps its component patch, so
    `diamond_pickaxe[enchantments={"minecraft:silk_touch":1}]` drops the block itself (tested
    against `blocks/stone`). A shulker box at the position keeps its contents, name and lock on
    the dropped item (`copy_components` from the block entity, same code as breaking it). A block
    without a `blocks/<name>` table fails with `commands.drop.no_loot_table.block`.
  - `kill <entity>`: rolls `entities/<type>` of the selected entity. A player source sets
    `killed_by_player` (vanilla `LAST_DAMAGE_PLAYER`) and its main-hand looting level; the
    console or a non-player source kills without credit (tested with `entities/blaze`). Players
    and types without a table fail with `commands.drop.no_loot_table.entity`.
  - `fish <table> <pos> [tool]` only passes the tool (vanilla passes origin and tool, no block).
  - `loot <table>` and `fish` answer with the plain message, `kill` and `mine` with the
    `..._with_table` message (vanilla's `RESPONSE_WITH(OUT)_LOOT_TABLE`). One stack reads
    "Dropped 3 Stone", anything else "Dropped N items", N being the sum of the counts; the result
    of `give` and `spawn` is that total. Oversized rolls are split at the stack size; stacks keep
    the components the loot functions set.
  - `insert` merges into chest/barrel/hopper/dropper/dispenser/furnace contents and reports
    "full"; `replace` takes `<slot> [count]` consecutive slots. Their messages are still the
    handler's own (`commands.drop.success.multiple` for insert), not vanilla's single/table
    forms: not changed, not verified against the jar.
  - Hand-off to lane B (advancement `rewards.loot`): `commands_loot::roll_reward_table`:
    ```rust
    pub fn roll_reward_table(table: &str) -> Result<Vec<ferrite_item::ItemStack>, String>
    ```
    It rolls the table under the empty reward context, splits stacks at their size and keeps
    loot-function components; the `Err` text is the "Unknown ID" message naming the table. The
    caller gives the stacks to the player (e.g. `Action::LootGive { targets, stacks, table:
    None }`, which hands items over and answers with the loot message, or `Command::Give` for
    each stack with `feedback: None` for a silent grant).
  - **Not done, with evidence** (all of it needs `crates/ferrite-loot`, which this lane may not
    edit; asked of the coordinator in the final report):
    - `fish` luck: `LootContext` has no luck field and `drops::fishing_catch` documents the same
      gap (`drops.rs`), so Luck of the Sea and the Luck effect do not change `gameplay/fishing`.
    - Looting on mob drops: 80 vanilla loot functions are `minecraft:enchanted_count_increase`
      and `ferrite-loot/src/function.rs` only evaluates `set_count`, `apply_bonus`,
      `explosion_decay`, `limit_count` and `copy_components` (the others, including
      `furnace_smelt`, `set_potion`, `enchant_randomly`, `enchant_with_levels`,
      `exploration_map`, `filtered`, `copy_state`, `set_name`, `set_damage`, are kept opaque).
      The looting level is in the context (`LootContext::looting`) but only `apply_bonus` and
      `table_bonus` read it.
    - Conditions on the killer or the damage source (`entity_properties`,
      `damage_source_properties`: 49 + 5 uses) are treated as satisfied, so e.g. a skeleton's
      "killed by a charged creeper" pool cannot be selected.
    - `kill` rolls `entities/<type>` only; mobs whose table depends on state (sheep colour,
      `DeathLootTable` NBT) are not handled.
  - The generated `reports/lang_en_us.json` has `commands.drop.no_loot_table` but the 26.3 jar
    throws `commands.drop.no_loot_table.entity` and `.block` (checked in `LootCommand`), so the
    English text for those two comes from `feedback.rs`, not from the language file.
- `/item replace|fill|override|modify` (lane A; semantics read from the 26.3 server jar's
  `ItemCommands`, `ItemProvider`, `SlotCollection.replaceSlotItems` and `SlotSelector`; tests in
  `game/slot_ops.rs` and `game/item_modifier.rs`). Corrected on 2026-11-13, the earlier text here
  ("fill only touches empty slots", "a single stack goes to every slot") was inferred and wrong:
  - `replace`: the target slots are walked in order and each takes the next stack; slots past
    the end of the stack list are left alone. A single `with` stack therefore lands in the
    **first** slot only.
  - `fill`: the stacks are cycled over **every** selected slot (`ItemProvider.cycle`), empty or
    not.
  - `override`: like `replace`, but slots past the last stack are cleared
    (`of(stacks).orElseProvide(EMPTY)`).
  - `modify`: only non-empty slots are modified (`SlotSelector.NON_EMPTY_SLOTS`); Ferrite writes
    and counts only the slots the modifier changed.
  - The success count is the number of slots set, not the number whose content differs.
    `/loot replace` shares this distribution (one rolled stack fills only the first slot).
  - Same code for block containers and players (`slot_ops::distribute`). Item components in
    the `with` argument are applied (`item_commands_keep_component_patches`); stacks read by
    `from block|entity` are copied whole, components included.
  - Item modifiers: vanilla ships **none** (`vanilla/26.3/generated/data/minecraft` has no
    `item_modifier` directory), so `modify`/`<modifier>` run data-pack modifiers from disk or
    inline JSON only. `item_modifier.rs` had never loaded a real 26.3 file: 26.3 keys functions
    and conditions by `"type"` (every vanilla loot table does), the loader only read
    `"function"`/`"condition"`. Both keys work now. Supported: `set_count`, `limit_count`,
    `set_damage`, `set_components`, `set_name` (`custom_name`, `item_name`), `set_lore` (all
    four list modes), `set_enchantments`, `set_item`, `set_potion`, `set_custom_data`,
    `set_ominous_bottle_amplifier`, `discard`, `explosion_decay` (a no-op, no explosion in the
    command context), `sequence`; conditions `random_chance` (constant), `inverted`, `all_of`,
    `any_of`. Anything else fails loading with a message naming it.
  - Vanilla functions still missing (42 types in `reports/registries.json`), by reason:
    needs registry/world data this server does not model: `enchant_randomly`,
    `enchant_with_levels`, `set_instrument`, `exploration_map`, `furnace_smelt`,
    `set_random_potion`, `set_random_dyes`, `fill_player_head`; needs loot-context parameters
    the command context lacks (vanilla rejects these in `item_modifier` too, or they read an
    entity): `apply_bonus`, `enchanted_count_increase`, `copy_name`, `copy_state`,
    `copy_custom_data`, `copy_components` (non-block sources); component formats not
    verified here: `set_attributes`, `set_banner_pattern`, `set_book_cover`,
    `set_contents`, `modify_contents`, `filtered`, `set_custom_model_data`,
    `set_firework_explosion`, `set_fireworks`, `set_loot_table`, `set_stew_effect`,
    `set_writable_book_pages`, `set_written_book_pages`, `toggle_tooltips`. Number providers
    other than constant and uniform (`binomial`, `score`, `storage`) and conditions other than
    those above are not supported either.
  - Not verified: the vanilla `slots` and `sourceSlots` arguments are a slot-source language
    (`SlotSourceArgument`: filters, limits, composites); Ferrite parses only plain names
    (`container.N`, `hotbar.*`, `weapon.mainhand`, ...). Non-player entities have no inventory
    here, so only players are written. Success/failure message texts were not re-read from the
    jar.
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
- `/damage <target> <amount> [<type> [at <pos> | by <entity> [from <cause>]]]` (lane A; read from
  the 26.3 jar's `DamageCommand`, `DamageSource`, `CombatTracker` and `Player.hurtServer`; tests
  in `game/damage_source.rs`, `commands_more.rs` and `region.rs`):
  - `by` is the direct *and* causing entity, `from` names a different causing entity.
    Knockback comes from `at` or the direct entity; the causing entity gets the kill credit of a
    mob (so `killed_by_player` loot and advancement criteria see it) and the mob's anger.
  - Damage type data comes from `data/minecraft/damage_type/*.json` (`message_id`, `scaling`,
    `death_message_type`, `exhaustion`); an unknown type is rejected like any registry argument
    (when the vanilla data is on the machine). Cross-check in
    `damage_kind_agrees_with_the_bypasses_invulnerability_tag`: for all 51 types the creative
    exemption matches the `bypasses_invulnerability` tag (`generic_kill`, `out_of_world`).
  - Player victims scale with difficulty when the type says so (`Player.hurtServer`: peaceful 0,
    easy `min(a/2+1, a)`, hard `a*3/2`; `when_caused_by_living_non_player` needs a living
    non-player causing entity). Creative and spectator players are invulnerable except to the
    `bypasses_invulnerability` types; a zero result fails with
    `commands.damage.invulnerable` as in vanilla.
  - A fatal hit announces `death.attack.<message_id>` with the attacker's name, `.item` when a
    living causing entity holds a custom-named item, `.player` plus the kill credit when nothing
    caused it, and `death.attack.badRespawnPoint.message` for `intentional_game_design`. The
    text comes from the loaded `lang_en_us.json` (`%1$s` arguments are supported now); every
    damage type's key is checked to exist in it.
  - Result and message follow vanilla: the command returns 1, the success message shows the
    requested amount as a Java float (`4.0`).
  - Not done: `fall_variants` (`fall`, `ender_pearl`) use the plain `death.attack.fall` text
    instead of the `death.fell.*` variants (needs the combat history and the victim's fall
    location, which Ferrite does not track); the sound/effect (`effects`) of a type, damage type
    exhaustion and the armor/enchantment reduction of `/damage` are not applied (the survival
    model ignores armor: `DamageKind::bypasses_armor` is true only for explosions while the
    vanilla `bypasses_armor` tag lists 19 other types and not explosions; it is not read by
    `Survival::damage`). A "living" attacker is any non-`misc` entity category, so an armor stand
    counts as non-living. Players are still hurt through the 13 hand-written `DamageKind`s
    (`damage_kind` in `game/mod.rs`); the other types only differ in message and scaling.
    Mobs take the plain amount and play the hurt animation (`mob::hurt`).
- `/locate biome|structure|poi` ask the vanilla host (ops 5, 6 and 9, need
  `generator = "vanilla"`); `poi` only finds points of interest of chunks that are stored.
- Plugin `World#generateTree` runs the matching vanilla feature through the host and keeps block
  states; it reports `false` when nothing was placed and is unavailable without the host.
