//! The player's own slots (vanilla `Inventory`).

use ferrite_item::{Component, ItemStack, component_ids};
use ferrite_nbt::Tag;

/// Hotbar slots, `0..HOTBAR`.
pub const HOTBAR: usize = 9;
/// Item slots (hotbar and main inventory), `0..ITEMS`.
pub const ITEMS: usize = 36;
/// Armor slots, in vanilla `EquipmentSlot` index order.
pub const FEET: usize = 36;
pub const LEGS: usize = 37;
pub const CHEST: usize = 38;
pub const HEAD: usize = 39;
/// The offhand slot, which is also what the `SWAP` button 40 (key F) names.
pub const OFFHAND: usize = 40;
/// Number of slots.
pub const SIZE: usize = 41;

/// Vanilla `Inventory`, numbered like vanilla: `0..9` hotbar, `9..36` main inventory,
/// [`FEET`]..=[`HEAD`] armor and [`OFFHAND`].
#[derive(Clone, Debug)]
pub struct PlayerInventory {
    slots: Vec<ItemStack>,
    /// The selected hotbar slot, `0..9`.
    pub selected: usize,
    /// Bumped whenever a slot changes. Menus use it to avoid comparing all 41 stacks on an
    /// idle tick; the revision is intentionally not part of equality or persistence.
    revision: u64,
}

impl PartialEq for PlayerInventory {
    fn eq(&self, other: &Self) -> bool {
        self.slots == other.slots && self.selected == other.selected
    }
}

impl Default for PlayerInventory {
    fn default() -> Self {
        Self {
            slots: vec![ItemStack::empty(); SIZE],
            selected: 0,
            revision: 0,
        }
    }
}

impl PlayerInventory {
    /// An empty inventory.
    pub fn new() -> Self {
        Self::default()
    }

    /// The stack in a slot; out-of-range slots read as empty.
    pub fn get(&self, slot: usize) -> &ItemStack {
        static EMPTY: ItemStack = ItemStack {
            item: 0,
            count: 0,
            components: ferrite_item::ComponentPatch::new(),
        };
        self.slots.get(slot).unwrap_or(&EMPTY)
    }

    /// Replaces a slot. Empty stacks are normalized to air; out-of-range slots are ignored.
    pub fn set(&mut self, slot: usize, stack: ItemStack) {
        if let Some(target) = self.slots.get_mut(slot) {
            let next = if stack.is_empty() {
                ItemStack::empty()
            } else {
                stack
            };
            if *target != next {
                *target = next;
                self.revision = self.revision.wrapping_add(1);
            }
        }
    }

    /// Monotonic revision of the slot contents (not the selected hotbar slot).
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Every slot, in vanilla order.
    pub fn slots(&self) -> &[ItemStack] {
        &self.slots
    }

    /// The hotbar slot index of the main hand.
    pub fn selected_slot(&self) -> usize {
        self.selected.min(HOTBAR - 1)
    }

    /// The main hand stack.
    pub fn selected_item(&self) -> &ItemStack {
        self.get(self.selected_slot())
    }

    /// Vanilla `Inventory.removeItem(slot, amount)`: splits up to `amount` items off a slot.
    pub fn remove(&mut self, slot: usize, amount: i32) -> ItemStack {
        match self.slots.get_mut(slot) {
            Some(stack) => {
                let taken = stack.split(amount).unwrap_or_default();
                if stack.is_empty() {
                    *stack = ItemStack::empty();
                }
                if !taken.is_empty() {
                    self.revision = self.revision.wrapping_add(1);
                }
                taken
            }
            None => ItemStack::empty(),
        }
    }

    fn has_remaining_space(&self, slot: usize, stack: &ItemStack) -> bool {
        let existing = self.get(slot);
        !existing.is_empty()
            && existing.is_same_item_same_components(stack)
            && is_stackable(existing)
            && existing.count < max_in_slot(existing)
    }

    /// Vanilla `getSlotWithRemainingSpace`: the selected slot, the offhand, then the items.
    fn slot_with_remaining_space(&self, stack: &ItemStack) -> Option<usize> {
        [self.selected_slot(), OFFHAND]
            .into_iter()
            .chain(0..ITEMS)
            .find(|&slot| self.has_remaining_space(slot, stack))
    }

    /// Vanilla `getFreeSlot`.
    fn free_slot(&self) -> Option<usize> {
        (0..ITEMS).find(|&slot| self.get(slot).is_empty())
    }

