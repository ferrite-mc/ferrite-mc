# Freebuff task 01: wandering trader eligibility and despawn

Read `README.md` in this directory first.

## Target and change

Complete the server-side wandering-trader spawn and lifetime rules. Current code already checks
`spawn_wandering_traders`, performs the daily chance roll, searches for a ground position with two
air blocks, and assigns a 48,000-tick lifetime. Keep those behaviors. Add the missing biome/light
eligibility checks to the candidate spawn and despawn a trader and its trader llamas when the
trader drinks invisibility or milk; preserve normal timed despawn.

## Ownership and constraints

Own `crates/ferrite-server/src/game/region.rs`, limited to `tick_trader_spawn` and the trader
lifetime/despawn callsite. Do not edit the leash or caravan logic in `ferrite-entity/src/ai.rs`.
Use existing biome, light, and effect state APIs; do not invent registry IDs or event codes.

## Acceptance

- Tests prove valid/invalid biome and light candidates, gamerule-off behavior, normal timeout, and
  invisibility/milk despawn including attached trader llamas.
- Failed spawn candidates do not consume a successful-spawn roll or create partial entities.
- Run focused server tests and report any branch/region lifecycle limitation.
