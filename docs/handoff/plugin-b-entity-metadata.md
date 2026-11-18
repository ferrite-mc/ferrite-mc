# Plugin B — Living-Entity Metadata & Equipment Edge Cases (`yxshadxw/plugin-b-entity-metadata`)

## Summary

Closed the remaining living-entity per-type metadata and equipment edge cases on branch `yxshadxw/plugin-b-entity-metadata`:
1. Added missing per-type `SynchedEntityData` metadata backed by `MobState` (`EnderDragon`, `Zombie`, `Parrot`, `Wolf`, `Cat`), wired `mob::sync_state_metadata` into `tracker.rs` (for both late-viewer pairing and live viewer updates) and `entity_save.rs` reload.
2. Extended `Equipment` / `EquipmentSlot` to all 8 vanilla slots (`HAND`, `OFF_HAND`, `FEET`, `LEGS`, `CHEST`, `HEAD`, `BODY`, `SADDLE`) and per-slot `drop_chances` (`0.085f` default).
3. Added `entity.drop_chance_set_async` (`EntityDropChanceSetRequest`) and `"drop_chances"` in `entity.state_get`, wired through the coordinator to the owning region via `worlds.schedule_at_chunk`.
4. Enabled `BODY`, `SADDLE`, `*DropChance` accessors, and full `nbt_base64` item component serialization/deserialization on `FerriteEntityEquipment` and `FerriteWorld#parseEquipment`.

## Metadata Matrix

| MobKind | Index | Value / Semantics | Backed by `MobState` / Synced | Persisted |
|---|---|---|---|---|
| `EnderDragon` | `16` (`PHASE_INDEX`) | `Int` (`DragonPhase::protocol_id()`, `0..=8`) | `MobState::dragon_phase` (`dragon::define_metadata`, `dragon::sync_metadata`) | `DragonPhase` |
| `Zombie` | `16` (`ZOMBIE_BABY_INDEX`) | `Boolean` (`age < 0`) | `MobState::age` | `Age` |
| `Zombie` | `17` (`ZOMBIE_SPECIAL_TYPE_INDEX`) | `Int` (`0`) | `define_zombie_metadata` | — |
| `Zombie` | `18` (`ZOMBIE_DROWNED_CONVERSION_INDEX`) | `Boolean` (`in_water_time >= 600`) | `MobState::in_water_time` | `InWaterTime` |
| `Parrot` | `20` (`PARROT_VARIANT_INDEX`) | `Int` (`variant.unwrap_or(0)`) | `MobState::variant` (`tame::define_metadata`) | `variant` |
| `Wolf` | `20` (`WOLF_INTERESTED_INDEX`) | `Boolean` (`false`) | `tame::define_metadata` | — |
| `Wolf` | `21` (`COLLAR_COLOR_INDEX`) | `Int` (`14`, default red) | `tame::define_metadata` | — |
| `Wolf` | `22` (`WOLF_ANGER_END_TIME_INDEX`) | `Long` (`anger > 0 ? anger : -1`) | `MobState::anger` | `AngerTime` (existing) |
| `Cat` | `21` (`COLLAR_COLOR_INDEX`) | `Int` (`14`, default red) | `tame::define_metadata` | — |
| `Pig` | `16` (`SADDLE_INDEX`), `17` (`BOOST_TIME_INDEX`) | `Boolean` (`saddled`), `Int` (`0`) | `MobState::saddled` (`pig::define_metadata` also restored on reload) | `equipment.saddle` / `Saddle` |
| `Spider` | `16` (`FLAGS_INDEX`) | `Byte` (climbing bit) | `spider::define_metadata` (also restored on reload) | — |
| Existing (`Sheep`, `Creeper`, `Enderman`, `Horse`/`Donkey`/`Mule`/`Llama`, `Villager`, `TamableAnimal`, `AgeableMob`) | unchanged | unchanged | synced via `mob::sync_state_metadata` before pairing/broadcast | existing |

## Persistence & Ownership Rules

- **Equipment (`equipment.rs`, `entity_save.rs`)**:
  - `Equipment` stores 8 slots (`HAND`, `OFF_HAND`, `FEET`, `LEGS`, `CHEST`, `HEAD`, `BODY`, `SADDLE`) plus `drop_chances: [f32; 8]` (default `0.085f`).
  - `entity_to_nbt` writes `equipment` (merging `data.equipment` with `mob.equipment`, `mob.armor`, and `mob.saddled`) and `drop_chances` (omitting default `0.085f` entries).
  - `entity_from_tag` restores all 8 `equipment` slots, `drop_chances`, and syncs `mob.equipment[0..6]`, `mob.armor` / `mob.armor_points`, and `mob.saddled`.
- **Region ownership**:
  - Java reads come from `entity.state_get` / `world.entities_snapshot` snapshots (`FerriteWorld#parseEquipment` + `parseDropChances` including `FerriteItemNbt.decodeItemBase64`).
  - Java writes on non-player `LivingEntity` (`FerriteEntityEquipment`) queue via `entity.equipment_set_async` (including `nbt_base64`) and `entity.drop_chance_set_async`, drain on `WorldStateProvider::tick`, and route through the coordinator to `worlds.schedule_at_chunk(dimension, chunk, ...)`. No direct cross-region live state access or synchronous cross-region waits exist.

## Remaining Limitations & Explicit Refusals

- **Player `BODY`/`SADDLE` and `*DropChance`**: `FerritePlayerEquipment` continues to throw `UnsupportedOperationException` for `EquipmentSlot.BODY`, `EquipmentSlot.SADDLE`, and all `*DropChance` getters/setters because players do not have mob equipment drop chances or body/saddle slots.
- **Unmodelled metadata indices**:
  - `Holder<WolfVariant>` / `Holder<CatVariant>` / `Holder<PigVariant>` / sound variants (require registry-holder `MetaValue` serializer not in `ferrite-protocol`).
  - `Skeleton` `DATA_STRAY_CONVERSION_ID` (stray conversion not modelled in `MobState`).
  - `Zombie` `DATA_SPECIAL_TYPE_ID` remains `0` (drowned/husk subtypes not modelled).
  - `Wolf`/`Cat` collar dye color remains fixed at default `14` (red) until collar dyeing is modelled on `MobState`.
- **No real Minecraft client**: Packet and tracker behavior is verified by Rust unit tests (`ferrite-entity`), `ferrite-server` persistence/state tests, JUnit tests, and the protocol-bot acceptance test (`plugin_b_server_acceptance`), not against a real 26.3 Mojang client.

## Verification

- Java: `gradle --no-daemon --console=plain :ferrite-api-impl:jar :ferrite-api-impl:test :ferrite-host:test :ferrite-host:jar :ferrite-nms:jar :ferrite-nms:test :fixtures:typical-plugin:jar :fixtures:nms-plugin:jar`
- Rust: `cargo test -p ferrite-entity`, `cargo test -p ferrite-server --test plugin_b_server_acceptance`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `git diff --check`.
