# World interaction audit — neighbor updates, gravity, fluids, crops

Checked against `crates/ferrite-fluid` and `crates/ferrite-server/src/game/{neighbors,gravity,fluid,random_tick,region}.rs`
on `lane/world-g` (2026-11-14). No Mojang source is available here; the vanilla rules below come from the
existing doc comments, the 26.3 generated block-state report and known `FallingBlock`, `CropBlock`,
`FlowingFluid` and `StemBlock` behaviour. Nothing was checked with a real client.

## Fixed (each with a same-region and a cross-region test)

| Gap | Fix | Tests |
|---|---|---|
| Neighbor updates across a region border were dropped (`Updates::apply` skipped unowned origins and targets), so a torch whose support broke in the next region never fell off | `NeighborWorld::notify_foreign` -> `Signal::NeighborUpdate` -> the owner calls `Region::remote_block_changed`; `Updates::remote_changed` re-validates only the owner's blocks and never forwards the remote origin back | `neighbors::tests::a_change_on_a_region_border_is_handed_to_the_owner_not_ignored`, `a_cascade_after_a_remote_change_is_forwarded_to_the_next_owner`, `a_remote_change_never_touches_blocks_beyond_the_owned_side`, `a_wall_torch_breaks_when_its_support_goes_in_the_same_region`; `region::neighbor_ownership_tests::*` |
| A gravity block next to a change in another region was never scheduled | remote origins feed `schedule_gravity` | `a_remote_change_schedules_gravity_for_an_unsupported_block` |
| A stack of sand fell only one block: the block above a vacated cell was not rescheduled | `tick_gravity` calls `schedule_gravity` for the cells it vacated | `a_gravity_stack_keeps_falling_and_wakes_the_fluid_around_each_vacated_cell` (fails without the fix; runs for a column in chunk 0 and chunk 1) |
| Cells vacated by gravity or by a failed support check did not wake adjacent fluid | `Region::wake_fluid` sends `Signal::FluidChange`, which the world loop routes to whichever region owns each neighbour | same test as above |
| `gravity::is_free` treated a waterlogged slab as a liquid and ignored replaceable blocks | now `isAir \| liquid block \| replaceable \| fire` | `waterlogged_blocks_are_not_free`, `replaceable_blocks_are_free_to_fall_into` |
| Water did not wash away non-solid, non-replaceable blocks | `holds_any_fluid` accepts the `minecraft:washed_away_by_fluids` tag (`ferrite-fluid/src/lib.rs`); `RegionFluidWorld::set_block` records the replaced state and `Region::tick_fluids` spawns its loot. `Region::apply_fluid_place` does the same in the region that owns a handed-off placement, then runs the ordinary neighbor pass there | `ferrite-fluid/tests/washed_away.rs::the_washed_away_tag_decides_what_water_can_replace`, `a_waterloggable_block_only_takes_the_source_type_of_water`, `water_flows_into_and_washes_away_a_sapling`; `fluid::tests::water_that_replaces_a_plant_records_it_for_its_drops`, `a_handed_off_water_washes_a_plant_and_drops_its_loot` |
| Fluid ticks changed blocks without waking their neighbors or redstone, and a handed-off placement skipped the support pass entirely | `RegionFluidWorld::modified` collects every local write; `Region::tick_fluids` queues `Updates::block_changed` for those cells, drops removed blocks, wakes fluid and sends `Signal::RedstoneChange`. `Region::apply_fluid_place` now takes the signal sender and runs the same pass in the owning region | `fluid::tests::a_handed_off_water_washes_a_plant_and_drops_its_loot`; the existing `water_crosses_a_region_border_through_the_hand_off` now also exercises the wave-up path |
| Water that reached a chunk that was not loaded stopped there forever, and a support rule that only the unloaded side could decide never ran | `Region::hold_chunk` queues a newly held chunk and `Region::wake_loaded_chunks` implements the border pass: it re-schedules border fluid on both sides, re-validates only blocks whose `canSurvive` now fails, schedules gravity blocks and hands foreign border fluid/support to the owner. **The call is currently disabled in `Region::tick`**: a full border scan costs about 1 s of region time per join and broke join-time integration tests (`survival_eating...`). The unit tests call the pass directly | `region::neighbor_ownership_tests::a_chunk_that_loads_later_resumes_water_stopped_at_the_seam`, `a_chunk_that_loads_later_breaks_a_torch_whose_support_is_air`, `a_late_chunk_wakes_water_owned_by_the_other_region_at_the_seam` |
| Stem, sugar cane, cactus, bamboo, kelp and pitcher crop fell through to `no_op` | New handlers in `random_tick.rs` use the same crop growth-speed helper where vanilla does. The 26.3 block report has bamboo ages 0 and 1 (not 0..15), so a bamboo stalk grows when its age is 1; sugar cane and cactus use age 15 and a three-block cap | `random_tick::tests::sugar_cane_and_cactus_grow_one_block_per_sixteen_ticks_and_stop_at_three`, `bamboo_grows_to_sixteen_and_ignores_a_stage_one_top`, `kelp_grows_a_body_block_and_a_new_head_in_water`, `a_mature_stem_places_its_fruit_on_a_dirt_neighbour`, `a_pitcher_crop_grows_a_matching_upper_half`, `a_sapling_advances_one_stage_in_light` |

