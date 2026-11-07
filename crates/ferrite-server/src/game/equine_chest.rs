//! The chest of a donkey or mule: its items, kept by entity uuid so a menu opened by a player
//! (who has no access to the region's entity store) and the entity save see the same list.
//! The menu is vanilla's: three rows of five columns for a donkey or mule, three rows of
//! `strength` columns for a llama.

use ferrite_entity::mob::MobKind;
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

/// Removes `animal`'s chest and returns its items, padded to [`MAX_SLOTS`] slots. Death uses this
/// so dropped contents cannot be saved or shown in a screen a second time.
pub fn take(animal: Uuid) -> Vec<ItemStack> {
    let mut items = chests()
        .get_or_insert_with(HashMap::new)
        .remove(&animal)
        .unwrap_or_default();
    items.resize(MAX_SLOTS, ItemStack::empty());
    items
}

/// Forgets `animal`'s chest without returning its items (a dead animal that dropped nothing).
pub fn clear(animal: Uuid) {
    chests().get_or_insert_with(HashMap::new).remove(&animal);
}

/// What a dead horse drops besides its loot table: its equipped saddle and horse armour. Pack
/// equines use [`gear`]; llamas never take horse armour, so they never drop it.
pub fn horse_gear(saddled: bool, armor: u16) -> Vec<ItemStack> {
    let mut gear = Vec::new();
    if saddled {
        gear.extend(ItemStack::from_name("minecraft:saddle"));
    }
    if armor != 0 && ferrite_entity::horse::body_armor_fits(MobKind::Horse, armor) {
        gear.push(ItemStack::with_count(armor, 1));
    }
    gear
}

/// What a dead donkey, mule or llama drops besides its loot table: the chest, its items, the
/// saddle and body gear (a llama's carpet). The chest is taken out of the store, so a save or a
/// screen opened around the death cannot duplicate the dropped contents.
pub fn gear(animal: Uuid, kind: MobKind, chest: bool, saddled: bool, armor: u16) -> Vec<ItemStack> {
    let mut gear = Vec::new();
    let stored = take(animal);
    if chest {
        gear.extend(stored);
        gear.extend(ItemStack::from_name("minecraft:chest"));
    }
    if saddled && !kind.is_llama() {
        gear.extend(ItemStack::from_name("minecraft:saddle"));
    }
    if armor != 0 && ferrite_entity::horse::body_armor_fits(kind, armor) {
        gear.push(ItemStack::with_count(armor, 1));
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
        let names: Vec<_> = gear(a, MobKind::Donkey, true, true, 0)
            .iter()
            .map(|s| s.name().unwrap().to_string())
            .collect();
        assert_eq!(
            names,
            ["minecraft:bread", "minecraft:chest", "minecraft:saddle"]
        );
        assert!(
            load(a, 15).iter().all(ItemStack::is_empty),
            "the dropped chest is gone"
        );
        assert!(gear(Uuid::from_u128(82), MobKind::Donkey, false, false, 0).is_empty());
    }

    #[test]
    fn a_dead_llama_drops_its_carpet_but_never_horse_armor() {
        let carpet = ItemStack::from_name("minecraft:white_carpet").unwrap();
        let names: Vec<_> = gear(
            Uuid::from_u128(83),
            MobKind::Llama,
            false,
            false,
            carpet.item,
        )
        .iter()
        .map(|stack| stack.name().unwrap())
        .collect();
        assert_eq!(names, ["minecraft:white_carpet"]);

        let armor = ItemStack::from_name("minecraft:iron_horse_armor").unwrap();
        assert!(
            gear(
                Uuid::from_u128(84),
                MobKind::Llama,
                false,
                false,
                armor.item
            )
            .is_empty(),
            "a llama never drops horse armour"
        );
        assert!(
            gear(
                Uuid::from_u128(85),
                MobKind::Donkey,
                false,
                true,
                armor.item
            )
            .iter()
            .all(|stack| stack.name() != Some("minecraft:iron_horse_armor")),
            "a donkey has no body slot"
        );
        assert!(
            gear(
                Uuid::from_u128(86),
                MobKind::Llama,
                true,
                false,
                carpet.item
            )
            .iter()
            .any(|stack| stack.name() == Some("minecraft:chest"))
        );
    }

    #[test]
    fn horse_gear_drops_a_saddle_and_every_horse_armor_tier() {
        for name in [
            "minecraft:leather_horse_armor",
            "minecraft:copper_horse_armor",
            "minecraft:iron_horse_armor",
            "minecraft:golden_horse_armor",
            "minecraft:diamond_horse_armor",
            "minecraft:netherite_horse_armor",
        ] {
            let armor = ItemStack::from_name(name).unwrap();
            let names: Vec<_> = horse_gear(true, armor.item)
                .iter()
                .map(|stack| stack.name().unwrap())
                .collect();
            assert_eq!(names, ["minecraft:saddle", name]);
        }
        assert!(horse_gear(false, 0).is_empty());
        let iron = ItemStack::from_name("minecraft:iron_horse_armor").unwrap();
        assert_eq!(
            horse_gear(false, iron.item)
                .iter()
                .map(|stack| stack.name().unwrap())
                .collect::<Vec<_>>(),
            ["minecraft:iron_horse_armor"]
        );
        assert_eq!(
            horse_gear(true, 0)
                .iter()
                .map(|stack| stack.name().unwrap())
                .collect::<Vec<_>>(),
            ["minecraft:saddle"]
        );
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
