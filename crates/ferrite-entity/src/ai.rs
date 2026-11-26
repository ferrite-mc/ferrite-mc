//! Mob behaviour: vanilla `LivingEntity.travel` physics plus the goals the modelled mobs use.
//!
//! Passive animals wander and look at nearby players (vanilla `WaterAvoidingRandomStrollGoal`
//! and `LookAtPlayerGoal`), and flee from an attacker after being hurt (`PanicGoal`). Hostile
//! monsters pick the nearest player as their target (`NearestAttackableTargetGoal`), follow a
//! block-grid A* path (`GroundPathNavigation` plus [`crate::path`]), and then run their attack
//! goal: melee for the zombie, spider and enderman (`MeleeAttackGoal`), bow fire for the
//! skeleton (`RangedAttackGoal`) and swell-then-explode for the creeper.
//!
//! Mob events that need the world or a player's health (a hit landing, an arrow spawning, a
//! creeper blast) are returned to the caller, because players are not in the entity store and
//! explosions are a region concern.

use crate::mob::{self, MobKind};
use crate::path;
use crate::physics::{self, BlockGetter};
use crate::rng::Rng;
use crate::store::{EntityStore, Flags};
use ferrite_math::Vec3;
use std::collections::HashSet;
use uuid::Uuid;

/// Vanilla `LivingEntity` gravity per tick.
pub const GRAVITY: f64 = 0.08;
/// Horizontal drag in the air (`0.91`).
pub const AIR_FRICTION: f64 = 0.91;
/// Vertical drag in the air (`0.98`).
pub const AIR_DRAG: f64 = 0.98;
/// Default block slipperiness.
pub const GROUND_FRICTION: f64 = 0.6;
/// Vanilla `LookAtPlayerGoal` range in blocks (monsters 8.0, farm animals 6.0).
pub const LOOK_DISTANCE: f64 = 8.0;
pub const ANIMAL_LOOK_DISTANCE: f64 = 6.0;
/// Vanilla `TemptGoal` search and stop distances.
pub const TEMPT_RANGE: f64 = 10.0;
const TEMPT_STOP_DISTANCE_SQR: f64 = 2.5 * 2.5;
/// Vanilla `AvoidEntityGoal(Wolf, 6.0f, 1.0, 1.2)` radius for skeletons.
const WOLF_AVOID_RADIUS: f64 = 6.0;
/// Player bounding-box height used for vertical melee reach checks.
const PLAYER_HEIGHT: f64 = 1.8;
/// Eye height of a player, added to the player's feet.
pub const PLAYER_EYE_HEIGHT: f64 = 1.62;
/// Share of the movement speed used as per-tick acceleration (friction caps terminal speed).
const WALK_ACCELERATION: f64 = 0.21;
/// Vanilla `LivingEntity` step-up height (slabs, stairs, paths, snow layers).
const MOB_STEP_HEIGHT: f64 = 0.6;
/// Vanilla creeper trigger radius (`Creeper` starts swelling within 3 blocks).
pub const CREEPER_TRIGGER_DISTANCE: f64 = 3.0;
/// Vanilla creeper fuse length (30 swell ticks).
pub const CREEPER_SWELL_TICKS: u16 = 30;
/// A lit creeper keeps swelling until its target is farther than this (`SwellGoal`, 7 blocks).
pub const CREEPER_ABORT_DISTANCE: f64 = 7.0;
/// Vanilla creeper explosion power.
pub const CREEPER_POWER: f32 = 3.0;
/// Skeleton bow range (`RangedBowAttackGoal` attack radius is 15.0F).
pub const SKELETON_RANGE: f64 = 15.0;
/// Skeleton arrows do a base 2 damage.
pub const SKELETON_ARROW_DAMAGE: f32 = 2.0;
/// Ticks between skeleton shots.
pub const SKELETON_COOLDOWN: u16 = 40;
/// `RangedBowAttackGoal` shoots every 20 ticks on Hard, every 40 otherwise.
pub const SKELETON_COOLDOWN_HARD: u16 = 20;
/// Vanilla `AbstractSkeleton.performRangedAttack`: `14 - difficulty * 4` inaccuracy. Normal and
/// easy are not distinguished here, so both use Normal's 6; Hard uses 2.
pub const SKELETON_INACCURACY: f64 = 6.0;
pub const SKELETON_INACCURACY_HARD: f64 = 2.0;
/// Vanilla `Zombie.TOTAL_CONVERSION_TIME` (600 ticks / 30 seconds underwater).
pub const DROWNED_CONVERSION_TICKS: u16 = 600;
/// Blaze ranged/melee attack constants (`Blaze.java`, `Blaze$BlazeAttackGoal`).
pub const BLAZE_MELEE_DIST_SQR: f64 = 4.0;
pub const BLAZE_MELEE_COOLDOWN: u16 = 20;
pub const BLAZE_CHARGE_TICKS: u16 = 60;
pub const BLAZE_VOLLEY_INTERVAL: u16 = 6;
pub const BLAZE_COOLDOWN_TICKS: u16 = 100;
pub const BLAZE_HEIGHT_OFFSET_TICKS: u16 = 100;
pub const BLAZE_HOVER_ACCEL: f64 = 0.30000001192092896;
pub const BLAZE_FALL_DRAG: f64 = 0.6;
pub const BLAZE_FIREBALL_SPEED: f64 = crate::arrow::SMALL_FIREBALL_ACCELERATION;
/// Witch attack constants (`Witch.java`, `RangedAttackGoal(this, 1.0, 60, 10.0f)`).
pub const WITCH_ATTACK_RADIUS: f64 = 10.0;
pub const WITCH_ATTACK_INTERVAL: u16 = 60;
pub const WITCH_SEE_TIME_STOP: u16 = 5;
pub const WITCH_THROW_INACCURACY: f64 = 8.0;
/// Piglin crossbow constants (`PiglinAi.java`, `CrossbowAttack.java`, `CrossbowItem.java`).
pub const PIGLIN_CROSSBOW_RANGE: f64 = 8.0;
pub const PIGLIN_BACKUP_DISTANCE: f64 = 5.0;
pub const PIGLIN_BACKUP_SPEED: f64 = 0.75;
pub const PIGLIN_CROSSBOW_CHARGE_TICKS: u16 = 25;
pub const PIGLIN_CROSSBOW_SPEED: f64 = 1.6;
pub const PIGLIN_CROSSBOW_DAMAGE: f32 = 2.0;

/// Vanilla `Zombie.hurtServer` reinforcement roll (`SPAWN_REINFORCEMENTS_CHANCE` base 0.1 on Hard).
pub fn zombie_reinforcement_roll(hard: bool, rng: &mut Rng) -> bool {
    hard && rng.next_int(10) == 0
}

/// Vanilla baby zombies move 1.5x as fast (`Zombie.SPEED_MODIFIER_BABY`).
pub const BABY_ZOMBIE_SPEED: f64 = 1.5;
/// Ticks between melee hits.
pub const MELEE_COOLDOWN: u16 = 20;
/// How often a hostile mob recomputes its path.
pub const REPATH_TICKS: u16 = 20;
/// Hostile mobs move a little faster than their wander gait while chasing.
const CHASE_ACCELERATION: f64 = 1.35;

/// Distance at which a lead starts to pull (`Leashable.ELASTIC_DISTANCE_START`).
const LEASH_PULL: f64 = 6.0;
/// Distance at which a lead snaps (`Leashable.LEASH_TOO_FAR_DIST`).
const LEASH_SNAP: f64 = 12.0;
/// Caravan llamas hurry after their head (`LlamaFollowCaravanGoal` speed modifier 2.1).
const CARAVAN_SPEED: f64 = 2.1;
/// Speed lost for every extra link behind the head, so a long line does not bunch up.
const CARAVAN_SPEED_PER_LINK: f64 = 0.1;
/// Floor for the long-line speed penalty.
const CARAVAN_MIN_SPEED: f64 = 1.5;
/// A follower walks once it is more than three blocks behind its head.
const CARAVAN_FOLLOW_DISTANCE: f64 = 3.0;
/// Closer than this, a follower steps back instead of bumping the llama ahead.
const CARAVAN_BUMP_DISTANCE: f64 = 1.5;
/// A line lets go past sixteen blocks.
const CARAVAN_LEAVE_DISTANCE: f64 = 16.0;
/// A free llama joins a line within nine blocks.
const CARAVAN_JOIN_DISTANCE: f64 = 9.0;
/// Maximum number of llamas in one caravan, including its head.
const CARAVAN_LIMIT: usize = 10;

/// Where the holder of a leashed animal is when it is more than six blocks away; a lead that
/// stretches past twelve snaps. `None` when the animal is not on a lead or the lead is slack.
///
/// The holder may be a player in this region, another mob in this store (a wandering trader),
/// or an entity owned by another region, whose last known position the caller passes in
/// `holders`. A holder that is gone takes the lead with it: the mob clears it once and breaks.
fn leash_pull(
    store: &mut EntityStore,
    key: crate::EntityKey,
    players: &[Target],
    holders: &[(Uuid, Vec3)],
    start: Vec3,
    events: &mut Vec<MobEvent>,
) -> Option<Vec3> {
    let (holder, _) = mob::state(store, key)?.leash?;
    // The holder is a player in this region, another mob, or an entity owned by another
    // region whose position the server resolved into `holders`.
    let holder_pos = players
        .iter()
        .find(|t| t.uuid == holder)
        .map(|t| t.pos)
        .or_else(|| {
            let k = store
                .keys()
                .iter()
                .copied()
                .find(|&k| store.data(k).is_some_and(|d| d.uuid == holder))?;
            store.pos(k)
        })
        .or_else(|| {
            holders
                .iter()
                .find(|(uuid, _)| *uuid == holder)
                .map(|(_, pos)| *pos)
        });
    let Some(holder_pos) = holder_pos else {
        // Removed, logged out or its region unloaded: never keep a stale holder.
        mob::state_mut(store, key)?.leash = None;
        events.push(MobEvent::LeashBroke { key });
        return None;
    };
    let dx = holder_pos.x - start.x;
    let dz = holder_pos.z - start.z;
    let distance = (dx * dx + dz * dz).sqrt();
    if distance > LEASH_SNAP {
        mob::state_mut(store, key)?.leash = None;
        events.push(MobEvent::LeashBroke { key });
        return None;
    }
    (distance > LEASH_PULL).then_some(holder_pos)
}

/// What a caravan llama does this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
enum CaravanAction {
    /// Close enough to the llama ahead: stand still instead of bumping into it.
    Hold,
    /// Walk toward the target; `speed` multiplies the llama's walk acceleration.
    Walk { target: Vec3, speed: f64 },
}

/// Where a llama in a caravan walks to: its head, once it is more than three blocks behind.
/// A free adult llama within nine blocks of a led or trailing llama with nobody behind it joins
/// the line; the line lets go past sixteen blocks. A follower that lost its head (death, another
/// region or a broken chain) looks for a new line in the same tick; a line that would close into
/// a loop is never joined.
fn caravan_action(
    store: &mut EntityStore,
    key: crate::EntityKey,
    start: Vec3,
) -> Option<CaravanAction> {
    let (kind, leashed, caravan, age, uuid) = {
        let state = mob::state(store, key)?;
        (
            state.kind,
            state.leash.is_some(),
            state.caravan,
            state.age,
            store.data(key)?.uuid,
        )
    };
    if !kind.is_llama() || leashed {
        return None;
    }
    if let Some(head) = caravan {
        let head_key = entity_by_uuid(store, head);
        let head_pos = head_key
            .filter(|&k| mob::state(store, k).is_some_and(|s| !s.is_dead()))
            .and_then(|k| store.pos(k));
        if let Some(pos) = head_pos {
            let (dx, dz) = (pos.x - start.x, pos.z - start.z);
            let distance = (dx * dx + dz * dz).sqrt();
            let looped = head_key.is_some_and(|head_key| caravan_contains(store, head_key, uuid));
            if distance > CARAVAN_LEAVE_DISTANCE || looped {
                mob::state_mut(store, key)?.caravan = None;
            } else {
                return Some(caravan_action_for(store, key, start, pos, distance));
            }
        } else {
            // The head died, left the store or changed region.
            mob::state_mut(store, key)?.caravan = None;
        }
    }
    if age < 0 {
        return None;
    }
    let found = nearest_caravan_head(store, key, start, uuid)?;
    mob::state_mut(store, key)?.caravan = Some(found);
    let pos = store.pos(entity_by_uuid(store, found)?)?;
    let (dx, dz) = (pos.x - start.x, pos.z - start.z);
    let distance = (dx * dx + dz * dz).sqrt();
    Some(caravan_action_for(store, key, start, pos, distance))
}

fn caravan_action_for(
    store: &EntityStore,
    key: crate::EntityKey,
    start: Vec3,
    head: Vec3,
    distance: f64,
) -> CaravanAction {
    if distance > CARAVAN_FOLLOW_DISTANCE {
        return CaravanAction::Walk {
            target: head,
            speed: caravan_speed(caravan_depth(store, key)),
        };
    }
    if distance < CARAVAN_BUMP_DISTANCE {
        // Keep a body width between the llamas instead of walking into the one ahead.
        return CaravanAction::Walk {
            target: Vec3::new(
                start.x + (start.x - head.x),
                start.y,
                start.z + (start.z - head.z),
            ),
            speed: 1.0,
        };
    }
    CaravanAction::Hold
}

/// Nearest llama with nobody behind it that this free llama may join. The scan is ordered by
/// distance and then UUID, so the same line is chosen regardless of entity-store order.
fn nearest_caravan_head(
    store: &EntityStore,
    key: crate::EntityKey,
    start: Vec3,
    uuid: Uuid,
) -> Option<Uuid> {
    store
        .keys()
        .iter()
        .copied()
        .filter_map(|candidate| {
            if candidate == key {
                return None;
            }
            let other = mob::state(store, candidate)?;
            if !other.kind.is_llama() || other.is_dead() {
                return None;
            }
            if other.leash.is_none() && other.caravan.is_none() {
                return None;
            }
            let data = store.data(candidate)?;
            if data.uuid == uuid || caravan_contains(store, candidate, uuid) {
                return None;
            }
            // Count this candidate and each llama it follows back to the line's head.
            if caravan_chain_len(store, candidate) >= CARAVAN_LIMIT {
                return None;
            }
            let pos = store.pos(candidate)?;
            let (dx, dz) = (pos.x - start.x, pos.z - start.z);
            let distance = dx * dx + dz * dz;
            if distance > CARAVAN_JOIN_DISTANCE * CARAVAN_JOIN_DISTANCE {
                return None;
            }
            let has_tail = store
                .keys()
                .iter()
                .any(|&tail| mob::state(store, tail).is_some_and(|s| s.caravan == Some(data.uuid)));
            if has_tail {
                return None;
            }
            Some((data.uuid, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)))
        .map(|(head, _)| head)
}

fn entity_by_uuid(store: &EntityStore, uuid: Uuid) -> Option<crate::EntityKey> {
    store
        .keys()
        .iter()
        .copied()
        .find(|&candidate| store.data(candidate).is_some_and(|data| data.uuid == uuid))
}

/// Whether following `key` upwards reaches `uuid` or an existing loop.
fn caravan_contains(store: &EntityStore, key: crate::EntityKey, uuid: Uuid) -> bool {
    let mut seen = HashSet::new();
    let mut current = Some(key);
    while let Some(key) = current {
        let Some(data) = store.data(key) else {
            return false;
        };
        if data.uuid == uuid || !seen.insert(data.uuid) {
            return true;
        }
        current = mob::state(store, key)
            .and_then(|state| state.caravan)
            .and_then(|next| entity_by_uuid(store, next));
    }
    false
}

fn caravan_chain_len(store: &EntityStore, key: crate::EntityKey) -> usize {
    let mut length = 1;
    let mut seen = HashSet::new();
    let mut current = key;
    while let Some(uuid) = mob::state(store, current).and_then(|state| state.caravan) {
        if length >= CARAVAN_LIMIT || !seen.insert(uuid) {
            return CARAVAN_LIMIT;
        }
        let Some(parent) = entity_by_uuid(store, uuid) else {
            return length;
        };
        length += 1;
        current = parent;
    }
    length
}

/// Number of links between `key` and the head of its line (0 for a free llama or head).
fn caravan_depth(store: &EntityStore, key: crate::EntityKey) -> usize {
    let mut depth = 0;
    let mut seen = HashSet::new();
    let mut current = key;
    while let Some(uuid) = mob::state(store, current).and_then(|state| state.caravan) {
        if depth >= CARAVAN_LIMIT || !seen.insert(uuid) {
            break;
        }
        let Some(parent) = entity_by_uuid(store, uuid) else {
            break;
        };
        depth += 1;
        current = parent;
    }
    depth
}

/// `CARAVAN_SPEED` for a llama directly behind its head, slowed for each extra link so a long
/// line does not bunch up into the head.
fn caravan_speed(depth: usize) -> f64 {
    let ahead = depth.saturating_sub(1) as f64;
    (CARAVAN_SPEED - ahead * CARAVAN_SPEED_PER_LINK).max(CARAVAN_MIN_SPEED)
}

/// Light level at which `getLightLevelDependentMagicValue` reaches 0.5 (overworld ambient 0).
const SPIDER_BRIGHT_LIGHT: u8 = 12;

fn spider_sees_bright_light(blocks: &impl BlockGetter, pos: Vec3, kind: MobKind) -> bool {
    let eye = pos.y + f64::from(kind.props().eye_height);
    blocks
        .brightness(
            pos.x.floor() as i32,
            eye.floor() as i32,
            pos.z.floor() as i32,
        )
        .is_some_and(|level| level >= SPIDER_BRIGHT_LIGHT)
}

/// A player as the goals see it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target {
    pub uuid: Uuid,
    pub pos: Vec3,
}

/// Cross-region per-player state needed by Witch potion choice and Piglin gold neutrality.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PlayerSnapshot {
    pub health: f32,
    pub has_slowness: bool,
    pub has_poison: bool,
    pub has_weakness: bool,
    pub wearing_gold: bool,
    pub sneaking: bool,
    pub vel: Vec3,
}

impl PlayerSnapshot {
    /// Default fallback used when no snapshot is published (treats target as full health).
    pub fn full_health() -> Self {
        Self {
            health: 20.0,
            ..Self::default()
        }
    }
}

/// What a mob wants the region to do this tick.
#[derive(Debug, Clone, PartialEq)]
pub enum MobEvent {
    /// A melee attack landed on a player.
    Melee {
        player: Uuid,
        damage: f32,
        from: Vec3,
        /// The attacking mob, so a raised shield can knock it back.
        attacker: crate::EntityKey,
    },
    /// A ranged mob fires an arrow (skeleton/stray/bogged bow or piglin crossbow).
    Arrow {
        from: Vec3,
        direction: Vec3,
        damage: f32,
        speed: f64,
        effect: Option<crate::effect::MobEffectInstance>,
        owner: Option<Uuid>,
    },
    /// A blaze launches a `minecraft:small_fireball`.
    SmallFireball {
        from: Vec3,
        velocity: Vec3,
        owner: Option<Uuid>,
    },
    /// A witch throws a `minecraft:splash_potion` carrying `potion`.
    SplashPotion {
        from: Vec3,
        velocity: Vec3,
        owner: Option<Uuid>,
        potion: mob::WitchPotion,
    },
    /// A mob applied a non-instant potion effect to itself (so the region can notify trackers).
    EffectAdded {
        key: crate::EntityKey,
        instance: crate::effect::MobEffectInstance,
    },
    /// An adult chicken laid an egg at `at`.
    LayEgg { at: Vec3 },
    /// A turtle laid a `minecraft:turtle_egg` cluster at `at` (1..=4 eggs). The region restores
    /// `has_egg` if it cannot place the block.
    LayTurtleEgg {
        key: crate::EntityKey,
        at: [i32; 3],
        eggs: u8,
    },
    /// A lead stretched too far and snapped.
    LeashBroke { key: crate::EntityKey },
    /// A llama spits at a player.
    Spit { from: Vec3, direction: Vec3 },
    /// The ender dragon spits a fireball (`dragon_fireball`) at a player.
    DragonFireball { from: Vec3, direction: Vec3 },
    /// The perched dragon breathes: an `AreaEffectCloud` forms at `pos`.
    Breath { pos: Vec3, owner: Option<Uuid> },
    /// A creeper's fuse ran out.
    Explode { center: Vec3, power: f32 },
    /// Two animals bred; the baby is already in the store.
    Bred(crate::breed::Bred),
    /// A zombie is chewing on the door at `at` (its lower half): `progress` is the 0..=9 crack
    /// stage, `broken` is set on the tick the door goes.
    BreakDoor {
        at: [i32; 3],
        progress: u8,
        broken: bool,
    },
    /// A door-using mob (villager or piglin) opens (`open = true`) or closes (`open = false`) a
    /// hand-openable door at `at` (its lower half), applied only by the owning region.
    SetDoor { at: [i32; 3], open: bool },
    /// A creeper's fuse started: the region plays the hiss at `at`
    /// (`crate::creeper::HISS_SOUND`).
    Hiss { at: Vec3 },
    /// A sheep ate at `at`: the region turns a grass block into dirt (`grass`) or removes the
    /// plant (`!grass`) when `mobGriefing` allows it.
    EatBlock { at: [i32; 3], grass: bool },
    /// A calm enderman wants to remove the holdable block at `at`; the region owns the
    /// `mobGriefing` and tag checks and sets the carried default state.
    TakeBlock { key: crate::EntityKey, at: [i32; 3] },
    /// A carrying enderman wants to place `state` at `at`; the region validates support and
    /// clears the carried state only when the placement succeeds.
    PlaceCarried {
        key: crate::EntityKey,
        at: [i32; 3],
        state: u16,
    },
    /// An enderman blinked from `from` to `to`; the region sends the teleport sound and
    /// portal particles at the old position.
    Teleport { from: Vec3, to: Vec3 },
    /// An entity event for the players tracking `key` (hearts, smoke).
    Status { key: crate::EntityKey, status: u8 },
    /// A submerged zombie finished its drowned conversion timer (`Zombie.inWaterTime`).
    DrownedConvert { key: crate::EntityKey },
    /// A piglin finished admiring a gold ingot and is ready to roll and throw a barter drop.
    PiglinBarter {
        key: crate::EntityKey,
        at: Vec3,
        toward: Option<Vec3>,
    },
    /// A creeper carrying active potion effects exploded (`Creeper.spawnLingeringCloud`).
    CreeperPotionCloud {
        center: Vec3,
        effects: Vec<crate::effect::MobEffectInstance>,
        owner: Option<Uuid>,
    },
    /// A melee attack with custom knockback, vertical launch, or poison (Iron Golem fling, Goat
    /// ram, Bee sting).
    SpecialMelee {
        player: Uuid,
        damage: f32,
        from: Vec3,
        attacker: crate::EntityKey,
        knockback_xz: f64,
        launch_y: f64,
        poison_ticks: i32,
    },
    /// A snow golem throws a snowball at a hostile mob.
    Snowball {
        from: Vec3,
        velocity: Vec3,
        owner: Option<Uuid>,
    },
    /// A snow golem wants to place a snow layer at `at`; the region checks `mobGriefing`, chunk
    /// ownership and support before placing `minecraft:snow`.
    SnowTrail { at: [i32; 3] },
    /// A frog ate a tiny `Slime` or `MagmaCube` with its tongue (`ShootTongue`).
    FrogEat {
        prey: crate::EntityKey,
        prey_kind: MobKind,
        frog_variant: Option<u8>,
        at: Vec3,
    },
    /// A mob plays a sound effect at `at`.
    Sound {
        at: Vec3,
        sound: &'static str,
        volume: f32,
        pitch: f32,
    },
    /// A goat rammed into a `#minecraft:snaps_goat_horn` block and dropped a horn.
    GoatSnapHorn {
        key: crate::EntityKey,
        at: Vec3,
        is_screaming: bool,
    },
    /// A fox harvested a mature `minecraft:sweet_berry_bush` (`FoxEatBerriesGoal`).
    HarvestSweetBerries { at: [i32; 3], pos: Vec3 },
    /// A rabbit nibbled a `minecraft:carrots` crop stage (`Rabbit.RaidGardenGoal`).
    RabbitNibbleCarrots { at: [i32; 3], new_age: u8 },
    /// A witch healed an injured nearby raider (`NearestHealableRaiderTargetGoal`).
    WitchHealRaider {
        witch: crate::EntityKey,
        victim: crate::EntityKey,
        at: Vec3,
    },
}

/// Nearest alive tiny `Slime` or `MagmaCube` within `max_distance` for `Frog.canEat`.
fn nearest_frog_prey(
    store: &EntityStore,
    from: Vec3,
    max_distance: f64,
) -> Option<(crate::EntityKey, Vec3, MobKind)> {
    let mut best: Option<(f64, crate::EntityKey, Vec3, MobKind)> = None;
    let max_sq = max_distance * max_distance;
    for &key in store.keys() {
        if !mob::is_mob(store, key) {
            continue;
        }
        let Some(state) = mob::state(store, key) else {
            continue;
        };
        if state.is_dead() || !crate::frog::is_edible_cube(state.kind, state.cube_size) {
            continue;
        }
        let Some(pos) = store.pos(key) else {
            continue;
        };
        let dx = pos.x - from.x;
        let dy = pos.y - from.y;
        let dz = pos.z - from.z;
        let dist_sq = dx * dx + dy * dy + dz * dz;
        if dist_sq <= max_sq && best.is_none_or(|(b, ..)| dist_sq < b) {
            best = Some((dist_sq, key, pos, state.kind));
        }
    }
    best.map(|(_, key, pos, kind)| (key, pos, kind))
}

fn nearest_mob_of(
    store: &EntityStore,
    from: Vec3,
    kind: MobKind,
    max_distance: f64,
    adult_only: bool,
) -> Option<(crate::EntityKey, Vec3)> {
    let max_sq = max_distance * max_distance;
    let mut best: Option<(f64, crate::EntityKey, Vec3)> = None;
    for &key in store.keys() {
        let Some(state) = mob::state(store, key) else {
            continue;
        };
        if state.kind != kind || state.is_dead() || (adult_only && state.age < 0) {
            continue;
        }
        let Some(pos) = store.pos(key) else {
            continue;
        };
        let d2 = pos.distance_to_sqr(from);
        if d2 <= max_sq && best.is_none_or(|(b, ..)| d2 < b) {
            best = Some((d2, key, pos));
        }
    }
    best.map(|(_, key, pos)| (key, pos))
}

fn nearest_injured_raider(
    store: &EntityStore,
    self_key: crate::EntityKey,
    from: Vec3,
    max_distance: f64,
) -> Option<(crate::EntityKey, Vec3)> {
    let max_sq = max_distance * max_distance;
    let mut best: Option<(f64, crate::EntityKey, Vec3)> = None;
    for &key in store.keys() {
        if key == self_key {
            continue;
        }
        let Some(state) = mob::state(store, key) else {
            continue;
        };
        if !state.kind.is_raider() || state.is_dead() || state.health >= state.kind.max_health() {
            continue;
        }
        let Some(pos) = store.pos(key) else {
            continue;
        };
        let d2 = pos.distance_to_sqr(from);
        if d2 <= max_sq && best.is_none_or(|(b, ..)| d2 < b) {
            best = Some((d2, key, pos));
        }
    }
    best.map(|(_, key, pos)| (key, pos))
}

/// Wraps an angle into `[-180, 180)`, like vanilla `Mth.wrapDegrees`.
pub fn wrap_degrees(angle: f32) -> f32 {
    let mut wrapped = angle % 360.0;
    if wrapped >= 180.0 {
        wrapped -= 360.0;
    } else if wrapped < -180.0 {
        wrapped += 360.0;
    }
    wrapped
}

/// The yaw that moves towards `(dx, dz)` with the server convention (`-sin(yaw), cos(yaw)`).
fn yaw_towards(dx: f64, dz: f64) -> f32 {
    (-dx).atan2(dz).to_degrees() as f32
}

/// The body yaw and pitch that make a mob at `pos` look at the nearest player within
/// [`LOOK_DISTANCE`], if any. `eye_height` is the mob's own eye offset above its feet.
pub fn look_at(pos: Vec3, eye_height: f64, players: &[Vec3]) -> Option<(f32, f32)> {
    look_at_within(pos, eye_height, players, LOOK_DISTANCE)
}

fn look_at_within(
    pos: Vec3,
    eye_height: f64,
    players: &[Vec3],
    max_distance: f64,
) -> Option<(f32, f32)> {
    let eye = pos.y + eye_height;
    let mut best: Option<(f64, Vec3)> = None;
    for player in players {
        let dx = player.x - pos.x;
        let dz = player.z - pos.z;
        let horizontal = (dx * dx + dz * dz).sqrt();
        if horizontal > max_distance || (dx == 0.0 && dz == 0.0 && player.y == eye) {
            continue;
        }
        if best.is_none_or(|(distance, _)| horizontal < distance) {
            best = Some((horizontal, *player));
        }
    }
    let (horizontal, target) = best?;
    look_at_point(pos, eye_height, target, horizontal)
}

fn look_at_point(pos: Vec3, eye_height: f64, target: Vec3, horizontal: f64) -> Option<(f32, f32)> {
    let eye = pos.y + eye_height;
    let dx = target.x - pos.x;
    let dz = target.z - pos.z;
    if dx == 0.0 && dz == 0.0 && target.y == eye {
        return None;
    }
    let dy = target.y + PLAYER_EYE_HEIGHT - eye;
    let yaw = if dx == 0.0 && dz == 0.0 {
        0.0
    } else {
        f64::from(wrap_degrees(dz.atan2(dx).to_degrees() as f32 - 90.0))
    };
    let pitch = -(dy.atan2(horizontal)).to_degrees();
    Some((yaw as f32, wrap_degrees(pitch as f32)))
}

/// The nearest player within `range` blocks horizontally and eight blocks vertically.
fn in_target_range(target: &Target, pos: Vec3, range: f64) -> bool {
    let dx = target.pos.x - pos.x;
    let dz = target.pos.z - pos.z;
    dx * dx + dz * dz <= range * range && (target.pos.y - pos.y).abs() <= 8.0
}

/// The player a hostile mob hunts: the current target while it stays in range (vanilla goals
/// only re-select after the target is lost, so a mob that was hurt keeps chasing its attacker),
/// otherwise the nearest player.
fn select_target(
    players: &[Target],
    current: Option<Uuid>,
    pos: Vec3,
    range: f64,
) -> Option<&Target> {
    current
        .and_then(|uuid| players.iter().find(|target| target.uuid == uuid))
        .filter(|target| in_target_range(target, pos, range))
        .or_else(|| nearest_target(players, pos, range))
}

