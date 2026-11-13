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

- `/item`: see the `/item` entry under "Partly done" (rewritten 2026-11-13 against the 26.3 server jar).
- `/attribute`: defaults come from the target's entity type (`attributes::limits_for`); an
  attribute the type does not carry (and every non-living type) fails `failed.no_attribute`.
- `/damage`: `from <cause>` is the causing entity now; see the `/damage` entry under "Partly done".

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
  - BLOCKED (lane/blocked-x, not started): the vanilla `slots` and `sourceSlots` arguments are a slot-source language
    (`SlotSourceArgument`: filters, limits, composites); Ferrite parses only plain names
    (`container.N`, `hotbar.*`, `weapon.mainhand`, ...). Non-player entities have no inventory
    here, so only players are written. Success/failure message texts were not re-read from the
    jar.
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
  - `fall_variants` now follow `CombatTracker.getDeathMessage` for a single fall entry
    (`death.fell.accident.*`, unit-tested); callers still pass the default fall data, and
    `death.fell.assist|finish|killer` need a combat history Ferrite lacks. Not done: the sound/effect (`effects`) of a type, damage type
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
