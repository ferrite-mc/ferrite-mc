# W7 lane 2 — persistent maps and the cartography table

Branch `w7-maps`, based on origin/main `236f0e2abae1a9ecc2da11008d2d224aa73a0611` (PR #6).
PR, head and merge SHA are recorded in "Delivery" at the end.

## Architecture

| Piece | Where | Notes |
|---|---|---|
| `minecraft:map_id` as a typed component | `ferrite-item` (`Component::MapId`, wire varint, NBT int) | Was opaque (`Raw`/`Opaque`). Round-trip tests in `wire/tests.rs` and `component.rs`. |
| Stack helpers | `ferrite-item/src/map.rs` | `map_id`, `set_map_id`, `map_post_processing` (network-only marker kept as its 1-byte payload). |
| Saved map model + store | `ferrite-server/src/game/map.rs` | NBT shape read from the 26.3 server codecs (below). |
| Cartography menu | `ferrite-inventory/src/menu.rs` (`MenuKind::Cartography`, `Menu::cartography`, `MapAccess`) | Pure state machine; the server supplies the store through `MapAccess`. |
| Store adapter + held-map packets | `ferrite-server/src/game/map_menu.rs` | `StoreMaps` implements `MapAccess`; `Player::send_held_maps`. |
| Open from block / plugin | `game/container.rs` (block use, `open_plugin_station("cartography")`), `plugins/inventory.rs` (`openCartographyTable`) | |
| Host ops | `plugins/maps.rs`, `plugins/inventory.rs` | See "Host operations". |
| Java | `FerriteMapView`, `FerritePlayerInventory` | |

### 26.3 storage facts (read from `server-26.3.jar` codecs with `javap`; nothing copied)

* Maps live in the **server-level** saved-data storage: `data/minecraft/maps/<id>.dat`, gzip NBT
  `{data: {...}, DataVersion}`; fields `dimension` (string), `xCenter`/`zCenter` (int), `scale`
  (byte, default 0), `colors` (byte[16384]), `trackingPosition` (default true),
  `unlimitedTracking` (default false), `locked` (default false), `banners`
  (`{pos:int[3], color, name?}`), `frames` (`{pos:int[3], rotation, entity_id}`).
* Allocator: `data/minecraft/maps/last_id.dat`, `{data: {map: int}}`, default `-1`, so the first id
  is `0`. (The pre-26 `idcounts.dat` no longer exists.)
* `scaled()` is a blank `createFresh` one level out around the same centre; `locked()` copies
  pixels and banners (not frames). Cartography: paper zooms out an unlocked map below scale 4, a
  glass pane locks an unlocked map, an empty map clones (count 2, same id, no new data). The new
  data is created when the result is *taken* (a `map_post_processing` marker is on the preview).

### World namespaces (explicit decision)

* Overworld, nether and end share one id space and one store, as in vanilla.
* Runtime worlds use the **same** server-wide store (registered with `game::set_map_server_root`
  from `main.rs`). A map item that travels between worlds therefore never resolves to a different
  map and ids cannot collide. The world a map belongs to is the map's own `dimension` string
  (`minecraft:...` or the runtime world's key); clone, zoom-out and lock preserve it. Tested in
  `map.rs::a_runtime_world_keeps_its_own_dimension_identity` and
  `map_menu.rs::zooming_out_creates_a_blank_larger_map_and_keeps_the_source`.
* Without a registered root (tests) each world directory gets its own store.

### Allocation safety

One mutex per store; map files are created with `create_new`, so an id whose file exists is skipped
and never overwritten; the counter is written after the map file and, on open, is
`max(counter, highest <id>.dat)`, so a lost or damaged counter cannot reuse an id. A damaged map
file is an error, is left untouched, and the cartography table offers no result for it.

### Host operations (new; no existing operation changed)

* `Server.allocateMap(String,int,int,int,boolean,boolean)` → id. Used by `FerriteMapView`.
* `Server.updateMap(int,String,int,int,int,boolean,boolean,boolean)` → "". Written by every
  `MapView` setter; keeps pixels and banners.
* `Player.setItemMapId(int,int)` → points the filled map in a slot at a saved map. Kept separate
  from the lane-3 `setItemMeta` string on purpose.

## Verified behaviour (tests)

* Codecs: `ferrite-item` wire + NBT round trip for `MapId`; helper tests incl. wire marker.
* Model/store (`game/map.rs`): NBT field names/types, defaults, malformed rejection, restart, lost
  or damaged counter, squatting file, concurrent allocation (8 threads; two stores on one
  directory), `release`.
* Menu (`ferrite-inventory/src/cartography_tests.rs`): slot rules, each recipe, locked, max scale,
  unknown map, take conserves inputs, busy cursor, store failure refuses the take, shift-click of
  the result (one allocation; id released when the inventory is full), shift-click of inputs, close
  returns inputs.
* Real menu against a real on-disk store (`game/map_menu.rs`): zoom-out blank/larger, lock copies
  pixels, clone allocates nothing, restart, damaged source, full-inventory release, concurrent
  takers get distinct maps, map packet contents.
* Host ops: `plugins/maps.rs`, `plugins/inventory.rs`, region `with_map_id`; Java `FerriteMapIdTest`.

## Not done / limits

* **No terrain rendering.** The block → map-colour table is not in the extracted vanilla reports
  and I did not copy one out of the jar. Fresh, scaled and explorer maps are blank; only stored
  colours are sent (`Player::send_held_maps`: full 128×128 packet on first hold and whenever the
  saved-data object is replaced). Nothing writes pixels.
* Using an empty map item (right click) to create a filled map is not implemented.
* Banner/frame/player decorations are stored but not sent (empty decoration list).
* No bot end-to-end test of opening the table from a block or through `Player`: a test `Player`
  cannot be built and the bot's `ItemStack` has no components, so a filled map cannot be injected.
  The block-open path reuses `open_menu` like the loom.
* Java→Rust carries only `map_id`; reading an inventory item back into Java still has no item
  meta, so a filled map read from a Java `Inventory` loses its view.
* An id allocated for a shift-click that then cannot move is released only if nothing was allocated
  after it; otherwise it remains an unused map file.
* Cross-region: the store is one short mutex shared by all regions; no region waits on another.
  Reopen reads the same write-through files.
* Real-client rendering is unverified.

## Test evidence

Local, before the PR: `cargo fmt`, clippy `-D warnings`, `ferrite-item` 43, `ferrite-inventory` 71,
`ferrite-server` lib 867 (one full-run failure of
`game::integration::four_crystals_respawn_the_dragon_through_the_full_sequence` under host load; it
passes alone in 102 s), Java `FerriteMapIdTest` 3 and `FerriteMerchantMapTest` 4.

## Delivery

* PR: https://github.com/ferrite-mc/ferrite-mc/pull/8, squash-merged as `3ec1678` (head `1a53087`).
* Required checks on that exact head: `test (ubuntu-latest)` and `test (windows-latest)` passed
  (run 43292870142). CI on the merged commit finished successfully.
* Three CI problems were found and fixed on the way, none by skipping tests:
  the Java host stub answered `allocateMap` with an empty string (`FerriteMapView` now falls back
  to a local id and three call-counting tests ignore the allocation call);
  stable clippy moved to 1.100 and flagged six index loops in `menu.rs` (rewritten as iterators);
  and a Windows run lost the connection in `runtime_world_gap7_create_edit_enter_leave_unload_reopen`,
  which passed in the next full run and, in the AI PR, was traced, for the AI PR, to an unrelated change of mine rather
  than to maps.
* Performance (follow-up branch `w7-ai-matrix`): `Player::send_held_maps` runs for both held
  stacks of every player on every tick. It cloned each stack and read the id through
  `ItemStack::effective_component`, which for an item without an explicit `map_id` decodes that
  item's whole default-component table from NBT. `map_id` now reads the stack's own patch by
  reference (no item has a default `map_id`; pinned by `no_item_has_a_default_map_id`).
  Measured with `cargo test --release -p ferrite-item map_id_read_cost -- --ignored --nocapture`
  (three runs, same host, 200 000 calls each):

  | stack | before | after |
  |---|---|---|
  | plain item (e.g. diamond sword) | 10.1-10.3 us | 0.7 ns |
  | filled map | 72-77 ns | 2.3-2.8 ns |

  This is a micro-benchmark of the id read only, not an end-to-end tick measurement; the 800-bot
  A/B belongs to lane 4.
