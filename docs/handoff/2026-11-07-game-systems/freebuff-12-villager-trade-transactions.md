# Freebuff task 12: authoritative villager trade transactions

Read `README.md`, `freebuff-03-villager-trading.md`, and `freebuff-09-villager-trade-handoff.md` first.

## Target and change

Make villager sales authoritative across multiple open menus and regions. Reconcile each accepted sale exactly once without stale snapshots rolling inventory backward, duplicate gossip, or undercounting two concurrent sales. Keep player-specific reputation pricing, demand, restock, offer validation, persistence, and region routing consistent.

## Ownership and constraints

Own villager trade validation/commit, menu synchronization, and their focused tests in villager entity code and `crates/ferrite-server/src/game/region.rs`, `game/mod.rs`, and `game/container.rs`. Coordinate before shared integration edits. Do not touch wandering traders, vault, equines, leash/combat, fluids, pistons, End, or worldgen.

## Acceptance

- Regression tests open independent menus for two players and prove two accepted sales both count once, stale snapshots cannot undo them, and each player's gossip is credited correctly.
- Cover nearly sold-out offers, dimension/region handoff, and save/load where practical.
- Existing price, merchant, demand, restock, level-up, and persistence tests remain green.
- Run focused tests, format, scoped clippy; commit and report any remaining transaction-ordering limit.
