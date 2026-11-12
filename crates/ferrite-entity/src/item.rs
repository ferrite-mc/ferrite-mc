//! Item entities (vanilla `ItemEntity`): physics, merging, pickup and despawning.

use crate::metadata::item::ITEM;
use crate::physics::{self, BlockGetter};
use crate::rng::Rng;
use crate::store::{EntityData, EntityRecord, EntityStore, Flags};
use crate::tracker::{self, Sink};
use crate::{EntityKey, allocate_id, random_uuid};
use ferrite_inventory::PlayerInventory;
use ferrite_math::{AABB, Vec3};
use ferrite_protocol::{
    VarInt,
    packets::play::{ItemStack, MetaValue, TakeItemEntity},
};
use std::sync::OnceLock;
use uuid::Uuid;

/// Ticks before an item despawns.
pub const LIFETIME: i32 = 6000;
/// A pickup delay that never runs out.
pub const INFINITE_PICKUP_DELAY: i32 = 32767;
/// An age that never advances, so the item never despawns.
pub const INFINITE_LIFETIME: i32 = -32768;
/// Pickup delay of dropped and popped items (`setDefaultPickUpDelay`).
pub const DEFAULT_PICKUP_DELAY: i32 = 10;
const GRAVITY: f64 = 0.04;
const AIR_DRAG: f64 = 0.98;
const WIDTH: f64 = 0.25;
const HEIGHT: f64 = 0.25;

/// Item-specific state; the stack itself is the synched `ITEM` data value.
///
/// The plugin-visible fields (`target`, `thrower`, `unlimited_lifetime`, the pickup flags and
/// `will_age`) mirror Bukkit's `Item` surface; the game keeps its own pickup and ageing rules
/// on top of them.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemState {
    /// The components of the carried stack. The synched `ITEM` value is the protocol stack,
    /// which has no component patch, so the server-side truth lives here: pickup, merging,
    /// hoppers and the entity save all read the stack through [`full_stack`].
    pub components: ferrite_item::ComponentPatch,
    pub age: i32,
    pub pickup_delay: i32,
    pub health: i32,
    /// Only this player may pick the item up (Bukkit `Item#getOwner`).
    pub target: Option<Uuid>,
    /// The player that threw the item (Bukkit `Item#getThrower`).
    pub thrower: Option<Uuid>,
    /// The item never despawns (Bukkit `Item#isUnlimitedLifetime`).
    pub unlimited_lifetime: bool,
    /// Whether mobs may pick the item up.
    pub can_mob_pickup: bool,
    /// Whether players may pick the item up.
    pub can_player_pickup: bool,
    /// Whether the item ages at all (Bukkit `Item#willAge`).
    pub will_age: bool,
}

impl Default for ItemState {
    fn default() -> Self {
        Self {
            components: ferrite_item::ComponentPatch::new(),
            age: 0,
            pickup_delay: 0,
            health: 5,
            target: None,
            thrower: None,
            unlimited_lifetime: false,
            can_mob_pickup: true,
            can_player_pickup: true,
            will_age: true,
        }
    }
}

/// Entity type id of `minecraft:item`.
pub fn item_type() -> u16 {
    static KIND: OnceLock<u16> = OnceLock::new();
    *KIND.get_or_init(|| {
        ferrite_data::entity_props::by_name("item")
            .expect("vanilla has an item entity type")
            .id
    })
}

/// The stack an item entity carries.
pub fn stack(data: &EntityData) -> ItemStack {
    match data.metadata.get(ITEM) {
        Some(MetaValue::ItemStack(stack)) => *stack,
        _ => ItemStack::default(),
    }
}

/// The complete stack an item entity carries: the synched id and count plus the components in
/// [`ItemState`]. An empty stack when the entity carries nothing.
pub fn full_stack(data: &EntityData) -> ferrite_item::ItemStack {
    let plain = stack(data);
    let Ok(item) = u16::try_from(plain.item_id) else {
        return ferrite_item::ItemStack::empty();
    };
    let mut full = ferrite_item::ItemStack::with_count(item, plain.count);
    if let Some(state) = &data.item {
        full.components = state.components.clone();
    }
    full
}

