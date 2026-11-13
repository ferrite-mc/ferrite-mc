//! Plugin-created inventories: parses what the Java bridge sends for `Player#openInventory` and
//! `Player#closeInventory` into game commands.
//!
//! Wire format of an inventory argument: `title`, `size`, `type`, then `size` item fields, all
//! separated by U+001E; an item is `MATERIAL U+001D amount`, and an empty field is an empty slot.
//! Values come from plugin code, so every field is validated before it reaches the game loop.

use ferrite_plugin::{HostCall, ItemSnapshot, MapDecoration, PlayerCommand};
use uuid::Uuid;

const FIELD: char = '\u{1e}';
const PART: char = '\u{1d}';
const MAX_ROWS: usize = 6;
/// Maximum UTF-16 units in a caption that still fit a network NBT string (65,535 bytes).
const MAX_MAP_CAPTION_UTF16: usize = 21_844;

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
        "openCartographyTable(Location,boolean)" => station(player, "cartography"),
        "sendMapData(int,int,boolean,byte[],String)" => {
            use base64::Engine;
            let colors = base64::engine::general_purpose::STANDARD
                .decode(call.args.get(3)?)
                .ok()
                .filter(|c| c.len() == 128 * 128)?;
            Some(PlayerCommand::MapData {
                player,
                id: call.args.first()?.parse().ok().filter(|id| *id >= 0)?,
                scale: call.args.get(1)?.parse::<u8>().ok().filter(|s| *s <= 4)?,
                locked: call.args.get(2)?.parse().ok()?,
                colors,
                decorations: map_decorations(call.args.get(4)?)?,
            })
        }
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
        "setItemMeta(int,String)" => {
            let slot = call.args.first()?.parse::<u8>().ok().filter(|s| *s <= 40)?;
            let mut fields = call.args.get(1)?.split(FIELD);
            let name = fields.next().filter(|s| !s.is_empty()).map(str::to_owned);
            let items = |field: Option<&str>| -> Vec<String> {
                field
                    .filter(|s| !s.is_empty())
                    .map(|s| {
                        s.split(ferrite_plugin::HOST_ITEM_SEPARATOR)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default()
            };
            let lore = items(fields.next());
            let mut enchants = Vec::new();
            for entry in items(fields.next()) {
                let (id, level) = entry.split_once('=')?;
                enchants.push((id.to_owned(), level.parse().ok()?));
            }
            let unbreakable = fields.next()? == "1";
            let model = fields
                .next()
                .filter(|s| !s.is_empty())
                .and_then(|s| s.parse().ok());
            let repair = fields.next()?.parse().ok().filter(|r| *r >= 0)?;
            let flags = fields
                .next()
                .filter(|s| !s.is_empty())
                .map(|s| s.split(',').map(str::to_owned).collect::<Vec<_>>())
                .unwrap_or_default();
            if flags.iter().any(|flag| !is_item_flag(flag)) {
                return None;
            }
            let custom_data = match fields.next() {
                Some(pdc) if !pdc.is_empty() => Some(item_pdc(pdc)?),
                _ => None,
            };
            Some(PlayerCommand::ItemMeta {
                player,
                slot,
                name,
                lore,
                enchants,
                unbreakable,
                model,
                repair,
                flags,
                custom_data,
            })
        }
        "setItemMapId(int,int)" => Some(PlayerCommand::ItemMapId {
            player,
            slot: call.args.first()?.parse::<u8>().ok().filter(|s| *s <= 40)?,
            id: call
                .args
                .get(1)?
                .parse::<i32>()
                .ok()
                .filter(|id| *id >= 0)?,
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

fn is_item_flag(flag: &str) -> bool {
    matches!(
        flag,
        "HIDE_ENCHANTS"
            | "HIDE_ATTRIBUTES"
            | "HIDE_UNBREAKABLE"
            | "HIDE_DESTROYS"
            | "HIDE_PLACED_ON"
            | "HIDE_DYE"
            | "HIDE_ARMOR_TRIM"
            | "HIDE_STORED_ENCHANTS"
            | "HIDE_ADDITIONAL_TOOLTIP"
    )
}

/// Decodes the item PDC bridge format: an SNBT compound whose entries are
/// `{"type":"INTEGER", "value":42}` wrappers. The wrapper keeps BOOLEAN distinct from BYTE
/// and rejects unsupported Bukkit PDC types instead of dropping them.
fn item_pdc(snbt: &str) -> Option<ferrite_nbt::Compound> {
    let ferrite_nbt::Tag::Compound(entries) = ferrite_nbt::snbt::parse(snbt).ok()? else {
        return None;
    };
    let mut data = ferrite_nbt::Compound::new();
    for (key, wrapper) in entries {
        let (namespace, path) = key.split_once(':')?;
        if namespace.is_empty() || path.is_empty() || data.contains_key(&key) {
            return None;
        }
        let ferrite_nbt::Tag::Compound(wrapper) = wrapper else {
            return None;
        };
        if wrapper.len() != 2 {
            return None;
        }
        let ferrite_nbt::Tag::String(kind) = wrapper.get("type")? else {
            return None;
        };
        let value = wrapper.get("value")?;
        let matches = match kind.as_str() {
            "BYTE" | "BOOLEAN" => matches!(value, ferrite_nbt::Tag::Byte(_)),
            "SHORT" => matches!(value, ferrite_nbt::Tag::Short(_)),
            "INTEGER" => matches!(value, ferrite_nbt::Tag::Int(_)),
            "LONG" => matches!(value, ferrite_nbt::Tag::Long(_)),
            "FLOAT" => matches!(value, ferrite_nbt::Tag::Float(_)),
            "DOUBLE" => matches!(value, ferrite_nbt::Tag::Double(_)),
            "STRING" => matches!(value, ferrite_nbt::Tag::String(_)),
            "BYTE_ARRAY" => matches!(value, ferrite_nbt::Tag::ByteArray(_)),
            "INTEGER_ARRAY" => matches!(value, ferrite_nbt::Tag::IntArray(_)),
            "LONG_ARRAY" => matches!(value, ferrite_nbt::Tag::LongArray(_)),
            _ => false,
        };
        if !matches {
            return None;
        }
        if *kind == "BOOLEAN" && !matches!(value, ferrite_nbt::Tag::Byte(0 | 1)) {
            return None;
        }
        data.insert(key, value.clone());
    }
    Some(data)
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
    let kind = fields.next()?;
    let shulker = kind == "SHULKER_BOX";
    let allowed = match kind {
        "CHEST" => size != 0 && size.is_multiple_of(9) && size <= MAX_ROWS * 9,
        "SHULKER_BOX" => size == ferrite_inventory::SHULKER_BOX_SLOTS,
        _ => false,
    };
    if !allowed {
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
        shulker,
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

/// Parses the bridge's visible-cursor list. Each cursor has six U+001E-separated fields:
/// decoration key, x, y, direction, has-caption, base64(UTF-8 caption); cursors are separated by
/// U+001D. Invisible cursors never arrive: 26.3 has no wire visibility bit, so omission is the
/// only exact representation and the Java bridge filters them out.
fn map_decorations(encoded: &str) -> Option<Vec<MapDecoration>> {
    if encoded.is_empty() {
        return Some(Vec::new());
    }
    encoded
        .split(PART)
        .map(|cursor| {
            let mut fields = cursor.split(FIELD);
            let kind =
                ferrite_data::registries::id("minecraft:map_decoration_type", fields.next()?)?;
            let x = fields.next()?.parse::<i8>().ok()?;
            let y = fields.next()?.parse::<i8>().ok()?;
            let direction = fields
                .next()?
                .parse::<i8>()
                .ok()
                .filter(|direction| (0..=15).contains(direction))?
                as u8;
            let has_caption = fields.next()?.parse::<bool>().ok()?;
            let encoded_caption = fields.next()?;
            if fields.next().is_some() {
                return None;
            }
            let caption = if has_caption {
                use base64::Engine;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(encoded_caption)
                    .ok()?;
                let text = String::from_utf8(bytes).ok()?;
                if text.encode_utf16().count() > MAX_MAP_CAPTION_UTF16 {
                    return None;
                }
                Some(text)
            } else {
                if !encoded_caption.is_empty() {
                    return None;
                }
                None
            };
            Some(MapDecoration {
                kind,
                x,
                y,
                direction,
                caption,
            })
        })
        .collect()
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
        // A cartography table is a real menu now: its result depends on the saved-map store,
        // which the player's region supplies when the menu opens.
        assert!(matches!(
            inventory_command(&call("openCartographyTable(Location,boolean)", "null")),
            Some(PlayerCommand::OpenStation { station, .. }) if station == "cartography"
        ));
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
    fn opens_a_plugin_shulker_box_inventory_as_a_shulker_menu() {
        let mut items = vec![""; 27];
        items[26] = "EMERALD\u{1d}5";
        let text = inventory("Loot", 27, "SHULKER_BOX", &items);
        let Some(PlayerCommand::OpenInventory {
            title,
            rows,
            shulker,
            items,
            ..
        }) = inventory_command(&call("openInventory(Inventory)", &text))
        else {
            panic!("expected OpenInventory");
        };
        assert_eq!((title.as_str(), rows, shulker), ("Loot", 3, true));
        assert_eq!(items.len(), 27);
        assert_eq!(
            items[26],
            Some(ItemSnapshot {
                material: "EMERALD".into(),
                amount: 5
            })
        );
        // A chest of the same size stays a chest, and a shulker box of another size is refused.
        let chest = inventory("Loot", 27, "CHEST", &[""; 27]);
        assert!(matches!(
            inventory_command(&call("openInventory(Inventory)", &chest)),
            Some(PlayerCommand::OpenInventory { shulker: false, .. })
        ));
        assert_eq!(
            inventory_command(&call(
                "openInventory(Inventory)",
                &inventory("Loot", 9, "SHULKER_BOX", &[""; 9])
            )),
            None
        );
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
    fn map_data_needs_a_full_colour_grid_and_valid_decorations() {
        use base64::Engine;
        let encode = |n: usize| base64::engine::general_purpose::STANDARD.encode(vec![5u8; n]);
        let call = |colors: String, decorations: &str| HostCall {
            owner: "Player".into(),
            subject: PLAYER.into(),
            method: "sendMapData(int,int,boolean,byte[],String)".into(),
            args: vec![
                "7".into(),
                "2".into(),
                "true".into(),
                colors,
                decorations.into(),
            ],
        };
        assert!(matches!(
            inventory_command(&call(encode(16384), "")),
            Some(PlayerCommand::MapData {
                id: 7,
                scale: 2,
                locked: true,
                decorations,
                ..
            }) if decorations.is_empty()
        ));
        assert_eq!(inventory_command(&call(encode(100), "")), None);
        assert_eq!(
            inventory_command(&call(encode(16384), "minecraft:not_a_type")),
            None
        );
    }

    #[test]
    fn map_decorations_keep_type_bounds_and_optional_plain_captions() {
        use base64::Engine;
        let encode_colours =
            |n: usize| base64::engine::general_purpose::STANDARD.encode(vec![5u8; n]);
        let caption = base64::engine::general_purpose::STANDARD.encode("Alex");
        let encoded = [
            format!("minecraft:player{FIELD}-128{FIELD}127{FIELD}3{FIELD}true{FIELD}{caption}"),
            format!("minecraft:red_x{FIELD}127{FIELD}-128{FIELD}15{FIELD}false{FIELD}"),
        ]
        .join(&PART.to_string());
        let call = HostCall {
            owner: "Player".into(),
            subject: PLAYER.into(),
            method: "sendMapData(int,int,boolean,byte[],String)".into(),
            args: vec![
                "7".into(),
                "2".into(),
                "true".into(),
                encode_colours(16384),
                encoded,
            ],
        };
        let Some(PlayerCommand::MapData {
            id,
            scale,
            locked,
            decorations,
            ..
        }) = inventory_command(&call)
        else {
            panic!("valid map data was rejected");
        };
        assert_eq!((id, scale, locked), (7, 2, true));
        assert_eq!(
            decorations,
            vec![
                MapDecoration {
                    kind: 0,
                    x: -128,
                    y: 127,
                    direction: 3,
                    caption: Some("Alex".into()),
                },
                MapDecoration {
                    kind: 26,
                    x: 127,
                    y: -128,
                    direction: 15,
                    caption: None,
                },
            ]
        );

        let valid_colours = encode_colours(16384);
        for bad in [
            "minecraft:player\u{1e}0\u{1e}0\u{1e}16\u{1e}false\u{1e}",
            "minecraft:player\u{1e}0\u{1e}0\u{1e}0\u{1e}true\u{1e}not-base64",
            "minecraft:player\u{1e}0\u{1e}0\u{1e}0\u{1e}false\u{1e}unexpected",
            "minecraft:player\u{1e}0\u{1e}0\u{1e}0\u{1e}false",
        ] {
            let call = HostCall {
                owner: "Player".into(),
                subject: PLAYER.into(),
                method: "sendMapData(int,int,boolean,byte[],String)".into(),
                args: vec![
                    "7".into(),
                    "2".into(),
                    "true".into(),
                    valid_colours.clone(),
                    bad.into(),
                ],
            };
            assert_eq!(inventory_command(&call), None, "{bad:?}");
        }
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
    fn item_meta_calls_become_a_slot_command() {
        let payload = [
            "{\"text\":\"Hi\"}",
            "a\u{1d}b",
            "minecraft:sharpness=3",
            "1",
            "7",
            "2",
            "HIDE_ENCHANTS,HIDE_DYE",
            "{\"ferrite:owner\":{type:\"STRING\",value:\"Steve\"},\"ferrite:level\":{type:\"INTEGER\",value:12}}",
        ]
        .join("\u{1e}");
        let call = HostCall {
            owner: "Player".into(),
            subject: PLAYER.into(),
            method: "setItemMeta(int,String)".into(),
            args: vec!["4".into(), payload],
        };
        assert_eq!(
            inventory_command(&call),
            Some(PlayerCommand::ItemMeta {
                player: Uuid::parse_str(PLAYER).unwrap(),
                slot: 4,
                name: Some("{\"text\":\"Hi\"}".into()),
                lore: vec!["a".into(), "b".into()],
                enchants: vec![("minecraft:sharpness".into(), 3)],
                unbreakable: true,
                model: Some(7),
                repair: 2,
                flags: vec!["HIDE_ENCHANTS".into(), "HIDE_DYE".into()],
                custom_data: Some({
                    let mut data = ferrite_nbt::Compound::new();
                    data.insert(
                        "ferrite:owner".into(),
                        ferrite_nbt::Tag::String("Steve".into()),
                    );
                    data.insert("ferrite:level".into(), ferrite_nbt::Tag::Int(12));
                    data
                }),
            })
        );
    }

    #[test]
    fn item_meta_rejects_unknown_flags_and_unsupported_pdc_values() {
        let payload =
            |flags: &str, pdc: &str| ["", "", "", "0", "", "0", flags, pdc].join("\u{1e}");
        let call = |text: String| HostCall {
            owner: "Player".into(),
            subject: PLAYER.into(),
            method: "setItemMeta(int,String)".into(),
            args: vec!["4".into(), text],
        };
        assert_eq!(
            inventory_command(&call(payload("HIDE_EVERYTHING", "{}"))),
            None
        );
        assert_eq!(
            inventory_command(&call(payload(
                "",
                "{\"ferrite:list\":{type:\"LIST\",value:{}}}"
            ))),
            None
        );
        assert_eq!(
            inventory_command(&call(payload(
                "",
                "{\"ferrite:n\":{type:\"INTEGER\",value:\"wrong\"}}"
            ))),
            None
        );
    }

    #[test]
    fn item_pdc_keeps_every_typed_value_and_escaped_strings() {
        let data = item_pdc(concat!(
            r#"{"ferrite:byte":{type:"BYTE",value:4b},"#,
            r#""ferrite:short":{type:"SHORT",value:5s},"#,
            r#""ferrite:int":{type:"INTEGER",value:6},"#,
            r#""ferrite:long":{type:"LONG",value:7l},"#,
            r#""ferrite:float":{type:"FLOAT",value:1.5f},"#,
            r#""ferrite:double":{type:"DOUBLE",value:2.5d},"#,
            r#""ferrite:bool":{type:"BOOLEAN",value:1b},"#,
            r#""ferrite:text":{type:"STRING",value:"line\nquote\"slash\\tab\t"},"#,
            r#""ferrite:nan":{type:"FLOAT",value:NaNf},"#,
            r#""ferrite:inf":{type:"DOUBLE",value:-Infinityd},"#,
            r#""ferrite:bytes":{type:"BYTE_ARRAY",value:[B;1b,-2b]},"#,
            r#""ferrite:ints":{type:"INTEGER_ARRAY",value:[I;3,-4]},"#,
            r#""ferrite:longs":{type:"LONG_ARRAY",value:[L;5l,-6l]}"#,
            "}"
        ))
        .expect("valid PDC SNBT");
        assert_eq!(data.get("ferrite:byte"), Some(&ferrite_nbt::Tag::Byte(4)));
        assert_eq!(data.get("ferrite:short"), Some(&ferrite_nbt::Tag::Short(5)));
        assert_eq!(data.get("ferrite:int"), Some(&ferrite_nbt::Tag::Int(6)));
        assert_eq!(data.get("ferrite:long"), Some(&ferrite_nbt::Tag::Long(7)));
        assert_eq!(
            data.get("ferrite:float"),
            Some(&ferrite_nbt::Tag::Float(1.5))
        );
        assert_eq!(
            data.get("ferrite:double"),
            Some(&ferrite_nbt::Tag::Double(2.5))
        );
        assert_eq!(data.get("ferrite:bool"), Some(&ferrite_nbt::Tag::Byte(1)));
        assert_eq!(
            data.get("ferrite:text"),
            Some(&ferrite_nbt::Tag::String(
                "line\nquote\"slash\\tab\t".into()
            ))
        );
        let Some(ferrite_nbt::Tag::Float(nan)) = data.get("ferrite:nan") else {
            panic!("NaN float was not preserved");
        };
        assert!(nan.is_nan(), "NaN float was not preserved");
        assert_eq!(
            data.get("ferrite:inf"),
            Some(&ferrite_nbt::Tag::Double(f64::NEG_INFINITY))
        );
        assert_eq!(
            data.get("ferrite:bytes"),
            Some(&ferrite_nbt::Tag::ByteArray(vec![1, -2]))
        );
        assert_eq!(
            data.get("ferrite:ints"),
            Some(&ferrite_nbt::Tag::IntArray(vec![3, -4]))
        );
        assert_eq!(
            data.get("ferrite:longs"),
            Some(&ferrite_nbt::Tag::LongArray(vec![5, -6]))
        );

        for invalid in [
            r#"{"ferrite":{type:"STRING",value:"x"}}"#,
            r#"{":key":{type:"STRING",value:"x"}}"#,
            r#"{"ferrite:key":{type:"STRING",value:"x",extra:1}}"#,
            r#"{"ferrite:key":{type:"BOOLEAN",value:2b}}"#,
            r#"{"ferrite:key":{type:"BYTE_ARRAY",value:[I;1]}}"#,
        ] {
            assert_eq!(item_pdc(invalid), None, "{invalid}");
        }
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
    fn map_id_calls_become_a_slot_command() {
        let call = |slot: &str, id: &str| HostCall {
            owner: "Player".into(),
            subject: PLAYER.into(),
            method: "setItemMapId(int,int)".into(),
            args: vec![slot.into(), id.into()],
        };
        assert_eq!(
            inventory_command(&call("2", "17")),
            Some(PlayerCommand::ItemMapId {
                player: Uuid::parse_str(PLAYER).unwrap(),
                slot: 2,
                id: 17
            })
        );
        assert_eq!(inventory_command(&call("41", "1")), None);
        assert_eq!(inventory_command(&call("2", "-1")), None);
        assert_eq!(inventory_command(&call("2", "x")), None);
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
