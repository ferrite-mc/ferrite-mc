# W7 lane 1 (expanded): commands, data packs, entity attributes

Branch `w7-api-expanded`, based on main `c2c3ec5` (which already contains the W7-1 scope: PR #7,
see `docs/handoff/w7-api.md`). Real graphical-client checks are excluded; everything below is
verified by Rust/Java unit and integration tests only.

## Command coverage matrix

Source of truth is the live dispatcher (`dispatcher()` in `game/commands.rs`), not
`ferrite-command/src/vanilla.rs`. All 94 vanilla roots are registered (`docs/COMMANDS-STATUS.md`);
72 match `commands.json` exactly and 22 differ in shape. A shape difference is not a defect by
itself, so only the behaviors below were judged.

| Behavior | State | Evidence |
|---|---|---|
| `/item replace/fill/override ... with <item[components]>` | defect fixed: components were ignored | `commands_item::tests::with_applies_the_components_in_the_item_argument`, `with_rejects_unknown_components_bad_syntax_and_bad_counts` |
| `/give` and `/item` with namespaced component keys (`item[minecraft:damage=5]`) | defect fixed: the key was not quoted when converted to SNBT | `commands::tests::item_component_lists_accept_namespaced_and_bare_ids_and_commas_inside_values` |
| `/item modify` and `from block/entity <modifier>` with data-pack and inline modifiers | implemented (was unsupported) | `game::item_modifier::tests` (6), `commands_item` modify test with a temporary data-pack and permission 2, `slot_ops` test that only changed slots are reported |
| Item modifier functions outside the supported list | unsupported, fails loading naming the function | `item_modifier` tests; supported set listed in `COMMANDS-STATUS.md` |
| `/attribute` defaults for mobs | defect fixed: every non-player used the generic table (a cow read `max_health` 20) | `attributes::tests::mobs_use_their_own_type_defaults`, `types_without_the_attribute_have_none`, `selectors::tests::attribute_limits_follow_the_selected_entity_type` |
| Advancement loot-table and function rewards | implemented already; the old doc line was stale | parsing: `recipe_unlock::tests::experience_rewards_are_parsed`; the console wiring in `game/mod.rs` has no end-to-end test (unverified end to end) |
| `/jfr` | unsupported by design: Ferrite is not a JVM, no recording exists; fails with vanilla's failure keys | documented; native diagnostics are `/perf`, `/tps`, `/mspt`, `/profile` |
| `/test` | unsupported: no test-instance runner (batching, environment, structure area, `block_based` blocks) | documented in `COMMANDS-STATUS.md`; vanilla ships only `always_pass` |
| `/ride` engine | lane 2 | not audited here |
| `execute`, `return`, `function`, data-pack loading beyond item modifiers; the other 20 shape-differing roots | unverified in this lane: not re-audited behaviorally | left as listed in `COMMANDS-STATUS.md` |

## Entity metadata and attributes (Part C)

- Server: `/attribute` now takes the default and presence of an attribute from the target's entity
  type (`ferrite_data::entity_props::default_attribute`, the table `MobKind::max_health` already
  uses), through `attributes::limits_for`. `SelectedEntity` gained `kind`.
- Plugin API: `getMaxHealth()` is answered by the host for spawned living entities
  (`plugins/facts.rs`), and `SpawnedEntity` forwards `LivingEntity#getMaxHealth`. Test:
  `facts::tests::living_entities_report_their_type_max_health`, Java `SpawnedEntityHealthTest`.
- Not done: `LivingEntity#getAttribute` for mobs (no AttributeInstance backing), `setHealth` for
  mobs, and entity-metadata packet schema agreement for each entity family were not audited.
  Per-mob attribute defaults are stored by `/attribute` and read by commands; the simulation
  still reads only `max_health`, movement speed and jump strength of horses.

## Tests run on the integrated branch

See the PR for the exact counts and CI links.

## Not implemented / open

`/test` runner, `/jfr` recording, mob `getAttribute`/`setHealth`, behavioral audit of the 20
remaining shape-differing command roots, metadata packet audit. No claim of full Paper or command
parity is made.
