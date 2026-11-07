# Freebuff task 09: concurrent villager trade menus

Read `README.md` and `freebuff-03-villager-trading.md` first.

## Target and change

Fix stale offer synchronization when two players have the same villager's trade screen open. A sale from either screen must not restore older uses/demand, permit overselling beyond the offer limit, or lose the correct player's reputation. Preserve player-adjusted offer pricing and save/load behavior.

## Ownership and constraints

Own only villager menu synchronization/validation and focused tests in `crates/ferrite-server/src/game/region.rs`, `game/container.rs`, and related villager code. Coordinate before changing shared integration points. Do not touch wandering trader, vault, horse, leash, wolf combat, fluids, pistons, End, or worldgen. Do not introduce undocumented global player state.

## Acceptance

- Regression test opens two merchant snapshots, completes a sale through one, then verifies the second cannot overwrite newer uses/demand or oversell; verify per-player gossip is credited correctly.
- Cover save/load where practical.
- Existing villager merchant, demand, restock, level-up, and persistence tests remain green.
- Run focused tests, format and scoped clippy; commit and report exact evidence and remaining runtime limits.
