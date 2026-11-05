//! Plugin-created inventories: parses what the Java bridge sends for `Player#openInventory` and
//! `Player#closeInventory` into game commands.
//!
//! Wire format of an inventory argument: `title`, `size`, `type`, then `size` item fields, all
//! separated by U+001E; an item is `MATERIAL U+001D amount`, and an empty field is an empty slot.
//! Values come from plugin code, so every field is validated before it reaches the game loop.

use ferrite_plugin::{HostCall, ItemSnapshot, PlayerCommand};
use uuid::Uuid;

const FIELD: char = '\u{1e}';
const PART: char = '\u{1d}';
const MAX_ROWS: usize = 6;

/// The command behind an inventory call, or `None` when it is not one this module handles
/// (a hopper or dispenser inventory, say, which the menu code cannot show yet).
pub fn inventory_command(call: &HostCall) -> Option<PlayerCommand> {
    if call.owner != "Player" {
        return None;
    }
    let player = Uuid::parse_str(&call.subject).ok()?;
    match call.method.as_str() {
        "openInventory(Inventory)" => open(player, call.args.first()?),
        "openWorkbench(Location,boolean)" => station(player, "workbench"),
        "openAnvil(Location,boolean)" => station(player, "anvil"),
        "openEnchanting(Location,boolean)" => station(player, "enchanting"),
        "openGrindstone(Location,boolean)" => station(player, "grindstone"),
        "openStonecutter(Location,boolean)" => station(player, "stonecutter"),
        "openSmithingTable(Location,boolean)" => station(player, "smithing"),
        "openLoom(Location,boolean)" => station(player, "loom"),
        "openBookContent(String)" => {
            let text = call.args.first()?;
            let mut fields = text.split(FIELD);
            let title = fields.next()?;
            let author = fields.next()?;
            let pages: Vec<String> = fields
                .next()?
                .split(ferrite_plugin::HOST_ITEM_SEPARATOR)
                .map(str::to_owned)
                .collect();
            if title.chars().count() > 32
                || author.chars().count() > 32
                || pages.len() > 100
                || pages.iter().any(|p| p.chars().count() > 32_767)
            {
                return None;
            }
            Some(PlayerCommand::OpenBook {
                player,
                title: title.to_owned(),
                author: author.to_owned(),
                pages,
            })
        }
        "startUsingItem(EquipmentSlot)" => Some(PlayerCommand::ItemUse {
            player,
            action: 0,
            hand: match call.args.first()?.as_str() {
                "HAND" => 0,
                "OFF_HAND" => 1,
                _ => return None,
            },
        }),
        "completeUsingActiveItem()" => Some(PlayerCommand::ItemUse {
            player,
            action: 1,
            hand: 0,
        }),
        "clearActiveItem()" => Some(PlayerCommand::ItemUse {
            player,
            action: 2,
            hand: 0,
        }),
        "setItemDamage(int,int)" => Some(PlayerCommand::ItemDamage {
            player,
            slot: call.args.first()?.parse::<u8>().ok().filter(|s| *s <= 40)?,
            damage: call.args.get(1)?.parse::<i32>().ok().filter(|d| *d >= 0)?,
        }),
        "closeInventory()" | "closeInventory(Reason)" => {
            Some(PlayerCommand::CloseInventory { player })
        }
        _ => None,
    }
}

fn station(player: Uuid, station: &str) -> Option<PlayerCommand> {
    Some(PlayerCommand::OpenStation {
        player,
        station: station.to_owned(),
    })
}

fn open(player: Uuid, text: &str) -> Option<PlayerCommand> {
    let mut fields = text.split(FIELD);
    let title = fields.next()?.to_owned();
    let size: usize = fields.next()?.parse().ok()?;
    if fields.next()? != "CHEST" || size == 0 || !size.is_multiple_of(9) || size > MAX_ROWS * 9 {
        return None;
    }
    let items: Vec<Option<ItemSnapshot>> = fields.map(item).collect::<Option<_>>()?;
    if items.len() != size || title.chars().any(char::is_control) {
        return None;
    }
    Some(PlayerCommand::OpenInventory {
        player,
        title,
        rows: (size / 9) as u8,
        items,
    })
}

