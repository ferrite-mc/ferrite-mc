# Freebuff task 04: trial vault activation and use

Read `README.md` in this directory first.

## Target and change

Complete the trial-vault lifecycle. The current item-use path awards a matching key only when NBT
already says the vault is `active`, and tracks one reward per player, but no server path activates
an inactive vault. Add player-range activation, the active/unlocking/ejecting state transitions,
client world events, and the documented cooldown needed to make generated vaults usable. Preserve
the per-player once-only reward invariant and do not consume a key for an invalid/inactive/busy
attempt.

## Ownership and constraints

Own `crates/ferrite-server/src/game/vault.rs`, `game/blocks.rs::use_vault`, and a dedicated vault
tick/use hook in `game/region.rs`. Use protocol/data definitions already present in the repository
to select world-event IDs and timings; do not guess packet or event constants.

## Acceptance

- Tests cover activation by an eligible nearby player, range rejection, active use, one reward per
  player, busy/cooldown behavior, key consumption, and persisted state across reload.
- Add a bot/session integration test if the current harness can observe the relevant block events;
  otherwise document the exact untested client-visible sequence.
