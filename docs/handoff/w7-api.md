# W7-1 API lane handoff (branch `w7-api`)

Base: `origin/main` 236f0e2abae1a9ecc2da11008d2d224aa73a0611 (verified ancestor, W6 work already included).
Head: see the PR; commits are listed below. Nothing here tags or publishes a release.

## Changed behavior

| Deliverable | Result | Where |
|---|---|---|
| 1. `Player#launchProjectile(Trident)` | Real trident: `launchThrown(TRIDENT)` -> `Region::plugin_spawn` builds `arrow::new_trident` (owner, velocity, trident damage 8.0) and tracks a default `minecraft:trident` item in `thrown_tridents`, so the existing `land_tridents` lifecycle drops it as an item when the flight ends (or returns it with Loyalty). Speed 2.5 (`Trident.PROJECTILE_SHOOT_POWER`). Not an arrow, velocity kept. | `PlayerMisc.launchProjectile`, `plugins/blasts.rs`, `region.rs::plugin_spawn` |
| 2. `Player#sendBlockUpdate` | Signs (unchanged) plus chest, trapped chest, barrel, shulker boxes (27 slots) and furnace/blast furnace/smoker (3 slots + timers) as a per-player `ClientboundBlockEntityDataPacket`. Slots, item ids, counts (1..99) and timers validated; duplicate slot, out-of-range slot, unknown item rejected. Banner, skull and any other `TileState`, and items carrying meta, throw `UnsupportedOperationException` (no model exists, nothing invented). No world mutation. | `PlayerMisc.sendBlockUpdate`, new `plugins/tile_update.rs`, `PluginPacket::TileUpdate`, `container.rs` send arm |
| 3. Paper lifecycle | `FerriteLifecycleManager` (priority, monitor, registration order across plugins, registration window from `allowsLifecycleRegistration`, per-plugin cleanup on disable/clear, handler exceptions logged and isolated). Service provider for `LifecycleEventTypeProvider` (needed so `LifecycleEvents` initialises). The `COMMANDS` event fires `INITIAL` after all plugins enable and `RELOAD` on `Server#reloadData`; its `Commands` registrar supports `BasicCommand` and Brigadier literal nodes (plain Brigadier argument types), registered as plugin commands in the command map and in the host command table (`Bootstrap.pluginCommands`). `JavaPlugin#registerCommand` works through it. | `FerriteLifecycleManager`, `FerriteLifecycleTypes`, `FerriteCommands`, `io/papermc/.../FerriteLifecycleEventTypeProvider`, `FerriteServer.registerPluginCommand` |
| 4. Shoulder bridge | `getShoulderEntity*` reads the game table (`game::ride`), `setShoulderEntity*`/`release*` become `PlayerCommand::Shoulder` -> mailbox `Command::PluginShoulder` -> drained by the player's own region (`apply_shoulder`), which saves and announces. Defect fixed: moving a parrot to the other shoulder dropped its owner and variant. `set(null)` clears without leaving a parrot, release puts it in the world. | `plugins/shoulders.rs`, `PlayerExtras`, `region.rs`, `container.rs`, `game/mod.rs` |

## Tests (all green on the head)

* Java: `cd java/ferrite-api-impl && ./gradlew --console=plain --max-workers=2 cleanTest build` -> 286 tests, 0 failures, 0 skipped. New: `PlayerBreadthTest` trident cases, `PlayerTileUpdateTest` (5), `FerriteLifecycleTest` (7), `PlayerShoulderTest` (5), real fixture plugin lifecycle command in `BootstrapTest` (`TestPlugin` registers `testlife`).
* Rust: `cargo +stable test -p ferrite-server --lib` -> 846 passed, 0 failed; `cargo +stable test -p ferrite-plugin` -> all passed (JVM integration with the Java fixtures). New: `a_plugin_launched_trident_...`, `tile_update` (4), `network::a_tile_snapshot_...`, `plugins::shoulders` (2), region shoulder tests (2).
* `cargo fmt --all --check` clean; `cargo clippy -p ferrite-plugin -p ferrite-server --all-targets -- -D warnings` clean.
* Linux toolchain: `RUSTUP_TOOLCHAIN=stable`; Java 25; fixture-world needs `vanilla/26.3/fixture-world` (gitignored; symlinked locally to the shared copy).

## Limitations (exact)

* Trident: no Loyalty/Channeling/Riptide enchantments (the default item has none); a plugin cannot choose the item or pickup status; the handle returned to Java exposes only generic entity state.
* sendBlockUpdate: banner, skull and every other block entity family are unmodelled in Rust and refused; furnace `BurnTimeTotal` is sent as 0 (Bukkit does not expose it); double chests send one half (`getSnapshotInventory`).
* Lifecycle: only `COMMANDS` can fire. `TAGS` and `DATAPACK_DISCOVERY` belong to `BootstrapContext`; the host does not run `PluginBootstrap`, so registering them is refused with `UnsupportedOperationException` (dependency: a bootstrap phase before plugin construction). Paper `ArgumentTypes` need a server-side provider that does not exist, so only plain Brigadier argument types work. `RELOAD` is fired when `reloadData()` is requested; the host reload itself is asynchronous and the host re-reads `pluginCommands` only at its own times. A disabled plugin cannot re-register (Paper has the same rule).
* Shoulders: parrot variant is not modelled on live parrots (always 0); a seated entity unknown to the region becomes a fresh parrot with that uuid; the Java handle for a seated parrot is detached (uuid only).

## Shared-file wiring (narrow hunks)

* `crates/ferrite-plugin/src/lib.rs`: `PluginPacket::TileUpdate`, `ShoulderOp`, `PlayerCommand::Shoulder`.
* `crates/ferrite-server/src/game/mod.rs`: one `PlayerCommand::Shoulder` arm (mailbox push).
* `crates/ferrite-server/src/game/region.rs`: `Command::PluginShoulder` + its arm, drain in `spawn_thrown`, `apply_shoulder` (replaces the unwired `set_shoulder`, kept `#[cfg(test)]`), `change_shoulder` move fix, `plugin_spawn` trident arm.
* `crates/ferrite-server/src/game/container.rs`: `menus.shoulders` field, `TileUpdate` send arm.
* `crates/ferrite-server/src/game/ride.rs`: removed the stale `expect(dead_code)` on `shoulder_set`.
* `crates/ferrite-server/src/plugins.rs`: module declarations and two chain entries; `plugins/network.rs`: `sendTileSnapshot(Location,String,List)`.
* New host op names (no numeric ids exist): `sendTileSnapshot(Location,String,List)`, `shoulder(String,String)`, `getShoulder(String)`.

## Integration

Merge `w7-api` normally; `META-INF/services/io.papermc.paper.plugin.lifecycle.event.types.LifecycleEventTypeProvider` must survive the shaded jar (it is under `src/main/resources`).
