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
    /// A channeling trident calls lightning on the mob it hits during a thunderstorm.
    pub channeling: bool,
}

/// Where channeling tridents hit a mob since the region last looked.
pub static CHANNELED: std::sync::Mutex<Vec<Vec3>> = std::sync::Mutex::new(Vec::new());

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
        channeling: false,
    });
    // Vanilla arrows do not randomise their start inside the block.
    EntityRecord {
        pos,
        vel,
        flags: Flags::default(),
        data,
    }
}

/// Vanilla `ThrowableProjectile` gravity per tick (snowball, egg, ender pearl, potion).
pub const THROWN_GRAVITY: f64 = 0.03;
/// `ThrownExperienceBottle.getDefaultGravity`.
pub const EXPERIENCE_BOTTLE_GRAVITY: f64 = 0.07;
/// Box size of a thrown snowball, egg, ender pearl or bottle.
pub const THROWN_SIZE: f64 = 0.25;
/// `WindCharge` disappears after five seconds (`AbstractWindCharge.getLifetime`).
pub const WIND_CHARGE_LIFETIME: u32 = 100;
/// How far a wind charge bursts and how hard it pushes.
pub const WIND_CHARGE_RADIUS: f64 = 3.0;
pub const WIND_CHARGE_PUSH: f64 = 1.2;
/// Vanilla wind charges deal one point of damage.
pub const WIND_CHARGE_DAMAGE: f32 = 1.0;

/// Which projectile an entity of this module is, told apart by its entity type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Arrow,
    Snowball,
    Egg,
    EnderPearl,
    /// `minecraft:splash_potion`: arcs and applies its effects where it breaks.
    SplashPotion,
    /// `minecraft:lingering_potion`: arcs and leaves an area-effect cloud where it breaks.
    LingeringPotion,
    /// `minecraft:fireball` (the large one): flies straight and explodes where it lands.
    Fireball,
    /// `minecraft:small_fireball` (blaze, fire charge): flies straight; the region burns what it hits.
    SmallFireball,
    /// `minecraft:dragon_fireball`: flies straight; the region leaves a breath cloud where it lands.
    DragonFireball,
    /// `minecraft:trident`: flies like an arrow for 8 damage; the region drops the item where it ends.
    Trident,
    /// `minecraft:llama_spit`: flies like an arrow for 1 damage.
    LlamaSpit,
    /// `minecraft:experience_bottle`: arcs like a thrown item and drops experience where it lands.
    ExperienceBottle,
    /// `minecraft:wind_charge`: flies straight, then bursts and pushes nearby entities.
    WindCharge,
}

impl Kind {
    fn entity_name(self) -> &'static str {
        match self {
            Self::Arrow => "arrow",
            Self::Snowball => "snowball",
            Self::Egg => "egg",
            Self::EnderPearl => "ender_pearl",
            Self::SplashPotion => "splash_potion",
            Self::LingeringPotion => "lingering_potion",
            Self::Fireball => "fireball",
            Self::SmallFireball => "small_fireball",
            Self::DragonFireball => "dragon_fireball",
            Self::Trident => "trident",
            Self::LlamaSpit => "llama_spit",
            Self::ExperienceBottle => "experience_bottle",
            Self::WindCharge => "wind_charge",
        }
    }

    /// The projectile for a `minecraft:` entity key; arrows are not "thrown".
    pub fn thrown_from_name(name: &str) -> Option<Self> {
        match name.strip_prefix("minecraft:").unwrap_or(name) {
            "snowball" => Some(Self::Snowball),
            "egg" => Some(Self::Egg),
            "ender_pearl" => Some(Self::EnderPearl),
            "splash_potion" => Some(Self::SplashPotion),
            "lingering_potion" => Some(Self::LingeringPotion),
            "fireball" => Some(Self::Fireball),
            "small_fireball" => Some(Self::SmallFireball),
            "experience_bottle" => Some(Self::ExperienceBottle),
            "wind_charge" => Some(Self::WindCharge),
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
        Kind::SplashPotion,
        Kind::LingeringPotion,
        Kind::Fireball,
        Kind::SmallFireball,
        Kind::DragonFireball,
        Kind::Trident,
        Kind::LlamaSpit,
        Kind::ExperienceBottle,
        Kind::WindCharge,
    ]
    .into_iter()
    .find(|kind| kind.entity_type() == data.kind)
}

/// A llama's spit: an arrow in flight for one point of damage.
pub fn new_spit(pos: Vec3, vel: Vec3) -> EntityRecord {
    let mut record = new_arrow(pos, vel, 1.0, None, &mut Rng::new(0));
    record.data.kind = Kind::LlamaSpit.entity_type();
    record.data.width = 0.25;
    record.data.height = 0.25;
    record
}

