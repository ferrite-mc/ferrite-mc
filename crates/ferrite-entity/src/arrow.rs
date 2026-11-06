//! Arrow projectiles (vanilla `AbstractArrow`): the flight physics, block collision and hit
//! detection used by skeleton bows and player bows.
//!
//! An arrow is an ordinary entity of type `minecraft:arrow` with an [`ArrowState`] carrying the
//! damage and owner; the tracker sends it like any other entity. [`tick_arrows`] advances every
//! arrow against the region's blocks and entities and reports player hits, which the region
//! applies because players are not in the entity store.

use crate::id::EntityKey;
use crate::mob;
use crate::physics::{self, BlockGetter};
use crate::rng::Rng;
use crate::store::{EntityData, EntityRecord, EntityStore, Flags};
use crate::{allocate_id, random_uuid};
use ferrite_math::{AABB, Vec3};
use std::sync::OnceLock;
use uuid::Uuid;

/// Vanilla `AbstractArrow` gravity per tick.
pub const GRAVITY: f64 = 0.05;
/// Vanilla arrow drag (`0.99`).
pub const DRAG: f64 = 0.99;
/// Vanilla `AbstractArrow` despawn age (`1200` ticks).
pub const LIFETIME: u32 = 1200;
/// Vanilla full-draw arrow speed.
pub const SPEED: f64 = 3.0;
pub const WIDTH: f64 = 0.5;
pub const HEIGHT: f64 = 0.5;
/// Ticks before an arrow can hit its shooter.
pub const OWNER_GRACE: u32 = 5;

/// Per-arrow state beside the entity columns.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrowState {
    /// Damage on the next hit.
    pub damage: f32,
    /// Shooter UUID, so an arrow does not hit its owner immediately.
    pub owner: Option<Uuid>,
    pub critical: bool,
    pub age: u32,
}

/// Entity type id of `minecraft:arrow`.
pub fn arrow_type() -> u16 {
    static KIND: OnceLock<u16> = OnceLock::new();
    *KIND.get_or_init(|| {
        ferrite_data::entity_props::by_name("arrow")
            .expect("vanilla has an arrow entity type")
            .id
    })
}

/// A new arrow at `pos` flying with `vel`.
pub fn new_arrow(
    pos: Vec3,
    vel: Vec3,
    damage: f32,
    owner: Option<Uuid>,
    _rng: &mut Rng,
) -> EntityRecord {
    let mut data = EntityData::new(allocate_id(), random_uuid(), arrow_type(), WIDTH, HEIGHT);
    let horizontal = (vel.x * vel.x + vel.z * vel.z).sqrt();
    data.yaw = (-vel.x).atan2(vel.z).to_degrees() as f32;
    data.pitch = (-vel.y).atan2(horizontal).to_degrees() as f32;
    data.head_yaw = data.yaw;
    data.arrow = Some(ArrowState {
        damage,
        owner,
        critical: false,
        age: 0,
    });
    // Vanilla arrows do not randomise their start inside the block.
    EntityRecord {
        pos,
        vel,
        flags: Flags::default(),
        data,
    }
}

/// Vanilla `ThrowableProjectile` gravity per tick (snowball, egg, ender pearl).
pub const THROWN_GRAVITY: f64 = 0.03;
/// Box size of a thrown snowball, egg or ender pearl.
pub const THROWN_SIZE: f64 = 0.25;

/// Which projectile an entity of this module is, told apart by its entity type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Arrow,
    Snowball,
    Egg,
    EnderPearl,
    /// `minecraft:fireball` (the large one): flies straight and explodes where it lands.
    Fireball,
}

impl Kind {
    fn entity_name(self) -> &'static str {
        match self {
            Self::Arrow => "arrow",
            Self::Snowball => "snowball",
            Self::Egg => "egg",
            Self::EnderPearl => "ender_pearl",
            Self::Fireball => "fireball",
        }
    }

    /// The projectile for a `minecraft:` entity key; arrows are not "thrown".
    pub fn thrown_from_name(name: &str) -> Option<Self> {
        match name.strip_prefix("minecraft:").unwrap_or(name) {
            "snowball" => Some(Self::Snowball),
            "egg" => Some(Self::Egg),
            "ender_pearl" => Some(Self::EnderPearl),
            "fireball" => Some(Self::Fireball),
            _ => None,
        }
    }

    pub fn entity_type(self) -> u16 {
        ferrite_data::entity_props::by_name(self.entity_name())
            .expect("vanilla has the projectile entity types")
            .id
    }
}