    /// Vanilla `addResource(slot, stack)`: moves what fits into `slot` and returns the rest.
    fn add_resource_to(&mut self, slot: usize, stack: &ItemStack) -> i32 {
        let mut existing = self.get(slot).clone();
        if existing.is_empty() {
            existing = stack.copy_with_count(0);
        }
        let moved = stack.count.min(max_in_slot(&existing) - existing.count);
        if moved <= 0 {
            return stack.count;
        }
        existing.count += moved;
        self.set(slot, existing);
        stack.count - moved
    }

    /// Vanilla `Inventory.add(stack)`: adds as much of `stack` as fits, shrinking it, and
    /// returns whether anything was taken. Damaged items take a free slot whole. A player with
    /// infinite materials (creative) takes the whole stack even when it does not fit.
    pub fn add(&mut self, stack: &mut ItemStack, infinite_materials: bool) -> bool {
        if stack.is_empty() {
            return false;
        }
        if stack.is_damageable() && stack.damage() > 0 {
            if let Some(slot) = self.free_slot() {
                self.set(slot, std::mem::take(stack));
                return true;
            }
            if infinite_materials {
                *stack = ItemStack::empty();
                return true;
            }
            return false;
        }
        let before = stack.count;
        loop {
            let last = stack.count;
            let rest = match self
                .slot_with_remaining_space(stack)
                .or_else(|| self.free_slot())
            {
                Some(slot) => self.add_resource_to(slot, stack),
                None => stack.count,
            };
            stack.count = rest;
            if stack.count <= 0 || stack.count >= last {
                break;
            }
        }
        if stack.count <= 0 {
            *stack = ItemStack::empty();
        } else if stack.count == before && infinite_materials {
            *stack = ItemStack::empty();
            return true;
        }
        stack.count < before
    }

    /// Vanilla `placeItemBackInInventory`: stores the stack in slots with room or free slots and
    /// returns what did not fit, which vanilla drops.
    pub fn place_back(&mut self, mut stack: ItemStack) -> Option<ItemStack> {
        while !stack.is_empty() {
            let Some(slot) = self
                .slot_with_remaining_space(&stack)
                .or_else(|| self.free_slot())
            else {
                return Some(stack);
            };
            let room = stack.max_stack_size() - self.get(slot).count;
            let part = stack.split(room).unwrap_or_default();
            let rest = self.add_resource_to(slot, &part);
            stack.count += rest;
            if rest == part.count {
                return Some(stack);
            }
        }
        None
    }
}

/// Vanilla `ItemStack.isStackable`.
pub(crate) fn is_stackable(stack: &ItemStack) -> bool {
    stack.max_stack_size() > 1 && (!stack.is_damageable() || stack.damage() == 0)
}

/// `Container.getMaxStackSize(stack)` of a plain container slot.
pub(crate) fn max_in_slot(stack: &ItemStack) -> i32 {
    stack.max_stack_size().min(ferrite_item::MAX_COUNT)
}

/// The slot an item is worn in (vanilla `LivingEntity.getEquipmentSlotForItem`): the `slot` of
/// its `minecraft:equippable` component, or `"mainhand"`.
pub fn equipment_slot(stack: &ItemStack) -> &'static str {
    let slot = match stack.effective_component(component_ids::EQUIPPABLE) {
        Some(Component::Opaque(Tag::Compound(equippable))) => match equippable.get("slot") {
            Some(Tag::String(slot)) => slot.clone(),
            _ => return "mainhand",
        },
        _ => return "mainhand",
    };
    match slot.strip_prefix("minecraft:").unwrap_or(&slot) {
        "offhand" => "offhand",
        "feet" => "feet",
        "legs" => "legs",
        "chest" => "chest",
        "head" => "head",
        "body" => "body",
        "saddle" => "saddle",
        _ => "mainhand",
    }
}

/// Whether an item has an enchantment with the `prevent_armor_change` effect (the curse of
/// binding), which locks it in an armor slot outside creative.
pub(crate) fn prevents_armor_change(stack: &ItemStack) -> bool {
    matches!(
        stack.effective_component(component_ids::ENCHANTMENTS),
        Some(Component::Enchantments(enchantments))
            if enchantments.level("minecraft:binding_curse").is_some_and(|level| level > 0)
    )
}