/// An empty field is an empty slot, and so is air; anything else must be a well-formed stack.
fn item(field: &str) -> Option<Option<ItemSnapshot>> {
    if field.is_empty() {
        return Some(None);
    }
    let (material, amount) = field.split_once(PART)?;
    let amount: u8 = amount.parse().ok()?;
    if matches!(material, "AIR" | "CAVE_AIR" | "VOID_AIR") || amount == 0 {
        return Some(None);
    }
    let valid = !material.is_empty()
        && material
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ':' | '.' | '-'));
    valid.then(|| {
        Some(ItemSnapshot {
            material: material.to_owned(),
            amount: amount.min(99),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYER: &str = "00000000-0000-0000-0000-000000000001";

    fn call(method: &str, arg: &str) -> HostCall {
        HostCall {
            owner: "Player".into(),
            subject: PLAYER.into(),
            method: method.into(),
            args: vec![arg.into()],
        }
    }

    fn inventory(title: &str, size: usize, kind: &str, items: &[&str]) -> String {
        let mut fields = vec![title.to_owned(), size.to_string(), kind.to_owned()];
        fields.extend(items.iter().map(|i| (*i).to_owned()));
        fields.join(&FIELD.to_string())
    }

    #[test]
    fn opens_a_chest_with_its_items() {
        let mut items = vec![""; 9];
        items[0] = "DIAMOND\u{1d}3";
        items[4] = "AIR\u{1d}1";
        let text = inventory("Shop", 9, "CHEST", &items);
        let Some(PlayerCommand::OpenInventory {
            title, rows, items, ..
        }) = inventory_command(&call("openInventory(Inventory)", &text))
        else {
            panic!("expected OpenInventory");
        };
        assert_eq!((title.as_str(), rows), ("Shop", 1));
        assert_eq!(
            items[0],
            Some(ItemSnapshot {
                material: "DIAMOND".into(),
                amount: 3
            })
        );
        assert!(
            items[1..].iter().all(Option::is_none),
            "air is an empty slot"
        );
    }

    #[test]
    fn stations_open_for_the_calling_player() {
        for (method, name) in [
            ("openWorkbench(Location,boolean)", "workbench"),
            ("openAnvil(Location,boolean)", "anvil"),
            ("openEnchanting(Location,boolean)", "enchanting"),
            ("openGrindstone(Location,boolean)", "grindstone"),
            ("openStonecutter(Location,boolean)", "stonecutter"),
            ("openSmithingTable(Location,boolean)", "smithing"),
            ("openLoom(Location,boolean)", "loom"),
        ] {
            let command = inventory_command(&call(method, "null"));
            assert!(
                matches!(&command, Some(PlayerCommand::OpenStation { station, .. }) if station == name),
                "{method}"
            );
        }
        // Cartography tables have no menu yet and stay unimplemented.
        assert_eq!(
            inventory_command(&call("openCartographyTable(Location,boolean)", "null")),
            None
        );
    }

    #[test]
    fn closing_targets_the_calling_player() {
        let command = inventory_command(&call("closeInventory(Reason)", "PLUGIN"));
        assert!(matches!(
            command,
            Some(PlayerCommand::CloseInventory { .. })
        ));
    }

    #[test]
    fn rejects_what_the_menu_cannot_show() {
        let nine = vec![""; 9];
        for text in [
            inventory("T", 9, "HOPPER", &nine),
            inventory("T", 10, "CHEST", &[""; 10]),
            inventory("T", 63, "CHEST", &[""; 63]),
            inventory("T", 9, "CHEST", &nine[..8]),
            inventory("bad\ntitle", 9, "CHEST", &nine),
            inventory(
                "T",
                9,
                "CHEST",
                &["STONE\u{1d}x", "", "", "", "", "", "", "", ""],
            ),
            inventory(
                "T",
                9,
                "CHEST",
                &["ST ONE\u{1d}1", "", "", "", "", "", "", "", ""],
            ),
        ] {
            assert_eq!(
                inventory_command(&call("openInventory(Inventory)", &text)),
                None,
                "{text:?}"
            );
        }
    }

    #[test]
    fn other_players_calls_are_not_inventory_calls() {
        let mut other = call("openInventory(Inventory)", "x");
        other.owner = "Server".into();
        assert_eq!(inventory_command(&other), None);
        other.owner = "Player".into();
        other.subject = "not-a-uuid".into();
        assert_eq!(inventory_command(&other), None);
    }

    #[test]
    fn book_content_becomes_an_open_book_command() {
        let call = |text: &str| HostCall {
            owner: "Player".into(),
            subject: PLAYER.into(),
            method: "openBookContent(String)".into(),
            args: vec![text.into()],
        };
        let text = format!(
            "Title{FIELD}Me{FIELD}one{}two",
            ferrite_plugin::HOST_ITEM_SEPARATOR
        );
        assert_eq!(
            inventory_command(&call(&text)),
            Some(PlayerCommand::OpenBook {
                player: Uuid::parse_str(PLAYER).unwrap(),
                title: "Title".into(),
                author: "Me".into(),
                pages: vec!["one".into(), "two".into()],
            })
        );
        let long = format!("{}{FIELD}Me{FIELD}x", "t".repeat(33));
        assert_eq!(inventory_command(&call(&long)), None);
    }

    #[test]
    fn item_use_calls_become_use_commands() {
        let call = |method: &str, args: &[&str]| HostCall {
            owner: "Player".into(),
            subject: PLAYER.into(),
            method: method.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
        };
        let player = Uuid::parse_str(PLAYER).unwrap();
        assert_eq!(
            inventory_command(&call("startUsingItem(EquipmentSlot)", &["OFF_HAND"])),
            Some(PlayerCommand::ItemUse {
                player,
                action: 0,
                hand: 1
            })
        );
        assert_eq!(
            inventory_command(&call("startUsingItem(EquipmentSlot)", &["HEAD"])),
            None
        );
        assert_eq!(
            inventory_command(&call("clearActiveItem()", &[])),
            Some(PlayerCommand::ItemUse {
                player,
                action: 2,
                hand: 0
            })
        );
    }

    #[test]
    fn item_damage_calls_become_a_slot_command() {
        let call = |slot: &str, damage: &str| HostCall {
            owner: "Player".into(),
            subject: PLAYER.into(),
            method: "setItemDamage(int,int)".into(),
            args: vec![slot.into(), damage.into()],
        };
        assert_eq!(
            inventory_command(&call("3", "40")),
            Some(PlayerCommand::ItemDamage {
                player: Uuid::parse_str(PLAYER).unwrap(),
                slot: 3,
                damage: 40
            })
        );
        assert_eq!(inventory_command(&call("41", "1")), None);
        assert_eq!(inventory_command(&call("3", "-1")), None);
        assert_eq!(inventory_command(&call("x", "1")), None);
    }
}
