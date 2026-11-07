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
/// Vanilla `LookAtPlayerGoal` range in blocks.
pub const LOOK_DISTANCE: f64 = 8.0;
/// Eye height of a player, added to the player's feet.
pub const PLAYER_EYE_HEIGHT: f64 = 1.62;
/// Share of the movement speed used as per-tick acceleration (friction caps terminal speed).
const WALK_ACCELERATION: f64 = 0.09;
/// Vanilla creeper trigger radius (`Creeper` starts swelling within 3 blocks).
pub const CREEPER_TRIGGER_DISTANCE: f64 = 3.0;
/// Vanilla creeper fuse length (30 swell ticks).
pub const CREEPER_SWELL_TICKS: u16 = 30;
/// Vanilla creeper explosion power.
pub const CREEPER_POWER: f32 = 3.0;
/// Skeleton bow range (`RangedAttackGoal` follow range is 15-16 blocks).
pub const SKELETON_RANGE: f64 = 16.0;
/// Skeleton arrows do a base 2 damage.
pub const SKELETON_ARROW_DAMAGE: f32 = 2.0;
/// Ticks between skeleton shots.
pub const SKELETON_COOLDOWN: u16 = 40;
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

/// A player as the goals see it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target {
    pub uuid: Uuid,
    pub pos: Vec3,
}

