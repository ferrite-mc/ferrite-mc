//! `Player#sendBlockUpdate(Location, TileState)`: a tile snapshot shown to one player.
//!
//! The Java side turns a `TileState` into entries (`item,<slot>,<material>,<count>` and
//! `furnace,<burn>,<burn total>,<cook>,<cook total>`); this module validates them against the block
//! entity families Ferrite models and builds the `ClientboundBlockEntityDataPacket` tag: the `Items`
//! containers of `block_container`/`furnace`, banner `patterns` (`BannerBlockEntity.saveAdditional`:
//! list of `{pattern, color}`), skull `profile` (`SkullBlockEntity`: `ResolvableProfile` with the
//! `name`/`id` keys of `ResolvableProfile$Partial` and its `properties`) and skull
//! `note_block_sound` (a resource location). Families Ferrite has no model for never get here:
//! the Java side refuses them, and an unknown type name is rejected again below.

use ferrite_nbt::{Compound, List, Tag};
use ferrite_plugin::PluginPacket;

/// `(block entity type, slot count, has furnace timers)`.
const FAMILIES: [(&str, usize, bool); 17] = [
    ("minecraft:chest", 27, false),
    ("minecraft:trapped_chest", 27, false),
    ("minecraft:barrel", 27, false),
    ("minecraft:shulker_box", 27, false),
    ("minecraft:furnace", 3, true),
    ("minecraft:blast_furnace", 3, true),
    ("minecraft:smoker", 3, true),
    ("minecraft:dispenser", 9, false),
    ("minecraft:dropper", 9, false),
    ("minecraft:hopper", 5, false),
    ("minecraft:brewing_stand", 5, false),
    ("minecraft:crafter", 9, false),
    ("minecraft:campfire", 4, false),
    ("minecraft:chiseled_bookshelf", 6, false),
    ("minecraft:decorated_pot", 1, false),
    ("minecraft:brushable_block", 1, false),
    ("minecraft:shelf", 3, false),
];

/// Block entities whose single stack is saved under its own key instead of `Items`
/// (`JukeboxBlockEntity` `RecordItem`, `LecternBlockEntity` `Book`, `DecoratedPotBlockEntity`
/// and `BrushableBlockEntity` both `item`).
const KEYED_STACK: [(&str, &str); 4] = [
    ("minecraft:jukebox", "RecordItem"),
    ("minecraft:lectern", "Book"),
    ("minecraft:decorated_pot", "item"),
    ("minecraft:brushable_block", "item"),
];

/// How a `put` value is typed in the block entity's saved form.
#[derive(Clone, Copy)]
enum Kind {
    Short,
    Int,
    Long,
    Float,
    Bool,
    Text,
    Choice(&'static [&'static str]),
}

const ROTATIONS: &[&str] = &["none", "clockwise_90", "180", "counterclockwise_90"];
const MIRRORS: &[&str] = &["none", "left_right", "front_back"];
const STRUCTURE_MODES: &[&str] = &["save", "load", "corner", "data"];

/// The keys a snapshot may set on a block entity type, with the type each is saved as. The names
/// and types come from the 26.3 server's `saveAdditional` of each block entity.
const SETTINGS: [(&str, &[(&str, Kind)]); 13] = [
    (
        "minecraft:jukebox",
        &[("ticks_since_song_started", Kind::Long)],
    ),
    ("minecraft:lectern", &[("Page", Kind::Int)]),
    // `BeaconBlockEntity.getUpdateTag` is `saveCustomOnly`: the effects are server-side data and
    // are not part of the update tag the client is sent.
    ("minecraft:beacon", &[("CustomName", Kind::Text)]),
    ("minecraft:enchanting_table", &[("CustomName", Kind::Text)]),
    ("minecraft:sculk_shrieker", &[("warning_level", Kind::Int)]),
    (
        "minecraft:end_gateway",
        &[("Age", Kind::Long), ("ExactTeleport", Kind::Bool)],
    ),
    (
        "minecraft:crafter",
        &[
            ("crafting_ticks_remaining", Kind::Int),
            ("triggered", Kind::Bool),
        ],
    ),
    (
        "minecraft:command_block",
        &[
            ("Command", Kind::Text),
            ("SuccessCount", Kind::Int),
            ("CustomName", Kind::Text),
            ("TrackOutput", Kind::Bool),
            ("UpdateLastExecution", Kind::Bool),
            ("auto", Kind::Bool),
            ("powered", Kind::Bool),
            ("conditionMet", Kind::Bool),
        ],
    ),
    (
        "minecraft:mob_spawner",
        &[
            ("Delay", Kind::Short),
            ("MinSpawnDelay", Kind::Short),
            ("MaxSpawnDelay", Kind::Short),
            ("SpawnCount", Kind::Short),
            ("MaxNearbyEntities", Kind::Short),
            ("RequiredPlayerRange", Kind::Short),
            ("SpawnRange", Kind::Short),
        ],
    ),
    (
        "minecraft:sculk_sensor",
        &[("last_vibration_frequency", Kind::Int)],
    ),
    (
        "minecraft:calibrated_sculk_sensor",
        &[("last_vibration_frequency", Kind::Int)],
    ),
    ("minecraft:beehive", &[]),
    (
        "minecraft:structure_block",
        &[
            ("name", Kind::Text),
            ("author", Kind::Text),
            ("metadata", Kind::Text),
            ("posX", Kind::Int),
            ("posY", Kind::Int),
            ("posZ", Kind::Int),
            ("sizeX", Kind::Int),
            ("sizeY", Kind::Int),
            ("sizeZ", Kind::Int),
            ("rotation", Kind::Choice(ROTATIONS)),
            ("mirror", Kind::Choice(MIRRORS)),
            ("mode", Kind::Choice(STRUCTURE_MODES)),
            ("ignoreEntities", Kind::Bool),
            ("strict", Kind::Bool),
            ("powered", Kind::Bool),
            ("showair", Kind::Bool),
            ("showboundingbox", Kind::Bool),
            ("integrity", Kind::Float),
            ("seed", Kind::Long),
        ],
    ),
];

/// The longest text a `put` may carry (a command block's command is the long one).
const MAX_TEXT: usize = 32_767;

