# Player travel into runtime worlds — 2026-11-11

Baseline `b6a7b3e`, branch `rw-player-travel`. Covers known gap 1 of
`docs/handoff-runtime-worlds-2026-11-11.md`. Not touched: `plugin_lifecycle` tests and the
unload transaction (other lanes).

## What changed

- **Teleport.** `PlayerCommand::Teleport` now resolves `location.world` through
  `travel_target` (default UUID -> vanilla overworld, otherwise a live runtime world in the
  catalog) and queues `Command::ChangeWorld` on the player's mailbox. The *owning region*
  decides: same world -> plain teleport, other world -> `Region::change_world`. The main loop
  resolves the target `AnvilChunkSource` and sends it inside the command, so no region does a
  shared lookup, takes a global lock or waits on another region. The player then leaves via the
  existing `Signal::ChangeDimension` path, which already inserts into the target world's own
  scheduler.
- **`Region::change_world`** (new; `change_dimension` is now a vanilla wrapper). Releases every
  chunk held in the *old* `WorldId`, repoints the tracking entry to the new `WorldId`, swaps the
  source. When the dimension key is unchanged (runtime overworld <-> overworld) it first sends
  a `Respawn` to another dimension so the client drops the old world's chunks (Paper does the
  same).
- **Chunk source identity.** `AnvilChunkSource::world_id()`; runtime sources are bound to their
  UUID in `RuntimeWorldCatalog` (`bind_runtime_world`), so code that only holds a source
  (fluids, random ticks, command-block level events, world commands) broadcasts to the right
  world. `WorldBlocks::world_id()` added with a vanilla default.
- Hold bookkeeping tolerates an unloaded world (signals queued before an unload are dropped,
  not a panic): `Worlds::{add_chunks,remove_chunk(s),schedule_at_chunks}` and `defer_release`.

## `Dimension` -> `WorldId` call sites migrated

| Area | Site |
|---|---|
| Signals | `Signal::{Hold, HoldBatch, FinishRelease, FinishReleaseBatch, SaveEntities, SaveCachedEntities, Leave}` field `dimension` -> `world: WorldId` |
| Tracking observers | `ChunkObservers`/`Tracking` tuple key; join insert; `Signal::ChangeDimension` update; `change_world` |
| Broadcast helpers | `broadcast_tracked_chunk`, `broadcast_player_metadata`, `broadcast_shoulder_bytes`, `end_player_spin` (take `impl Into<WorldId>`), `should_broadcast_player_metadata`; callers in region/blocks/container/fluid/commands_world now pass `world_id`; the same-dimension mob/player snapshot in `tick_mobs` |
| Holds | main-loop `holds` map, `add_holds`, `release_hold(s)`, autosave hold walk, `/forceload` startup holds (wrapped `Vanilla`) |
| Transient entities | `TransientEntities` key `(WorldId, ChunkPos)`; `release_chunk_entities`, `add_holds` restore |
| Entity sidecar paths | new `world_storage_root` (vanilla: `Dimension::storage_root`; runtime: its own directory) used by `SaveEntities`, `SaveCachedEntities` and the `add_holds` load |
| Chunk flush | `FinishRelease(Batch)` uses `world_source` (runtime sources too) |
| Scheduler plumbing | `Worlds::{owner_world, add_chunks, remove_chunk(s), schedule_at_chunks}`, `defer_release(_many)` take `WorldId` |
| Plugin chunk-loaded hook | `set_loaded` only for `Vanilla(Overworld)` (was `Dimension::Overworld`, which a runtime overworld would have corrupted) |

## Intentionally left on `Dimension`

- `Signal::{EnderPearlTeleport, MobMelee, FluidChange, FluidPlace, RedstoneChange}` and the
  matching `Command::{EnderPearlTeleport, MobMelee}`, `Worlds::{owner, schedule_at_chunk}`
  and their callers: they route to a region scheduler by `Dimension`, so on a runtime world they
  would reach the *vanilla* scheduler. Not player travel; needs a follow-up before fluids,
  redstone or pearls work in runtime worlds (`scheduler()` will panic/misroute for them).
- `/execute in <dimension> run tp` (`Command::ChangeDimension`) stays vanilla-only by design.
- `GlobalPlayer.dimension`/`online.dimension` stay `Dimension` (rules); `world_id` sits beside them.
- `crate::plugins::chunks`, `worldinfo` and the other plugin queries remain default-world only.
- `Dimension::Overworld` in `DimensionRegistry` lookups for portals (vanilla portals only).

## Tests

- `integration::player_travels_into_a_runtime_world_and_back_keeping_holds_and_entities`:
  real server + bot; cow in default world -> teleport into a runtime world (cow untracked,
  double `Respawn`, entity placed in the runtime world is tracked there) -> teleport back
  (runtime entity untracked, the same cow entity id re-sent) -> runtime world unloads cleanly.
- `tests::holds_and_entities_are_keyed_per_world_across_travel`: same chunk coords in two worlds
  hold independently, parked entities are keyed per world and come back after the round trip.
- `tests::runtime_world_holds_survive_region_merge_and_split`: runtime scheduler, cross-region
  holds merge on a bridge and split on release; entity and holds intact.
- `tests::holds_for_an_unloaded_world_are_dropped_instead_of_panicking`.

## Known limits

- Same-region and cross-region world travel are both covered by the two paths above; a real
  Bukkit `Player#teleport(Location)` with a Java fixture was **not** run here.
- A player in a runtime world still reports `online.dimension` only; plugin world queries for
  that player return the default world.
- Shutdown persistence of runtime entities/chunks and unload transactionality remain with
  their lanes.

## Verification (target `~/ferrite/target-rw2`)

- `cargo fmt --all -- --check`: exit 0. `cargo clippy --workspace --all-targets -- -D warnings`: exit 0.
- `cargo test --workspace`: every suite passes (ferrite-server lib: 728 passed, 0 failed, including
  the 4 new tests; `player_travels_into_a_runtime_world_and_back_keeping_holds_and_entities`
  ok in 5.46s when run alone). Overall exit 101 only because `tests/plugin_lifecycle.rs`
  fails the same 3 tests listed in the previous handoff (other lane, untouched):
  `server_loads_plugins_at_startup_and_disables_them_on_stop`,
  `plugin_api_calls_reach_the_client_as_packets`,
  `typical_plugin_uses_config_commands_listener_and_repeating_task`.
- Changes are uncommitted in the worktree.
