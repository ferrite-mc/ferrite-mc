//! Chiseled bookshelf block entities (vanilla `ChiseledBookShelfBlock` /
//! `ChiseledBookShelfBlockEntity`).
//!
//! Stores up to six book stacks in its block entity NBT (`Items`, `last_interacted_slot`) and
//! synchronizes the `slot_0_occupied`..=`slot_5_occupied` block state properties.

use ferrite_item::ItemStack;
use ferrite_nbt::{Compound, List, Tag};
use ferrite_world::BlockEntity;

use super::anvil::AnvilChunkSource;
use super::blocks::with_props;

/// The block and block entity type name of a chiseled bookshelf.
pub const BLOCK: &str = "minecraft:chiseled_bookshelf";
/// Number of book slots in a chiseled bookshelf.
pub const SLOTS: usize = 6;

const SLOT_PROPS: [&str; SLOTS] = [
    "slot_0_occupied",
    "slot_1_occupied",
    "slot_2_occupied",
    "slot_3_occupied",
    "slot_4_occupied",
    "slot_5_occupied",
];

/// Whether an item name is accepted by `#minecraft:bookshelf_books`.
pub fn is_bookshelf_book(name: &str) -> bool {
    matches!(
        name,
        "minecraft:book"
            | "minecraft:written_book"
            | "minecraft:writable_book"
            | "minecraft:enchanted_book"
            | "minecraft:knowledge_book"
    )
}

/// Stored state of a chiseled bookshelf.
#[derive(Clone, Debug, PartialEq)]
pub struct BookshelfData {
    pub items: [ItemStack; SLOTS],
    pub last_interacted_slot: i32,
}

impl Default for BookshelfData {
    fn default() -> Self {
        Self {
            items: std::array::from_fn(|_| ItemStack::empty()),
            last_interacted_slot: -1,
        }
    }
}

impl BookshelfData {
    /// Vanilla comparator output (`ChiseledBookShelfBlock.getAnalogOutputSignal`):
    /// `last_interacted_slot + 1` (0 when untouched, 1..=6 after an interaction).
    pub fn comparator_signal(&self) -> u8 {
        if (0..SLOTS as i32).contains(&self.last_interacted_slot) {
            (self.last_interacted_slot + 1) as u8
        } else {
            0
        }
    }
}

/// Loads a chiseled bookshelf's books and `last_interacted_slot` from its block entity NBT.
pub fn load(source: &AnvilChunkSource, [x, y, z]: [i32; 3]) -> BookshelfData {
    let mut data = BookshelfData::default();
    let Some(entity) = source.block_entity(x, y, z) else {
        return data;
    };
    if let Some(Tag::Int(slot)) = entity.data.get("last_interacted_slot") {
        data.last_interacted_slot = *slot;
    }
    if let Some(Tag::List(List::Compound(entries))) = entity.data.get("Items") {
        for entry in entries {
            let slot = match entry.get("Slot") {
                Some(Tag::Byte(s)) => usize::try_from(*s).ok(),
                Some(Tag::Int(s)) => usize::try_from(*s).ok(),
                _ => None,
            };
            let Some(slot) = slot.filter(|&s| s < SLOTS) else {
                continue;
            };
            if let Ok(stack) = ItemStack::from_nbt(entry)
                && !stack.is_empty()
            {
                data.items[slot] = stack;
            }
        }
    }
    data
}

/// Writes a chiseled bookshelf's books and `last_interacted_slot` back to its block entity NBT.
pub fn store(source: &AnvilChunkSource, [x, y, z]: [i32; 3], data: &BookshelfData) {
    let Some(type_id) = ferrite_data::registries::id("minecraft:block_entity_type", BLOCK)
        .and_then(|id| u32::try_from(id).ok())
    else {
        return;
    };
    let mut list = Vec::new();
    for (slot, stack) in data.items.iter().enumerate() {
        if stack.is_empty() {
            continue;
        }
        if let Ok(mut entry) = stack.to_nbt() {
            entry.insert("Slot".into(), Tag::Byte(slot as i8));
            list.push(entry);
        }
    }
    let result = source.update_block_entity(x, y, z, |entity| {
        let entity = entity.get_or_insert_with(|| {
            let mut nbt = Compound::new();
            nbt.insert("id".into(), Tag::String(BLOCK.into()));
            nbt.insert("x".into(), Tag::Int(x));
            nbt.insert("y".into(), Tag::Int(y));
            nbt.insert("z".into(), Tag::Int(z));
            BlockEntity { type_id, data: nbt }
        });
        entity.data.insert(
            "last_interacted_slot".into(),
            Tag::Int(data.last_interacted_slot),
        );
        entity
            .data
            .insert("Items".into(), Tag::List(List::Compound(list.clone())));
        (true, ())
    });
    if let Err(error) = result {
        tracing::warn!(x, y, z, %error, "could not store chiseled bookshelf state");
    }
}