/// Parses a `put` value into the tag its key is saved as; `None` when it does not fit the type.
fn typed(kind: Kind, value: &str) -> Option<Tag> {
    Some(match kind {
        Kind::Short => Tag::Short(value.parse().ok().filter(|v: &i16| *v >= 0)?),
        Kind::Int => Tag::Int(value.parse().ok()?),
        Kind::Long => Tag::Long(value.parse().ok()?),
        Kind::Float => Tag::Float(value.parse().ok().filter(|v: &f32| v.is_finite())?),
        Kind::Bool => Tag::Byte(match value {
            "true" => 1,
            "false" => 0,
            _ => return None,
        }),
        Kind::Text => Tag::String(
            Some(value)
                .filter(|v| v.chars().count() <= MAX_TEXT)?
                .to_owned(),
        ),
        Kind::Choice(allowed) => Tag::String(allowed.contains(&value).then(|| value.to_owned())?),
    })
}

/// The four `PotDecorations` order slots; vanilla's `sherds` list has exactly these entries.
const POT_SIDES: [&str; 4] = ["back", "left", "right", "front"];
/// `PotDecorations.CODEC` uses `minecraft:brick` for an empty side.
const DEFAULT_SHERD: &str = "minecraft:brick";

/// Vanilla's `minecraft:banner_pattern` entries (`data/minecraft/banner_pattern`), without namespace.
const BANNER_PATTERNS: [&str; 43] = [
    "base",
    "border",
    "bricks",
    "circle",
    "creeper",
    "cross",
    "curly_border",
    "diagonal_left",
    "diagonal_right",
    "diagonal_up_left",
    "diagonal_up_right",
    "flow",
    "flower",
    "globe",
    "gradient",
    "gradient_up",
    "guster",
    "half_horizontal",
    "half_horizontal_bottom",
    "half_vertical",
    "half_vertical_right",
    "mojang",
    "piglin",
    "rhombus",
    "skull",
    "small_stripes",
    "square_bottom_left",
    "square_bottom_right",
    "square_top_left",
    "square_top_right",
    "straight_cross",
    "stripe_bottom",
    "stripe_center",
    "stripe_downleft",
    "stripe_downright",
    "stripe_left",
    "stripe_middle",
    "stripe_right",
    "stripe_top",
    "triangle_bottom",
    "triangle_top",
    "triangles_bottom",
    "triangles_top",
];

/// `DyeColor.CODEC` names.
const DYES: [&str; 16] = [
    "white",
    "orange",
    "magenta",
    "light_blue",
    "yellow",
    "lime",
    "pink",
    "gray",
    "light_gray",
    "cyan",
    "purple",
    "blue",
    "brown",
    "green",
    "red",
    "black",
];

/// `ExtraCodecs.PLAYER_NAME`: at most 16 characters, none a space, control or non-ASCII.
fn player_name_ok(name: &str) -> bool {
    name.chars().count() <= 16 && name.chars().all(|c| c > ' ' && c < '\u{7f}')
}

/// `ResolvableProfile` properties: at most 16, a value up to 32767 characters and a signature up
/// to 1024 (`ExtraCodecs.PROPERTY`/`Property.MAX_SIGNATURE_LENGTH`).
const MAX_PROPERTIES: usize = 16;
const MAX_PROPERTY_VALUE: usize = 32767;
const MAX_PROPERTY_SIGNATURE: usize = 1024;

