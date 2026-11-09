//! Vanilla loot tables for plugins: `LootTable#populateLoot` and `#fillInventory`.
//!
//! The table data and its evaluation live in `ferrite-loot`; block drops already use the same
//! loader, so a plugin roll and a broken block cannot drift apart. The reply is one
//! `namespaced:item count` line per rolled stack. A missing table replies with [`HOST_NULL`],
//! which the Java side turns into an empty collection instead of an exception.

use ferrite_plugin::{HOST_NULL, HostCall};

/// The reply to `Server#rollLootTable(String,int)`, or `None` for any other call.
///
/// The second argument is the looting level from Bukkit's `LootContext`; negative levels are
/// rejected rather than clamped, because Bukkit treats them as a caller bug.
pub fn answer(call: &HostCall) -> Option<String> {
    if call.owner != "Server" || call.method != "rollLootTable(String,int)" {
        return None;
    }
    let key = call.args.first()?;
    if !super::is_resource_key(key) {
        return None;
    }
    let looting: i32 = call.args.get(1)?.parse().ok().filter(|level| *level >= 0)?;
    let context = ferrite_loot::LootContext {
        looting,
        ..Default::default()
    };
    Some(match crate::game::drops::roll_table(key, &context) {
        Some(stacks) => stacks
            .into_iter()
            .map(|(id, count)| format!("{id} {count}"))
            .collect::<Vec<_>>()
            .join("\n"),
        None => HOST_NULL.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(method: &str, args: &[&str]) -> HostCall {
        HostCall {
            owner: "Server".into(),
            subject: "server".into(),
            method: method.into(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        }
    }

    fn data_available() -> bool {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../vanilla/26.3/generated/data/minecraft/loot_table")
            .is_dir()
    }

    #[test]
    fn a_vanilla_block_table_rolls_its_drops() {
        if !data_available() {
            return;
        }
        assert_eq!(
            answer(&call(
                "rollLootTable(String,int)",
                &["minecraft:blocks/stone", "0"]
            ))
            .as_deref(),
            Some("minecraft:cobblestone 1")
        );
    }

    #[test]
    fn a_table_can_be_named_without_the_namespace() {
        if !data_available() {
            return;
        }
        assert_eq!(
            answer(&call("rollLootTable(String,int)", &["blocks/stone", "0"])).as_deref(),
            Some("minecraft:cobblestone 1")
        );
    }

    #[test]
    fn an_unknown_table_replies_null() {
        if !data_available() {
            return;
        }
        assert_eq!(
            answer(&call(
                "rollLootTable(String,int)",
                &["minecraft:blocks/not_a_table", "0"]
            ))
            .as_deref(),
            Some(HOST_NULL)
        );
    }

    #[test]
    fn malformed_calls_are_not_answered() {
        assert_eq!(
            answer(&call("rollLootTable(String)", &["blocks/stone"])),
            None
        );
        assert_eq!(
            answer(&call("rollLootTable(String,int)", &["blocks/stone", "-1"])),
            None
        );
        assert_eq!(
            answer(&call("rollLootTable(String,int)", &["not a key", "0"])),
            None
        );
    }
}
