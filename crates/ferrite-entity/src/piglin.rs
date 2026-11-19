//! Piglin bartering and the piglin's eight-slot inventory.
//!
//! Everything here is region-owned state on the entity record: the inventory lives in
//! [`crate::EntityData::piglin_inventory`], the admiring timer in [`crate::mob::MobState`] and
//! the held ingot in the generic off-hand equipment slot, so all of it travels with the entity
//! through region splits, merges and entity saves without any shared lock.
//!
//! Vanilla 26.3 reference (`PiglinAi`, `Piglin`, `BehaviorUtils`, `StopHoldingItemIfNoLongerAdmiring`,
//! read from the official server jar):
//!
//! * `PiglinAi.mobInteract`: an adult piglin that is neither admiring nor has admiring
//!   disabled, offered `minecraft:gold_ingot` (`BARTERING_ITEM`), takes one ingot
//!   (`consumeAndReturn(1, player)`, so creative players keep theirs) into its off hand
//!   (`setItemSlotAndDropWhenKilled`), sets `ADMIRING_ITEM` with a 119 tick expiry and stops
//!   walking.
//! * When that memory expires, `StopHoldingItemIfNoLongerAdmiring` calls
//!   `stopHoldingOffHandItem(.., shouldBarter = true)`: the off hand is emptied and an adult
//!   rolls `minecraft:gameplay/piglin_bartering` (`THIS_ENTITY` = the piglin) and throws the
//!   result toward the nearest visible player (else toward a random nearby spot).
//! * `throwItem`: the item spawns at the piglin's eye height minus 0.3 with a velocity of
//!   `normalize(target + (0, 1, 0) - piglin) * 0.3` and the default pickup delay of 10 ticks.
//! * `PiglinAi.wasHurtBy` while holding an off-hand item calls `stopHoldingOffHandItem(.., false)`:
//!   the off hand is emptied and a barter currency is simply gone, not refunded or rolled.
//! * The inventory is a `SimpleContainer(8)` saved as the `Inventory` list of item stacks
//!   (non-empty slots only, no `Slot` key) and dropped in full on death
//!   (`Piglin.dropCustomDeathLoot`).
//! * `ADMIRING_ITEM` is a persisted brain memory: `Brain.memories."minecraft:admiring_item"`
//!   holds `{value: true, ttl: <remaining ticks>}`.

use crate::mob::{self, MobKind};
use crate::{EntityKey, EntityStore, EquipmentSlot};
use ferrite_item::ItemStack;
use ferrite_math::Vec3;
use ferrite_nbt::{Compound, List, Tag};

/// The only barter currency (`PiglinAi.BARTERING_ITEM`).
pub const BARTER_ITEM: &str = "minecraft:gold_ingot";
/// The loot table a finished barter rolls (`BuiltInLootTables.PIGLIN_BARTERING`).
pub const BARTER_LOOT_TABLE: &str = "minecraft:gameplay/piglin_bartering";
/// Slots of the piglin's inventory (`new SimpleContainer(8)`).
pub const INVENTORY_SLOTS: usize = 8;
/// `SimpleContainer` stack cap (`Container.DEFAULT_MAX_STACK_SIZE`).
const CONTAINER_MAX_STACK: i32 = 99;
/// `PiglinAi.admireGoldItem`: the `ADMIRING_ITEM` memory expires after this many ticks.
pub const ADMIRE_TICKS: u16 = 119;
/// Vanilla brain memory id of the admiring flag.
pub const ADMIRING_MEMORY: &str = "minecraft:admiring_item";
/// `BehaviorUtils.throwItem` speed on each axis.
pub const THROW_SPEED: f64 = 0.3;
/// `BehaviorUtils.throwItem` spawns the item `eyeY - 0.3` above the feet.
pub const THROW_EYE_DROP: f64 = 0.3;
/// `PiglinAi.throwItemsTowardPos` aims one block above the target position.
pub const THROW_TARGET_RISE: f64 = 1.0;
/// `PiglinAi.angerNearbyPiglins` looks for piglins in the player's box inflated by 16 blocks.
pub const ANGER_RADIUS: f64 = 16.0;

/// The eight-slot piglin inventory (`Piglin.inventory`).
#[derive(Debug, Clone, PartialEq)]
pub struct PiglinInventory {
    slots: Vec<ItemStack>,
}

