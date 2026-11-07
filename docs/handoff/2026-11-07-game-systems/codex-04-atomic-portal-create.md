# Codex task 04: atomic portal creation at chunk boundaries

Read `README.md` in this directory first.

## Target and change

Make `game/portal.rs::create_portal` preflight every chunk in its complete read/write footprint
before the first block mutation. If a required chunk is missing, return the existing failure
outcome without leaving any partial frame or portal blocks. Keep the existing geometry and
coordinate scaling unchanged.

## Ownership and constraints

Own `crates/ferrite-server/src/game/portal.rs` and its module-local tests only. Do not change
`region.rs` prefetch behavior or other dimension state. Do not make the helper load chunks itself.

## Acceptance

- Anvil-backed tests place the portal center on both positive and negative chunk boundaries;
  with a required neighbor missing, failure leaves all existing blocks unchanged.
- After all footprint chunks are loaded, creation writes the complete frame/portal successfully.
- Run focused portal/dimension tests and format checks. Do not stage or commit; this subagent shares
  the coordinator checkout.