/// Decodes lowercase or uppercase hex; `None` for an odd length or a non-hex digit.
fn hex_decode(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

/// The most a vanilla stack holds; the client rejects larger counts.
const MAX_COUNT: i32 = 99;

/// The packet for a snapshot, or `None` when the family, a slot, an item or a timer is invalid.
pub fn packet(pos: [i32; 3], entity: &str, entries: &str, separator: char) -> Option<PluginPacket> {
    let banner = entity == "minecraft:banner";
    let skull = entity == "minecraft:skull";
    let keyed = KEYED_STACK.iter().any(|(name, _)| *name == entity);
    let settings = SETTINGS
        .iter()
        .find(|(name, _)| *name == entity)
        .map(|(_, keys)| *keys);
    // A family may also carry settings (the crafter does); settings alone mean no item slots.
    let family = FAMILIES.iter().find(|(name, ..)| *name == entity).copied();
    let (size, furnace) = if banner || skull {
        (0, false)
    } else if let Some((_, size, furnace)) = family {
        (size, furnace)
    } else if keyed {
        (1, false)
    } else {
        // A settings-only block entity (`command_block`, `beacon`, ...).
        settings.map(|_| (0, false))?
    };
    let mut patterns: Vec<(String, String)> = Vec::new();
    let mut profile = None;
    let mut properties: Vec<(String, String, Option<String>)> = Vec::new();
    let mut items: Vec<(u8, String, i32)> = Vec::new();
    let mut metas: Vec<(u8, ferrite_plugin::ItemMetaSpec)> = Vec::new();
    let mut timers = None;
    let mut extra = Compound::new();
    let mut exit = None;
    let mut sherds: Vec<Option<String>> = vec![None; POT_SIDES.len()];
    let mut disabled: Vec<i32> = Vec::new();
    for entry in entries.split(separator).filter(|e| !e.is_empty()) {
        // `put,<key>,<value>`: the value may hold commas (a command), so it takes the rest.
        if let Some(rest) = entry.strip_prefix("put,") {
            let (key, value) = rest.split_once(',')?;
            let (_, kind) = settings?.iter().find(|(name, _)| *name == key)?;
            if extra.contains_key(key) {
                return None;
            }
            extra.insert(key.to_owned(), typed(*kind, value)?);
            continue;
        }
        let mut parts = entry.split(',');
        match parts.next()? {
            // A spawner's mob: `SpawnData` holds the entity's saved form, here only its id.
            "spawn" if entity == "minecraft:mob_spawner" && !extra.contains_key("SpawnData") => {
                let id = parts.next()?;
                if parts.next().is_some() || ferrite_data::entity_props::by_name(id).is_none() {
                    return None;
                }
                let mut entity_tag = Compound::new();
                entity_tag.insert("id".into(), Tag::String(id.to_owned()));
                let mut data = Compound::new();
                data.insert("entity".into(), Tag::Compound(entity_tag));
                extra.insert("SpawnData".into(), Tag::Compound(data));
            }
            // A hive's flower: a block position saved as three ints.
            "flower" if entity == "minecraft:beehive" && !extra.contains_key("flower_pos") => {
                let mut coordinate = || -> Option<i32> { parts.next()?.parse().ok() };
                let at = [coordinate()?, coordinate()?, coordinate()?];
                if parts.next().is_some() {
                    return None;
                }
                extra.insert("flower_pos".into(), Tag::IntArray(at.to_vec()));
            }
            "item" => {
                let slot: u8 = parts
                    .next()?
                    .parse()
                    .ok()
                    .filter(|s| usize::from(*s) < size)?;
                let material = parts.next()?;
                let count: i32 = parts
                    .next()?
                    .parse()
                    .ok()
                    .filter(|c| (1..=MAX_COUNT).contains(c))?;
                if parts.next().is_some() || items.iter().any(|(s, ..)| *s == slot) {
                    return None;
                }
                ferrite_item::item_id(material)?;
                items.push((slot, material.to_owned(), count));
            }
            // The metadata of a stack, hex encoded because it holds the entry separator itself.
            "meta" if size > 0 => {
                let slot: u8 = parts.next()?.parse().ok()?;
                let payload = String::from_utf8(hex_decode(parts.next()?)?).ok()?;
                if parts.next().is_some() || metas.iter().any(|(s, _)| *s == slot) {
                    return None;
                }
                metas.push((slot, super::inventory::parse_item_meta(&payload)?));
            }
            "pattern" if banner && patterns.len() < 16 => {
                let id = parts.next()?;
                let color = parts.next()?;
                let short = id.strip_prefix("minecraft:").unwrap_or(id);
                if parts.next().is_some()
                    || !BANNER_PATTERNS.contains(&short)
                    || !DYES.contains(&color)
                {
                    return None;
                }
                patterns.push((format!("minecraft:{short}"), color.to_owned()));
            }
            "profile" if skull && profile.is_none() => {
                let name = parts.next()?;
                let id = parts.next()?;
                if parts.next().is_some() || !(name.is_empty() || player_name_ok(name)) {
                    return None;
                }
                let id = if id.is_empty() {
                    None
                } else {
                    let uuid = uuid::Uuid::parse_str(id).ok()?.as_u128();
                    Some([
                        (uuid >> 96) as i32,
                        (uuid >> 64) as i32,
                        (uuid >> 32) as i32,
                        uuid as i32,
                    ])
                };
                profile = Some(((!name.is_empty()).then(|| name.to_owned()), id));
            }
            "property" if skull && properties.len() < MAX_PROPERTIES => {
                let name = parts.next()?;
                let value = parts.next()?;
                let signature = parts.next()?;
                if parts.next().is_some()
                    || name.is_empty()
                    || value.chars().count() > MAX_PROPERTY_VALUE
                    || signature.chars().count() > MAX_PROPERTY_SIGNATURE
                {
                    return None;
                }
                properties.push((
                    name.to_owned(),
                    value.to_owned(),
                    (!signature.is_empty()).then(|| signature.to_owned()),
                ));
            }
            // `SkullBlockEntity.saveAdditional`: `note_block_sound` is a resource location.
            "note" if skull && !extra.contains_key("note_block_sound") => {
                let key = parts.next()?;
                if parts.next().is_some() || !super::is_resource_key(key) {
                    return None;
                }
                extra.insert("note_block_sound".into(), Tag::String(key.to_owned()));
            }
            // `TheEndGatewayBlockEntity.saveAdditional`: `exit_portal` is an X/Y/Z compound.
            "exit" if entity == "minecraft:end_gateway" && exit.is_none() => {
                let mut coordinate = || -> Option<i32> { parts.next()?.parse().ok() };
                let at = [coordinate()?, coordinate()?, coordinate()?];
                if parts.next().is_some() {
                    return None;
                }
                exit = Some(at);
            }
            // `DecoratedPotBlockEntity.saveAdditional`: `sherds` in PotDecorations order.
            "sherd" if entity == "minecraft:decorated_pot" => {
                let side = parts.next()?;
                let material = parts.next()?;
                let index = POT_SIDES.iter().position(|name| *name == side)?;
                if parts.next().is_some() || sherds[index].is_some() {
                    return None;
                }
                ferrite_item::item_id(material)?;
                sherds[index] = Some(material.to_owned());
            }
            // `CrafterBlockEntity.saveAdditional`: `disabled_slots` is an int array.
            "disabled" if entity == "minecraft:crafter" => {
                let slot: i32 = parts.next()?.parse().ok().filter(|s| (0..9).contains(s))?;
                if parts.next().is_some() || disabled.contains(&slot) {
                    return None;
                }
                disabled.push(slot);
            }
            "furnace" if furnace && timers.is_none() => {
                let mut value =
                    || -> Option<i16> { parts.next()?.parse().ok().filter(|v| *v >= 0) };
                timers = Some([value()?, value()?, value()?, value()?]);
                if parts.next().is_some() {
                    return None;
                }
            }
            _ => return None,
        }
    }
    // Metadata only makes sense for a stack that is in the snapshot.
    if metas
        .iter()
        .any(|(slot, _)| !items.iter().any(|(item, ..)| item == slot))
    {
        return None;
    }
    items.sort_by_key(|(slot, ..)| *slot);
    if let Some(at) = exit {
        let mut portal = Compound::new();
        portal.insert("X".into(), Tag::Int(at[0]));
        portal.insert("Y".into(), Tag::Int(at[1]));
        portal.insert("Z".into(), Tag::Int(at[2]));
        extra.insert("exit_portal".into(), Tag::Compound(portal));
    }
    if sherds.iter().flatten().count() > 0 {
        // A side with no sherd is `minecraft:brick`, exactly as `PotDecorations` defaults it.
        let list: Vec<String> = sherds
            .iter()
            .map(|sherd| sherd.clone().unwrap_or_else(|| DEFAULT_SHERD.to_owned()))
            .collect();
        extra.insert("sherds".into(), Tag::List(List::String(list)));
    }
    if !disabled.is_empty() {
        disabled.sort_unstable();
        extra.insert("disabled_slots".into(), Tag::IntArray(disabled));
    }
    Some(PluginPacket::TileUpdate {
        pos,
        entity: entity.to_owned(),
        items,
        furnace: timers,
        patterns,
        profile,
        properties,
        metas,
        extra,
    })
}

/// Applies a stack's metadata to the stack (`with_meta` in the game loop).
pub type MetaApplier<'a> =
    &'a dyn Fn(ferrite_item::ItemStack, &ferrite_plugin::ItemMetaSpec) -> ferrite_item::ItemStack;

/// What a snapshot holds, borrowed from the [`PluginPacket::TileUpdate`] that carries it.
pub struct TileData<'a> {
    pub items: &'a [(u8, String, i32)],
    pub furnace: Option<[i16; 4]>,
    pub patterns: &'a [(String, String)],
    pub profile: Option<(Option<String>, Option<[i32; 4]>)>,
    pub properties: &'a [(String, String, Option<String>)],
    pub metas: &'a [(u8, ferrite_plugin::ItemMetaSpec)],
    pub extra: &'a Compound,
}

