# Freebuff task 15: cat and parrot gameplay

Read `README.md` in this directory and the existing animal behavior in `crates/ferrite-entity`
before editing.

## Target and change

Implement the missing core cat and parrot behaviors as one animal-focused slice: player taming and
ownership, their supported feeding/breeding interactions, sitting/following behavior, and the
client-visible state events/metadata needed for those actions. First compare current source with
local 26.3 entity data and existing wolf/horse patterns; do not duplicate behavior already present.

## Ownership and constraints

Own `crates/ferrite-entity/src/tame.rs`, species-specific AI in `crates/ferrite-entity/src/ai.rs`,
and the narrow cat/parrot interaction and entity-event paths in `crates/ferrite-server/src/game/region.rs`.
Do not edit horse/equine inventory, villager/trader behavior, or plugin APIs. Preserve entity save
compatibility and deterministic test seams. Do not add Mojang code/assets.

## Acceptance

- Unit tests cover successful/failed taming, owner persistence, sit/follow transitions, breeding
  eligibility, and emitted tame/heart/state events for each species implemented.
- Server tests prove the corresponding interaction is routed and state survives entity save/load.
- Run focused tests, `cargo fmt --all -- --check`, and scoped clippy. Real client visuals remain
  unverified unless actually exercised.
- Commit the work, then run `cargo clean` in this worktree to remove its build cache. Send the
  final report with commit hash and exact checks, then stop; do not delete shared/global caches.