/// The protocol stack of an item: its id and count, without components.
pub fn plain_stack(stack: &ferrite_item::ItemStack) -> ItemStack {
    ItemStack {
        count: stack.count,
        item_id: i32::from(stack.item),
    }
}

/// Gives a freshly built item entity the components of `stack`.
fn carry(mut record: EntityRecord, stack: &ferrite_item::ItemStack) -> EntityRecord {
    if let Some(state) = &mut record.data.item {
        state.components = stack.components.clone();
    }
    record
}

/// Replaces the stack, marking it dirty for trackers when it changed.
pub fn set_stack(data: &mut EntityData, stack: ItemStack) {
    let stack = if stack.count <= 0 {
        ItemStack::default()
    } else {
        stack
    };
    data.metadata.set(ITEM, MetaValue::ItemStack(stack));
}

/// A new item entity (vanilla `new ItemEntity(level, x, y, z, stack, dx, dy, dz)`).
pub fn new_item(pos: Vec3, vel: Vec3, stack: ItemStack, rng: &mut Rng) -> EntityRecord {
    let mut data = EntityData::new(allocate_id(), random_uuid(), item_type(), WIDTH, HEIGHT);
    data.yaw = rng.next_f32() * 360.0;
    data.metadata
        .define(ITEM, MetaValue::ItemStack(ItemStack::default()));
    set_stack(&mut data, stack);
    data.item = Some(ItemState::default());
    EntityRecord {
        pos,
        vel,
        flags: Flags::default(),
        data,
    }
}

/// [`new_item`] for a stack with components.
pub fn new_item_stack(
    pos: Vec3,
    vel: Vec3,
    stack: &ferrite_item::ItemStack,
    rng: &mut Rng,
) -> EntityRecord {
    carry(new_item(pos, vel, plain_stack(stack), rng), stack)
}

/// [`pop_resource`] for a stack with components.
pub fn pop_resource_stack(
    block: [i32; 3],
    stack: &ferrite_item::ItemStack,
    rng: &mut Rng,
) -> EntityRecord {
    carry(pop_resource(block, plain_stack(stack), rng), stack)
}

/// [`drop_from_container`] for a stack with components: every part keeps them.
pub fn drop_from_container_stack(
    block: [i32; 3],
    stack: &ferrite_item::ItemStack,
    rng: &mut Rng,
) -> Vec<EntityRecord> {
    drop_from_container(block, plain_stack(stack), rng)
        .into_iter()
        .map(|record| carry(record, stack))
        .collect()
}

/// An item dropped from a broken block (vanilla `Block.popResource`): a random spot inside the
/// block, a small random push and the default pickup delay.
pub fn pop_resource(block: [i32; 3], stack: ItemStack, rng: &mut Rng) -> EntityRecord {
    let half = HEIGHT / 2.0;
    let pos = Vec3::new(
        f64::from(block[0]) + 0.5 + rng.range(-0.25, 0.25),
        f64::from(block[1]) + 0.5 + rng.range(-0.25, 0.25) - half,
        f64::from(block[2]) + 0.5 + rng.range(-0.25, 0.25),
    );
    let vel = Vec3::new(rng.next_f64() * 0.2 - 0.1, 0.2, rng.next_f64() * 0.2 - 0.1);
    let mut record = new_item(pos, vel, stack, rng);
    if let Some(item) = &mut record.data.item {
        item.pickup_delay = DEFAULT_PICKUP_DELAY;
    }
    record
}

/// The items a broken container spills (vanilla `Containers.dropItemStack`): one random spot
/// inside the block, split into stacks of 10 to 30 with a small random push each.
pub fn drop_from_container(
    block: [i32; 3],
    mut stack: ItemStack,
    rng: &mut Rng,
) -> Vec<EntityRecord> {
    let spread = 1.0 - WIDTH;
    let pos = Vec3::new(
        f64::from(block[0]) + rng.next_f64() * spread + WIDTH / 2.0,
        f64::from(block[1]) + rng.next_f64() * spread,
        f64::from(block[2]) + rng.next_f64() * spread + WIDTH / 2.0,
    );
    let mut records = Vec::new();
    while stack.count > 0 {
        let part = stack.count.min(rng.next_int(21) + 10);
        stack.count -= part;
        let mut triangle =
            |mode: f64| mode + 0.114_850_001_711_398_36 * (rng.next_f64() - rng.next_f64());
        let vel = Vec3::new(triangle(0.0), triangle(0.2), triangle(0.0));
        // No pickup delay: vanilla adds these without `setDefaultPickUpDelay`.
        records.push(new_item(
            pos,
            vel,
            ItemStack {
                count: part,
                ..stack
            },
            rng,
        ));
    }
    records
}

