//! `Player#sendBlockUpdate(Location, TileState)`: a tile snapshot shown to one player.
//!
//! The Java side turns a `TileState` into entries (`item,<slot>,<material>,<count>` and
//! `furnace,<burn>,<burn total>,<cook>,<cook total>`); this module validates them against the block
//! entity families Ferrite models and builds the `ClientboundBlockEntityDataPacket` tag: the `Items`
//! containers of `block_container`/`furnace`, banner `patterns` (`BannerBlockEntity.saveAdditional`:
//! list of `{pattern, color}`) and skull `profile` (`SkullBlockEntity`: `ResolvableProfile` with the
//! `name`/`id` keys of `ResolvableProfile$Partial`). Families Ferrite has no model for never get
//! here: the Java side refuses them, and an unknown type name is rejected again below.

use ferrite_nbt::{Compound, List, Tag};
use ferrite_plugin::PluginPacket;

/// `(block entity type, slot count, has furnace timers)`.
const FAMILIES: [(&str, usize, bool); 7] = [
    ("minecraft:chest", 27, false),
    ("minecraft:trapped_chest", 27, false),
    ("minecraft:barrel", 27, false),
    ("minecraft:shulker_box", 27, false),
    ("minecraft:furnace", 3, true),
    ("minecraft:blast_furnace", 3, true),
    ("minecraft:smoker", 3, true),
];

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

/// The most a vanilla stack holds; the client rejects larger counts.
const MAX_COUNT: i32 = 99;

/// The packet for a snapshot, or `None` when the family, a slot, an item or a timer is invalid.
pub fn packet(pos: [i32; 3], entity: &str, entries: &str, separator: char) -> Option<PluginPacket> {
    let banner = entity == "minecraft:banner";
    let skull = entity == "minecraft:skull";
    let (size, furnace) = if banner || skull {
        (0, false)
    } else {
        let (_, size, furnace) = FAMILIES
            .iter()
            .find(|(name, ..)| *name == entity)
            .copied()?;
        (size, furnace)
    };
    let mut patterns: Vec<(String, String)> = Vec::new();
    let mut profile = None;
    let mut items: Vec<(u8, String, i32)> = Vec::new();
    let mut timers = None;
    for entry in entries.split(separator).filter(|e| !e.is_empty()) {
        let mut parts = entry.split(',');
        match parts.next()? {
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
    items.sort_by_key(|(slot, ..)| *slot);
    Some(PluginPacket::TileUpdate {
        pos,
        entity: entity.to_owned(),
        items,
        furnace: timers,
        patterns,
        profile,
    })
}

/// The block entity update tag: vanilla's `Items` list, plus the furnace timers Ferrite keeps.
pub fn nbt(
    entity: &str,
    items: &[(u8, String, i32)],
    furnace: Option<[i16; 4]>,
    patterns: &[(String, String)],
    profile: Option<(Option<String>, Option<[i32; 4]>)>,
) -> Compound {
    let mut data = Compound::new();
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
        if let Some((name, id)) = profile {
            if let Some(name) = name {
                tag.insert("name".into(), Tag::String(name));
            }
            if let Some(id) = id {
                tag.insert("id".into(), Tag::IntArray(id.to_vec()));
            }
            data.insert("profile".into(), Tag::Compound(tag));
        }
    }
    if entity == "minecraft:banner" || entity == "minecraft:skull" {
        return data;
    }
    let list = items
        .iter()
        .filter_map(|(slot, material, count)| {
            let id = ferrite_item::item_id(material)?;
            let mut entry = ferrite_item::ItemStack::with_count(id, *count)
                .to_nbt()
                .ok()?;
            entry.insert("Slot".into(), Tag::Byte(*slot as i8));
            Some(entry)
        })
        .collect();
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
    fn invalid_snapshots_are_rejected_not_repaired() {
        let bad = |entity: &str, entries: &[&str]| packet([0, 0, 0], entity, &join(entries), SEP);
        // Unmodelled family, and a family that does not match the entries.
        assert_eq!(bad("minecraft:beacon", &[]), None);
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
            nbt("minecraft:chest", &[], None, &[], None).get("Items"),
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
            nbt: nbt("minecraft:blast_furnace", &items, furnace, &[], None),
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
            ..
        }) = packet([0, 70, 0], entity, &join(entries), SEP)
        else {
            panic!("a valid {entity} snapshot");
        };
        let type_id = ferrite_data::registries::id("minecraft:block_entity_type", entity).unwrap();
        let sent = BlockEntityData {
            location: 1,
            block_type: VarInt(type_id),
            nbt: nbt(entity, &items, furnace, &patterns, profile),
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
}
