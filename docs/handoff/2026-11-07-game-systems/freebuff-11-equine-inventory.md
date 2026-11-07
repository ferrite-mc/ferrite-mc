# Freebuff task 11: complete equine inventory lifecycle

Read `README.md`, `freebuff-07-horse-bucking.md`, and `freebuff-10-horse-gear-drops.md` first.

## Target and change

Audit and complete the player-facing inventory lifecycle for horses, donkeys, mules, and llamas: open/close rules, saddle/body gear slots, pack slots, item movement and validation, persistence across chunk unload/region transfer/reload, and correct contents when the animal dies. Preserve entity ownership/taming rules and prevent item duplication or loss.

## Ownership and constraints

Own equine-specific paths in `crates/ferrite-entity/src/horse.rs`, `crates/ferrite-server/src/game/equine_chest.rs`, `game/region.rs`, and `game/entity_save.rs`. Coordinate before editing shared region methods. Do not edit villager/trader, vault, leash, fluids, pistons, End, or worldgen.

## Acceptance

- Tests cover every equine family, gear-slot validation, pack inventory persistence, chunk/region handoff, death drops exactly once, and invalid/duplicate item moves.
- Existing taming, mount, saddle, armour, and chest tests remain green.
- Run focused tests, format, and scoped clippy; commit and report evidence and client-only limits.
