# Freebuff task 08: dimension-safe leash handoff

Read `README.md` and `freebuff-02-cross-region-leads.md` first.

## Target and change

Fix cross-region leash holder resolution so a holder from another dimension is never treated as a valid local target. Make region handoff refresh the holder's position during the grace window, and make true holder removal expire without following stale coordinates. Keep the existing one-time leash break and lead drop behavior.

## Ownership and constraints

Own only leash holder resolution, its dimension/ownership signal path, and focused tests in `crates/ferrite-entity/src/ai.rs`, `crates/ferrite-server/src/game/region.rs`, and narrowly required entity-index interfaces. Coordinate before changing shared paths. Do not touch villager, vault, horse, wolf combat, fluids, pistons, End, or worldgen. No Mojang code/assets.

## Acceptance

- Test same-dimension region handoff by publishing a new owner position during the grace window.
- Test that a cross-dimension owner is rejected and that logout/removal does not follow stale position indefinitely.
- Verify exactly one leash break/lead drop across repeated ticks.
- Run focused tests, format and scoped clippy; commit and report exact evidence and runtime-only limits.
