//! Remaining 26.3 non-mob entities: Display entities (`block_display`, `item_display`,
//! `text_display`), `interaction`, `mannequin`, `ominous_item_spawner`, `armor_stand`,
//! `item_frame`, `glow_item_frame`, and `painting`.

use crate::id::EntityKey;
use crate::store::{EntityData, EntityRecord, EntityStore, Flags};
use crate::{allocate_id, random_uuid};
use ferrite_math::Vec3;
use ferrite_protocol::packets::play::{ItemStack as WireItem, MetaValue};

/// All non-mob decorative/utility 26.3 entity types handled by this module.
pub const NON_MOB_TYPES: [&str; 10] = [
    "block_display",
    "item_display",
    "text_display",
    "interaction",
    "mannequin",
    "ominous_item_spawner",
    "armor_stand",
    "item_frame",
    "glow_item_frame",
    "painting",
];

fn bare(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

/// Whether `name` is one of the supported non-mob decorative/display/interaction entity types.
pub fn is_non_mob_name(name: &str) -> bool {
    let short = bare(name);
    NON_MOB_TYPES.contains(&short)
}

/// Metadata index of `ItemFrame.DATA_ITEM` (index 9) and `DATA_ROTATION` (index 10).
pub const ITEM_FRAME_ITEM_INDEX: u8 = 9;
pub const ITEM_FRAME_ROTATION_INDEX: u8 = 10;

/// Constructs a new non-mob 26.3 entity record at `pos` with its base metadata initialized.
pub fn new_non_mob(name: &str, pos: Vec3, yaw: f32) -> Option<EntityRecord> {
    let short = bare(name);
    if !NON_MOB_TYPES.contains(&short) {
        return None;
    }
    let props = ferrite_data::entity_props::by_name(short)?;
    let mut data = EntityData::new(
        allocate_id(),
        random_uuid(),
        props.id,
        f64::from(props.width),
        f64::from(props.height),
    );
    data.yaw = yaw;
    data.head_yaw = yaw;
    match short {
        "item_frame" | "glow_item_frame" => {
            data.metadata.define(
                ITEM_FRAME_ITEM_INDEX,
                MetaValue::ItemStack(WireItem::default()),
            );
            data.metadata
                .define(ITEM_FRAME_ROTATION_INDEX, MetaValue::Int(0));
            data.metadata.clear_dirty();
        }
        "interaction" => {
            // Interaction: index 8 = width (Float), 9 = height (Float), 10 = response (Boolean)
            data.metadata.define(8, MetaValue::Float(1.0));
            data.metadata.define(9, MetaValue::Float(1.0));
            data.metadata.define(10, MetaValue::Boolean(false));
            data.metadata.clear_dirty();
        }
        _ => {}
    }
    Some(EntityRecord {
        pos,
        vel: Vec3::ZERO,
        flags: Flags::default(),
        data,
    })
}

/// Vanilla `ItemFrame.interact`: inserts 1 item when empty (returning `Some(true)` to consume
/// 1 held item), or rotates an existing framed item 0..=7 (returning `Some(false)`).
pub fn interact_item_frame(
    store: &mut EntityStore,
    key: EntityKey,
    held: &ferrite_item::ItemStack,
) -> Option<bool> {
    let data = store.data_mut(key)?;
    let kind_key = bare(ferrite_data::entity_props::get(data.kind).key);
    if !matches!(kind_key, "item_frame" | "glow_item_frame") {
        return None;
    }
    let current_empty = match data.metadata.get(ITEM_FRAME_ITEM_INDEX) {
        Some(MetaValue::ItemStack(s)) => s.count <= 0,
        _ => true,
    };
    if current_empty {
        if held.is_empty() {
            return Some(false);
        }
        data.metadata.set(
            ITEM_FRAME_ITEM_INDEX,
            MetaValue::ItemStack(WireItem {
                count: 1,
                item_id: i32::from(held.item),
            }),
        );
        data.metadata
            .set(ITEM_FRAME_ROTATION_INDEX, MetaValue::Int(0));
        Some(true)
    } else {
        let rot = match data.metadata.get(ITEM_FRAME_ROTATION_INDEX) {
            Some(MetaValue::Int(r)) => *r,
            _ => 0,
        };
        data.metadata
            .set(ITEM_FRAME_ROTATION_INDEX, MetaValue::Int((rot + 1) % 8));
        Some(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_ten_non_mob_entity_types_spawn_and_item_frame_interacts() {
        let mut store = EntityStore::default();
        for name in NON_MOB_TYPES {
            let rec = new_non_mob(name, Vec3::new(0.5, 64.0, 0.5), 0.0)
                .unwrap_or_else(|| panic!("non-mob {name} must construct"));
            assert_eq!(
                bare(ferrite_data::entity_props::get(rec.data.kind).key),
                name
            );
            store.insert(rec);
        }
        let frame_key = store
            .keys()
            .iter()
            .copied()
            .find(|&k| {
                store.data(k).is_some_and(|d| {
                    bare(ferrite_data::entity_props::get(d.kind).key) == "item_frame"
                })
            })
            .unwrap();
        let apple = ferrite_item::ItemStack::from_name("minecraft:apple").unwrap();
        assert_eq!(
            interact_item_frame(&mut store, frame_key, &apple),
            Some(true)
        );
        assert_eq!(
            interact_item_frame(&mut store, frame_key, &apple),
            Some(false)
        );
        assert_eq!(
            store
                .data(frame_key)
                .unwrap()
                .metadata
                .get(ITEM_FRAME_ROTATION_INDEX),
            Some(&MetaValue::Int(1))
        );
    }
}
