# W7 lane 2 — mob AI targeting and riding parity

Delivered in three PRs. Everything below was checked against the code and its tests in this
repository; nothing was validated with a graphical client.

| PR | Branch | Merge SHA | Scope |
|---|---|---|---|
| #8 | `w7-maps` | `3ec1678` | persistent maps and the cartography table (see `w7-maps.md`) |
| #9 | `w7-ai-vehicles` | `aa66167` | creative/spectator targeting, ride order/capacity/chains |
| follow-up | `w7-ai-matrix` | see its PR | Hard skeleton cooldown, spider light, ride dimension/death fixes, this matrix |

## Defects fixed

| # | Defect | Fix | Test |
|---|---|---|---|
| D1 | Hostile mobs targeted creative and spectator players (shared snapshots carry no game mode). | `game/targeting.rs`: the owning region publishes untargetable players (gamemode 1/3), cleared on leave. `ai::MobRules.untargetable` is applied **only to hostile target selection**; animals, villagers and owners still see every player. | `ai::hostile_mobs_never_target_untargetable_players`, `targeting::…published_until_they_change_mode_or_leave`; `integration::a_creeper_blast_damages_blocks_for_a_bot` now uses a survival bot |
| D2 | Skeleton shot interval was a constant 40 ticks; vanilla `RangedBowAttackGoal` uses 20 on Hard. | `MobRules.hard` from `GameState.difficulty == 3`. | `ai::a_skeleton_shoots_twice_as_often_on_hard` |
| D3 | Spiders were always aggressive; vanilla is neutral at light level 12+ (magic value >= 0.5): no new target, and a 1-in-100 per-tick drop of the current one. | `BlockGetter::brightness` (default unknown = dark); `TickBlocks` answers it from the light source natural spawning uses. | `ai::a_spider_is_neutral_in_bright_light_and_hostile_in_the_dark` |
| R1 | `passengers_of` returned UUID order. | per-seat mount order. | `passengers_keep_the_order_they_mounted_in` |
| R2 | Unlimited riders on any vehicle. | `MountError::Full`, `capacity_of` from the 26.3 `canAddPassenger` overrides: default 1, boats/rafts 2, chest boats 1, camel 3 (`size() <= 2`), happy ghast 4. `/ride` and `Entity#addPassenger` pass the vehicle type; unknown type = 1 seat. | `a_vehicle_takes_only_as_many_riders_as_vanilla_allows`, `mounting_without_a_known_type_uses_the_default_single_seat` |
| R3 | A rider's rider used a stale position. | `plan` places chains shallow-first. | `a_rider_of_a_rider_follows_the_final_position_in_the_same_plan` |
| R4 | A ride survived a dimension change; the rider's coordinates were rewritten in the wrong world. | `plan` ends the ride (vanilla) when rider and vehicle are published in different dimensions. | `changing_dimension_ends_the_ride_instead_of_moving_the_rider_across_worlds` |
| R5 | Non-player riders were only moved through the Overworld region owner, so mobs riding in the Nether/End never followed. | `Step::Move` carries the dimension. | `a_rider_in_the_nether_is_moved_in_the_nether` |
| R6 | A player who died kept riding (the corpse followed the vehicle until respawn). | `process_death` queues the existing dismount. | **none automated** — `process_death` has no unit entry point and the ride table is process-global, so an in-process bot test could pass vacuously. Covered by review only. |

The leave path is pinned by `a_leaving_player_frees_the_seat_and_detaches_their_passengers`.
R4 and R5 tests were written together with their fixes; they were not run red first.

## Species coverage (what the code models; tests are in `ferrite-entity` unless noted)

Modelled kinds (20): chicken, cow, pig, sheep, zombie, skeleton, spider, creeper, enderman,
villager, wandering trader, ender dragon, wolf, cat, parrot, horse, donkey, mule, llama,
trader llama. Anything else has no AI.

