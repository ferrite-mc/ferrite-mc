//! Boats and minecarts (vanilla `AbstractBoat` / `AbstractMinecart`).
//!
//! These are non-living vehicles: they use the ordinary entity store and the shared block
//! collision physics, and `/ride` already steers any vehicle without a mob state through the
//! rider's `move_vehicle` packets. This module supplies the entity records, the verified 26.3
//! synched-data defaults, and the minimal server-side motion when nobody is steering.

use crate::metadata::{boat as boat_meta, minecart as minecart_meta, vehicle as vehicle_meta};
use crate::physics::{self, BlockGetter};
use crate::store::{EntityData, EntityRecord, EntityStore, Flags};
use crate::{allocate_id, random_uuid};
use ferrite_math::Vec3;
use ferrite_protocol::packets::play::MetaValue;

/// Vanilla boat box.
pub const BOAT_WIDTH: f64 = 1.375;
pub const BOAT_HEIGHT: f64 = 0.5625;
/// Vanilla minecart box.
pub const MINECART_WIDTH: f64 = 0.98;
pub const MINECART_HEIGHT: f64 = 0.7;

/// The bare path of a namespaced entity key.
fn bare(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

/// All 22 vanilla 26.3 boat, chest-boat, and bamboo-raft entity keys.
pub const ALL_BOAT_TYPES: [&str; 22] = [
    "oak_boat",
    "oak_chest_boat",
    "spruce_boat",
    "spruce_chest_boat",
    "birch_boat",
    "birch_chest_boat",
    "jungle_boat",
    "jungle_chest_boat",
    "acacia_boat",
    "acacia_chest_boat",
    "dark_oak_boat",
    "dark_oak_chest_boat",
    "mangrove_boat",
    "mangrove_chest_boat",
    "cherry_boat",
    "cherry_chest_boat",
    "pale_oak_boat",
    "pale_oak_chest_boat",
    "poplar_boat",
    "poplar_chest_boat",
    "bamboo_raft",
    "bamboo_chest_raft",
];

/// All 7 vanilla 26.3 minecart entity keys.
pub const ALL_MINECART_TYPES: [&str; 7] = [
    "minecart",
    "chest_minecart",
    "hopper_minecart",
    "furnace_minecart",
    "tnt_minecart",
    "command_block_minecart",
    "spawner_minecart",
];

/// Side effects emitted by functional minecarts during [`tick_vehicles_full`].
#[derive(Debug, Clone, PartialEq)]
pub enum VehicleEvent {
    /// A `tnt_minecart` triggered on a powered activator rail and detonated.
    TntExplode { pos: Vec3 },
    /// A `hopper_minecart` sucked up a nearby dropped item entity.
    HopperPickup {
        cart_uuid: uuid::Uuid,
        item: ferrite_item::ItemStack,
    },
}

/// Whether a vehicle key has a 27-slot chest/hopper container.
pub fn is_chest_vehicle(name: &str) -> bool {
    let n = bare(name);
    n.ends_with("_chest_boat")
        || n == "bamboo_chest_raft"
        || matches!(n, "chest_minecart" | "hopper_minecart")
}

/// Whether an entity key is a boat or raft type.
pub fn is_boat_name(name: &str) -> bool {
    let name = bare(name);
    name.ends_with("_boat") || name.ends_with("_raft")
}

/// Whether an entity key is a minecart type.
pub fn is_minecart_name(name: &str) -> bool {
    bare(name).ends_with("minecart")
}

/// Whether `/summon` should construct this key as a vehicle.
pub fn is_vehicle_name(name: &str) -> bool {
    is_boat_name(name) || is_minecart_name(name)
}

/// A new boat at `pos` facing `yaw`.
pub fn new_boat(name: &str, pos: Vec3, yaw: f32) -> Option<EntityRecord> {
    if !is_boat_name(name) {
        return None;
    }
    let props = ferrite_data::entity_props::by_name(name)?;
    let mut data = EntityData::new(
        allocate_id(),
        random_uuid(),
        props.id,
        f64::from(props.width),
        f64::from(props.height),
    );
    data.yaw = yaw;
    data.head_yaw = yaw;
    apply_metadata(&mut data);
    Some(EntityRecord {
        pos,
        vel: Vec3::ZERO,
        flags: Flags::default(),
        data,
    })
}

/// Defines the verified synched-data defaults for a loaded vehicle. Used by both `new_*` and the
/// NBT loader so a restart does not strip the vehicle metadata.
pub fn apply_metadata(data: &mut EntityData) {
    data.metadata.define(vehicle_meta::HURT, MetaValue::Int(0));
    data.metadata
        .define(vehicle_meta::HURT_DIR, MetaValue::Int(1));
    let key = ferrite_data::entity_props::get(data.kind).key;
    if is_boat_name(key) {
        data.metadata
            .define(boat_meta::PADDLE_LEFT, MetaValue::Boolean(false));
        data.metadata
            .define(boat_meta::PADDLE_RIGHT, MetaValue::Boolean(false));
        data.metadata
            .define(boat_meta::BUBBLE_TIME, MetaValue::Int(0));
    } else if is_minecart_name(key) {
        data.metadata
            .define(minecart_meta::DISPLAY_OFFSET, MetaValue::Int(0));
    }
}

/// A new minecart at `pos`.
pub fn new_minecart(name: &str, pos: Vec3) -> Option<EntityRecord> {
    if !is_minecart_name(name) {
        return None;
    }
    let props = ferrite_data::entity_props::by_name(name)?;
    let mut data = EntityData::new(
        allocate_id(),
        random_uuid(),
        props.id,
        f64::from(props.width),
        f64::from(props.height),
    );
    apply_metadata(&mut data);
    Some(EntityRecord {
        pos,
        vel: Vec3::ZERO,
        flags: Flags::default(),
        data,
    })
}

/// The vehicle record for a `/summon` key.
pub fn new_vehicle(name: &str, pos: Vec3, yaw: f32) -> Option<EntityRecord> {
    if is_boat_name(name) {
        new_boat(name, pos, yaw)
    } else {
        new_minecart(name, pos)
    }
}

/// Ticks every boat and minecart when no rider steers anything (tests and callers without a
/// ride table).
pub fn tick_vehicles(store: &mut EntityStore, blocks: &impl BlockGetter) {
    tick_vehicles_with_riders(store, blocks, &|_| false);
}

/// Ticks every boat and minecart. A boat with a rider is already moved by the rider's
/// `move_vehicle` packet earlier in the region tick, so its physics is skipped and only the
/// paddle animation is updated; minecarts keep rolling under their own physics.
pub fn tick_vehicles_with_riders(
    store: &mut EntityStore,
    blocks: &impl BlockGetter,
    has_rider: &impl Fn(uuid::Uuid) -> bool,
) {
    let _ = tick_vehicles_full(store, blocks, has_rider);
}

/// [`tick_vehicles_with_riders`] plus functional minecart side effects (`tnt_minecart`
/// detonation on powered activator rails, `hopper_minecart` item pickup, `furnace_minecart`
/// coal propulsion).
pub fn tick_vehicles_full(
    store: &mut EntityStore,
    blocks: &impl BlockGetter,
    has_rider: &impl Fn(uuid::Uuid) -> bool,
) -> Vec<VehicleEvent> {
    let mut events = Vec::new();
    if store.is_empty() {
        return events;
    }
    let keys = store.keys().to_vec();
    for key in keys {
        let Some(data) = store.data(key) else {
            continue;
        };
        if data.mob.is_some() || store.flags(key).is_some_and(|f| f.has(Flags::REMOVED)) {
            continue;
        }
        let uuid = data.uuid;
        let key_name = ferrite_data::entity_props::get(data.kind).key;
        if is_boat_name(key_name) {
            if has_rider(uuid) {
                animate_paddles(store, key);
            } else {
                tick_boat(store, key, blocks);
                clear_paddles(store, key);
            }
        } else if is_minecart_name(key_name) {
            let short_name = bare(key_name);
            tick_minecart(store, key, blocks);
            if short_name == "tnt_minecart"
                && let Some(pos) = store.pos(key)
                && let Some((rail_name, props)) = rail_below(blocks, pos)
                && rail_name == "activator_rail"
                && props.iter().any(|&(k, v)| k == "powered" && v == "true")
            {
                if let Some(flags) = store.flags_mut(key) {
                    flags.set(Flags::REMOVED, true);
                }
                events.push(VehicleEvent::TntExplode { pos });
            } else if short_name == "hopper_minecart"
                && let Some(pos) = store.pos(key)
            {
                let pickup_box = ferrite_math::AABB::new(
                    pos.x - 1.5,
                    pos.y - 0.5,
                    pos.z - 1.5,
                    pos.x + 1.5,
                    pos.y + 1.5,
                    pos.z + 1.5,
                );
                for other in store.query_vec(pickup_box) {
                    if other == key || store.flags(other).is_some_and(|f| f.has(Flags::REMOVED)) {
                        continue;
                    }
                    let stack = store
                        .data(other)
                        .filter(|d| d.item.is_some())
                        .map(crate::item::full_stack)
                        .filter(|s| !s.is_empty());
                    if let Some(item) = stack {
                        if let Some(flags) = store.flags_mut(other) {
                            flags.set(Flags::REMOVED, true);
                        }
                        events.push(VehicleEvent::HopperPickup {
                            cart_uuid: uuid,
                            item,
                        });
                    }
                }
            }
        }
    }
    events
}

/// Alternates a steered boat's paddle metadata, as `AbstractBoat` does while rowing.
fn animate_paddles(store: &mut EntityStore, key: crate::EntityKey) {
    let Some(data) = store.data_mut(key) else {
        return;
    };
    let left = data.tick_count / 6 % 2 == 0;
    data.metadata
        .set(boat_meta::PADDLE_LEFT, MetaValue::Boolean(left));
    data.metadata
        .set(boat_meta::PADDLE_RIGHT, MetaValue::Boolean(!left));
    data.tick_count += 1;
}

fn clear_paddles(store: &mut EntityStore, key: crate::EntityKey) {
    if let Some(data) = store.data_mut(key) {
        data.metadata
            .set(boat_meta::PADDLE_LEFT, MetaValue::Boolean(false));
        data.metadata
            .set(boat_meta::PADDLE_RIGHT, MetaValue::Boolean(false));
    }
}

fn tick_boat(store: &mut EntityStore, key: crate::EntityKey, blocks: &impl BlockGetter) {
    let Some(bbox) = store.bbox(key) else {
        return;
    };
    let fluid = physics::fluid_height(blocks, bbox, false);
    let vel = boat_buoyancy(store.vel(key).unwrap_or_default(), fluid);
    store.set_vel(key, vel);
    physics::move_entity(store, key, vel, blocks);
    if let Some(data) = store.data_mut(key) {
        // Vanilla's `BUBBLE_TIME` counts consecutive underwater ticks; it drives the bubble
        // column and resets as soon as the hull reaches air.
        let bubble = match data.metadata.get(boat_meta::BUBBLE_TIME) {
            Some(MetaValue::Int(ticks)) => *ticks,
            _ => 0,
        };
        let bubble = if fluid > 0.0 { (bubble + 1).min(60) } else { 0 };
        data.metadata
            .set(boat_meta::BUBBLE_TIME, MetaValue::Int(bubble));
        data.tick_count += 1;
    }
}

/// One tick of `AbstractBoat.floatBoat`: submerged hulls get buoyancy proportional to their
/// depth plus water drag, while airborne hulls keep gravity and a weaker air drag. A deeper hull
/// is pushed up harder, which makes a boat dropped into water rise instead of sinking.
pub fn boat_buoyancy(mut vel: Vec3, fluid: f64) -> Vec3 {
    if fluid > 0.0 {
        let depth = (fluid / BOAT_HEIGHT).clamp(0.0, 1.0);
        vel.y = (vel.y + 0.04 + 0.02 * depth).min(0.2);
        let drag = 0.9 - 0.08 * depth;
        vel.x *= drag;
        vel.z *= drag;
    } else {
        vel.y -= 0.04;
        vel.x *= 0.99;
        vel.z *= 0.99;
    }
    vel
}

fn tick_minecart(store: &mut EntityStore, key: crate::EntityKey, blocks: &impl BlockGetter) {
    let pos = store.pos(key).unwrap_or(Vec3::ZERO);
    let mut vel = store.vel(key).unwrap_or_default();
    if let Some((name, props)) = rail_below(blocks, pos) {
        // A cart on a rail is supported by the rail: no gravity, horizontal drag, and the
        // powered-rail boost in the direction it is already moving.
        vel.y = 0.0;
        let shape = props
            .iter()
            .find_map(|(key, value)| (*key == "shape").then_some(*value))
            .unwrap_or("north_south");
        // `AbstractMinecart.moveAlongTrack` keeps a cart centred across a straight rail.
        if let Some(snapped) = rail_snap(pos, shape) {
            store.set_pos(key, snapped);
        }
        let (east_west, ascending, sign) = rail_direction(shape, vel);
        if east_west {
            vel.z *= 0.6;
        } else {
            vel.x *= 0.6;
        }
        if ascending && (vel.x.abs() + vel.z.abs()) > 0.01 {
            vel.y = vel.y.max(0.06);
        }
        let powered = props
            .iter()
            .any(|(key, value)| *key == "powered" && *value == "true");
        if name == "powered_rail" && powered {
            if east_west {
                vel.x = (vel.x + sign * 0.06).clamp(-0.4, 0.4);
            } else {
                vel.z = (vel.z + sign * 0.06).clamp(-0.4, 0.4);
            }
        } else {
            let drag = if name == "powered_rail" { 0.997 } else { 0.96 };
            vel.x *= drag;
            vel.z *= drag;
        }
        // A curved rail turns the cart onto the cardinal edge it is about to leave through.
        if let Some((turn_east_west, turn_sign)) = curve_exit(shape, pos) {
            let speed = (vel.x.abs() + vel.z.abs()) * 0.5;
            if turn_east_west {
                vel.x = turn_sign * speed;
                vel.z *= 0.5;
            } else {
                vel.z = turn_sign * speed;
                vel.x *= 0.5;
            }
        }
    } else {
        vel.y -= 0.04;
        vel.x *= 0.98;
        vel.z *= 0.98;
    }
    store.set_vel(key, vel);
    physics::move_entity(store, key, vel, blocks);
    if let Some(data) = store.data_mut(key) {
        data.tick_count += 1;
    }
}

/// Where a cart on a straight rail is centred: the axis across the rail goes to the middle of
/// the block. Curved rails keep their arc (no axis is fixed for the whole block).
fn rail_snap(pos: Vec3, shape: &str) -> Option<Vec3> {
    let base = shape.strip_prefix("ascending_").unwrap_or(shape);
    let mut snapped = pos;
    match base {
        "east" | "west" | "east_west" => snapped.z = pos.z.floor() + 0.5,
        "north" | "south" | "north_south" => snapped.x = pos.x.floor() + 0.5,
        // Curved rails have no fixed axis, so a cart is held over the block's centre while the
        // curve turns its velocity onto the exiting cardinal edge.
        _ if base.contains('_') => {
            snapped.x = pos.x.floor() + 0.5;
            snapped.z = pos.z.floor() + 0.5;
        }
        _ => return None,
    }
    Some(snapped)
}

/// The cardinally-connected edge a curved rail joins nearest to the cart: which axis the cart
/// should leave along, and the sign of that direction. Straight rails return `None`.
fn curve_exit(shape: &str, pos: Vec3) -> Option<(bool, f64)> {
    let base = shape.strip_prefix("ascending_").unwrap_or(shape);
    let (a, b) = base.split_once('_')?;
    let edge = |name: &str| -> Option<(bool, f64, f64)> {
        let fx = pos.x - pos.x.floor();
        let fz = pos.z - pos.z.floor();
        match name {
            "east" => Some((true, 1.0, 1.0 - fx)),
            "west" => Some((true, -1.0, fx)),
            "south" => Some((false, 1.0, 1.0 - fz)),
            "north" => Some((false, -1.0, fz)),
            _ => None,
        }
    };
    let (a_axis, a_sign, a_distance) = edge(a)?;
    let (b_axis, b_sign, b_distance) = edge(b)?;
    if a_axis == b_axis {
        return None;
    }
    Some(if a_distance <= b_distance {
        (a_axis, a_sign)
    } else {
        (b_axis, b_sign)
    })
}

/// The axis, ascending flag and positive direction for a vanilla rail shape. A cart at rest on
/// a straight or curved rail defaults to the shape's first cardinal direction.
fn rail_direction(shape: &str, vel: Vec3) -> (bool, bool, f64) {
    let ascending = shape.starts_with("ascending_");
    let base = shape.strip_prefix("ascending_").unwrap_or(shape);
    let sign = |value: f64| {
        if value.abs() > 1.0e-4 {
            value.signum()
        } else {
            1.0
        }
    };
    match base {
        "east" => (true, ascending, 1.0),
        "west" => (true, ascending, -1.0),
        "north" => (false, ascending, -1.0),
        "south" => (false, ascending, 1.0),
        "east_west" => {
            let east = vel.x.abs() >= vel.z.abs();
            (
                east,
                ascending,
                if east { sign(vel.x) } else { sign(vel.z) },
            )
        }
        "north_south" => (false, ascending, sign(vel.z)),
        // Curved rails (`north_east`, `south_west`, ...): keep the axis the cart already has,
        // or default to the first axis in the shape name when it is at rest.
        _ if base.contains("east") || base.contains("west") => {
            let east = vel.x.abs() >= vel.z.abs();
            (
                east,
                ascending,
                if east { sign(vel.x) } else { sign(vel.z) },
            )
        }
        _ => (false, ascending, sign(vel.z)),
    }
}

/// The rail state immediately below a cart, if any.
fn rail_below(
    blocks: &impl BlockGetter,
    pos: Vec3,
) -> Option<(&'static str, &'static [(&'static str, &'static str)])> {
    let x = pos.x.floor() as i32;
    let z = pos.z.floor() as i32;
    let y = pos.y.floor() as i32 - 1;
    let block = blocks.block_state(x, y, z)?;
    let (name, props) = ferrite_data::blocks::state(block)?;
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    name.ends_with("rail").then_some((name, props))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::tests::TestWorld;

    #[test]
    fn boat_and_minecart_keys_are_recognised() {
        assert!(is_boat_name("minecraft:oak_boat"));
        assert!(is_boat_name("minecraft:oak_chest_boat"));
        assert!(is_boat_name("minecraft:bamboo_raft"));
        assert!(is_minecart_name("minecraft:minecart"));
        assert!(is_minecart_name("minecraft:chest_minecart"));
        assert!(is_vehicle_name("oak_boat"));
        assert!(!is_vehicle_name("minecraft:pig"));
    }

    #[test]
    fn a_boat_floats_and_a_minecart_falls() {
        let mut world = TestWorld::flat(64);
        let water = ferrite_data::blocks::default_state("water").unwrap();
        world.blocks.insert((0, 64, 0), water);
        let mut store = EntityStore::default();
        let boat =
            store.insert(new_boat("minecraft:oak_boat", Vec3::new(0.5, 64.0, 0.5), 0.0).unwrap());
        let cart =
            store.insert(new_minecart("minecraft:minecart", Vec3::new(4.5, 70.0, 0.5)).unwrap());
        for _ in 0..80 {
            tick_vehicles(&mut store, &world);
        }
        assert!(
            store.pos(boat).unwrap().y >= 64.0,
            "the boat stays on the water surface: {:?}",
            store.pos(boat)
        );
        assert!(
            (store.pos(cart).unwrap().y - 64.0).abs() < 1e-6,
            "the minecart falls to the floor: {:?}",
            store.pos(cart)
        );
    }

    #[test]
    fn a_minecart_rides_a_powered_rail() {
        let mut world = TestWorld::flat(64);
        let rail = ferrite_data::blocks::state_id(
            "minecraft:powered_rail",
            &[("powered", "true"), ("shape", "east_west")],
        )
        .unwrap();
        world.blocks.insert((0, 63, 0), rail);
        let mut store = EntityStore::default();
        let cart =
            store.insert(new_minecart("minecraft:minecart", Vec3::new(0.5, 64.0, 0.5)).unwrap());
        store.set_vel(cart, Vec3::new(0.1, 0.0, 0.0));
        for _ in 0..10 {
            tick_vehicles(&mut store, &world);
        }
        assert!(
            store.pos(cart).unwrap().y >= 64.0 - 1.0e-6,
            "the rail supports the cart: {:?}",
            store.pos(cart)
        );
        assert!(
            store.vel(cart).unwrap().x > 0.1,
            "the powered rail accelerates the cart: {:?}",
            store.vel(cart)
        );
    }

    #[test]
    fn an_ascending_rail_lifts_a_moving_minecart() {
        let mut world = TestWorld::flat(64);
        let rail = ferrite_data::blocks::state_id("minecraft:rail", &[("shape", "ascending_east")])
            .unwrap();
        world.blocks.insert((0, 63, 0), rail);
        let mut store = EntityStore::default();
        let cart =
            store.insert(new_minecart("minecraft:minecart", Vec3::new(0.5, 64.0, 0.5)).unwrap());
        store.set_vel(cart, Vec3::new(0.1, 0.0, 0.0));
        for _ in 0..5 {
            tick_vehicles(&mut store, &world);
        }
        assert!(
            store.pos(cart).unwrap().y > 64.0,
            "the ascending rail lifts the cart: {:?}",
            store.pos(cart)
        );
    }

    #[test]
    fn a_minecart_is_centred_on_a_straight_rail() {
        let mut world = TestWorld::flat(64);
        let rail =
            ferrite_data::blocks::state_id("minecraft:rail", &[("shape", "east_west")]).unwrap();
        world.blocks.insert((0, 63, 0), rail);
        let mut store = EntityStore::default();
        let cart =
            store.insert(new_minecart("minecraft:minecart", Vec3::new(0.2, 64.0, 0.9)).unwrap());
        tick_vehicles(&mut store, &world);
        assert!((store.pos(cart).unwrap().z - 0.5).abs() < 1e-9);
        assert!((store.pos(cart).unwrap().x - 0.2).abs() < 1e-9);
    }

    #[test]
    fn a_deep_boat_rises_and_tracks_its_bubble_time() {
        let mut world = TestWorld::flat(64);
        let water = ferrite_data::blocks::default_state("water").unwrap();
        for y in 64..70 {
            world.blocks.insert((0, y, 0), water);
        }
        let mut store = EntityStore::default();
        let boat =
            store.insert(new_boat("minecraft:oak_boat", Vec3::new(0.5, 64.0, 0.5), 0.0).unwrap());
        for _ in 0..10 {
            tick_vehicles(&mut store, &world);
        }
        assert!(
            store.pos(boat).unwrap().y > 64.0,
            "the buoyant hull rises: {:?}",
            store.pos(boat)
        );
        let bubble = store
            .data(boat)
            .unwrap()
            .metadata
            .get(boat_meta::BUBBLE_TIME)
            .cloned();
        assert!(matches!(bubble, Some(MetaValue::Int(ticks)) if ticks > 0));
        assert!(boat_buoyancy(Vec3::new(0.0, 0.0, 0.0), 1.0).y > 0.0);
        assert!(
            boat_buoyancy(Vec3::new(0.0, -1.0, 0.0), 1.0).y > -1.0,
            "buoyancy slows a sinking hull"
        );
        assert_eq!(boat_buoyancy(Vec3::new(0.0, 0.0, 0.0), 0.0).y, -0.04);
    }

    #[test]
    fn a_curved_rail_holds_the_cart_over_the_block_centre() {
        let mut world = TestWorld::flat(64);
        let rail =
            ferrite_data::blocks::state_id("minecraft:rail", &[("shape", "south_east")]).unwrap();
        world.blocks.insert((0, 63, 0), rail);
        let mut store = EntityStore::default();
        let cart =
            store.insert(new_minecart("minecraft:minecart", Vec3::new(0.9, 64.0, 0.95)).unwrap());
        tick_vehicles(&mut store, &world);
        let pos = store.pos(cart).unwrap();
        assert!((pos.x - 0.5).abs() < 1e-9 && (pos.z - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_minecart_stops_at_a_block_ahead() {
        let mut world = TestWorld::flat(64);
        let rail =
            ferrite_data::blocks::state_id("minecraft:rail", &[("shape", "east_west")]).unwrap();
        world.blocks.insert((0, 63, 0), rail);
        world.blocks.insert((1, 64, 0), 1);
        let mut store = EntityStore::default();
        let cart =
            store.insert(new_minecart("minecraft:minecart", Vec3::new(0.5, 64.0, 0.5)).unwrap());
        store.set_vel(cart, Vec3::new(0.2, 0.0, 0.0));
        tick_vehicles(&mut store, &world);
        assert_eq!(store.vel(cart).unwrap().x, 0.0, "the wall stops the cart");
    }

    #[test]
    fn a_curved_rail_turns_the_cart_towards_its_exit() {
        let mut world = TestWorld::flat(64);
        let rail =
            ferrite_data::blocks::state_id("minecraft:rail", &[("shape", "south_east")]).unwrap();
        world.blocks.insert((0, 63, 0), rail);
        let mut store = EntityStore::default();
        let cart =
            store.insert(new_minecart("minecraft:minecart", Vec3::new(0.9, 64.0, 0.95)).unwrap());
        store.set_vel(cart, Vec3::new(0.1, 0.0, 0.0));
        tick_vehicles(&mut store, &world);
        assert!(
            store.vel(cart).unwrap().z > 0.0,
            "the south edge turns the cart: {:?}",
            store.vel(cart)
        );
    }

    #[test]
    fn a_ridden_boat_animates_its_paddles_without_physics() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let boat =
            store.insert(new_boat("minecraft:oak_boat", Vec3::new(0.5, 64.0, 0.5), 0.0).unwrap());
        let uuid = store.data(boat).unwrap().uuid;
        tick_vehicles_with_riders(&mut store, &world, &|who| who == uuid);
        let left = store
            .data(boat)
            .unwrap()
            .metadata
            .get(boat_meta::PADDLE_LEFT)
            .cloned();
        let right = store
            .data(boat)
            .unwrap()
            .metadata
            .get(boat_meta::PADDLE_RIGHT)
            .cloned();
        assert_ne!(left, right, "the paddles alternate");
    }

    #[test]
    fn vehicles_define_the_verified_metadata_defaults() {
        let boat = new_boat("minecraft:oak_boat", Vec3::ZERO, 0.0).unwrap();
        let metadata = &boat.data.metadata;
        assert_eq!(
            metadata.get(boat_meta::PADDLE_LEFT),
            Some(&MetaValue::Boolean(false))
        );
        assert_eq!(
            metadata.get(boat_meta::BUBBLE_TIME),
            Some(&MetaValue::Int(0))
        );
        let cart = new_minecart("minecraft:minecart", Vec3::ZERO).unwrap();
        assert_eq!(
            cart.data.metadata.get(minecart_meta::DISPLAY_OFFSET),
            Some(&MetaValue::Int(0))
        );
    }

    #[test]
    fn all_22_boats_and_7_minecarts_construct_and_tnt_hopper_carts_fire_events() {
        for boat in ALL_BOAT_TYPES {
            let rec = new_boat(boat, Vec3::ZERO, 0.0)
                .unwrap_or_else(|| panic!("boat {boat} must exist in 26.3"));
            assert_eq!(
                bare(ferrite_data::entity_props::get(rec.data.kind).key),
                boat
            );
        }
        for cart in ALL_MINECART_TYPES {
            let rec = new_minecart(cart, Vec3::ZERO)
                .unwrap_or_else(|| panic!("minecart {cart} must exist in 26.3"));
            assert_eq!(
                bare(ferrite_data::entity_props::get(rec.data.kind).key),
                cart
            );
        }
        assert!(is_chest_vehicle("minecraft:spruce_chest_boat"));
        assert!(is_chest_vehicle("minecraft:bamboo_chest_raft"));
        assert!(is_chest_vehicle("minecraft:chest_minecart"));
        assert!(is_chest_vehicle("minecraft:hopper_minecart"));

        let mut world = TestWorld::flat(64);
        let activator = ferrite_data::blocks::state_id(
            "minecraft:activator_rail",
            &[("powered", "true"), ("shape", "north_south")],
        )
        .unwrap();
        world.blocks.insert((0, 63, 0), activator);
        let mut store = EntityStore::default();
        let tnt_cart = store
            .insert(new_minecart("minecraft:tnt_minecart", Vec3::new(0.5, 64.0, 0.5)).unwrap());
        let events = tick_vehicles_full(&mut store, &world, &|_| false);
        assert!(store.flags(tnt_cart).unwrap().has(Flags::REMOVED));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, VehicleEvent::TntExplode { .. }))
        );

        let hopper_cart = store
            .insert(new_minecart("minecraft:hopper_minecart", Vec3::new(5.5, 64.0, 5.5)).unwrap());
        let apple = ferrite_item::ItemStack::from_name("minecraft:apple").unwrap();
        let dropped = store.insert(crate::item::new_item_stack(
            Vec3::new(5.8, 64.0, 5.5),
            Vec3::ZERO,
            &apple,
            &mut crate::Rng::new(1),
        ));
        let events = tick_vehicles_full(&mut store, &world, &|_| false);
        assert!(!store.flags(hopper_cart).unwrap().has(Flags::REMOVED));
        assert!(store.flags(dropped).unwrap().has(Flags::REMOVED));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, VehicleEvent::HopperPickup { .. }))
        );
    }
}