/// Which projectile `data` is, when its type is one of ours.
pub fn kind_of(data: &EntityData) -> Option<Kind> {
    [
        Kind::Arrow,
        Kind::Snowball,
        Kind::Egg,
        Kind::EnderPearl,
        Kind::Fireball,
    ]
    .into_iter()
    .find(|kind| kind.entity_type() == data.kind)
}

/// A new snowball, egg or ender pearl at `pos` flying with `vel`.
pub fn new_thrown(kind: Kind, pos: Vec3, vel: Vec3, owner: Option<Uuid>) -> EntityRecord {
    let mut record = new_arrow(pos, vel, 0.0, owner, &mut Rng::new(0));
    record.data.kind = kind.entity_type();
    record.data.width = THROWN_SIZE;
    record.data.height = THROWN_SIZE;
    record
}

/// Where a thrown projectile ended.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Landing {
    pub kind: Kind,
    pub owner: Option<Uuid>,
    pub pos: Vec3,
}

/// Advances every snowball, egg and ender pearl one tick and reports where each one landed.
/// Mobs are pushed away (they take no damage); the region teleports pearl owners.
pub fn tick_thrown(
    store: &mut EntityStore,
    blocks: &impl BlockGetter,
    players: &[(Uuid, AABB)],
) -> Vec<Landing> {
    let keys = store.keys().to_vec();
    let mut landings = Vec::new();
    for key in keys {
        let Some(data) = store.data(key) else {
            continue;
        };
        let (Some(mut state), Some(kind)) = (data.arrow, kind_of(data)) else {
            continue;
        };
        if kind == Kind::Arrow
            || store
                .flags(key)
                .is_some_and(|flags| flags.has(Flags::REMOVED))
        {
            continue;
        }
        state.age += 1;
        if state.age > LIFETIME {
            remove(store, key);
            continue;
        }
        let mut vel = store.vel(key).unwrap_or_default();
        // A fireball keeps its speed and ignores gravity; the rest arc like thrown items.
        if kind != Kind::Fireball {
            vel.y -= THROWN_GRAVITY;
            vel.x *= DRAG;
            vel.y *= DRAG;
            vel.z *= DRAG;
        }
        store.set_vel(key, vel);
        physics::move_entity(store, key, vel, blocks);
        let pos = store.pos(key).unwrap_or_default();
        let land = |store: &mut EntityStore, landings: &mut Vec<Landing>| {
            landings.push(Landing {
                kind,
                owner: state.owner,
                pos,
            });
            remove(store, key);
        };
        if store.flags(key).is_some_and(|flags| {
            flags.has(Flags::HORIZONTAL_COLLISION) || flags.has(Flags::VERTICAL_COLLISION)
        }) {
            land(store, &mut landings);
            continue;
        }
        let Some(bbox) = store.bbox(key) else {
            continue;
        };
        let grace = state.age <= OWNER_GRACE;
        if players
            .iter()
            .any(|(uuid, box_)| box_.intersects(bbox) && !(grace && state.owner == Some(*uuid)))
        {
            land(store, &mut landings);
            continue;
        }
        let target = store
            .query_vec(bbox)
            .into_iter()
            .find(|other| *other != key && mob::is_mob(store, *other));
        if let Some(target) = target {
            // Vanilla knockback of a thrown item: 0.4 away from the projectile's heading.
            let length = (vel.x * vel.x + vel.z * vel.z).sqrt().max(1.0e-4);
            let mut push = store.vel(target).unwrap_or_default();
            push.x += vel.x / length * 0.4;
            push.z += vel.z / length * 0.4;
            push.y = push.y.max(0.3);
            store.set_vel(target, push);
            land(store, &mut landings);
            continue;
        }
        if let Some(data) = store.data_mut(key) {
            data.arrow = Some(state);
        }
    }
    landings
}

/// A player an arrow hit; the region applies the damage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerHit {
    pub uuid: Uuid,
    pub damage: f32,
}