/// Pickup delay of items a player throws (vanilla `Player.drop`).
pub const THROWN_PICKUP_DELAY: i32 = 40;

/// An item a player throws (vanilla `LivingEntity.drop(stack, false, true)`): from 0.3 below the
/// eyes, pushed 0.3 along the look direction with a little jitter, and not collectable for two
/// seconds.
pub fn thrown_by_player(
    feet: Vec3,
    eye_height: f64,
    yaw: f32,
    pitch: f32,
    stack: ItemStack,
    rng: &mut Rng,
) -> EntityRecord {
    let pos = Vec3::new(feet.x, feet.y + eye_height - 0.3, feet.z);
    let rad = std::f32::consts::PI / 180.0;
    let (sin_pitch, cos_pitch) = (pitch * rad).sin_cos();
    let (sin_yaw, cos_yaw) = (yaw * rad).sin_cos();
    let angle = rng.next_f32() * std::f32::consts::TAU;
    let spread = 0.02 * rng.next_f32();
    let up = -sin_pitch * 0.3 + 0.1 + (rng.next_f32() - rng.next_f32()) * 0.1;
    let vel = Vec3::new(
        f64::from(-sin_yaw * cos_pitch * 0.3) + f64::from(angle).cos() * f64::from(spread),
        f64::from(up),
        f64::from(cos_yaw * cos_pitch * 0.3) + f64::from(angle).sin() * f64::from(spread),
    );
    let mut record = new_item(pos, vel, stack, rng);
    if let Some(item) = &mut record.data.item {
        item.pickup_delay = THROWN_PICKUP_DELAY;
    }
    record
}

/// [`thrown_by_player`] for a stack with components.
pub fn thrown_by_player_stack(
    feet: Vec3,
    eye_height: f64,
    yaw: f32,
    pitch: f32,
    stack: &ferrite_item::ItemStack,
    rng: &mut Rng,
) -> EntityRecord {
    carry(
        thrown_by_player(feet, eye_height, yaw, pitch, plain_stack(stack), rng),
        stack,
    )
}

fn is_mergeable(store: &EntityStore, key: EntityKey) -> bool {
    let (Some(data), Some(flags)) = (store.data(key), store.flags(key)) else {
        return false;
    };
    let Some(item) = &data.item else { return false };
    let stack = stack(data);
    !flags.has(Flags::REMOVED)
        && item.pickup_delay != INFINITE_PICKUP_DELAY
        && item.age != INFINITE_LIFETIME
        && item.age < LIFETIME
        && stack.count < max_stack_size(stack.item_id)
}

/// Moves items from `from` onto `to` (vanilla `ItemEntity.merge`).
fn merge(store: &mut EntityStore, to: EntityKey, from: EntityKey) {
    let (Some(to_data), Some(from_data)) = (store.data(to), store.data(from)) else {
        return;
    };
    let (mut to_stack, mut from_stack) = (stack(to_data), stack(from_data));
    let (Some(to_item), Some(from_item)) = (to_data.item.clone(), from_data.item.clone()) else {
        return;
    };
    let moved = (max_stack_size(to_stack.item_id).min(64) - to_stack.count).min(from_stack.count);
    to_stack.count += moved;
    from_stack.count -= moved;
    let to_data = store.data_mut(to).unwrap();
    set_stack(to_data, to_stack);
    let item = to_data.item.as_mut().unwrap();
    item.pickup_delay = to_item.pickup_delay.max(from_item.pickup_delay);
    item.age = to_item.age.min(from_item.age);
    set_stack(store.data_mut(from).unwrap(), from_stack);
    if from_stack.count <= 0 {
        store.flags_mut(from).unwrap().set(Flags::REMOVED, true);
    }
}