| Species | Target acquisition / loss | Attack & cooldown | Other behaviour | Not done |
|---|---|---|---|---|
| zombie | nearest targetable player in follow range, keeps current while in range (`a_zombie_reaches_and_hits_its_target`) | melee | path to target | doors, swimming, burning in daylight, babies, reinforcements |
| skeleton | as zombie | arrows, 40 ticks (20 on Hard); strafes (`a_skeleton_strafes_…`) | | sun burning, arrow inaccuracy by difficulty |
| spider | as zombie, but neutral at light >= 12 (D3) | melee | | wall climbing, leap attack, Nether/End ambient light |
| creeper | as zombie | fuse 3 / abort 7 blocks, keeps swelling (`a_creeper_swells_…`) | | charged creepers, cat avoidance |
| enderman | neutral until angered | melee | blink (1/400 a tick) | teleporting away from water/rain, gaze trigger |
| chicken / cow / pig / sheep | none | none | wander, look at players, panic after being hurt (`a_hurt_cow_flees_…`), breeding with the right food and love cooldown (`breed.rs` tests), baby growth | tempt-with-food follow, sheep eating grass, egg laying beyond hatching |
| wolf | wild wolves bite whoever hurt them and the pack angers (`hurting_one_wild_wolf_angers_the_pack…`); tame wolves defend and attack with the owner | bite | bone taming, sit/follow/teleport (`tame.rs`) | hunting sheep/skeletons, collar colour |
| cat / parrot | none | none | fish/seed taming, follow owner, sit; shoulder parrots | cat gifts, creeper scare, parrot imitation |
| horse / donkey / mule | none | none | taming by riding, bucking, saddle/armor/chest rules, foal stats (`horse.rs` tests) | |
| llama / trader llama | spits at whoever hurt it (`a_wild_llama_spits_…`) | spit | caravans (limit 10), lead following (`a_lead_*`) | |
| villager / wandering trader | none | none | schedules, workstations/beds, restock, gossip, trading (`villager.rs` tests); walks around water/lava | panic from zombies, iron golem, breeding, door use |
| ender dragon | `dragon.rs` | | | outside this audit |

Cross-cutting navigation (`path.rs`): flat paths, detours around walls, one-block step
up/down, unloaded columns as walls (`a_flat_path_is_straight_and_bounded`,
`a_wall_forces_a_detour`, `a_step_up_and_a_drop_are_walkable_but_a_wall_is_not`,
`unloaded_columns_are_walls`). **No doors, swimming, ladders or fluid avoidance** except the
villager water/lava check. Flee exists only as the post-hurt panic for passive animals.

This matrix lists behaviour found in code and tests; the "Not done" column lists gaps seen
while reading, not an exhaustive comparison against vanilla.

## Cross-region behaviour

- Hostile targets come from shared position snapshots plus the owner's fresh positions; the
  untargetable table is the only extra shared state (a `Mutex<HashSet>` bounded by online players).
- Leashes across regions have tests (`a_lead_follows_a_holder_in_another_region`,
  `a_lead_snaps_once_when_its_cross_region_holder_is_too_far`).
- Rides use a process-global table keyed by UUID; riders are moved by scheduling on the region
  that owns the rider's chunk each tick, so a vehicle crossing a region border keeps working in
  principle. There is **no test** that moves a vehicle across a region border, splits or merges
  regions with a ride active, unloads a vehicle's chunk while mounted, or saves and reloads a ride.
- Rides are not persisted across a restart.
- Runtime worlds share a vanilla dimension type and are indistinguishable in the command
  snapshot, so a ride between two runtime worlds of the same type is not ended (R4 limitation).

## Unsupported / not done

- **Per-type seat offsets.** Placement is a fixed lift (`SEAT_LIFT = 0.6`). Vanilla uses
  `EntityAttachments` passenger points, which are not in the generated reports; extracting them
  from the jar's `EntityType` initialiser was not done and no numbers were invented.
- Player steering for vehicles other than saddled horses; dismount-position safety search.
- Nether/End ambient light for the spider rule.
- Boats, minecarts and camels have a seat capacity but no steering.
- No real graphical client was used; nothing visual is verified.

## Validation

`cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` (stable 1.100, the CI
toolchain), `cargo test -p ferrite-entity`, `cargo test -p ferrite-server --lib ride`/`mob`, and the
`plugin_lifecycle` integration test (5 of 5, three runs) were run for the follow-up; CI for the
follow-up PR is recorded on the PR.

A first version of D1 filtered the shared player list and made
`runtime_world_gap7_create_edit_enter_leave_unload_reopen` fail 3 of 3 locally (0 of 3 on main);
the filter was narrowed to hostile selection and the test then passed 3 of 3.
