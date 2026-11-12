# World interaction audit — neighbor updates, gravity, fluids, crops

Branch `w6-world`. Checked against the behaviour implemented in `crates/ferrite-fluid` and
`crates/ferrite-server/src/game/{neighbors,gravity,fluid,random_tick,region}.rs`. No Mojang source
is available here; the vanilla rules below come from the existing doc comments and from known
`FallingBlock`, `CropBlock` and `FlowingFluid` behaviour. Nothing was checked with a real client.

## Fixed (each with a same-region and a cross-region test)

| Gap | Fix | Tests |
|---|---|---|
| Neighbor updates across a region border were dropped (`Updates::apply` skipped unowned origins and targets), so a torch whose support broke in the next region never fell off | `NeighborWorld::notify_foreign` -> `Signal::NeighborUpdate` -> the owner calls `Region::remote_block_changed`; `Updates::remote_changed` re-validates only the owner's blocks and never forwards the remote origin back | `neighbors::tests::a_change_on_a_region_border_is_handed_to_the_owner_not_ignored`, `a_cascade_after_a_remote_change_is_forwarded_to_the_next_owner`, `a_remote_change_never_touches_blocks_beyond_the_owned_side`, `a_wall_torch_breaks_when_its_support_goes_in_the_same_region`; `region::neighbor_ownership_tests::*` |
| A gravity block next to a change in another region was never scheduled | remote origins feed `schedule_gravity` | `a_remote_change_schedules_gravity_for_an_unsupported_block` |
| A stack of sand fell only one block: the block above a vacated cell was not rescheduled | `tick_gravity` calls `schedule_gravity` for the cells it vacated | `a_gravity_stack_keeps_falling_and_wakes_the_fluid_around_each_vacated_cell` (fails without the fix; runs for a column in chunk 0 and chunk 1) |
| Cells vacated by gravity or by a failed support check did not wake adjacent fluid | `Region::wake_fluid` sends `Signal::FluidChange`, which the world loop routes to whichever region owns each neighbour | same test as above |
| `gravity::is_free` treated a waterlogged slab as a liquid and ignored replaceable blocks | now `isAir \|\| liquid block \|\| replaceable \|\| fire` | `waterlogged_blocks_are_not_free`, `replaceable_blocks_are_free_to_fall_into` |

## Audited, no change needed

- **Fluid flow across a region border.** `FluidWorld::set_block` hands a write into a loaded foreign chunk to its owner as
  `Signal::FluidPlace`; the owner applies it and schedules neighbour ticks. Covered by
  `fluid::tests::water_crosses_a_region_border_through_the_hand_off` and, for one region,
  `water_crosses_a_chunk_border_inside_one_region`.
- **Flow into unloaded chunks.** An unloaded column reads as stone and nothing is loaded or written
  (`fluid_tick_never_loads_unheld_neighbor`). Water at the edge does not resume when that chunk later loads; no
  neighbour update fires on load either. Not compared with vanilla chunk-load behaviour.
- **Waterlogging.** Only source water waterlogs (`FluidTicks::put`); waterlogged blocks are not liquid blocks and never
  tick (`is_liquid_block`). Lava reacts with waterlogged neighbours through `fluid_state`.
- **Crop growth.** `random_tick::growth_speed` and `crop_tick` match `CropBlock.getGrowthSpeed` and `randomTick`: 3x3
  farmland factor 1 or 3, `/4` off-centre, halved when both axes or any diagonal neighbour hold the same crop, light >= 9,
  one in `25/speed + 1`. Reads of another region's farmland are read-only.

## Still open

- Water does not wash away non-replaceable, non-solid blocks (redstone dust, torches, rails, buttons, plants):
  `can_hold_fluid` accepts replaceable and waterlogged-capable blocks only. Fixing it needs a drops and neighbor-update
  path for fluid-destroyed blocks, and the exact vanilla rule list was not available to check. Not changed.
- Random-tick growth exists only for wheat, carrots, potatoes, beetroots, torchflower, nether wart, sweet berries and
  cocoa. Stems, sugar cane, cactus, bamboo, kelp, saplings, vines and pitcher crops fall through to `no_op` in
  `random_tick::handlers`.
- Neighbor updates cover the six face neighbours only; redstone has its own `RedstoneChange` signal.
- A large explosion on a border sends one `NeighborUpdate` per border cell. The receiver is bounded by `UPDATE_LIMIT`
  (512) per tick.
