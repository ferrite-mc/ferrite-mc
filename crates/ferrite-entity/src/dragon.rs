//! The ender dragon's flight: a slow circle above the main island.
//!
//! This is the holding pattern of vanilla's phase machine, without strafing, charging or
//! perching. It is a pure function of the dragon's age, so it is deterministic and cheap.

use crate::ai::{MobEvent, Target};
use crate::id::{EntityKey, allocate_id, random_uuid};
use crate::mob;
use crate::store::{EntityData, EntityRecord, EntityStore, Flags};
use ferrite_math::{AABB, Vec3};

/// Centre of the circle: the end's world origin.
pub const CENTER: [f64; 2] = [0.0, 0.0];
/// Radius of the circle in blocks.
pub const RADIUS: f64 = 40.0;
/// Average height of the circle and how far the dragon bobs around it.
pub const HEIGHT: f64 = 90.0;
pub const BOB: f64 = 6.0;
/// Angular speed in radians per tick (about 0.6 blocks per tick at the radius above).
pub const ANGULAR_SPEED: f64 = 0.015;

/// Where the dragon is `age` ticks after it started to fly, and the yaw it faces.
pub fn position_at(age: u32) -> (Vec3, f32) {
    let angle = f64::from(age) * ANGULAR_SPEED;
    let pos = Vec3::new(
        CENTER[0] + RADIUS * angle.cos(),
        HEIGHT + BOB * (f64::from(age) * 0.05).sin(),
        CENTER[1] + RADIUS * angle.sin(),
    );
    // Tangent of the circle: d/dangle = (-sin, cos).
    let (dx, dz) = (-angle.sin(), angle.cos());
    let yaw = (-dx).atan2(dz).to_degrees() as f32;
    (pos, yaw)
}

/// The dragon never moves faster than this many blocks per tick.
pub const MAX_SPEED: f64 = 1.2;
/// A dive at the nearest player starts every `CHARGE_PERIOD` ticks, lasts `CHARGE_TICKS` and
/// only happens for players within `CHARGE_RANGE` blocks.
pub const CHARGE_PERIOD: u32 = 600;
pub const CHARGE_TICKS: u32 = 80;
pub const CHARGE_RANGE: f64 = 100.0;
/// Players closer than this to the dragon's body are hit (wings and body, `knockBack`).
pub const CONTACT_RANGE: f64 = 5.0;
/// Damage of one hit and the ticks between hits.
pub const CONTACT_DAMAGE: f32 = 6.0;
pub const CONTACT_COOLDOWN: u16 = 20;
/// Every `PERCH_PERIOD` ticks the dragon lands on the exit portal for `PERCH_TICKS` and breathes
/// a cloud every `BREATH_INTERVAL` ticks (vanilla perch phases, without the sitting animation).
pub const PERCH_PERIOD: u32 = 1500;
pub const PERCH_TICKS: u32 = 200;
pub const PERCH_SPOT: Vec3 = Vec3::new(0.0, 68.0, 0.0);
pub const BREATH_INTERVAL: u32 = 20;
/// A breath cloud lasts 30 seconds, reaches 3 blocks and hurts everyone in it every 20 ticks.
/// shortcut: the cloud keeps its radius instead of shrinking, and hurts for a flat 6.
pub const BREATH_TICKS: u32 = 600;
pub const BREATH_RADIUS: f64 = 3.0;
pub const BREATH_HIT_INTERVAL: u32 = 20;
pub const BREATH_DAMAGE: f32 = 6.0;
/// The dragon smashes blocks every this many ticks.
pub const SMASH_INTERVAL: u64 = 3;
/// A fireball is spat at the nearest player in [`FIREBALL_RANGE`] every `FIREBALL_PERIOD` ticks.
pub const FIREBALL_PERIOD: u32 = 400;
pub const FIREBALL_RANGE: f64 = 64.0;
pub const FIREBALL_SPEED: f64 = 1.0;
/// The dragon smashes every block inside its body except these (`#minecraft:dragon_immune`).
pub const IMMUNE: &[&str] = &[
    "minecraft:barrier",
    "minecraft:bedrock",
    "minecraft:end_portal",
    "minecraft:end_portal_frame",
    "minecraft:end_gateway",
    "minecraft:command_block",
    "minecraft:repeating_command_block",
    "minecraft:chain_command_block",
    "minecraft:structure_block",
    "minecraft:jigsaw",
    "minecraft:moving_piston",
    "minecraft:obsidian",
    "minecraft:crying_obsidian",
    "minecraft:end_stone",
    "minecraft:iron_bars",
    "minecraft:respawn_anchor",
    "minecraft:reinforced_deepslate",
    "minecraft:test_block",
    "minecraft:test_instance_block",
];
/// Crystals within this many blocks heal the dragon (`EnderDragon.checkCrystals`).
pub const HEAL_RANGE: f64 = 32.0;
/// The dragon heals this much every [`HEAL_INTERVAL`] ticks while a crystal is in range.
pub const HEAL_INTERVAL: u32 = 10;
pub const HEAL_AMOUNT: f32 = 1.0;

