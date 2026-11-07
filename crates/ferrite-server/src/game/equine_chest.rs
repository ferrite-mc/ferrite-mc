//! The chest of a donkey or mule: its items, kept by entity uuid so a menu opened by a player
//! (who has no access to the region's entity store) and the entity save see the same list.
//! The menu is vanilla's: three rows of five columns for a donkey or mule, three rows of
//! `strength` columns for a llama.

use ferrite_item::ItemStack;
use ferrite_nbt::{Compound, List, Tag};
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use uuid::Uuid;

/// The most slots any animal's chest holds (5 columns of 3).
const MAX_SLOTS: usize = 15;

static CHESTS: Mutex<Option<HashMap<Uuid, Vec<ItemStack>>>> = Mutex::new(None);

fn chests() -> MutexGuard<'static, Option<HashMap<Uuid, Vec<ItemStack>>>> {
    CHESTS.lock().unwrap_or_else(|e| e.into_inner())
}

/// The items of `animal`'s chest, padded to `len` slots.
pub fn load(animal: Uuid, len: usize) -> Vec<ItemStack> {
    let mut items = chests()
        .get_or_insert_with(HashMap::new)
        .get(&animal)
        .cloned()
        .unwrap_or_default();
    items.resize(len, ItemStack::empty());
    items
}

/// Replaces the items of `animal`'s chest.
pub fn store(animal: Uuid, items: &[ItemStack]) {
    chests()
        .get_or_insert_with(HashMap::new)
        .insert(animal, items.to_vec());
}

/// What a dead donkey, mule or llama drops besides its loot table: the chest, its items and the saddle.
pub fn gear(animal: Uuid, chest: bool, saddled: bool) -> Vec<ItemStack> {
    let mut gear = Vec::new();
    if chest {
        gear.extend(load(animal, MAX_SLOTS));
        gear.extend(ItemStack::from_name("minecraft:chest"));
    }
    if saddled {
        gear.extend(ItemStack::from_name("minecraft:saddle"));
    }
    gear.retain(|stack| !stack.is_empty());
    gear
}

/// The chest as an NBT list of `{Slot, ...item}` compounds (empty slots left out).
pub fn to_nbt(animal: Uuid) -> Tag {
    let list = load(animal, MAX_SLOTS)
        .iter()
        .enumerate()
        .filter(|(_, stack)| !stack.is_empty())
        .filter_map(|(slot, stack)| {
            let mut c = stack.to_nbt().ok()?;
            c.insert("Slot".into(), Tag::Byte(slot as i8));
            Some(c)
        })
        .collect::<Vec<Compound>>();
    Tag::List(List::Compound(list))
}

/// Reads what [`to_nbt`] wrote back into the chest of `animal`.
pub fn from_nbt(animal: Uuid, tag: &Tag) {
    let Tag::List(List::Compound(list)) = tag else {
        return;
    };
    let mut items = vec![ItemStack::empty(); MAX_SLOTS];
    for c in list {
        let Some(Tag::Byte(slot)) = c.get("Slot") else {
            continue;
        };
        if let (Some(at), Ok(stack)) = (items.get_mut(*slot as usize), ItemStack::from_nbt(c)) {
            *at = stack;
        }
    }
    store(animal, &items);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dead_pack_animal_drops_its_chest_items_and_saddle() {
        let a = Uuid::from_u128(81);
        let mut items = load(a, 15);
        items[0] = ItemStack::from_name("minecraft:bread")
            .unwrap()
            .copy_with_count(2);
        store(a, &items);
        let names: Vec<_> = gear(a, true, true)
            .iter()
            .map(|s| s.name().unwrap().to_string())
            .collect();
        assert_eq!(
            names,
            ["minecraft:bread", "minecraft:chest", "minecraft:saddle"]
        );
        assert!(gear(Uuid::from_u128(82), false, false).is_empty());
    }

    #[test]
    fn a_chest_keeps_its_items_through_nbt() {
        let (a, b) = (Uuid::from_u128(71), Uuid::from_u128(72));
        let mut items = load(a, 15);
        items[3] = ItemStack::from_name("minecraft:bread")
            .unwrap()
            .copy_with_count(5);
        store(a, &items);
        from_nbt(b, &to_nbt(a));
        let back = load(b, 15);
        assert_eq!(back[3].name(), Some("minecraft:bread"));
        assert_eq!(back[3].count, 5);
        assert!(back[0].is_empty());
    }
}