/// A thrown trident: an arrow in flight with a bigger box and a trident damage.
pub fn new_trident(pos: Vec3, vel: Vec3, owner: Option<Uuid>) -> EntityRecord {
    let mut record = new_arrow(pos, vel, TRIDENT_DAMAGE, owner, &mut Rng::new(0));
    record.data.kind = Kind::Trident.entity_type();
    record.data.width = 0.5;
    record.data.height = 0.5;
    record
}

/// Damage of a thrown trident (`ThrownTrident.BASE_DAMAGE`).
pub const TRIDENT_DAMAGE: f32 = 8.0;
/// A trident is thrown after being drawn this many ticks (`TridentItem.releaseUsing`).
pub const TRIDENT_MIN_DRAW: i32 = 10;
/// Launch speed of a thrown trident.
pub const TRIDENT_SPEED: f64 = 2.5;

/// A new snowball, egg or ender pearl at `pos` flying with `vel`.
pub fn new_thrown(kind: Kind, pos: Vec3, vel: Vec3, owner: Option<Uuid>) -> EntityRecord {
    let mut record = new_arrow(pos, vel, 0.0, owner, &mut Rng::new(0));
    record.data.kind = kind.entity_type();
    record.data.width = THROWN_SIZE;
    record.data.height = THROWN_SIZE;
    record
}

