//! Allay delivery state (vanilla `Allay` + `AllayAi`, 26.3).
//!
//! An allay only works while it has an errand: a player gives it one item, it collects nearby
//! stacks of the same item (up to a stack) and then throws everything at that player, or at a
//! note block it heard within the last thirty seconds. Vanilla keeps the given item in the
//! allay's one-slot inventory; this
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
/// `AllayAi.TIME_TO_FORGET_NOTEBLOCK`: a played note block is remembered for 600 ticks.
pub const NOTE_BLOCK_MEMORY_TICKS: u16 = 600;
/// An allay deposits at a remembered note block within 32 blocks (`isCloseEnough(..., 1024)`).
pub const NOTE_BLOCK_DELIVERY_DISTANCE: f64 = 32.0;
/// The allay's inventory holds one stack (`new SimpleContainer(1)`).
const CARGO_SLOTS: usize = 1;

/// A note block an allay heard (`LIKED_NOTEBLOCK_POSITION` + cooldown memory).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteBlockMemory {
    pub pos: [i32; 3],
    /// Ticks before the memory is forgotten; refreshed by hearing the same block again.
    pub ticks: u16,
}

/// What an allay was given and is carrying.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AllayState {
    /// The stack being collected for delivery.
    pub cargo: Option<ItemStack>,
    /// The item the player gave it; it only picks up matching stacks for this errand.
    pub interest: Option<ItemStack>,
    /// Ticks before it may pick up another item.
    pub cooldown: u16,
    /// The note block this allay heard, until it forgets it after 30 seconds.
    pub note_block: Option<NoteBlockMemory>,
}

impl AllayState {
    /// The state after a player gives the allay one item: an errand for that item and nothing
    /// carried yet.
    pub fn given(stack: &ItemStack) -> Self {
        Self {
            cargo: None,
            interest: Some(stack.copy_with_count(1)),
            cooldown: 0,
            note_block: None,
        }
    }

    /// `AllayAi.hearNoteblock`: the first block remembered wins; hearing the same block again
    /// refreshes its 600-tick memory.
    pub fn hear_note_block(&mut self, pos: [i32; 3]) {
        match &mut self.note_block {
            Some(memory) if memory.pos == pos => memory.ticks = NOTE_BLOCK_MEMORY_TICKS,
            Some(_) => {}
            None => {
                self.note_block = Some(NoteBlockMemory {
                    pos,
                    ticks: NOTE_BLOCK_MEMORY_TICKS,
                });
            }
        }
    }

    /// Counts the note block memory down by one tick.
    pub fn tick_note_block(&mut self) {
        let Some(memory) = &mut self.note_block else {
            return;
        };
        memory.ticks = memory.ticks.saturating_sub(1);
        if memory.ticks == 0 {
            self.note_block = None;
        }
    }

    /// The note block an allay should deposit at, if it still remembers one.
    pub fn liked_note_block(&self) -> Option<[i32; 3]> {
        self.note_block
            .filter(|memory| memory.ticks > 0)
            .map(|memory| memory.pos)
    }

    /// Forgets a note block that was broken or is out of range.
    pub fn forget_note_block(&mut self) {
        self.note_block = None;
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
        if let Some(memory) = self.note_block {
            c.insert("AllayNoteBlock".into(), Tag::IntArray(memory.pos.to_vec()));
            c.insert(
                "AllayNoteBlockTicks".into(),
                Tag::Int(i32::from(memory.ticks)),
            );
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
        let note_block = match (c.get("AllayNoteBlock"), c.get("AllayNoteBlockTicks")) {
            (Some(Tag::IntArray(pos)), ticks) if pos.len() == 3 => {
                let ticks = match ticks {
                    Some(Tag::Int(value)) => (*value).clamp(0, i32::from(u16::MAX)) as u16,
                    Some(Tag::Short(value)) => (*value).max(0) as u16,
                    _ => 0,
                };
                (ticks > 0).then_some(NoteBlockMemory {
                    pos: [pos[0], pos[1], pos[2]],
                    ticks,
                })
            }
            _ => None,
        };
        Self {
            cargo,
            interest,
            cooldown,
            note_block,
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
        allay.hear_note_block([4, 70, -3]);
        let tag = allay.to_nbt();
        let back = AllayState::from_nbt(&tag);
        assert_eq!(back, allay);
        assert!(AllayState::from_nbt(&Compound::new()).interest.is_none());
    }

    #[test]
    fn a_note_block_is_remembered_for_thirty_seconds() {
        let mut allay = AllayState::default();
        assert_eq!(allay.liked_note_block(), None);
        allay.hear_note_block([1, 64, 2]);
        assert_eq!(allay.liked_note_block(), Some([1, 64, 2]));
        // A different note block does not replace the remembered one.
        allay.hear_note_block([9, 64, 9]);
        assert_eq!(allay.liked_note_block(), Some([1, 64, 2]));
        for _ in 0..NOTE_BLOCK_MEMORY_TICKS - 1 {
            allay.tick_note_block();
        }
        assert_eq!(allay.liked_note_block(), Some([1, 64, 2]));
        // Hearing the same block again refreshes the memory.
        allay.hear_note_block([1, 64, 2]);
        for _ in 0..NOTE_BLOCK_MEMORY_TICKS {
            allay.tick_note_block();
        }
        assert_eq!(allay.liked_note_block(), None);
        allay.hear_note_block([1, 64, 2]);
        allay.forget_note_block();
        assert_eq!(allay.liked_note_block(), None);
    }
}