## Audited, no change needed

- **Fluid flow across a region border.** `FluidWorld::set_block` hands a write into a loaded foreign chunk to its owner as
  `Signal::FluidPlace`; the owner applies it and schedules neighbour ticks. Covered by
  `fluid::tests::water_crosses_a_region_border_through_the_hand_off` and, for one region,
  `water_crosses_a_chunk_border_inside_one_region`.
- **Waterlogging.** `SimpleWaterloggedBlock.placeLiquid` accepts source water only and schedules a fluid tick; both are
  modelled (`FluidTicks::put`, `can_hold_fluid_as`). A waterlogged state is not a standalone liquid block for
  `is_liquid_block`, so it is not treated as a placeable source by fluid-spread decisions, while lava still reacts to its
  water through `fluid_state`. Covered by `tests/spread.rs::waterlogged_slab_spreads_sideways_but_blocks_downward_flow`,
  `waterlogged_block_beside_lava_still_solidifies_it`, `tests/interactions.rs::waterlogged_source_schedules_spread_without_replacing_its_block`,
  and `washed_away.rs`.
- **Neighbor update order.** `neighbors::SHAPE_ORDER` and `redstone::UPDATE_ORDER` are both west, east, down, up, north,
  south. Vanilla's `Block.UPDATE_SHAPE_ORDER` and `NeighborUpdater.UPDATE_ORDER` have the same order, so no reordering was
  needed. Redstone keeps its own `RedstoneChange` handoff because a fluid or shape change is not a power change.
- **Enchanting bookshelf validation.** `enchant_table::bookshelves` requires a bookshelf and a replaceable block between
  it and the table. The generated `minecraft:enchantment_power_transmitter` tag is exactly `#minecraft:replaceable`, so the
  existing check matches the 26.3 data. The cost formula, the 15-shelf cap, the per-player seed and the list reseed were
  already tested in `enchant_table.rs`.
- **Chunk packet cache.** `AnvilChunkSource::update_block_entity` invalidates the cached packet; the regression test
  `block_entity_change_reaches_a_viewer_after_a_cache_hit` covers a viewer whose next request is a cache hit.
- **Structures and maps (already landed, docs were stale).** `CapturedStructure#getEntities` returns detached entities and
  `placeEntities` runs every non-empty `EntityTransformer` before spawning (`CapturedStructure.java`, `StructureEntitiesTest`).
  Styled map captions travel as component NBT and hidden cursors are omitted because the 26.3 decoration record has no
  visibility bit (`FerriteMapView.java`, `PlayerExtrasTest.styledMapCaptionsTravelAsComponentNBT`).

## Still open / blockers

- **Chunk-load wake is implemented but not wired into the tick loop.** `Region::wake_loaded_chunks` and its tests
exist, but a border scan of a freshly loaded chunk reads roughly 60k block positions and costs about 1 s of region
time during a join (measured on the fixture world). With the call enabled, `survival_eating_uses_food_component_after_use_duration`
failed consistently (9 s instead of 5 s) while the baseline passed. Re-enabling needs the scan to read states through the
loaded chunk's palettes instead of `AnvilChunkSource::block`, or a per-position blocked-fluid queue, plus a join benchmark.
Until then water that stopped at an unloaded chunk edge does not resume after that chunk loads in the running server.

- **Vines** still fall through to `no_op`. The exact vanilla `VineBlock` random-tick rule could not be verified from the
  data available here, and guessing the attachment/decay order would be worse than leaving the block stable.
- **Saplings** advance stage 0 -> 1 like vanilla but do not grow a tree: tree placement belongs to the worldgen feature set,
  which this lane does not own. The stage-1 state is the state vanilla leaves after a failed growth attempt.
- **Bamboo leaves** are not updated when a stalk grows; only the age/stage growth is implemented. The exact `leaves`
  transition needs the vanilla stalk code.
- **Pitcher crop** lower/upper timing follows the known shape (upper half from age 3) but has not been compared with a real
  client or the vanilla class. The state report confirms ages 0..4 and two halves.
- **Explosions on a region border** still send one `NeighborUpdate` per border cell; the receiver is bounded by
  `UPDATE_LIMIT` (512) per tick.
- Real-client validation is not possible here. Fluid destruction, growth visuals, map captions and bamboo shape changes
  were verified with unit tests only.