/// The nearest adult of the same kind within eight blocks that a baby should trail
/// (`FollowParentGoal`); `None` once the baby is within three blocks of it.
fn follow_parent(store: &EntityStore, key: crate::EntityKey, start: Vec3) -> Option<Vec3> {
    let me = mob::state(store, key)?;
    if me.age >= 0 || !me.kind.is_animal() {
        return None;
    }
    let kind = me.kind;
    let nearest = store
        .keys()
        .iter()
        .copied()
        .filter(|&other| other != key)
        .filter(|&other| mob::state(store, other).is_some_and(|m| m.kind == kind && m.age >= 0))
        .filter_map(|other| store.pos(other))
        .map(|pos| (pos, (pos.x - start.x).powi(2) + (pos.z - start.z).powi(2)))
        .filter(|(_, d)| *d <= 64.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    (nearest.1 >= 9.0).then_some(nearest.0)
}

fn nearest_target(players: &[Target], pos: Vec3, range: f64) -> Option<&Target> {
    players
        .iter()
        .filter(|target| in_target_range(target, pos, range))
        .min_by(|a, b| {
            let da = (a.pos.x - pos.x).powi(2) + (a.pos.z - pos.z).powi(2);
            let db = (b.pos.x - pos.x).powi(2) + (b.pos.z - pos.z).powi(2);
            da.total_cmp(&db)
        })
}

/// Advances every mob's clocks, goals and movement, applies fall damage and runs despawn rules.
/// This wrapper is for passive-only callers and tests; it reports no events.
pub fn tick(store: &mut EntityStore, blocks: &impl BlockGetter, players: &[Vec3], rng: &mut Rng) {
    let targets: Vec<Target> = players
        .iter()
        .enumerate()
        .map(|(i, pos)| Target {
            uuid: Uuid::from_u128(i as u128 + 1),
            pos: *pos,
        })
        .collect();
    tick_with(store, blocks, &targets, rng);
}

/// The hostile and passive goals, movement and despawn rules for one tick.
pub fn tick_with(
    store: &mut EntityStore,
    blocks: &impl BlockGetter,
    players: &[Target],
    rng: &mut Rng,
) -> Vec<MobEvent> {
    tick_with_owner_attacks_and_holders(store, blocks, players, &[], &[], rng)
}

/// Advances mob goals while allowing tamed wolves to target a mob their owner attacked this tick.
pub fn tick_with_owner_attacks(
    store: &mut EntityStore,
    blocks: &impl BlockGetter,
    players: &[Target],
    owner_attacks: &[(Uuid, crate::EntityKey)],
    rng: &mut Rng,
) -> Vec<MobEvent> {
    tick_with_owner_attacks_and_holders(store, blocks, players, owner_attacks, &[], rng)
}

/// Like [`tick_with`], with the last known positions of leash holders owned by other regions.
/// A lead whose holder is not in this store and not one of `players` follows `holders` and
/// snaps at the same distances as a local holder.
pub fn tick_with_holders(
    store: &mut EntityStore,
    blocks: &impl BlockGetter,
    players: &[Target],
    holders: &[(Uuid, Vec3)],
    rng: &mut Rng,
) -> Vec<MobEvent> {
    tick_with_owner_attacks_and_holders(store, blocks, players, &[], holders, rng)
}

/// Runs mob goals with both owner-attack targets and cross-region leash-holder positions.
pub fn tick_with_owner_attacks_and_holders(
    store: &mut EntityStore,
    blocks: &impl BlockGetter,
    players: &[Target],
    owner_attacks: &[(Uuid, crate::EntityKey)],
    holders: &[(Uuid, Vec3)],
    rng: &mut Rng,
) -> Vec<MobEvent> {
    tick_full(
        store,
        blocks,
        players,
        &MobRules::default(),
        owner_attacks,
        holders,
        rng,
    )
}

/// World rules the mob goals read.
#[derive(Debug, Default)]
pub struct MobRules {
    /// Where players look and whether they wear a carved pumpkin, for the enderman stare.
    /// Players without an entry are not looking at anything.
    pub gazes: std::collections::HashMap<Uuid, crate::enderman::Gaze>,
    /// Main-hand and off-hand item names held by players, for `TemptGoal`.
    pub held_items: std::collections::HashMap<Uuid, [Option<&'static str>; 2]>,
    /// Whether sunlight burns monsters here (the `monsters_burn` environment attribute).
    pub monsters_burn: bool,
    /// Optional explicit `Level.isBrightOutside()` (daytime sky-light state). When `None`, light
    /// checks fall back to `BlockGetter::brightness`.
    pub bright_outside: Option<bool>,
    /// `mobGriefing`: zombies only break doors when it is on.
    pub griefing: bool,
    /// Whether it is raining (endermen are hurt by rain under open sky).
    pub raining: bool,
    /// Creative and spectator players: seen by every goal except hostile target selection.
    pub untargetable: std::collections::HashSet<Uuid>,
    /// Per-player health, active witch-relevant effects, velocity, and golden armor presence.
    pub player_info: std::collections::HashMap<Uuid, PlayerSnapshot>,
    /// Hard difficulty (skeletons shoot twice as often).
    pub hard: bool,
    /// Peaceful difficulty (`difficulty == 0`): hostile monsters despawn immediately.
    pub peaceful: bool,
}

/// Reusable buffers for the per-tick mob goal pass. The region keeps one across ticks so the
/// key list and per-player vectors do not allocate on every tick.
#[derive(Default)]
pub struct AiScratch {
    keys: Vec<crate::EntityKey>,
    player_positions: Vec<Vec3>,
    owners: Vec<(Uuid, Vec3)>,
    cats: Vec<Vec3>,
    wolves: Vec<Vec3>,
    stare_candidates: Vec<crate::enderman::Candidate>,
    piglin_candidates: Vec<Target>,
}

/// [`tick_with_owner_attacks_and_holders`] where `hard` is the Hard difficulty (skeletons shoot
/// twice as often) and `untargetable` players (creative and
/// spectator) are still seen by every other goal (look at, follow, tempt, owner, despawn
/// range) but are never picked as a hostile mob's target.
pub fn tick_full(
    store: &mut EntityStore,
    blocks: &impl BlockGetter,
    players: &[Target],
    rules: &MobRules,
    owner_attacks: &[(Uuid, crate::EntityKey)],
    holders: &[(Uuid, Vec3)],
    rng: &mut Rng,
) -> Vec<MobEvent> {
    tick_full_with_scratch(
        store,
        blocks,
        players,
        rules,
        owner_attacks,
        holders,
        rng,
        &mut AiScratch::default(),
    )
}

/// [`tick_full`] with caller-owned reusable buffers. The buffers are cleared and refilled, so
/// passing a fresh default scratch is equivalent to [`tick_full`].
#[allow(clippy::too_many_arguments)]
pub fn tick_full_with_scratch(
    store: &mut EntityStore,
    blocks: &impl BlockGetter,
    players: &[Target],
    rules: &MobRules,
    owner_attacks: &[(Uuid, crate::EntityKey)],
    holders: &[(Uuid, Vec3)],
    rng: &mut Rng,
    scratch: &mut AiScratch,
) -> Vec<MobEvent> {
    let MobRules {
        untargetable,
        player_info,
        hard,
        peaceful,
        gazes,
        held_items,
        raining,
        monsters_burn,
        bright_outside,
        griefing,
    } = rules;
    let bow_cooldown = if *hard {
        SKELETON_COOLDOWN_HARD
    } else {
        SKELETON_COOLDOWN
    };
    let bow_inaccuracy = if *hard {
        SKELETON_INACCURACY_HARD
    } else {
        SKELETON_INACCURACY
    };
    let AiScratch {
        keys,
        player_positions,
        owners,
        cats,
        wolves,
        stare_candidates,
        piglin_candidates,
    } = scratch;
    keys.clear();
    keys.extend_from_slice(store.keys());
    player_positions.clear();
    player_positions.extend(players.iter().map(|target| target.pos));
    owners.clear();
    owners.extend(players.iter().map(|target| (target.uuid, target.pos)));
    cats.clear();
    wolves.clear();
    let mut events = Vec::new();
    let hostile_targets: std::borrow::Cow<'_, [Target]> = if untargetable.is_empty() {
        std::borrow::Cow::Borrowed(players)
    } else {
        std::borrow::Cow::Owned(
            players
                .iter()
                .filter(|target| !untargetable.contains(&target.uuid))
                .copied()
                .collect(),
        )
    };
    for &k in keys.iter() {
        if let Some(s) = mob::state(store, k)
            && s.health > 0.0
        {
            if s.kind == MobKind::Cat {
                if let Some(p) = store.pos(k) {
                    cats.push(p);
                }
            } else if s.kind == MobKind::Wolf
                && let Some(p) = store.pos(k)
            {
                wolves.push(p);
            }
        }
    }
    static BOW_AND_CROSSBOW: std::sync::OnceLock<(u16, u16)> = std::sync::OnceLock::new();
    let &(bow_item_id, crossbow_item_id) = BOW_AND_CROSSBOW.get_or_init(|| {
        (
            ferrite_item::item_id("minecraft:bow").unwrap_or(0),
            ferrite_item::item_id("minecraft:crossbow").unwrap_or(0),
        )
    });
    stare_candidates.clear();
    piglin_candidates.clear();
    for &key in keys.iter() {
        if !mob::is_mob(store, key) {
            continue;
        }
        mob::tick_clock(store, key);
        let Some(kind) = mob::state(store, key).map(|state| state.kind) else {
            continue;
        };
        let bbox = store.bbox(key);
        let fluid_depth = bbox.map_or(0.0, |area| physics::fluid_height(blocks, area, false));
        let in_water = fluid_depth > 0.0;
        if matches!(kind, MobKind::Zombie | MobKind::Husk) && bbox.is_some() {
            let mut converted = false;
            if let Some(state) = mob::state_mut(store, key) {
                if in_water {
                    state.in_water_time = state.in_water_time.saturating_add(1);
                    if state.in_water_time == DROWNED_CONVERSION_TICKS {
                        converted = true;
                    }
                } else {
                    state.in_water_time = state.in_water_time.saturating_sub(2);
                }
            }
            if converted {
                events.push(MobEvent::DrownedConvert { key });
            }
        }
        if kind.is_animal() {
            crate::breed::tick_age(store, key);
            // Vanilla `Chicken.aiStep`: an adult lays an egg every 6000 to 11999 ticks.
            if kind == MobKind::Chicken {
                let mut lay = false;
                if let Some(state) = mob::state_mut(store, key)
                    && state.age >= 0
                    && !state.is_chicken_jockey
                {
                    state.egg_time = state.egg_time.saturating_sub(1);
                    if state.egg_time == 0 {
                        state.egg_time =
                            mob::CHICKEN_EGG_MIN + rng.next_int(mob::CHICKEN_EGG_SPAN) as u32;
                        lay = true;
                    }
                }
                if lay && let Some(at) = store.pos(key) {
                    events.push(MobEvent::LayEgg { at });
                }
            }
            // Vanilla `Animal.aiStep`: hearts every half second while in love (event 18).
            if mob::state(store, key).is_some_and(|s| s.love > 0 && s.love % 10 == 0) {
                events.push(MobEvent::Status { key, status: 18 });
            }
        }
        if kind == MobKind::EnderDragon {
            crate::dragon::fly(store, key, players, rng, &mut events);
            continue;
        }
        let mut directed = false;
        let mut eating = false;
        let mut blaze_hover_up = false;
        let start = store.pos(key).unwrap_or_default();
        let flags = store.flags(key).unwrap_or_default();
        let was_on_ground = flags.has(Flags::ON_GROUND);
        let mut walking = None;
        let mut look = None;
        let mut speed = 1.0;

        {
            let Some(state) = mob::state_mut(store, key) else {
                continue;
            };
            state.attack_cooldown = state.attack_cooldown.saturating_sub(1);
            state.bow_timer = state.bow_timer.saturating_sub(1);
            state.panic = state.panic.saturating_sub(1);
            state.path_timer = state.path_timer.saturating_sub(1);
            if matches!(
                state.kind,
                MobKind::Wolf
                    | MobKind::Llama
                    | MobKind::TraderLlama
                    | MobKind::Enderman
                    | MobKind::Piglin
                    | MobKind::ZombifiedPiglin
                    | MobKind::IronGolem
                    | MobKind::Bee
            ) {
                state.anger = state.anger.saturating_sub(1);
                if state.anger == 0 {
                    state.target = None;
                }
            }
        }
        if kind == MobKind::Bee && crate::bee::tick_post_sting(store, key, rng) {
            continue;
        }
        if kind == MobKind::Piglin
            && crate::piglin::tick_admiring(store, key) == crate::piglin::Admire::Barter
        {
            let eye_height = f64::from(kind.props().eye_height);
            let range = kind.follow_range();
            let toward = players
                .iter()
                .filter(|t| {
                    in_target_range(t, start, range)
                        && has_line_of_sight(blocks, start, eye_height, t.pos)
                })
                .min_by(|a, b| {
                    start
                        .distance_to_sqr(a.pos)
                        .total_cmp(&start.distance_to_sqr(b.pos))
                })
                .map(|t| t.pos);
            events.push(MobEvent::PiglinBarter {
                key,
                at: start,
                toward,
            });
        }

        let start_bx = start.x.floor() as i32;
        let start_by = start.y.floor() as i32;
        let start_bz = start.z.floor() as i32;
        let bright_now = *monsters_burn
            && bright_outside.unwrap_or_else(|| {
                blocks
                    .brightness(start_bx, start_by + 1, start_bz)
                    .is_some_and(|l| crate::sun::light_magic_value(l) > 0.5)
            });
        let path_opts = mob_path_options(store, key, *hard, *griefing, bright_now);

        // `Mob.aiStep`: sun-sensitive monsters catch fire in daylight.
        crate::sun::tick_sun(store, key, blocks, *monsters_burn, *raining, rng);
        if matches!(kind, MobKind::Blaze | MobKind::SnowGolem)
            && crate::enderman::in_water_or_rain(store, key, blocks, *raining)
        {
            mob::hurt(store, key, crate::enderman::WATER_DAMAGE, None);
        }
        if kind == MobKind::SnowGolem {
            crate::golem::emit_snow_trail(start, was_on_ground, *griefing, blocks, &mut events);
        }
        if kind == MobKind::Enderman {
            // `EndermanLookForPlayerGoal`: a calm enderman angers at a player who stares at it.
            if mob::state(store, key).is_some_and(|s| s.anger == 0) {
                stare_candidates.clear();
                stare_candidates.extend(hostile_targets.iter().filter_map(|target| {
                    gazes
                        .get(&target.uuid)
                        .map(|gaze| crate::enderman::Candidate {
                            uuid: target.uuid,
                            pos: target.pos,
                            gaze: *gaze,
                        })
                }));
                crate::enderman::tick_stare(store, key, blocks, stare_candidates.as_slice(), rng);
            }
            // `isSensitiveToWater`: one point of drowning damage a tick in water or rain, and
            // nine hurts in ten teleport it away.
            if crate::enderman::in_water_or_rain(store, key, blocks, *raining)
                && matches!(
                    mob::hurt(store, key, crate::enderman::WATER_DAMAGE, None),
                    Some(mob::Hurt::Survived { .. })
                )
                && rng.next_int(10) != 0
            {
                let at = store.pos(key).unwrap_or_default();
                if let Some((from, to)) = blink(store, key, blocks, at, rng) {
                    events.push(MobEvent::Teleport { from, to });
                }
            }
            crate::enderman::sync(store, key);
        }
        let killer_bunny = mob::state(store, key).is_some_and(crate::rabbit::is_killer_bunny);
        if kind == MobKind::Piglin {
            let ended = if let Some(state) = mob::state_mut(store, key)
                && state.dancing
            {
                if state.special_timer > 0 {
                    state.special_timer -= 1;
                }
                if state.special_timer == 0 {
                    state.dancing = false;
                    true
                } else {
                    false
                }
            } else {
                false
            };
            if ended {
                sync_mob_metadata_now(store, key);
            }
        }
        if kind.is_hostile() || killer_bunny {
            let (current, calm, piglin_busy) =
                mob::state(store, key).map_or((None, false, false), |s| {
                    (
                        s.target,
                        s.anger == 0,
                        s.kind == MobKind::Piglin && (s.age < 0 || s.admiring > 0),
                    )
                });
            // Piglin Hoglin hunt + celebration dance (when no player target is active).
            if kind == MobKind::Piglin
                && !piglin_busy
                && let Some((hog_key, hog_pos)) =
                    nearest_mob_of(store, start, MobKind::Hoglin, 16.0, true)
            {
                directed = true;
                let h_dx = hog_pos.x - start.x;
                let h_dz = hog_pos.z - start.z;
                let h_horiz = (h_dx * h_dx + h_dz * h_dz).sqrt();
                look = Some((yaw_towards(h_dx, h_dz), 0.0));
                if h_horiz <= kind.attack_reach() && (hog_pos.y - start.y).abs() <= 2.0 {
                    if mob::state(store, key).is_some_and(|s| s.attack_cooldown == 0) {
                        if let Some(state) = mob::state_mut(store, key) {
                            state.attack_cooldown = MELEE_COOLDOWN;
                        }
                        if matches!(
                            mob::hurt(store, hog_key, kind.attack_damage(), Some(start)),
                            Some(mob::Hurt::Died)
                        ) {
                            if let Some(state) = mob::state_mut(store, key) {
                                state.dancing = true;
                                state.special_timer = 300;
                            }
                            sync_mob_metadata_now(store, key);
                        }
                    }
                } else {
                    walking = follow_path_with(store, key, blocks, start, hog_pos, &path_opts)
                        .or_else(|| Some(yaw_towards(h_dx, h_dz)));
                    speed = chase_speed(store, key);
                }
            } else if kind == MobKind::Witch
                && mob::state(store, key).is_some_and(|s| s.bow_timer == 0 && s.drink_timer == 0)
                && let Some((raider_key, raider_pos)) =
                    nearest_injured_raider(store, key, start, 16.0)
            {
                directed = true;
                let r_dx = raider_pos.x - start.x;
                let r_dz = raider_pos.z - start.z;
                let r_horiz = (r_dx * r_dx + r_dz * r_dz).sqrt();
                look = look_at_point(
                    start,
                    f64::from(kind.props().eye_height),
                    raider_pos,
                    r_horiz,
                );
                if let Some(state) = mob::state_mut(store, key) {
                    state.bow_timer = WITCH_ATTACK_INTERVAL;
                }
                events.push(MobEvent::WitchHealRaider {
                    witch: key,
                    victim: raider_key,
                    at: raider_pos,
                });
            } else if kind.is_zombie_like() {
                // `MoveThroughVillageGoal`: when no player is being hunted, walk to a nearby
                // village bell. The scan is throttled because `nearest_bell` covers a 65x65x9
                // volume and this runs for every idle zombie.
                let bell = match mob::state_mut(store, key) {
                    Some(state) if state.target.is_none() => {
                        state.village_timer = state.village_timer.saturating_sub(1);
                        if state.village_target.is_none() && state.village_timer == 0 {
                            state.village_target = crate::villager::nearest_bell(blocks, start);
                            state.village_timer = VILLAGE_SCAN_TICKS;
                        }
                        state.village_target
                    }
                    _ => None,
                };
                if let Some([bx, by, bz]) = bell {
                    // Walk to the empty block beside the bell: the bell itself is not walkable.
                    let target = Vec3::new(f64::from(bx) - 0.5, f64::from(by), f64::from(bz) + 0.5);
                    let dx = target.x - start.x;
                    let dz = target.z - start.z;
                    if dx * dx + dz * dz <= 4.0 {
                        if let Some(state) = mob::state_mut(store, key) {
                            state.village_target = None;
                            state.village_timer = VILLAGE_SCAN_TICKS;
                        }
                    } else {
                        directed = true;
                        walking = follow_path_with(store, key, blocks, start, target, &path_opts)
                            .or_else(|| Some(yaw_towards(dx, dz)));
                        speed = 1.0;
                    }
                }
            }
            if !directed {
                // An enderman or zombified piglin stays neutral until something angers it.
                // A calm piglin ignores players wearing golden armor (`#minecraft:piglin_safe_armor`).
                // Baby piglins and admiring piglins do not start or pursue attacks.
                let mut target = if piglin_busy
                    || (matches!(kind, MobKind::Enderman | MobKind::ZombifiedPiglin) && calm)
                {
                    None
                } else if kind == MobKind::Piglin && calm {
                    let is_unarmored = |t: &&Target| {
                        !player_info
                            .get(&t.uuid)
                            .is_some_and(|snapshot| snapshot.wearing_gold)
                    };
                    let valid_current = current.filter(|uuid| {
                        !player_info
                            .get(uuid)
                            .is_some_and(|snapshot| snapshot.wearing_gold)
                    });
                    piglin_candidates.clear();
                    piglin_candidates.extend(hostile_targets.iter().filter(is_unarmored).copied());
                    select_target(
                        piglin_candidates.as_slice(),
                        valid_current,
                        start,
                        kind.follow_range(),
                    )
                    .copied()
                } else {
                    select_target(&hostile_targets, current, start, kind.follow_range()).copied()
                };
                // Vanilla spiders are neutral in bright light (`getLightLevelDependentMagicValue`
                // >= 0.5, i.e. light level 12 or more at the eyes): `SpiderTargetGoal` picks no new
                // target, and `SpiderAttackGoal` drops the current one with a 1 in 100 chance a tick.
                if kind.is_spider_like() && spider_sees_bright_light(blocks, start, kind) {
                    target = match target {
                        Some(t) if Some(t.uuid) == current && rng.next_int(100) != 0 => Some(t),
                        _ => None,
                    };
                }
                {
                    let Some(state) = mob::state_mut(store, key) else {
                        continue;
                    };
                    if state.target != target.map(|target| target.uuid) {
                        state.target = target.map(|target| target.uuid);
                        state.path_len = 0;
                        state.path_index = 0;
                        state.path_timer = 0;
                    }
                }
                if kind == MobKind::Witch {
                    let target_dist_sqr = target.map(|t| start.distance_to_sqr(t.pos));
                    tick_witch_drinking(
                        store,
                        key,
                        fluid_depth >= f64::from(kind.props().eye_height),
                        target_dist_sqr,
                        rng,
                        &mut events,
                    );
                }
                if kind == MobKind::Blaze {
                    blaze_hover_up = tick_blaze_hover(store, key, start, target, rng);
                }
                // `AbstractSkeleton` `AvoidEntityGoal(Wolf, 6.0, 1.0, 1.2)` (priority 3) outranks
                // attack goals (priority 4) and `WaterAvoidingRandomStrollGoal` (priority 5).
                let skeleton_flee_wolf = kind
                    .is_skeleton_like()
                    .then(|| flee_entity_direction(start, wolves.as_slice(), WOLF_AVOID_RADIUS))
                    .flatten();
                if let Some(away) = skeleton_flee_wolf {
                    directed = true;
                    walking = Some(yaw_towards(away.x, away.z));
                    speed = 1.2;
                    look = look_at(
                        start,
                        f64::from(kind.props().eye_height),
                        player_positions.as_slice(),
                    );
                } else if let Some(target) = target {
                    let dx = target.pos.x - start.x;
                    let dz = target.pos.z - start.z;
                    let horizontal = (dx * dx + dz * dz).sqrt();
                    let yaw = yaw_towards(dx, dz);
                    let eye_height = f64::from(kind.props().eye_height);
                    look = look_at_point(start, eye_height, target.pos, horizontal)
                        .or(Some((yaw, 0.0)));
                    match kind {
                        MobKind::Creeper => {
                            directed = true;
                            let eye_height = f64::from(kind.props().eye_height);
                            let dist_sqr = start.distance_to_sqr(target.pos);
                            let has_los = has_line_of_sight(blocks, start, eye_height, target.pos);
                            let mut explode = false;
                            let mut hiss = false;
                            let mut swelling = false;
                            let mut power = CREEPER_POWER;
                            if let Some(state) = mob::state_mut(store, key) {
                                // `SwellGoal`: ignite strictly inside 3 blocks with line of sight (or
                                // when flint-and-steel `ignited`), then keep swelling until >7 blocks
                                // away or line of sight is lost.
                                let keep = if state.swell > 0 {
                                    dist_sqr <= CREEPER_ABORT_DISTANCE * CREEPER_ABORT_DISTANCE
                                } else {
                                    dist_sqr < CREEPER_TRIGGER_DISTANCE * CREEPER_TRIGGER_DISTANCE
                                };
                                if state.ignited || (keep && has_los) {
                                    // `Creeper.tick`: the fuse hisses as the swell leaves zero.
                                    hiss = state.swell == 0;
                                    state.swell = state.swell.saturating_add(1);
                                    state.swell_dir = 1;
                                    swelling = true;
                                    explode = state.swell >= CREEPER_SWELL_TICKS;
                                } else {
                                    state.swell = state.swell.saturating_sub(1);
                                    state.swell_dir = -1;
                                }
                                power = crate::creeper::explosion_power(state.powered);
                            }
                            if hiss {
                                events.push(MobEvent::Hiss { at: start });
                            }
                            if explode {
                                let center = Vec3::new(start.x, start.y + 0.5, start.z);
                                events.push(MobEvent::Explode { center, power });
                                let active_effects: Vec<_> = store
                                    .data(key)
                                    .map(|d| d.effects.iter().cloned().collect())
                                    .unwrap_or_default();
                                if !active_effects.is_empty() {
                                    let owner = store.data(key).map(|d| d.uuid);
                                    events.push(MobEvent::CreeperPotionCloud {
                                        center,
                                        effects: active_effects,
                                        owner,
                                    });
                                }
                                mob::kill(store, key);
                                continue;
                            }
                            if swelling {
                                // `SwellGoal.start()` stops navigation while the fuse burns.
                                walking = None;
                            } else if let Some(away) =
                                crate::creeper::flee_cat_direction(start, cats.as_slice())
                            {
                                walking = Some(yaw_towards(away.x, away.z));
                                speed = 1.2;
                            } else {
                                // `Creeper` `MeleeAttackGoal(1.0, false)`: chase the target into fuse range.
                                walking = follow_path_with(
                                    store, key, blocks, start, target.pos, &path_opts,
                                );
                                speed = chase_speed(store, key);
                            }
                            crate::creeper::sync(store, key);
                        }
                        MobKind::Skeleton | MobKind::Stray | MobKind::Bogged => {
                            directed = true;
                            let has_bow = mob::state(store, key).is_some_and(|state| {
                                bow_item_id != 0
                                    && state.equipment[mob::MAIN_HAND as usize] == bow_item_id
                            });
                            if !has_bow {
                                // `AbstractSkeleton.reassessWeaponGoal`: melee fallback without a bow.
                                tick_melee_or_chase(
                                    store,
                                    key,
                                    kind,
                                    start,
                                    target,
                                    horizontal,
                                    blocks,
                                    &path_opts,
                                    &mut events,
                                    &mut walking,
                                    &mut speed,
                                );
                            } else {
                                let eye_height = f64::from(kind.props().eye_height);
                                let has_los =
                                    has_line_of_sight(blocks, start, eye_height, target.pos);
                                if horizontal <= SKELETON_RANGE && has_los {
                                    if horizontal > 8.0 {
                                        walking = Some(yaw);
                                    } else if horizontal < 4.0 {
                                        walking = Some(wrap_degrees(yaw + 180.0));
                                    } else {
                                        // `RangedBowAttackGoal` strafes while it shoots, flipping side
                                        // every second and a half.
                                        let ticks =
                                            store.data(key).map_or(0, |data| data.tick_count);
                                        let side = if (ticks / 30).is_multiple_of(2) {
                                            90.0
                                        } else {
                                            -90.0
                                        };
                                        walking = Some(wrap_degrees(yaw + side));
                                    }
                                    let ready = mob::state(store, key)
                                        .is_some_and(|state| state.bow_timer == 0);
                                    if ready {
                                        let from =
                                            Vec3::new(start.x, start.y + eye_height, start.z);
                                        let to = Vec3::new(
                                            target.pos.x,
                                            target.pos.y + PLAYER_EYE_HEIGHT,
                                            target.pos.z,
                                        );
                                        let owner = store.data(key).map(|data| data.uuid);
                                        events.push(MobEvent::Arrow {
                                            from,
                                            direction: arrow_direction(
                                                from,
                                                to,
                                                bow_inaccuracy,
                                                rng,
                                            ),
                                            damage: SKELETON_ARROW_DAMAGE,
                                            speed: crate::arrow::SPEED,
                                            effect: ranged_skeleton_effect(kind),
                                            owner,
                                        });
                                        if let Some(state) = mob::state_mut(store, key) {
                                            // Vanilla `Bogged.getHardAttackInterval` = 50, `getAttackInterval` = 70.
                                            state.bow_timer = if kind == MobKind::Bogged {
                                                if *hard { 50 } else { 70 }
                                            } else {
                                                bow_cooldown
                                            };
                                        }
                                    }
                                } else {
                                    // Out of bow range or sight blocked: path toward the target.
                                    walking = follow_path_with(
                                        store, key, blocks, start, target.pos, &path_opts,
                                    );
                                    speed = chase_speed(store, key);
                                }
                            }
                        }
                        MobKind::Blaze => {
                            directed = true;
                            tick_blaze_goal(
                                store,
                                key,
                                kind,
                                start,
                                target,
                                blocks,
                                &path_opts,
                                rng,
                                &mut events,
                                &mut walking,
                                &mut speed,
                                &mut look,
                            );
                        }
                        MobKind::Witch => {
                            directed = true;
                            tick_witch_goal(
                                store,
                                key,
                                kind,
                                start,
                                target,
                                blocks,
                                &path_opts,
                                player_info,
                                rng,
                                &mut events,
                                &mut walking,
                                &mut speed,
                                &mut look,
                            );
                        }
                        MobKind::Piglin => {
                            directed = true;
                            if !tick_piglin_crossbow(
                                store,
                                key,
                                kind,
                                start,
                                target,
                                crossbow_item_id,
                                blocks,
                                &path_opts,
                                *hard,
                                rng,
                                &mut events,
                                &mut walking,
                                &mut speed,
                                &mut look,
                            ) {
                                tick_melee_or_chase(
                                    store,
                                    key,
                                    kind,
                                    start,
                                    target,
                                    horizontal,
                                    blocks,
                                    &path_opts,
                                    &mut events,
                                    &mut walking,
                                    &mut speed,
                                );
                            }
                        }
                        MobKind::Pillager => {
                            directed = true;
                            if !tick_piglin_crossbow(
                                store,
                                key,
                                kind,
                                start,
                                target,
                                crossbow_item_id,
                                blocks,
                                &path_opts,
                                *hard,
                                rng,
                                &mut events,
                                &mut walking,
                                &mut speed,
                                &mut look,
                            ) {
                                tick_melee_or_chase(
                                    store,
                                    key,
                                    kind,
                                    start,
                                    target,
                                    horizontal,
                                    blocks,
                                    &path_opts,
                                    &mut events,
                                    &mut walking,
                                    &mut speed,
                                );
                            }
                        }
                        MobKind::Hoglin
                        | MobKind::Zoglin
                        | MobKind::PiglinBrute
                        | MobKind::Vindicator
                        | MobKind::Ravager
                        | MobKind::Warden
                        | MobKind::Creaking
                        | MobKind::Phantom
                        | MobKind::Breeze
                        | MobKind::Evoker
                        | MobKind::Guardian
                        | MobKind::ElderGuardian
                        | MobKind::Wither
                        | MobKind::Shulker => {
                            directed = true;
                            tick_melee_or_chase(
                                store,
                                key,
                                kind,
                                start,
                                target,
                                horizontal,
                                blocks,
                                &path_opts,
                                &mut events,
                                &mut walking,
                                &mut speed,
                            );
                        }
                        MobKind::Rabbit if killer_bunny => {
                            directed = true;
                            if in_melee_reach(kind, start, target.pos, horizontal) {
                                if mob::state(store, key).is_some_and(|s| s.attack_cooldown == 0) {
                                    events.push(MobEvent::Melee {
                                        player: target.uuid,
                                        damage: crate::rabbit::KILLER_BUNNY_DAMAGE,
                                        from: start,
                                        attacker: key,
                                    });
                                    if let Some(state) = mob::state_mut(store, key) {
                                        state.attack_cooldown = MELEE_COOLDOWN;
                                    }
                                }
                            } else {
                                walking = follow_path_with(
                                    store, key, blocks, start, target.pos, &path_opts,
                                );
                                speed = 1.4;
                            }
                        }
                        MobKind::Zombie
                        | MobKind::Drowned
                        | MobKind::Husk
                        | MobKind::WitherSkeleton
                        | MobKind::Spider
                        | MobKind::CaveSpider
                        | MobKind::Enderman
                        | MobKind::ZombifiedPiglin => {
                            let has_rider = mob::state(store, key).is_some_and(|s| s.has_rider);
                            if kind.is_spider_like() && has_rider {
                                // `SpiderAttackGoal.canUse` returns false when `isVehicle` (jockey):
                                // let the spider wander normally while its rider shoots.
                                look = look_at(
                                    start,
                                    f64::from(kind.props().eye_height),
                                    player_positions.as_slice(),
                                );
                            } else {
                                directed = true;
                                if kind.is_spider_like() {
                                    // `LeapAtTargetGoal(0.4)`: pounce from two to four blocks away.
                                    let dy = target.pos.y - start.y;
                                    if crate::spider::can_leap(
                                        dx * dx + dy * dy + dz * dz,
                                        was_on_ground,
                                        has_rider,
                                    ) && crate::spider::leap_roll(rng)
                                    {
                                        let vel = store.vel(key).unwrap_or_default();
                                        store.set_vel(
                                            key,
                                            crate::spider::leap_velocity(vel, start, target.pos),
                                        );
                                    }
                                }
                                if matches!(
                                    kind,
                                    MobKind::Zombie | MobKind::Husk | MobKind::Drowned
                                ) {
                                    match crate::sun::tick_break_door(
                                        store, key, blocks, target.pos, *hard, *griefing,
                                    ) {
                                        crate::sun::DoorStep::Idle
                                        | crate::sun::DoorStep::Chewing => {}
                                        crate::sun::DoorStep::Breaking { at, progress } => {
                                            events.push(MobEvent::BreakDoor {
                                                at,
                                                progress,
                                                broken: false,
                                            });
                                        }
                                        crate::sun::DoorStep::Broken { at } => {
                                            events.push(MobEvent::BreakDoor {
                                                at,
                                                progress: 10,
                                                broken: true,
                                            });
                                        }
                                    }
                                }
                                if in_melee_reach(kind, start, target.pos, horizontal) {
                                    let ready = mob::state(store, key)
                                        .is_some_and(|state| state.attack_cooldown == 0);
                                    if ready {
                                        events.push(MobEvent::Melee {
                                            player: target.uuid,
                                            damage: kind.attack_damage(),
                                            from: start,
                                            attacker: key,
                                        });
                                        if let Some(state) = mob::state_mut(store, key) {
                                            state.attack_cooldown = MELEE_COOLDOWN;
                                        }
                                    }
                                } else {
                                    walking = follow_path_with(
                                        store, key, blocks, start, target.pos, &path_opts,
                                    );
                                    // `WallClimberNavigation`: when the grid A* finds no path, head
                                    // toward `pathToPosition` until horizontally within `bbWidth` (or
                                    // above the target within `bbWidth`).
                                    if kind.is_spider_like() && walking.is_none() {
                                        let bb_width = f64::from(kind.props().width);
                                        let reached = start.distance_to_sqr(target.pos)
                                            < bb_width * bb_width
                                            || (start.y > target.pos.y && horizontal < bb_width);
                                        if !reached {
                                            walking = Some(yaw);
                                        }
                                    }
                                    // `EndermanFreezeWhenLookedAt`: stop chasing while stared at by
                                    // the targeted player within 16 blocks.
                                    if kind == MobKind::Enderman {
                                        let eye = Vec3::new(
                                            start.x,
                                            start.y + f64::from(kind.props().eye_height),
                                            start.z,
                                        );
                                        let stared = mob::state(store, key)
                                            .is_some_and(|s| s.stare_time > 0)
                                            || (start.distance_to_sqr(target.pos) <= 256.0
                                                && gazes.get(&target.uuid).is_some_and(|gaze| {
                                                    crate::enderman::is_stared_at_by(
                                                        blocks, eye, target.pos, *gaze,
                                                    )
                                                }));
                                        if stared {
                                            walking = None;
                                        }
                                    }
                                    speed = chase_speed(store, key);
                                }
                                // Enderman basics: blink to a nearby walkable spot now and then.
                                if kind == MobKind::Enderman
                                    && rng.next_int(400) == 0
                                    && let Some((from, to)) = blink(store, key, blocks, start, rng)
                                {
                                    events.push(MobEvent::Teleport { from, to });
                                }
                            }
                        }
                        _ => {}
                    }
                } else if kind == MobKind::Creeper {
                    let mut explode = false;
                    let mut hiss = false;
                    let mut power = CREEPER_POWER;
                    if let Some(state) = mob::state_mut(store, key) {
                        if state.ignited {
                            hiss = state.swell == 0;
                            state.swell = state.swell.saturating_add(1);
                            state.swell_dir = 1;
                            explode = state.swell >= CREEPER_SWELL_TICKS;
                        } else {
                            // No target: `Creeper.tick` keeps adding the swell direction (-1), so a
                            // fuse that lost its target winds down instead of staying lit.
                            state.swell_dir = -1;
                            state.swell = state.swell.saturating_sub(1);
                        }
                        power = crate::creeper::explosion_power(state.powered);
                    }
                    if hiss {
                        events.push(MobEvent::Hiss { at: start });
                    }
                    if explode {
                        let center = Vec3::new(start.x, start.y + 0.5, start.z);
                        events.push(MobEvent::Explode { center, power });
                        let active_effects: Vec<_> = store
                            .data(key)
                            .map(|d| d.effects.iter().cloned().collect())
                            .unwrap_or_default();
                        if !active_effects.is_empty() {
                            let owner = store.data(key).map(|d| d.uuid);
                            events.push(MobEvent::CreeperPotionCloud {
                                center,
                                effects: active_effects,
                                owner,
                            });
                        }
                        mob::kill(store, key);
                        continue;
                    }
                    if let Some(away) = crate::creeper::flee_cat_direction(start, cats.as_slice()) {
                        directed = true;
                        walking = Some(yaw_towards(away.x, away.z));
                        speed = 1.2;
                    }
                    crate::creeper::sync(store, key);
                    look = look_at(
                        start,
                        f64::from(kind.props().eye_height),
                        player_positions.as_slice(),
                    );
                } else if kind == MobKind::Blaze {
                    reset_blaze_goal(store, key);
                    look = look_at(
                        start,
                        f64::from(kind.props().eye_height),
                        player_positions.as_slice(),
                    );
                } else if kind == MobKind::Witch {
                    if let Some(state) = mob::state_mut(store, key) {
                        state.see_time = 0;
                    }
                    look = look_at(
                        start,
                        f64::from(kind.props().eye_height),
                        player_positions.as_slice(),
                    );
                } else if kind == MobKind::Piglin {
                    reset_piglin_charge(store, key);
                    if crate::piglin::is_admiring(store, key) {
                        directed = true;
                    }
                    look = look_at(
                        start,
                        f64::from(kind.props().eye_height),
                        player_positions.as_slice(),
                    );
                } else if kind == MobKind::Enderman {
                    // `EndermanTakeBlockGoal` and `EndermanLeaveBlockGoal`: the region applies the
                    // actual block change after its `mobGriefing`, support and entity checks.
                    if rng.next_int(crate::enderman::TAKE_INTERVAL) == 0
                        && let Some(at) =
                            crate::enderman::take_block(store, key, blocks, *griefing, rng)
                    {
                        events.push(MobEvent::TakeBlock { key, at });
                    }
                    if rng.next_int(crate::enderman::LEAVE_INTERVAL) == 0
                        && let Some((at, state)) =
                            crate::enderman::leave_block(store, key, blocks, *griefing, rng)
                    {
                        events.push(MobEvent::PlaceCarried { key, at, state });
                    }
                    let can_see_sky =
                        crate::sun::sees_sky(blocks, start_bx, start_by + 1, start_bz);
                    let wet = crate::enderman::in_water_or_rain(store, key, blocks, *raining);
                    let block_bright = blocks
                        .brightness(start_bx, start_by + 1, start_bz)
                        .is_some_and(|l| crate::sun::light_magic_value(l) > 0.5);
                    if crate::enderman::daylight_teleport_roll(
                        can_see_sky,
                        bright_now,
                        wet,
                        false,
                        rng,
                    ) && let Some(to) = crate::enderman::teleport_random(store, key, blocks, rng)
                    {
                        events.push(MobEvent::Teleport { from: start, to });
                    } else if bright_outside.unwrap_or(true)
                        && (can_see_sky || block_bright)
                        && rng.next_int(200) == 0
                        && let Some((from, to)) = blink(store, key, blocks, start, rng)
                    {
                        events.push(MobEvent::Teleport { from, to });
                    }
                    look = look_at(
                        start,
                        f64::from(kind.props().eye_height),
                        player_positions.as_slice(),
                    );
                } else if kind.is_bow_skeleton()
                    && let Some(shade) = crate::sun::tick_flee_sun_restrict(
                        store, key, blocks, start, bright_now, rng,
                    )
                {
                    // `FleeSunGoal`: run for the shade at normal speed.
                    directed = true;
                    walking = follow_path_with(
                        store,
                        key,
                        blocks,
                        start,
                        Vec3::new(
                            f64::from(shade[0]) + 0.5,
                            f64::from(shade[1]),
                            f64::from(shade[2]) + 0.5,
                        ),
                        &path_opts,
                    );
                    look = look_at(
                        start,
                        f64::from(kind.props().eye_height),
                        player_positions.as_slice(),
                    );
                } else {
                    look = look_at(
                        start,
                        f64::from(kind.props().eye_height),
                        player_positions.as_slice(),
                    );
                }
            }
        } else {
            // Passive goals: flee when hurt/burning, lead/caravan/tame, breed, tempt, follow
            // parent, eat grass (sheep), wander; look at a nearby player.
            let (panic, panic_from, fire, moving, wander_yaw) =
                mob::state(store, key).map_or((0, None, 0, false, 0.0), |state| {
                    (
                        state.panic,
                        state.panic_from,
                        state.fire,
                        state.moving,
                        state.wander_yaw,
                    )
                });
            let angry_at = if matches!(
                kind,
                MobKind::Wolf
                    | MobKind::Llama
                    | MobKind::TraderLlama
                    | MobKind::IronGolem
                    | MobKind::Bee
            ) {
                mob::state(store, key)
                    .filter(|state| !(kind == MobKind::Bee && state.has_stung))
                    // `IronGolem.canAttack` rejects players for player-created golems.
                    .filter(|state| kind != MobKind::IronGolem || !state.player_created)
                    .and_then(|state| state.target)
                    .and_then(|uuid| players.iter().find(|target| target.uuid == uuid))
                    .copied()
            } else {
                None
            };
            let defend = if kind == MobKind::Wolf && angry_at.is_none() {
                crate::tame::defend_target_with_owner_attack(
                    store,
                    key,
                    owners.as_slice(),
                    owner_attacks,
                    start,
                )
            } else {
                None
            };
            if let Some((victim, victim_pos)) = defend {
                // A tamed wolf bites the monster hunting its owner.
                directed = true;
                let dx = victim_pos.x - start.x;
                let dz = victim_pos.z - start.z;
                let horizontal = (dx * dx + dz * dz).sqrt();
                if in_melee_reach(kind, start, victim_pos, horizontal) {
                    let ready =
                        mob::state(store, key).is_some_and(|state| state.attack_cooldown == 0);
                    if ready {
                        mob::hurt(store, victim, crate::tame::TAMED_BITE, Some(start));
                        if let Some(state) = mob::state_mut(store, key) {
                            state.attack_cooldown = MELEE_COOLDOWN;
                        }
                    }
                } else {
                    walking = follow_path_with(store, key, blocks, start, victim_pos, &path_opts);
                    speed = CHASE_ACCELERATION;
                }
            } else if let Some(target) = angry_at {
                // A wild wolf chases and bites the player who hurt it.
                directed = true;
                let dx = target.pos.x - start.x;
                let dz = target.pos.z - start.z;
                let horizontal = (dx * dx + dz * dz).sqrt();
                if kind.is_llama() {
                    // `LlamaSpit`: a wild llama spits at whoever hurt it from up to 10 blocks.
                    if horizontal <= 10.0
                        && mob::state(store, key).is_some_and(|state| state.bow_timer == 0)
                    {
                        let from = Vec3::new(
                            start.x,
                            start.y + f64::from(kind.props().eye_height),
                            start.z,
                        );
                        let to =
                            Vec3::new(target.pos.x, target.pos.y + PLAYER_EYE_HEIGHT, target.pos.z);
                        events.push(MobEvent::Spit {
                            from,
                            direction: (to - from).normalize(),
                        });
                        if let Some(state) = mob::state_mut(store, key) {
                            state.bow_timer = SKELETON_COOLDOWN;
                        }
                    }
                } else if in_melee_reach(kind, start, target.pos, horizontal) {
                    let ready =
                        mob::state(store, key).is_some_and(|state| state.attack_cooldown == 0);
                    if ready {
                        match kind {
                            MobKind::IronGolem => {
                                crate::golem::iron_golem_hit_player(
                                    store,
                                    key,
                                    target,
                                    start,
                                    rng,
                                    &mut events,
                                );
                            }
                            MobKind::Bee => {
                                crate::bee::sting_player(
                                    store,
                                    key,
                                    target,
                                    start,
                                    *hard,
                                    &mut events,
                                );
                            }
                            _ => {
                                events.push(MobEvent::Melee {
                                    player: target.uuid,
                                    damage: kind.attack_damage(),
                                    from: start,
                                    attacker: key,
                                });
                                if let Some(state) = mob::state_mut(store, key) {
                                    state.attack_cooldown = MELEE_COOLDOWN;
                                }
                            }
                        }
                    }
                } else {
                    walking = follow_path_with(store, key, blocks, start, target.pos, &path_opts)
                        .or_else(|| (kind == MobKind::Bee).then(|| yaw_towards(dx, dz)));
                    speed = CHASE_ACCELERATION;
                }
            } else if kind == MobKind::IronGolem
                && let Some((victim, victim_pos)) =
                    crate::golem::nearest_hostile_mob(store, key, start, kind.follow_range(), true)
            {
                directed = true;
                let dx = victim_pos.x - start.x;
                let dz = victim_pos.z - start.z;
                let horizontal = (dx * dx + dz * dz).sqrt();
                look = Some((yaw_towards(dx, dz), 0.0));
                if in_melee_reach(kind, start, victim_pos, horizontal) {
                    if mob::state(store, key).is_some_and(|s| s.attack_cooldown == 0) {
                        crate::golem::iron_golem_hit_mob(
                            store,
                            key,
                            victim,
                            start,
                            rng,
                            &mut events,
                        );
                    }
                } else {
                    walking = follow_path_with(store, key, blocks, start, victim_pos, &path_opts);
                    speed = CHASE_ACCELERATION;
                }
            } else if kind == MobKind::SnowGolem
                && let Some((move_to, aim_pos)) =
                    crate::golem::tick_snow_golem(store, key, blocks, start, rng, &mut events)
            {
                directed = true;
                let dx = aim_pos.x - start.x;
                let dz = aim_pos.z - start.z;
                look = Some((yaw_towards(dx, dz), 0.0));
                if let Some(target_pos) = move_to {
                    walking = follow_path_with(store, key, blocks, start, target_pos, &path_opts);
                    speed = crate::golem::SNOW_GOLEM_SPEED;
                }
            } else if kind == MobKind::Bee
                && let Some(state) = mob::state(store, key)
                && let Some(hive) = state.hive_pos
                && (!*monsters_burn
                    || *raining
                    || state.has_nectar
                    || u32::from(state.ticks_without_nectar)
                        >= crate::bee::TICKS_WITHOUT_NECTAR_BEFORE_GOING_HOME)
            {
                // `Bee.BeeGoToHiveGoal` / `wantsToEnterHive`: at night, in rain, carrying
                // nectar, or after an hour without nectar, a bee with a valid home flies back
                // to it. The region stores the bee in the hive block entity when it gets close
                // enough; this branch only steers it there.
                let target = Vec3::new(
                    f64::from(hive[0]) + 0.5,
                    f64::from(hive[1]) + 0.5,
                    f64::from(hive[2]) + 0.5,
                );
                let dx = target.x - start.x;
                let dz = target.z - start.z;
                let distance = (dx * dx + dz * dz).sqrt();
                if distance > crate::bee::HIVE_CLOSE_ENOUGH_DISTANCE
                    && distance <= crate::bee::TOO_FAR_DISTANCE
                {
                    directed = true;
                    look = Some((yaw_towards(dx, dz), 0.0));
                    walking = follow_path_with(store, key, blocks, start, target, &path_opts)
                        .or_else(|| Some(yaw_towards(dx, dz)));
                    speed = 1.0;
                }
            } else if kind == MobKind::Bee
                && let Some(state) = mob::state(store, key)
                && !state.has_nectar
                && !*monsters_burn
                && !*raining
                && let Some(flower) = state.flower_pos
            {
                // `Bee.PollinateGoal`: fly to the remembered flower and hold there for the
                // 20-tick pollinating phase. The region sets `has_nectar` when it completes.
                let target = Vec3::new(
                    f64::from(flower[0]) + 0.5,
                    f64::from(flower[1]) + 0.5,
                    f64::from(flower[2]) + 0.5,
                );
                let dx = target.x - start.x;
                let dz = target.z - start.z;
                let distance = (dx * dx + dz * dz).sqrt();
                if distance > 1.5 {
                    directed = true;
                    look = Some((yaw_towards(dx, dz), 0.0));
                    walking = follow_path_with(store, key, blocks, start, target, &path_opts)
                        .or_else(|| Some(yaw_towards(dx, dz)));
                    speed = 1.0;
                } else {
                    directed = true;
                }
            } else if panic > 0 || (kind.is_animal() && fire > 0) {
                directed = true;
                // `TurtlePanicGoal`: a panicking turtle runs for nearby water before it runs
                // blindly away from the damage source.
                if kind == MobKind::Turtle
                    && let Some(target) = crate::turtle::find_water(
                        blocks,
                        start,
                        crate::turtle::PANIC_WATER_RADIUS,
                        rng,
                    )
                {
                    let target = crate::turtle::block_center(target);
                    walking = follow_path_with(store, key, blocks, start, target, &path_opts)
                        .or_else(|| Some(yaw_towards(target.x - start.x, target.z - start.z)));
                } else if let Some(from) = panic_from {
                    walking = Some(yaw_towards(start.x - from.x, start.z - from.z));
                } else {
                    walking = Some(wander_yaw);
                }
                speed = panic_speed(kind);
            } else if kind == MobKind::Turtle {
                match crate::turtle::tick_turtle(store, key, blocks, start, rng, &mut events) {
                    crate::turtle::TurtleStep::Idle => {
                        if moving {
                            walking = Some(wander_yaw);
                        }
                    }
                    crate::turtle::TurtleStep::Walking {
                        target,
                        speed: turtle_speed,
                    } => {
                        directed = true;
                        walking = follow_path_with(store, key, blocks, start, target, &path_opts)
                            .or_else(|| Some(yaw_towards(target.x - start.x, target.z - start.z)));
                        speed = turtle_speed;
                    }
                    crate::turtle::TurtleStep::Laying => {
                        directed = true;
                        walking = None;
                    }
                }
            } else if let Some(holder) =
                leash_pull(store, key, players, holders, start, &mut events)
            {
                // A lead pulls its animal along once it is more than six blocks from the holder.
                directed = true;
                walking = Some(yaw_towards(holder.x - start.x, holder.z - start.z));
            } else if let Some(action) = caravan_action(store, key, start) {
                directed = true;
                if let CaravanAction::Walk {
                    target,
                    speed: multiplier,
                } = action
                {
                    walking = Some(yaw_towards(target.x - start.x, target.z - start.z));
                    speed = multiplier;
                }
            } else if kind.is_tameable()
                && let Some(owner_pos) =
                    crate::tame::follow_target(store, key, owners.as_slice(), start)
            {
                directed = true;
                walking = Some(yaw_towards(owner_pos.x - start.x, owner_pos.z - start.z));
            } else if kind.is_tameable() && mob::state(store, key).is_some_and(|s| s.sitting) {
                directed = true;
            } else if let Some((mate, mate_pos)) = crate::breed::mate_for(store, key) {
                directed = true;
                walking = follow_path_with(store, key, blocks, start, mate_pos, &path_opts)
                    .or_else(|| Some(yaw_towards(mate_pos.x - start.x, mate_pos.z - start.z)));
                if let Some(bred) = crate::breed::approach(store, key, mate, mate_pos, rng) {
                    events.push(MobEvent::Bred(bred));
                }
            } else if let Some(tempt) = tempt_target(kind, start, players, held_items) {
                directed = true;
                let dx = tempt.pos.x - start.x;
                let dz = tempt.pos.z - start.z;
                let horiz = (dx * dx + dz * dz).sqrt();
                look = look_at_point(start, f64::from(kind.props().eye_height), tempt.pos, horiz);
                if start.distance_to_sqr(tempt.pos) >= TEMPT_STOP_DISTANCE_SQR {
                    walking = follow_path_with(store, key, blocks, start, tempt.pos, &path_opts)
                        .or_else(|| Some(yaw_towards(dx, dz)));
                    speed = tempt_speed(kind);
                }
            } else if let Some(parent) = follow_parent(store, key, start) {
                directed = true;
                walking = follow_path_with(store, key, blocks, start, parent, &path_opts)
                    .or_else(|| Some(yaw_towards(parent.x - start.x, parent.z - start.z)));
                speed = follow_parent_speed(kind);
            } else if kind == MobKind::Villager
                && let Some((activity, destination)) = store.data(key).and_then(|data| {
                    let villager = data.villager.as_ref()?;
                    Some((villager.activity, villager.destination?))
                })
                && activity != crate::villager::Activity::Idle
            {
                directed = true;
                let target = Vec3::new(
                    f64::from(destination[0]) + 0.5,
                    f64::from(destination[1]),
                    f64::from(destination[2]) + 0.5,
                );
                let dx = target.x - start.x;
                let dz = target.z - start.z;
                if dx * dx + dz * dz > 4.0 {
                    walking = follow_path_with(store, key, blocks, start, target, &path_opts)
                        .or_else(|| Some(yaw_towards(dx, dz)));
                    speed = VILLAGER_SPEED;
                }
            } else if kind == MobKind::Goat {
                match crate::goat::tick_goat(
                    store,
                    key,
                    blocks,
                    players,
                    was_on_ground,
                    rng,
                    &mut events,
                ) {
                    crate::goat::GoatStep::Idle => {
                        if moving {
                            walking = Some(wander_yaw);
                        }
                    }
                    crate::goat::GoatStep::Preparing { target } => {
                        directed = true;
                        look = Some((yaw_towards(target.x - start.x, target.z - start.z), 0.0));
                    }
                    crate::goat::GoatStep::Charging { target } => {
                        directed = true;
                        let yaw = yaw_towards(target.x - start.x, target.z - start.z);
                        look = Some((yaw, 0.0));
                        walking = Some(yaw);
                        speed = crate::goat::RAM_SPEED_MULT;
                    }
                    crate::goat::GoatStep::LongJumped => {
                        directed = true;
                    }
                }
            } else if kind == MobKind::Fox {
                match crate::fox::tick_fox_with_sneaking(
                    store,
                    key,
                    blocks,
                    players,
                    |u| player_info.get(&u).is_some_and(|s| s.sneaking),
                    start,
                    was_on_ground,
                ) {
                    crate::fox::FoxStep::Idle => {
                        if let Some(bush) = (*griefing && rng.next_int(60) == 0)
                            .then(|| crate::fox::find_sweet_berry_bush(blocks, start))
                            .flatten()
                        {
                            events.push(MobEvent::HarvestSweetBerries {
                                at: bush,
                                pos: start,
                            });
                        } else if moving {
                            walking = Some(wander_yaw);
                        }
                        if was_on_ground
                            && crate::fox::in_snow_at(blocks, start)
                            && mob::state(store, key).is_some_and(|s| s.fox_crouching)
                            && let Some(state) = mob::state_mut(store, key)
                        {
                            state.fox_crouching = false;
                            sync_mob_metadata_now(store, key);
                        }
                    }
                    crate::fox::FoxStep::Flee { away } => {
                        directed = true;
                        walking = Some(yaw_towards(away.x, away.z));
                        speed = crate::fox::FOX_FLEE_SPEED;
                    }
                    crate::fox::FoxStep::Stalk { target } => {
                        directed = true;
                        let yaw = yaw_towards(target.x - start.x, target.z - start.z);
                        look = Some((yaw, 0.0));
                        walking = follow_path_with(store, key, blocks, start, target, &path_opts)
                            .or(Some(yaw));
                        speed = 1.25;
                    }
                    crate::fox::FoxStep::Crouching { target } => {
                        directed = true;
                        look = Some((yaw_towards(target.x - start.x, target.z - start.z), 0.0));
                        sync_mob_metadata_now(store, key);
                    }
                    crate::fox::FoxStep::Pounced { target } => {
                        directed = true;
                        look = Some((yaw_towards(target.x - start.x, target.z - start.z), 0.0));
                    }
                }
            } else if kind == MobKind::Frog {
                if let Some((prey_key, prey_pos, prey_kind)) = nearest_frog_prey(store, start, 10.0)
                {
                    let yaw = yaw_towards(prey_pos.x - start.x, prey_pos.z - start.z);
                    match crate::frog::tick_tongue_against(store, key, prey_key, &mut events) {
                        crate::frog::TongueStep::Approach => {
                            directed = true;
                            walking = Some(yaw);
                            speed = 1.25;
                            look = Some((yaw, 0.0));
                        }
                        crate::frog::TongueStep::Extending | crate::frog::TongueStep::Catching => {
                            directed = true;
                            look = Some((yaw, 0.0));
                        }
                        crate::frog::TongueStep::Eaten => {
                            directed = true;
                            let frog_variant = mob::state(store, key).and_then(|s| s.variant);
                            events.push(MobEvent::FrogEat {
                                prey: prey_key,
                                prey_kind,
                                frog_variant,
                                at: prey_pos,
                            });
                        }
                        crate::frog::TongueStep::Idle => {}
                    }
                } else if crate::frog::tick_frog_long_jump(
                    store,
                    key,
                    blocks,
                    start,
                    was_on_ground,
                    rng,
                    &mut events,
                ) {
                    directed = true;
                }
            } else if kind == MobKind::Rabbit
                && let Some(away) = crate::rabbit::flee_player_direction(start, players)
            {
                directed = true;
                walking = Some(yaw_towards(away.x, away.z));
                speed = crate::rabbit::FLEE_SPEED_MOD;
            } else if kind == MobKind::Rabbit
                && *griefing
                && rng.next_int(40) == 0
                && let Some((crop_at, new_age)) = crate::rabbit::find_carrot_crop(blocks, start)
            {
                events.push(MobEvent::RabbitNibbleCarrots {
                    at: crop_at,
                    new_age,
                });
            } else if moving {
                walking = Some(wander_yaw);
            }
            if kind == MobKind::Sheep {
                if directed {
                    crate::sheep::cancel_eat(store, key);
                } else {
                    eating = crate::sheep::tick_eat(store, key, blocks, rng, &mut events);
                }
            }
            if look.is_none() {
                let look_range = if kind.is_animal() {
                    ANIMAL_LOOK_DISTANCE
                } else {
                    LOOK_DISTANCE
                };
                look = look_at_within(
                    start,
                    f64::from(kind.props().eye_height),
                    player_positions.as_slice(),
                    look_range,
                );
            }
        }

        // `WaterAvoidingRandomStrollGoal` picks a new interval now and then when idle (both
        // passive animals and idle hostile monsters).
        if !directed && panic_of(store, key) == 0 {
            let wanting = mob::state(store, key).is_some_and(|state| state.moving);
            let timer = mob::state(store, key).map_or(0, |state| state.wander_timer);
            if timer == 0 {
                let (moving, yaw, length) = if wanting {
                    (false, 0.0, 20 + rng.next_int(40) as u32)
                } else if rng.next_int(60) == 0 {
                    let yaw = pick_dry_wander_yaw(blocks, start, in_water, rng);
                    (true, yaw, 40 + rng.next_int(80) as u32)
                } else {
                    (false, 0.0, 0)
                };
                if let Some(state) = mob::state_mut(store, key) {
                    state.moving = moving;
                    state.wander_yaw = yaw;
                    state.wander_timer = length;
                }
            } else if let Some(state) = mob::state_mut(store, key) {
                state.wander_timer = timer - 1;
            }
            if wanting {
                let yaw = mob::state(store, key).map_or(0.0, |state| state.wander_yaw);
                if !in_water && ahead_is_fluid(blocks, start, yaw) {
                    walking = None;
                    if let Some(state) = mob::state_mut(store, key) {
                        state.moving = false;
                        state.wander_timer = 0;
                    }
                } else {
                    walking = Some(yaw);
                    speed = wander_speed(kind);
                }
            }
        }

        // `EatBlockGoal` holds MOVE, LOOK and JUMP: an eating sheep stands and looks down.
        if eating {
            walking = None;
            look = None;
        }
        // Villagers stay within sight of home and refuse to walk into water or lava. A
        // scheduled destination (workstation, bed or bell) is allowed outside the home radius.
        if kind == MobKind::Villager {
            let home = store
                .data(key)
                .and_then(|data| data.villager.as_ref().map(|villager| villager.home));
            let directed_activity = store.data(key).is_some_and(|data| {
                data.villager.as_ref().is_some_and(|villager| {
                    villager.activity != crate::villager::Activity::Idle
                        && villager.destination.is_some()
                })
            });
            if let Some(home) = home {
                let dx = home[0] - start.x;
                let dz = home[2] - start.z;
                if !directed_activity && dx * dx + dz * dz > crate::villager::WANDER_RADIUS.powi(2)
                {
                    walking = Some(yaw_towards(dx, dz));
                } else if let Some(yaw) = walking
                    && crate::villager::ahead_is_liquid(blocks, start, yaw)
                {
                    walking = None;
                    if let Some(state) = mob::state_mut(store, key) {
                        state.moving = false;
                        state.wander_timer = 0;
                    }
                }
            }
        }

        // Look at player: wins over the walking direction for body, head and pitch.
        if let Some((yaw, pitch)) = look
            && let Some(data) = store.data_mut(key)
        {
            data.yaw = yaw;
            data.head_yaw = yaw;
            data.pitch = pitch;
        }

        let no_ai = mob::state(store, key).is_some_and(|state| state.no_ai);
        if no_ai {
            walking = None;
            look = None;
            blaze_hover_up = false;
        }
        if kind == MobKind::Witch && mob::state(store, key).is_some_and(|s| s.drink_timer > 0) {
            // `Witch.SPEED_MODIFIER_DRINKING` (`-0.25` ADD_VALUE) offsets its `0.25` base speed.
            walking = None;
        }
        if !no_ai && matches!(kind, MobKind::Villager | MobKind::Piglin) {
            tick_mob_doors(store, key, kind, blocks, start, &mut events);
        }
        let mut vel = store.vel(key).unwrap_or_default();
        if kind == MobKind::Rabbit && !no_ai {
            let urgent = panic_of(store, key) > 0 || speed > 1.5;
            walking =
                crate::rabbit::apply_hop(store, key, walking, was_on_ground, urgent, &mut vel);
        }
        // Vanilla `Slime.SlimeKeepOnJumpingGoal`: slimes and magma cubes hop periodically instead
        // of sliding flat along the ground.
        if matches!(kind, MobKind::Slime | MobKind::MagmaCube) && !no_ai && walking.is_some() {
            let hop_now = was_on_ground && rng.next_int(14) == 0;
            if hop_now {
                let cube_size = mob::state(store, key).map_or(1, |s| s.cube_size);
                let jump = if kind == MobKind::MagmaCube {
                    0.42 + f64::from(cube_size) * 0.1
                } else {
                    0.42
                };
                vel.y = vel.y.max(jump);
            } else if was_on_ground {
                // Rest briefly on the ground between hops.
                walking = None;
            }
        }
        if kind == MobKind::Blaze {
            // `Blaze.aiStep` dampens downward velocity before `customServerAiStep` applies hover.
            if !was_on_ground && vel.y < 0.0 {
                vel.y *= BLAZE_FALL_DRAG;
            }
            if blaze_hover_up {
                vel.y += (BLAZE_HOVER_ACCEL - vel.y) * BLAZE_HOVER_ACCEL;
            }
        }
        if let Some(yaw) = walking {
            let acceleration = kind.movement_speed() * WALK_ACCELERATION * speed;
            let (sin, cos) = f64::from(yaw).to_radians().sin_cos();
            vel.x -= sin * acceleration;
            vel.z += cos * acceleration;
            if look.is_none()
                && let Some(data) = store.data_mut(key)
            {
                data.yaw = yaw;
                data.head_yaw = yaw;
            }
            // Jump over a block walked into last tick, or swim up onto a bank from water.
            if (was_on_ground || (in_water && has_float_goal(kind)))
                && flags.has(Flags::HORIZONTAL_COLLISION)
            {
                vel.y = vel.y.max(kind.jump_strength());
            }
        }
        // `FloatGoal`: mobs that float paddle upward at 80% of ticks when submerged deeper than
        // their fluid jump threshold (`0.4`, or `0.0` for baby chickens <= 0.4 tall).
        if in_water && !no_ai && has_float_goal(kind) {
            let jump_threshold = if f64::from(kind.props().height) <= 0.4 {
                0.0
            } else {
                0.4
            };
            if fluid_depth > jump_threshold && rng.next_f32() < 0.8 {
                vel.y += 0.04;
            }
        }
        if kind == MobKind::Bee && !no_ai {
            if !was_on_ground {
                vel.y = (vel.y - GRAVITY * 0.1).max(-0.05);
            }
        } else {
            // Always apply gravity before `move_entity_with_step` so `delta.y < 0` tests ground
            // support every tick; otherwise a mob walking off a ledge would keep `ON_GROUND`.
            vel.y -= if in_water { GRAVITY * 0.25 } else { GRAVITY };
        }
        // `Spider` climbs a wall it is pushed against (the collision of the last move).
        if kind.is_spider_like() && flags.has(Flags::HORIZONTAL_COLLISION) {
            vel.y = crate::spider::climb_velocity(vel.y, walking.is_some());
        }
        store.set_vel(key, vel);
        physics::move_entity_with_step(store, key, vel, blocks, MOB_STEP_HEIGHT);
        if kind.is_spider_like() {
            let climbing = store
                .flags(key)
                .is_some_and(|flags| flags.has(Flags::HORIZONTAL_COLLISION));
            crate::spider::set_climbing(store, key, climbing);
        }

        // Vertical drag and the friction of the block the mob now stands on (or water drag).
        let on_ground = store
            .flags(key)
            .is_some_and(|flags| flags.has(Flags::ON_GROUND));
        let mut vel = store.vel(key).unwrap_or_default();
        if in_water {
            vel.x *= 0.8;
            vel.z *= 0.8;
            if !on_ground {
                vel.y *= 0.8;
            }
        } else {
            let friction = if on_ground { GROUND_FRICTION } else { 1.0 };
            vel.x *= AIR_FRICTION * friction;
            vel.z *= AIR_FRICTION * friction;
            if !on_ground {
                vel.y *= AIR_DRAG;
            }
        }
        store.set_vel(key, vel);

        // Fall distance grows while airborne and hurts on the landing tick.
        let end = store.pos(key).unwrap_or_default();
        let fallen = (start.y - end.y).max(0.0) as f32;
        let Some(state) = mob::state_mut(store, key) else {
            continue;
        };
        if on_ground {
            let distance = state.fall_distance + fallen;
            state.fall_distance = 0.0;
            if distance > 0.0 {
                mob::apply_fall_damage(store, key, distance);
            }
        } else {
            state.fall_distance += fallen;
        }
        if mob::is_mob(store, key) {
            mob::check_despawn(store, key, player_positions.as_slice(), *peaceful, rng);
        }
    }
    events
}

/// Walk speed multiplier for a villager moving to a scheduled destination.
const VILLAGER_SPEED: f64 = 0.8;
/// How often a zombie looks for a village bell (`MoveThroughVillageGoal`).
const VILLAGE_SCAN_TICKS: u16 = 200;

fn panic_of(store: &EntityStore, key: crate::EntityKey) -> u16 {
    mob::state(store, key).map_or(0, |state| state.panic)
}

fn has_float_goal(kind: MobKind) -> bool {
    !(kind.is_zombie_like() || kind.is_skeleton_like() || kind == MobKind::IronGolem)
}

fn panic_speed(kind: MobKind) -> f64 {
    match kind {
        MobKind::Chicken => 1.4,
        MobKind::Cow | MobKind::Mooshroom => 2.0,
        MobKind::Pig | MobKind::Sheep => 1.25,
        _ => 1.5,
    }
}

fn tempt_speed(kind: MobKind) -> f64 {
    match kind {
        MobKind::Chicken => 1.0,
        MobKind::Cow | MobKind::Mooshroom => 1.25,
        MobKind::Pig => 1.2,
        MobKind::Sheep => 1.1,
        _ => 1.0,
    }
}

fn follow_parent_speed(kind: MobKind) -> f64 {
    match kind {
        MobKind::Cow | MobKind::Mooshroom => 1.25,
        MobKind::Chicken | MobKind::Pig | MobKind::Sheep => 1.1,
        _ => 1.0,
    }
}

fn wander_speed(kind: MobKind) -> f64 {
    match kind {
        MobKind::Spider | MobKind::CaveSpider | MobKind::Creeper => 0.8,
        _ => 1.0,
    }
}

fn mob_path_range(state: &mob::MobState) -> i32 {
    let base = state.kind.follow_range().ceil() as i32;
    match state.kind {
        // Vanilla `Villager`: `getNavigation().setRequiredPathLength(48.0F)`.
        MobKind::Villager => base.max(48),
        _ => base.max(16),
    }
}

fn tick_mob_doors(
    store: &mut EntityStore,
    key: crate::EntityKey,
    kind: MobKind,
    blocks: &impl BlockGetter,
    start: Vec3,
    events: &mut Vec<MobEvent>,
) {
    let (path, index, len, opened_door) =
        mob::state(store, key).map_or(([[0; 3]; path::MAX_PATH], 0, 0, None), |state| {
            (
                state.path,
                usize::from(state.path_index),
                usize::from(state.path_len),
                state.opened_door,
            )
        });
    let door_to_open = path::door_to_open_on_path(blocks, start, &path, index, len);
    let mut next_opened = opened_door;
    if let Some(prev_door) = opened_door
        && Some(prev_door) != door_to_open
    {
        // Only inspects local mobs in this region's `EntityStore` (never cross-region state).
        let others: Vec<(Vec3, &[[i32; 3]])> = store
            .keys()
            .iter()
            .copied()
            .filter(|&other_key| other_key != key)
            .filter_map(|other_key| {
                let other = mob::state(store, other_key)?;
                if other.kind != kind || other.health <= 0.0 {
                    return None;
                }
                let other_pos = store.pos(other_key)?;
                let other_index = usize::from(other.path_index);
                let other_len = usize::from(other.path_len).min(other.path.len());
                let slice = other.path.get(other_index..other_len).unwrap_or(&[]);
                Some((other_pos, slice))
            })
            .collect();
        match path::check_door_to_close(blocks, start, &path, index, len, prev_door, others) {
            path::DoorCloseCheck::Close => {
                events.push(MobEvent::SetDoor {
                    at: prev_door,
                    open: false,
                });
                next_opened = None;
            }
            path::DoorCloseCheck::Cleared => {
                next_opened = None;
            }
            path::DoorCloseCheck::KeepOpen => {}
        }
    }
    if let Some(door) = door_to_open {
        events.push(MobEvent::SetDoor {
            at: door,
            open: true,
        });
        next_opened = Some(door);
    }
    if next_opened != opened_door
        && let Some(state) = mob::state_mut(store, key)
    {
        state.opened_door = next_opened;
    }
}

fn mob_path_options(
    store: &EntityStore,
    key: crate::EntityKey,
    hard: bool,
    griefing: bool,
    bright_now: bool,
) -> path::PathOptions {
    let Some(state) = mob::state(store, key) else {
        return path::PathOptions::default();
    };
    let can_float = has_float_goal(state.kind);
    let water_cost = match state.kind {
        MobKind::Enderman | MobKind::Blaze => None,
        MobKind::Chicken | MobKind::Drowned | MobKind::Frog | MobKind::Turtle => Some(0),
        _ => Some(path::DEFAULT_WATER_COST),
    };
    let break_doors = matches!(
        state.kind,
        MobKind::Zombie | MobKind::Husk | MobKind::Drowned
    ) && state.can_break_doors
        && hard
        && griefing;
    let open_doors = matches!(state.kind, MobKind::Villager | MobKind::Piglin);
    let can_swim = matches!(
        state.kind,
        MobKind::Drowned
            | MobKind::Turtle
            | MobKind::Frog
            | MobKind::Guardian
            | MobKind::ElderGuardian
            | MobKind::Axolotl
    );
    path::PathOptions {
        can_climb: state.kind.is_spider_like(),
        can_open_doors: break_doors || open_doors,
        water_cost,
        can_float,
        can_swim,
        avoid_sun: state.kind.is_bow_skeleton() && bright_now && state.equipment[5] == 0,
        tall: state.kind.props().height > 2.0,
    }
}

/// Vertical + horizontal melee check matching vanilla `Mob.getAttackBoundingBox(reach)`:
/// horizontal reach within `kind.attack_reach()` and vertical bounding-box overlap.
fn in_melee_reach(kind: MobKind, start: Vec3, target_pos: Vec3, horizontal: f64) -> bool {
    if horizontal > kind.attack_reach() {
        return false;
    }
    let mob_top = start.y + f64::from(kind.props().height);
    let target_top = target_pos.y + PLAYER_HEIGHT;
    start.y < target_top && mob_top > target_pos.y
}

/// Line of sight between a mob's eyes and a target player's eyes (vanilla `Sensing.hasLineOfSight`).
pub(crate) fn has_line_of_sight(
    blocks: &impl BlockGetter,
    from_feet: Vec3,
    eye_height: f64,
    target_feet: Vec3,
) -> bool {
    let from = Vec3::new(from_feet.x, from_feet.y + eye_height, from_feet.z);
    let to = Vec3::new(
        target_feet.x,
        target_feet.y + PLAYER_EYE_HEIGHT,
        target_feet.z,
    );
    let delta = to - from;
    let length = delta.length();
    if length < 1e-6 {
        return true;
    }
    let steps = (length / 0.25).ceil().max(1.0) as u32;
    (1..steps).all(|step| {
        let at = from + delta * (f64::from(step) / f64::from(steps));
        physics::no_collision(
            blocks,
            ferrite_math::AABB::new(at.x, at.y, at.z, at.x + 1e-3, at.y + 1e-3, at.z + 1e-3),
        )
    })
}

/// Direction away from the nearest threat within `radius` blocks (`AvoidEntityGoal`).
fn flee_entity_direction(pos: Vec3, threats: &[Vec3], radius: f64) -> Option<Vec3> {
    let radius_sqr = radius * radius;
    let nearest = threats
        .iter()
        .filter_map(|&threat| {
            let dx = pos.x - threat.x;
            let dy = pos.y - threat.y;
            let dz = pos.z - threat.z;
            let dist_sqr = dx * dx + dy * dy + dz * dz;
            (dist_sqr <= radius_sqr).then_some((dist_sqr, Vec3::new(dx, 0.0, dz)))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))?;
    let away = nearest.1;
    (away.length_sqr() > 1e-6).then(|| away.normalize())
}

/// Nearest player within [`TEMPT_RANGE`] holding an item that tempts `kind` in main or off hand.
fn tempt_target(
    kind: MobKind,
    pos: Vec3,
    players: &[Target],
    held_items: &std::collections::HashMap<Uuid, [Option<&'static str>; 2]>,
) -> Option<Target> {
    if !kind.is_animal() || held_items.is_empty() {
        return None;
    }
    let range_sqr = TEMPT_RANGE * TEMPT_RANGE;
    players
        .iter()
        .filter(|target| {
            let Some(hands) = held_items.get(&target.uuid) else {
                return false;
            };
            let tempting = hands
                .iter()
                .flatten()
                .any(|item| crate::breed::is_tempt_item(kind, item));
            tempting && pos.distance_to_sqr(target.pos) <= range_sqr
        })
        .min_by(|a, b| {
            pos.distance_to_sqr(a.pos)
                .total_cmp(&pos.distance_to_sqr(b.pos))
                .then_with(|| a.uuid.cmp(&b.uuid))
        })
        .copied()
}

fn ahead_is_fluid(blocks: &impl BlockGetter, pos: Vec3, yaw: f32) -> bool {
    let (sin, cos) = f64::from(yaw).to_radians().sin_cos();
    let y = pos.y.floor() as i32;
    for step in [1.0, 2.0] {
        let x = (pos.x - sin * step).floor() as i32;
        let z = (pos.z + cos * step).floor() as i32;
        for probe_y in [y, y - 1] {
            if let Some(state) = blocks.block_state(x, probe_y, z)
                && usize::from(state) < ferrite_data::blocks::STATE_COUNT
                && ferrite_data::block_props::get(state).fluid.is_some()
            {
                return true;
            }
        }
    }
    false
}

fn pick_dry_wander_yaw(
    blocks: &impl BlockGetter,
    start: Vec3,
    in_water: bool,
    rng: &mut Rng,
) -> f32 {
    let first = rng.next_f32() * 360.0;
    if in_water || !ahead_is_fluid(blocks, start, first) {
        return first;
    }
    for _ in 0..7 {
        let candidate = rng.next_f32() * 360.0;
        if !ahead_is_fluid(blocks, start, candidate) {
            return candidate;
        }
    }
    first
}

/// Chase acceleration for `key`: baby zombies move half again as fast as adults.
fn chase_speed(store: &EntityStore, key: crate::EntityKey) -> f64 {
    let baby =
        mob::state(store, key).is_some_and(|state| state.kind.is_zombie_like() && state.age < 0);
    CHASE_ACCELERATION * if baby { BABY_ZOMBIE_SPEED } else { 1.0 }
}

/// Vanilla `Projectile.shoot`: the aim direction plus a Gaussian spread of
/// `0.0075 * inaccuracy` on each axis.
fn arrow_direction(from: Vec3, to: Vec3, inaccuracy: f64, rng: &mut Rng) -> Vec3 {
    let aim = (to - from).normalize();
    let spread = 0.0075 * inaccuracy;
    aim + Vec3::new(
        rng.next_gaussian() * spread,
        rng.next_gaussian() * spread,
        rng.next_gaussian() * spread,
    )
}

/// Vanilla 26.3 `Projectile.getMovementToShoot`: normalized direction perturbed by triangular
/// noise scaled by `0.0172275 * uncertainty`, scaled by `speed`.
fn projectile_velocity(delta: Vec3, speed: f64, uncertainty: f64, rng: &mut Rng) -> Vec3 {
    let dir = if delta.length_sqr() > 1.0e-12 {
        delta.normalize()
    } else {
        Vec3::new(0.0, 0.0, 1.0)
    };
    let dev = 0.0172275 * uncertainty;
    Vec3::new(
        dir.x + rng.triangle(0.0, dev),
        dir.y + rng.triangle(0.0, dev),
        dir.z + rng.triangle(0.0, dev),
    ) * speed
}

fn ranged_skeleton_effect(kind: MobKind) -> Option<crate::effect::MobEffectInstance> {
    match kind {
        MobKind::Stray => Some(crate::effect::MobEffectInstance::new(
            crate::effect::Effect::Slowness.id(),
            crate::arrow::STRAY_SLOWNESS_TICKS,
            0,
        )),
        MobKind::Bogged => Some(crate::effect::MobEffectInstance::new(
            crate::effect::Effect::Poison.id(),
            crate::arrow::BOGGED_POISON_TICKS,
            0,
        )),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn tick_melee_or_chase(
    store: &mut EntityStore,
    key: crate::EntityKey,
    kind: MobKind,
    start: Vec3,
    target: Target,
    horizontal: f64,
    blocks: &impl BlockGetter,
    path_opts: &path::PathOptions,
    events: &mut Vec<MobEvent>,
    walking: &mut Option<f32>,
    speed: &mut f64,
) {
    if in_melee_reach(kind, start, target.pos, horizontal) {
        let ready = mob::state(store, key).is_some_and(|state| state.attack_cooldown == 0);
        if ready {
            events.push(MobEvent::Melee {
                player: target.uuid,
                damage: kind.attack_damage(),
                from: start,
                attacker: key,
            });
            if let Some(state) = mob::state_mut(store, key) {
                state.attack_cooldown = MELEE_COOLDOWN;
            }
        }
    } else {
        *walking = follow_path_with(store, key, blocks, start, target.pos, path_opts);
        *speed = chase_speed(store, key);
    }
}

fn sync_mob_metadata_now(store: &mut EntityStore, key: crate::EntityKey) {
    if let Some(data) = store.data_mut(key) {
        mob::sync_state_metadata(data);
    }
}

fn reset_blaze_goal(store: &mut EntityStore, key: crate::EntityKey) {
    let changed = if let Some(state) = mob::state_mut(store, key) {
        let dirty = state.charged || state.attack_step != 0 || state.blaze_last_seen != 0;
        state.attack_step = 0;
        state.charged = false;
        state.blaze_last_seen = 0;
        dirty
    } else {
        false
    };
    if changed {
        sync_mob_metadata_now(store, key);
    }
}

/// Vanilla `Blaze.customServerAiStep`: refresh `allowedHeightOffset` every 100 ticks and rise
/// toward `target.eyeY > blaze.eyeY + allowedHeightOffset`.
fn tick_blaze_hover(
    store: &mut EntityStore,
    key: crate::EntityKey,
    start: Vec3,
    target: Option<Target>,
    rng: &mut Rng,
) -> bool {
    let Some(state) = mob::state_mut(store, key) else {
        return false;
    };
    state.height_offset_timer = state.height_offset_timer.saturating_sub(1);
    if state.height_offset_timer == 0 {
        state.height_offset_timer = BLAZE_HEIGHT_OFFSET_TICKS;
        state.height_offset = rng.triangle(0.5, 6.891) as f32;
    }
    let offset = f64::from(state.height_offset);
    target.is_some_and(|t| {
        t.pos.y + PLAYER_EYE_HEIGHT
            > start.y + f64::from(MobKind::Blaze.props().eye_height) + offset
    })
}

/// Vanilla `Blaze.BlazeAttackGoal`: melee inside 2 blocks (when visible), else 3-fireball volley
/// after a 60-tick charge with 6 ticks between shots and a 100-tick cooldown.
#[allow(clippy::too_many_arguments)]
fn tick_blaze_goal(
    store: &mut EntityStore,
    key: crate::EntityKey,
    kind: MobKind,
    start: Vec3,
    target: Target,
    blocks: &impl BlockGetter,
    path_opts: &path::PathOptions,
    rng: &mut Rng,
    events: &mut Vec<MobEvent>,
    walking: &mut Option<f32>,
    speed: &mut f64,
    look: &mut Option<(f32, f32)>,
) {
    let eye_height = f64::from(kind.props().eye_height);
    let has_los = has_line_of_sight(blocks, start, eye_height, target.pos);
    let dist_sqr = start.distance_to_sqr(target.pos);
    let follow_range = kind.follow_range();
    let horizontal = (target.pos.x - start.x).hypot(target.pos.z - start.z);

    let last_seen = if let Some(state) = mob::state_mut(store, key) {
        if has_los {
            state.blaze_last_seen = 0;
        } else {
            state.blaze_last_seen = state.blaze_last_seen.saturating_add(1);
        }
        state.blaze_last_seen
    } else {
        return;
    };

    if dist_sqr < BLAZE_MELEE_DIST_SQR {
        if !has_los {
            reset_blaze_goal(store, key);
            return;
        }
        *look = look_at_point(start, eye_height, target.pos, horizontal);
        let ready = mob::state(store, key).is_some_and(|s| s.bow_timer == 0);
        if ready {
            events.push(MobEvent::Melee {
                player: target.uuid,
                damage: kind.attack_damage(),
                from: start,
                attacker: key,
            });
            if let Some(state) = mob::state_mut(store, key) {
                state.bow_timer = BLAZE_MELEE_COOLDOWN;
                state.attack_step = 0;
                state.charged = false;
            }
            sync_mob_metadata_now(store, key);
        }
        *walking = follow_path_with(store, key, blocks, start, target.pos, path_opts);
        *speed = chase_speed(store, key);
    } else if dist_sqr < follow_range * follow_range && has_los {
        *look = look_at_point(start, eye_height, target.pos, horizontal);
        let (shoot, charged_changed) = if let Some(state) = mob::state_mut(store, key) {
            if state.bow_timer == 0 {
                let prev_charged = state.charged;
                state.attack_step = state.attack_step.saturating_add(1);
                if state.attack_step == 1 {
                    state.bow_timer = BLAZE_CHARGE_TICKS;
                    state.charged = true;
                } else if state.attack_step <= 4 {
                    state.bow_timer = BLAZE_VOLLEY_INTERVAL;
                } else {
                    state.bow_timer = BLAZE_COOLDOWN_TICKS;
                    state.attack_step = 0;
                    state.charged = false;
                }
                (state.attack_step > 1, state.charged != prev_charged)
            } else {
                (false, false)
            }
        } else {
            (false, false)
        };
        if charged_changed {
            sync_mob_metadata_now(store, key);
        }
        if shoot {
            let spread_scale = dist_sqr.sqrt().sqrt() * 0.5;
            let dx = target.pos.x - start.x;
            let dy = (target.pos.y + PLAYER_HEIGHT * 0.5)
                - (start.y + f64::from(kind.props().height) * 0.5);
            let dz = target.pos.z - start.z;
            let dir = Vec3::new(
                rng.triangle(dx, 2.297 * spread_scale),
                dy,
                rng.triangle(dz, 2.297 * spread_scale),
            );
            let dir = if dir.length_sqr() > 1.0e-12 {
                dir.normalize()
            } else {
                Vec3::new(0.0, 0.0, 1.0)
            };
            let from = Vec3::new(
                start.x,
                start.y + f64::from(kind.props().height) * 0.5 + 0.5,
                start.z,
            );
            let owner = store.data(key).map(|data| data.uuid);
            events.push(MobEvent::SmallFireball {
                from,
                velocity: dir * BLAZE_FIREBALL_SPEED,
                owner,
            });
        }
    } else if last_seen < 5 {
        *look = look_at_point(start, eye_height, target.pos, horizontal);
        *walking = follow_path_with(store, key, blocks, start, target.pos, path_opts);
    } else {
        reset_blaze_goal(store, key);
    }
}

/// Vanilla `Witch.aiStep` potion drinking state machine (without a synthetic inventory).
fn tick_witch_drinking(
    store: &mut EntityStore,
    key: crate::EntityKey,
    eye_in_water: bool,
    target_dist_sqr: Option<f64>,
    rng: &mut Rng,
    events: &mut Vec<MobEvent>,
) {
    let (drink_timer, fire, last_damage_fire, health) = match mob::state(store, key) {
        Some(s) => (s.drink_timer, s.fire, s.last_damage_fire, s.health),
        None => return,
    };
    if drink_timer > 0 {
        let finished = if let Some(state) = mob::state_mut(store, key) {
            state.drink_timer -= 1;
            if state.drink_timer == 0 {
                state.using_item = false;
                state.drink_potion.take()
            } else {
                None
            }
        } else {
            None
        };
        if drink_timer == 1 {
            mob::set_equipment_stack(store, key, crate::EquipmentSlot::MainHand, None);
            sync_mob_metadata_now(store, key);
            if let Some(potion) = finished
                && let Some(def) = crate::potion::by_name(potion.potion_name())
            {
                for &(effect, duration, amplifier) in def.effects {
                    let instance =
                        crate::effect::MobEffectInstance::new(effect.id(), duration, amplifier);
                    if let Some(stored) = mob::add_effect(store, key, instance) {
                        events.push(MobEvent::EffectAdded {
                            key,
                            instance: stored,
                        });
                    }
                }
            }
        }
        return;
    }

    let has = |effect: crate::effect::Effect| -> bool {
        store.data(key).is_some_and(|d| d.effects.has(effect))
    };
    let picked =
        if rng.next_f32() < 0.15 && eye_in_water && !has(crate::effect::Effect::WaterBreathing) {
            Some(mob::WitchPotion::WaterBreathing)
        } else if rng.next_f32() < 0.15
            && (fire > 0 || last_damage_fire)
            && !has(crate::effect::Effect::FireResistance)
        {
            Some(mob::WitchPotion::FireResistance)
        } else if rng.next_f32() < 0.05 && health < MobKind::Witch.max_health() {
            Some(mob::WitchPotion::Healing)
        } else if rng.next_f32() < 0.5
            && target_dist_sqr.is_some_and(|d2| d2 > 121.0)
            && !has(crate::effect::Effect::Speed)
        {
            Some(mob::WitchPotion::Swiftness)
        } else {
            None
        };

    if let Some(potion) = picked {
        if let Some(state) = mob::state_mut(store, key) {
            state.drink_potion = Some(potion);
            state.drink_timer = mob::WITCH_DRINK_TICKS;
            state.using_item = true;
            state.last_damage_fire = false;
        }
        mob::set_equipment_stack(
            store,
            key,
            crate::EquipmentSlot::MainHand,
            Some(potion.stack("minecraft:potion")),
        );
        sync_mob_metadata_now(store, key);
    }
}

/// Vanilla `RangedAttackGoal` + `Witch.performRangedAttack`.
#[allow(clippy::too_many_arguments)]
fn tick_witch_goal(
    store: &mut EntityStore,
    key: crate::EntityKey,
    kind: MobKind,
    start: Vec3,
    target: Target,
    blocks: &impl BlockGetter,
    path_opts: &path::PathOptions,
    player_info: &std::collections::HashMap<Uuid, PlayerSnapshot>,
    rng: &mut Rng,
    events: &mut Vec<MobEvent>,
    walking: &mut Option<f32>,
    speed: &mut f64,
    look: &mut Option<(f32, f32)>,
) {
    let eye_height = f64::from(kind.props().eye_height);
    let has_los = has_line_of_sight(blocks, start, eye_height, target.pos);
    let dist_sqr = start.distance_to_sqr(target.pos);
    let see_time = if let Some(state) = mob::state_mut(store, key) {
        if has_los {
            state.see_time = state.see_time.saturating_add(1);
        } else {
            state.see_time = 0;
        }
        state.see_time
    } else {
        return;
    };

    let horizontal_raw = (target.pos.x - start.x).hypot(target.pos.z - start.z);
    *look = look_at_point(start, eye_height, target.pos, horizontal_raw);

    if dist_sqr <= WITCH_ATTACK_RADIUS * WITCH_ATTACK_RADIUS && see_time >= WITCH_SEE_TIME_STOP {
        *walking = None;
    } else {
        *walking = follow_path_with(store, key, blocks, start, target.pos, path_opts);
        *speed = 1.0;
    }

    let (ready, drinking) =
        mob::state(store, key).map_or((false, false), |s| (s.bow_timer == 0, s.drink_timer > 0));
    if !ready || !has_los {
        return;
    }
    if let Some(state) = mob::state_mut(store, key) {
        state.bow_timer = WITCH_ATTACK_INTERVAL;
    }
    if drinking {
        return;
    }

    let snapshot = player_info
        .get(&target.uuid)
        .copied()
        .unwrap_or_else(PlayerSnapshot::full_health);
    let dx = (target.pos.x + snapshot.vel.x) - start.x;
    let dy = (target.pos.y + PLAYER_EYE_HEIGHT - 1.1) - start.y;
    let dz = (target.pos.z + snapshot.vel.z) - start.z;
    let horiz = (dx * dx + dz * dz).sqrt();
    let potion = if horiz >= 8.0 && !snapshot.has_slowness {
        mob::WitchPotion::Slowness
    } else if snapshot.health >= 8.0 && !snapshot.has_poison {
        mob::WitchPotion::Poison
    } else if horiz <= 3.0 && !snapshot.has_weakness && rng.next_f32() < 0.25 {
        mob::WitchPotion::Weakness
    } else {
        mob::WitchPotion::Harming
    };
    let throw_speed = if horiz <= 2.0 { 0.45 } else { 0.75 };
    let velocity = projectile_velocity(
        Vec3::new(dx, dy + horiz * 0.2, dz),
        throw_speed,
        WITCH_THROW_INACCURACY,
        rng,
    );
    let from = Vec3::new(start.x, start.y + eye_height - 0.1, start.z);
    let owner = store.data(key).map(|data| data.uuid);
    events.push(MobEvent::SplashPotion {
        from,
        velocity,
        owner,
        potion,
    });
}

fn reset_piglin_charge(store: &mut EntityStore, key: crate::EntityKey) {
    let changed = if let Some(state) = mob::state_mut(store, key) {
        let dirty = state.charging_crossbow || state.crossbow_state == mob::CrossbowState::Charging;
        state.charging_crossbow = false;
        if state.crossbow_state == mob::CrossbowState::Charging {
            state.crossbow_state = mob::CrossbowState::Uncharged;
        }
        dirty
    } else {
        false
    };
    if changed {
        sync_mob_metadata_now(store, key);
    }
}

/// Vanilla `Piglin` `CrossbowAttack` + `BackUpIfTooClose(5, 0.75f)`. Returns `false` if not
/// an adult holding a crossbow so the caller runs shared melee.
#[allow(clippy::too_many_arguments)]
fn tick_piglin_crossbow(
    store: &mut EntityStore,
    key: crate::EntityKey,
    kind: MobKind,
    start: Vec3,
    target: Target,
    crossbow_item_id: u16,
    blocks: &impl BlockGetter,
    path_opts: &path::PathOptions,
    hard: bool,
    rng: &mut Rng,
    events: &mut Vec<MobEvent>,
    walking: &mut Option<f32>,
    speed: &mut f64,
    look: &mut Option<(f32, f32)>,
) -> bool {
    let has_crossbow = mob::state(store, key).is_some_and(|s| {
        s.age >= 0
            && crossbow_item_id != 0
            && s.equipment[mob::MAIN_HAND as usize] == crossbow_item_id
    });
    if !has_crossbow {
        return false;
    }
    let eye_height = f64::from(kind.props().eye_height);
    let dx = target.pos.x - start.x;
    let dz = target.pos.z - start.z;
    let horizontal = (dx * dx + dz * dz).sqrt();
    let yaw = yaw_towards(dx, dz);
    let dist_sqr = start.distance_to_sqr(target.pos);
    let has_los = has_line_of_sight(blocks, start, eye_height, target.pos);
    *look = look_at_point(start, eye_height, target.pos, horizontal);

    if dist_sqr > PIGLIN_CROSSBOW_RANGE * PIGLIN_CROSSBOW_RANGE || !has_los {
        reset_piglin_charge(store, key);
        *walking = follow_path_with(store, key, blocks, start, target.pos, path_opts);
        *speed = chase_speed(store, key);
        return true;
    }

    if dist_sqr < PIGLIN_BACKUP_DISTANCE * PIGLIN_BACKUP_DISTANCE {
        *walking = Some(wrap_degrees(yaw + 180.0));
        *speed = PIGLIN_BACKUP_SPEED;
    } else {
        *walking = None;
    }

    let (shoot, changed) = if let Some(state) = mob::state_mut(store, key) {
        let prev_charging = state.charging_crossbow;
        let mut fire = false;
        match state.crossbow_state {
            mob::CrossbowState::Uncharged => {
                state.crossbow_state = mob::CrossbowState::Charging;
                state.bow_timer = PIGLIN_CROSSBOW_CHARGE_TICKS;
                state.charging_crossbow = true;
            }
            mob::CrossbowState::Charging => {
                if state.bow_timer == 0 {
                    state.crossbow_state = mob::CrossbowState::Charged;
                    state.bow_timer = 20 + rng.next_int(20) as u16;
                    state.charging_crossbow = false;
                }
            }
            mob::CrossbowState::Charged => {
                if state.bow_timer == 0 {
                    state.crossbow_state = mob::CrossbowState::ReadyToAttack;
                }
            }
            mob::CrossbowState::ReadyToAttack => {
                state.crossbow_state = mob::CrossbowState::Uncharged;
                state.charging_crossbow = false;
                fire = true;
            }
        }
        (fire, state.charging_crossbow != prev_charging)
    } else {
        (false, false)
    };
    if changed {
        sync_mob_metadata_now(store, key);
    }
    if shoot {
        let from = Vec3::new(start.x, start.y + eye_height, start.z);
        let aim_y = (target.pos.y + PLAYER_HEIGHT / 3.0) + horizontal * 0.2 - from.y;
        let uncertainty = if hard {
            SKELETON_INACCURACY_HARD
        } else {
            SKELETON_INACCURACY
        };
        let direction = projectile_velocity(Vec3::new(dx, aim_y, dz), 1.0, uncertainty, rng);
        let owner = store.data(key).map(|data| data.uuid);
        events.push(MobEvent::Arrow {
            from,
            direction,
            damage: PIGLIN_CROSSBOW_DAMAGE,
            speed: PIGLIN_CROSSBOW_SPEED,
            effect: None,
            owner,
        });
    }
    true
}

/// Recomputes and follows the block path towards `target`, returning the yaw to walk along.
fn follow_path_with(
    store: &mut EntityStore,
    key: crate::EntityKey,
    blocks: &impl BlockGetter,
    start: Vec3,
    target: Vec3,
    options: &path::PathOptions,
) -> Option<f32> {
    let (mut len, mut index, mut timer, mut path, max_range) =
        mob::state(store, key).map_or((0u8, 0u8, 0u16, [[0i32; 3]; path::MAX_PATH], 32), |state| {
            (
                state.path_len,
                state.path_index,
                state.path_timer,
                state.path,
                mob_path_range(state),
            )
        });
    if timer == 0 || (len > 0 && index >= len) {
        let goal = [
            target.x.floor() as i32,
            target.y.floor() as i32,
            target.z.floor() as i32,
        ];
        let found = path::find_path_with(
            blocks,
            [
                start.x.floor() as i32,
                start.y.floor() as i32,
                start.z.floor() as i32,
            ],
            goal,
            max_range,
            options,
        );
        len = u8::try_from(found.len().min(path::MAX_PATH)).unwrap_or(u8::MAX);
        index = 0;
        timer = REPATH_TICKS;
        path = [[0; 3]; path::MAX_PATH];
        for (slot, waypoint) in path.iter_mut().zip(found.iter()) {
            *slot = *waypoint;
        }
    }
    let mut yaw = None;
    while index < len {
        let waypoint = path[usize::from(index)];
        let dx = f64::from(waypoint[0]) + 0.5 - start.x;
        let dz = f64::from(waypoint[2]) + 0.5 - start.z;
        let horiz = (dx * dx + dz * dz).sqrt();
        let wy = f64::from(waypoint[1]);
        let reached = if wy > start.y + 0.25 {
            horiz < 0.6 && (start.y - wy).abs() < 0.25
        } else {
            horiz < 0.6 && (start.y - wy).abs() < 1.5
        };
        if reached {
            index += 1;
            continue;
        }
        // `WallClimberNavigation`: when climbing straight up a vertical wall (waypoint is above
        // current cell within the spider's half-width), push directly into an adjacent solid wall
        // face so `HORIZONTAL_COLLISION` drives `spider::climb_velocity`.
        if options.can_climb && horiz < 0.75 && wy > start.y + 0.25 {
            let (bx, by, bz) = (
                start.x.floor() as i32,
                start.y.floor() as i32,
                start.z.floor() as i32,
            );
            let look_ahead = path[usize::from(index) + 1..usize::from(len)]
                .iter()
                .find_map(|next| {
                    let ndx = f64::from(next[0]) + 0.5 - start.x;
                    let ndz = f64::from(next[2]) + 0.5 - start.z;
                    (ndx.hypot(ndz) >= 0.25).then_some((ndx, ndz))
                });
            let solid_wall_dir = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .into_iter()
                .filter(|&(wx, wz)| {
                    blocks
                        .block_state(bx + wx, by, bz + wz)
                        .filter(|s| usize::from(*s) < ferrite_data::blocks::STATE_COUNT)
                        .is_some_and(|s| !ferrite_data::block_props::collision_shape(s).is_empty())
                })
                .max_by(|&(ax, az), &(bx_dir, bz_dir)| {
                    let score = |(wx, wz): (i32, i32)| match look_ahead {
                        Some((lx, lz)) => f64::from(wx) * lx + f64::from(wz) * lz,
                        None => 0.0,
                    };
                    score((ax, az)).total_cmp(&score((bx_dir, bz_dir)))
                })
                .map(|(wx, wz)| (f64::from(wx), f64::from(wz)));
            if let Some((wdx, wdz)) = solid_wall_dir.or(look_ahead) {
                yaw = Some(yaw_towards(wdx, wdz));
                break;
            }
        }
        // When sidestepping mid-air along a wall face around a partial overhang, hug the adjacent
        // solid wall face so `HORIZONTAL_COLLISION` stays active and the spider does not fall.
        if options.can_climb && wy <= start.y + 0.25 && (start.y - wy).abs() < 0.75 {
            let (bx, by, bz) = (
                start.x.floor() as i32,
                start.y.floor() as i32,
                start.z.floor() as i32,
            );
            let has_floor = blocks
                .block_state(bx, by - 1, bz)
                .filter(|s| usize::from(*s) < ferrite_data::blocks::STATE_COUNT)
                .is_some_and(|s| !ferrite_data::block_props::collision_shape(s).is_empty());
            if !has_floor {
                let mut hug_x = 0.0;
                let mut hug_z = 0.0;
                for (wx, wz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    if blocks
                        .block_state(bx + wx, by, bz + wz)
                        .filter(|s| usize::from(*s) < ferrite_data::blocks::STATE_COUNT)
                        .is_some_and(|s| !ferrite_data::block_props::collision_shape(s).is_empty())
                    {
                        hug_x += f64::from(wx);
                        hug_z += f64::from(wz);
                    }
                }
                if hug_x != 0.0 || hug_z != 0.0 {
                    yaw = Some(yaw_towards(dx + hug_x * 0.45, dz + hug_z * 0.45));
                    break;
                }
            }
        }
        yaw = Some(yaw_towards(dx, dz));
        break;
    }
    if let Some(state) = mob::state_mut(store, key) {
        state.path = path;
        state.path_len = len;
        state.path_index = index;
        state.path_timer = timer;
    }
    if yaw.is_none() {
        let dx = target.x - start.x;
        let dz = target.z - start.z;
        let horiz_sqr = dx * dx + dz * dz;
        if horiz_sqr > 0.25 && horiz_sqr <= 144.0 {
            return Some(yaw_towards(dx, dz));
        }
    }
    yaw
}

/// Vanilla enderman teleport: a random walkable spot within eight blocks.
fn blink(
    store: &mut EntityStore,
    key: crate::EntityKey,
    blocks: &impl BlockGetter,
    start: Vec3,
    rng: &mut Rng,
) -> Option<(Vec3, Vec3)> {
    for _ in 0..8 {
        let x = start.x.floor() as i32 + rng.next_int(17) - 8;
        let z = start.z.floor() as i32 + rng.next_int(17) - 8;
        let y = start.y.floor() as i32 + rng.next_int(5) - 2;
        if path::start_node(blocks, [x, y, z]) == Some([x, y, z]) {
            let to = Vec3::new(f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5);
            crate::tracker::teleport(store, key, to, Vec3::ZERO);
            return Some((start, to));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mob::{MobKind, new_mob};
    use crate::physics::tests::TestWorld;

    fn cow(x: f64, y: f64, z: f64) -> (EntityStore, crate::EntityKey) {
        let mut store = EntityStore::default();
        let key = store.insert(new_mob(MobKind::Cow, Vec3::new(x, y, z), &mut Rng::new(1)));
        (store, key)
    }

    fn target(x: f64, z: f64) -> Target {
        Target {
            uuid: Uuid::from_u128(9),
            pos: Vec3::new(x, 64.0, z),
        }
    }

    #[test]
    fn a_villager_walks_to_its_scheduled_destination() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let key = store.insert(new_mob(
            MobKind::Villager,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(11),
        ));
        {
            let villager = store.data_mut(key).unwrap().villager.as_mut().unwrap();
            villager.activity = crate::villager::Activity::Work;
            villager.destination = Some([8, 64, 0]);
        }
        let start = store.pos(key).unwrap();
        let mut rng = Rng::new(12);
        for _ in 0..120 {
            tick(&mut store, &world, &[], &mut rng);
        }
        let end = store.pos(key).unwrap();
        assert!(
            end.x > start.x + 2.0,
            "villager should walk toward work: {start:?} -> {end:?}"
        );
    }

    #[test]
    fn a_zombie_walks_toward_a_village_bell() {
        let mut world = TestWorld::flat(64);
        let bell = ferrite_data::blocks::default_state("minecraft:bell").unwrap();
        world.blocks.insert((8, 64, 0), bell);
        let mut store = EntityStore::default();
        let key = store.insert(new_mob(
            MobKind::Husk,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(13),
        ));
        let start = store.pos(key).unwrap();
        let mut rng = Rng::new(14);
        // Keep a player within the despawn radius but outside the husk's follow range so the
        // hostile mob is not removed before the village goal can run.
        let players = [Vec3::new(100.0, 64.0, 0.0)];
        for tick_idx in 0..80 {
            tick(&mut store, &world, &players, &mut rng);
            if tick_idx == 10 {
                assert_eq!(
                    mob::state(&store, key).unwrap().village_target,
                    Some([8, 64, 0])
                );
            }
        }
        assert!(
            store.pos(key).unwrap().x > start.x + 1.0,
            "the husk should walk toward the bell: {start:?} -> {:?}",
            store.pos(key)
        );
    }

    #[test]
    fn gravity_lands_a_mob_on_the_ground() {
        let world = TestWorld::flat(64);
        let (mut store, key) = cow(0.5, 70.0, 0.5);
        let mut rng = Rng::new(2);
        for _ in 0..80 {
            tick(&mut store, &world, &[], &mut rng);
        }
        let pos = store.pos(key).unwrap();
        assert!((pos.y - 64.0).abs() < 1e-9, "rests on the floor: {pos:?}");
        assert!(store.flags(key).unwrap().has(Flags::ON_GROUND));
        assert_eq!(store.vel(key).unwrap().y, 0.0);
        assert_eq!(mob::state(&store, key).unwrap().fall_distance, 0.0);
    }

    #[test]
    fn mob_falls_off_ledge_instead_of_floating() {
        let mut world = TestWorld::flat(61);
        let stone = ferrite_data::blocks::default_state("minecraft:stone").unwrap();
        for x in -4..=1 {
            for y in 61..=63 {
                for z in -4..=4 {
                    world.blocks.insert((x, y, z), stone);
                }
            }
        }
        let mut store = EntityStore::default();
        let key = store.insert(new_mob(
            MobKind::Zombie,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(77),
        ));
        let tgt = Target {
            uuid: Uuid::from_u128(10),
            pos: Vec3::new(6.5, 61.0, 0.5),
        };
        let mut rng = Rng::new(78);
        for _ in 0..80 {
            tick_with_holders(&mut store, &world, &[tgt], &[], &mut rng);
        }
        let pos = store.pos(key).unwrap();
        assert!(
            pos.x > 2.0 && pos.y < 63.0,
            "mob should walk off ledge and fall onto lower floor: {pos:?}"
        );
    }

    #[test]
    fn mob_steps_up_half_slab() {
        let mut world = TestWorld::flat(64);
        let slab = ferrite_data::blocks::default_state("minecraft:stone_slab").unwrap();
        for z in -2..=2 {
            world.blocks.insert((2, 64, z), slab);
        }
        let mut store = EntityStore::default();
        let key = store.insert(new_mob(
            MobKind::Zombie,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(88),
        ));
        let tgt = Target {
            uuid: Uuid::from_u128(11),
            pos: Vec3::new(6.5, 64.0, 0.5),
        };
        let mut rng = Rng::new(89);
        for _ in 0..80 {
            tick_with_holders(&mut store, &world, &[tgt], &[], &mut rng);
        }
        let pos = store.pos(key).unwrap();
        assert!(
            pos.x > 2.5,
            "mob should step up 0.5-block slab smoothly: {pos:?}"
        );
    }

    #[test]
    fn a_long_fall_kills_a_cow() {
        let world = TestWorld::flat(64);
        let (mut store, key) = cow(0.5, 100.5, 0.5);
        let mut rng = Rng::new(3);
        for _ in 0..120 {
            tick(&mut store, &world, &[], &mut rng);
            if !mob::is_mob(&store, key) {
                break;
            }
        }
        assert!(!mob::is_mob(&store, key));
        assert!(store.flags(key).unwrap().has(Flags::REMOVED));
    }

    #[test]
    fn a_short_drop_hurts_but_does_not_kill() {
        let world = TestWorld::flat(64);
        let (mut store, key) = cow(0.5, 68.0, 0.5);
        let mut rng = Rng::new(4);
        for _ in 0..60 {
            tick(&mut store, &world, &[], &mut rng);
        }
        let health = mob::state(&store, key).unwrap().health;
        assert!((health - 9.0).abs() < 1e-6, "four-block fall: {health}");
    }

    #[test]
    fn despawn_rules_run_every_tick() {
        let world = TestWorld::flat(64);
        let (mut store, key) = cow(0.5, 64.0, 0.5);
        mob::state_mut(&mut store, key).unwrap().despawns = true;
        let mut rng = Rng::new(5);
        tick(
            &mut store,
            &world,
            &[Vec3::new(1000.0, 64.0, 1000.0)],
            &mut rng,
        );
        assert!(store.flags(key).unwrap().has(Flags::REMOVED));
    }

    #[test]
    fn wander_walks_along_the_chosen_yaw() {
        let world = TestWorld::flat(64);
        let (mut store, key) = cow(0.5, 64.0, 0.5);
        {
            let mob = mob::state_mut(&mut store, key).unwrap();
            mob.moving = true;
            mob.wander_timer = 200;
            mob.wander_yaw = 0.0;
        }
        let mut rng = Rng::new(6);
        for _ in 0..40 {
            tick(&mut store, &world, &[], &mut rng);
        }
        let pos = store.pos(key).unwrap();
        // Yaw zero looks south, +z.
        assert!(pos.z > 1.0, "walked south: {pos:?}");
        assert!((pos.y - 64.0).abs() < 1e-9);
    }

    #[test]
    fn a_nearby_player_is_looked_at() {
        let mob = Vec3::new(0.5, 64.0, 0.5);
        let (yaw, _) = look_at(
            mob,
            f64::from(MobKind::Cow.props().eye_height),
            &[Vec3::new(4.5, 64.0, 0.5)],
        )
        .expect("player within eight blocks");
        assert!((yaw + 90.0).abs() < 1e-3, "faces east: {yaw}");
        assert!(
            look_at(
                mob,
                f64::from(MobKind::Cow.props().eye_height),
                &[Vec3::new(9.5, 64.0, 0.5)]
            )
            .is_none()
        );
    }

    #[test]
    fn a_tamed_wolf_bites_the_zombie_hunting_its_owner() {
        let world = TestWorld::flat(64);
        let mut rng = Rng::new(5);
        let mut store = EntityStore::default();
        let owner = Uuid::from_u128(1);
        let wolf = store.insert(mob::new_mob(
            MobKind::Wolf,
            Vec3::new(2.5, 64.0, 0.5),
            &mut rng,
        ));
        {
            let state = mob::state_mut(&mut store, wolf).unwrap();
            state.owner = Some(owner);
            state.sitting = false;
        }
        let zombie = store.insert(mob::new_mob(
            MobKind::Zombie,
            Vec3::new(4.5, 64.0, 0.5),
            &mut rng,
        ));
        mob::state_mut(&mut store, zombie).unwrap().no_ai = true;
        mob::state_mut(&mut store, zombie).unwrap().target = Some(owner);
        let targets = [Target {
            uuid: owner,
            pos: Vec3::new(0.5, 64.0, 0.5),
        }];
        let before = mob::state(&store, zombie).unwrap().health;
        for _ in 0..200 {
            tick_with(&mut store, &world, &targets, &mut rng);
            mob::state_mut(&mut store, zombie).unwrap().target = Some(owner);
        }
        let after = mob::state(&store, zombie).map_or(0.0, |m| m.health);
        assert!(after < before, "{after} >= {before}");
    }

    #[test]
    fn a_tamed_wolf_bites_the_mob_its_owner_attacked() {
        let world = TestWorld::flat(64);
        let mut rng = Rng::new(9);
        let mut store = EntityStore::default();
        let owner = Uuid::from_u128(42);
        let wolf = store.insert(mob::new_mob(
            MobKind::Wolf,
            Vec3::new(3.5, 64.0, 0.5),
            &mut rng,
        ));
        mob::state_mut(&mut store, wolf).unwrap().owner = Some(owner);
        let attacked = store.insert(mob::new_mob(
            MobKind::Cow,
            Vec3::new(4.5, 64.0, 0.5),
            &mut rng,
        ));
        mob::state_mut(&mut store, attacked).unwrap().no_ai = true;
        let other = store.insert(mob::new_mob(
            MobKind::Cow,
            Vec3::new(4.5, 64.0, 1.5),
            &mut rng,
        ));
        mob::state_mut(&mut store, other).unwrap().no_ai = true;
        let players = [Target {
            uuid: owner,
            pos: Vec3::new(0.5, 64.0, 0.5),
        }];
        let health = mob::state(&store, attacked).unwrap().health;
        let other_health = mob::state(&store, other).unwrap().health;

        tick_with_owner_attacks(&mut store, &world, &players, &[(owner, attacked)], &mut rng);

        assert!(mob::state(&store, attacked).unwrap().health < health);
        assert_eq!(mob::state(&store, other).unwrap().health, other_health);
    }

    #[test]
    fn a_mob_without_ai_stays_where_it_stands() {
        let world = TestWorld::flat(64);
        let (mut store, key) = cow(0.5, 64.0, 0.5);
        mob::state_mut(&mut store, key).unwrap().no_ai = true;
        let mut rng = Rng::new(8);
        for _ in 0..400 {
            tick(&mut store, &world, &[], &mut rng);
        }
        let pos = store.pos(key).unwrap();
        assert!(
            (pos.x - 0.5).abs() < 0.3 && (pos.z - 0.5).abs() < 0.3,
            "{pos:?}"
        );
    }

    #[test]
    fn a_hurt_cow_flees_from_its_attacker() {
        let world = TestWorld::flat(64);
        let (mut store, key) = cow(0.5, 64.0, 0.5);
        mob::hurt(&mut store, key, 1.0, Some(Vec3::new(-3.0, 64.0, 0.5)));
        let mut rng = Rng::new(8);
        for _ in 0..30 {
            tick(&mut store, &world, &[], &mut rng);
        }
        let pos = store.pos(key).unwrap();
        assert!(pos.x > 1.0, "fled away from the attacker: {pos:?}");
        assert!(mob::state(&store, key).unwrap().panic > 0);
    }

    #[test]
    fn hostile_mobs_never_target_untargetable_players() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let zombie = store.insert(new_mob(
            MobKind::Zombie,
            Vec3::new(6.5, 64.0, 0.5),
            &mut Rng::new(1),
        ));
        let creative = target(0.5, 0.5);
        let hidden = MobRules {
            untargetable: [creative.uuid].into(),
            ..Default::default()
        };
        let mut rng = Rng::new(2);
        let mut hits = 0;
        for _ in 0..200 {
            for event in tick_full(&mut store, &world, &[creative], &hidden, &[], &[], &mut rng) {
                if matches!(event, MobEvent::Melee { .. }) {
                    hits += 1;
                }
            }
        }
        assert_eq!(hits, 0, "a creative player is never attacked");
        assert_eq!(mob::state(&store, zombie).unwrap().target, None);
        // Control: the same setup without the exclusion does attack.
        let none = MobRules::default();
        let attacked = (0..200).any(|_| {
            tick_full(&mut store, &world, &[creative], &none, &[], &[], &mut rng)
                .iter()
                .any(|event| matches!(event, MobEvent::Melee { .. }))
        });
        assert!(attacked, "without the exclusion the zombie attacks");
    }

    #[test]
    fn a_zombie_reaches_and_hits_its_target() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let zombie = store.insert(new_mob(
            MobKind::Zombie,
            Vec3::new(6.5, 64.0, 0.5),
            &mut Rng::new(1),
        ));
        let mut rng = Rng::new(2);
        let player = target(0.5, 0.5);
        let mut hits = Vec::new();
        for _ in 0..200 {
            for event in tick_with(&mut store, &world, &[player], &mut rng) {
                if let MobEvent::Melee { player, .. } = event {
                    hits.push(player);
                }
            }
            if !hits.is_empty() {
                break;
            }
        }
        assert_eq!(hits, [player.uuid], "the zombie hit the player");
        let pos = store.pos(zombie).unwrap();
        assert!(pos.x < 6.5, "the zombie walked closer: {pos:?}");
        assert_eq!(
            mob::state(&store, zombie).unwrap().health,
            MobKind::Zombie.max_health()
        );
    }

    #[test]
    fn a_wild_wolf_bites_whoever_hurt_it_and_a_tame_one_does_not() {
        let world = TestWorld::flat(64);
        let player = target(0.5, 0.5);
        for tame in [false, true] {
            let mut store = EntityStore::default();
            let wolf = store.insert(new_mob(
                MobKind::Wolf,
                Vec3::new(5.5, 64.0, 0.5),
                &mut Rng::new(1),
            ));
            if tame {
                mob::state_mut(&mut store, wolf).unwrap().owner = Some(player.uuid);
            }
            mob::hurt(&mut store, wolf, 1.0, Some(Vec3::new(0.5, 64.0, 0.5)));
            mob::retaliate(&mut store, wolf, player.uuid);
            let mut rng = Rng::new(2);
            let mut bit = false;
            for _ in 0..200 {
                for event in tick_with(&mut store, &world, &[player], &mut rng) {
                    bit |= matches!(event, MobEvent::Melee { .. });
                }
            }
            assert_eq!(bit, !tame, "tame: {tame}");
        }
    }

    #[test]
    fn hurting_one_wild_wolf_angers_the_pack_but_not_tame_or_far_wolves() {
        let mut store = EntityStore::default();
        let mut rng = Rng::new(1);
        let mut wolf = |store: &mut EntityStore, x: f64| {
            store.insert(new_mob(MobKind::Wolf, Vec3::new(x, 64.0, 0.5), &mut rng))
        };
        let hurt = wolf(&mut store, 0.5);
        let near = wolf(&mut store, 6.5);
        let far = wolf(&mut store, 60.5);
        let tame = wolf(&mut store, 3.5);
        mob::state_mut(&mut store, tame).unwrap().owner = Some(uuid::Uuid::from_u128(9));
        let player = uuid::Uuid::from_u128(1);
        mob::retaliate(&mut store, hurt, player);
        let target = |store: &EntityStore, k| mob::state(store, k).unwrap().target;
        assert_eq!(target(&store, near), Some(player));
        assert_eq!(target(&store, far), None);
        assert_eq!(target(&store, tame), None);
    }

    #[test]
    fn a_wild_llama_spits_at_whoever_hurt_it() {
        let world = TestWorld::flat(64);
        let player = target(0.5, 0.5);
        let mut store = EntityStore::default();
        let llama = store.insert(new_mob(
            MobKind::Llama,
            Vec3::new(6.5, 64.0, 0.5),
            &mut Rng::new(1),
        ));
        mob::hurt(&mut store, llama, 1.0, Some(Vec3::new(0.5, 64.0, 0.5)));
        assert!(mob::retaliate(&mut store, llama, player.uuid));
        let mut rng = Rng::new(2);
        let mut spat = false;
        for _ in 0..100 {
            for event in tick_with(&mut store, &world, &[player], &mut rng) {
                spat |= matches!(event, MobEvent::Spit { .. });
            }
        }
        assert!(spat);
    }

    #[test]
    fn a_trader_llama_follows_the_trader_that_leads_it() {
        let world = TestWorld::flat(64);
        let player = target(0.5, 0.5);
        let mut store = EntityStore::default();
        let trader = store.insert(new_mob(
            MobKind::WanderingTrader,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(1),
        ));
        let llama = store.insert(new_mob(
            MobKind::TraderLlama,
            Vec3::new(9.5, 64.0, 0.5),
            &mut Rng::new(2),
        ));
        let holder = store.data(trader).unwrap().uuid;
        mob::state_mut(&mut store, llama).unwrap().leash = Some((holder, 1));
        let mut rng = Rng::new(3);
        for _ in 0..40 {
            tick_with(&mut store, &world, &[player], &mut rng);
        }
        assert!(
            store.pos(llama).unwrap().x < 9.5,
            "the led llama is pulled to its trader"
        );
        assert_eq!(mob::state(&store, llama).unwrap().leash, Some((holder, 1)));
    }

    #[test]
    fn a_lead_pulls_its_animal_and_snaps_when_stretched() {
        let world = TestWorld::flat(64);
        let player = target(0.5, 0.5);
        let mut store = EntityStore::default();
        let cow = store.insert(new_mob(
            MobKind::Cow,
            Vec3::new(9.5, 64.0, 0.5),
            &mut Rng::new(1),
        ));
        mob::state_mut(&mut store, cow).unwrap().leash = Some((player.uuid, 1));
        let mut rng = Rng::new(2);
        let before = store.pos(cow).unwrap().x;
        for _ in 0..40 {
            tick_with(&mut store, &world, &[player], &mut rng);
        }
        assert!(
            store.pos(cow).unwrap().x < before,
            "the lead drags the cow closer"
        );
        // Far past twelve blocks it breaks.
        let far = store.insert(new_mob(
            MobKind::Cow,
            Vec3::new(30.5, 64.0, 0.5),
            &mut Rng::new(3),
        ));
        mob::state_mut(&mut store, far).unwrap().leash = Some((player.uuid, 1));
        let events = tick_with(&mut store, &world, &[player], &mut rng);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, MobEvent::LeashBroke { key } if *key == far))
        );
        assert!(mob::state(&store, far).unwrap().leash.is_none());
    }

    #[test]
    fn a_lead_follows_a_holder_in_another_region() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let cow = store.insert(new_mob(
            MobKind::Cow,
            Vec3::new(9.5, 64.0, 0.5),
            &mut Rng::new(1),
        ));
        let holder = Uuid::from_u128(77);
        mob::state_mut(&mut store, cow).unwrap().leash = Some((holder, 5));
        let holders = [(holder, Vec3::new(0.5, 64.0, 0.5))];
        let mut rng = Rng::new(2);
        let before = store.pos(cow).unwrap().x;
        for _ in 0..40 {
            tick_with_holders(&mut store, &world, &[], &holders, &mut rng);
        }
        assert!(
            store.pos(cow).unwrap().x < before,
            "the lead drags the cow towards the other region"
        );
        assert!(
            mob::state(&store, cow).unwrap().leash.is_some(),
            "a reachable holder keeps the lead"
        );
    }

    #[test]
    fn a_lead_snaps_once_when_its_cross_region_holder_is_too_far() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let cow = store.insert(new_mob(
            MobKind::Cow,
            Vec3::new(30.5, 64.0, 0.5),
            &mut Rng::new(1),
        ));
        let holder = Uuid::from_u128(78);
        mob::state_mut(&mut store, cow).unwrap().leash = Some((holder, 5));
        let holders = [(holder, Vec3::new(0.5, 64.0, 0.5))];
        let mut rng = Rng::new(2);
        let events = tick_with_holders(&mut store, &world, &[], &holders, &mut rng);
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, MobEvent::LeashBroke { key } if *key == cow))
                .count(),
            1,
            "the stretched lead snaps"
        );
        assert!(mob::state(&store, cow).unwrap().leash.is_none());
        let again = tick_with_holders(&mut store, &world, &[], &holders, &mut rng);
        assert!(
            !again
                .iter()
                .any(|event| matches!(event, MobEvent::LeashBroke { key } if *key == cow)),
            "a snapped lead does not break a second time"
        );
    }

    #[test]
    fn a_lead_breaks_once_when_its_holder_is_gone() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let cow = store.insert(new_mob(
            MobKind::Cow,
            Vec3::new(9.5, 64.0, 0.5),
            &mut Rng::new(1),
        ));
        mob::state_mut(&mut store, cow).unwrap().leash = Some((Uuid::from_u128(79), 5));
        let mut rng = Rng::new(2);
        let events = tick_with_holders(&mut store, &world, &[], &[], &mut rng);
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, MobEvent::LeashBroke { key } if *key == cow))
                .count(),
            1,
            "a removed holder breaks the lead"
        );
        assert!(mob::state(&store, cow).unwrap().leash.is_none());
        let again = tick_with_holders(&mut store, &world, &[], &[], &mut rng);
        assert!(
            !again
                .iter()
                .any(|event| matches!(event, MobEvent::LeashBroke { key } if *key == cow)),
            "only one break event"
        );
    }

    #[test]
    fn a_free_llama_joins_a_caravan_and_follows_its_head() {
        let world = TestWorld::flat(64);
        let player = target(0.5, 0.5);
        let mut store = EntityStore::default();
        let head = store.insert(new_mob(
            MobKind::Llama,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(1),
        ));
        mob::state_mut(&mut store, head).unwrap().leash = Some((player.uuid, 1));
        let tail = store.insert(new_mob(
            MobKind::Llama,
            Vec3::new(6.5, 64.0, 0.5),
            &mut Rng::new(2),
        ));
        let mut rng = Rng::new(3);
        tick_with(&mut store, &world, &[player], &mut rng);
        let head_uuid = store.data(head).unwrap().uuid;
        assert_eq!(mob::state(&store, tail).unwrap().caravan, Some(head_uuid));
        let before = store.pos(tail).unwrap().x;
        for _ in 0..30 {
            tick_with(&mut store, &world, &[player], &mut rng);
        }
        assert!(
            store.pos(tail).unwrap().x < before,
            "the tail llama trails its head"
        );
    }

    #[test]
    fn a_caravan_at_the_ten_llama_limit_rejects_another_member() {
        let mut store = EntityStore::default();
        let head = store.insert(new_mob(
            MobKind::Llama,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(1),
        ));
        mob::state_mut(&mut store, head).unwrap().leash = Some((Uuid::nil(), 1));
        let head_uuid = store.data(head).unwrap().uuid;
        let mut parent = head_uuid;
        for seed in 2..=10 {
            let member = store.insert(new_mob(
                MobKind::Llama,
                Vec3::new(0.5, 64.0, 0.5),
                &mut Rng::new(seed),
            ));
            mob::state_mut(&mut store, member).unwrap().caravan = Some(parent);
            parent = store.data(member).unwrap().uuid;
        }
        let extra = store.insert(new_mob(
            MobKind::Llama,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(11),
        ));

        let extra_pos = store.pos(extra).unwrap();
        assert_eq!(caravan_action(&mut store, extra, extra_pos), None);
        assert_eq!(mob::state(&store, extra).unwrap().caravan, None);
    }

    fn llama_at(store: &mut EntityStore, x: f64, seed: u64) -> crate::EntityKey {
        let mut rng = Rng::new(seed);
        store.insert(new_mob(MobKind::Llama, Vec3::new(x, 64.0, 0.5), &mut rng))
    }

    #[test]
    fn caravan_speed_slows_longer_lines() {
        assert_eq!(caravan_speed(1), CARAVAN_SPEED);
        assert!(caravan_speed(4) < caravan_speed(1));
        assert!(caravan_speed(9) < caravan_speed(4));
        assert_eq!(caravan_speed(20), CARAVAN_MIN_SPEED);
    }

    #[test]
    fn a_long_caravan_walks_slower_than_a_short_one() {
        let mut store = EntityStore::default();
        let head = llama_at(&mut store, 0.5, 1);
        mob::state_mut(&mut store, head).unwrap().leash = Some((Uuid::nil(), 1));
        let mut parent = head;
        let mut parent_uuid = store.data(head).unwrap().uuid;
        for (index, seed) in [2_u64, 3, 4, 5].into_iter().enumerate() {
            let member = llama_at(&mut store, 2.5 + index as f64 * 2.0, seed);
            mob::state_mut(&mut store, member).unwrap().caravan = Some(parent_uuid);
            parent = member;
            parent_uuid = store.data(member).unwrap().uuid;
        }
        let tail = llama_at(&mut store, 12.0, 6);
        mob::state_mut(&mut store, tail).unwrap().caravan = Some(parent_uuid);

        let start = store.pos(tail).unwrap();
        match caravan_action(&mut store, tail, start) {
            Some(CaravanAction::Walk { target, speed }) => {
                assert_eq!(target, store.pos(parent).unwrap());
                assert!(speed < CARAVAN_SPEED, "the long line is slower: {speed}");
                let expected = CARAVAN_SPEED - 4.0 * CARAVAN_SPEED_PER_LINK;
                assert!((speed - expected).abs() < 1e-9, "{speed} != {expected}");
            }
            other => panic!("expected a long-line follow step, got {other:?}"),
        }
    }

    #[test]
    fn a_caravan_follower_keeps_its_gap_instead_of_bumping_its_head() {
        let mut store = EntityStore::default();
        let head = llama_at(&mut store, 0.5, 1);
        let head_uuid = store.data(head).unwrap().uuid;

        let close = llama_at(&mut store, 1.4, 2);
        mob::state_mut(&mut store, close).unwrap().caravan = Some(head_uuid);
        let start = store.pos(close).unwrap();
        match caravan_action(&mut store, close, start) {
            Some(CaravanAction::Walk { target, .. }) => {
                assert!(target.x > start.x, "steps away from the head: {target:?}");
            }
            other => panic!("expected a separation step, got {other:?}"),
        }

        let hold = llama_at(&mut store, 2.0, 3);
        mob::state_mut(&mut store, hold).unwrap().caravan = Some(head_uuid);
        let start = store.pos(hold).unwrap();
        assert_eq!(
            caravan_action(&mut store, hold, start),
            Some(CaravanAction::Hold)
        );

        let far = llama_at(&mut store, 6.5, 4);
        mob::state_mut(&mut store, far).unwrap().caravan = Some(head_uuid);
        let start = store.pos(far).unwrap();
        match caravan_action(&mut store, far, start) {
            Some(CaravanAction::Walk { target, speed }) => {
                assert_eq!(target.x, 0.5);
                assert_eq!(speed, CARAVAN_SPEED);
            }
            other => panic!("expected a follow step, got {other:?}"),
        }
    }

    #[test]
    fn a_caravan_releases_a_head_that_died_or_left_the_region() {
        let mut store = EntityStore::default();
        let head = llama_at(&mut store, 0.5, 1);
        let head_uuid = store.data(head).unwrap().uuid;
        let tail = llama_at(&mut store, 2.0, 2);
        mob::state_mut(&mut store, tail).unwrap().caravan = Some(head_uuid);
        store.remove(head);
        let start = store.pos(tail).unwrap();
        assert_eq!(caravan_action(&mut store, tail, start), None);
        assert_eq!(mob::state(&store, tail).unwrap().caravan, None);

        let mut store = EntityStore::default();
        let head = llama_at(&mut store, 0.5, 1);
        let head_uuid = store.data(head).unwrap().uuid;
        let tail = llama_at(&mut store, 2.0, 2);
        mob::state_mut(&mut store, tail).unwrap().caravan = Some(head_uuid);
        mob::kill(&mut store, head);
        assert!(mob::state(&store, head).unwrap().is_dead());
        let start = store.pos(tail).unwrap();
        assert_eq!(caravan_action(&mut store, tail, start), None);
        assert_eq!(mob::state(&store, tail).unwrap().caravan, None);
    }

    #[test]
    fn a_caravan_rejoins_a_new_line_after_its_head_leaves_the_region() {
        let mut store = EntityStore::default();
        let old_head = llama_at(&mut store, 0.5, 1);
        mob::state_mut(&mut store, old_head).unwrap().leash = Some((Uuid::nil(), 1));
        let old_uuid = store.data(old_head).unwrap().uuid;
        let tail = llama_at(&mut store, 2.0, 2);
        mob::state_mut(&mut store, tail).unwrap().caravan = Some(old_uuid);
        let replacement = llama_at(&mut store, 5.5, 3);
        mob::state_mut(&mut store, replacement).unwrap().leash = Some((Uuid::nil(), 1));
        let replacement_uuid = store.data(replacement).unwrap().uuid;

        // The old head moved to another region, so it is no longer in this store.
        store.remove(old_head);
        let start = store.pos(tail).unwrap();
        match caravan_action(&mut store, tail, start) {
            Some(CaravanAction::Walk { target, speed }) => {
                assert_eq!(target.x, 5.5);
                assert_eq!(speed, CARAVAN_SPEED);
            }
            other => panic!("expected the tail to follow the replacement line, got {other:?}"),
        }
        assert_eq!(
            mob::state(&store, tail).unwrap().caravan,
            Some(replacement_uuid)
        );
    }

    #[test]
    fn a_caravan_tail_reattaches_when_a_middle_member_is_removed() {
        let mut store = EntityStore::default();
        let head = llama_at(&mut store, 0.5, 1);
        let head_uuid = store.data(head).unwrap().uuid;
        mob::state_mut(&mut store, head).unwrap().leash = Some((Uuid::nil(), 1));
        let middle = llama_at(&mut store, 3.5, 2);
        let middle_uuid = store.data(middle).unwrap().uuid;
        mob::state_mut(&mut store, middle).unwrap().caravan = Some(head_uuid);
        let tail = llama_at(&mut store, 6.5, 3);
        mob::state_mut(&mut store, tail).unwrap().caravan = Some(middle_uuid);

        store.remove(middle);
        let start = store.pos(tail).unwrap();
        let action = caravan_action(&mut store, tail, start);

        assert_eq!(mob::state(&store, tail).unwrap().caravan, Some(head_uuid));
        assert!(matches!(
            action,
            Some(CaravanAction::Walk { target, speed })
                if target.x == 0.5 && speed == CARAVAN_SPEED
        ));
    }

    #[test]
    fn a_caravan_never_closes_a_loop() {
        let mut store = EntityStore::default();
        let head = llama_at(&mut store, 0.5, 1);
        let head_uuid = store.data(head).unwrap().uuid;
        let follower = llama_at(&mut store, 3.5, 2);
        let follower_uuid = store.data(follower).unwrap().uuid;
        mob::state_mut(&mut store, follower).unwrap().caravan = Some(head_uuid);

        // Joining its own follower would close a two-llama loop.
        let start = store.pos(head).unwrap();
        assert_eq!(caravan_action(&mut store, head, start), None);
        assert_eq!(mob::state(&store, head).unwrap().caravan, None);

        // A chain already stored as a loop is detected and released.
        mob::state_mut(&mut store, head).unwrap().caravan = Some(follower_uuid);
        assert_eq!(caravan_chain_len(&store, head), CARAVAN_LIMIT);
        assert_eq!(caravan_action(&mut store, head, start), None);
        assert_eq!(mob::state(&store, head).unwrap().caravan, None);
    }

    #[test]
    fn a_caravan_joins_the_nearest_line_whatever_the_store_order() {
        for near_first in [true, false] {
            let mut store = EntityStore::default();
            let (near, far) = if near_first {
                (llama_at(&mut store, 4.5, 1), llama_at(&mut store, 8.0, 2))
            } else {
                let far = llama_at(&mut store, 8.0, 2);
                let near = llama_at(&mut store, 4.5, 1);
                (near, far)
            };
            for key in [near, far] {
                mob::state_mut(&mut store, key).unwrap().leash = Some((Uuid::nil(), 1));
            }
            let expected = store.data(near).unwrap().uuid;
            let joiner = llama_at(&mut store, 0.5, 3);
            let start = store.pos(joiner).unwrap();
            let _ = caravan_action(&mut store, joiner, start);
            assert_eq!(mob::state(&store, joiner).unwrap().caravan, Some(expected));
        }
    }

    #[test]
    fn snakes_and_spiders_shoot_and_bite() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        store.insert(new_mob(
            MobKind::Skeleton,
            Vec3::new(8.5, 64.0, 0.5),
            &mut Rng::new(3),
        ));
        let mut rng = Rng::new(4);
        let player = target(0.5, 0.5);
        let mut arrows = 0;
        for _ in 0..10 {
            for event in tick_with(&mut store, &world, &[player], &mut rng) {
                if let MobEvent::Arrow { damage, .. } = event {
                    assert_eq!(damage, SKELETON_ARROW_DAMAGE);
                    arrows += 1;
                }
            }
        }
        assert_eq!(arrows, 1, "one arrow every cooldown");
    }

    #[test]
    fn a_skeleton_shoots_twice_as_often_on_hard() {
        let world = TestWorld::flat(64);
        let shots = |hard: bool| {
            let mut store = EntityStore::default();
            store.insert(new_mob(
                MobKind::Skeleton,
                Vec3::new(8.5, 64.0, 0.5),
                &mut Rng::new(3),
            ));
            let mut rng = Rng::new(4);
            let rules = MobRules {
                hard,
                ..Default::default()
            };
            let player = target(0.5, 0.5);
            let mut ticks = Vec::new();
            for tick in 0..100 {
                let events = tick_full(&mut store, &world, &[player], &rules, &[], &[], &mut rng);
                if events.iter().any(|e| matches!(e, MobEvent::Arrow { .. })) {
                    ticks.push(tick);
                }
            }
            ticks
        };
        let (normal, hard) = (shots(false), shots(true));
        assert!(normal.len() >= 2 && hard.len() >= 4, "{normal:?} {hard:?}");
        assert!(normal.windows(2).all(|w| w[1] - w[0] == 40), "{normal:?}");
        assert!(hard.windows(2).all(|w| w[1] - w[0] == 20), "{hard:?}");
    }

    /// A flat world with one light level everywhere.
    struct Lit<'a> {
        world: &'a TestWorld,
        level: Option<u8>,
    }

    impl BlockGetter for Lit<'_> {
        fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
            self.world.block_state(x, y, z)
        }

        fn brightness(&self, _x: i32, _y: i32, _z: i32) -> Option<u8> {
            self.level
        }
    }

    #[test]
    fn a_spider_is_neutral_in_bright_light_and_hostile_in_the_dark() {
        let flat = TestWorld::flat(64);
        let hunted = |level: Option<u8>, start_with_target: bool| {
            let world = Lit {
                world: &flat,
                level,
            };
            let mut store = EntityStore::default();
            let spider = store.insert(new_mob(
                MobKind::Spider,
                Vec3::new(8.5, 64.0, 0.5),
                &mut Rng::new(3),
            ));
            let player = target(0.5, 0.5);
            if start_with_target {
                mob::state_mut(&mut store, spider).unwrap().target = Some(player.uuid);
            }
            let mut rng = Rng::new(4);
            let mut targeted = 0;
            for _ in 0..400 {
                tick_with(&mut store, &world, &[player], &mut rng);
                if mob::state(&store, spider).unwrap().target.is_some() {
                    targeted += 1;
                }
            }
            targeted
        };
        assert!(hunted(Some(0), false) > 300, "dark: hunts");
        assert!(hunted(None, false) > 300, "unknown light counts as dark");
        assert!(
            hunted(Some(11), false) > 300,
            "light 11 is still dark enough"
        );
        assert_eq!(
            hunted(Some(12), false),
            0,
            "light 12 never acquires a target"
        );
        let kept = hunted(Some(15), true);
        assert!(
            kept < 300,
            "a bright spider gives up its target eventually: {kept}"
        );
        assert!(kept > 0, "but not on the first tick");
    }

    #[test]
    fn a_creeper_swells_next_to_a_player_and_explodes() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let creeper = store.insert(new_mob(
            MobKind::Creeper,
            Vec3::new(2.0, 64.0, 0.5),
            &mut Rng::new(5),
        ));
        let mut rng = Rng::new(6);
        let player = target(0.5, 0.5);
        let mut exploded = false;
        for _ in 0..60 {
            for event in tick_with(&mut store, &world, &[player], &mut rng) {
                if let MobEvent::Explode { power, .. } = event {
                    assert_eq!(power, CREEPER_POWER);
                    exploded = true;
                }
            }
            if exploded {
                break;
            }
        }
        assert!(exploded, "the creeper exploded");
        assert!(
            !mob::is_mob(&store, creeper),
            "the creeper died in the blast"
        );
    }

    #[test]
    fn endermen_blink_to_walkable_ground() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let enderman = store.insert(new_mob(
            MobKind::Enderman,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(7),
        ));
        let mut rng = Rng::new(8);
        let player = target(30.5, 0.5);
        let mut moved = false;
        for _ in 0..600 {
            tick_with(&mut store, &world, &[player], &mut rng);
            if store
                .pos(enderman)
                .is_some_and(|pos| (pos.x - 0.5).abs() > 2.0)
            {
                moved = true;
                break;
            }
        }
        assert!(moved, "the enderman teleported");
    }

    #[test]
    fn a_cat_in_love_shows_hearts_but_a_parrot_never_does() {
        let world = TestWorld::flat(64);
        let owner = Uuid::from_u128(1);
        let mut store = EntityStore::default();
        let cat = store.insert(new_mob(
            MobKind::Cat,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(1),
        ));
        let parrot = store.insert(new_mob(
            MobKind::Parrot,
            Vec3::new(2.5, 64.0, 0.5),
            &mut Rng::new(2),
        ));
        for key in [cat, parrot] {
            let state = mob::state_mut(&mut store, key).unwrap();
            state.owner = Some(owner);
            state.sitting = false;
        }
        assert_eq!(
            crate::breed::feed(&mut store, cat, "minecraft:cod", owner),
            crate::breed::Fed::Love
        );
        assert_eq!(
            crate::breed::feed(&mut store, parrot, "minecraft:wheat_seeds", owner),
            crate::breed::Fed::No,
            "parrots are tameable but not breedable"
        );

        let player = Target {
            uuid: owner,
            pos: Vec3::new(0.5, 64.0, 0.5),
        };
        let mut rng = Rng::new(3);
        let mut cat_hearts = 0;
        let mut parrot_hearts = 0;
        for _ in 0..11 {
            for event in tick_with(&mut store, &world, &[player], &mut rng) {
                if let MobEvent::Status { key, status } = event
                    && status == 18
                {
                    if key == cat {
                        cat_hearts += 1;
                    } else if key == parrot {
                        parrot_hearts += 1;
                    }
                }
            }
        }
        assert_eq!(cat_hearts, 1, "one heart event per ten love ticks");
        assert_eq!(parrot_hearts, 0, "a parrot cannot fall in love");
        assert_eq!(mob::state(&store, parrot).unwrap().love, 0);
    }

    fn one_mob(kind: MobKind, x: f64) -> (EntityStore, crate::EntityKey) {
        let mut store = EntityStore::default();
        let key = store.insert(new_mob(kind, Vec3::new(x, 64.0, 0.5), &mut Rng::new(5)));
        (store, key)
    }

    #[test]
    fn a_lit_creeper_keeps_swelling_until_the_target_is_seven_blocks_away() {
        let world = TestWorld::flat(64);
        let (mut store, creeper) = one_mob(MobKind::Creeper, 0.5);
        mob::state_mut(&mut store, creeper).unwrap().no_ai = true;
        let mut rng = Rng::new(6);
        // Ignite inside three blocks...
        tick_with(&mut store, &world, &[target(2.5, 0.5)], &mut rng);
        assert_eq!(mob::state(&store, creeper).unwrap().swell, 1);
        // ...then the target backs off to five blocks: the fuse keeps burning.
        tick_with(&mut store, &world, &[target(5.5, 0.5)], &mut rng);
        assert_eq!(mob::state(&store, creeper).unwrap().swell, 2);
        // Beyond seven blocks it winds down instead.
        tick_with(&mut store, &world, &[target(9.0, 0.5)], &mut rng);
        assert_eq!(mob::state(&store, creeper).unwrap().swell, 1);
    }

    #[test]
    fn an_unlit_creeper_does_not_ignite_at_five_blocks() {
        let world = TestWorld::flat(64);
        let (mut store, creeper) = one_mob(MobKind::Creeper, 0.5);
        mob::state_mut(&mut store, creeper).unwrap().no_ai = true;
        tick_with(&mut store, &world, &[target(5.5, 0.5)], &mut Rng::new(6));
        assert_eq!(mob::state(&store, creeper).unwrap().swell, 0);
    }

    #[test]
    fn an_enderman_ignores_players_until_provoked() {
        let world = TestWorld::flat(64);
        let (mut store, enderman) = one_mob(MobKind::Enderman, 0.5);
        let player = target(1.5, 0.5);
        let mut rng = Rng::new(9);
        for _ in 0..30 {
            let events = tick_with(&mut store, &world, &[player], &mut rng);
            assert!(!events.iter().any(|e| matches!(e, MobEvent::Melee { .. })));
        }
        assert_eq!(mob::state(&store, enderman).unwrap().target, None);

        assert!(mob::retaliate(&mut store, enderman, player.uuid));
        assert_eq!(
            mob::state(&store, enderman).unwrap().anger,
            mob::ENDERMAN_ANGER_TICKS
        );
        let mut struck = false;
        for _ in 0..30 {
            struck |= tick_with(&mut store, &world, &[player], &mut rng)
                .iter()
                .any(|e| matches!(e, MobEvent::Melee { .. }));
        }
        assert!(struck, "a provoked enderman attacks");
    }

    #[test]
    fn an_enderman_emits_take_and_teleport_events_through_the_ai_tick() {
        let mut world = TestWorld::flat(64);
        // A ring of holdable blocks at the enderman's feet: leave the cell it stands in
        // empty so the line-of-sight check reaches every candidate.
        let grass = ferrite_data::blocks::default_state("minecraft:grass_block").unwrap();
        for x in -2..=2 {
            for z in -2..=2 {
                if x != 0 || z != 0 {
                    world.blocks.insert((x, 64, z), grass);
                }
            }
        }
        let mut store = EntityStore::default();
        let key = store.insert(new_mob(
            MobKind::Enderman,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(4),
        ));
        let rules = MobRules {
            griefing: true,
            ..Default::default()
        };
        let player = target(8.5, 0.5);
        let mut rng = Rng::new(9);
        let mut took = false;
        let mut teleported = false;
        for _ in 0..4000 {
            for event in tick_full(&mut store, &world, &[player], &rules, &[], &[], &mut rng) {
                match event {
                    MobEvent::TakeBlock { key: at_key, at } => {
                        assert_eq!(at_key, key);
                        assert_eq!(at[1], 64);
                        assert!(at[0].abs() <= 2 && at[2].abs() <= 2, "{at:?}");
                        took = true;
                    }
                    MobEvent::Teleport { from, to } => {
                        assert_ne!(from, to);
                        teleported = true;
                    }
                    _ => {}
                }
            }
        }
        assert!(took, "the AI must report the block it wants to take");
        assert!(
            teleported,
            "the AI must report a blink for the region to animate"
        );
    }

    #[test]
    fn a_provoked_enderman_calms_down_when_its_anger_runs_out() {
        let world = TestWorld::flat(64);
        let (mut store, enderman) = one_mob(MobKind::Enderman, 0.5);
        let player = target(30.5, 0.5);
        mob::retaliate(&mut store, enderman, player.uuid);
        mob::state_mut(&mut store, enderman).unwrap().anger = 2;
        let mut rng = Rng::new(9);
        for _ in 0..4 {
            tick_with(&mut store, &world, &[player], &mut rng);
        }
        let state = mob::state(&store, enderman).unwrap();
        assert_eq!((state.anger, state.target), (0, None));
    }

    #[test]
    fn a_hurt_zombie_keeps_chasing_its_attacker_over_a_nearer_player() {
        let world = TestWorld::flat(64);
        let (mut store, zombie) = one_mob(MobKind::Zombie, 0.5);
        mob::state_mut(&mut store, zombie).unwrap().no_ai = true;
        let attacker = Target {
            uuid: Uuid::from_u128(77),
            pos: Vec3::new(20.5, 64.0, 0.5),
        };
        let bystander = target(4.5, 0.5);
        mob::retaliate(&mut store, zombie, attacker.uuid);
        tick_with(&mut store, &world, &[attacker, bystander], &mut Rng::new(3));
        assert_eq!(
            mob::state(&store, zombie).unwrap().target,
            Some(attacker.uuid)
        );
        // Once the attacker is out of follow range the nearest player is chosen again.
        let far = Target {
            pos: Vec3::new(300.5, 64.0, 0.5),
            ..attacker
        };
        tick_with(&mut store, &world, &[far, bystander], &mut Rng::new(3));
        assert_eq!(
            mob::state(&store, zombie).unwrap().target,
            Some(bystander.uuid)
        );
    }

    #[test]
    fn a_skeleton_strafes_inside_its_firing_band_and_flips_sides() {
        let world = TestWorld::flat(64);
        let (mut store, skeleton) = one_mob(MobKind::Skeleton, 0.5);
        let player = target(6.5, 0.5);
        let mut rng = Rng::new(4);
        let mut z_at = [0.5; 3];
        for tick in 1..=55 {
            tick_with(&mut store, &world, &[player], &mut rng);
            match tick {
                25 => z_at[1] = store.pos(skeleton).unwrap().z,
                55 => z_at[2] = store.pos(skeleton).unwrap().z,
                _ => {}
            }
        }
        let (first, second) = (z_at[1] - z_at[0], z_at[2] - z_at[1]);
        assert!(first.abs() > 0.2, "strafed sideways: {first}");
        assert!(first * second < 0.0, "flipped sides: {first} then {second}");
        let x = store.pos(skeleton).unwrap().x;
        assert!(
            (x - 0.5).abs() < 3.0,
            "strafing does not close the distance"
        );
    }

    #[test]
    fn an_adult_chicken_lays_an_egg_but_a_baby_does_not() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let chicken = store.insert(new_mob(
            MobKind::Chicken,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(1),
        ));
        mob::state_mut(&mut store, chicken).unwrap().egg_time = 2;
        let mut rng = Rng::new(2);
        let mut laid = 0;
        for _ in 0..2 {
            laid += tick_with(&mut store, &world, &[], &mut rng)
                .iter()
                .filter(|event| matches!(event, MobEvent::LayEgg { .. }))
                .count();
        }
        assert_eq!(laid, 1, "one egg when the timer runs out");
        assert!(
            mob::state(&store, chicken).unwrap().egg_time >= mob::CHICKEN_EGG_MIN,
            "the timer restarts for the next egg"
        );

        let baby = store.insert(new_mob(
            MobKind::Chicken,
            Vec3::new(3.5, 64.0, 0.5),
            &mut Rng::new(3),
        ));
        {
            let state = mob::state_mut(&mut store, baby).unwrap();
            state.age = -6000;
            state.egg_time = 1;
        }
        let events = tick_with(&mut store, &world, &[], &mut rng);
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, MobEvent::LayEgg { .. })),
            "a baby chicken never lays"
        );
    }

    #[test]
    fn a_baby_zombie_chases_faster_than_an_adult() {
        let mut store = EntityStore::default();
        let adult = store.insert(new_mob(
            MobKind::Zombie,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(1),
        ));
        let baby = store.insert(new_mob(
            MobKind::Zombie,
            Vec3::new(0.5, 64.0, 2.5),
            &mut Rng::new(2),
        ));
        mob::state_mut(&mut store, baby).unwrap().age = -6000;
        assert_eq!(chase_speed(&store, adult), CHASE_ACCELERATION);
        assert!(
            (chase_speed(&store, baby) - CHASE_ACCELERATION * BABY_ZOMBIE_SPEED).abs() < 1e-9,
            "the baby gets the vanilla speed modifier"
        );
    }

    #[test]
    fn skeleton_arrows_spread_less_on_hard() {
        let from = Vec3::new(0.5, 64.0, 0.5);
        let to = Vec3::new(20.5, 65.62, 0.5);
        let aim = (to - from).normalize();
        let mean_deviation = |inaccuracy: f64, seed: u64| {
            let mut rng = Rng::new(seed);
            let mut total = 0.0;
            let runs = 200;
            for _ in 0..runs {
                total += (arrow_direction(from, to, inaccuracy, &mut rng) - aim).length();
            }
            total / f64::from(runs)
        };
        let normal = mean_deviation(SKELETON_INACCURACY, 7);
        let hard = mean_deviation(SKELETON_INACCURACY_HARD, 8);
        assert!(normal > 0.001, "normal shots are not perfect: {normal}");
        assert!(hard > 0.0, "hard shots still spread a little: {hard}");
        assert!(
            hard < normal * 0.6,
            "hard is more accurate: normal {normal}, hard {hard}"
        );
    }

    #[test]
    fn a_baby_animal_trails_an_adult_of_its_kind_but_not_when_close() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let adult = store.insert(new_mob(
            MobKind::Cow,
            Vec3::new(6.5, 64.0, 0.5),
            &mut Rng::new(1),
        ));
        let baby = store.insert(new_mob(
            MobKind::Cow,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(2),
        ));
        mob::state_mut(&mut store, baby).unwrap().age = -6000;
        for key in [adult, baby] {
            mob::state_mut(&mut store, key).unwrap().moving = false;
            mob::state_mut(&mut store, key).unwrap().wander_timer = 1_000;
        }
        let before = store.pos(baby).unwrap();
        let mut rng = Rng::new(3);
        for _ in 0..40 {
            tick_with(&mut store, &world, &[], &mut rng);
        }
        let after = store.pos(baby).unwrap();
        assert!(after.x > before.x + 1.0, "the calf walked toward the cow");
        assert!(
            follow_parent(&store, baby, Vec3::new(5.5, 64.0, 0.5)).is_none(),
            "within three blocks it stops"
        );
        assert!(
            follow_parent(&store, adult, Vec3::new(0.0, 64.0, 0.0)).is_none(),
            "adults never trail"
        );
    }

    #[test]
    fn skeleton_without_bow_falls_back_to_melee() {
        let world = TestWorld::flat(64);
        let (mut store, key) = one_mob(MobKind::Skeleton, 0.5);
        assert!(mob::set_equipment(
            &mut store,
            key,
            mob::MAIN_HAND as usize,
            0
        ));
        let targets = [Target {
            uuid: Uuid::from_u128(1),
            pos: Vec3::new(1.5, 64.0, 0.5),
        }];
        let mut rng = Rng::new(4);
        let events = tick_with(&mut store, &world, &targets, &mut rng);
        assert!(
            events.iter().any(|e| matches!(e, MobEvent::Melee { .. })),
            "unarmed skeleton melees"
        );
        assert!(
            !events.iter().any(|e| matches!(e, MobEvent::Arrow { .. })),
            "unarmed skeleton never fires an arrow"
        );
    }

    #[test]
    fn chicken_jockey_never_lays_eggs() {
        let world = TestWorld::flat(64);
        let (mut store, key) = one_mob(MobKind::Chicken, 0.5);
        let state = mob::state_mut(&mut store, key).unwrap();
        state.egg_time = 1;
        state.is_chicken_jockey = true;
        let mut rng = Rng::new(5);
        let events = tick_with(&mut store, &world, &[], &mut rng);
        assert!(!events.iter().any(|e| matches!(e, MobEvent::LayEgg { .. })));
    }

    #[test]
    fn submerged_zombie_emits_drowned_convert_at_600_ticks() {
        struct WaterWorld;
        impl BlockGetter for WaterWorld {
            fn block_state(&self, _x: i32, y: i32, _z: i32) -> Option<u16> {
                ferrite_data::blocks::default_state(if y < 64 {
                    "stone"
                } else if y < 66 {
                    "water"
                } else {
                    "air"
                })
            }
        }
        let (mut store, key) = one_mob(MobKind::Zombie, 0.5);
        mob::state_mut(&mut store, key).unwrap().in_water_time = DROWNED_CONVERSION_TICKS - 1;
        let mut rng = Rng::new(6);
        let events = tick_with(&mut store, &WaterWorld, &[], &mut rng);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, MobEvent::DrownedConvert { key: k } if *k == key))
        );
    }

    #[test]
    fn enderman_freezes_horizontal_chase_while_stared_at() {
        let world = TestWorld::flat(64);
        let (mut store, key) = one_mob(MobKind::Enderman, 0.5);
        let state = mob::state_mut(&mut store, key).unwrap();
        state.anger = 400;
        state.stare_time = 2;
        let before = store.pos(key).unwrap();
        let targets = [Target {
            uuid: Uuid::from_u128(1),
            pos: Vec3::new(10.5, 64.0, 0.5),
        }];
        let mut rng = Rng::new(7);
        tick_with(&mut store, &world, &targets, &mut rng);
        let after = store.pos(key).unwrap();
        assert!((after.x - before.x).abs() < 1e-6);
    }

    #[test]
    fn enderman_freezes_active_target_when_stared_at_via_gaze_and_stays_in_dark_cave() {
        let world = TestWorld::flat(64);
        let (mut store, key) = one_mob(MobKind::Enderman, 0.5);
        let player_id = Uuid::from_u128(9);
        let state = mob::state_mut(&mut store, key).unwrap();
        state.anger = 400;
        state.target = Some(player_id);
        state.stare_time = 0;

        let mob_eye = Vec3::new(
            0.5,
            64.0 + f64::from(MobKind::Enderman.props().eye_height),
            0.5,
        );
        let player_pos = Vec3::new(8.5, 64.0, 0.5);
        let player_eye = Vec3::new(player_pos.x, player_pos.y + 1.62, player_pos.z);
        let mut rules = MobRules::default();
        rules.gazes.insert(
            player_id,
            crate::enderman::Gaze {
                look: (mob_eye - player_eye).normalize(),
                pumpkin: false,
            },
        );
        let targets = [Target {
            uuid: player_id,
            pos: player_pos,
        }];
        let before = store.pos(key).unwrap();
        let mut rng = Rng::new(11);
        for _ in 0..10 {
            tick_full(&mut store, &world, &targets, &rules, &[], &[], &mut rng);
        }
        let after = store.pos(key).unwrap();
        assert!(
            (after.x - before.x).abs() < 1e-6,
            "active stared-at enderman stays frozen horizontally: before={before:?} after={after:?}"
        );

        struct DarkCaveWorld;
        impl BlockGetter for DarkCaveWorld {
            fn block_state(&self, _x: i32, y: i32, _z: i32) -> Option<u16> {
                ferrite_data::blocks::default_state(if !(64..70).contains(&y) {
                    "stone"
                } else {
                    "air"
                })
            }
            fn brightness(&self, _x: i32, _y: i32, _z: i32) -> Option<u8> {
                Some(0)
            }
        }
        let (mut cave_store, _cave_key) = one_mob(MobKind::Enderman, 0.5);
        let dark_rules = MobRules {
            monsters_burn: true,
            bright_outside: Some(false),
            ..Default::default()
        };
        let distant = [Target {
            uuid: Uuid::from_u128(10),
            pos: Vec3::new(60.5, 64.0, 0.5),
        }];
        let mut cave_rng = Rng::new(12);
        for _ in 0..200 {
            let events = tick_full(
                &mut cave_store,
                &DarkCaveWorld,
                &distant,
                &dark_rules,
                &[],
                &[],
                &mut cave_rng,
            );
            assert!(
                !events
                    .iter()
                    .any(|e| matches!(e, MobEvent::Teleport { .. })),
                "enderman must not daylight-teleport inside a dark cave"
            );
        }
    }

    #[test]
    fn spider_climbs_elevated_platform_cannot_hit_from_below_and_stops_attacking_with_rider() {
        struct PillarWorld;
        impl BlockGetter for PillarWorld {
            fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
                if y < 64
                    || ((2..=5).contains(&x) && (-1..=1).contains(&z) && (64..=66).contains(&y))
                {
                    ferrite_data::blocks::default_state("stone")
                } else {
                    ferrite_data::blocks::default_state("air")
                }
            }
        }

        let (mut store, spider) = one_mob(MobKind::Spider, 0.5);
        let targets = [Target {
            uuid: Uuid::from_u128(21),
            pos: Vec3::new(4.5, 67.0, 0.5),
        }];
        let mut rng = Rng::new(13);
        let mut hit_before_climb = false;
        let mut peak_y = 64.0_f64;
        for _ in 0..80 {
            let events = tick_with(&mut store, &PillarWorld, &targets, &mut rng);
            let pos = store.pos(spider).unwrap();
            peak_y = peak_y.max(pos.y);
            if pos.y < 65.5 && events.iter().any(|e| matches!(e, MobEvent::Melee { .. })) {
                hit_before_climb = true;
            }
        }
        assert!(
            !hit_before_climb,
            "spider on the ground cannot melee-hit a player 3 blocks higher"
        );
        assert!(
            peak_y >= 66.9,
            "spider climbed the pillar toward elevated target (peak_y={peak_y})"
        );

        let flat = TestWorld::flat(64);
        let (mut ridden_store, ridden) = one_mob(MobKind::Spider, 0.5);
        mob::state_mut(&mut ridden_store, ridden).unwrap().has_rider = true;
        let close = [Target {
            uuid: Uuid::from_u128(22),
            pos: Vec3::new(1.5, 64.0, 0.5),
        }];
        let mut ridden_rng = Rng::new(14);
        for _ in 0..40 {
            let events = tick_with(&mut ridden_store, &flat, &close, &mut ridden_rng);
            assert!(
                !events.iter().any(|e| matches!(e, MobEvent::Melee { .. })),
                "spider with rider disables SpiderAttackGoal"
            );
        }
    }

    #[test]
    fn creeper_chases_distant_target_and_does_not_swell_through_wall() {
        let flat = TestWorld::flat(64);
        let (mut store, _creeper) = one_mob(MobKind::Creeper, 0.5);
        let targets = [Target {
            uuid: Uuid::from_u128(31),
            pos: Vec3::new(8.5, 64.0, 0.5),
        }];
        let mut rng = Rng::new(15);
        let mut exploded = false;
        for _ in 0..160 {
            let events = tick_with(&mut store, &flat, &targets, &mut rng);
            if events.iter().any(|e| matches!(e, MobEvent::Explode { .. })) {
                exploded = true;
                break;
            }
        }
        assert!(
            exploded,
            "creeper chased target from 8 blocks away and detonated"
        );

        struct WallWorld;
        impl BlockGetter for WallWorld {
            fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
                if y < 64 || (x == 2 && (-2..=2).contains(&z) && (64..=66).contains(&y)) {
                    ferrite_data::blocks::default_state("stone")
                } else {
                    ferrite_data::blocks::default_state("air")
                }
            }
        }
        let (mut wall_store, wall_creeper) = one_mob(MobKind::Creeper, 1.5);
        let behind_wall = [Target {
            uuid: Uuid::from_u128(32),
            pos: Vec3::new(3.5, 64.0, 0.5),
        }];
        let mut wall_rng = Rng::new(16);
        for _ in 0..10 {
            tick_with(&mut wall_store, &WallWorld, &behind_wall, &mut wall_rng);
            assert_eq!(
                mob::state(&wall_store, wall_creeper).unwrap().swell,
                0,
                "creeper must not swell when LOS is blocked by a solid wall"
            );
        }
    }

    #[test]
    fn skeleton_flees_wolf_checks_los_chases_far_target_and_seeks_shade() {
        let flat = TestWorld::flat(64);
        let mut wolf_store = EntityStore::default();
        let mut rng = Rng::new(17);
        let skel = wolf_store.insert(new_mob(
            MobKind::Skeleton,
            Vec3::new(3.5, 64.0, 0.5),
            &mut rng,
        ));
        let wolf = wolf_store.insert(new_mob(MobKind::Wolf, Vec3::new(1.5, 64.0, 0.5), &mut rng));
        mob::state_mut(&mut wolf_store, wolf).unwrap().no_ai = true;
        let distant = [Target {
            uuid: Uuid::from_u128(41),
            pos: Vec3::new(60.5, 64.0, 0.5),
        }];
        for _ in 0..30 {
            tick_with(&mut wolf_store, &flat, &distant, &mut rng);
        }
        assert!(
            wolf_store.pos(skel).unwrap().x > 5.0,
            "skeleton fled from nearby wolf: {:?}",
            wolf_store.pos(skel)
        );

        struct WallWorld;
        impl BlockGetter for WallWorld {
            fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
                if y < 64 || (x == 3 && (-1..=1).contains(&z) && (64..=66).contains(&y)) {
                    ferrite_data::blocks::default_state("stone")
                } else {
                    ferrite_data::blocks::default_state("air")
                }
            }
        }
        let (mut wall_store, wall_skel) = one_mob(MobKind::Skeleton, 1.5);
        let hidden = [Target {
            uuid: Uuid::from_u128(42),
            pos: Vec3::new(5.5, 64.0, 0.5),
        }];
        let mut wall_rng = Rng::new(18);
        for _ in 0..5 {
            let events = tick_with(&mut wall_store, &WallWorld, &hidden, &mut wall_rng);
            assert!(
                !events.iter().any(|e| matches!(e, MobEvent::Arrow { .. })),
                "skeleton must not shoot while LOS is blocked by stone wall"
            );
        }
        assert!(
            wall_store.pos(wall_skel).unwrap().z.abs() > 0.1
                || (wall_store.pos(wall_skel).unwrap().x - 1.5).abs() > 0.1,
            "skeleton moves to regain LOS instead of standing still"
        );

        let (mut far_store, far_skel) = one_mob(MobKind::Skeleton, 0.5);
        let far_target = [Target {
            uuid: Uuid::from_u128(43),
            pos: Vec3::new(16.1, 64.0, 0.5),
        }];
        let mut far_rng = Rng::new(19);
        for _ in 0..30 {
            tick_with(&mut far_store, &flat, &far_target, &mut far_rng);
        }
        assert!(
            far_store.pos(far_skel).unwrap().x > 1.5,
            "skeleton chases target between 15-block bow range and 16-block follow range: {:?}",
            far_store.pos(far_skel)
        );

        struct DayShadeWorld;
        impl BlockGetter for DayShadeWorld {
            fn block_state(&self, x: i32, y: i32, _z: i32) -> Option<u16> {
                if y < 64 || (y == 70 && x >= 4) {
                    ferrite_data::blocks::default_state("stone")
                } else {
                    ferrite_data::blocks::default_state("air")
                }
            }
            fn brightness(&self, _x: i32, _y: i32, _z: i32) -> Option<u8> {
                Some(15)
            }
        }
        let (mut shade_store, shade_skel) = one_mob(MobKind::Skeleton, 0.5);
        let day_rules = MobRules {
            monsters_burn: true,
            bright_outside: Some(true),
            ..Default::default()
        };
        let mut shade_rng = Rng::new(20);
        let mut sought_shade = false;
        for _ in 0..40 {
            tick_full(
                &mut shade_store,
                &DayShadeWorld,
                &distant,
                &day_rules,
                &[],
                &[],
                &mut shade_rng,
            );
            if mob::state(&shade_store, shade_skel)
                .unwrap()
                .shade_pos
                .is_some()
            {
                sought_shade = true;
                break;
            }
        }
        assert!(
            sought_shade,
            "helmetless idle skeleton seeks shade in bright daylight"
        );
    }

    #[test]
    fn float_goal_keeps_floaters_afloat_while_zombies_and_skeletons_sink() {
        struct DeepWaterWorld;
        impl BlockGetter for DeepWaterWorld {
            fn block_state(&self, _x: i32, y: i32, _z: i32) -> Option<u16> {
                ferrite_data::blocks::default_state(if y < 64 {
                    "stone"
                } else if y <= 67 {
                    "water"
                } else {
                    "air"
                })
            }
        }

        let mut store = EntityStore::default();
        let mut rng = Rng::new(21);
        let cow = store.insert(new_mob(MobKind::Cow, Vec3::new(0.5, 65.5, 0.5), &mut rng));
        let spider = store.insert(new_mob(
            MobKind::Spider,
            Vec3::new(3.5, 65.5, 0.5),
            &mut rng,
        ));
        let zombie = store.insert(new_mob(
            MobKind::Zombie,
            Vec3::new(6.5, 65.5, 0.5),
            &mut rng,
        ));
        let skel = store.insert(new_mob(
            MobKind::Skeleton,
            Vec3::new(9.5, 65.5, 0.5),
            &mut rng,
        ));

        let distant = [Target {
            uuid: Uuid::from_u128(51),
            pos: Vec3::new(60.5, 68.0, 0.5),
        }];
        for _ in 0..60 {
            tick_with(&mut store, &DeepWaterWorld, &distant, &mut rng);
        }
        assert!(
            store.pos(cow).unwrap().y >= 66.2,
            "cow floats in deep water: {:?}",
            store.pos(cow)
        );
        assert!(
            store.pos(spider).unwrap().y >= 66.2,
            "spider floats in deep water: {:?}",
            store.pos(spider)
        );
        assert!(
            store.pos(zombie).unwrap().y < 64.5,
            "zombie sinks to seabed: {:?}",
            store.pos(zombie)
        );
        assert!(
            store.pos(skel).unwrap().y < 64.5,
            "skeleton sinks to seabed: {:?}",
            store.pos(skel)
        );
    }

    #[test]
    fn tempt_goal_draws_animals_for_food_and_carrot_on_a_stick() {
        let world = TestWorld::flat(64);
        for (kind, item, offhand, should_follow) in [
            (MobKind::Cow, "wheat", false, true),
            (MobKind::Pig, "carrot_on_a_stick", true, true),
            (MobKind::Chicken, "wheat_seeds", false, true),
            (MobKind::Sheep, "wheat", false, true),
            (MobKind::Cow, "stone", false, false),
        ] {
            let (mut store, mob_key) = one_mob(kind, 0.5);
            let player_id = Uuid::from_u128(61);
            let mut rules = MobRules::default();
            rules.held_items.insert(
                player_id,
                if offhand {
                    [None, Some(item)]
                } else {
                    [Some(item), None]
                },
            );
            let player = [Target {
                uuid: player_id,
                pos: Vec3::new(6.5, 64.0, 0.5),
            }];
            let mut rng = Rng::new(22);
            for _ in 0..40 {
                tick_full(&mut store, &world, &player, &rules, &[], &[], &mut rng);
            }
            let x = store.pos(mob_key).unwrap().x;
            if should_follow {
                assert!(
                    x > 2.0 && x <= 6.5,
                    "{kind:?} should walk toward player holding {item}: x={x}"
                );
            } else {
                assert!(x < 2.0, "{kind:?} should not be tempted by {item}: x={x}");
            }
        }
    }

    #[test]
    fn sheep_eat_goal_is_preempted_by_breed_and_animals_panic_on_fire() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let mut rng = Rng::new(23);
        let s1 = store.insert(new_mob(MobKind::Sheep, Vec3::new(0.5, 64.0, 0.5), &mut rng));
        let s2 = store.insert(new_mob(MobKind::Sheep, Vec3::new(5.5, 64.0, 0.5), &mut rng));
        mob::state_mut(&mut store, s1).unwrap().eat_timer = 20;
        mob::state_mut(&mut store, s1).unwrap().love = 100;
        mob::state_mut(&mut store, s2).unwrap().love = 100;
        tick_with(&mut store, &world, &[], &mut rng);
        assert_eq!(
            mob::state(&store, s1).unwrap().eat_timer,
            0,
            "BreedGoal pre-empts Sheep EatBlockGoal"
        );

        let (mut p_store, cow) = one_mob(MobKind::Cow, 0.5);
        mob::state_mut(&mut p_store, cow).unwrap().fire = 20;
        let before_cow = p_store.pos(cow).unwrap();
        for _ in 0..20 {
            tick_with(&mut p_store, &world, &[], &mut rng);
        }
        let after_cow = p_store.pos(cow).unwrap();
        assert!(
            (after_cow.x - before_cow.x).hypot(after_cow.z - before_cow.z) > 1.0,
            "burning cow runs at PanicGoal speed (2.0x): before={before_cow:?} after={after_cow:?}"
        );
    }

    #[test]
    fn idle_hostiles_look_at_player_and_wander_avoids_water() {
        let world = TestWorld::flat(64);
        let (mut store, zombie) = one_mob(MobKind::Zombie, 0.5);
        let player_id = Uuid::from_u128(71);
        let mut rules = MobRules::default();
        rules.untargetable.insert(player_id);
        let targets = [Target {
            uuid: player_id,
            pos: Vec3::new(0.5, 64.0, 6.5),
        }];
        let mut rng = Rng::new(24);
        tick_full(&mut store, &world, &targets, &rules, &[], &[], &mut rng);
        let yaw = store.data(zombie).unwrap().yaw;
        assert!(
            yaw.abs() < 15.0,
            "idle zombie looks toward south player (yaw ~0): yaw={yaw}"
        );

        struct RingWaterWorld;
        impl BlockGetter for RingWaterWorld {
            fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
                if y < 64 {
                    ferrite_data::blocks::default_state("stone")
                } else if y == 64 && x == 1 && z == 0 {
                    ferrite_data::blocks::default_state("water")
                } else {
                    ferrite_data::blocks::default_state("air")
                }
            }
        }
        assert!(ahead_is_fluid(
            &RingWaterWorld,
            Vec3::new(0.5, 64.0, 0.5),
            -90.0
        ));
        assert!(!ahead_is_fluid(
            &RingWaterWorld,
            Vec3::new(0.5, 64.0, 0.5),
            90.0
        ));
        let mut rng2 = Rng::new(25);
        for _ in 0..20 {
            let yaw =
                pick_dry_wander_yaw(&RingWaterWorld, Vec3::new(0.5, 64.0, 0.5), false, &mut rng2);
            assert!(
                !ahead_is_fluid(&RingWaterWorld, Vec3::new(0.5, 64.0, 0.5), yaw),
                "dry wander yaw {yaw} must not step straight into water"
            );
        }
    }

    #[test]
    fn villager_opens_and_closes_wooden_doors_and_waits_for_neighbor_villager() {
        use std::collections::HashMap;
        struct DoorWorld {
            blocks: HashMap<[i32; 3], u16>,
        }
        impl DoorWorld {
            fn set_door(&mut self, x: i32, y: i32, z: i32, open: bool) {
                let state_str = if open { "true" } else { "false" };
                let lo = ferrite_data::blocks::state_id(
                    "minecraft:oak_door",
                    &[
                        ("facing", "east"),
                        ("hinge", "left"),
                        ("half", "lower"),
                        ("open", state_str),
                    ],
                )
                .unwrap();
                let hi = ferrite_data::blocks::state_id(
                    "minecraft:oak_door",
                    &[
                        ("facing", "east"),
                        ("hinge", "left"),
                        ("half", "upper"),
                        ("open", state_str),
                    ],
                )
                .unwrap();
                self.blocks.insert([x, y, z], lo);
                self.blocks.insert([x, y + 1, z], hi);
            }
            fn is_open(&self, x: i32, y: i32, z: i32) -> bool {
                self.blocks
                    .get(&[x, y, z])
                    .is_some_and(|&s| path::is_open_door(s))
            }
        }
        impl BlockGetter for DoorWorld {
            fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
                if let Some(&state) = self.blocks.get(&[x, y, z]) {
                    return Some(state);
                }
                if y < 64 {
                    return ferrite_data::blocks::default_state("stone");
                }
                if x == 3 && z != 0 && (64..=66).contains(&y) {
                    return ferrite_data::blocks::default_state("stone");
                }
                ferrite_data::blocks::default_state("air")
            }
        }

        let mut world = DoorWorld {
            blocks: HashMap::new(),
        };
        world.set_door(3, 64, 0, false);

        let mut store = EntityStore::default();
        let mut rng = Rng::new(77);
        let mut record = new_mob(MobKind::Villager, Vec3::new(1.5, 64.0, 0.5), &mut rng);
        if let Some(v) = record.data.villager.as_mut() {
            v.activity = crate::villager::Activity::Work;
            v.destination = Some([8, 64, 0]);
        }
        let v1 = store.insert(record);
        let rules = MobRules::default();
        let mut opened = false;
        let mut closed = false;

        for _ in 0..80 {
            let events = tick_full(&mut store, &world, &[], &rules, &[], &[], &mut rng);
            for ev in events {
                if let MobEvent::SetDoor { at, open } = ev {
                    world.set_door(at[0], at[1], at[2], open);
                    if open {
                        opened = true;
                    } else if store.pos(v1).unwrap().x > 5.5 {
                        closed = true;
                    }
                }
            }
            if closed {
                break;
            }
        }
        assert!(opened, "villager must open closed oak door on path");
        assert!(
            store.pos(v1).unwrap().x > 5.5,
            "villager walked through opened door: {:?}",
            store.pos(v1)
        );
        assert!(closed, "villager must close the door once >2 blocks past");
        assert!(!world.is_open(3, 64, 0));

        // Reset with a second villager near the doorway: the walker keeps it open for v2.
        world.set_door(3, 64, 0, true);
        let mut v1_rec = new_mob(MobKind::Villager, Vec3::new(5.8, 64.0, 0.5), &mut rng);
        v1_rec.data.mob.as_mut().unwrap().opened_door = Some([3, 64, 0]);
        if let Some(v) = v1_rec.data.villager.as_mut() {
            v.activity = crate::villager::Activity::Work;
            v.destination = Some([9, 64, 0]);
        }
        let v2_rec = new_mob(MobKind::Villager, Vec3::new(3.8, 64.0, 0.5), &mut rng);
        let mut shared_store = EntityStore::default();
        let k1 = shared_store.insert(v1_rec);
        let _k2 = shared_store.insert(v2_rec);
        let events = tick_full(&mut shared_store, &world, &[], &rules, &[], &[], &mut rng);
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, MobEvent::SetDoor { open: false, .. })),
            "must not slam door while another villager is within 2 blocks"
        );
        assert_eq!(
            mob::state(&shared_store, k1).unwrap().opened_door,
            Some([3, 64, 0])
        );
    }

    #[test]
    fn snow_golem_throws_snowballs_at_hostiles_and_emits_snow_trail() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let mut rng = Rng::new(101);
        let golem = store.insert(mob::new_mob(
            MobKind::SnowGolem,
            Vec3::new(0.5, 64.0, 0.5),
            &mut rng,
        ));
        let _zombie = store.insert(mob::new_mob(
            MobKind::Zombie,
            Vec3::new(6.5, 64.0, 0.5),
            &mut rng,
        ));
        store.flags_mut(golem).unwrap().set(Flags::ON_GROUND, true);

        let mut rules = MobRules {
            griefing: true,
            ..MobRules::default()
        };
        let events = tick_full(&mut store, &world, &[], &rules, &[], &[], &mut rng);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, MobEvent::Snowball { .. })),
            "snow golem should throw a snowball at a hostile zombie within 10 blocks"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, MobEvent::SnowTrail { at } if *at == [0, 64, 0])),
            "snow golem on ground with mobGriefing=true should emit SnowTrail"
        );

        // Next tick is on cooldown (20 ticks) and with griefing=false emits no trail.
        rules.griefing = false;
        let events2 = tick_full(&mut store, &world, &[], &rules, &[], &[], &mut rng);
        assert!(
            !events2
                .iter()
                .any(|e| matches!(e, MobEvent::Snowball { .. })),
            "snow golem must respect its 20-tick snowball cooldown"
        );
        assert!(
            !events2
                .iter()
                .any(|e| matches!(e, MobEvent::SnowTrail { .. })),
            "snow golem must not emit SnowTrail when mobGriefing is false"
        );
    }

    #[test]
    fn spider_climbs_and_sidesteps_partial_overhang_through_ai_tick() {
        struct OverhangWorld;
        impl BlockGetter for OverhangWorld {
            fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
                if y < 63 {
                    return ferrite_data::blocks::default_state("stone");
                }
                if y == 63 {
                    // Lava flanking z=0 on west ground so spider must detour along the wall face.
                    if x <= 2 && z.abs() >= 2 {
                        return ferrite_data::blocks::default_state("lava");
                    }
                    return ferrite_data::blocks::default_state("stone");
                }
                // 5-block tall cliff at x=3..=6, z=-3..=3 (walkable top at y=69).
                if (3..=6).contains(&x) && (-3..=3).contains(&z) && (64..=68).contains(&y) {
                    return ferrite_data::blocks::default_state("stone");
                }
                // Partial overhang blocking straight upward climb along z=-1..=1 at y=67.
                if x == 2 && (-1..=1).contains(&z) && y == 67 {
                    return ferrite_data::blocks::default_state("stone");
                }
                ferrite_data::blocks::default_state("air")
            }
        }
        let mut store = EntityStore::default();
        let mut rng = Rng::new(31);
        let key = store.insert(new_mob(
            MobKind::Spider,
            Vec3::new(2.2, 64.0, 0.5),
            &mut rng,
        ));
        let targets = [Target {
            uuid: Uuid::from_u128(301),
            pos: Vec3::new(5.5, 69.0, 0.5),
        }];
        let rules = MobRules::default();
        let mut climbed_top = false;
        for _ in 0..180 {
            tick_full(
                &mut store,
                &OverhangWorld,
                &targets,
                &rules,
                &[],
                &[],
                &mut rng,
            );
            let pos = store.pos(key).unwrap();
            if pos.y >= 68.9 && pos.x >= 3.0 {
                climbed_top = true;
                break;
            }
        }
        assert!(
            climbed_top,
            "spider must climb, sidestep around partial overhang, and top the wall: {:?}, path={:?}",
            store.pos(key),
            mob::state(&store, key).map(|s| &s.path[..usize::from(s.path_len)])
        );
    }

    #[test]
    fn skeleton_avoid_sun_detours_into_shaded_corridor_in_ai_tick() {
        struct ShadedCorridor;
        impl BlockGetter for ShadedCorridor {
            fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
                if y < 64 {
                    return ferrite_data::blocks::default_state("stone");
                }
                // Sight-blocking pillar on the direct path so the skeleton has to pathfind
                // (`!has_line_of_sight`), choosing between sunlit z=1 vs shaded z>=2.
                if x == 6 && z == 0 && (64..=66).contains(&y) {
                    return ferrite_data::blocks::default_state("stone");
                }
                if y == 68 && (1..=11).contains(&x) && (2..=4).contains(&z) {
                    return ferrite_data::blocks::default_state("stone");
                }
                ferrite_data::blocks::default_state("air")
            }
            fn brightness(&self, _x: i32, _y: i32, _z: i32) -> Option<u8> {
                Some(15)
            }
        }
        let mut store = EntityStore::default();
        let mut rng = Rng::new(41);
        let skel = store.insert(new_mob(
            MobKind::Skeleton,
            Vec3::new(0.5, 64.0, 0.5),
            &mut rng,
        ));
        let rules = MobRules {
            monsters_burn: true,
            bright_outside: Some(true),
            ..Default::default()
        };
        let target = [Target {
            uuid: Uuid::from_u128(401),
            pos: Vec3::new(12.5, 64.0, 0.5),
        }];
        let mut saw_shaded_path = false;
        let mut entered_shade = false;
        for _ in 0..40 {
            tick_full(
                &mut store,
                &ShadedCorridor,
                &target,
                &rules,
                &[],
                &[],
                &mut rng,
            );
            let st = mob::state(&store, skel).unwrap();
            if st.path[..usize::from(st.path_len)]
                .iter()
                .any(|p| p[2] >= 2)
            {
                saw_shaded_path = true;
            }
            if store.pos(skel).unwrap().z >= 1.5 {
                entered_shade = true;
            }
        }
        assert!(
            saw_shaded_path,
            "skeleton path must detour into shaded corridor"
        );
        assert!(
            entered_shade,
            "skeleton must physically follow the shaded detour"
        );
    }

    #[test]
    fn zombie_long_detour_exceeds_32_nodes_and_unreachable_throttles_repath() {
        struct BigUWall;
        impl BlockGetter for BigUWall {
            fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
                if y < 64 {
                    return ferrite_data::blocks::default_state("stone");
                }
                if y <= 67
                    && ((x == 12 && (-18..=18).contains(&z))
                        || ((6..=12).contains(&x) && z.abs() == 18))
                {
                    return ferrite_data::blocks::default_state("stone");
                }
                ferrite_data::blocks::default_state("air")
            }
        }
        let mut store = EntityStore::default();
        let mut rng = Rng::new(51);
        let zom = store.insert(new_mob(
            MobKind::Zombie,
            Vec3::new(8.5, 64.0, 0.5),
            &mut rng,
        ));
        let target = [Target {
            uuid: Uuid::from_u128(501),
            pos: Vec3::new(18.5, 64.0, 0.5),
        }];
        let rules = MobRules::default();
        tick_full(&mut store, &BigUWall, &target, &rules, &[], &[], &mut rng);
        let len = mob::state(&store, zom).unwrap().path_len;
        assert!(
            len > 32,
            "long U-wall detour must store >32 waypoints, got {len}"
        );

        // Sealed unreachable vault obeys REPATH_TICKS cooldown instead of re-searching every tick.
        struct SealedVault;
        impl BlockGetter for SealedVault {
            fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
                if y < 64 {
                    return ferrite_data::blocks::default_state("stone");
                }
                if y <= 67
                    && (x - 10).abs() <= 2
                    && z.abs() <= 2
                    && !((x - 10).abs() <= 1 && z.abs() <= 1)
                {
                    return ferrite_data::blocks::default_state("stone");
                }
                ferrite_data::blocks::default_state("air")
            }
        }
        let mut u_store = EntityStore::default();
        let u_zom = u_store.insert(new_mob(
            MobKind::Zombie,
            Vec3::new(0.5, 64.0, 0.5),
            &mut rng,
        ));
        let u_target = [Target {
            uuid: Uuid::from_u128(502),
            pos: Vec3::new(10.5, 64.0, 0.5),
        }];
        tick_full(
            &mut u_store,
            &SealedVault,
            &u_target,
            &rules,
            &[],
            &[],
            &mut rng,
        );
        let timer_t1 = mob::state(&u_store, u_zom).unwrap().path_timer;
        assert_eq!(timer_t1, REPATH_TICKS);
        tick_full(
            &mut u_store,
            &SealedVault,
            &u_target,
            &rules,
            &[],
            &[],
            &mut rng,
        );
        assert_eq!(
            mob::state(&u_store, u_zom).unwrap().path_timer,
            REPATH_TICKS - 1,
            "unreachable goal must count down path_timer instead of re-searching every tick"
        );
    }

    #[test]
    fn iron_golem_retaliates_with_heavy_fling_and_defends_against_non_creepers() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let mut rng = Rng::new(202);
        let golem = store.insert(mob::new_mob(
            MobKind::IronGolem,
            Vec3::new(0.5, 64.0, 0.5),
            &mut rng,
        ));
        let player = Uuid::from_u128(55);
        assert!(mob::retaliate(&mut store, golem, player));
        let targets = [Target {
            uuid: player,
            pos: Vec3::new(1.5, 64.0, 0.5),
        }];
        let events = tick_with(&mut store, &world, &targets, &mut rng);
        let hit = events.iter().find_map(|e| match *e {
            MobEvent::SpecialMelee {
                player: p,
                damage,
                launch_y,
                ..
            } => Some((p, damage, launch_y)),
            _ => None,
        });
        let (hit_player, damage, launch_y) = hit.expect("angry iron golem should hit player");
        assert_eq!(hit_player, player);
        assert!((7.5..=21.5).contains(&damage));
        assert!((launch_y - crate::golem::IRON_GOLEM_LAUNCH_Y).abs() < 1e-6);
        assert!(
            events.iter().any(|e| matches!(
                e,
                MobEvent::Status {
                    key,
                    status: crate::golem::IRON_GOLEM_ATTACK_STATUS
                } if *key == golem
            )),
            "iron golem attack should emit entity status 4"
        );

        // Calm golem ignores creepers but attacks zombies.
        if let Some(s) = mob::state_mut(&mut store, golem) {
            s.anger = 0;
            s.target = None;
            s.attack_cooldown = 0;
        }
        let creeper = store.insert(mob::new_mob(
            MobKind::Creeper,
            Vec3::new(2.0, 64.0, 0.5),
            &mut rng,
        ));
        let creeper_hp = mob::state(&store, creeper).unwrap().health;
        tick_with(&mut store, &world, &[], &mut rng);
        assert_eq!(
            mob::state(&store, creeper).unwrap().health,
            creeper_hp,
            "iron golem must ignore creepers"
        );

        let zombie = store.insert(mob::new_mob(
            MobKind::Zombie,
            Vec3::new(2.0, 64.0, 0.5),
            &mut rng,
        ));
        let zombie_hp = mob::state(&store, zombie).unwrap().health;
        tick_with(&mut store, &world, &[], &mut rng);
        let after_hp = mob::state(&store, zombie).map_or(0.0, |s| s.health);
        assert!(
            after_hp < zombie_hp,
            "calm iron golem should hit nearby hostile zombie"
        );
    }

    #[test]
    fn a_player_built_iron_golem_never_targets_players_but_still_defends_against_mobs() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let mut rng = Rng::new(203);
        let golem = store.insert(mob::new_mob(
            MobKind::IronGolem,
            Vec3::new(0.5, 64.0, 0.5),
            &mut rng,
        ));
        mob::state_mut(&mut store, golem).unwrap().player_created = true;
        let player = Uuid::from_u128(56);
        assert!(!mob::retaliate(&mut store, golem, player));

        // Even a stale target from another path is blocked by the `canAttack` gate.
        mob::state_mut(&mut store, golem).unwrap().target = Some(player);
        let zombie = store.insert(mob::new_mob(
            MobKind::Zombie,
            Vec3::new(1.5, 64.0, 0.5),
            &mut rng,
        ));
        let zombie_hp = mob::state(&store, zombie).unwrap().health;
        let targets = [Target {
            uuid: player,
            pos: Vec3::new(1.5, 64.0, 0.5),
        }];
        let events = tick_with(&mut store, &world, &targets, &mut rng);
        assert!(
            !events.iter().any(|event| matches!(
                event,
                MobEvent::SpecialMelee { player: hit, .. } if *hit == player
            )),
            "a player-created iron golem must not hit a player"
        );
        assert!(
            mob::state(&store, zombie).unwrap().health < zombie_hp,
            "a player-created iron golem still defends against hostile mobs"
        );
    }

    #[test]
    fn floater_swims_across_deep_water_moat_in_ai_tick() {
        struct MoatWorld;
        impl BlockGetter for MoatWorld {
            fn block_state(&self, x: i32, y: i32, _z: i32) -> Option<u16> {
                if (3..=5).contains(&x) && (62..=63).contains(&y) {
                    return ferrite_data::blocks::default_state("water");
                }
                if y < 62 || (!(3..=5).contains(&x) && y < 64) {
                    return ferrite_data::blocks::default_state("stone");
                }
                ferrite_data::blocks::default_state("air")
            }
        }
        let mut store = EntityStore::default();
        let mut rng = Rng::new(61);
        // Wolf is a floater (`has_float_goal`) that follows a path target across the moat.
        let spider = store.insert(new_mob(
            MobKind::Spider,
            Vec3::new(0.5, 64.0, 0.5),
            &mut rng,
        ));
        let target = [Target {
            uuid: Uuid::from_u128(601),
            pos: Vec3::new(9.5, 64.0, 0.5),
        }];
        let rules = MobRules::default();
        let mut crossed = false;
        for _ in 0..160 {
            tick_full(&mut store, &MoatWorld, &target, &rules, &[], &[], &mut rng);
            let pos = store.pos(spider).unwrap();
            if pos.x >= 6.2 && pos.y >= 63.9 {
                crossed = true;
                break;
            }
        }
        assert!(
            crossed,
            "floater must swim across deep water moat onto far bank: {:?}",
            store.pos(spider)
        );
    }

    #[test]
    fn goat_prepares_charges_rams_and_long_jumps_with_reduced_fall_damage() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let mut rng = Rng::new(303);
        let goat = store.insert(mob::new_mob(
            MobKind::Goat,
            Vec3::new(0.5, 64.0, 0.5),
            &mut rng,
        ));
        store.flags_mut(goat).unwrap().set(Flags::ON_GROUND, true);
        if let Some(s) = mob::state_mut(&mut store, goat) {
            s.special_cooldown = 0;
            s.long_jump_cooldown = 999;
        }
        let player = Uuid::from_u128(88);
        let targets = [Target {
            uuid: player,
            pos: Vec3::new(5.5, 64.0, 0.5),
        }];
        let mut rammed = false;
        for _ in 0..80 {
            let events = tick_with(&mut store, &world, &targets, &mut rng);
            if events.iter().any(|e| {
                matches!(
                    e,
                    MobEvent::SpecialMelee {
                        player: p,
                        knockback_xz,
                        ..
                    } if *p == player && (*knockback_xz - crate::goat::ADULT_RAM_KNOCKBACK).abs() < 1e-6
                )
            }) {
                rammed = true;
                break;
            }
        }
        assert!(rammed, "goat should prepare, charge, and ram the player");

        // Long jump when idle and cooldown ready.
        let (mut store2, goat2) = one_mob(MobKind::Goat, 0.5);
        store2.flags_mut(goat2).unwrap().set(Flags::ON_GROUND, true);
        if let Some(s) = mob::state_mut(&mut store2, goat2) {
            s.special_cooldown = 999;
            s.long_jump_cooldown = 0;
        }
        let mut jumped = false;
        for _ in 0..10 {
            let events = tick_with(&mut store2, &world, &[], &mut rng);
            if events.iter().any(|e| {
                matches!(
                    e,
                    MobEvent::Sound {
                        sound: crate::goat::LONG_JUMP_SOUND,
                        ..
                    }
                )
            }) {
                jumped = true;
                break;
            }
        }
        assert!(jumped, "goat should launch a long jump when idle");
        assert!(
            store2.vel(goat2).unwrap().y > 0.3,
            "goat long jump should impart upward velocity"
        );

        // Fall damage reduction of 10 blocks: a 12-block drop hurts a cow but not a goat.
        let before_hp = mob::state(&store2, goat2).unwrap().health;
        assert!(mob::apply_fall_damage(&mut store2, goat2, 12.0).is_none());
        assert_eq!(mob::state(&store2, goat2).unwrap().health, before_hp);
    }

    #[test]
    fn fox_crouches_pounces_at_chicken_and_flees_players() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let mut rng = Rng::new(404);
        let fox = store.insert(mob::new_mob(
            MobKind::Fox,
            Vec3::new(0.5, 64.0, 0.5),
            &mut rng,
        ));
        let _chicken = store.insert(mob::new_mob(
            MobKind::Chicken,
            Vec3::new(4.5, 64.0, 0.5),
            &mut rng,
        ));
        store.flags_mut(fox).unwrap().set(Flags::ON_GROUND, true);

        let mut pounced = false;
        for _ in 0..30 {
            tick_with(&mut store, &world, &[], &mut rng);
            if store.vel(fox).unwrap().y > 0.6 {
                pounced = true;
                break;
            }
        }
        assert!(pounced, "fox should crouch and pounce toward chicken prey");

        // Nearby player causes fox to flee away from the player.
        let (mut store2, fox2) = one_mob(MobKind::Fox, 0.5);
        store2.flags_mut(fox2).unwrap().set(Flags::ON_GROUND, true);
        let start_x = store2.pos(fox2).unwrap().x;
        let targets = [Target {
            uuid: Uuid::from_u128(99),
            pos: Vec3::new(3.5, 64.0, 0.5),
        }];
        for _ in 0..10 {
            tick_with(&mut store2, &world, &targets, &mut rng);
        }
        assert!(
            store2.pos(fox2).unwrap().x < start_x,
            "fox should flee away from a nearby player"
        );
    }

    #[test]
    fn rabbit_hops_with_delay_and_flees_players() {
        let world = TestWorld::flat(64);
        let mut rng = Rng::new(505);
        let (mut store, rabbit) = one_mob(MobKind::Rabbit, 0.5);
        store.flags_mut(rabbit).unwrap().set(Flags::ON_GROUND, true);
        let targets = [Target {
            uuid: Uuid::from_u128(111),
            pos: Vec3::new(3.5, 64.0, 0.5),
        }];
        let start_x = store.pos(rabbit).unwrap().x;
        let mut saw_hop = false;
        for _ in 0..15 {
            tick_with(&mut store, &world, &targets, &mut rng);
            if store.vel(rabbit).unwrap().y > 0.2 {
                saw_hop = true;
            }
        }
        assert!(saw_hop, "rabbit should hop vertically while moving");
        assert!(
            store.pos(rabbit).unwrap().x < start_x,
            "rabbit should flee away from a nearby player"
        );
    }

    #[test]
    fn frog_long_jumps_reduces_fall_damage_and_runs_tongue_state_machine() {
        let world = TestWorld::flat(64);
        let mut rng = Rng::new(606);
        let (mut store, frog) = one_mob(MobKind::Frog, 0.5);
        store.flags_mut(frog).unwrap().set(Flags::ON_GROUND, true);
        if let Some(s) = mob::state_mut(&mut store, frog) {
            s.long_jump_cooldown = 0;
        }
        let mut jumped = false;
        for _ in 0..10 {
            let events = tick_with(&mut store, &world, &[], &mut rng);
            if events.iter().any(|e| {
                matches!(
                    e,
                    MobEvent::Sound {
                        sound: crate::frog::LONG_JUMP_SOUND,
                        ..
                    }
                )
            }) {
                jumped = true;
                break;
            }
        }
        assert!(jumped, "frog should launch a long jump when idle");
        assert!(mob::apply_fall_damage(&mut store, frog, 7.0).is_none());

        // Tongue catch state machine extends, catches, and eats a target within 1.75 blocks.
        let mut store2 = EntityStore::default();
        let frog2 = store2.insert(mob::new_mob(
            MobKind::Frog,
            Vec3::new(0.5, 64.0, 0.5),
            &mut rng,
        ));
        let prey = store2.insert(mob::new_mob(
            MobKind::Chicken,
            Vec3::new(1.5, 64.0, 0.5),
            &mut rng,
        ));
        let mut events = Vec::new();
        let mut eaten = false;
        for _ in 0..25 {
            if crate::frog::tick_tongue_against(&mut store2, frog2, prey, &mut events)
                == crate::frog::TongueStep::Eaten
            {
                eaten = true;
                break;
            }
        }
        assert!(eaten, "frog tongue state machine should eat prey in range");
        assert!(mob::state(&store2, prey).unwrap().is_dead());
        assert!(!crate::frog::is_edible_prey(MobKind::Chicken));
        assert!(crate::frog::is_edible_cube(MobKind::Slime, 1));
        assert!(crate::frog::is_edible_cube(MobKind::MagmaCube, 1));
        assert!(!crate::frog::is_edible_cube(MobKind::Slime, 2));
    }

    #[test]
    fn frog_eats_tiny_magma_cube_through_ai_and_ignores_large_slime() {
        let world = TestWorld::flat(64);
        let mut rng = Rng::new(607);
        let mut store = EntityStore::default();
        let frog = store.insert(mob::new_mob(
            MobKind::Frog,
            Vec3::new(0.5, 64.0, 0.5),
            &mut rng,
        ));
        if let Some(s) = mob::state_mut(&mut store, frog) {
            s.variant = Some(2); // warm -> pearlescent froglight
            s.long_jump_cooldown = 100;
        }
        let big_slime = store.insert(mob::new_mob(
            MobKind::Slime,
            Vec3::new(1.0, 64.0, 0.5),
            &mut rng,
        ));
        mob::set_cube_size(&mut store, big_slime, 2);
        let tiny_cube = store.insert(mob::new_mob(
            MobKind::MagmaCube,
            Vec3::new(1.5, 64.0, 0.5),
            &mut rng,
        ));
        mob::set_cube_size(&mut store, tiny_cube, 1);
        let players = [Target {
            uuid: Uuid::from_u128(999),
            pos: Vec3::new(0.5, 64.0, 0.5),
        }];

        let mut ate_event = None;
        for _ in 0..25 {
            let events = tick_with(&mut store, &world, &players, &mut rng);
            if let Some(ev) = events
                .into_iter()
                .find(|e| matches!(e, MobEvent::FrogEat { .. }))
            {
                ate_event = Some(ev);
                break;
            }
        }
        assert!(
            matches!(
                ate_event,
                Some(MobEvent::FrogEat {
                    prey,
                    prey_kind: MobKind::MagmaCube,
                    frog_variant: Some(2),
                    ..
                }) if prey == tiny_cube
            ),
            "frog should eat the tiny magma cube and emit FrogEat"
        );
        assert!(
            mob::state(&store, tiny_cube).unwrap().is_dead(),
            "tiny magma cube should be dead after frog eats it"
        );
        assert!(!mob::state(&store, big_slime).unwrap().is_dead());
        assert_eq!(
            crate::frog::froglight_for_variant(Some(2)),
            "minecraft:pearlescent_froglight"
        );
    }

    #[test]
    fn bee_retaliates_stings_with_poison_and_dies_after_countdown() {
        let world = TestWorld::flat(64);
        let mut rng = Rng::new(707);
        let (mut store, bee) = one_mob(MobKind::Bee, 0.5);
        let player = Uuid::from_u128(222);
        assert!(mob::retaliate(&mut store, bee, player));
        let targets = [Target {
            uuid: player,
            pos: Vec3::new(1.5, 64.0, 0.5),
        }];
        let mut rules = MobRules::default();
        let events = tick_full(&mut store, &world, &targets, &rules, &[], &[], &mut rng);
        let poison = events.iter().find_map(|e| match *e {
            MobEvent::SpecialMelee {
                player: p,
                poison_ticks,
                ..
            } if p == player => Some(poison_ticks),
            _ => None,
        });
        assert_eq!(poison, Some(crate::bee::POISON_TICKS_NORMAL));
        assert!(mob::state(&store, bee).unwrap().has_stung);
        assert!(
            !mob::retaliate(&mut store, bee, player),
            "stung bee must not retaliate again"
        );

        // Hard difficulty applies 18s (360 ticks) of Poison.
        let (mut store_hard, bee_hard) = one_mob(MobKind::Bee, 0.5);
        assert!(mob::retaliate(&mut store_hard, bee_hard, player));
        rules.hard = true;
        let hard_events = tick_full(
            &mut store_hard,
            &world,
            &targets,
            &rules,
            &[],
            &[],
            &mut rng,
        );
        let hard_poison = hard_events.iter().find_map(|e| match *e {
            MobEvent::SpecialMelee { poison_ticks, .. } => Some(poison_ticks),
            _ => None,
        });
        assert_eq!(hard_poison, Some(crate::bee::POISON_TICKS_HARD));

        // Post-sting death countdown kills the bee by STING_DEATH_COUNTDOWN ticks.
        if let Some(s) = mob::state_mut(&mut store, bee) {
            s.sting_ticks = crate::bee::STING_DEATH_COUNTDOWN - 1;
        }
        tick_with(&mut store, &world, &[], &mut rng);
        assert!(
            mob::state(&store, bee).unwrap().is_dead(),
            "stung bee should die once its post-sting countdown expires"
        );
    }

    #[test]
    fn blaze_volley_cadence_melee_under_2_blocks_and_hover_slow_fall() {
        use ferrite_protocol::packets::play::MetaValue;

        let world = TestWorld::flat(64);
        let (mut store, blaze) = one_mob(MobKind::Blaze, 0.5);
        let player = [Target {
            uuid: Uuid::from_u128(0xB1A2),
            pos: Vec3::new(10.5, 64.0, 0.5),
        }];
        let mut rng = Rng::new(41);
        let mut shot_ticks = Vec::new();
        for tick in 1..=185 {
            let events = tick_with(&mut store, &world, &player, &mut rng);
            if events
                .iter()
                .any(|e| matches!(e, MobEvent::SmallFireball { .. }))
            {
                shot_ticks.push(tick);
            }
            if tick == 1 {
                let s = mob::state(&store, blaze).unwrap();
                assert!(s.charged);
                assert_eq!(s.bow_timer, BLAZE_CHARGE_TICKS);
                assert_eq!(
                    store
                        .data(blaze)
                        .unwrap()
                        .metadata
                        .get(mob::BLAZE_FLAGS_INDEX),
                    Some(&MetaValue::Byte(mob::BLAZE_FLAG_ON_FIRE))
                );
            }
            if tick == 79 {
                let s = mob::state(&store, blaze).unwrap();
                assert!(!s.charged);
                assert_eq!(s.bow_timer, BLAZE_COOLDOWN_TICKS);
                assert_eq!(
                    store
                        .data(blaze)
                        .unwrap()
                        .metadata
                        .get(mob::BLAZE_FLAGS_INDEX),
                    Some(&MetaValue::Byte(0))
                );
            }
        }
        assert_eq!(
            shot_ticks,
            vec![61, 67, 73],
            "volley shoots 3 fireballs 6 ticks apart after 60-tick charge, then 100-tick cooldown"
        );

        // Inside 2 blocks: melee at 20-tick interval rather than fireball.
        let (mut melee_store, melee_blaze) = one_mob(MobKind::Blaze, 0.5);
        let close = [Target {
            uuid: Uuid::from_u128(0xB1A3),
            pos: Vec3::new(1.8, 64.0, 0.5),
        }];
        let mut melee_rng = Rng::new(42);
        let mut melee_ticks = Vec::new();
        for tick in 1..=25 {
            let events = tick_with(&mut melee_store, &world, &close, &mut melee_rng);
            if events.iter().any(
                |e| matches!(e, MobEvent::Melee { damage, .. } if (*damage - 6.0).abs() < 1e-4),
            ) {
                melee_ticks.push(tick);
            }
            assert!(
                !events
                    .iter()
                    .any(|e| matches!(e, MobEvent::SmallFireball { .. })),
                "no fireballs inside 2 blocks"
            );
        }
        assert_eq!(melee_ticks, vec![1, 21]);
        assert!(!mob::state(&melee_store, melee_blaze).unwrap().charged);

        // Slow fall & hover toward elevated target.
        let mut air_store = EntityStore::default();
        let air_blaze = air_store.insert(new_mob(
            MobKind::Blaze,
            Vec3::new(0.5, 80.0, 0.5),
            &mut Rng::new(43),
        ));
        air_store.set_vel(air_blaze, Vec3::new(0.0, -0.5, 0.0));
        let mut no_aggro_rules = MobRules::default();
        no_aggro_rules.untargetable.insert(Uuid::from_u128(0xB1A4));
        let high_target = [Target {
            uuid: Uuid::from_u128(0xB1A4),
            pos: Vec3::new(10.5, 86.0, 0.5),
        }];
        tick_full(
            &mut air_store,
            &world,
            &high_target,
            &no_aggro_rules,
            &[],
            &[],
            &mut Rng::new(44),
        );
        assert!(
            air_store.vel(air_blaze).unwrap().y > -0.4,
            "airborne blaze falls slowly: {:?}",
            air_store.vel(air_blaze)
        );
        air_store.set_vel(air_blaze, Vec3::ZERO);
        mob::state_mut(&mut air_store, air_blaze)
            .unwrap()
            .height_offset = 0.5;
        mob::state_mut(&mut air_store, air_blaze)
            .unwrap()
            .height_offset_timer = 50;
        let y_before = air_store.pos(air_blaze).unwrap().y;
        for _ in 0..20 {
            tick_with(&mut air_store, &world, &high_target, &mut Rng::new(45));
        }
        assert!(
            air_store.pos(air_blaze).unwrap().y > y_before,
            "blaze hovers upward when target eye is above height offset: before={y_before}, after={:?}",
            air_store.pos(air_blaze)
        );
    }

    #[test]
    fn stray_and_bogged_emit_slowness_and_poison_arrows() {
        use crate::effect::Effect;

        let world = TestWorld::flat(64);
        let target = [Target {
            uuid: Uuid::from_u128(0x57A4),
            pos: Vec3::new(8.5, 64.0, 0.5),
        }];
        let (mut stray_store, _) = one_mob(MobKind::Stray, 0.5);
        let stray_events = tick_with(&mut stray_store, &world, &target, &mut Rng::new(51));
        assert!(
            stray_events.iter().any(|e| matches!(
                e,
                MobEvent::Arrow { effect: Some(eff), .. }
                if eff.kind() == Effect::Slowness && eff.duration == crate::arrow::STRAY_SLOWNESS_TICKS
            )),
            "stray emits Slowness 600t arrow: {stray_events:?}"
        );

        let (mut bogged_store, bogged) = one_mob(MobKind::Bogged, 0.5);
        let bogged_events = tick_with(&mut bogged_store, &world, &target, &mut Rng::new(52));
        assert!(
            bogged_events.iter().any(|e| matches!(
                e,
                MobEvent::Arrow { effect: Some(eff), .. }
                if eff.kind() == Effect::Poison && eff.duration == crate::arrow::BOGGED_POISON_TICKS
            )),
            "bogged emits Poison 100t arrow: {bogged_events:?}"
        );
        assert_eq!(mob::state(&bogged_store, bogged).unwrap().bow_timer, 70);
    }

    #[test]
    fn witch_potion_selection_throw_arc_and_self_heal() {
        use ferrite_protocol::packets::play::MetaValue;

        let world = TestWorld::flat(64);
        let target_id = Uuid::from_u128(0xA17C);

        let throw_for = |dist: f64, snapshot: PlayerSnapshot| -> Option<(Vec3, mob::WitchPotion)> {
            let (mut store, _) = one_mob(MobKind::Witch, 0.5);
            let mut rules = MobRules::default();
            rules.player_info.insert(target_id, snapshot);
            let targets = [Target {
                uuid: target_id,
                pos: Vec3::new(0.5 + dist, 64.0, 0.5),
            }];
            let mut rng = Rng::new(61);
            let events = tick_full(&mut store, &world, &targets, &rules, &[], &[], &mut rng);
            events.into_iter().find_map(|e| match e {
                MobEvent::SplashPotion {
                    velocity, potion, ..
                } => Some((velocity, potion)),
                _ => None,
            })
        };

        let (vel_slow, pot_slow) = throw_for(9.0, PlayerSnapshot::full_health()).unwrap();
        assert_eq!(pot_slow, mob::WitchPotion::Slowness);
        assert!(vel_slow.y > 0.0, "thrown potion arcs upward");

        let (_, pot_poison) = throw_for(
            5.0,
            PlayerSnapshot {
                health: 20.0,
                has_slowness: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(pot_poison, mob::WitchPotion::Poison);

        let (_, pot_harm) = throw_for(
            5.0,
            PlayerSnapshot {
                health: 6.0,
                has_slowness: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(pot_harm, mob::WitchPotion::Harming);

        // Close range <= 3.0 with injured slowed non-weak target eventually rolls Weakness (25%).
        let mut saw_weakness = false;
        let (mut store, witch) = one_mob(MobKind::Witch, 0.5);
        let mut rules = MobRules::default();
        rules.player_info.insert(
            target_id,
            PlayerSnapshot {
                health: 6.0,
                has_slowness: true,
                ..Default::default()
            },
        );
        let close = [Target {
            uuid: target_id,
            pos: Vec3::new(3.0, 64.0, 0.5),
        }];
        let mut rng = Rng::new(62);
        for _ in 0..40 {
            mob::state_mut(&mut store, witch).unwrap().bow_timer = 0;
            mob::state_mut(&mut store, witch).unwrap().drink_timer = 0;
            mob::state_mut(&mut store, witch).unwrap().health = MobKind::Witch.max_health();
            let evs = tick_full(&mut store, &world, &close, &rules, &[], &[], &mut rng);
            if evs.iter().any(|e| {
                matches!(
                    e,
                    MobEvent::SplashPotion {
                        potion: mob::WitchPotion::Weakness,
                        ..
                    }
                )
            }) {
                saw_weakness = true;
                break;
            }
        }
        assert!(saw_weakness, "witch picks weakness at <= 3 blocks");

        // Distant target (> 11 blocks) causes Witch to drink Swiftness; injured idle Witch drinks Healing.
        let (mut speed_store, speed_witch) = one_mob(MobKind::Witch, 0.5);
        let far_target = [Target {
            uuid: target_id,
            pos: Vec3::new(15.5, 64.0, 0.5),
        }];
        let mut speed_rng = Rng::new(63);
        let mut drank_swiftness = false;
        let mut threw_while_drinking = false;
        for _ in 0..100 {
            let events = tick_with(&mut speed_store, &world, &far_target, &mut speed_rng);
            let s = mob::state(&speed_store, speed_witch).unwrap();
            if s.using_item {
                assert_eq!(s.drink_potion, Some(mob::WitchPotion::Swiftness));
                assert_eq!(
                    speed_store
                        .data(speed_witch)
                        .unwrap()
                        .metadata
                        .get(mob::WITCH_USING_ITEM_INDEX),
                    Some(&MetaValue::Boolean(true))
                );
                if events
                    .iter()
                    .any(|e| matches!(e, MobEvent::SplashPotion { .. }))
                {
                    threw_while_drinking = true;
                }
                drank_swiftness = true;
            } else if drank_swiftness {
                break;
            }
        }
        assert!(
            drank_swiftness,
            "witch drank swiftness when chasing > 11 blocks"
        );
        assert!(!threw_while_drinking, "witch must not throw while drinking");
        assert!(
            speed_store
                .data(speed_witch)
                .unwrap()
                .effects
                .has(crate::effect::Effect::Speed)
        );

        // Injured witch (within player keep-alive radius, no distant target) drinks healing potion
        // over 32 ticks, clears mainhand, and heals +4 HP.
        let (mut heal_store, heal_witch) = one_mob(MobKind::Witch, 0.5);
        mob::state_mut(&mut heal_store, heal_witch).unwrap().health = 10.0;
        let mut heal_rules = MobRules::default();
        heal_rules.untargetable.insert(target_id);
        let nearby = [Target {
            uuid: target_id,
            pos: Vec3::new(5.5, 64.0, 0.5),
        }];
        let mut heal_rng = Rng::new(64);
        let mut started_drink = false;
        for _ in 0..300 {
            tick_full(
                &mut heal_store,
                &world,
                &nearby,
                &heal_rules,
                &[],
                &[],
                &mut heal_rng,
            );
            let s = mob::state(&heal_store, heal_witch).unwrap();
            if s.using_item {
                assert_eq!(s.drink_potion, Some(mob::WitchPotion::Healing));
                started_drink = true;
            } else if started_drink {
                break;
            }
        }
        assert!(
            started_drink,
            "injured witch started drinking healing potion"
        );
        let after = mob::state(&heal_store, heal_witch).unwrap();
        assert_eq!(after.health, 14.0);
        assert_eq!(after.equipment[mob::MAIN_HAND as usize], 0);
    }

    #[test]
    fn piglin_crossbow_gold_armor_neutrality_and_anger() {
        use ferrite_protocol::packets::play::MetaValue;

        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let piglin = store.insert(new_mob(
            MobKind::Piglin,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(71),
        ));
        let crossbow = ferrite_item::ItemStack::from_name("minecraft:crossbow").unwrap();
        mob::set_equipment_stack(
            &mut store,
            piglin,
            crate::EquipmentSlot::MainHand,
            Some(crossbow),
        );
        mob::state_mut(&mut store, piglin).unwrap().age = 0;

        let player = Uuid::from_u128(0xF162u128);
        let targets = [Target {
            uuid: player,
            pos: Vec3::new(6.5, 64.0, 0.5),
        }];

        // Gold armor keeps a calm piglin neutral.
        let mut gold_rules = MobRules::default();
        gold_rules.player_info.insert(
            player,
            PlayerSnapshot {
                wearing_gold: true,
                ..PlayerSnapshot::full_health()
            },
        );
        let mut rng = Rng::new(72);
        let events = tick_full(
            &mut store,
            &world,
            &targets,
            &gold_rules,
            &[],
            &[],
            &mut rng,
        );
        assert!(events.is_empty());
        assert_eq!(mob::state(&store, piglin).unwrap().target, None);

        // Removing gold starts charging -> 25 ticks -> attack delay -> crossbow arrow.
        let unarmored_rules = MobRules::default();
        tick_full(
            &mut store,
            &world,
            &targets,
            &unarmored_rules,
            &[],
            &[],
            &mut rng,
        );
        let s = mob::state(&store, piglin).unwrap();
        assert_eq!(s.crossbow_state, mob::CrossbowState::Charging);
        assert!(s.charging_crossbow);
        assert_eq!(s.bow_timer, PIGLIN_CROSSBOW_CHARGE_TICKS);
        assert_eq!(
            store
                .data(piglin)
                .unwrap()
                .metadata
                .get(mob::PIGLIN_CHARGING_CROSSBOW_INDEX),
            Some(&MetaValue::Boolean(true))
        );

        let mut fired = None;
        for _ in 0..80 {
            let evs = tick_full(
                &mut store,
                &world,
                &targets,
                &unarmored_rules,
                &[],
                &[],
                &mut rng,
            );
            if let Some(arrow) = evs
                .into_iter()
                .find(|e| matches!(e, MobEvent::Arrow { .. }))
            {
                fired = Some(arrow);
                break;
            }
        }
        assert!(
            matches!(
                fired,
                Some(MobEvent::Arrow { damage, speed, effect: None, .. })
                if (damage - PIGLIN_CROSSBOW_DAMAGE).abs() < 1e-4
                    && (speed - PIGLIN_CROSSBOW_SPEED).abs() < 1e-4
            ),
            "piglin fires 1.6-speed base-2.0 crossbow arrow: {fired:?}"
        );

        // Provocation overrides golden armor neutrality.
        assert!(mob::retaliate(&mut store, piglin, player));
        let ev_angry = tick_full(
            &mut store,
            &world,
            &targets,
            &gold_rules,
            &[],
            &[],
            &mut rng,
        );
        assert_eq!(mob::state(&store, piglin).unwrap().target, Some(player));
        assert!(mob::state(&store, piglin).unwrap().anger > 0);
        let _ = ev_angry;
    }

    #[test]
    fn piglin_bartering_ai_ticks_admire_emits_event_and_skips_babies() {
        let mut store = EntityStore::default();
        let mut rng = Rng::new(71);
        let world = TestWorld::flat(64);
        let player = Uuid::from_u128(0xB417);
        let player_pos = Vec3::new(4.5, 64.0, 0.5);
        let targets = [Target {
            uuid: player,
            pos: player_pos,
        }];
        let rules = MobRules::default();

        let adult = store.insert(new_mob(
            MobKind::Piglin,
            Vec3::new(0.5, 64.0, 0.5),
            &mut rng,
        ));
        mob::state_mut(&mut store, adult).unwrap().age = 0;

        let baby = store.insert(new_mob(
            MobKind::Piglin,
            Vec3::new(1.5, 64.0, 0.5),
            &mut rng,
        ));
        mob::state_mut(&mut store, baby).unwrap().age = mob::PIGLIN_BABY_AGE;

        assert!(crate::piglin::start_admiring(&mut store, adult).is_some());

        let mut barters = Vec::new();
        for _ in 0..crate::piglin::ADMIRE_TICKS {
            let evs = tick_full(&mut store, &world, &targets, &rules, &[], &[], &mut rng);
            // Baby piglins never target players; admiring adults pause targeting.
            assert_eq!(mob::state(&store, baby).unwrap().target, None);
            for ev in evs {
                if let MobEvent::PiglinBarter { key, toward, .. } = ev {
                    barters.push((key, toward));
                }
            }
        }
        assert_eq!(barters, vec![(adult, Some(player_pos))]);
        assert!(!crate::piglin::is_admiring(&store, adult));
        assert!(mob::equipment_stack(&store, adult, crate::EquipmentSlot::OffHand).is_none());

        // Hurting an admiring piglin cancels the barter and clears its off-hand.
        assert!(crate::piglin::start_admiring(&mut store, adult).is_some());
        assert!(mob::retaliate(&mut store, adult, player));
        assert!(!crate::piglin::is_admiring(&store, adult));
        assert!(mob::equipment_stack(&store, adult, crate::EquipmentSlot::OffHand).is_none());
        for _ in 0..crate::piglin::ADMIRE_TICKS {
            let evs = tick_full(&mut store, &world, &targets, &rules, &[], &[], &mut rng);
            assert!(
                !evs.iter()
                    .any(|e| matches!(e, MobEvent::PiglinBarter { .. }))
            );
        }
    }

    #[test]
    fn piglin_hunts_hoglin_and_celebrates_by_dancing() {
        let mut store = EntityStore::default();
        let mut rng = Rng::new(72);
        let world = TestWorld::flat(64);
        let rules = MobRules::default();

        let piglin = store.insert(new_mob(
            MobKind::Piglin,
            Vec3::new(0.5, 64.0, 0.5),
            &mut rng,
        ));
        mob::state_mut(&mut store, piglin).unwrap().age = 0;

        let hoglin = store.insert(new_mob(
            MobKind::Hoglin,
            Vec3::new(1.5, 64.0, 0.5),
            &mut rng,
        ));
        mob::state_mut(&mut store, hoglin).unwrap().age = 0;
        mob::state_mut(&mut store, hoglin).unwrap().health = 1.0;

        tick_full(&mut store, &world, &[], &rules, &[], &[], &mut rng);
        let p = mob::state(&store, piglin).unwrap();
        assert!(p.dancing, "piglin should dance after hunting a hoglin");
        assert!(mob::state(&store, hoglin).unwrap().is_dead());
    }

    #[test]
    fn witch_heals_injured_raider_with_potion_event() {
        let mut store = EntityStore::default();
        let mut rng = Rng::new(73);
        let world = TestWorld::flat(64);
        let rules = MobRules::default();

        let witch = store.insert(new_mob(MobKind::Witch, Vec3::new(0.5, 64.0, 0.5), &mut rng));
        let pillager = store.insert(new_mob(
            MobKind::Pillager,
            Vec3::new(3.5, 64.0, 0.5),
            &mut rng,
        ));
        mob::state_mut(&mut store, pillager).unwrap().health = 8.0;

        let evs = tick_full(&mut store, &world, &[], &rules, &[], &[], &mut rng);
        assert!(evs.iter().any(|e| matches!(
            e,
            MobEvent::WitchHealRaider { witch: w, victim: v, .. }
            if *w == witch && *v == pillager
        )));
    }

    #[test]
    fn goat_snaps_horn_on_snaps_goat_horn_block_ram() {
        let mut store = EntityStore::default();
        let mut rng = Rng::new(74);
        let mut world = TestWorld::flat(64);
        let log = ferrite_data::blocks::default_state("minecraft:oak_log").unwrap();
        world.blocks.insert((1, 64, 0), log);
        world.blocks.insert((1, 65, 0), log);

        let goat = store.insert(new_mob(MobKind::Goat, Vec3::new(0.6, 64.0, 0.5), &mut rng));
        {
            let s = mob::state_mut(&mut store, goat).unwrap();
            s.ram_phase = 2; // PHASE_CHARGE
            s.special_timer = 10;
            s.ram_target = Some(Vec3::new(5.5, 64.0, 0.5));
        }
        store
            .flags_mut(goat)
            .unwrap()
            .set(Flags::HORIZONTAL_COLLISION, true);

        let mut events = Vec::new();
        crate::goat::tick_goat(&mut store, goat, &world, &[], true, &mut rng, &mut events);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, MobEvent::GoatSnapHorn { key, .. } if *key == goat)),
            "expected horn snap on oak_log ram: {events:?}"
        );
    }

    #[test]
    fn fox_ignores_sneaking_player_and_finds_sweet_berries() {
        let mut store = EntityStore::default();
        let mut rng = Rng::new(75);
        let mut world = TestWorld::flat(64);
        let bush =
            ferrite_data::blocks::state_id("minecraft:sweet_berry_bush", &[("age", "3")]).unwrap();
        world.blocks.insert((1, 64, 0), bush);

        let fox = store.insert(new_mob(MobKind::Fox, Vec3::new(0.5, 64.0, 0.5), &mut rng));
        let player = Uuid::from_u128(0xF0C5);
        let targets = [Target {
            uuid: player,
            pos: Vec3::new(2.5, 64.0, 0.5),
        }];
        let step = crate::fox::tick_fox_with_sneaking(
            &mut store,
            fox,
            &world,
            &targets,
            |u| u == player,
            Vec3::new(0.5, 64.0, 0.5),
            true,
        );
        assert!(matches!(step, crate::fox::FoxStep::Idle));
        assert_eq!(
            crate::fox::find_sweet_berry_bush(&world, Vec3::new(0.5, 64.0, 0.5)),
            Some([1, 64, 0])
        );
    }

    #[test]
    fn killer_bunny_attacks_and_normal_rabbit_finds_carrots() {
        let mut store = EntityStore::default();
        let mut rng = Rng::new(76);
        let mut world = TestWorld::flat(64);
        let carrots = ferrite_data::blocks::state_id("minecraft:carrots", &[("age", "7")]).unwrap();
        world.blocks.insert((1, 64, 0), carrots);

        assert_eq!(
            crate::rabbit::find_carrot_crop(&world, Vec3::new(0.5, 64.0, 0.5)),
            Some(([1, 64, 0], 6))
        );

        let killer = store.insert(new_mob(
            MobKind::Rabbit,
            Vec3::new(0.5, 64.0, 0.5),
            &mut rng,
        ));
        mob::state_mut(&mut store, killer).unwrap().variant = Some(crate::rabbit::KILLER_BUNNY);
        let player = Uuid::from_u128(0x881);
        let targets = [Target {
            uuid: player,
            pos: Vec3::new(1.5, 64.0, 0.5),
        }];
        let evs = tick_full(
            &mut store,
            &world,
            &targets,
            &MobRules::default(),
            &[],
            &[],
            &mut rng,
        );
        assert!(evs.iter().any(|e| matches!(
            e,
            MobEvent::Melee { attacker, damage, .. }
            if *attacker == killer && (*damage - crate::rabbit::KILLER_BUNNY_DAMAGE).abs() < 1e-6
        )));
    }
}