impl Default for PiglinInventory {
    fn default() -> Self {
        Self::new()
    }
}

impl PiglinInventory {
    /// An empty inventory of [`INVENTORY_SLOTS`] slots.
    pub fn new() -> Self {
        Self {
            slots: vec![ItemStack::empty(); INVENTORY_SLOTS],
        }
    }

    /// The stack in `slot`, if the slot exists.
    pub fn get(&self, slot: usize) -> Option<&ItemStack> {
        self.slots.get(slot)
    }

    /// Replaces the stack in `slot` (clamped to the container stack cap). Returns the old stack,
    /// or `None` when the slot does not exist.
    pub fn set(&mut self, slot: usize, mut stack: ItemStack) -> Option<ItemStack> {
        let at = self.slots.get_mut(slot)?;
        if !stack.is_empty() && stack.count > CONTAINER_MAX_STACK {
            stack.count = CONTAINER_MAX_STACK;
        }
        if stack.is_empty() {
            stack = ItemStack::empty();
        }
        Some(std::mem::replace(at, stack))
    }

    /// Whether every slot is empty.
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(ItemStack::is_empty)
    }

    /// Every stack, in slot order (empty slots included).
    pub fn slots(&self) -> &[ItemStack] {
        &self.slots
    }

    /// Vanilla `SimpleContainer.canAddItem`: whether any slot is empty or can take some of
    /// `stack`.
    pub fn can_add(&self, stack: &ItemStack) -> bool {
        if stack.is_empty() {
            return false;
        }
        self.slots.iter().any(|slot| {
            slot.is_empty()
                || (slot.is_same_item_same_components(stack) && slot.count < slot.max_stack_size())
        })
    }

    /// Vanilla `SimpleContainer.addItem`: tops up matching stacks first, then fills the first
    /// empty slot. Returns what did not fit (an empty stack when everything did).
    pub fn add_item(&mut self, stack: &ItemStack) -> ItemStack {
        let mut rest = stack.clone();
        if rest.is_empty() {
            return ItemStack::empty();
        }
        for slot in &mut self.slots {
            if slot.is_same_item_same_components(&rest) {
                slot.merge(&mut rest);
                if rest.is_empty() {
                    return ItemStack::empty();
                }
            }
        }
        if let Some(slot) = self.slots.iter_mut().find(|slot| slot.is_empty()) {
            let placed = rest.count.min(CONTAINER_MAX_STACK);
            *slot = rest.copy_with_count(placed);
            rest.count -= placed;
        }
        if rest.is_empty() {
            ItemStack::empty()
        } else {
            rest
        }
    }

    /// Vanilla `SimpleContainer.removeAllItems`: empties the inventory and returns the stacks.
    pub fn take_all(&mut self) -> Vec<ItemStack> {
        let mut out = Vec::new();
        for slot in &mut self.slots {
            let stack = std::mem::replace(slot, ItemStack::empty());
            if !stack.is_empty() {
                out.push(stack);
            }
        }
        out
    }

    /// The vanilla `Inventory` NBT list (`storeAsItemList`): non-empty stacks in slot order,
    /// without slot indices.
    pub fn to_nbt(&self) -> Tag {
        let list: Vec<Compound> = self
            .slots
            .iter()
            .filter(|stack| !stack.is_empty())
            .filter_map(|stack| stack.to_nbt().ok())
            .collect();
        Tag::List(List::Compound(list))
    }

    /// Reads what [`Self::to_nbt`] wrote (`fromItemList`: every stack goes through `addItem`).
    pub fn from_nbt(tag: &Tag) -> Self {
        let mut inventory = Self::new();
        if let Tag::List(List::Compound(list)) = tag {
            for entry in list {
                if let Ok(stack) = ItemStack::from_nbt(entry) {
                    inventory.add_item(&stack);
                }
            }
        }
        inventory
    }
}

/// The inventory of the piglin at `key`, if it is one.
pub fn inventory(store: &EntityStore, key: EntityKey) -> Option<&PiglinInventory> {
    store.data(key)?.piglin_inventory.as_ref()
}

/// The mutable inventory of the piglin at `key`, if it is one.
pub fn inventory_mut(store: &mut EntityStore, key: EntityKey) -> Option<&mut PiglinInventory> {
    store.data_mut(key)?.piglin_inventory.as_mut()
}

