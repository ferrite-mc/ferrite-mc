# Plugin B — Generic Living-Entity Equipment & Persistence (`yxshadxw/plugin-b-entity-equipment`)

## Summary

Implemented generic six-slot `EntityEquipment` (`HAND`, `OFF_HAND`, `FEET`, `LEGS`, `CHEST`, `HEAD`) across the Rust entity/tracker/persistence layers, the host contract (`entity.equipment_set_async` + `entity.state_get` `"equipment"`), the Bukkit/Paper API (`FerritePlayer#getEquipment()` and `FerriteEntitySnapshot` `LivingEntity#getEquipment()`), and the NMS `HostStateReader` snapshot parser.

## Ownership & Thread-Safety Rules

- **Players (`FerritePlayerEquipment`)**: Delegates all six slots (`HAND`, `OFF_HAND`, `FEET`, `LEGS`, `CHEST`, `HEAD`) directly to `FerritePlayer#getInventory()` (`FerritePlayerInventory`), which enforces main-thread access and routes async writes through `player.inventory_set_async` -> coordinator -> owning region `worlds.schedule_at_world_chunk` -> `player.apply_host_inventory`.
- **Non-player living entities (`FerriteEntityEquipment`)**:
  - Reads return defensive copies of the snapshot's six slots (`getItemInMainHand()` / `getItemInOffHand()` return `Material.AIR` when empty; armor getters return `null` when empty).
  - Setters update the local snapshot and route asynchronously through `Bootstrap.entityEquipmentSetAsync` (`entity.equipment_set_async`).
  - `WorldStateProvider::entity_equipment_set_async` validates the UUID, slot name (`HAND`/`OFF_HAND`/`FEET`/`LEGS`/`CHEST`/`HEAD`), and item patch, verifies the entity exists in `EntityIndex` / `entities` / `players` (returning `entity_not_found` otherwise), and queues `QueuedEntityEquipmentWrite`.
  - On `WorldStateProvider::tick` (`drain()`), queued writes invoke `EntityEquipmentSyncFn` -> coordinator `equip_sync_rx` -> `worlds.schedule_at_chunk(dimension, chunk, ...)` -> `ferrite_entity::mob::set_equipment_stack` (or `data.equipment.set` for non-mob living entities).
  - No direct cross-region live state reads or synchronous cross-region waits occur.

## Tracker & Persistence Format

- **`crates/ferrite-entity/src/equipment.rs`**: Added a `dirty: u8` bitmask (`PartialEq` ignores `dirty`) with `is_dirty()`, `clear_dirty()`, and `take_dirty_wire_slots()` (emitting `ItemStack::empty()` when a slot is cleared).
- **`crates/ferrite-entity/src/mob.rs`**: Added `mob::set_equipment_stack` (and routed `mob::set_equipment` through it) to keep `MobState::equipment` and `EntityData::equipment` synchronized and mark the modified slot dirty.
- **`crates/ferrite-entity/src/tracker.rs`**:
  - On viewer pairing start, merges `data.equipment` with `mob.equipment` defaults (preserving skeleton bow and horse saddle/body defaults).
  - On `sync`/tick, broadcasts `ClientboundSetEquipmentPacket` (`SetEquipment`) to tracking viewers when `data.equipment.is_dirty()`, and clears dirty bits.
- **`crates/ferrite-server/src/game/entity_save.rs`**:
  - `entity_to_tag` merges `data.equipment.to_nbt()` with `mob.equipment` fallback so full `ItemStack` NBT (including components like `CustomName`) persists for mobs as well as non-mob entities.
  - `entity_from_tag` restores `data.equipment = Equipment::from_nbt(eq)` (with `clear_dirty()`) and syncs `mob.equipment` item IDs.
- **`crates/ferrite-server/src/game/entity_index.rs`**: `EntityIndex` now tracks `equipment: BTreeMap<Uuid, ferrite_entity::Equipment>` synced from `data.equipment` + `mob.equipment` so `entity.state_get` returns live equipment without cross-region access.

## Remaining Limitations & Explicit Refusals

- **Unsupported slots & drop chances**: `EquipmentSlot.BODY` / `EquipmentSlot.SADDLE` and all `*DropChance` accessors (`getDropChance`, `setDropChance`, `getHelmetDropChance`, etc.) throw `UnsupportedOperationException` via `Unsupported.exception(...)`.
- **Java non-player item NBT**: `FerriteEntityEquipment#routeHostEquipmentWrite` currently serializes `{id, count}` for non-player entity equipment writes (full NBT persistence is covered on the Rust/host side).
- **No real Minecraft client test**: Packet and tracker behavior is verified in Rust unit tests (`ferrite-entity`) and protocol-bot acceptance tests (`plugin_b_server_acceptance`), not against a real Mojang client.

## Verification

- Java: `:ferrite-api-impl:jar`, `:ferrite-api-impl:test` (62 tests including `EntityEquipmentTest`), `:ferrite-host:test`, `:ferrite-host:jar`, `:ferrite-nms:jar`, `:ferrite-nms:test` (including `EntityTest` equipment parsing), `:fixtures:typical-plugin:jar`, `:fixtures:nms-plugin:jar`.
- Rust: `cargo test -p ferrite-entity` (361 tests including `equipment_change_broadcasts_set_equipment_and_late_viewers_receive_current_slots`), `cargo test -p ferrite-server entity_save::tests` (22 tests including `mob_equipment_with_nbt_survives_disk_save_and_restart`), `cargo test -p ferrite-server plugin_host::state::tests` (11 tests including `entity_equipment_set_async_validates_drains_and_reads_via_entity_state_get`), `cargo test -p ferrite-server --test plugin_b_server_acceptance` (verifying player and summoned-pig `EntityEquipment` end-to-end), `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check`.
