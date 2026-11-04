//! Item type facts from the vanilla item table (stack size, durability), so a plugin sees
//! `ENDER_PEARL` stack to 16 and a sword to 1 instead of a flat 64.

use ferrite_plugin::HostCall;

/// The answer to `ItemType#getMaxStackSize()` or `getMaxDurability()` for the item named by the
/// call's subject; `None` for other calls or an item the table does not know.
pub fn item_type_answer(call: &HostCall) -> Option<String> {
    if call.owner != "ItemType" || !call.args.is_empty() {
        return None;
    }
    let defaults = ferrite_item::item_defaults_by_name(&call.subject)?;
    match call.method.as_str() {
        "getMaxStackSize()" => Some(defaults.max_stack_size.to_string()),
        "getMaxDurability()" => Some(defaults.max_damage.unwrap_or(0).to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(subject: &str, method: &str) -> HostCall {
        HostCall {
            owner: "ItemType".into(),
            subject: subject.into(),
            method: method.into(),
            args: Vec::new(),
        }
    }

    #[test]
    fn stack_sizes_come_from_the_vanilla_table() {
        let size = |key| item_type_answer(&call(key, "getMaxStackSize()"));
        assert_eq!(size("minecraft:stone").as_deref(), Some("64"));
        assert_eq!(size("minecraft:ender_pearl").as_deref(), Some("16"));
        assert_eq!(size("minecraft:diamond_sword").as_deref(), Some("1"));
        assert_eq!(size("minecraft:not_an_item"), None);
    }

    #[test]
    fn durability_is_zero_for_items_that_do_not_wear() {
        let wear = |key| item_type_answer(&call(key, "getMaxDurability()"));
        assert_eq!(wear("minecraft:stone").as_deref(), Some("0"));
        assert_eq!(wear("minecraft:diamond_sword").as_deref(), Some("1561"));
    }

    #[test]
    fn other_owners_and_methods_are_not_item_facts() {
        let mut other = call("minecraft:stone", "getMaxStackSize()");
        other.owner = "Player".into();
        assert_eq!(item_type_answer(&other), None);
        assert_eq!(item_type_answer(&call("minecraft:stone", "getKey()")), None);
    }
}
