# Codex task 01: tamed wolf owner-defense targeting

Read `README.md` in this directory first.

## Target and change

When a player attacks a mob, a standing tamed wolf owned by that player should acquire that mob
as a target and defend its owner. Preserve the current behavior that wolves defend an owner from
hostile mobs and do not attack while sitting. Do not expand this task to new tameable species,
random anger durations, or combat-critical-hit modifiers.

## Ownership and constraints

Own `crates/ferrite-entity/src/tame.rs` and its module-local tests only. The coordinator will wire
server events if the existing event interface is insufficient; report the smallest required API
change before editing outside this file.

## Acceptance

- Unit tests cover owner-attacked target acquisition, out-of-range rejection, sitting wolves, and
  owner isolation.
- Run `cargo test -p ferrite-entity` and formatting checks. Do not stage or commit; this subagent
  shares the coordinator checkout.
