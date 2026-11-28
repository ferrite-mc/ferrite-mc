//! Allay delivery state (vanilla `Allay` + `AllayAi`, 26.3).
//!
//! An allay only works while it has an errand: a player gives it one item, it collects nearby
//! stacks of the same item (up to a stack) and then throws everything at that player, or drops
//! a cake on a note block. Vanilla keeps the given item in the allay's one-slot inventory; this
//! module keeps the same information on the entity record so it survives region hand-offs and
//! is written to the entity save.

use ferrite_item::ItemStack;
use ferrite_nbt::{Compound, List, Tag};

/// `AllayAi` looks for loose items this far away (vanilla searches 32 blocks).
pub const SEARCH_RADIUS: f64 = 32.0;
/// A played note block attracts an allay within 16 blocks.
pub const NOTE_BLOCK_RADIUS: i32 = 16;
/// Ticks between two pickups: vanilla's three-second allay pickup cooldown.
pub const PICKUP_COOLDOWN: u16 = 60;
/// The allay's inventory holds one stack (`new SimpleContainer(1)`).
const CARGO_SLOTS: usize = 1;

/// What an allay was given and is carrying.
#[derive(Debug, Clone, PartialEq)]
pub struct AllayState {
    /// The stack being collected for delivery.
    pub cargo: Option<ItemStack>,
    /// The item the player gave it; it only picks up matching stacks for this errand.
    pub interest: Option<ItemStack>,
    /// Ticks before it may pick up another item.
    pub cooldown: u16,
}

impl AllayState {
    /// The state after a player gives the allay one item: an errand for that item and nothing
    /// carried yet.
    pub fn given(stack: &ItemStack) -> Self {
        Self {
            cargo: None,
            interest: Some(stack.copy_with_count(1)),
            cooldown: 0,
        }
    }

    /// Whether the allay still has an errand (vanilla stops collecting once the given item was
    /// delivered).
    pub fn has_errand(&self) -> bool {
        self.interest.is_some()
    }

    /// Whether a loose stack is the item this allay is collecting (`isSameItemSameComponents`).
    pub fn wants(&self, stack: &ItemStack) -> bool {
        self.interest
            .as_ref()
            .is_some_and(|want| want.is_same_item_same_components(stack))
    }

    /// Adds a picked-up stack, returning whatever did not fit in the one cargo slot.
    pub fn add(&mut self, mut stack: ItemStack) -> ItemStack {
        match &mut self.cargo {
            Some(cargo) => {
                cargo.merge(&mut stack);
                stack
            }
            None => {
                let mut cargo = ItemStack::empty();
                cargo.merge(&mut stack);
                self.cargo = Some(cargo);
                stack
            }
        }
    }

    /// Removes the cargo for delivery.
    pub fn take_cargo(&mut self) -> Option<ItemStack> {
        self.cargo.take()
    }

    /// Ends the errand after a delivery: empty hands, no interest, and the pickup cooldown.
    pub fn finish(&mut self) {
        self.cargo = None;
        self.interest = None;
        self.cooldown = PICKUP_COOLDOWN;
    }

    /// The allay's save fields: `Inventory` (the vanilla one-slot container list), the item it
    /// was given, and the pickup cooldown.
    pub fn to_nbt(&self) -> Compound {
        let mut c = Compound::new();
        if let Some(cargo) = &self.cargo
            && let Ok(tag) = cargo.to_nbt()
        {
            c.insert("Inventory".into(), Tag::List(List::Compound(vec![tag])));
        }
        if let Some(interest) = &self.interest
            && let Ok(mut tag) = interest.to_nbt()
        {
            // The save format stores the container's single stack as a plain item compound.
            tag.insert("count".into(), Tag::Int(1));
            c.insert("AllayInterest".into(), Tag::Compound(tag));
        }
        if self.cooldown != 0 {
            c.insert("AllayCooldown".into(), Tag::Short(self.cooldown as i16));
        }
        c
    }

    /// Reads what [`Self::to_nbt`] wrote.
    pub fn from_nbt(c: &Compound) -> Self {
        let cargo = match c.get("Inventory") {
            Some(Tag::List(List::Compound(list))) => list
                .first()
                .and_then(|tag| ItemStack::from_nbt(tag).ok())
                .filter(|stack| !stack.is_empty()),
            _ => None,
        };
        let interest = match c.get("AllayInterest") {
            Some(Tag::Compound(tag)) => ItemStack::from_nbt(tag)
                .ok()
                .filter(|stack| !stack.is_empty()),
            _ => None,
        };
        let cooldown = match c.get("AllayCooldown") {
            Some(Tag::Short(value)) => (*value).max(0) as u16,
            Some(Tag::Int(value)) => (*value).clamp(0, i32::from(u16::MAX)) as u16,
            _ => 0,
        };
        Self {
            cargo,
            interest,
            cooldown,
        }
    }
}

/// The allay's cargo slot count, for tests and documentation.
pub const fn cargo_slots() -> usize {
    CARGO_SLOTS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(name: &str, count: i32) -> ItemStack {
        ItemStack::from_name(name)
            .expect("a vanilla item")
            .copy_with_count(count)
    }

    #[test]
    fn an_allay_only_collects_the_item_it_was_given() {
        let mut allay = AllayState::given(&stack("minecraft:dirt", 1));
        assert!(allay.has_errand());
        assert!(allay.wants(&stack("minecraft:dirt", 64)));
        assert!(!allay.wants(&stack("minecraft:stone", 1)));
        assert!(allay.cargo.is_none());

        let rest = allay.add(stack("minecraft:dirt", 3));
        assert!(rest.is_empty());
        assert_eq!(allay.cargo.as_ref().unwrap().count, 3);
        // A second pickup fills the same stack.
        let rest = allay.add(stack("minecraft:dirt", 2));
        assert!(rest.is_empty());
        assert_eq!(allay.cargo.as_ref().unwrap().count, 5);

        allay.finish();
        assert!(!allay.has_errand());
        assert!(allay.cargo.is_none());
        assert_eq!(allay.cooldown, PICKUP_COOLDOWN);
    }

    #[test]
    fn a_full_cargo_stack_leaves_the_rest_on_the_ground() {
        let mut allay = AllayState::given(&stack("minecraft:dirt", 1));
        allay.add(stack("minecraft:dirt", 64));
        let rest = allay.add(stack("minecraft:dirt", 5));
        assert_eq!(rest.count, 5);
        assert_eq!(allay.cargo.as_ref().unwrap().count, 64);
        assert_eq!(cargo_slots(), 1);
    }

    #[test]
    fn allay_state_round_trips_through_its_save_fields() {
        let mut allay = AllayState::given(&stack("minecraft:cake", 1));
        allay.add(stack("minecraft:cake", 2));
        allay.cooldown = 17;
        let tag = allay.to_nbt();
        let back = AllayState::from_nbt(&tag);
        assert_eq!(back, allay);
        assert!(AllayState::from_nbt(&Compound::new()).interest.is_none());
    }
}
