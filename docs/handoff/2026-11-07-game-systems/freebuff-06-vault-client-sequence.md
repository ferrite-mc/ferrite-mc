# Freebuff task 06: vault packet sequence regression

Read `README.md` and `freebuff-04-vault.md` in this directory first. This is the follow-up task for the same vault worker session after commit `667bf3e`.

## Target and change

Verify the server-facing packet and item sequence when a player activates and unlocks a trial vault. Add a focused in-process regression that observes the key consumption, state/level events, and reward spawn exactly once per eligible player. Fix any concrete mismatch the test exposes in the vault path.

## Ownership and constraints

Own the vault-only paths in `crates/ferrite-server/src/game/vault.rs`, `game/region.rs`, and a focused server test module. Touch a shared integration test file only if there is no narrower test seam. Preserve the existing vault state machine and other workers' changes. Do not edit trading, leads, piston, End, fluid, or world generation code. Do not use a real client result as proof unless one was actually run.

## Acceptance

- A test observes activation, successful unlock, key consumption, ejection, and a second player's independent eligibility through the actual game tick/packet path where practical.
- Duplicate interaction and reload do not produce an extra reward or consume an extra key.
- Report exact tests, packet/event evidence, any remaining client-only check, and commit hash. Send `worker_done` for this new dispatch.