/// Where a thrown projectile ended.
#[derive(Debug, Clone, PartialEq)]
pub struct Landing {
    pub kind: Kind,
    pub owner: Option<Uuid>,
    pub pos: Vec3,
    /// The potion stack carried by a splash or lingering potion.
    pub potion: Option<ferrite_item::ItemStack>,
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
        let potion = data.potion.clone();
        if matches!(kind, Kind::Arrow | Kind::Trident | Kind::LlamaSpit)
            || store
                .flags(key)
                .is_some_and(|flags| flags.has(Flags::REMOVED))
        {
            continue;
        }
        state.age += 1;
        let lifetime = match kind {
            Kind::WindCharge => WIND_CHARGE_LIFETIME,
            _ => LIFETIME,
        };
        if state.age > lifetime {
            remove(store, key);
            continue;
        }
        let mut vel = store.vel(key).unwrap_or_default();
        // A fireball and a wind charge keep their speed and ignore gravity; a thrown bottle
        // falls faster than a snowball; the rest arc like thrown items.
        match kind {
            Kind::Fireball | Kind::SmallFireball | Kind::DragonFireball => {}
            Kind::WindCharge => {
                vel.x *= DRAG;
                vel.z *= DRAG;
            }
            Kind::ExperienceBottle => {
                vel.y -= EXPERIENCE_BOTTLE_GRAVITY;
                vel.x *= DRAG;
                vel.y *= DRAG;
                vel.z *= DRAG;
            }
            _ => {
                vel.y -= THROWN_GRAVITY;
                vel.x *= DRAG;
                vel.y *= DRAG;
                vel.z *= DRAG;
            }
        }
        store.set_vel(key, vel);
        physics::move_entity(store, key, vel, blocks);
        let pos = store.pos(key).unwrap_or_default();
        let land = |store: &mut EntityStore, landings: &mut Vec<Landing>| {
            landings.push(Landing {
                kind,
                owner: state.owner,
                pos,
                potion: potion.clone(),
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

/// A wind charge burst: pushes every nearby mob away from the impact and hurts it for one.
/// Players are handled by the region through `player_hits`, because they are not stored here.
pub fn wind_burst(store: &mut EntityStore, center: Vec3) -> Vec<PlayerHit> {
    let area = AABB::new(
        center.x - WIND_CHARGE_RADIUS,
        center.y - WIND_CHARGE_RADIUS,
        center.z - WIND_CHARGE_RADIUS,
        center.x + WIND_CHARGE_RADIUS,
        center.y + WIND_CHARGE_RADIUS,
        center.z + WIND_CHARGE_RADIUS,
    );
    for key in store.query_vec(area) {
        if !mob::is_mob(store, key) {
            continue;
        }
        let Some(pos) = store.pos(key) else {
            continue;
        };
        let dx = pos.x - center.x;
        let dy = pos.y - center.y;
        let dz = pos.z - center.z;
        let length = (dx * dx + dy * dy + dz * dz).sqrt().max(1.0e-4);
        let mut vel = store.vel(key).unwrap_or_default();
        vel.x += dx / length * WIND_CHARGE_PUSH;
        vel.y += (dy / length).max(0.2) * WIND_CHARGE_PUSH;
        vel.z += dz / length * WIND_CHARGE_PUSH;
        store.set_vel(key, vel);
        if let Some(flags) = store.flags_mut(key) {
            flags.set(Flags::NEEDS_SYNC, true);
        }
        mob::hurt(store, key, WIND_CHARGE_DAMAGE, Some(center));
    }
    Vec::new()
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
    tick_arrows_with_parts(store, blocks, players, &[])
}

/// [`tick_arrows`] with the eight vanilla dragon sub-hitboxes. Arrows only hit a dragon on one of
/// these parts, and non-head parts reduce the damage exactly like `EnderDragon.hurt`.
pub fn tick_arrows_with_parts(
    store: &mut EntityStore,
    blocks: &impl BlockGetter,
    players: &[(Uuid, AABB)],
    parts: &[(EntityKey, Vec<(crate::dragon::DragonPart, AABB)>)],
) -> Vec<PlayerHit> {
    let keys = store.keys().to_vec();
    let mut hits = Vec::new();
    for key in keys {
        let Some(mut arrow) = store
            .data(key)
            .filter(|data| {
                matches!(
                    kind_of(data),
                    Some(Kind::Arrow | Kind::Trident | Kind::LlamaSpit)
                )
            })
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
        let part_hit = parts.iter().find_map(|(target, boxes)| {
            boxes
                .iter()
                .find(|(_, part_box)| part_box.intersects(bbox))
                .map(|(part, _)| (*target, *part))
        });
        let target = part_hit.or_else(|| {
            store
                .query_vec(bbox)
                .into_iter()
                .find(|other| {
                    *other != key
                        && mob::is_mob(store, *other)
                        && !parts.iter().any(|(target, _)| target == other)
                })
                .map(|target| {
                    let part = crate::dragon::DragonPart::Body;
                    (target, part)
                })
        });
        if let Some((target, _part)) = target {
            let part = part_hit.map(|(_, part)| part);
            let damage = part.map_or(arrow.damage, |part| {
                crate::dragon::damage_on_part(part, arrow.damage)
            });
            let hurt = mob::hurt(store, target, damage, None);
            if arrow.channeling
                && let Some(at) = store.pos(target)
            {
                CHANNELED.lock().unwrap_or_else(|e| e.into_inner()).push(at);
            }
            if let Some(owner) = arrow.owner {
                mob::credit(store, target, owner);
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
    fn an_experience_bottle_arcs_faster_and_lands() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let bottle = store.insert(new_thrown(
            Kind::ExperienceBottle,
            Vec3::new(0.5, 70.0, 0.5),
            Vec3::new(0.0, 0.0, 1.0),
            None,
        ));
        tick_thrown(&mut store, &world, &[]);
        let vel = store.vel(bottle).unwrap();
        assert!((vel.y + EXPERIENCE_BOTTLE_GRAVITY * DRAG).abs() < 1.0e-9);
        store.set_vel(bottle, Vec3::new(0.0, -8.0, 0.0));
        let landed = tick_thrown(&mut store, &world, &[]);
        assert_eq!(landed[0].kind, Kind::ExperienceBottle);
        assert!(store.flags(bottle).unwrap().has(Flags::REMOVED));
    }

    #[test]
    fn a_wind_charge_flies_level_and_expires_after_its_lifetime() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let charge = store.insert(new_thrown(
            Kind::WindCharge,
            Vec3::new(0.5, 70.0, 0.5),
            Vec3::new(0.0, 0.0, 1.0),
            None,
        ));
        tick_thrown(&mut store, &world, &[]);
        assert_eq!(store.vel(charge).unwrap().y, 0.0, "no gravity");
        let age = store.data_mut(charge).unwrap().arrow.as_mut().unwrap();
        age.age = WIND_CHARGE_LIFETIME;
        tick_thrown(&mut store, &world, &[]);
        assert!(
            store.flags(charge).unwrap().has(Flags::REMOVED),
            "a wind charge expires after 100 ticks"
        );
    }

    #[test]
    fn a_wind_charge_burst_pushes_and_hurts_a_cow() {
        let mut store = EntityStore::default();
        let cow = store.insert(new_mob(
            MobKind::Cow,
            Vec3::new(2.0, 64.0, 0.0),
            &mut Rng::new(2),
        ));
        wind_burst(&mut store, Vec3::new(0.0, 64.0, 0.0));
        assert!(
            store.vel(cow).unwrap().x > 0.1,
            "pushed away from the burst"
        );
        assert_eq!(
            mob::state(&store, cow).unwrap().health,
            MobKind::Cow.max_health() - WIND_CHARGE_DAMAGE
        );
    }

    #[test]
    fn a_wind_charge_entity_type_is_mapped_from_its_item() {
        assert_eq!(
            Kind::thrown_from_name("minecraft:wind_charge"),
            Some(Kind::WindCharge)
        );
        assert_eq!(
            Kind::thrown_from_name("experience_bottle"),
            Some(Kind::ExperienceBottle)
        );
        assert_eq!(Kind::WindCharge.entity_name(), "wind_charge");
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
    fn a_small_fireball_flies_level_like_a_fireball() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let owner = Uuid::from_u128(0x51);
        let ball = store.insert(new_thrown(
            Kind::SmallFireball,
            Vec3::new(0.5, 70.0, 0.5),
            Vec3::new(0.0, 0.0, 1.0),
            Some(owner),
        ));
        tick_thrown(&mut store, &world, &[]);
        assert_eq!(store.vel(ball).unwrap().y, 0.0, "no gravity");
        assert_eq!(
            kind_of(store.data(ball).unwrap()),
            Some(Kind::SmallFireball)
        );
        store.set_vel(ball, Vec3::new(0.0, -8.0, 0.0));
        let landed = tick_thrown(&mut store, &world, &[]);
        assert_eq!(landed[0].kind, Kind::SmallFireball);
        assert_eq!(landed[0].owner, Some(owner));
    }

    #[test]
    fn a_small_fireball_ignores_its_owner_and_stops_on_another_player() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let owner = Uuid::from_u128(0x51);
        let target = Uuid::from_u128(0x52);
        let ball = store.insert(new_thrown(
            Kind::SmallFireball,
            Vec3::new(0.5, 70.0, 0.5),
            Vec3::new(0.0, 0.0, 2.0),
            Some(owner),
        ));
        let box_ = AABB::new(0.2, 69.0, 0.2, 0.8, 71.0, 40.0);
        let landed = tick_thrown(&mut store, &world, &[(owner, box_), (target, box_)]);
        assert_eq!(landed.len(), 1);
        assert_eq!(landed[0].kind, Kind::SmallFireball);
        assert_eq!(landed[0].owner, Some(owner));
        assert!(store.flags(ball).unwrap().has(Flags::REMOVED));
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
    fn arrows_hit_dragon_parts_and_scale_non_head_damage() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let dragon = store.insert(new_mob(
            MobKind::EnderDragon,
            Vec3::new(0.0, 64.0, 0.0),
            &mut Rng::new(2),
        ));
        store.data_mut(dragon).unwrap().yaw = 0.0;
        let parts = crate::dragon::part_boxes(Vec3::new(0.0, 64.0, 0.0), 0.0).to_vec();
        spawn_arrow(&mut store, Vec3::new(0.0, 67.0, 5.0), Vec3::ZERO);
        tick_arrows_with_parts(&mut store, &world, &[], &[(dragon, parts.clone())]);
        assert_eq!(mob::state(&store, dragon).unwrap().health, 196.0);

        let body = parts
            .iter()
            .find(|(part, _)| *part == crate::dragon::DragonPart::Body)
            .unwrap()
            .1
            .center();
        mob::state_mut(&mut store, dragon).unwrap().hurt_time = 0;
        spawn_arrow(&mut store, body, Vec3::ZERO);
        tick_arrows_with_parts(&mut store, &world, &[], &[(dragon, parts)]);
        assert_eq!(mob::state(&store, dragon).unwrap().health, 194.0);
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
    fn a_trident_hurts_a_mob_like_an_arrow_for_eight() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let cow = store.insert(new_mob(
            MobKind::Cow,
            Vec3::new(0.5, 64.0, 6.0),
            &mut Rng::new(2),
        ));
        let trident = store.insert(new_trident(
            Vec3::new(0.5, 64.9, 0.5),
            Vec3::new(0.0, 0.0, 2.0),
            None,
        ));
        assert_eq!(kind_of(store.data(trident).unwrap()), Some(Kind::Trident));
        for _ in 0..40 {
            tick_arrows(&mut store, &world, &[]);
        }
        let health = mob::state(&store, cow).map_or(0.0, |m| m.health);
        assert_eq!(health, MobKind::Cow.max_health() - TRIDENT_DAMAGE);
        assert!(store.flags(trident).unwrap().has(Flags::REMOVED));
    }

    #[test]
    fn a_channeling_trident_reports_where_it_hit_a_mob() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        store.insert(new_mob(
            MobKind::Cow,
            Vec3::new(0.5, 64.0, 6.0),
            &mut Rng::new(2),
        ));
        let trident = store.insert(new_trident(
            Vec3::new(0.5, 64.9, 0.5),
            Vec3::new(0.0, 0.0, 2.0),
            None,
        ));
        store
            .data_mut(trident)
            .unwrap()
            .arrow
            .as_mut()
            .unwrap()
            .channeling = true;
        CHANNELED.lock().unwrap().clear();
        for _ in 0..40 {
            tick_arrows(&mut store, &world, &[]);
        }
        let hits = CHANNELED.lock().unwrap().clone();
        assert_eq!(hits.len(), 1);
        assert!((hits[0].z - 6.0).abs() < 1.0);
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
