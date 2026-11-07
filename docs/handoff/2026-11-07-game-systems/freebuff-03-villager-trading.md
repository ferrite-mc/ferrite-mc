# Freebuff task 03: villager trade reputation

Read `README.md` in this directory first.

## Target and change

Add player-specific villager reputation pricing. Existing trade use/demand synchronization,
restock timer, trade XP, and profession-level thresholds are already implemented; do not rewrite
them. Persist villager gossip/reputation with the villager and apply the resulting price adjustment
when that villager's offers are shown and validated. Use the existing trade `price_multiplier` and
offer model where possible. If a required vanilla rule cannot be grounded in local data or a
verified reference, stop at the boundary and report the missing fact instead of guessing.

## Ownership and constraints

Own `crates/ferrite-entity/src/villager.rs`, villager serialization in
`crates/ferrite-server/src/game/entity_save.rs`, and the villager-only paths in
`crates/ferrite-server/src/game/region.rs` / `game/container.rs` that open and synchronize trade
offers. Do not change wandering trader offers or vault behavior.

## Acceptance

- Tests cover neutral and positive/negative reputation pricing, player isolation, persistence over
  save/load and region handoff, and trade-result validation using the displayed price.
- Existing demand/restock/level-up tests remain green; add no undocumented global player state.
