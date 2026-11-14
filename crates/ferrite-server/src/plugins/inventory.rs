//! Plugin-created inventories: parses what the Java bridge sends for `Player#openInventory` and
//! `Player#closeInventory` into game commands.
//!
//! Wire format of an inventory argument: `title`, `size`, `type`, then `size` item fields, all
//! separated by U+001E; an item is `MATERIAL U+001D amount`, and an empty field is an empty slot.
//! Values come from plugin code, so every field is validated before it reaches the game loop.

use ferrite_plugin::{HostCall, ItemSnapshot, MapCaption, MapDecoration, PlayerCommand};
use uuid::Uuid;

const FIELD: char = '\u{1e}';
const PART: char = '\u{1d}';
const MAX_ROWS: usize = 6;
/// Maximum UTF-16 units in a caption that still fit a network NBT string (65,535 bytes).
const MAX_MAP_CAPTION_UTF16: usize = 21_844;
/// Upper bound for a styled caption's network NBT.
const MAX_RICH_CAPTION_BYTES: usize = 65_535;

/// Decodes the payload of `setItemMeta(int,String)`: the metadata fields joined by [`FIELD`].
/// Anything unreadable refuses the whole payload, so half a stack of metadata is never applied.
pub fn parse_item_meta(payload: &str) -> Option<ferrite_plugin::ItemMetaSpec> {
    let mut fields = payload.split(FIELD);
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
    // The book field is optional; one that is present but unreadable refuses the whole
    // call, so a half-written book is never applied.
    let book = match fields.next() {
        Some(book) if !book.is_empty() => Some(item_book(book)?),
        _ => None,
    };
    Some(ferrite_plugin::ItemMetaSpec {
        name,
        lore,
        enchants,
        unbreakable,
        model,
        repair,
        flags,
        custom_data,
        book,
    })
}

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
            let meta = parse_item_meta(call.args.get(1)?)?;
            Some(PlayerCommand::ItemMeta {
                player,
                slot,
                name: meta.name,
                lore: meta.lore,
                enchants: meta.enchants,
                unbreakable: meta.unbreakable,
                model: meta.model,
                repair: meta.repair,
                flags: meta.flags,
                custom_data: meta.custom_data,
                book: meta.book,
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
        // Vanilla keeps the spin in a byte-sized counter; a non-finite or negative damage is refused.
        "startRiptideAttack(int,float)" => Some(PlayerCommand::RiptideAttack {
            player,
            ticks: call.args.first()?.parse::<u8>().ok().filter(|t| *t > 0)?,
            damage: call
                .args
                .get(1)?
                .parse::<f32>()
                .ok()
                .filter(|d| d.is_finite() && *d >= 0.0)?,
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

/// The most pages a book holds, and the most characters a page of a book and quill holds
/// (vanilla `WritableBookContent` / `WrittenBookContent` codecs).
const BOOK_MAX_PAGES: usize = 100;
const WRITABLE_PAGE_CHARS: usize = 1024;
/// The longest title of a signed book, in UTF-16 units.
const BOOK_TITLE_UNITS: usize = 32;

/// Decodes the book field of `setItemMeta`: a JSON object
/// `{"k":"B"|"W","t":title,"a":author,"g":generation,"p":[pages]}`. JSON keeps any text a page
/// can hold apart from the field separators. Over-long books, unknown kinds and pages that are
/// not text components are refused, never trimmed.
fn item_book(json: &str) -> Option<ferrite_plugin::BookFields> {
    use serde_json::Value;
    let Value::Object(object) = serde_json::from_str(json).ok()? else {
        return None;
    };
    let written = match object.get("k")?.as_str()? {
        "B" => true,
        "W" => false,
        _ => return None,
    };
    let text = |key: &str| match object.get(key) {
        None | Some(Value::Null) => Some(None),
        Some(Value::String(text)) => Some(Some(text.clone())),
        Some(_) => None,
    };
    let title = text("t")?;
    let author = text("a")?;
    let generation = i32::try_from(object.get("g")?.as_i64()?)
        .ok()
        .filter(|generation| (0..=3).contains(generation))?;
    let pages: Vec<String> = object
        .get("p")?
        .as_array()?
        .iter()
        .map(|page| page.as_str().map(str::to_owned))
        .collect::<Option<_>>()?;
    if pages.len() > BOOK_MAX_PAGES {
        return None;
    }
    if written {
        if title
            .as_ref()
            .is_some_and(|title| title.encode_utf16().count() > BOOK_TITLE_UNITS)
        {
            return None;
        }
        if pages
            .iter()
            .any(|page| ferrite_text::Component::from_json(page).is_err())
        {
            return None;
        }
    } else if title.is_some()
        || author.is_some()
        || generation != 0
        || pages
            .iter()
            .any(|page| page.chars().count() > WRITABLE_PAGE_CHARS)
    {
        return None;
    }
    Some(ferrite_plugin::BookFields {
        written,
        title,
        author,
        generation,
        pages,
    })
}

/// The deepest nesting of containers and lists the bridge accepts (the Java side refuses the
/// same depth, so a longer chain can only come from a corrupt or hostile payload).
const PDC_DEPTH: usize = 16;

/// Decodes the item PDC bridge format: an SNBT compound whose entries are
/// `{type:"INTEGER", value:42}` wrappers. The wrapper keeps BOOLEAN distinct from BYTE and
/// rejects unsupported Bukkit PDC types instead of dropping them.
///
/// A nested container is a compound of wrappers (`TAG_CONTAINER`), an array of them is a list of
/// such compounds (`TAG_CONTAINER_ARRAY`), and a list names its element type
/// (`{type:"LIST", element:"STRING", value:[...]}`; the element of a list of lists reads
/// `LIST:STRING`). The stored form drops the wrappers: bare values, nested compounds and typed
/// lists, which is what Paper writes to `minecraft:custom_data`.
fn item_pdc(snbt: &str) -> Option<ferrite_nbt::Compound> {
    let ferrite_nbt::Tag::Compound(entries) = ferrite_nbt::snbt::parse(snbt).ok()? else {
        return None;
    };
    pdc_compound(&entries, 0)
}

fn pdc_compound(entries: &ferrite_nbt::Compound, depth: usize) -> Option<ferrite_nbt::Compound> {
    use ferrite_nbt::Tag;
    if depth > PDC_DEPTH {
        return None;
    }
    let mut data = ferrite_nbt::Compound::new();
    for (key, wrapper) in entries {
        let (namespace, path) = key.split_once(':')?;
        if namespace.is_empty() || path.is_empty() {
            return None;
        }
        let Tag::Compound(wrapper) = wrapper else {
            return None;
        };
        let Tag::String(kind) = wrapper.get("type")? else {
            return None;
        };
        let value = wrapper.get("value")?;
        let shape = if kind == "LIST" {
            // `type`, `element` and `value`, nothing else.
            let Tag::String(element) = wrapper.get("element")? else {
                return None;
            };
            if wrapper.len() != 3 {
                return None;
            }
            format!("LIST:{element}")
        } else {
            if wrapper.len() != 2 {
                return None;
            }
            kind.clone()
        };
        data.insert(key.clone(), pdc_value(&shape, value, depth)?);
    }
    Some(data)
}

/// The stored form of `value` when it is declared as `shape`; `None` when it is not one.
fn pdc_value(shape: &str, value: &ferrite_nbt::Tag, depth: usize) -> Option<ferrite_nbt::Tag> {
    use ferrite_nbt::{List, Tag};
    if depth > PDC_DEPTH {
        return None;
    }
    let ok = match shape {
        "BYTE" => matches!(value, Tag::Byte(_)),
        "BOOLEAN" => matches!(value, Tag::Byte(0 | 1)),
        "SHORT" => matches!(value, Tag::Short(_)),
        "INTEGER" => matches!(value, Tag::Int(_)),
        "LONG" => matches!(value, Tag::Long(_)),
        "FLOAT" => matches!(value, Tag::Float(_)),
        "DOUBLE" => matches!(value, Tag::Double(_)),
        "STRING" => matches!(value, Tag::String(_)),
        "BYTE_ARRAY" => matches!(value, Tag::ByteArray(_)),
        "INTEGER_ARRAY" => matches!(value, Tag::IntArray(_)),
        "LONG_ARRAY" => matches!(value, Tag::LongArray(_)),
        _ => false,
    };
    if ok {
        return Some(value.clone());
    }
    match (shape, value) {
        ("TAG_CONTAINER", Tag::Compound(inner)) => {
            pdc_compound(inner, depth + 1).map(Tag::Compound)
        }
        ("TAG_CONTAINER_ARRAY", Tag::List(List::End)) => Some(Tag::List(List::End)),
        ("TAG_CONTAINER_ARRAY", Tag::List(List::Compound(items))) => items
            .iter()
            .map(|item| pdc_compound(item, depth + 1))
            .collect::<Option<Vec<_>>>()
            .map(|items| Tag::List(List::Compound(items))),
        (list, Tag::List(items)) if list.starts_with("LIST:") => {
            let element = &list["LIST:".len()..];
            pdc_list(element, items, depth + 1).map(Tag::List)
        }
        _ => None,
    }
}

/// A list whose elements are all `element`, converted to their stored form. The list keeps its
/// element type even when empty.
fn pdc_list(element: &str, items: &ferrite_nbt::List, depth: usize) -> Option<ferrite_nbt::List> {
    use ferrite_nbt::{List, Tag};
    if depth > PDC_DEPTH {
        return None;
    }
    let tags: Vec<Tag> = match items {
        List::End => Vec::new(),
        List::Byte(v) => v.iter().map(|x| Tag::Byte(*x)).collect(),
        List::Short(v) => v.iter().map(|x| Tag::Short(*x)).collect(),
        List::Int(v) => v.iter().map(|x| Tag::Int(*x)).collect(),
        List::Long(v) => v.iter().map(|x| Tag::Long(*x)).collect(),
        List::Float(v) => v.iter().map(|x| Tag::Float(*x)).collect(),
        List::Double(v) => v.iter().map(|x| Tag::Double(*x)).collect(),
        List::ByteArray(v) => v.iter().map(|x| Tag::ByteArray(x.clone())).collect(),
        List::String(v) => v.iter().map(|x| Tag::String(x.clone())).collect(),
        List::List(v) => v.iter().map(|x| Tag::List(x.clone())).collect(),
        List::Compound(v) => v.iter().map(|x| Tag::Compound(x.clone())).collect(),
        List::IntArray(v) => v.iter().map(|x| Tag::IntArray(x.clone())).collect(),
        List::LongArray(v) => v.iter().map(|x| Tag::LongArray(x.clone())).collect(),
    };
    let converted: Vec<Tag> = tags
        .iter()
        .map(|tag| pdc_value(element, tag, depth))
        .collect::<Option<_>>()?;
    macro_rules! collect {
        ($variant:ident) => {
            converted
                .into_iter()
                .map(|tag| match tag {
                    Tag::$variant(x) => Some(x),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()
                .map(List::$variant)
        };
    }
    match element {
        "BYTE" | "BOOLEAN" => collect!(Byte),
        "SHORT" => collect!(Short),
        "INTEGER" => collect!(Int),
        "LONG" => collect!(Long),
        "FLOAT" => collect!(Float),
        "DOUBLE" => collect!(Double),
        "STRING" => collect!(String),
        "BYTE_ARRAY" => collect!(ByteArray),
        "INTEGER_ARRAY" => collect!(IntArray),
        "LONG_ARRAY" => collect!(LongArray),
        "TAG_CONTAINER" => collect!(Compound),
        _ if element.starts_with("LIST:") || element == "TAG_CONTAINER_ARRAY" => collect!(List),
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
            // `0`/`false`: no caption, `1`/`true`: plain text, `2`: base64 of a network NBT component.
            let mode = match fields.next()? {
                "0" | "false" => 0,
                "1" | "true" => 1,
                "2" => 2,
                _ => return None,
            };
            let encoded_caption = fields.next()?;
            if fields.next().is_some() {
                return None;
            }
            let caption = if mode == 0 {
                if !encoded_caption.is_empty() {
                    return None;
                }
                None
            } else {
                use base64::Engine;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(encoded_caption)
                    .ok()?;
                if mode == 1 {
                    let text = String::from_utf8(bytes).ok()?;
                    if text.encode_utf16().count() > MAX_MAP_CAPTION_UTF16 {
                        return None;
                    }
                    Some(MapCaption::Plain(text))
                } else {
                    if bytes.len() > MAX_RICH_CAPTION_BYTES {
                        return None;
                    }
                    let (tag, used) = ferrite_nbt::binary::read_network_prefix(&bytes).ok()?;
                    if used != bytes.len()
                        || !matches!(
                            tag,
                            ferrite_nbt::Tag::Compound(_) | ferrite_nbt::Tag::String(_)
                        )
                    {
                        return None;
                    }
                    Some(MapCaption::Rich(tag))
                }
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
                    caption: Some(MapCaption::Plain("Alex".into())),
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
    fn styled_and_numeric_caption_modes_are_parsed() {
        use base64::Engine;
        let mut styled = ferrite_nbt::Compound::new();
        styled.insert("text".into(), ferrite_nbt::Tag::String("Ranger".into()));
        styled.insert("bold".into(), ferrite_nbt::Tag::Byte(1));
        let tag = ferrite_nbt::Tag::Compound(styled);
        let bytes = ferrite_nbt::binary::write_network(&tag).unwrap();
        let rich = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let plain = base64::engine::general_purpose::STANDARD.encode("Hi");
        let parsed = map_decorations(&format!(
            "minecraft:player{FIELD}1{FIELD}2{FIELD}3{FIELD}2{FIELD}{rich}{PART}minecraft:red_x{FIELD}0{FIELD}0{FIELD}0{FIELD}1{FIELD}{plain}{PART}minecraft:red_x{FIELD}0{FIELD}0{FIELD}0{FIELD}0{FIELD}"
        ))
        .expect("numeric modes parse");
        assert_eq!(parsed[0].caption, Some(MapCaption::Rich(tag)));
        assert_eq!(parsed[1].caption, Some(MapCaption::Plain("Hi".into())));
        assert_eq!(parsed[2].caption, None);
        // Trailing bytes, a non-component tag and garbage are refused.
        let mut trailing = bytes.clone();
        trailing.push(0);
        let int = ferrite_nbt::binary::write_network(&ferrite_nbt::Tag::Int(1)).unwrap();
        for bad in [trailing, int, vec![10, 1, 2]] {
            let encoded = base64::engine::general_purpose::STANDARD.encode(bad);
            assert_eq!(
                map_decorations(&format!(
                    "minecraft:player{FIELD}0{FIELD}0{FIELD}0{FIELD}2{FIELD}{encoded}"
                )),
                None
            );
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
                book: None,
            })
        );
    }

    fn meta_call(book: &str) -> HostCall {
        HostCall {
            owner: "Player".into(),
            subject: PLAYER.into(),
            method: "setItemMeta(int,String)".into(),
            args: vec![
                "2".into(),
                ["", "", "", "0", "", "0", "", "", book].join("\u{1e}"),
            ],
        }
    }

    fn book_of(call: &HostCall) -> Option<Option<ferrite_plugin::BookFields>> {
        match inventory_command(call)? {
            PlayerCommand::ItemMeta { book, .. } => Some(book),
            other => panic!("expected an item meta command, got {other:?}"),
        }
    }

    #[test]
    fn a_written_book_arrives_with_unicode_pages_and_its_generation() {
        let json = serde_json::json!({
            "k": "B",
            "t": "Das Buch ✨",
            "a": "Álvaro",
            "g": 2,
            "p": [
                r#"{"text":"Seite 1: Grüße 日本語 😀"}"#,
                "{\"text\":\"a\\u001eb\\u001dc\"}",
            ],
        })
        .to_string();
        assert!(
            !json.contains('\u{1e}') && !json.contains('\u{1d}'),
            "JSON escapes the control characters that separate the fields"
        );
        let book = book_of(&meta_call(&json)).unwrap().expect("a book");
        assert!(book.written);
        assert_eq!(book.title.as_deref(), Some("Das Buch ✨"));
        assert_eq!(book.author.as_deref(), Some("Álvaro"));
        assert_eq!(book.generation, 2);
        assert_eq!(book.pages.len(), 2);
        assert!(book.pages[0].contains("日本語 😀"));
        assert_eq!(
            book.pages[1], r#"{"text":"a\u001eb\u001dc"}"#,
            "the page arrives exactly as sent"
        );
        assert!(ferrite_text::Component::from_json(&book.pages[1]).is_ok());
        // A call with no book field has no book.
        assert_eq!(book_of(&meta_call("")).unwrap(), None);
    }

    #[test]
    fn a_book_and_quill_carries_plain_pages_only() {
        let json = r#"{"k":"W","t":null,"a":null,"g":0,"p":["§cred page","second"]}"#;
        let book = book_of(&meta_call(json)).unwrap().expect("a book");
        assert!(!book.written);
        assert_eq!(book.pages, ["§cred page", "second"]);
    }

    #[test]
    fn malformed_or_over_long_books_refuse_the_whole_call() {
        let pages = |n: usize| vec!["{\"text\":\"p\"}"; n];
        let over = |len: usize| "x".repeat(len);
        for bad in [
            "not json".to_owned(),
            "[]".to_owned(),
            r#"{"k":"X","g":0,"p":[]}"#.to_owned(),
            r#"{"k":"B","g":9,"p":[]}"#.to_owned(),
            r#"{"k":"B","g":-1,"p":[]}"#.to_owned(),
            r#"{"k":"B","g":0}"#.to_owned(),
            r#"{"k":"B","g":0,"p":[1]}"#.to_owned(),
            r#"{"k":"B","g":0,"p":["not a component {"]}"#.to_owned(),
            serde_json::json!({"k":"B","g":0,"p":pages(101)}).to_string(),
            serde_json::json!({"k":"B","t":over(33),"g":0,"p":[]}).to_string(),
            serde_json::json!({"k":"W","g":0,"p":[over(1025)]}).to_string(),
            // A book and quill has no title, author or generation.
            r#"{"k":"W","t":"T","g":0,"p":[]}"#.to_owned(),
            r#"{"k":"W","g":1,"p":[]}"#.to_owned(),
        ] {
            assert!(book_of(&meta_call(&bad)).is_none(), "{bad} must be refused");
        }
        // The limits themselves are allowed.
        for good in [
            serde_json::json!({"k":"B","t":over(32),"g":3,"p":pages(100)}).to_string(),
            serde_json::json!({"k":"W","g":0,"p":vec![over(1024); 100]}).to_string(),
            r#"{"k":"B","g":0,"p":[]}"#.to_owned(),
        ] {
            assert!(book_of(&meta_call(&good)).is_some(), "limit case refused");
        }
        // 32 UTF-16 units, not 32 chars: sixteen emoji are 32 units, seventeen are 34.
        let emoji = |n: usize| "😀".repeat(n);
        assert!(
            book_of(&meta_call(
                &serde_json::json!({"k":"B","t":emoji(16),"g":0,"p":[]}).to_string()
            ))
            .is_some()
        );
        assert!(
            book_of(&meta_call(
                &serde_json::json!({"k":"B","t":emoji(17),"g":0,"p":[]}).to_string()
            ))
            .is_none()
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
    fn item_pdc_keeps_nested_containers_arrays_and_typed_lists() {
        use ferrite_nbt::{List, Tag};
        let data = item_pdc(concat!(
            r#"{"ferrite:inner":{type:"TAG_CONTAINER",value:{"#,
            r#""ferrite:name":{type:"STRING",value:"Grüße 日本語 😀"},"#,
            r#""ferrite:deeper":{type:"TAG_CONTAINER",value:{"ferrite:n":{type:"INTEGER",value:1}}}}},"#,
            r#""ferrite:array":{type:"TAG_CONTAINER_ARRAY",value:[{"ferrite:a":{type:"BYTE",value:1b}},{}]},"#,
            r#""ferrite:ids":{type:"LIST",element:"INTEGER",value:[1,2,3]},"#,
            r#""ferrite:flags":{type:"LIST",element:"BOOLEAN",value:[1b,0b]},"#,
            r#""ferrite:none":{type:"LIST",element:"STRING",value:[]},"#,
            r#""ferrite:boxes":{type:"LIST",element:"TAG_CONTAINER",value:[{"ferrite:s":{type:"SHORT",value:2s}}]},"#,
            r#""ferrite:matrix":{type:"LIST",element:"LIST:STRING",value:[["p","q"],[]]},"#,
            r#""ferrite:blobs":{type:"LIST",element:"BYTE_ARRAY",value:[[B;1b],[B;]]}}"#,
        ))
        .expect("nested PDC");
        let Some(Tag::Compound(inner)) = data.get("ferrite:inner") else {
            panic!("a nested container is a compound of bare values");
        };
        assert_eq!(
            inner.get("ferrite:name"),
            Some(&Tag::String("Grüße 日本語 😀".into()))
        );
        let Some(Tag::Compound(deeper)) = inner.get("ferrite:deeper") else {
            panic!("two levels deep");
        };
        assert_eq!(deeper.get("ferrite:n"), Some(&Tag::Int(1)));
        let Some(Tag::List(List::Compound(array))) = data.get("ferrite:array") else {
            panic!("an array of containers is a list of compounds");
        };
        assert_eq!(array.len(), 2);
        assert_eq!(array[0].get("ferrite:a"), Some(&Tag::Byte(1)));
        assert!(array[1].is_empty());
        assert_eq!(
            data.get("ferrite:ids"),
            Some(&Tag::List(List::Int(vec![1, 2, 3])))
        );
        assert_eq!(
            data.get("ferrite:flags"),
            Some(&Tag::List(List::Byte(vec![1, 0])))
        );
        assert_eq!(
            data.get("ferrite:none"),
            Some(&Tag::List(List::String(Vec::new()))),
            "an empty list keeps its element type"
        );
        assert!(matches!(
            data.get("ferrite:boxes"),
            Some(Tag::List(List::Compound(boxes))) if boxes[0].get("ferrite:s") == Some(&Tag::Short(2))
        ));
        assert_eq!(
            data.get("ferrite:matrix"),
            Some(&Tag::List(List::List(vec![
                List::String(vec!["p".into(), "q".into()]),
                List::String(Vec::new())
            ])))
        );
        assert_eq!(
            data.get("ferrite:blobs"),
            Some(&Tag::List(List::ByteArray(vec![vec![1], vec![]])))
        );
    }

    #[test]
    fn item_pdc_refuses_malformed_or_too_deep_nesting() {
        for invalid in [
            // A list without its element type, or with a wrong one.
            r#"{"ferrite:l":{type:"LIST",value:[1]}}"#,
            r#"{"ferrite:l":{type:"LIST",element:"STRING",value:[1]}}"#,
            r#"{"ferrite:l":{type:"LIST",element:"NOPE",value:[1]}}"#,
            r#"{"ferrite:l":{type:"LIST",element:"INTEGER",value:"x"}}"#,
            // A container whose entries are not wrappers, or a wrong value kind.
            r#"{"ferrite:c":{type:"TAG_CONTAINER",value:{"ferrite:x":5}}}"#,
            r#"{"ferrite:c":{type:"TAG_CONTAINER",value:[1]}}"#,
            r#"{"ferrite:c":{type:"TAG_CONTAINER",value:{"nonamespace":{type:"BYTE",value:1b}}}}"#,
            r#"{"ferrite:c":{type:"TAG_CONTAINER_ARRAY",value:[1,2]}}"#,
            // An extra key on a wrapper.
            r#"{"ferrite:c":{type:"TAG_CONTAINER",value:{},extra:1}}"#,
            // A boolean list holding something that is not 0 or 1.
            r#"{"ferrite:l":{type:"LIST",element:"BOOLEAN",value:[2b]}}"#,
        ] {
            assert!(item_pdc(invalid).is_none(), "{invalid} must be refused");
        }
        let mut deep = String::from(r#"{"ferrite:n":{type:"BYTE",value:1b}}"#);
        for level in 0..20 {
            deep = format!(r#"{{"ferrite:l{level}":{{type:"TAG_CONTAINER",value:{deep}}}}}"#);
        }
        assert!(item_pdc(&deep).is_none(), "20 levels is past the limit");
        let mut fine = String::from(r#"{"ferrite:n":{type:"BYTE",value:1b}}"#);
        for level in 0..10 {
            fine = format!(r#"{{"ferrite:l{level}":{{type:"TAG_CONTAINER",value:{fine}}}}}"#);
        }
        assert!(item_pdc(&fine).is_some(), "10 levels is fine");
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
    fn riptide_attack_calls_become_a_spin_command() {
        let call = |ticks: &str, damage: &str| HostCall {
            owner: "Player".into(),
            subject: PLAYER.into(),
            method: "startRiptideAttack(int,float)".into(),
            args: vec![ticks.into(), damage.into()],
        };
        assert_eq!(
            inventory_command(&call("20", "8.5")),
            Some(PlayerCommand::RiptideAttack {
                player: Uuid::parse_str(PLAYER).unwrap(),
                ticks: 20,
                damage: 8.5
            })
        );
        assert_eq!(inventory_command(&call("0", "1")), None);
        assert_eq!(inventory_command(&call("256", "1")), None);
        assert_eq!(inventory_command(&call("20", "-1")), None);
        assert_eq!(inventory_command(&call("20", "NaN")), None);
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
