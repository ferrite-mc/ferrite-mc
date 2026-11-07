# Freebuff task 10: equine gear drops on death

Read `README.md` and `freebuff-07-horse-bucking.md` first.

## Target and change

When a horse dies, drop its equipped saddle and horse armour exactly once. Preserve existing donkey/mule/llama chest, inventory, and saddle drops; do not make llamas drop horse armour. Keep baby-equine and loot-table behavior correct.

## Ownership and constraints

Own only equine death-drop selection and its focused tests in `crates/ferrite-server/src/game/region.rs` and, if useful, a narrow helper in `game/equine_chest.rs`. Do not change horse taming, movement, trading, vault, leads, fluids, pistons, End, or worldgen. Do not stage unrelated changes.

## Acceptance

- Tests cover saddled horse, each supported horse armour tier, unsaddled/unarmoured horse, baby horse, and one-time death handling.
- Existing chest/saddle drops for pack equines remain unchanged.
- Run focused tests, `cargo fmt --all -- --check`, and scoped Clippy; commit and report exact evidence.