/// What a mob wants the region to do this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MobEvent {
    /// A melee attack landed on a player.
    Melee {
        player: Uuid,
        damage: f32,
        from: Vec3,
        /// The attacking mob, so a raised shield can knock it back.
        attacker: crate::EntityKey,
    },
    /// A skeleton fires an arrow.
    Arrow {
        from: Vec3,
        direction: Vec3,
        damage: f32,
    },
    /// A lead stretched too far and snapped.
    LeashBroke { key: crate::EntityKey },
    /// A llama spits at a player.
    Spit { from: Vec3, direction: Vec3 },
    /// The ender dragon spits a fireball (`dragon_fireball`) at a player.
    DragonFireball { from: Vec3, direction: Vec3 },
    /// The perched dragon breathes: a lingering cloud forms at `pos`.
    Breath { pos: Vec3 },
    /// A creeper's fuse ran out.
    Explode { center: Vec3, power: f32 },
    /// Two animals bred; the baby is already in the store.
    Bred(crate::breed::Bred),
    /// An entity event for the players tracking `key` (hearts, smoke).
    Status { key: crate::EntityKey, status: u8 },
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
    let eye = pos.y + eye_height;
    let mut best: Option<(f64, Vec3)> = None;
    for player in players {
        let dx = player.x - pos.x;
        let dz = player.z - pos.z;
        let horizontal = (dx * dx + dz * dz).sqrt();
        if horizontal > LOOK_DISTANCE || (dx == 0.0 && dz == 0.0 && player.y == eye) {
            continue;
        }
        if best.is_none_or(|(distance, _)| horizontal < distance) {
            best = Some((horizontal, *player));
        }
    }
    let (horizontal, target) = best?;
    let dx = target.x - pos.x;
    let dz = target.z - pos.z;
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
fn nearest_target(players: &[Target], pos: Vec3, range: f64) -> Option<&Target> {
    players
        .iter()
        .filter(|target| {
            let dx = target.pos.x - pos.x;
            let dz = target.pos.z - pos.z;
            dx * dx + dz * dz <= range * range && (target.pos.y - pos.y).abs() <= 8.0
        })
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
    let mut events = Vec::new();
    let player_positions: Vec<Vec3> = players.iter().map(|target| target.pos).collect();
    let owners: Vec<(Uuid, Vec3)> = players
        .iter()
        .map(|target| (target.uuid, target.pos))
        .collect();
    for key in store.keys().to_vec() {
        if !mob::is_mob(store, key) {
            continue;
        }
        mob::tick_clock(store, key);
        let Some(kind) = mob::state(store, key).map(|state| state.kind) else {
            continue;
        };
        if kind.is_animal() {
            crate::breed::tick_age(store, key);
            // Vanilla `Animal.aiStep`: hearts every half second while in love (event 18).
            if mob::state(store, key).is_some_and(|s| s.love > 0 && s.love % 10 == 0) {
                events.push(MobEvent::Status { key, status: 18 });
            }
        }
        let mut directed = false;
        if kind == MobKind::EnderDragon {
            crate::dragon::fly(store, key, players, rng, &mut events);
            continue;
        }
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
            if state.lifetime == 1 {
                // Gone without a trace and without loot.
                state.loot_dropped = true;
                state.health = 0.0;
            }
            state.lifetime = state.lifetime.saturating_sub(1);
            if matches!(
                state.kind,
                MobKind::Wolf | MobKind::Llama | MobKind::TraderLlama
            ) {
                state.anger = state.anger.saturating_sub(1);
                if state.anger == 0 {
                    state.target = None;
                }
            }
        }

        if kind.is_hostile() {
            let target = nearest_target(players, start, kind.follow_range()).copied();
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
            if let Some(target) = target {
                let dx = target.pos.x - start.x;
                let dz = target.pos.z - start.z;
                let horizontal = (dx * dx + dz * dz).sqrt();
                let yaw = yaw_towards(dx, dz);
                look = Some((yaw, 0.0));
                match kind {
                    MobKind::Creeper => {
                        let mut explode = false;
                        if let Some(state) = mob::state_mut(store, key) {
                            if horizontal <= CREEPER_TRIGGER_DISTANCE
                                && (target.pos.y - start.y).abs() <= 2.0
                            {
                                state.swell = state.swell.saturating_add(1);
                                explode = state.swell >= CREEPER_SWELL_TICKS;
                            } else {
                                state.swell = state.swell.saturating_sub(1);
                            }
                        }
                        if explode {
                            events.push(MobEvent::Explode {
                                center: Vec3::new(start.x, start.y + 0.5, start.z),
                                power: CREEPER_POWER,
                            });
                            mob::kill(store, key);
                            continue;
                        }
                    }
                    MobKind::Skeleton => {
                        let eye_height = f64::from(kind.props().eye_height);
                        if horizontal <= SKELETON_RANGE {
                            if horizontal > 8.0 {
                                walking = Some(yaw);
                            } else if horizontal < 4.0 {
                                walking = Some(wrap_degrees(yaw + 180.0));
                            }
                            let ready =
                                mob::state(store, key).is_some_and(|state| state.bow_timer == 0);
                            if ready {
                                let from = Vec3::new(start.x, start.y + eye_height, start.z);
                                let to = Vec3::new(
                                    target.pos.x,
                                    target.pos.y + PLAYER_EYE_HEIGHT,
                                    target.pos.z,
                                );
                                events.push(MobEvent::Arrow {
                                    from,
                                    direction: (to - from).normalize(),
                                    damage: SKELETON_ARROW_DAMAGE,
                                });
                                if let Some(state) = mob::state_mut(store, key) {
                                    state.bow_timer = SKELETON_COOLDOWN;
                                }
                            }
                        }
                    }
                    MobKind::Zombie | MobKind::Spider | MobKind::Enderman => {
                        if horizontal <= kind.attack_reach() {
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
                            walking = follow_path(store, key, blocks, start, target.pos);
                            speed = CHASE_ACCELERATION;
                        }
                        // Enderman basics: blink to a nearby walkable spot now and then.
                        if kind == MobKind::Enderman && rng.next_int(400) == 0 {
                            blink(store, key, blocks, start, rng);
                        }
                    }
                    _ => {}
                }
            } else if kind == MobKind::Enderman && rng.next_int(200) == 0 {
                blink(store, key, blocks, start, rng);
            }
        } else {
            // Passive goals: flee when hurt, otherwise wander; look at a nearby player.
            let (panic, panic_from, moving, wander_yaw) =
                mob::state(store, key).map_or((0, None, false, 0.0), |state| {
                    (
                        state.panic,
                        state.panic_from,
                        state.moving,
                        state.wander_yaw,
                    )
                });
            let angry_at = if matches!(kind, MobKind::Wolf | MobKind::Llama | MobKind::TraderLlama)
            {
                mob::state(store, key)
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
                    &owners,
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
                if (dx * dx + dz * dz).sqrt() <= kind.attack_reach() {
                    let ready =
                        mob::state(store, key).is_some_and(|state| state.attack_cooldown == 0);
                    if ready {
                        mob::hurt(store, victim, crate::tame::TAMED_BITE, Some(start));
                        if let Some(state) = mob::state_mut(store, key) {
                            state.attack_cooldown = MELEE_COOLDOWN;
                        }
                    }
                } else {
                    walking = follow_path(store, key, blocks, start, victim_pos);
                    speed = CHASE_ACCELERATION;
                }
            } else if let Some(target) = angry_at {
                // A wild wolf chases and bites the player who hurt it.
                directed = true;
                let dx = target.pos.x - start.x;
                let dz = target.pos.z - start.z;
                if kind.is_llama() {
                    // `LlamaSpit`: a wild llama spits at whoever hurt it from up to 10 blocks.
                    if (dx * dx + dz * dz).sqrt() <= 10.0
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
                } else if (dx * dx + dz * dz).sqrt() <= kind.attack_reach() {
                    let ready =
                        mob::state(store, key).is_some_and(|state| state.attack_cooldown == 0);
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
                    walking = follow_path(store, key, blocks, start, target.pos);
                    speed = CHASE_ACCELERATION;
                }
            } else if panic > 0 {
                if let Some(from) = panic_from {
                    walking = Some(yaw_towards(start.x - from.x, start.z - from.z));
                    speed = 1.5;
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
                && let Some(owner_pos) = crate::tame::follow_target(store, key, &owners, start)
            {
                directed = true;
                walking = Some(yaw_towards(owner_pos.x - start.x, owner_pos.z - start.z));
            } else if kind.is_tameable() && mob::state(store, key).is_some_and(|s| s.sitting) {
                directed = true;
            } else if let Some((mate, mate_pos)) = crate::breed::mate_for(store, key) {
                directed = true;
                walking = Some(yaw_towards(mate_pos.x - start.x, mate_pos.z - start.z));
                if let Some(bred) = crate::breed::approach(store, key, mate, mate_pos, rng) {
                    events.push(MobEvent::Bred(bred));
                }
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
                    walking = follow_path(store, key, blocks, start, target)
                        .or_else(|| Some(yaw_towards(dx, dz)));
                    speed = VILLAGER_SPEED;
                }
            } else if moving {
                walking = Some(wander_yaw);
            }
            look = look_at(start, f64::from(kind.props().eye_height), &player_positions);
        }

        // The wander goal picks a new interval now and then (passive only).
        if !kind.is_hostile() && mob::state(store, key).is_some_and(|state| state.panic == 0) {
            let wanting = mob::state(store, key).is_some_and(|state| state.moving);
            let timer = mob::state(store, key).map_or(0, |state| state.wander_timer);
            if timer == 0 {
                let (moving, yaw, length) = if rng.next_int(4) == 0 {
                    (false, 0.0, 20 + rng.next_int(40) as u32)
                } else {
                    (true, rng.next_f32() * 360.0, 40 + rng.next_int(80) as u32)
                };
                if let Some(state) = mob::state_mut(store, key) {
                    state.moving = moving;
                    state.wander_yaw = yaw;
                    state.wander_timer = length;
                }
            } else if let Some(state) = mob::state_mut(store, key) {
                state.wander_timer = timer - 1;
            }
            if wanting && !directed && panic_of(store, key) == 0 {
                walking = Some(mob::state(store, key).map_or(0.0, |state| state.wander_yaw));
            }
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

        if mob::state(store, key).is_some_and(|state| state.no_ai) {
            walking = None;
            look = None;
        }
        let mut vel = store.vel(key).unwrap_or_default();
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
            // Jump over a block walked into last tick.
            if was_on_ground && flags.has(Flags::HORIZONTAL_COLLISION) {
                vel.y = kind.jump_strength();
            }
        }
        if !was_on_ground {
            vel.y -= GRAVITY;
        }
        store.set_vel(key, vel);
        physics::move_entity(store, key, vel, blocks);

        // Vertical drag and the friction of the block the mob now stands on.
        let on_ground = store
            .flags(key)
            .is_some_and(|flags| flags.has(Flags::ON_GROUND));
        let mut vel = store.vel(key).unwrap_or_default();
        let friction = if on_ground { GROUND_FRICTION } else { 1.0 };
        vel.x *= AIR_FRICTION * friction;
        vel.z *= AIR_FRICTION * friction;
        if !on_ground {
            vel.y *= AIR_DRAG;
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
            mob::check_despawn(store, key, &player_positions, false, rng);
        }
    }
    events
}

