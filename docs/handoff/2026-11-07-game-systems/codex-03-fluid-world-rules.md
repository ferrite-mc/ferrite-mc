# Codex task 03: dimension and source-conversion rules for fluids

Read `README.md` in this directory first.

## Target and change

Fix the server fluid adapter: `RegionFluidWorld::ultrawarm()` currently always returns false,
and production does not pass `lava_source_conversion` / `water_source_conversion` gamerules into
fluid updates. Make Nether lava behavior use the dimension, and make source creation respect each
world's configured gamerule. Preserve current unloaded/unowned-chunk no-load behavior.

## Ownership and constraints

Own `crates/ferrite-server/src/game/fluid.rs` and its module-local tests; only the fluid tick
closure in `crates/ferrite-server/src/game/mod.rs`; only `Region::tick_fluids` in
`crates/ferrite-server/src/game/region.rs`; and the narrow fluid source-conversion API/logic/tests
in `crates/ferrite-fluid/src/lib.rs`, `src/tick.rs`, and `tests/interactions.rs`. Pass the
`TickContext` gamerule values through the production call; do not edit other `mod.rs` or
`region.rs` behavior. Do not alter general fluid-cauldron semantics or redstone behavior.

Also fix the current CI clippy failure in `ferrite-fluid/src/lib.rs` (`clippy::collapsible_if` in
the cauldron conversion branch) without changing behavior or weakening lints. This CI issue is
inside the owned fluid files and must be clean under `cargo clippy --workspace --all-targets -- -D
warnings`.

## Acceptance

- Tests cover Overworld vs Nether lava scheduling/flow and water/lava source-conversion rules in
  both enabled and disabled states; disabling water-source conversion must prevent two-source
  water generation while preserving ordinary source flow.
- Existing region-boundary/no-load tests stay green.
- Run focused server fluid tests, format check, and scoped clippy. Do not stage or commit; this
  subagent shares the coordinator checkout.