fn try_merge(store: &mut EntityStore, this: EntityKey, other: EntityKey) {
    let (Some(a), Some(b)) = (store.data(this), store.data(other)) else {
        return;
    };
    let (this_stack, other_stack) = (stack(a), stack(b));
    let same_target = a.item.as_ref().map(|i| i.target) == b.item.as_ref().map(|i| i.target);
    // `areMergable`: the same item with the same components and room in `other`.
    let same_components =
        a.item.as_ref().map(|i| &i.components) == b.item.as_ref().map(|i| &i.components);
    if !same_target
        || !same_components
        || this_stack.item_id != other_stack.item_id
        || other_stack.count + this_stack.count > max_stack_size(other_stack.item_id)
    {
        return;
    }
    if other_stack.count < this_stack.count {
        merge(store, this, other);
    } else {
        merge(store, other, this);
    }
}

fn merge_with_neighbours(store: &mut EntityStore, key: EntityKey) {
    let Some(bbox) = store.bbox(key) else { return };
    let kind = item_type();
    for other in store.query_vec(bbox.inflate(0.5, 0.0, 0.5)) {
        if other == key || store.data(other).is_none_or(|d| d.kind != kind) {
            continue;
        }
        if !is_mergeable(store, key) {
            break;
        }
        if is_mergeable(store, other) {
            try_merge(store, key, other);
            if store.flags(key).is_none_or(|f| f.has(Flags::REMOVED)) {
                break;
            }
        }
    }
}

/// Ticks every item entity in the store (vanilla `ItemEntity.tick`).
pub fn tick_items(store: &mut EntityStore, blocks: &impl BlockGetter, rng: &mut Rng) {
    let kind = item_type();
    let keys = store.keys().to_vec();
    for key in keys {
        let Some(data) = store.data_mut(key) else {
            continue;
        };
        if data.kind != kind || data.item.is_none() {
            continue;
        }
        if store.flags(key).unwrap().has(Flags::REMOVED) {
            continue;
        }
        tick_item(store, key, blocks, rng);
    }
}

fn tick_item(store: &mut EntityStore, key: EntityKey, blocks: &impl BlockGetter, rng: &mut Rng) {
    let data = store.data_mut(key).unwrap();
    if stack(data).count <= 0 {
        store.flags_mut(key).unwrap().set(Flags::REMOVED, true);
        return;
    }
    // The level increments tickCount before ticking (`ServerLevel.tickNonPassenger`).
    data.tick_count += 1;
    let tick_count = data.tick_count;
    let id = data.id;
    let item = data.item.as_mut().unwrap();
    if item.pickup_delay > 0 && item.pickup_delay != INFINITE_PICKUP_DELAY {
        item.pickup_delay -= 1;
    }
    let old_pos = store.pos(key).unwrap();
    let old_vel = store.vel(key).unwrap();
    let bbox = store.bbox(key).unwrap();
    let no_gravity = store.flags(key).unwrap().has(Flags::NO_GRAVITY);
    let mut vel = old_vel;
    if physics::fluid_height(blocks, bbox, false) > 0.1 {
        vel = Vec3::new(
            vel.x * 0.99,
            vel.y + if vel.y < 0.06 { 5.0E-4 } else { 0.0 },
            vel.z * 0.99,
        );
    } else if physics::fluid_height(blocks, bbox, true) > 0.1 {
        vel = Vec3::new(
            vel.x * 0.95,
            vel.y + if vel.y < 0.06 { 5.0E-4 } else { 0.0 },
            vel.z * 0.95,
        );
    } else if !no_gravity {
        vel.y -= GRAVITY;
    }
    store.set_vel(key, vel);
    let stuck = !physics::no_collision(blocks, bbox.deflate_all(1.0E-7));
    store.flags_mut(key).unwrap().set(Flags::NO_PHYSICS, stuck);
    if stuck {
        let center = Vec3::new(old_pos.x, (bbox.min.y + bbox.max.y) / 2.0, old_pos.z);
        physics::move_towards_closest_space(store, key, center, blocks, rng);
    }
    let on_ground = store.flags(key).unwrap().has(Flags::ON_GROUND);
    let vel = store.vel(key).unwrap();
    let resting = on_ground
        && vel.horizontal_distance_sqr() <= f64::from(1.0E-5_f32)
        && (tick_count as i64 + i64::from(id)) % 4 != 0;
    if !resting {
        physics::move_entity(store, key, vel, blocks);
        let pos = store.pos(key).unwrap();
        let on_ground = store.flags(key).unwrap().has(Flags::ON_GROUND);
        let ground = if on_ground {
            AIR_DRAG * physics::friction(blocks, physics::on_pos(pos, 0.999_999))
        } else {
            AIR_DRAG
        };
        let mut vel = store.vel(key).unwrap();
        vel = Vec3::new(vel.x * ground, vel.y * AIR_DRAG, vel.z * ground);
        if on_ground && vel.y < 0.0 {
            vel.y *= -0.5;
        }
        store.set_vel(key, vel);
    }
    let pos = store.pos(key).unwrap();
    let moved = old_pos.x.floor() != pos.x.floor()
        || old_pos.y.floor() != pos.y.floor()
        || old_pos.z.floor() != pos.z.floor();
    let rate = if moved { 2 } else { 40 };
    if tick_count.is_multiple_of(rate) && is_mergeable(store, key) {
        merge_with_neighbours(store, key);
    }
    let Some(data) = store.data_mut(key) else {
        return;
    };
    let item = data.item.as_mut().unwrap();
    if item.age != INFINITE_LIFETIME && item.will_age {
        item.age += 1;
    }
    let expired = item.age >= LIFETIME && !item.unlimited_lifetime;
    let vel = store.vel(key).unwrap();
    let flags = store.flags_mut(key).unwrap();
    if (vel - old_vel).length_sqr() > 0.01 {
        flags.set(Flags::NEEDS_SYNC, true);
    }
    if expired {
        flags.set(Flags::REMOVED, true);
    }
}

