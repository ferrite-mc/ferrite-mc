# Mob AI parity audit (2026-11-14)

Scope: the nine mobs named in the Entities & AI lane — zombie, skeleton, spider, creeper,
enderman, chicken, cow, pig and sheep. This is an audit, not a claim of full parity. The
**evaluated** table lists behaviour that exists and is pinned by a test. The **not evaluated**
table lists vanilla goals that are absent or only approximated; those are the next work.

Vanilla evidence comes from the 26.3 server jar (`vanilla/26.3/versions/26.3/server-26.3.jar`,
official names visible through `javap`) and the generated entity data. Ferrite evidence is the
source path plus the test name. The jar is local and gitignored; it is not distributed.

## Behaviour matrix (evaluated)

| Mob | Behaviour | Vanilla evidence | Ferrite | Status |
|---|---|---|---|---|
| all hostile | nearest attackable player, current target kept until out of range | `NearestAttackableTargetGoal`, `follow_range` | `ai::select_target`, `ai::nearest_target` | implemented, tests |
| all hostile | creative/spectator never targeted, also across regions | `TargetingConditions` | `game/targeting.rs`, `MobRules.untargetable` | implemented, test |
| all hostile | melee reach, cooldown, path following | `MeleeAttackGoal` | `ai.rs` melee arms, `path.rs` | implemented; cooldown is one second |
| zombie | melee, chase | `ZombieAttackGoal` | `ai.rs` | implemented |
| zombie | baby speed ×1.5 | `Zombie.SPEED_MODIFIER_BABY` = 0.5 `ADD_MULTIPLIED_BASE` (jar) | `ai::chase_speed`, `BABY_ZOMBIE_SPEED` | implemented, unit test |
| skeleton | bow every 40 ticks (20 on Hard) | `AbstractSkeleton` attack intervals | `SKELETON_COOLDOWN`, `SKELETON_COOLDOWN_HARD` | implemented, tests |
| skeleton | shot spread `14 - difficulty * 4` | `RangedAttackMob.rangedAttackUncertainty` (jar) | `ai::arrow_direction`, Normal 6 / Hard 2 | implemented, unit test |
| skeleton | strafe while shooting, flip side | `RangedBowAttackGoal` | `ai.rs` skeleton arm | implemented, test |
| spider | neutral in bright light (≥12 at the eyes) | `SpiderTargetGoal` / `SpiderAttackGoal` | `spider_sees_bright_light` | implemented, test |
| creeper | swell inside 3 blocks, abort past 7, 30 ticks, power 3 | `SwellGoal`, `Creeper` | `ai.rs` creeper arm | implemented, tests |
| enderman | neutral until provoked, anger timer, blink | `NeutralMob`, `EnderMan` teleport | `ai.rs`, `mob::retaliate` | implemented, tests |
| chicken | wander, panic, breed, follow parent, tempt, look-at | `Chicken.registerGoals` (jar) | `ai.rs` passive goals, `breed.rs` | implemented |
| chicken | adult lays an egg every 6000–11999 ticks | `Chicken.aiStep`: `isBaby` check, `6000 + nextInt(6000)` (jar) | `ai::MobEvent::LayEgg`, `MobState::egg_time` | implemented, unit + region test |
| sheep | wool colour and sheared flag in synched byte 18 (`colour \| 0x10`), saved as `Color`/`Sheared` | `Sheep.DATA_WOOL_ID`, `addAdditionalSaveData` (jar) | `sheep.rs`, `entity_save.rs`; tests `a_new_sheep_has_a_colour_and_synched_wool_data`, `a_sheep_keeps_its_colour_and_shorn_wool_across_a_save` | implemented (same region) |
| sheep | spawn colour: 5% black/gray/light gray, 3% brown, 82% white | `SheepColorSpawnRules` temperate table (jar) | `sheep::random_color`, test `spawn_colours_follow_the_temperate_table` | approximated: warm and cold biome tables absent (no biome at spawn in the entity crate) |
| sheep | lamb colour is the two-dye recipe mix, else either parent | `DyeColor.getMixedColor` (jar), two-dye recipes in `recipe/*_dye*.json` | `sheep::offspring_color`, `breed::approach` | implemented, test `offspring_mix_the_recipe_colours_and_otherwise_take_a_parent` |
| sheep | `EatBlockGoal`: 1/1000 (1/50 baby), 40 ticks, eats when the counter reaches 4, grass block to dirt, `#edible_for_sheep` plant removed, `mobGriefing` gates the block change, `ate()` regrows wool and ages a baby 60 ticks, event 10 starts the head-down animation | `EatBlockGoal`, `Sheep.ate` (jar) | `sheep::tick_eat`, `MobEvent::EatBlock`, `Region::eat_block`; tests `eating_grass_turns_the_block_to_dirt_on_the_36th_tick_and_regrows_the_wool`, `eating_a_short_grass_plant_removes_it_and_leaves_the_grass_alone`, `mob_griefing_off_leaves_the_eaten_block_alone`, `an_eaten_grass_block_becomes_dirt_and_a_plant_disappears` | implemented; only `PanicGoal` pre-empts it (not love or follow-parent) |
| sheep | shears: ready if not sheared and not a baby, 1-3 wool of its colour from `shearing/sheep`, shears lose 1 durability | `Sheep.mobInteract`/`shear`, `loot_table/shearing/sheep*.json` | `Region::shear_sheep`, `use_shears_on_sheep`, `drops::shearing_drops`; tests `shearing_a_sheep_drops_its_coloured_wool_and_a_second_shear_drops_nothing`, `a_lamb_is_not_sheared_and_a_cow_is_not_a_sheep` | implemented; no shear game event, plugin shear event or real client |
| sheep | death drops pick wool by colour and sheared state | `loot_table/entities/sheep.json` | `drops::mob_drops_with` with `sheep::loot_properties`; tests `a_dead_sheep_drops_wool_of_its_colour_unless_it_was_sheared` (region and loot) | implemented |
| enderman | stare-to-anger: nearest player within 64 who looks at its eyes (`dot > 1 - 0.025 / distance`, clear line of sight, no carved pumpkin) for 5 ticks, scanned on one tick in ten, then anger for 20-39 s | `EndermanLookForPlayerGoal`, `isBeingStaredBy`, `isLookingAtMe` (jar) | `enderman::tick_stare`, `targeting::publish_gaze`, `MobRules.gazes`; tests `a_stare_that_lasts_five_ticks_angers_the_enderman_for_20_to_39_seconds`, `the_anger_comes_exactly_five_ticks_after_the_stare_starts`, `looking_away_during_the_stare_cancels_it`, `a_carved_pumpkin_and_a_wall_both_hide_the_player`, cross-region `an_enderman_is_angered_by_a_stare_from_a_player_in_another_region` | implemented; the line of sight is a 0.25-block march against collision boxes (unloaded blocks are see-through), not vanilla's `ClipContext` |
| enderman | water and rain damage: 1 drowning damage a tick in water or in rain under open sky, 9 in 10 hurts teleport it | `isSensitiveToWater`, `LivingEntity.aiStep`, `Enderman.hurtServer` (jar) | `enderman::in_water_or_rain`, `ai.rs`; tests `an_enderman_in_water_loses_health_and_a_dry_one_does_not`, `rain_hurts_an_enderman_in_the_open_only` | implemented; the hurt cooldown decides how often it lands; the teleport reuses Ferrite's eight-block blink, not vanilla's 64-try random teleport |
| enderman | creepy (index 17) and stared-at (index 18) flags | `DATA_CREEPY`, `DATA_STARED_AT` | `enderman::sync`, test `the_ai_angers_a_calm_enderman_from_a_published_gaze_and_sets_the_creepy_flag` | implemented (client face unverified) |
| enderman | carried block | `DATA_CARRY_STATE`, `carriedBlockState` | `MobState.carried`, `entity_save.rs`; test `an_enderman_keeps_its_carried_block_state_across_a_save` | **blocked for the wire**: kept and saved, but synched byte 16 is an optional block state and `ferrite-protocol` has no such `MetaValue` (lane B owns that crate). No take/leave-block goals, no drop of the carried block on death |
| creeper | charged state: explosion power `3 * 2`, synched powered flag (index 17), saved as `powered`, set by lightning (`thunderHit`) | `Creeper.explodeCreeper`, `DATA_IS_POWERED`, `thunderHit` (jar) | `creeper.rs`, `entity_save.rs`, `strike_channeled`; tests `a_charged_creeper_explodes_with_double_power`, `a_charged_creeper_stays_charged_across_a_save` | implemented; the only lightning source in Ferrite is channeling (no natural thunderstorm strikes, no `/summon lightning_bolt`), so a charged creeper arises from a channeled trident only |
| creeper | hiss when the fuse starts (`creeper.primed`, volume 1, pitch 0.5) and the swell-direction flag (index 16) | `Creeper.tick`, `DATA_SWELL_DIR` | `MobEvent::Hiss`, `Region::play_sound_near`; tests `an_uncharged_creeper_explodes_with_power_3_after_one_hiss`, `the_hiss_comes_again_when_the_fuse_winds_down_and_restarts`, `the_synched_flags_follow_the_fuse_and_the_charge` | implemented (event emitted and tested; the sound packet itself is not asserted, and the audible result is unverified). A creeper that loses its target now winds its fuse down like vanilla |
| cow/pig/sheep | wander, panic, breed, follow parent, tempt, look-at | `Animal` goals | `ai.rs`, `breed.rs` | implemented |
| all animals | baby half size, follow parent within 8 blocks | `AgeableMob`, `FollowParentGoal` | `breed.rs`, `ai::follow_parent` | implemented, test |
| wolf/llama/trader | taming, leads, caravan (not in this lane's nine) | `Leashable`, `LlamaFollowCaravanGoal` | `ai.rs`, `tame.rs` | implemented, tests |
| cross-region | players in another region are visible to targeting | position snapshot | `TickContext::positions`, `Region::tick_mobs` | implemented, test `hostile_mob_sees_a_nearby_player_in_another_region` |
| cross-region | melee reaches the player's owning region and names the attacker | server-side routing | `Signal::MobMelee` | implemented, test `a_zombie_hits_a_player_across_a_region_boundary_and_names_itself` |
| cross-region | a shield-blocked hit knocks the attacker back | `LivingEntity.blockedByShield` | `Signal::MobKnockback` | implemented, test `a_blocked_cross_region_melee_knocks_the_attacking_mob_back` |
| split/merge | the cross-region target survives region split and merge | state moves with the mob | `RegionData::{split,merge}` | implemented, test `a_cross_region_target_survives_a_region_split_and_merge` |
| chunk re-entry | two viewers, one leaving first, keep one entity | tracker pairings | `game/mod.rs`, `region.rs` | implemented, regression tests |

## Not evaluated / known gaps

| Mob | Missing or approximate | Why it matters |
|---|---|---|
| zombie | no door breaking, reinforcements, village goal, daylight burning, drowned conversion | the jar registers `BreakDoorGoal`, `MoveThroughVillageGoal` and sun handling in the skeleton/zombie family |
| skeleton | no `RestrictSunGoal`/`FleeSunGoal`, no melee fallback, no arrow pickup/tipped arrows | registered by `AbstractSkeleton` in the jar |
| spider | no wall climbing, leap attack or jockey | `Spider` has its own navigation and leap goal |
| creeper | flint-and-steel ignition (`ignited`), skull drops when a charged creeper kills, potion-effect clouds, cat/ocelot avoidance | `Creeper.mobInteract`, `killedEntity`, `AvoidEntityGoal` | not modelled |
| enderman | take/leave-block goals, carried-block metadata on the wire, vanilla's teleport search, daylight teleport, freeze-when-looked-at | `EndermanTakeBlockGoal`, `EndermanLeaveBlockGoal`, `customServerAiStep`, `EndermanFreezeWhenLookedAt` | interface needed from the protocol crate: an optional block-state `MetaValue` (serializer `OPTIONAL_BLOCK_STATE`) |
| chicken/cow/pig | no chicken jockey, no pig riding, milking only through the existing interaction path | `Chicken` jockey and `Pig` saddle goals are not modelled |
| sheep | dyeing a sheep with a dye item, warm/cold biome colours, plugin shear events | `Sheep.mobInteract` dye branch, `SheepColorSpawnRules` | `sheep::set_color` exists but nothing calls it from a dye use |
| all | pathfinding is a block-grid A* with a 32-node cap, not `GroundPathNavigation` | long detours, doors and water are approximated |
| attributes | default attributes now sent when a viewer starts tracking; equipment/passengers on pairing still incomplete | vanilla `ServerEntity.sendPairingData` sends attributes, equipment and passengers |
| client | no real 26.3 client was used | visuals, animations and sounds are unverified |

## Fixed in this lane (2026-11-14)

- Chicken egg laying (adult timer, egg item entity, timer saved).
- Skeleton shot spread, Normal/Hard inaccuracy.
- Baby zombie chase speed.
- Living-entity default attributes on tracking start (including a horse's rolled speed/jump).
- End gateway teleport refusal while the purple beam cools down; the 200/40/2400 constants
  were confirmed from `TheEndGatewayBlockEntity` in the jar.
- Dragon perched timings confirmed from the jar and corrected: scan box 20×10, attack 40,
  each flaming phase 200 ticks with one cloud at tick 10, and four flaming phases before
  takeoff. The scan give-up charge and attack turn toward the player are still missing.

- Sheep (see the matrix): colour, sheared state, eating and shearing. Cross-region: none needed,
  the sheep, its drops and the eaten block are owned by one region (the block write goes through
  the region scheduler like any other `place_blocks`).

## Jar notes that contradict older docs

- Gateway outer-island search: vanilla `findExitPosition` calls
  `findTallestBlock(..., 5, ...)` (radius 5); Ferrite searches 16 blocks. The older
  `docs/end-dimension.md` said Ferrite's radius was smaller; it is larger.
- The gateway creation beam does **not** block a teleport in vanilla (`EndGatewayBlock` only
  checks `isCoolingDown`), so the fix gates on the 40-tick cooldown only.