/// Removes and returns every item in the piglin's inventory (`Piglin.dropCustomDeathLoot`).
pub fn take_inventory(store: &mut EntityStore, key: EntityKey) -> Vec<ItemStack> {
    inventory_mut(store, key)
        .map(PiglinInventory::take_all)
        .unwrap_or_default()
}

/// `PiglinAi.angerNearbyPiglins`: every idle, living adult piglin in `player`'s box inflated by
/// [`ANGER_RADIUS`] (and with line of sight when `require_los` is set) targets `player` for
/// [`mob::PIGLIN_ANGER_TICKS`] ticks, ignoring golden armor. Returns how many piglins angered.
pub fn anger_nearby(
    store: &mut EntityStore,
    blocks: &impl crate::BlockGetter,
    player: uuid::Uuid,
    player_pos: Vec3,
    require_los: bool,
) -> usize {
    let (half_width, height) = ferrite_data::entity_props::by_name("player")
        .map_or((0.3, 1.8), |p| {
            (f64::from(p.width) * 0.5, f64::from(p.height))
        });
    let area = ferrite_math::AABB::new(
        player_pos.x - half_width,
        player_pos.y,
        player_pos.z - half_width,
        player_pos.x + half_width,
        player_pos.y + height,
        player_pos.z + half_width,
    )
    .inflate_all(ANGER_RADIUS);
    let eye_height = f64::from(MobKind::Piglin.props().eye_height);
    let mut targets = Vec::new();
    store.query(area, |key| {
        let Some(state) = mob::state(store, key) else {
            return;
        };
        if state.kind != MobKind::Piglin
            || state.age < 0
            || state.health <= 0.0
            || state.loot_dropped
            || state.admiring > 0
            || state.target.is_some()
        {
            return;
        }
        if require_los {
            let Some(piglin_pos) = store.pos(key) else {
                return;
            };
            if !crate::ai::has_line_of_sight(blocks, piglin_pos, eye_height, player_pos) {
                return;
            }
        }
        targets.push(key);
    });
    for &key in &targets {
        if let Some(state) = mob::state_mut(store, key) {
            state.target = Some(player);
            state.anger = mob::PIGLIN_ANGER_TICKS;
        }
    }
    targets.len()
}

/// Whether the mob at `key` is a living adult piglin.
fn is_live_adult(store: &EntityStore, key: EntityKey) -> bool {
    mob::state(store, key).is_some_and(|s| {
        s.kind == MobKind::Piglin && s.age >= 0 && s.health > 0.0 && !s.loot_dropped
    })
}

/// Whether the piglin at `key` is holding an ingot while admiring it (`ADMIRING_ITEM`).
pub fn is_admiring(store: &EntityStore, key: EntityKey) -> bool {
    mob::state(store, key).is_some_and(|s| s.admiring > 0)
}

/// `PiglinAi.canAdmire`: a living adult piglin that is not already admiring accepts exactly the
/// barter currency. (`isAdmiringDisabled` is set only by the unmodelled item-pickup behaviours,
/// so it is never active here.)
pub fn can_admire(store: &EntityStore, key: EntityKey, held: &str) -> bool {
    held == BARTER_ITEM && is_live_adult(store, key) && !is_admiring(store, key)
}

/// What taking an ingot displaced.
#[derive(Debug, Clone, PartialEq)]
pub struct Admiring {
    /// The stack that was already in the off hand and is spilled to the world
    /// (`holdInOffhand` calls `spawnAtLocation`), if any.
    pub spilled: Option<ItemStack>,
}

/// `PiglinAi.mobInteract` on the piglin side: puts one gold ingot in the off hand with a
/// guaranteed drop on death and starts the [`ADMIRE_TICKS`] timer. Returns `None` (and changes
/// nothing) when [`can_admire`] is false, so the caller keeps the player's stack untouched.
pub fn start_admiring(store: &mut EntityStore, key: EntityKey) -> Option<Admiring> {
    if !can_admire(store, key, BARTER_ITEM) {
        return None;
    }
    let ingot = ItemStack::from_name(BARTER_ITEM)?;
    let spilled =
        mob::equipment_stack(store, key, EquipmentSlot::OffHand).filter(|s| !s.is_empty());
    if !mob::set_equipment_stack(store, key, EquipmentSlot::OffHand, Some(ingot)) {
        return None;
    }
    if let Some(data) = store.data_mut(key) {
        data.equipment.set_guaranteed_drop(EquipmentSlot::OffHand);
    }
    mob::state_mut(store, key)?.admiring = ADMIRE_TICKS;
    Some(Admiring { spilled })
}

