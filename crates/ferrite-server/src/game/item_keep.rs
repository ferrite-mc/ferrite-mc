//! Keeping item components where the game only has a material and an amount.
//!
//! A plugin sees a death drop as `ItemSnapshot { material, amount }` and may add, remove or
//! change entries. The stacks the game actually dropped carry components (names, enchantments,
//! shulker box contents, books), so a drop the plugin left alone has to get its own stack back
//! instead of a plain one.

use ferrite_item::{Component, ItemStack};
use ferrite_nbt::Compound;
use ferrite_plugin::ItemSnapshot;

/// The save-format NBT of a stack, never losing more than it has to.
///
/// A component without an NBT form (raw network bytes of a type the server has no typed model
/// of) would make `ItemStack::to_nbt` fail for the whole stack, and callers that skip failed
/// stacks then lose the item. This drops only those components, naming each in a warning, and
/// keeps the item, its count and every other component. `None` only for an empty stack.
pub fn to_nbt_keeping_item(stack: &ItemStack) -> Option<Compound> {
    if stack.is_empty() {
        return None;
    }
    if let Ok(nbt) = stack.to_nbt() {
        return Some(nbt);
    }
    let mut saveable = stack.copy_with_count(stack.count);
    saveable.components = ferrite_item::ComponentPatch::new();
    for (id, component) in stack.components.added() {
        if matches!(component, Component::Raw(_)) {
            tracing::warn!(
                item = stack.name(),
                component = ferrite_item::component_name(id),
                "a component with no save format was not saved with its item"
            );
        } else {
            saveable.components.insert(id, component.clone());
        }
    }
    for id in stack.components.removed() {
        saveable.components.remove(id);
    }
    saveable.to_nbt().ok()
}

/// The stacks to spawn for the drops a plugin returned.
///
/// Each drop takes the first unused original with the same material and amount, so an
/// untouched drop keeps its components. A drop with no match was added or changed by a plugin
/// and is a plain stack of that material (a plugin can only describe material and amount).
/// A drop of an unknown material comes back as `Err` with the material, for the caller to
/// report.
pub fn drop_stacks(
    mut originals: Vec<ItemStack>,
    drops: &[ItemSnapshot],
) -> Vec<Result<ItemStack, String>> {
    let mut stacks = Vec::with_capacity(drops.len());
    for drop in drops {
        let Some(item) = ferrite_item::item_id(&drop.material) else {
            stacks.push(Err(drop.material.clone()));
            continue;
        };
        let amount = i32::from(drop.amount);
        let kept = originals
            .iter()
            .position(|stack| stack.item == item && stack.count == amount)
            .map(|index| originals.remove(index));
        stacks.push(Ok(
            kept.unwrap_or_else(|| ItemStack::with_count(item, amount))
        ));
    }
    stacks
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_item::{Component, component_ids};

    fn snapshot(stack: &ItemStack) -> ItemSnapshot {
        ItemSnapshot {
            material: stack.name().unwrap().to_owned(),
            amount: u8::try_from(stack.count).unwrap(),
        }
    }

    fn marked(name: &str, count: i32, cost: i32) -> ItemStack {
        let mut stack = ItemStack::from_name(name).unwrap().copy_with_count(count);
        stack
            .components
            .insert(component_ids::REPAIR_COST, Component::RepairCost(cost));
        stack
    }

    #[test]
    fn untouched_drops_get_their_own_components_back() {
        let sword = marked("diamond_sword", 1, 3);
        let other = marked("diamond_sword", 1, 9);
        let originals = vec![sword.clone(), other.clone()];
        let drops = [snapshot(&other), snapshot(&sword)];
        let stacks = drop_stacks(originals, &drops);
        // Same material and amount: the first original claims the first drop.
        assert_eq!(stacks[0], Ok(sword));
        assert_eq!(stacks[1], Ok(other));
    }

    #[test]
    fn a_changed_or_added_drop_is_a_plain_stack() {
        let sword = marked("diamond_sword", 1, 3);
        let drops = [
            ItemSnapshot {
                material: "minecraft:diamond_sword".into(),
                amount: 2,
            },
            ItemSnapshot {
                material: "minecraft:dirt".into(),
                amount: 5,
            },
        ];
        let stacks = drop_stacks(vec![sword], &drops);
        assert_eq!(
            stacks[0],
            Ok(ItemStack::from_name("diamond_sword")
                .unwrap()
                .copy_with_count(2))
        );
        assert_eq!(
            stacks[1],
            Ok(ItemStack::from_name("dirt").unwrap().copy_with_count(5))
        );
    }

    #[test]
    fn each_original_is_used_once_and_unknown_materials_are_reported() {
        let sword = marked("diamond_sword", 1, 3);
        let drops = [snapshot(&sword), snapshot(&sword)];
        let stacks = drop_stacks(vec![sword.clone()], &drops);
        assert_eq!(stacks[0], Ok(sword));
        assert_eq!(
            stacks[1],
            Ok(ItemStack::from_name("diamond_sword").unwrap())
        );
        let unknown = [ItemSnapshot {
            material: "minecraft:not_an_item".into(),
            amount: 1,
        }];
        assert_eq!(
            drop_stacks(Vec::new(), &unknown),
            [Err("minecraft:not_an_item".to_owned())]
        );
    }

    #[test]
    fn a_raw_component_costs_only_itself_not_the_item() {
        let mut stack = marked("diamond_sword", 1, 4);
        // A network-only payload for a type with no typed model (bundle contents).
        stack
            .components
            .insert(component_ids::BUNDLE_CONTENTS, Component::Raw(vec![0]));
        assert!(
            stack.to_nbt().is_err(),
            "the premise: this stack cannot be saved whole"
        );
        let saved = to_nbt_keeping_item(&stack).expect("the item is kept");
        let back = ItemStack::from_nbt(&saved).unwrap();
        assert_eq!(back, marked("diamond_sword", 1, 4));
        assert_eq!(to_nbt_keeping_item(&ItemStack::empty()), None);
    }

    #[test]
    fn a_fully_saveable_stack_is_saved_unchanged() {
        let stack = marked("diamond_sword", 1, 4);
        assert_eq!(to_nbt_keeping_item(&stack), stack.to_nbt().ok());
    }
}
