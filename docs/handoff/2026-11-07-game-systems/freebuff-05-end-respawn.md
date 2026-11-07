# Freebuff task 05: End crystal dragon respawn sequence

Read `README.md` in this directory first.

## Target and change

Replace the immediate simplified dragon-respawn path with the staged End-crystal respawn
sequence: detect the four required crystals, show the verified beam/activation events, wait the
vanilla delay, restore spike crystals, and spawn the dragon once. Preserve saved fight state,
gateway count, and first-kill rewards. Do not attempt a full dragon AI/phase rewrite in this task.

## Ownership and constraints

Own `crates/ferrite-server/src/game/end_fight.rs`, the respawn-only portion of
`game/region.rs::run_end_fight`, and focused End-fight tests. Do not edit general portal travel,
gateway destination, piston, or villager code. Verify packet/event IDs from local 26.3 generated
data or existing protocol definitions.

## Acceptance

- State-machine tests prove the delay, event ordering, one-time spawn, interruption/restart
  behavior, and no respawn from an invalid crystal layout.
- A bot integration test covers placing the four crystals and observing the dragon reappear when
  the harness supports it; otherwise record that client sequence as unverified.