/// The outcome of one admiring tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admire {
    /// Not admiring, or still waiting.
    Idle,
    /// The memory just expired: the off-hand ingot is gone and the caller must roll
    /// [`BARTER_LOOT_TABLE`] and throw the result.
    Barter,
}

/// One brain tick of `ADMIRING_ITEM`. A `NoAI` piglin has no brain tick: the ingot stays in its
/// hand and the timer holds until the AI is switched back on.
pub fn tick_admiring(store: &mut EntityStore, key: EntityKey) -> Admire {
    let Some(state) = mob::state_mut(store, key) else {
        return Admire::Idle;
    };
    if state.kind != MobKind::Piglin || state.admiring == 0 || state.no_ai {
        return Admire::Idle;
    }
    if state.health <= 0.0 || state.loot_dropped {
        return Admire::Idle;
    }
    state.admiring -= 1;
    if state.admiring > 0 {
        return Admire::Idle;
    }
    let held = mob::equipment_stack(store, key, EquipmentSlot::OffHand).filter(|s| !s.is_empty());
    mob::set_equipment_stack(store, key, EquipmentSlot::OffHand, None);
    match held {
        Some(stack) if stack.name() == Some(BARTER_ITEM) && is_live_adult(store, key) => {
            Admire::Barter
        }
        // `stopHoldingOffHandItem` only barters a gold ingot; anything else is re-equipped or
        // stored by item-pickup behaviours that are not modelled, so it is put in the inventory.
        Some(stack) => {
            if let Some(inventory) = inventory_mut(store, key) {
                inventory.add_item(&stack);
            }
            Admire::Idle
        }
        None => Admire::Idle,
    }
}

/// `PiglinAi.wasHurtBy` / `cancelAdmiring`: hurt while holding the ingot, the piglin drops the
/// admiring state and empties its off hand without bartering, so the ingot is gone.
pub fn cancel_admiring(store: &mut EntityStore, key: EntityKey) -> bool {
    let admiring = is_admiring(store, key);
    let holding =
        mob::equipment_stack(store, key, EquipmentSlot::OffHand).is_some_and(|s| !s.is_empty());
    if !admiring && !holding {
        return false;
    }
    if let Some(state) = mob::state_mut(store, key) {
        state.admiring = 0;
    }
    mob::set_equipment_stack(store, key, EquipmentSlot::OffHand, None);
    true
}

/// Where a thrown barter result spawns: `eyeY - 0.3` above the piglin's feet.
pub fn throw_origin(feet: Vec3) -> Vec3 {
    Vec3::new(
        feet.x,
        feet.y + f64::from(MobKind::Piglin.props().eye_height) - THROW_EYE_DROP,
        feet.z,
    )
}

/// `BehaviorUtils.throwItem` velocity: toward `target + (0, 1, 0)` from the piglin's *feet*,
/// normalised and scaled by 0.3 on every axis.
pub fn throw_velocity(feet: Vec3, target: Vec3) -> Vec3 {
    let delta = Vec3::new(
        target.x - feet.x,
        target.y + THROW_TARGET_RISE - feet.y,
        target.z - feet.z,
    );
    let length = delta.length();
    if length < 1e-9 {
        return Vec3::ZERO;
    }
    Vec3::new(
        delta.x / length * THROW_SPEED,
        delta.y / length * THROW_SPEED,
        delta.z / length * THROW_SPEED,
    )
}

/// The persisted `Brain` compound for an admiring piglin, or `None` when nothing is stored.
pub fn brain_to_nbt(admiring: u16) -> Option<Compound> {
    if admiring == 0 {
        return None;
    }
    let mut memory = Compound::new();
    memory.insert("value".into(), Tag::Byte(1));
    memory.insert("ttl".into(), Tag::Long(i64::from(admiring)));
    let mut memories = Compound::new();
    memories.insert(ADMIRING_MEMORY.into(), Tag::Compound(memory));
    let mut brain = Compound::new();
    brain.insert("memories".into(), Tag::Compound(memories));
    Some(brain)
}

