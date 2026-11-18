//! Keeping item components when saving stacks to NBT.

use ferrite_item::{Component, ItemStack};
use ferrite_nbt::Compound;

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

#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_item::{Component, component_ids};

    fn marked(name: &str, count: i32, cost: i32) -> ItemStack {
        let mut stack = ItemStack::from_name(name).unwrap().copy_with_count(count);
        stack
            .components
            .insert(component_ids::REPAIR_COST, Component::RepairCost(cost));
        stack
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