/// The block entity update tag: vanilla's `Items` list, plus the furnace timers Ferrite keeps.
pub fn nbt(entity: &str, tile: &TileData<'_>, with_meta: MetaApplier<'_>) -> Compound {
    let TileData {
        items,
        furnace,
        patterns,
        profile,
        properties,
        metas,
        extra,
    } = tile;
    let (furnace, profile) = (*furnace, profile.clone());
    let mut data = Compound::new();
    for (key, value) in extra.iter() {
        data.insert(key.clone(), value.clone());
    }
    if entity == "minecraft:beehive" {
        // `BeehiveBlockEntity` always saves its occupants; a snapshot has none.
        data.insert("bees".into(), Tag::List(List::Compound(Vec::new())));
    }
    if entity == "minecraft:banner" {
        let layers = patterns
            .iter()
            .map(|(pattern, color)| {
                let mut layer = Compound::new();
                layer.insert("pattern".into(), Tag::String(pattern.clone()));
                layer.insert("color".into(), Tag::String(color.clone()));
                layer
            })
            .collect();
        data.insert("patterns".into(), Tag::List(List::Compound(layers)));
    }
    if entity == "minecraft:skull" {
        let mut tag = Compound::new();
        if profile.is_some() || !properties.is_empty() {
            if let Some((name, id)) = profile {
                if let Some(name) = name {
                    tag.insert("name".into(), Tag::String(name));
                }
                if let Some(id) = id {
                    tag.insert("id".into(), Tag::IntArray(id.to_vec()));
                }
            }
            if !properties.is_empty() {
                let list = properties
                    .iter()
                    .map(|(name, value, signature)| {
                        let mut property = Compound::new();
                        property.insert("name".into(), Tag::String(name.clone()));
                        property.insert("value".into(), Tag::String(value.clone()));
                        if let Some(signature) = signature {
                            property.insert("signature".into(), Tag::String(signature.clone()));
                        }
                        property
                    })
                    .collect();
                tag.insert("properties".into(), Tag::List(List::Compound(list)));
            }
            data.insert("profile".into(), Tag::Compound(tag));
        }
    }
    if entity == "minecraft:banner" || entity == "minecraft:skull" {
        return data;
    }
    let keyed = KEYED_STACK.iter().find(|(name, _)| *name == entity);
    let container = FAMILIES.iter().any(|(name, ..)| *name == entity);
    if keyed.is_none() && !container && SETTINGS.iter().any(|(name, _)| *name == entity) {
        return data;
    }
    let list = items
        .iter()
        .filter_map(|(slot, material, count)| {
            let id = ferrite_item::item_id(material)?;
            let mut stack = ferrite_item::ItemStack::with_count(id, *count);
            if let Some((_, meta)) = metas.iter().find(|(meta_slot, _)| meta_slot == slot) {
                stack = with_meta(stack, meta);
            }
            let mut entry = stack.to_nbt().ok()?;
            entry.insert("Slot".into(), Tag::Byte(*slot as i8));
            Some(entry)
        })
        .collect::<Vec<_>>();
    if let Some((_, key)) = keyed {
        // A single stack under its own key, without the container slot.
        if let Some(mut stack) = list.into_iter().next() {
            stack.shift_remove("Slot");
            data.insert((*key).to_owned(), Tag::Compound(stack));
        }
        return data;
    }
    data.insert("Items".into(), Tag::List(List::Compound(list)));
    if let Some([burn, burn_total, cook, cook_total]) = furnace {
        data.insert("BurnTime".into(), Tag::Short(burn));
        data.insert("BurnTimeTotal".into(), Tag::Short(burn_total));
        data.insert("CookTime".into(), Tag::Short(cook));
        data.insert("CookTimeTotal".into(), Tag::Short(cook_total));
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_protocol::packets::play::BlockEntityData;
    use ferrite_protocol::{Decode, Encode, VarInt};

    const SEP: char = '\u{1d}';

    fn join(entries: &[&str]) -> String {
        entries.join(&SEP.to_string())
    }

    #[test]
    fn a_chest_snapshot_keeps_its_slots_sorted_and_counted() {
        let entries = join(&["item,13,minecraft:diamond,3", "item,0,minecraft:stone,64"]);
        let Some(PluginPacket::TileUpdate {
            pos,
            entity,
            items,
            furnace,
            ..
        }) = packet([1, 64, 2], "minecraft:chest", &entries, SEP)
        else {
            panic!("a valid chest snapshot");
        };
        assert_eq!(
            (pos, entity.as_str(), furnace),
            ([1, 64, 2], "minecraft:chest", None)
        );
        assert_eq!(
            items,
            [
                (0, "minecraft:stone".to_owned(), 64),
                (13, "minecraft:diamond".to_owned(), 3)
            ]
        );
    }

    #[test]
    fn the_item_container_families_are_modelled_with_their_slot_counts() {
        for (entity, slots) in [
            ("minecraft:dispenser", 9),
            ("minecraft:dropper", 9),
            ("minecraft:hopper", 5),
            ("minecraft:brewing_stand", 5),
            ("minecraft:crafter", 9),
            ("minecraft:campfire", 4),
            ("minecraft:chiseled_bookshelf", 6),
        ] {
            let last = format!("item,{},minecraft:stone,1", slots - 1);
            assert!(packet([0, 0, 0], entity, &last, SEP).is_some(), "{entity}");
            let past = format!("item,{slots},minecraft:stone,1");
            assert_eq!(packet([0, 0, 0], entity, &past, SEP), None, "{entity}");
            // None of them carries furnace timers.
            assert_eq!(
                packet([0, 0, 0], entity, "furnace,0,0,0,0", SEP),
                None,
                "{entity}"
            );
        }
    }

    #[test]
    fn invalid_snapshots_are_rejected_not_repaired() {
        let bad = |entity: &str, entries: &[&str]| packet([0, 0, 0], entity, &join(entries), SEP);
        // Unmodelled family, and a family that does not match the entries.
        assert_eq!(bad("minecraft:trial_spawner", &[]), None);
        assert_eq!(bad("minecraft:banner", &["item,0,minecraft:stone,1"]), None);
        assert_eq!(
            bad("minecraft:banner", &["pattern,minecraft:nope,red"]),
            None
        );
        assert_eq!(
            bad("minecraft:banner", &["pattern,minecraft:cross,plaid"]),
            None
        );
        assert_eq!(
            bad("minecraft:chest", &["pattern,minecraft:cross,red"]),
            None
        );
        assert_eq!(bad("minecraft:skull", &["profile,has space,"]), None);
        assert_eq!(bad("minecraft:skull", &["profile,,not-a-uuid"]), None);
        assert_eq!(bad("minecraft:skull", &["profile,a,", "profile,b,"]), None);
        assert_eq!(
            bad("minecraft:skull", &["profile,this_name_is_way_too_long,"]),
            None
        );
        assert_eq!(bad("minecraft:chest", &["furnace,0,0,0,0"]), None);
        // Slot out of the container, repeated, count out of range, unknown item, trailing field.
        assert_eq!(bad("minecraft:chest", &["item,27,minecraft:stone,1"]), None);
        assert_eq!(
            bad("minecraft:furnace", &["item,3,minecraft:stone,1"]),
            None
        );
        assert_eq!(
            bad(
                "minecraft:chest",
                &["item,1,minecraft:stone,1", "item,1,minecraft:stone,2"]
            ),
            None
        );
        assert_eq!(bad("minecraft:chest", &["item,1,minecraft:stone,0"]), None);
        assert_eq!(
            bad("minecraft:chest", &["item,1,minecraft:stone,100"]),
            None
        );
        assert_eq!(
            bad("minecraft:chest", &["item,1,minecraft:not_an_item,1"]),
            None
        );
        assert_eq!(
            bad("minecraft:chest", &["item,1,minecraft:stone,1,extra"]),
            None
        );
        // Negative or oversized timers, a repeated timer entry.
        assert_eq!(bad("minecraft:furnace", &["furnace,-1,0,0,0"]), None);
        assert_eq!(bad("minecraft:furnace", &["furnace,40000,0,0,0"]), None);
        assert_eq!(
            bad("minecraft:furnace", &["furnace,1,1,1,1", "furnace,1,1,1,1"]),
            None
        );
    }

    #[test]
    fn every_modelled_family_builds_and_an_empty_snapshot_is_an_empty_item_list() {
        for (name, ..) in FAMILIES {
            assert!(packet([0, 0, 0], name, "", SEP).is_some(), "{name}");
            assert!(
                ferrite_data::registries::id("minecraft:block_entity_type", name).is_some(),
                "{name} is a block entity type"
            );
        }
        assert_eq!(
            nbt(
                "minecraft:chest",
                &TileData {
                    items: &[],
                    furnace: None,
                    patterns: &[],
                    profile: None,
                    properties: &[],
                    metas: &[],
                    extra: &Compound::new(),
                },
                &|s, _| s
            )
            .get("Items"),
            Some(&Tag::List(List::Compound(Vec::new())))
        );
    }

    #[test]
    fn a_furnace_snapshot_round_trips_through_the_block_entity_packet() {
        let entries = join(&["item,0,minecraft:iron_ore,5", "furnace,12,200,30,100"]);
        let Some(PluginPacket::TileUpdate { items, furnace, .. }) =
            packet([3, 70, -4], "minecraft:blast_furnace", &entries, SEP)
        else {
            panic!("a valid furnace snapshot");
        };
        let type_id =
            ferrite_data::registries::id("minecraft:block_entity_type", "minecraft:blast_furnace")
                .unwrap();
        let sent = BlockEntityData {
            location: 0x1234,
            block_type: VarInt(type_id),
            nbt: nbt(
                "minecraft:blast_furnace",
                &TileData {
                    items: &items,
                    furnace,
                    patterns: &[],
                    profile: None,
                    properties: &[],
                    metas: &[],
                    extra: &Compound::new(),
                },
                &|s, _| s,
            ),
        };
        let mut bytes = Vec::new();
        sent.encode(&mut bytes);
        let back = BlockEntityData::decode(&mut bytes.as_slice()).unwrap();
        assert_eq!(back, sent);
        assert_eq!(back.nbt.get("BurnTime"), Some(&Tag::Short(12)));
        assert_eq!(back.nbt.get("BurnTimeTotal"), Some(&Tag::Short(200)));
        assert_eq!(back.nbt.get("CookTime"), Some(&Tag::Short(30)));
        assert_eq!(back.nbt.get("CookTimeTotal"), Some(&Tag::Short(100)));
        let Some(Tag::List(List::Compound(list))) = back.nbt.get("Items") else {
            panic!("Items list");
        };
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].get("Slot"), Some(&Tag::Byte(0)));
        assert_eq!(
            list[0].get("id"),
            Some(&Tag::String("minecraft:iron_ore".into()))
        );
    }

    fn round_trip(entity: &str, entries: &[&str]) -> Compound {
        let Some(PluginPacket::TileUpdate {
            items,
            furnace,
            patterns,
            profile,
            properties,
            metas,
            extra,
            ..
        }) = packet([0, 70, 0], entity, &join(entries), SEP)
        else {
            panic!("a valid {entity} snapshot");
        };
        let type_id = ferrite_data::registries::id("minecraft:block_entity_type", entity).unwrap();
        let sent = BlockEntityData {
            location: 1,
            block_type: VarInt(type_id),
            nbt: nbt(
                entity,
                &TileData {
                    items: &items,
                    furnace,
                    patterns: &patterns,
                    profile,
                    properties: &properties,
                    metas: &metas,
                    extra: &extra,
                },
                &|s, _| s,
            ),
        };
        let mut bytes = Vec::new();
        sent.encode(&mut bytes);
        BlockEntityData::decode(&mut bytes.as_slice()).unwrap().nbt
    }

    #[test]
    fn a_banner_sends_its_pattern_layers_and_an_empty_one_sends_an_empty_list() {
        let data = round_trip(
            "minecraft:banner",
            &[
                "pattern,minecraft:stripe_top,red",
                "pattern,cross,light_blue",
            ],
        );
        let Some(Tag::List(List::Compound(layers))) = data.get("patterns") else {
            panic!("patterns list");
        };
        assert_eq!(layers.len(), 2);
        assert_eq!(
            layers[0].get("pattern"),
            Some(&Tag::String("minecraft:stripe_top".into()))
        );
        assert_eq!(
            layers[1].get("color"),
            Some(&Tag::String("light_blue".into()))
        );
        assert_eq!(
            layers[1].get("pattern"),
            Some(&Tag::String("minecraft:cross".into()))
        );
        assert_eq!(data.get("Items"), None);
        let empty = round_trip("minecraft:banner", &[]);
        assert_eq!(
            empty.get("patterns"),
            Some(&Tag::List(List::Compound(Vec::new())))
        );
    }

    #[test]
    fn a_skull_sends_the_profile_name_and_uuid_in_the_resolvable_profile_shape() {
        let data = round_trip(
            "minecraft:skull",
            &["profile,Notch,069a79f4-44e9-4726-a5be-fca90e38aaf5"],
        );
        let Some(Tag::Compound(profile)) = data.get("profile") else {
            panic!("profile compound");
        };
        assert_eq!(profile.get("name"), Some(&Tag::String("Notch".into())));
        assert_eq!(
            profile.get("id"),
            Some(&Tag::IntArray(vec![
                0x069a79f4,
                0x44e94726_u32 as i32,
                0xa5befca9_u32 as i32,
                0x0e38aaf5
            ]))
        );
        // A plain mob head has no profile at all.
        assert_eq!(round_trip("minecraft:skull", &[]).get("profile"), None);
        // A name alone is the unresolved `ResolvableProfile$Partial` form.
        let named = round_trip("minecraft:skull", &["profile,Steve,"]);
        let Some(Tag::Compound(profile)) = named.get("profile") else {
            panic!("profile compound");
        };
        assert_eq!(
            (profile.get("name").is_some(), profile.get("id")),
            (true, None)
        );
    }

    #[test]
    fn a_skull_carries_its_texture_properties_with_and_without_a_signature() {
        let data = round_trip(
            "minecraft:skull",
            &[
                "profile,Notch,",
                "property,textures,eyJ0ZXh0dXJlcyI6e319,c2ln",
                "property,extra,dmFsdWU=,",
            ],
        );
        let Some(Tag::Compound(profile)) = data.get("profile") else {
            panic!("profile compound");
        };
        assert_eq!(profile.get("name"), Some(&Tag::String("Notch".into())));
        let Some(Tag::List(List::Compound(properties))) = profile.get("properties") else {
            panic!("properties list");
        };
        assert_eq!(properties.len(), 2);
        assert_eq!(
            properties[0].get("value"),
            Some(&Tag::String("eyJ0ZXh0dXJlcyI6e319".into()))
        );
        assert_eq!(
            properties[0].get("signature"),
            Some(&Tag::String("c2ln".into()))
        );
        assert_eq!(properties[1].get("signature"), None);
        // Textures alone are a profile too, and only a skull may have them.
        let bare = round_trip("minecraft:skull", &["property,textures,dmFsdWU=,"]);
        assert!(matches!(bare.get("profile"), Some(Tag::Compound(p)) if p.get("name").is_none()));
        let bad = |entity: &str, entries: &[&str]| packet([0, 0, 0], entity, &join(entries), SEP);
        assert_eq!(
            bad("minecraft:banner", &["property,textures,dmFsdWU=,"]),
            None
        );
        assert_eq!(bad("minecraft:skull", &["property,,dmFsdWU=,"]), None);
        assert_eq!(
            bad("minecraft:skull", &["property,textures,dmFsdWU=,sig,extra"]),
            None
        );
        let long = format!("property,textures,{},", "x".repeat(32768));
        assert_eq!(bad("minecraft:skull", &[&long]), None);
        let long_sig = format!("property,textures,v,{}", "s".repeat(1025));
        assert_eq!(bad("minecraft:skull", &[&long_sig]), None);
    }

    #[test]
    fn a_skull_carries_its_note_block_sound() {
        let data = round_trip(
            "minecraft:skull",
            &["profile,Notch,", "note,minecraft:entity.pig.ambient"],
        );
        assert_eq!(
            data.get("note_block_sound"),
            Some(&Tag::String("minecraft:entity.pig.ambient".into()))
        );
        // Without a sound the key is absent rather than an empty string.
        assert_eq!(
            round_trip("minecraft:skull", &[]).get("note_block_sound"),
            None
        );
        let bad = |entity: &str, entries: &[&str]| packet([0, 0, 0], entity, &join(entries), SEP);
        // Repeated, malformed, over-long and wrong-family entries are rejected, not repaired.
        assert_eq!(
            bad(
                "minecraft:skull",
                &[
                    "note,minecraft:entity.pig.ambient",
                    "note,minecraft:entity.cow.ambient",
                ]
            ),
            None
        );
        assert_eq!(bad("minecraft:skull", &["note,not a key"]), None);
        assert_eq!(
            bad(
                "minecraft:skull",
                &["note,minecraft:entity.pig.ambient,extra"]
            ),
            None
        );
        assert_eq!(
            bad("minecraft:banner", &["note,minecraft:entity.pig.ambient"]),
            None
        );
        assert_eq!(
            bad("minecraft:chest", &["note,minecraft:entity.pig.ambient"]),
            None
        );
    }

    fn hex(text: &str) -> String {
        text.bytes().map(|b| format!("{b:02x}")).collect()
    }

    /// A `setItemMeta` payload with a name, a sharpness enchantment and nothing else.
    fn named_sharp() -> String {
        [
            "\"Hi\"",
            "",
            "minecraft:sharpness=3",
            "0",
            "",
            "0",
            "",
            "",
            "",
        ]
        .join("\u{1e}")
    }

    #[test]
    fn a_stack_in_the_snapshot_can_carry_metadata() {
        let meta = format!("meta,1,{}", hex(&named_sharp()));
        let Some(PluginPacket::TileUpdate { metas, items, .. }) = packet(
            [0, 0, 0],
            "minecraft:chest",
            &join(&["item,1,minecraft:diamond_sword,1", &meta]),
            SEP,
        ) else {
            panic!("a valid chest snapshot");
        };
        assert_eq!(items.len(), 1);
        assert_eq!(metas.len(), 1);
        assert_eq!(metas[0].0, 1);
        assert_eq!(metas[0].1.name.as_deref(), Some("\"Hi\""));
        assert_eq!(
            metas[0].1.enchants,
            vec![("minecraft:sharpness".to_owned(), 3)]
        );
        // The applier decides what the metadata does to the stack; `nbt` hands it each stack once.
        let named = nbt(
            "minecraft:chest",
            &TileData {
                items: &items,
                furnace: None,
                patterns: &[],
                profile: None,
                properties: &[],
                metas: &metas,
                extra: &Compound::new(),
            },
            &|mut stack, meta| {
                stack.count = i32::try_from(meta.enchants.len()).unwrap() + 4;
                stack
            },
        );
        let Some(Tag::List(List::Compound(list))) = named.get("Items") else {
            panic!("items list");
        };
        assert_eq!(list[0].get("count"), Some(&Tag::Int(5)));
    }

    #[test]
    fn metadata_without_its_stack_or_in_a_bad_encoding_is_rejected() {
        let good = hex(&named_sharp());
        let bad = |entries: &[&str]| packet([0, 0, 0], "minecraft:chest", &join(entries), SEP);
        assert_eq!(
            bad(&[&format!("meta,0,{good}")]),
            None,
            "no stack in slot 0"
        );
        assert_eq!(
            bad(&["item,0,minecraft:stone,1", &format!("meta,0,{good}x")]),
            None,
            "odd hex"
        );
        assert_eq!(
            bad(&["item,0,minecraft:stone,1", "meta,0,zz"]),
            None,
            "not hex"
        );
        assert_eq!(
            bad(&[
                "item,0,minecraft:stone,1",
                &format!("meta,0,{good}"),
                &format!("meta,0,{good}")
            ]),
            None,
            "two metas for a slot"
        );
        assert_eq!(
            bad(&[
                "item,0,minecraft:stone,1",
                &format!("meta,0,{}", hex("not a payload"))
            ]),
            None,
            "unreadable payload"
        );
        assert_eq!(
            packet(
                [0, 0, 0],
                "minecraft:banner",
                &format!("meta,0,{good}"),
                SEP
            ),
            None,
            "banners hold no items"
        );
    }

    fn nbt_of(entity: &str, entries: &[&str]) -> Compound {
        let Some(PluginPacket::TileUpdate {
            items,
            furnace,
            patterns,
            profile,
            properties,
            metas,
            extra,
            ..
        }) = packet([0, 0, 0], entity, &join(entries), SEP)
        else {
            panic!("{entity} {entries:?} should be a valid snapshot");
        };
        nbt(
            entity,
            &TileData {
                items: &items,
                furnace,
                patterns: &patterns,
                profile,
                properties: &properties,
                metas: &metas,
                extra: &extra,
            },
            &|stack, _| stack,
        )
    }

    #[test]
    fn a_jukebox_and_a_lectern_keep_their_single_stack_under_its_own_key() {
        let jukebox = nbt_of(
            "minecraft:jukebox",
            &[
                "item,0,minecraft:music_disc_cat,1",
                "put,ticks_since_song_started,40",
            ],
        );
        let Some(Tag::Compound(record)) = jukebox.get("RecordItem") else {
            panic!("a RecordItem compound: {jukebox:?}");
        };
        assert_eq!(
            record.get("id"),
            Some(&Tag::String("minecraft:music_disc_cat".into()))
        );
        assert!(!record.contains_key("Slot"));
        assert_eq!(
            jukebox.get("ticks_since_song_started"),
            Some(&Tag::Long(40))
        );
        assert!(!jukebox.contains_key("Items"));

        let lectern = nbt_of(
            "minecraft:lectern",
            &["item,0,minecraft:writable_book,1", "put,Page,3"],
        );
        assert!(matches!(lectern.get("Book"), Some(Tag::Compound(_))));
        assert_eq!(lectern.get("Page"), Some(&Tag::Int(3)));
        // Only one slot exists.
        assert_eq!(
            packet(
                [0, 0, 0],
                "minecraft:lectern",
                "item,1,minecraft:stone,1",
                SEP
            ),
            None
        );
    }

    #[test]
    fn the_remaining_saved_block_entity_fields_match_their_vanilla_tags() {
        // `DecoratedPotBlockEntity`: `sherds` is four PotDecorations slots and `item` is keyed.
        let pot = nbt_of(
            "minecraft:decorated_pot",
            &[
                "item,0,minecraft:diamond,1",
                "sherd,back,minecraft:angler_pottery_sherd",
                "sherd,front,minecraft:skull_pottery_sherd",
            ],
        );
        assert!(matches!(pot.get("item"), Some(Tag::Compound(_))));
        let Some(Tag::List(List::String(sherds))) = pot.get("sherds") else {
            panic!("a sherds string list: {pot:?}");
        };
        assert_eq!(
            sherds,
            &[
                "minecraft:angler_pottery_sherd".to_owned(),
                "minecraft:brick".to_owned(),
                "minecraft:brick".to_owned(),
                "minecraft:skull_pottery_sherd".to_owned()
            ]
        );
        // A pot with no sherds omits the key, as `PotDecorations.EMPTY` does.
        assert_eq!(nbt_of("minecraft:decorated_pot", &[]).get("sherds"), None);

        // `CrafterBlockEntity`: timers, trigger and the disabled-slot int array.
        let crafter = nbt_of(
            "minecraft:crafter",
            &[
                "item,0,minecraft:stone,1",
                "put,crafting_ticks_remaining,30",
                "put,triggered,true",
                "disabled,7",
                "disabled,2",
            ],
        );
        assert_eq!(crafter.get("crafting_ticks_remaining"), Some(&Tag::Int(30)));
        assert_eq!(crafter.get("triggered"), Some(&Tag::Byte(1)));
        assert_eq!(
            crafter.get("disabled_slots"),
            Some(&Tag::IntArray(vec![2, 7]))
        );

        // `ShelfBlockEntity`: three items under `Items`.
        let shelf = nbt_of("minecraft:shelf", &["item,2,minecraft:book,1"]);
        assert!(
            matches!(shelf.get("Items"), Some(Tag::List(List::Compound(list))) if list.len() == 1)
        );

        // `BrushableBlockEntity`: the hidden item under `item`.
        let brushable = nbt_of("minecraft:brushable_block", &["item,0,minecraft:emerald,1"]);
        assert!(matches!(brushable.get("item"), Some(Tag::Compound(_))));

        // `BeaconBlockEntity.getUpdateTag` saves the custom name only.
        let beacon = nbt_of(
            "minecraft:beacon",
            &["put,CustomName,{\"text\":\"Beacon\"}"],
        );
        assert_eq!(
            beacon.get("CustomName"),
            Some(&Tag::String("{\"text\":\"Beacon\"}".into()))
        );
        assert_eq!(beacon.get("primary_effect"), None);
        assert_eq!(
            nbt_of(
                "minecraft:enchanting_table",
                &["put,CustomName,{\"text\":\"Table\"}"]
            )
            .get("CustomName"),
            Some(&Tag::String("{\"text\":\"Table\"}".into()))
        );

        // `SculkShriekerBlockEntity`.
        assert_eq!(
            nbt_of("minecraft:sculk_shrieker", &["put,warning_level,2"]).get("warning_level"),
            Some(&Tag::Int(2))
        );

        // `TheEndGatewayBlockEntity`: `exit_portal` is an X/Y/Z compound.
        let gateway = nbt_of(
            "minecraft:end_gateway",
            &["put,Age,90", "put,ExactTeleport,true", "exit,1,64,-2"],
        );
        assert_eq!(gateway.get("Age"), Some(&Tag::Long(90)));
        assert_eq!(gateway.get("ExactTeleport"), Some(&Tag::Byte(1)));
        let Some(Tag::Compound(exit)) = gateway.get("exit_portal") else {
            panic!("exit_portal compound: {gateway:?}");
        };
        assert_eq!(exit.get("X"), Some(&Tag::Int(1)));
        assert_eq!(exit.get("Y"), Some(&Tag::Int(64)));
        assert_eq!(exit.get("Z"), Some(&Tag::Int(-2)));

        // Bad sides, duplicate sides, out-of-range slots and a fourth shelf stack are refused.
        for (entity, entry) in [
            ("minecraft:decorated_pot", "sherd,side,minecraft:brick"),
            (
                "minecraft:decorated_pot",
                "sherd,back,minecraft:not_an_item",
            ),
            ("minecraft:crafter", "disabled,9"),
            ("minecraft:end_gateway", "exit,1,2"),
        ] {
            assert_eq!(
                packet([0, 0, 0], entity, entry, SEP),
                None,
                "{entity} {entry}"
            );
        }
        assert_eq!(
            packet(
                [0, 0, 0],
                "minecraft:decorated_pot",
                &join(&["sherd,back,minecraft:brick", "sherd,back,minecraft:brick",]),
                SEP
            ),
            None
        );
        assert_eq!(
            packet(
                [0, 0, 0],
                "minecraft:shelf",
                "item,3,minecraft:stone,1",
                SEP
            ),
            None
        );
    }

    #[test]
    fn settings_are_typed_per_family_and_unknown_keys_are_rejected() {
        let command = nbt_of(
            "minecraft:command_block",
            &[
                "put,Command,say hi, there",
                "put,auto,true",
                "put,SuccessCount,2",
            ],
        );
        assert_eq!(
            command.get("Command"),
            Some(&Tag::String("say hi, there".into()))
        );
        assert_eq!(command.get("auto"), Some(&Tag::Byte(1)));
        assert_eq!(command.get("SuccessCount"), Some(&Tag::Int(2)));

        let spawner = nbt_of(
            "minecraft:mob_spawner",
            &["spawn,minecraft:zombie", "put,SpawnCount,6", "put,Delay,20"],
        );
        assert_eq!(spawner.get("SpawnCount"), Some(&Tag::Short(6)));
        let Some(Tag::Compound(data)) = spawner.get("SpawnData") else {
            panic!("SpawnData");
        };
        assert!(matches!(data.get("entity"), Some(Tag::Compound(entity))
            if entity.get("id") == Some(&Tag::String("minecraft:zombie".into()))));

        let structure = nbt_of(
            "minecraft:structure_block",
            &[
                "put,name,test:house",
                "put,mode,save",
                "put,rotation,180",
                "put,integrity,0.5",
                "put,sizeX,4",
                "put,seed,9",
            ],
        );
        assert_eq!(structure.get("rotation"), Some(&Tag::String("180".into())));
        assert_eq!(structure.get("integrity"), Some(&Tag::Float(0.5)));
        assert_eq!(structure.get("seed"), Some(&Tag::Long(9)));

        let sensor = nbt_of(
            "minecraft:sculk_sensor",
            &["put,last_vibration_frequency,7"],
        );
        assert_eq!(sensor.get("last_vibration_frequency"), Some(&Tag::Int(7)));

        let hive = nbt_of("minecraft:beehive", &["flower,1,64,-2"]);
        assert_eq!(
            hive.get("flower_pos"),
            Some(&Tag::IntArray(vec![1, 64, -2]))
        );
        assert!(matches!(hive.get("bees"), Some(Tag::List(_))));

        for (entity, entry) in [
            ("minecraft:command_block", "put,Page,1"),
            ("minecraft:command_block", "put,auto,maybe"),
            ("minecraft:mob_spawner", "put,SpawnCount,-1"),
            ("minecraft:mob_spawner", "spawn,minecraft:not_a_mob"),
            ("minecraft:structure_block", "put,mode,explode"),
            ("minecraft:structure_block", "put,integrity,nan"),
            ("minecraft:jukebox", "put,Page,1"),
            ("minecraft:chest", "put,Page,1"),
            ("minecraft:beehive", "flower,1,2"),
        ] {
            assert_eq!(
                packet([0, 0, 0], entity, entry, SEP),
                None,
                "{entity} {entry}"
            );
        }
        // A key may be set once.
        assert_eq!(
            packet(
                [0, 0, 0],
                "minecraft:lectern",
                &join(&["put,Page,1", "put,Page,2"]),
                SEP
            ),
            None
        );
    }
}