/// Remaining admiring ticks stored in a `Brain` compound (zero when absent or false). A memory
/// without a `ttl` never expires in vanilla; it is capped to a full admire here so an old save
/// cannot leave a piglin holding the ingot forever.
pub fn admiring_from_nbt(brain: &Compound) -> u16 {
    let Some(Tag::Compound(memories)) = brain.get("memories") else {
        return 0;
    };
    let Some(Tag::Compound(memory)) = memories.get(ADMIRING_MEMORY) else {
        return 0;
    };
    let active = match memory.get("value") {
        Some(Tag::Byte(v)) => *v != 0,
        _ => false,
    };
    if !active {
        return 0;
    }
    match memory.get("ttl") {
        Some(Tag::Long(ttl)) => (*ttl).clamp(0, i64::from(ADMIRE_TICKS)) as u16,
        Some(Tag::Int(ttl)) => (*ttl).clamp(0, i32::from(ADMIRE_TICKS)) as u16,
        _ => ADMIRE_TICKS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rng;

    fn piglin(store: &mut EntityStore, rng: &mut Rng) -> EntityKey {
        let key = store.insert(mob::new_mob(
            MobKind::Piglin,
            Vec3::new(0.5, 64.0, 0.5),
            rng,
        ));
        mob::state_mut(store, key).unwrap().age = 0;
        key
    }

    fn stack(name: &str, count: i32) -> ItemStack {
        ItemStack::from_name(name).unwrap().copy_with_count(count)
    }

    #[test]
    fn a_new_piglin_has_an_eight_slot_empty_inventory() {
        let mut store = EntityStore::default();
        let mut rng = Rng::new(1);
        let key = piglin(&mut store, &mut rng);
        let inv = inventory(&store, key).unwrap();
        assert_eq!(inv.slots().len(), INVENTORY_SLOTS);
        assert!(inv.is_empty());
        let cow = store.insert(mob::new_mob(MobKind::Cow, Vec3::ZERO, &mut rng));
        assert!(inventory(&store, cow).is_none());
    }

    #[test]
    fn the_inventory_stacks_then_fills_empty_slots_and_reports_overflow() {
        let mut inv = PiglinInventory::new();
        assert!(inv.add_item(&stack("minecraft:gold_ingot", 40)).is_empty());
        assert!(inv.add_item(&stack("minecraft:gold_ingot", 40)).is_empty());
        // 64 + 16: the second stack topped up the first.
        assert_eq!(inv.get(0).unwrap().count, 64);
        assert_eq!(inv.get(1).unwrap().count, 16);
        for item in [
            "minecraft:bread",
            "minecraft:apple",
            "minecraft:stick",
            "minecraft:coal",
            "minecraft:iron_ingot",
            "minecraft:flint",
        ] {
            assert!(inv.add_item(&stack(item, 1)).is_empty());
        }
        let extra = stack("minecraft:cobblestone", 3);
        assert!(!inv.can_add(&extra));
        assert_eq!(inv.add_item(&extra), extra);
        assert_eq!(inv.take_all().len(), 8);
        assert!(inv.is_empty());
    }

    #[test]
    fn the_inventory_round_trips_through_the_vanilla_item_list() {
        let mut inv = PiglinInventory::new();
        inv.add_item(&stack("minecraft:gold_ingot", 5));
        inv.add_item(&stack("minecraft:bread", 2));
        let tag = inv.to_nbt();
        // Vanilla's `Inventory` is a bare list of item compounds: no `Slot` key.
        let Tag::List(List::Compound(list)) = &tag else {
            panic!("inventory is a compound list");
        };
        assert_eq!(list.len(), 2);
        assert!(list.iter().all(|c| !c.contains_key("Slot")));
        assert_eq!(PiglinInventory::from_nbt(&tag), inv);
        assert!(PiglinInventory::from_nbt(&Tag::Int(1)).is_empty());
    }

    #[test]
    fn an_adult_takes_one_ingot_and_barters_after_119_ticks() {
        let mut store = EntityStore::default();
        let mut rng = Rng::new(2);
        let key = piglin(&mut store, &mut rng);
        assert!(can_admire(&store, key, BARTER_ITEM));
        assert!(!can_admire(&store, key, "minecraft:gold_block"));
        let started = start_admiring(&mut store, key).unwrap();
        assert_eq!(started.spilled, None);
        let held = mob::equipment_stack(&store, key, EquipmentSlot::OffHand).unwrap();
        assert_eq!((held.name(), held.count), (Some(BARTER_ITEM), 1));
        assert_eq!(
            store
                .data(key)
                .unwrap()
                .equipment
                .drop_chance(EquipmentSlot::OffHand),
            crate::equipment::GUARANTEED_DROP_CHANCE
        );
        for _ in 0..ADMIRE_TICKS - 1 {
            assert_eq!(tick_admiring(&mut store, key), Admire::Idle);
        }
        assert!(is_admiring(&store, key));
        assert_eq!(tick_admiring(&mut store, key), Admire::Barter);
        assert!(!is_admiring(&store, key));
        assert!(mob::equipment_stack(&store, key, EquipmentSlot::OffHand).is_none());
        assert_eq!(tick_admiring(&mut store, key), Admire::Idle);
    }

    #[test]
    fn a_baby_a_busy_piglin_and_a_dead_one_refuse_a_second_ingot() {
        let mut store = EntityStore::default();
        let mut rng = Rng::new(3);
        let adult = piglin(&mut store, &mut rng);
        let baby = piglin(&mut store, &mut rng);
        mob::state_mut(&mut store, baby).unwrap().age = crate::mob::PIGLIN_BABY_AGE;
        assert!(!can_admire(&store, baby, BARTER_ITEM));
        assert!(start_admiring(&mut store, baby).is_none());
        assert!(mob::equipment_stack(&store, baby, EquipmentSlot::OffHand).is_none());

        assert!(start_admiring(&mut store, adult).is_some());
        // The cooldown: while admiring, no second ingot is accepted and nothing changes.
        assert!(!can_admire(&store, adult, BARTER_ITEM));
        assert!(start_admiring(&mut store, adult).is_none());
        assert_eq!(
            mob::state(&store, adult).unwrap().admiring,
            ADMIRE_TICKS,
            "a refused ingot must not restart the timer"
        );

        let dead = piglin(&mut store, &mut rng);
        mob::state_mut(&mut store, dead).unwrap().health = 0.0;
        assert!(!can_admire(&store, dead, BARTER_ITEM));
    }

    #[test]
    fn a_no_ai_piglin_holds_the_ingot_without_ticking_and_resumes_later() {
        let mut store = EntityStore::default();
        let mut rng = Rng::new(4);
        let key = piglin(&mut store, &mut rng);
        mob::state_mut(&mut store, key).unwrap().no_ai = true;
        assert!(start_admiring(&mut store, key).is_some());
        for _ in 0..500 {
            assert_eq!(tick_admiring(&mut store, key), Admire::Idle);
        }
        assert_eq!(mob::state(&store, key).unwrap().admiring, ADMIRE_TICKS);
        assert!(mob::equipment_stack(&store, key, EquipmentSlot::OffHand).is_some());
        mob::state_mut(&mut store, key).unwrap().no_ai = false;
        let mut barters = 0;
        for _ in 0..ADMIRE_TICKS {
            if tick_admiring(&mut store, key) == Admire::Barter {
                barters += 1;
            }
        }
        assert_eq!(barters, 1);
    }

    #[test]
    fn being_hurt_while_admiring_cancels_the_barter_and_the_ingot_is_gone() {
        let mut store = EntityStore::default();
        let mut rng = Rng::new(5);
        let key = piglin(&mut store, &mut rng);
        assert!(!cancel_admiring(&mut store, key));
        start_admiring(&mut store, key).unwrap();
        assert!(cancel_admiring(&mut store, key));
        assert!(!is_admiring(&store, key));
        assert!(mob::equipment_stack(&store, key, EquipmentSlot::OffHand).is_none());
        for _ in 0..200 {
            assert_eq!(tick_admiring(&mut store, key), Admire::Idle);
        }
    }

    #[test]
    fn a_displaced_off_hand_item_is_returned_to_be_spilled() {
        let mut store = EntityStore::default();
        let mut rng = Rng::new(6);
        let key = piglin(&mut store, &mut rng);
        mob::set_equipment_stack(
            &mut store,
            key,
            EquipmentSlot::OffHand,
            Some(stack("minecraft:bread", 1)),
        );
        let started = start_admiring(&mut store, key).unwrap();
        assert_eq!(started.spilled, Some(stack("minecraft:bread", 1)));
    }

    #[test]
    fn the_throw_matches_behavior_utils() {
        let feet = Vec3::new(0.0, 64.0, 0.0);
        let origin = throw_origin(feet);
        let expected_eye = f64::from(MobKind::Piglin.props().eye_height);
        assert!((origin.y - (64.0 + expected_eye - THROW_EYE_DROP)).abs() < 1e-9);
        let v = throw_velocity(feet, Vec3::new(3.0, 64.0, 4.0));
        // Target + (0,1,0) from the feet: (3, 1, 4) normalised, times 0.3.
        let len = (9.0f64 + 1.0 + 16.0).sqrt();
        assert!((v.x - 3.0 / len * 0.3).abs() < 1e-9);
        assert!((v.y - 1.0 / len * 0.3).abs() < 1e-9);
        assert!((v.z - 4.0 / len * 0.3).abs() < 1e-9);
        assert_eq!(throw_velocity(feet, Vec3::new(0.0, 63.0, 0.0)), Vec3::ZERO);
    }

    #[test]
    fn the_admiring_memory_round_trips_as_a_vanilla_brain() {
        assert!(brain_to_nbt(0).is_none());
        let brain = brain_to_nbt(57).unwrap();
        assert_eq!(admiring_from_nbt(&brain), 57);
        assert_eq!(admiring_from_nbt(&Compound::new()), 0);
        let mut off = brain.clone();
        if let Some(Tag::Compound(memories)) = off.get_mut("memories")
            && let Some(Tag::Compound(memory)) = memories.get_mut(ADMIRING_MEMORY)
        {
            memory.insert("value".into(), Tag::Byte(0));
        }
        assert_eq!(admiring_from_nbt(&off), 0);
    }

    struct Wall(i32);

    impl crate::BlockGetter for Wall {
        fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
            if x == self.0 && (64..=67).contains(&y) && (-2..=2).contains(&z) {
                ferrite_data::blocks::default_state("minecraft:stone")
            } else {
                Some(0)
            }
        }
    }

    #[test]
    fn anger_nearby_targets_idle_adults_within_16_blocks_and_checks_line_of_sight() {
        let mut store = EntityStore::default();
        let mut rng = Rng::new(7);
        let player = uuid::Uuid::from_u128(0xBEEF);
        let player_pos = Vec3::new(0.5, 64.0, 0.5);

        let adult = piglin(&mut store, &mut rng);
        store.set_pos(adult, Vec3::new(8.5, 64.0, 0.5));

        let baby = piglin(&mut store, &mut rng);
        mob::state_mut(&mut store, baby).unwrap().age = mob::PIGLIN_BABY_AGE;
        store.set_pos(baby, Vec3::new(3.5, 64.0, 0.5));

        let admiring = piglin(&mut store, &mut rng);
        store.set_pos(admiring, Vec3::new(4.5, 64.0, 0.5));
        start_admiring(&mut store, admiring).unwrap();

        let far = piglin(&mut store, &mut rng);
        store.set_pos(far, Vec3::new(17.5, 64.0, 0.5));

        // A wall at x = 4 blocks line of sight for `adult` (at x = 8.5).
        assert_eq!(
            anger_nearby(&mut store, &Wall(4), player, player_pos, true),
            0
        );
        assert_eq!(mob::state(&store, adult).unwrap().target, None);

        // Without line-of-sight requirement (or with no wall), only the idle adult angers.
        assert_eq!(
            anger_nearby(&mut store, &Wall(4), player, player_pos, false),
            1
        );
        let adult_state = mob::state(&store, adult).unwrap();
        assert_eq!(adult_state.target, Some(player));
        assert_eq!(adult_state.anger, mob::PIGLIN_ANGER_TICKS);
        assert_eq!(mob::state(&store, baby).unwrap().target, None);
        assert_eq!(mob::state(&store, admiring).unwrap().target, None);
        assert_eq!(mob::state(&store, far).unwrap().target, None);
    }
}
