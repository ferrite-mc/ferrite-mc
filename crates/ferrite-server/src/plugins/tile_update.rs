//! `Player#sendBlockUpdate(Location, TileState)`: a tile snapshot shown to one player.
//!
//! The Java side turns a `TileState` into entries (`item,<slot>,<material>,<count>` and
//! `furnace,<burn>,<burn total>,<cook>,<cook total>`); this module validates them against the block
//! entity families Ferrite models (the `Items` containers of `block_container`/`furnace`) and builds the
//! `ClientboundBlockEntityDataPacket` tag. Families Ferrite has no model for (banner, skull, ...) never
//! get here: the Java side refuses them, and an unknown type name is rejected again below.

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

/// The most a vanilla stack holds; the client rejects larger counts.
const MAX_COUNT: i32 = 99;

/// The packet for a snapshot, or `None` when the family, a slot, an item or a timer is invalid.
pub fn packet(pos: [i32; 3], entity: &str, entries: &str, separator: char) -> Option<PluginPacket> {
    let (_, size, furnace) = FAMILIES
        .iter()
        .find(|(name, ..)| *name == entity)
        .copied()?;
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
    })
}

/// The block entity update tag: vanilla's `Items` list, plus the furnace timers Ferrite keeps.
pub fn nbt(items: &[(u8, String, i32)], furnace: Option<[i16; 4]>) -> Compound {
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
    let mut data = Compound::new();
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
        assert_eq!(bad("minecraft:banner", &[]), None);
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
            nbt(&[], None).get("Items"),
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
            nbt: nbt(&items, furnace),
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
}
