# Freebuff task 07: trial-vault reward delivery lifecycle

Read `README.md` in this directory first. This task follows `freebuff-04-vault.md` and the
client-sequence fix in `freebuff-06-vault-client-sequence.md`.

## Target and change

Make trial-vault reward delivery reliable across chunk unload, region handoff, restart, and
duplicate interaction. The invariant is: every reward rolled for one unlock is delivered exactly
once, and a rejected or duplicate interaction never consumes a key or claims a reward.

The current implementation defers the `items_to_eject` ledger write until after the region spawns
the item entity. That removes the loss window before the spawn, but it is not idempotent: an
autosave or unload can persist the spawned entity while the block entity still contains the old
ledger, and a restart then ejects the same reward again. Chunk-unload and region-split coverage is
also missing.

Add a durable delivery token to each pending reward:

- Store the token beside the item stack in `items_to_eject`, and use it as the spawned item
  entity's UUID.
- When a region resumes an `ejecting` vault, skip ledger entries whose token already exists in the
  region's entity store. Remove those stale ledger entries without spawning or emitting another
  `VAULT_EJECTS_ITEM` event.
- Keep the ledger write after the spawn. If the ledger write fails, remove the just-spawned item
  entities so the reward remains pending instead of being duplicated on a later tick.
- Preserve the existing atomic interaction check: exactly one concurrent key use starts the
  unlock; all other attempts are refused without consuming the key.

## Ownership and constraints

Own `crates/ferrite-server/src/game/vault.rs`, the vault block in
`crates/ferrite-server/src/game/region.rs`, the focused `vault.rs` test module, and this document.
Do not edit `game/integration.rs` or unrelated region systems. Do not change the vanilla 26.3
level-event payload: `VAULT_EJECTS_ITEM` remains event 3017 with data 0; the item id is carried by
the item entity.

The token is server-side persistence data. Keep the item stack part of the ledger decodable by
`ItemStack::from_nbt`, and do not add Mojang code or assets.

## Acceptance

Add lifecycle tests in `vault.rs` that prove:

- A chunk unload pauses an ejecting vault, leaves its timer and ledger intact, and a reload
  delivers the already-spawned items plus the remaining ledger exactly once.
- A region split moves the vault to the receiving region, only that region ticks it, and a merge
  back does not duplicate rewards.
- A restart after an item entity and the stale ledger were both persisted reconciles by token:
  the spawned entity is not re-ejected, while every unspawned reward still is.
- Concurrent duplicate key uses produce one `Unlock::Started`, one claimed player, one reward
  ledger, and one set of ejected items.
- Existing activation, cooldown, key-consumption, item-id, and restart tests continue to pass.

Run `cargo test -p ferrite-server --lib game::vault`, `cargo fmt --all -- --check`, and scoped
`cargo clippy -p ferrite-server --lib --no-deps -- -D warnings`. Report the commit hash and any
remaining persistence limitation. A real Minecraft client is still required to verify the
client-visible shutter animation; this task does not claim that visual check.
