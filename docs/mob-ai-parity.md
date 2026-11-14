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
| creeper | no charged (lightning) explosion, no hiss event | `Creeper` explosion power depends on charged state |
| enderman | no stare-to-anger, no water damage, no carried-block metadata | `EndermanLookForPlayerGoal` and `EnderMan` water handling |
| chicken/cow/pig | no chicken jockey, no pig riding, milking only through the existing interaction path | `Chicken` jockey and `Pig` saddle goals are not modelled |
| sheep | no `EatBlockGoal`, and **no wool colour/sheared state or shearing** | `Sheep.registerGoals` includes `EatBlockGoal`; this is the largest passive gap |
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

## Jar notes that contradict older docs

- Gateway outer-island search: vanilla `findExitPosition` calls
  `findTallestBlock(..., 5, ...)` (radius 5); Ferrite searches 16 blocks. The older
  `docs/end-dimension.md` said Ferrite's radius was smaller; it is larger.
- The gateway creation beam does **not** block a teleport in vanilla (`EndGatewayBlock` only
  checks `isCoolingDown`), so the fix gates on the 40-tick cooldown only.
