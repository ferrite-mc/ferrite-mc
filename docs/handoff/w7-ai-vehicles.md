# W7 lane 2 — mob AI targeting and riding parity (partial)

Branch `w7-ai-vehicles`. This is a **partial** delivery: two defect groups are fixed and
tested; the full per-species audit matrix was **not** completed (see "Not audited").

## Defects fixed

| # | Defect | Fix | Regression test |
|---|---|---|---|
| D1 | Hostile mobs targeted every online player of their world, including creative and spectator players (the shared position snapshot carries no game mode). | `game/targeting.rs`: each owning region publishes untargetable (gamemode 1/3) players into a small table, cleared on leave (`ride::forget`). `ai::tick_full` takes that set and excludes it **only from hostile target selection**; every other goal (look at, follow, tempt, owners, despawn range) still sees all players. A first version filtered the shared player list and broke a runtime-world plugin test, which is how the overbreadth was caught. | `ai::tests::hostile_mobs_never_target_untargetable_players`, `targeting::tests::creative_and_spectator_players_are_published_until_they_change_mode_or_leave`; `integration::a_creeper_blast_damages_blocks_for_a_bot` now uses a survival bot |
| R1 | `passengers_of` returned UUID order, not mount order. | Each seat carries a monotonically increasing order. | `passengers_keep_the_order_they_mounted_in` |
| R2 | Any number of riders could board any vehicle. | `MountError::Full`, `ride::capacity_of`. Capacity read from 26.3 `canAddPassenger` overrides: default 1, boats/rafts 2, chest boats/rafts 1, camel 3 (`size() <= 2`, a vanilla quirk), happy ghast 4. `/ride` and `Entity#addPassenger` pass the vehicle type; an unknown type gets one seat. | `a_vehicle_takes_only_as_many_riders_as_vanilla_allows`, `mounting_without_a_known_type_uses_the_default_single_seat` |
| R3 | A rider of a rider was placed from the middle rider's stale start-of-tick position. | `plan` places chains shallow-first on the new seat position. | `a_rider_of_a_rider_follows_the_final_position_in_the_same_plan` |
| R4 | (test only) leaving player frees the seat and detaches their passengers. | already worked; now pinned. | `a_leaving_player_frees_the_seat_and_detaches_their_passengers` |

All tests are same-process unit tests. They do not exercise region borders.

## Known but not fixed

- Spider is always aggressive; vanilla is neutral in bright light unless provoked. The AI has no
  light access (`BlockGetter` exposes only `block_state`), so this needs light plumbing.
- Skeleton shot cooldown is a constant 40 ticks; vanilla uses 20 on Hard. Difficulty is not modelled.
- Seat position is a fixed lift (`SEAT_LIFT = 0.6`) for every vehicle. Per-type seat offsets need
  `EntityAttachments` data that has not been extracted from the 26.3 jar.
- Ride state is a process-global table keyed by UUID. Disconnect (`GameMessage::Left`) clears it;
  death, world change, unload, save/reload and region transfer of vehicles were **not** verified.
- Rides are not persisted across restart.

## Not audited (honest gap)

The explicit per-species matrix (zombie, skeleton, spider, creeper, enderman, chicken, cow, pig,
sheep, horse/llama, villager/trader) for goal selection, navigation, collision, cooldowns,
flee/follow/breed/tame was not completed. Cross-region targeting, split/merge, and unload/reload
of targets and vehicles were not tested. Nothing here claims those behave correctly.
Unsupported entity families for riding are not enumerated. Real graphical clients were not used;
no visual behaviour is verified.

## Validation

- `cargo clippy --workspace --all-targets -- -D warnings` clean on stable 1.100 (CI toolchain).
- `cargo test -p ferrite-server --lib ride` and `targeting` pass; full lib run recorded in the PR.

## Delivery

Filled in on the PR; see the PR description for the head SHA, CI links and merge SHA.
Maps (separate PR #8) merged as `3ec1678`.