/// Ensures a freshly placed chiseled bookshelf has its block entity.
pub fn ensure(source: &AnvilChunkSource, pos: [i32; 3]) {
    store(source, pos, &BookshelfData::default());
}

/// Outcome of right-clicking a chiseled bookshelf.
#[derive(Debug, PartialEq)]
pub struct BookshelfAction {
    pub new_state: u16,
    pub extracted: Option<ItemStack>,
    pub consumed_held: bool,
}

/// Inserts a held book into the first free slot or extracts the last occupied book with an empty
/// hand, persisting NBT and computing the updated block state.
pub fn interact(
    source: &AnvilChunkSource,
    pos: [i32; 3],
    current_state: u16,
    held: &ItemStack,
    creative: bool,
) -> Option<BookshelfAction> {
    let mut data = load(source, pos);
    let (extracted, consumed_held) =
        if !held.is_empty() && held.name().is_some_and(is_bookshelf_book) {
            let slot = data.items.iter().position(ItemStack::is_empty)?;
            let mut single = held.clone();
            single.count = 1;
            data.items[slot] = single;
            data.last_interacted_slot = slot as i32;
            (None, !creative)
        } else if held.is_empty() {
            let slot = data.items.iter().rposition(|s| !s.is_empty())?;
            let book = std::mem::replace(&mut data.items[slot], ItemStack::empty());
            data.last_interacted_slot = slot as i32;
            (Some(book), false)
        } else {
            return None;
        };

    store(source, pos, &data);
    let changes: [(&str, &str); SLOTS] = std::array::from_fn(|i| {
        (
            SLOT_PROPS[i],
            if data.items[i].is_empty() {
                "false"
            } else {
                "true"
            },
        )
    });
    let new_state = with_props(current_state, &changes).unwrap_or(current_state);
    Some(BookshelfAction {
        new_state,
        extracted,
        consumed_held,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bookshelf_inserts_extracts_and_tracks_block_state_and_comparator() {
        let source = AnvilChunkSource::new_memory();
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let pos = [4, 64, 4];
        let base = ferrite_data::blocks::default_state("chiseled_bookshelf").unwrap();
        source
            .set_block(pos[0], pos[1], pos[2], u32::from(base))
            .unwrap();
        ensure(&source, pos);
        assert_eq!(load(&source, pos).comparator_signal(), 0);

        let book = ItemStack::from_name("minecraft:book").unwrap();
        let insert = interact(&source, pos, base, &book, false).expect("book inserts into slot 0");
        assert!(insert.consumed_held);
        assert!(insert.extracted.is_none());
        assert_eq!(
            super::super::blocks::property(insert.new_state, "slot_0_occupied"),
            Some("true")
        );
        assert_eq!(
            super::super::blocks::property(insert.new_state, "slot_1_occupied"),
            Some("false")
        );
        assert_eq!(load(&source, pos).comparator_signal(), 1);

        let enchanted = ItemStack::from_name("minecraft:enchanted_book").unwrap();
        let insert2 = interact(&source, pos, insert.new_state, &enchanted, true)
            .expect("second book inserts into slot 1");
        assert!(!insert2.consumed_held, "creative does not consume book");
        assert_eq!(
            super::super::blocks::property(insert2.new_state, "slot_1_occupied"),
            Some("true")
        );
        assert_eq!(load(&source, pos).comparator_signal(), 2);

        let take = interact(&source, pos, insert2.new_state, &ItemStack::empty(), false)
            .expect("empty hand extracts last book");
        assert_eq!(
            take.extracted.as_ref().and_then(ItemStack::name),
            Some("minecraft:enchanted_book")
        );
        assert_eq!(
            super::super::blocks::property(take.new_state, "slot_1_occupied"),
            Some("false")
        );
        assert_eq!(
            super::super::blocks::property(take.new_state, "slot_0_occupied"),
            Some("true")
        );
    }
}
