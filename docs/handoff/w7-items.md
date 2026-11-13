# W7-3 handoff: lossless item, loot and block-entity preservation

Branch `w7-items`, based on `236f0e2`, reconciled with `origin/main` (maps #8, API #7, AI #9,
net-validation #10/#11). Brief: `docs/handoff/w7-items-brief.md`.

No actual-client visuals were checked. Every claim below is backed by unit, codec, bot
(protocol-level) or Java tests.

## Loss paths found and fixed

| Path | Before | Now |
|---|---|---|
| Item entities (drop, Q, spill, trident, death, hopper, dispenser) | stack flattened to item id + count | `ItemState.components` carries the patch; merge requires equal components; pickup returns the full stack |
| Entity save/load of item entities | components dropped | full stack saved and loaded |
| Death drops given to plugins | plugin saw material/amount and the game re-spawned plain stacks | untouched drops get their own stack back (`game/item_keep.rs`), changed/added drops are plain |
| `ItemStack::to_nbt().ok()?` callers | one unsaveable component dropped the whole stack | only the component with no save form is dropped, with a warning |
| Container component | opaque after load, no network form | typed `Component::Container`, NBT + wire; malformed saved data stays opaque and is written back verbatim |
| Books | raw network bytes | typed `WritableBookContent` / `WrittenBookContent`, NBT + wire |
| Block container slots | editing one slot lost unsendable components of the others | untouched slots keep their original stack |
| Shulker box break | contents spilled loose, box item empty | contents travel in the item (`copy_components` from the block entity), creative breaks drop the box, placing restores contents |
| Explosion / neighbour / gravity / piston removal | loot ignored the block entity; chest, furnace, brewing stand, jukebox contents lost | one helper (`Region::spawn_block_drops`) rolls loot with the box's components and spills other containers' contents |
| Loot `copy_components` | not implemented | `source: block_entity` with include/exclude (all 17 vanilla shulker tables tested) |
| Java PDC | primitives only, nested types threw | `TAG_CONTAINER`, `TAG_CONTAINER_ARRAY`, typed `LIST` (nested), deep-copied |
| Java books | `setItem` of a book threw | title, author, generation, pages travel as one JSON field |

## Behaviour that is deliberately not changed

* Nested shulker boxes are refused (hopper/dropper/inventory) and, when an item carrying one is
  placed, the inner stack is returned to the world as an item instead of deleted.
* `copy_components` for any `source` other than `block_entity` copies nothing and keeps the item.
* The `lock` key is read as `lock` or `Lock`.

## Test evidence

Commands (Linux, `RUSTUP_TOOLCHAIN=stable`, Java 25, Gradle fixtures built first):

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
(cd java/ferrite-api-impl && ./gradlew build shadedJar :test-plugin-typical:jar)
FERRITE_JAVA_PREBUILT=1 cargo test --workspace --no-fail-fast
```

New or extended tests:

* `ferrite-loot/tests/copy_components.rs` (6): include/exclude, no block entity, unsupported source,
  tuple `roll` unchanged, all 17 vanilla shulker tables.
* `ferrite-item/tests/containers_books.rs` (9): wire, sparse save format, saved-then-sent,
  nesting, malformed (kept verbatim), truncated/oversized wire, Unicode books, empty book.
* `ferrite-entity` item tests (+5): components merge/pickup rules.
* `ferrite-server`: shulker unit tests (10: all colours, full/empty, nested refused, out-of-range
  slots returned, malformed container, name/lock), explosion over all 17 shulker blocks, removal
  update (empty and full box), chest spill-once, block container slot edits, hopper pickup with
  components, entity save with components, creative shulker break + place end to end through a bot.
* Plugin bridge: `item_pdc` nested/typed/malformed/depth tests, book field parse/limits/Unicode,
  `with_meta` book components through save + wire.
* Java: `ItemPdcNestingTest` (8), `BookMetaBridgeTest` (5), existing `PlayerExtrasTest` updated
  (nested containers used to be refused).

## Limitations (not implemented, stated precisely)

1. **Plugin view of a slot is a snapshot of material and amount.** `setItem(slot, getItem(slot))`
   writes a plain stack, so a plugin that re-sets an item it read loses that item's components.
   Fixing it needs host-to-Java item metadata (the bootstrap snapshot carries none) and is not
   done. Writing metadata the plugin set (name, lore, enchantments, flags, PDC, books) works.
2. Item entities' synched metadata is still a plain stack, so the client's view of a ground item
   has no components until it is picked up (server state is correct).
3. Components with no typed model have no NBT form; they are dropped from a saved stack with a
   warning naming the component, never the whole item.
4. A dispenser placing a shulker box is not handled; Bukkit `World#dropItem` and
   `PlayerDeathEvent#getDrops` give plugins material/amount only.

## Shared functions changed

`Player::break_block`, `Region::drop_broken_blocks`, `Region::place`, new
`Region::drop_creative_shulker`, `Region::spawn_block_drops` (now takes the chunk source),
`spawn_thrown`, `land_tridents`, `process_death`, `Containers` (`broken_entities`),
`block_container::{load_items, store, store_slots, save_items, take_all_contents}`,
`hopper::suck_up`, dispenser item spawn, `drops::{block_drop_stacks, block_drops}`, `entity_save`,
the `ferrite-entity` item API, `ferrite-item` `Component` additions and `wire.rs`,
`PlayerCommand::ItemMeta` / `Command::ItemMeta` / `ItemMetaFields` (new `book` field),
`plugins/inventory.rs` (`item_pdc`, new `item_book`), Java `FerriteDataContainer`,
`FerriteItemMeta`, `FerritePlayerInventory.write`.

## Housekeeping

* Disk: removed my own finished cargo targets (`target-rw1`, `rw2`, `rw2b`, `rw3`), 39 GB;
  free space went from 5.8 GB to 45 GB. `target-rw4` is in use.
* rustc 1.100 (released 2026-11-09) adds a `needless_range_loop` finding in
  `ferrite-inventory/src/menu.rs` (4 loops, not touched by this branch). CI was green on `main`
  at the time of writing, so it does not block yet; it will once the runner image updates.
* `.github/workflows/release.yml` uses unpinned third-party actions with broad permissions
  (not changed here, release workflow is lane 4's).

## Performance

Recorded after merge, see the section appended by the performance phase.
