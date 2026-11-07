# Codex task 02: piston state and entity movement regressions

Read `README.md` in this directory first.

## Target and change

Fix two source-confirmed piston issues: (1) coalesce or invalidate delayed block updates so a
retraction within the two-tick settle window cannot be overwritten by an older extension update;
(2) do not push an entity merely because the fixed piston base appears as a non-air edit in the
movement operation. Move entities only for the actual moved blocks/head according to the plan.

## Ownership and constraints

Own `crates/ferrite-server/src/game/piston.rs`, the piston action/delayed-update portion of
`game/region.rs`, and piston tests in the piston module. Do not edit `game/integration.rs` or other
workers' feature functions. Moving-piston visual animation still requires a real client and is not
part of this code-only acceptance.

## Acceptance

- Regression test for extension followed by retraction before settle, asserting clients/world end
  with the latest state.
- Regression test proving an entity beside/above the fixed base is not spuriously pushed while an
  entity in the moved block is pushed.
- Run focused piston/server tests and update `docs/pistons.md` to match verified behavior.
- Do not stage or commit; this subagent shares the coordinator checkout.