/// Mobs a player's arrow killed since the region last looked: shooter and entity type.
pub static KILLS: std::sync::Mutex<Vec<(Uuid, &'static str)>> = std::sync::Mutex::new(Vec::new());

fn remove(store: &mut EntityStore, key: EntityKey) {
    if let Some(flags) = store.flags_mut(key) {
        flags.set(Flags::REMOVED, true);
    }
}

/// Advances every arrow one tick: gravity, drag, block collision and entity/player hits.
///
/// Mobs in `store` are hurt directly (vanilla `AbstractArrow.onHitEntity`), players are
/// returned for the region to damage. An arrow that hits anything is removed.
pub fn tick_arrows(
    store: &mut EntityStore,
    blocks: &impl BlockGetter,
    players: &[(Uuid, AABB)],
) -> Vec<PlayerHit> {
    let keys = store.keys().to_vec();
    let mut hits = Vec::new();
    for key in keys {
        let Some(mut arrow) = store
            .data(key)
            .filter(|data| kind_of(data) == Some(Kind::Arrow))
            .and_then(|data| data.arrow)
        else {
            continue;
        };
        if store
            .flags(key)
            .is_some_and(|flags| flags.has(Flags::REMOVED))
        {
            continue;
        }
        arrow.age += 1;
        if arrow.age > LIFETIME {
            remove(store, key);
            continue;
        }
        let mut vel = store.vel(key).unwrap_or_default();
        vel.y -= GRAVITY;
        vel.x *= DRAG;
        vel.y *= DRAG;
        vel.z *= DRAG;
        store.set_vel(key, vel);
        physics::move_entity(store, key, vel, blocks);
        // An arrow stops in the first block it touches (vanilla sticks; removal is enough for
        // a region-owned projectile).
        if store.flags(key).is_some_and(|flags| {
            flags.has(Flags::HORIZONTAL_COLLISION) || flags.has(Flags::VERTICAL_COLLISION)
        }) {
            remove(store, key);
            continue;
        }
        let Some(bbox) = store.bbox(key) else {
            continue;
        };
        // Vanilla `AbstractArrow.onHitEntity` ignores only its shooter, and only during the
        // first [`OWNER_GRACE`] ticks; every other player in the flight path is hit at once.
        let owner_grace = arrow.age <= OWNER_GRACE;
        if let Some((uuid, _)) = players.iter().find(|(uuid, box_)| {
            box_.intersects(bbox) && !(owner_grace && arrow.owner == Some(*uuid))
        }) {
            hits.push(PlayerHit {
                uuid: *uuid,
                damage: arrow.damage,
            });
            remove(store, key);
            continue;
        }
        let target = store
            .query_vec(bbox)
            .into_iter()
            .find(|other| *other != key && mob::is_mob(store, *other));
        if let Some(target) = target {
            let hurt = mob::hurt(store, target, arrow.damage, None);
            if let Some(owner) = arrow.owner {
                mob::retaliate(store, target, owner);
                if matches!(hurt, Some(mob::Hurt::Died))
                    && let Some(state) = mob::state(store, target)
                {
                    KILLS
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push((owner, state.kind.name()));
                }
            }
            remove(store, key);
            continue;
        }
        if let Some(data) = store.data_mut(key) {
            data.arrow = Some(arrow);
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mob::{MobKind, new_mob};
    use crate::physics::tests::TestWorld;

    fn spawn_arrow(store: &mut EntityStore, pos: Vec3, vel: Vec3) -> EntityKey {
        store.insert(new_arrow(pos, vel, 4.0, None, &mut Rng::new(1)))
    }

    fn spawn_arrow_from(store: &mut EntityStore, pos: Vec3, vel: Vec3, owner: Uuid) -> EntityKey {
        store.insert(new_arrow(pos, vel, 4.0, Some(owner), &mut Rng::new(1)))
    }

    #[test]
    fn a_pearl_lands_where_it_hits_the_floor_and_a_snowball_shoves_a_cow() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let owner = Uuid::from_u128(5);
        let pearl = store.insert(new_thrown(
            Kind::EnderPearl,
            Vec3::new(0.5, 64.4, 0.5),
            Vec3::new(0.0, -1.0, 0.0),
            Some(owner),
        ));
        let landings = tick_thrown(&mut store, &world, &[]);
        assert_eq!(landings.len(), 1);
        assert_eq!(landings[0].kind, Kind::EnderPearl);
        assert_eq!(landings[0].owner, Some(owner));
        assert!(store.flags(pearl).unwrap().has(Flags::REMOVED));

        let cow = store.insert(new_mob(
            MobKind::Cow,
            Vec3::new(0.5, 64.0, 5.0),
            &mut Rng::new(2),
        ));
        store.insert(new_thrown(
            Kind::Snowball,
            Vec3::new(0.5, 64.9, 0.5),
            Vec3::new(0.0, 0.0, 1.5),
            None,
        ));
        let mut landed = Vec::new();
        for _ in 0..20 {
            landed = tick_thrown(&mut store, &world, &[]);
            if !landed.is_empty() {
                break;
            }
        }
        assert_eq!(landed[0].kind, Kind::Snowball);
        assert!(store.vel(cow).unwrap().z > 0.1, "the cow was shoved");
        assert_eq!(
            mob::state(&store, cow).unwrap().health,
            MobKind::Cow.max_health(),
            "snowballs do no damage"
        );
    }

    #[test]
    fn a_fireball_flies_level_and_lands_on_a_wall() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let fireball = store.insert(new_thrown(
            Kind::Fireball,
            Vec3::new(0.5, 70.0, 0.5),
            Vec3::new(0.0, 0.0, 1.0),
            None,
        ));
        tick_thrown(&mut store, &world, &[]);
        assert_eq!(store.vel(fireball).unwrap().y, 0.0, "no gravity");
        assert_eq!(kind_of(store.data(fireball).unwrap()), Some(Kind::Fireball));
        store.set_vel(fireball, Vec3::new(0.0, -8.0, 0.0));
        let landed = tick_thrown(&mut store, &world, &[]);
        assert_eq!(landed[0].kind, Kind::Fireball);
    }

    #[test]
    fn arrows_fall_with_gravity_and_stick_in_blocks() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        // One step above the floor, falling: the next tick sticks in it.
        let key = spawn_arrow(
            &mut store,
            Vec3::new(0.5, 64.6, 0.5),
            Vec3::new(0.0, -1.0, 0.0),
        );
        tick_arrows(&mut store, &world, &[]);
        assert!(
            store.flags(key).unwrap().has(Flags::REMOVED),
            "an arrow flying into a floor sticks"
        );
    }

    #[test]
    fn a_players_arrow_that_kills_a_mob_is_credited_to_the_shooter() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let cow = store.insert(new_mob(
            MobKind::Cow,
            Vec3::new(0.5, 64.0, 6.0),
            &mut Rng::new(2),
        ));
        let shooter = Uuid::from_u128(0xa11ce);
        store.insert(new_arrow(
            Vec3::new(0.5, 64.9, 0.5),
            Vec3::new(0.0, 0.0, 2.0),
            100.0,
            Some(shooter),
            &mut Rng::new(1),
        ));
        for _ in 0..40 {
            tick_arrows(&mut store, &world, &[]);
        }
        assert!(mob::state(&store, cow).is_some_and(|mob| mob.is_dead()));
        let kills = KILLS.lock().unwrap();
        assert!(kills.contains(&(shooter, "minecraft:cow")));
    }

    #[test]
    fn an_arrow_hurts_a_mob_it_passes_through() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let cow = store.insert(new_mob(
            MobKind::Cow,
            Vec3::new(0.5, 64.0, 6.0),
            &mut Rng::new(2),
        ));
        spawn_arrow(
            &mut store,
            Vec3::new(0.5, 64.9, 0.5),
            Vec3::new(0.0, 0.0, 2.0),
        );
        for _ in 0..40 {
            tick_arrows(&mut store, &world, &[]);
            if mob::state(&store, cow).is_none_or(|mob| mob.health < MobKind::Cow.max_health()) {
                break;
            }
        }
        assert!(
            mob::state(&store, cow).unwrap().health < MobKind::Cow.max_health(),
            "arrow hit the cow"
        );
    }

    #[test]
    fn arrows_hit_a_player_who_is_not_the_shooter_at_once() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let shooter = Uuid::from_u128(1);
        let target = Uuid::from_u128(2);
        let key = spawn_arrow_from(
            &mut store,
            Vec3::new(0.5, 68.0, 0.5),
            Vec3::new(0.0, 0.0, 2.0),
            shooter,
        );
        // The box starts behind the arrow and covers its first step, so the grace must not
        // apply to a player who is not the shooter.
        let box_ = AABB::new(0.2, 66.0, 1.0, 0.8, 70.0, 40.0);
        assert_eq!(
            tick_arrows(&mut store, &world, &[(target, box_)]),
            [PlayerHit {
                uuid: target,
                damage: 4.0
            }]
        );
        assert!(store.flags(key).unwrap().has(Flags::REMOVED));
    }

    #[test]
    fn arrows_ignore_their_shooter_for_the_grace() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let shooter = Uuid::from_u128(7);
        // The box spans the arrow's whole flight, so the owner grace is what delays the hit.
        let box_ = AABB::new(0.2, 64.0, 0.2, 0.8, 70.0, 40.0);
        let key = spawn_arrow_from(
            &mut store,
            Vec3::new(0.5, 68.0, 0.5),
            Vec3::new(0.0, 0.0, 0.2),
            shooter,
        );
        for _ in 0..OWNER_GRACE {
            assert!(tick_arrows(&mut store, &world, &[(shooter, box_)]).is_empty());
            assert!(!store.flags(key).unwrap().has(Flags::REMOVED));
        }
        assert_eq!(
            tick_arrows(&mut store, &world, &[(shooter, box_)]),
            [PlayerHit {
                uuid: shooter,
                damage: 4.0
            }]
        );
    }
}