/// Maximum stack size of an item (64 for unknown ids).
pub fn max_stack_size(item_id: i32) -> i32 {
    u16::try_from(item_id)
        .ok()
        .and_then(ferrite_item::item_defaults)
        .map_or(64, |d| d.max_stack_size)
}

/// A player that can pick items up.
pub struct Picker<'a> {
    pub uuid: Uuid,
    /// Network entity id, the collector in `take_item_entity`.
    pub entity_id: i32,
    pub pos: Vec3,
    /// The player's inventory; its selected slot is filled first.
    pub inventory: &'a mut PlayerInventory,
    /// Creative players take stacks that do not fit.
    pub infinite: bool,
}

/// Picks up the items a player touches (vanilla `Player.touch` + `ItemEntity.playerTouch`):
/// items in the player's box widened by 1 x 0.5 x 1 whose pickup delay ran out. Sends
/// `take_item_entity` to everyone tracking each item and returns whether anything was taken.
pub fn pick_up(store: &mut EntityStore, picker: &mut Picker<'_>, sink: &mut Sink<'_>) -> bool {
    let p = picker.pos;
    let area = AABB::new(p.x - 0.3, p.y, p.z - 0.3, p.x + 0.3, p.y + 1.8, p.z + 0.3)
        .inflate(1.0, 0.5, 1.0);
    let kind = item_type();
    let mut changed = false;
    for key in store.query_vec(area) {
        let (Some(data), Some(flags)) = (store.data(key), store.flags(key)) else {
            continue;
        };
        let Some(item) = &data.item else { continue };
        if data.kind != kind
            || flags.has(Flags::REMOVED)
            || item.pickup_delay != 0
            || !item.can_player_pickup
            || item.target.is_some_and(|t| t != picker.uuid)
        {
            continue;
        }
        let mut taken = stack(data);
        let original = taken.count;
        let mut full = full_stack(data);
        if full.is_empty() {
            continue;
        }
        if !picker.inventory.add(&mut full, picker.infinite) {
            continue;
        }
        taken.count = if full.is_empty() { 0 } else { full.count };
        changed = true;
        tracker::broadcast(
            store,
            key,
            &TakeItemEntity {
                item: VarInt(data.id),
                collector: VarInt(picker.entity_id),
                amount: VarInt(original),
            },
            sink,
        );
        if taken.count <= 0 {
            store.flags_mut(key).unwrap().set(Flags::REMOVED, true);
        } else {
            set_stack(store.data_mut(key).unwrap(), taken);
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::tests::TestWorld;

    fn dirt(count: i32) -> ItemStack {
        ItemStack {
            count,
            item_id: ferrite_data::registries::id("minecraft:item", "minecraft:dirt").unwrap(),
        }
    }

    #[test]
    fn dropped_item_falls_and_comes_to_rest_on_the_ground() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let mut rng = Rng::new(7);
        let key = store.insert(pop_resource([0, 64, 0], dirt(1), &mut rng));
        let start = store.pos(key).unwrap();
        assert!(start.y > 64.0 && start.y < 65.0);
        let mut peak = start.y;
        for _ in 0..60 {
            tick_items(&mut store, &world, &mut rng);
            peak = peak.max(store.pos(key).unwrap().y);
        }
        let pos = store.pos(key).unwrap();
        assert!(peak > start.y, "the item is thrown upward first");
        assert!(
            (pos.y - 64.0).abs() < 1e-9,
            "rests on the stone top: {pos:?}"
        );
        assert!(store.flags(key).unwrap().has(Flags::ON_GROUND));
        // Ground friction (0.6 * 0.98 per tick) has stopped it.
        assert!(store.vel(key).unwrap().horizontal_distance() < 1e-3);
        assert_eq!(
            store.data(key).unwrap().item.as_ref().unwrap().pickup_delay,
            0
        );
    }

    #[test]
    fn falls_through_air_with_vanilla_gravity_and_drag() {
        let world = TestWorld::flat(0);
        let mut store = EntityStore::default();
        let mut rng = Rng::new(1);
        let key = store.insert(new_item(
            Vec3::new(0.5, 100.0, 0.5),
            Vec3::ZERO,
            dirt(1),
            &mut rng,
        ));
        tick_items(&mut store, &world, &mut rng);
        // v = (0 - 0.04) * 0.98 after moving by -0.04.
        assert!((store.pos(key).unwrap().y - 99.96).abs() < 1e-12);
        assert!((store.vel(key).unwrap().y + 0.0392).abs() < 1e-12);
    }

    #[test]
    fn stuck_items_are_pushed_out() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let mut rng = Rng::new(3);
        let key = store.insert(new_item(
            Vec3::new(0.5, 63.3, 0.5),
            Vec3::ZERO,
            dirt(1),
            &mut rng,
        ));
        for _ in 0..20 {
            tick_items(&mut store, &world, &mut rng);
        }
        assert!(store.pos(key).unwrap().y >= 64.0 - 1e-9);
    }

    #[test]
    fn nearby_items_merge_and_keep_the_longer_delay() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let mut rng = Rng::new(5);
        let a = store.insert(new_item(
            Vec3::new(0.5, 64.0, 0.5),
            Vec3::ZERO,
            dirt(3),
            &mut rng,
        ));
        let b = store.insert(new_item(
            Vec3::new(0.9, 64.0, 0.5),
            Vec3::ZERO,
            dirt(5),
            &mut rng,
        ));
        store
            .data_mut(a)
            .unwrap()
            .item
            .as_mut()
            .unwrap()
            .pickup_delay = 100;
        for _ in 0..40 {
            tick_items(&mut store, &world, &mut rng);
        }
        let flags_a = store.flags(a).unwrap();
        assert!(flags_a.has(Flags::REMOVED));
        let survivor = store.data(b).unwrap();
        assert_eq!(stack(survivor).count, 8);
        assert!(survivor.item.as_ref().unwrap().pickup_delay > 0);
        // Different items never merge.
        let stone = ItemStack {
            count: 1,
            item_id: ferrite_data::registries::id("minecraft:item", "minecraft:stone").unwrap(),
        };
        let c = store.insert(new_item(
            Vec3::new(0.7, 64.0, 0.5),
            Vec3::ZERO,
            stone,
            &mut rng,
        ));
        for _ in 0..80 {
            tick_items(&mut store, &world, &mut rng);
        }
        assert!(!store.flags(c).unwrap().has(Flags::REMOVED));
    }

    #[test]
    fn items_despawn_after_their_lifetime() {
        let world = TestWorld::flat(64);
        let mut store = EntityStore::default();
        let mut rng = Rng::new(9);
        let key = store.insert(new_item(
            Vec3::new(0.5, 64.0, 0.5),
            Vec3::ZERO,
            dirt(1),
            &mut rng,
        ));
        store.data_mut(key).unwrap().item.as_mut().unwrap().age = LIFETIME - 2;
        tick_items(&mut store, &world, &mut rng);
        assert!(!store.flags(key).unwrap().has(Flags::REMOVED));
        tick_items(&mut store, &world, &mut rng);
        assert!(store.flags(key).unwrap().has(Flags::REMOVED));
        assert_eq!(store.sweep_removed().len(), 1);
    }

    #[test]
    fn pickup_respects_delay_range_and_fills_the_inventory() {
        let mut store = EntityStore::default();
        let mut rng = Rng::new(2);
        let key = store.insert(new_item(
            Vec3::new(1.0, 64.0, 0.5),
            Vec3::ZERO,
            dirt(2),
            &mut rng,
        ));
        store
            .data_mut(key)
            .unwrap()
            .item
            .as_mut()
            .unwrap()
            .pickup_delay = 1;
        let mut inventory = PlayerInventory::new();
        let mut sent = Vec::new();
        let mut sink = |_: tracker::ViewerKey, packet: &[u8]| sent.push(packet.to_vec());
        let mut picker = Picker {
            uuid: Uuid::from_u128(1),
            entity_id: 77,
            pos: Vec3::new(0.0, 64.0, 0.5),
            inventory: &mut inventory,
            infinite: false,
        };
        assert!(!pick_up(&mut store, &mut picker, &mut sink));
        store
            .data_mut(key)
            .unwrap()
            .item
            .as_mut()
            .unwrap()
            .pickup_delay = 0;
        // Too far away: the box reaches 1.3 blocks sideways.
        picker.pos = Vec3::new(3.0, 64.0, 0.5);
        assert!(!pick_up(&mut store, &mut picker, &mut sink));
        picker.pos = Vec3::new(0.0, 64.0, 0.5);
        assert!(pick_up(&mut store, &mut picker, &mut sink));
        assert!(store.flags(key).unwrap().has(Flags::REMOVED));
        assert_eq!(
            inventory.get(0),
            &ferrite_item::ItemStack::with_count(dirt(2).item_id as u16, 2)
        );
    }

    #[test]
    fn thrown_items_fly_where_the_player_looks() {
        let mut rng = Rng::new(9);
        let record = thrown_by_player(Vec3::new(0.5, 64.0, 0.5), 1.62, 0.0, 0.0, dirt(1), &mut rng);
        assert!((record.pos.y - 65.32).abs() < 1e-9);
        assert!((0.28..=0.32).contains(&record.vel.z), "{:?}", record.vel);
        assert!(record.vel.x.abs() <= 0.02 && (0.0..=0.2).contains(&record.vel.y));
        assert_eq!(record.data.item.unwrap().pickup_delay, THROWN_PICKUP_DELAY);
        // Yaw 90 looks toward -x.
        let record = thrown_by_player(Vec3::ZERO, 1.62, 90.0, 0.0, dirt(1), &mut rng);
        assert!(record.vel.x < -0.28);
    }

    /// A stack of `count` dirt blocks whose repair cost marks it as one of a kind.
    fn marked(count: i32, mark: i32) -> ferrite_item::ItemStack {
        let mut stack = ferrite_item::ItemStack::with_count(dirt(1).item_id as u16, count);
        stack.components.insert(
            ferrite_item::component_ids::REPAIR_COST,
            ferrite_item::Component::RepairCost(mark),
        );
        stack
    }

    #[test]
    fn pickup_keeps_the_components_of_the_item_entity() {
        let mut store = EntityStore::default();
        let mut rng = Rng::new(5);
        let key = store.insert(new_item_stack(
            Vec3::new(0.5, 64.0, 0.5),
            Vec3::ZERO,
            &marked(2, 7),
            &mut rng,
        ));
        assert_eq!(full_stack(store.data(key).unwrap()), marked(2, 7));
        let mut inventory = PlayerInventory::new();
        let mut sink = |_: tracker::ViewerKey, _: &[u8]| {};
        let mut picker = Picker {
            uuid: Uuid::from_u128(1),
            entity_id: 1,
            pos: Vec3::new(0.5, 64.0, 0.5),
            inventory: &mut inventory,
            infinite: false,
        };
        assert!(pick_up(&mut store, &mut picker, &mut sink));
        assert_eq!(inventory.get(0), &marked(2, 7));
    }

    #[test]
    fn a_partly_picked_up_item_keeps_its_components_on_the_ground() {
        let mut store = EntityStore::default();
        let mut rng = Rng::new(5);
        let key = store.insert(new_item_stack(
            Vec3::new(0.5, 64.0, 0.5),
            Vec3::ZERO,
            &marked(3, 7),
            &mut rng,
        ));
        let mut inventory = PlayerInventory::new();
        // Fill every slot with a different marked stack that is already full.
        for slot in 0..ferrite_inventory::SIZE {
            inventory.set(slot, marked(64, 99));
        }
        let mut sink = |_: tracker::ViewerKey, _: &[u8]| {};
        let mut picker = Picker {
            uuid: Uuid::from_u128(1),
            entity_id: 1,
            pos: Vec3::new(0.5, 64.0, 0.5),
            inventory: &mut inventory,
            infinite: false,
        };
        assert!(!pick_up(&mut store, &mut picker, &mut sink));
        assert_eq!(full_stack(store.data(key).unwrap()), marked(3, 7));
    }

    #[test]
    fn item_entities_merge_only_when_their_components_match() {
        let world = TestWorld::flat(64);
        let mut rng = Rng::new(11);
        let mut store = EntityStore::default();
        let at = Vec3::new(0.5, 64.0, 0.5);
        let a = store.insert(new_item_stack(at, Vec3::ZERO, &marked(1, 1), &mut rng));
        let b = store.insert(new_item_stack(at, Vec3::ZERO, &marked(1, 2), &mut rng));
        for _ in 0..80 {
            tick_items(&mut store, &world, &mut rng);
        }
        assert!(!store.flags(a).unwrap().has(Flags::REMOVED));
        assert!(!store.flags(b).unwrap().has(Flags::REMOVED));
        assert_eq!(full_stack(store.data(a).unwrap()), marked(1, 1));
        assert_eq!(full_stack(store.data(b).unwrap()), marked(1, 2));

        let mut store = EntityStore::default();
        let a = store.insert(new_item_stack(at, Vec3::ZERO, &marked(1, 3), &mut rng));
        let b = store.insert(new_item_stack(at, Vec3::ZERO, &marked(1, 3), &mut rng));
        for _ in 0..80 {
            tick_items(&mut store, &world, &mut rng);
        }
        let survivors: Vec<_> = [a, b]
            .into_iter()
            .filter(|key| !store.flags(*key).unwrap().has(Flags::REMOVED))
            .collect();
        assert_eq!(survivors.len(), 1, "identical stacks merge into one entity");
        assert_eq!(full_stack(store.data(survivors[0]).unwrap()), marked(2, 3));
    }

    #[test]
    fn a_spilled_stack_keeps_its_components_on_every_part() {
        let mut rng = Rng::new(3);
        let records = drop_from_container_stack([0, 64, 0], &marked(40, 5), &mut rng);
        assert!(
            records.len() > 1,
            "a stack of 40 splits into several entities"
        );
        let mut total = 0;
        for record in &records {
            let stack = full_stack(&record.data);
            assert_eq!(stack.components, marked(1, 5).components);
            total += stack.count;
        }
        assert_eq!(total, 40);
    }

    #[test]
    fn thrown_and_popped_stacks_carry_their_components() {
        let mut rng = Rng::new(3);
        let thrown = thrown_by_player_stack(Vec3::ZERO, 1.62, 0.0, 0.0, &marked(1, 8), &mut rng);
        assert_eq!(full_stack(&thrown.data), marked(1, 8));
        let popped = pop_resource_stack([0, 64, 0], &marked(1, 9), &mut rng);
        assert_eq!(full_stack(&popped.data), marked(1, 9));
    }
}
