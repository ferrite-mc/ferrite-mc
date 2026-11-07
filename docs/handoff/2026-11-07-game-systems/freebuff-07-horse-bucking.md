# Freebuff task 07: timed horse bucking behavior

Read `README.md` first. This follows trader commit `6304839` in the same Freebuff session.

## Target and change

Replace the per-tick probability approximation documented in `crates/ferrite-entity/src/horse.rs` with a timed run-around bucking state for wild mounted equines. Preserve taming chance, temper growth, rider ejection, and tame-horse controls. Ground timing and behavior in the local 26.3 vanilla data or a verified reference; if exact timing cannot be established, report that boundary and implement only the part supported by evidence.

## Ownership and constraints

Own horse behavior in `crates/ferrite-entity/src/horse.rs` and the narrow equine tick/riding call path in the server if needed. Do not touch trader, wolf, leads, villager, vault, fluid, piston, End, worldgen, or plugin code. Coordinate before editing shared `game/region.rs`; other workers have changes there. No Mojang code or assets.

## Acceptance

- Regression tests cover timer start, continued bucking, ejection/taming at the proper point, and no bucking after tame or dismount.
- Existing equine behavior tests pass; run focused tests, format check, and scoped Clippy.
- Commit the result and report exact verification and remaining client-only behavior in `worker_done`.