/// An end crystal standing at `pos` (the block position above its base, centred).
pub fn new_crystal(pos: Vec3) -> EntityRecord {
    let props = ferrite_data::entity_props::by_name("minecraft:end_crystal")
        .expect("the vanilla data knows the end crystal");
    EntityRecord {
        pos,
        vel: Vec3::ZERO,
        flags: Flags::default(),
        data: EntityData::new(
            allocate_id(),
            random_uuid(),
            props.id,
            f64::from(props.width),
            f64::from(props.height),
        ),
    }
}

/// The entity type id of the end crystal.
pub fn crystal_type() -> u16 {
    ferrite_data::entity_props::by_name("minecraft:end_crystal").map_or(u16::MAX, |p| p.id)
}

/// Whether `key` is a standing end crystal.
pub fn is_crystal(store: &EntityStore, key: EntityKey) -> bool {
    store
        .data(key)
        .is_some_and(|data| data.kind == crystal_type())
        && store
            .flags(key)
            .is_some_and(|flags| !flags.has(Flags::REMOVED))
}

/// Every end crystal an arrow is inside of: crystal, arrow and the crystal position.
pub fn arrows_in_crystals(store: &EntityStore) -> Vec<(EntityKey, EntityKey, Vec3)> {
    let arrow_type = ferrite_data::entity_props::by_name("minecraft:arrow").map(|p| p.id);
    let mut hits = Vec::new();
    for key in store.keys().to_vec() {
        if !is_crystal(store, key) {
            continue;
        }
        let (Some(bbox), Some(pos)) = (store.bbox(key), store.pos(key)) else {
            continue;
        };
        let arrow = store.query_vec(bbox).into_iter().find(|&other| {
            store.data(other).map(|data| data.kind) == arrow_type
                && store
                    .flags(other)
                    .is_some_and(|flags| !flags.has(Flags::REMOVED))
        });
        if let Some(arrow) = arrow {
            hits.push((key, arrow, pos));
        }
    }
    hits
}

/// Lets every living dragon of the store regain health from crystals within [`HEAL_RANGE`].
pub fn heal_from_crystals(store: &mut EntityStore) {
    for key in store.keys().to_vec() {
        let Some(state) = mob::state(store, key) else {
            continue;
        };
        if state.kind != mob::MobKind::EnderDragon
            || state.is_dead()
            || !state.wander_timer.is_multiple_of(HEAL_INTERVAL)
        {
            continue;
        }
        let Some(pos) = store.pos(key) else { continue };
        let area = AABB::new(
            pos.x - HEAL_RANGE,
            pos.y - HEAL_RANGE,
            pos.z - HEAL_RANGE,
            pos.x + HEAL_RANGE,
            pos.y + HEAL_RANGE,
            pos.z + HEAL_RANGE,
        );
        let near = store
            .query_vec(area)
            .into_iter()
            .any(|other| is_crystal(store, other));
        if near && let Some(state) = mob::state_mut(store, key) {
            state.health = (state.health + HEAL_AMOUNT).min(state.kind.max_health());
        }
    }
}

