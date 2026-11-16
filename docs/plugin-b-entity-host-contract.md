# Plugin B Entity/Player Host Contract & Schema

This document describes the entity/player NMS snapshot and async-write contract between
`crates/ferrite-entity`, `crates/ferrite-plugin-host`, `java/ferrite-api-impl`, and
`java/ferrite-nms` for Minecraft 26.3 (protocol 777).

## 1. Ownership & Threading Rules

- Java (`Entity`, `ServerPlayer`, `CraftEntity`, `CraftPlayer`) **never** holds a live Rust
  `EntityData` / `EntityStore` / `ServerPlayer` pointer or reference.
- All reads resolve immutable value snapshots (`EntitySnapshot`, `PlayerSnapshot`, `EntityState`,
  `PlayerState`).
- All state mutations return `CompletableFuture<Void>` routed to the entity's owning region (or
  the server main thread for runtime-hosted player state) without parking the caller's Java thread.
- Synchronous Bukkit cross-region mutators (`teleport`, `setVelocity`, `addPassenger`,
  `removePassenger`, `setPose`, `setSilent`, `setGlowing`, `setInvisible`, `setLevel`, etc.) fail
  explicitly with `UnsupportedOperationException` (`NmsUnsupported`).

## 2. Rust Host Ops (`ferrite-plugin-host` Contract v2)

`crates/ferrite-plugin-host/src/state.rs` reserves:
- `player.state_get`: `{ "uuid": "<lowercase-hyphenated-uuid>" }`
- `entity.state_get`: `{ "uuid": "<lowercase-hyphenated-uuid>" }`

Error codes returned in the v2 error envelope (`StateError`):
- `invalid_request`: malformed UUID or out-of-range field.
- `unknown_world`: target world key is not registered.
- `unknown_player` / `unknown_entity`: entity/player is not loaded.
- `invalid_equipment_slot`: equipment slot is not one of `HAND`, `OFF_HAND`, `FEET`, `LEGS`, `CHEST`, `HEAD`.
- `unsupported`: provider does not implement the op.

## 3. Java `NmsHost` Entity/Player Surface (`dev.ferrite.nms.host`)

### `EntityState` Snapshot Fields
- `networkId: int`, `removed: boolean`, `ticksLived: int`, `fireTicks: int`, `fallDistance: float`,
  `onGround: boolean`, `portalCooldown: int`, `pose: String`
- `customName: String?`, `customNameVisible: boolean`, `silent: boolean`, `noGravity: boolean`,
  `glowing: boolean`, `invisible: boolean`
- `equipment: Map<String, String>` (normalized slot key `HAND|OFF_HAND|FEET|LEGS|CHEST|HEAD` ->
  namespaced item key, e.g. `minecraft:iron_sword`)

### Owner-Routed Async Writes on `NmsHost`
- `teleport(entityId, worldId, x, y, z, yaw, pitch)`
- `setGameMode(playerId, gameMode)`
- `setHealth(playerId, health)`
- `setFoodLevel(playerId, foodLevel)` (`0..=20`)
- `setEquipment(entityId, slot, itemKey)` (`slot` normalized via `EntityState.normalizeEquipmentSlot`)
- `setVelocity(entityId, vx, vy, vz)` (finite doubles)
- `setRotation(entityId, yaw, pitch)` (finite floats)
- `setPose(entityId, pose)`
- `setCustomName(entityId, customName, visible)`
- `addPassenger(vehicleId, passengerId)` / `removePassenger(vehicleId, passengerId)`

## 4. Rust `crates/ferrite-entity` Support

- `crates/ferrite-entity/src/metadata.rs`: 26.3 accessor indices for `base`, `living`, `mob`,
  `player`, plus `Metadata::define_living`, `Metadata::define_mob(MobKind)`, and
  `Metadata::for_player()`.
- `crates/ferrite-entity/src/equipment.rs`: `EquipmentSlot` and `Equipment` (6-slot component
  attached to `EntityData`, serialized via `SetEquipment` and persisted via `to_nbt`/`from_nbt`).
- `EntityStore::split` and `EntityStore::merge` preserve `metadata`, `custom_name`, and
  `equipment` across region boundary hand-offs.