/// Walk speed multiplier for a villager moving to a scheduled destination.
const VILLAGER_SPEED: f64 = 0.8;

fn panic_of(store: &EntityStore, key: crate::EntityKey) -> u16 {
    mob::state(store, key).map_or(0, |state| state.panic)
}

/// Recomputes and follows the block path towards `target`, returning the yaw to walk along.
fn follow_path(
    store: &mut EntityStore,
    key: crate::EntityKey,
    blocks: &impl BlockGetter,
    start: Vec3,
    target: Vec3,
) -> Option<f32> {
    let (mut len, mut index, mut timer, mut path) =
        mob::state(store, key).map_or((0u8, 0u8, 0u16, [[0i32; 3]; path::MAX_PATH]), |state| {
            (
                state.path_len,
                state.path_index,
                state.path_timer,
                state.path,
            )
        });
    if timer == 0 || index >= len {
        let goal = [
            target.x.floor() as i32,
            target.y.floor() as i32,
            target.z.floor() as i32,
        ];
        let found = path::find_path(
            blocks,
            [
                start.x.floor() as i32,
                start.y.floor() as i32,
                start.z.floor() as i32,
            ],
            goal,
            32,
        );
        len = u8::try_from(found.len()).unwrap_or(u8::MAX);
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
        if (dx * dx + dz * dz).sqrt() < 0.6 && (start.y - f64::from(waypoint[1])).abs() < 1.5 {
            index += 1;
            continue;
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
    yaw
}

/// Vanilla enderman teleport: a random walkable spot within eight blocks.
fn blink(
    store: &mut EntityStore,
    key: crate::EntityKey,
    blocks: &impl BlockGetter,
    start: Vec3,
    rng: &mut Rng,
) {
    for _ in 0..8 {
        let x = start.x.floor() as i32 + rng.next_int(17) - 8;
        let z = start.z.floor() as i32 + rng.next_int(17) - 8;
        let y = start.y.floor() as i32 + rng.next_int(5) - 2;
        if path::start_node(blocks, [x, y, z]) == Some([x, y, z]) {
            crate::tracker::teleport(
                store,
                key,
                Vec3::new(f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5),
                Vec3::ZERO,
            );
            return;
        }
    }
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
    fn a_trader_llama_follows_the_trader_that_leads_it_and_vanishes_when_its_time_is_up() {
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
        let state = mob::state_mut(&mut store, llama).unwrap();
        state.leash = Some((holder, 1));
        state.lifetime = 50;
        let mut rng = Rng::new(3);
        for _ in 0..40 {
            tick_with(&mut store, &world, &[player], &mut rng);
        }
        assert!(
            store.pos(llama).unwrap().x < 9.5,
            "the led llama is pulled to its trader"
        );
        for _ in 0..20 {
            tick_with(&mut store, &world, &[player], &mut rng);
        }
        assert!(mob::state(&store, llama).unwrap().is_dead());
        assert!(
            mob::state(&store, llama).unwrap().loot_dropped,
            "it leaves no loot"
        );
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
}