/// Advances one dragon by a tick. The age lives in the mob's `wander_timer`; hits on players
/// that get in its way are reported as melee events.
pub fn fly(
    store: &mut EntityStore,
    key: EntityKey,
    players: &[Target],
    events: &mut Vec<MobEvent>,
) {
    let age = match mob::state_mut(store, key) {
        Some(state) => {
            state.wander_timer = state.wander_timer.wrapping_add(1);
            state.attack_cooldown = state.attack_cooldown.saturating_sub(1);
            state.wander_timer
        }
        None => return,
    };
    let previous = store.pos(key).unwrap_or_default();
    let (circle, circle_yaw) = position_at(age);
    // Every `CHARGE_PERIOD` ticks the dragon dives at the nearest player for a while, then
    // rejoins the circle; between those it never moves faster than `MAX_SPEED`.
    let perching = age % PERCH_PERIOD >= PERCH_PERIOD - PERCH_TICKS;
    let charging = !perching && age % CHARGE_PERIOD >= CHARGE_PERIOD - CHARGE_TICKS;
    let target = players
        .iter()
        .filter(|target| target.pos.distance_to_sqr(previous) <= CHARGE_RANGE * CHARGE_RANGE)
        .min_by(|a, b| {
            a.pos
                .distance_to_sqr(previous)
                .total_cmp(&b.pos.distance_to_sqr(previous))
        });
    let goal = match target {
        _ if perching => PERCH_SPOT,
        Some(target) if charging => target.pos,
        _ => circle,
    };
    let step = goal - previous;
    let length = (step.x * step.x + step.y * step.y + step.z * step.z).sqrt();
    let pos = if length <= MAX_SPEED {
        goal
    } else {
        previous
            + Vec3::new(
                step.x / length * MAX_SPEED,
                step.y / length * MAX_SPEED,
                step.z / length * MAX_SPEED,
            )
    };
    let yaw = if length > 0.5 {
        (-step.x).atan2(step.z).to_degrees() as f32
    } else {
        circle_yaw
    };
    store.set_pos(key, pos);
    store.set_vel(key, pos - previous);
    // Strafing: a fireball at the nearest player now and then, from the head.
    if age % FIREBALL_PERIOD == FIREBALL_PERIOD / 2
        && !perching
        && let Some(target) = players
            .iter()
            .filter(|t| t.pos.distance_to_sqr(pos) <= FIREBALL_RANGE * FIREBALL_RANGE)
            .min_by(|a, b| {
                a.pos
                    .distance_to_sqr(pos)
                    .total_cmp(&b.pos.distance_to_sqr(pos))
            })
    {
        let to = target.pos - pos;
        let length = (to.x * to.x + to.y * to.y + to.z * to.z).sqrt().max(1.0e-4);
        events.push(MobEvent::DragonFireball {
            from: pos,
            direction: Vec3::new(to.x / length, to.y / length, to.z / length),
        });
    }
    // Perched on the portal it breathes a cloud at its feet.
    if perching && pos.distance_to_sqr(PERCH_SPOT) < 16.0 && age.is_multiple_of(BREATH_INTERVAL) {
        events.push(MobEvent::Breath {
            pos: Vec3::new(pos.x, PERCH_SPOT.y - 3.0, pos.z),
        });
    }
    if let Some(data) = store.data_mut(key) {
        data.yaw = yaw;
        data.head_yaw = yaw;
        data.pitch = 0.0;
    }
    if mob::state(store, key).is_some_and(|state| state.attack_cooldown == 0)
        && let Some(target) = players
            .iter()
            .find(|target| target.pos.distance_to_sqr(pos) <= CONTACT_RANGE * CONTACT_RANGE)
    {
        events.push(MobEvent::Melee {
            player: target.uuid,
            damage: CONTACT_DAMAGE,
            from: pos,
            attacker: key,
        });
        if let Some(state) = mob::state_mut(store, key) {
            state.attack_cooldown = CONTACT_COOLDOWN;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{rng::Rng, store::EntityStore};

    #[test]
    fn the_dragon_circles_the_island_at_a_steady_radius() {
        for age in (0..2000).step_by(37) {
            let (pos, _) = position_at(age);
            let r = pos.x.hypot(pos.z);
            assert!((r - RADIUS).abs() < 1e-6, "age {age}: radius {r}");
            assert!((pos.y - HEIGHT).abs() <= BOB + 1e-6);
        }
        let (start, _) = position_at(0);
        assert_eq!((start.x, start.z), (CENTER[0] + RADIUS, CENTER[1]));
    }

    #[test]
    fn it_faces_where_it_flies() {
        let (a, yaw) = position_at(100);
        let (b, _) = position_at(101);
        let (dx, dz) = (b.x - a.x, b.z - a.z);
        let flown = (-dx).atan2(dz).to_degrees() as f32;
        let mut diff = (flown - yaw).abs() % 360.0;
        if diff > 180.0 {
            diff = 360.0 - diff;
        }
        assert!(diff < 2.0, "yaw {yaw} vs heading {flown}");
    }

    #[test]
    fn crystals_heal_the_dragon_and_a_player_in_its_way_is_hit() {
        let mut rng = Rng::new(1);
        let mut store = EntityStore::new(4);
        let (spot, _) = position_at(1);
        let key = store.insert(mob::new_mob(mob::MobKind::EnderDragon, spot, &mut rng));
        let mut events = Vec::new();
        fly(&mut store, key, &[], &mut events);
        mob::state_mut(&mut store, key).unwrap().health = 100.0;
        // No crystal: no healing, whatever the age.
        for _ in 0..30 {
            fly(&mut store, key, &[], &mut events);
            heal_from_crystals(&mut store);
        }
        assert_eq!(mob::state(&store, key).unwrap().health, 100.0);
        let here = store.pos(key).unwrap();
        store.insert(new_crystal(Vec3::new(here.x + 10.0, here.y, here.z)));
        for _ in 0..30 {
            fly(&mut store, key, &[], &mut events);
            heal_from_crystals(&mut store);
        }
        assert_eq!(mob::state(&store, key).unwrap().health, 103.0);
        // A player right under the dragon is hit once per cooldown.
        for _ in 0..21 {
            let target = Target {
                uuid: uuid::Uuid::from_u128(7),
                pos: store.pos(key).unwrap(),
            };
            fly(&mut store, key, &[target], &mut events);
        }
        let hits = events
            .iter()
            .filter(|e| matches!(e, MobEvent::Melee { damage, .. } if *damage == CONTACT_DAMAGE))
            .count();
        assert_eq!(hits, 2);
    }

    #[test]
    fn the_dragon_dives_at_a_player_and_comes_back_to_the_circle() {
        let mut rng = Rng::new(1);
        let mut store = EntityStore::new(4);
        let (spot, _) = position_at(0);
        let key = store.insert(mob::new_mob(mob::MobKind::EnderDragon, spot, &mut rng));
        let player = Target {
            uuid: uuid::Uuid::from_u128(9),
            pos: Vec3::new(10.0, 70.0, 10.0),
        };
        let mut events = Vec::new();
        let mut closest = f64::MAX;
        for _ in 0..(CHARGE_PERIOD + 200) {
            fly(&mut store, key, &[player], &mut events);
            closest = closest.min(store.pos(key).unwrap().distance_to_sqr(player.pos));
        }
        assert!(
            closest.sqrt() < CONTACT_RANGE,
            "it got within {}",
            closest.sqrt()
        );
        assert!(!events.is_empty(), "the dive hits the player");
        // Without a player it holds the circle.
        let mut calm = EntityStore::new(4);
        let key = calm.insert(mob::new_mob(mob::MobKind::EnderDragon, spot, &mut rng));
        for _ in 0..(CHARGE_PERIOD + 200) {
            fly(&mut calm, key, &[], &mut Vec::new());
        }
        let age = CHARGE_PERIOD + 200;
        assert!(calm.pos(key).unwrap().distance_to_sqr(position_at(age).0) < 1e-9);
    }

    #[test]
    fn an_arrow_inside_a_crystal_is_found() {
        let mut store = EntityStore::new(4);
        let crystal = store.insert(new_crystal(Vec3::new(5.5, 100.0, 5.5)));
        assert!(arrows_in_crystals(&store).is_empty());
        let mut rng = Rng::new(1);
        let arrow = store.insert(crate::arrow::new_arrow(
            Vec3::new(5.5, 100.5, 5.5),
            Vec3::ZERO,
            2.0,
            None,
            &mut rng,
        ));
        let hits = arrows_in_crystals(&store);
        assert_eq!(hits.len(), 1);
        assert_eq!((hits[0].0, hits[0].1), (crystal, arrow));
        store.flags_mut(arrow).unwrap().set(Flags::REMOVED, true);
        assert!(
            arrows_in_crystals(&store).is_empty(),
            "a spent arrow breaks nothing"
        );
    }

    #[test]
    fn a_spawned_dragon_has_vanilla_health_and_moves_each_tick() {
        let mut rng = Rng::new(1);
        let mut store = EntityStore::new(4);
        let record = mob::new_mob(
            mob::MobKind::EnderDragon,
            Vec3::new(0.0, 80.0, 0.0),
            &mut rng,
        );
        let key = store.insert(record);
        assert_eq!(mob::state(&store, key).unwrap().health, 200.0);
        let before = store.pos(key).unwrap();
        let mut events = Vec::new();
        fly(&mut store, key, &[], &mut events);
        fly(&mut store, key, &[], &mut events);
        let after = store.pos(key).unwrap();
        assert_ne!(before, after);
        assert!(mob::is_mob(&store, key));
        // It can be killed like any mob.
        assert!(matches!(
            mob::hurt(&mut store, key, 500.0, None),
            Some(mob::Hurt::Died)
        ));
    }

    #[test]
    fn it_spits_fireballs_and_breathes_while_perched() {
        let mut rng = Rng::new(1);
        let mut store = EntityStore::new(4);
        let (spot, _) = position_at(0);
        let key = store.insert(mob::new_mob(mob::MobKind::EnderDragon, spot, &mut rng));
        let player = Target {
            uuid: uuid::Uuid::from_u128(3),
            pos: Vec3::new(30.0, 90.0, 0.0),
        };
        let mut events = Vec::new();
        for _ in 0..PERCH_PERIOD {
            fly(&mut store, key, &[player], &mut events);
        }
        assert!(
            events
                .iter()
                .any(|e| matches!(e, MobEvent::DragonFireball { .. }))
        );
        assert!(events.iter().any(|e| matches!(e, MobEvent::Breath { .. })));
    }
}
