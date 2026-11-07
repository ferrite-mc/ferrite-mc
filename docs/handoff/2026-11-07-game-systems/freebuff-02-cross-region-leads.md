# Freebuff task 02: cross-region leads

Read `README.md` in this directory first.

## Target and change

Make leash behavior work when the holder and leashed mob are owned by different regions. Today
`leash_pull` can resolve only a player or entity in the same local region store. Reuse the existing
cross-region ownership/lookup patterns (including `game/ride.rs`) so a mob follows a live holder,
breaks the lead at the configured snap distance, and does not retain a stale holder after removal.

## Ownership and constraints

Own `crates/ferrite-entity/src/ai.rs` leash behavior and the leash-holder lookup/refresh path in
`crates/ferrite-server/src/game/region.rs`. Do not edit trader spawn, villager trade, or piston
functions in `region.rs`. Keep messages/commands routed through region ownership; do not read or
mutate another region's `EntityStore` directly.

## Acceptance

- Regression coverage for same-region follow, cross-region follow, cross-region snap, and holder
  removal/transfer without duplicate lead-break events.
- Preserve current caravan chain limit and same-region behavior.
- Run focused entity and server tests; explain any runtime behavior that cannot be tested without
  multiple live regions.
