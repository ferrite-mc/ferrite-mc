//! Synched entity data (vanilla `SynchedEntityData`): typed values at accessor indices, each
//! with its default and a dirty bit, so trackers send only what changed.

use ferrite_protocol::packets::play::{MetaEntry, MetaValue};

/// Accessor indices every entity defines (vanilla `Entity`).
pub mod base {
    pub const SHARED_FLAGS: u8 = 0;
    pub const AIR_SUPPLY: u8 = 1;
    pub const CUSTOM_NAME_VISIBLE: u8 = 3;
    pub const SILENT: u8 = 4;
    pub const NO_GRAVITY: u8 = 5;
    pub const POSE: u8 = 6;
    pub const TICKS_FROZEN: u8 = 7;
}

/// Accessor indices of item entities (vanilla `ItemEntity`).
pub mod item {
    pub const ITEM: u8 = 8;
}

/// Accessor indices of experience orbs (vanilla `ExperienceOrb`).
pub mod experience {
    pub const VALUE: u8 = 8;
}

/// Accessor indices of fishing bobbers (vanilla `FishingHook`): the hooked entity's network id
/// plus one (0 for none) and whether a fish is biting.
pub mod fishing_hook {
    pub const HOOKED_ENTITY: u8 = 8;
    pub const BITING: u8 = 9;
}

/// Accessor indices of area effect clouds (vanilla `AreaEffectCloud`): radius, whether the cloud
/// is still in its wait phase, and the particle it emits. Vanilla does not synch the colour,
/// `radius_on_use`, duration or wait time separately.
pub mod area_effect_cloud {
    pub const RADIUS: u8 = 8;
    pub const WAITING: u8 = 9;
    pub const PARTICLE: u8 = 10;
}

#[derive(Debug, Clone, PartialEq)]
struct Item {
    index: u8,
    value: MetaValue,
    default: MetaValue,
    dirty: bool,
}

/// The data values of one entity.
#[derive(Debug, Clone, PartialEq)]
pub struct Metadata {
    items: Vec<Item>,
    dirty: bool,
}

impl Default for Metadata {
    /// The values every entity defines. Vanilla's index 2 (optional custom name) is omitted
    /// until components are supported; it is never non-default, so nothing is lost on the wire.
    fn default() -> Self {
        let mut data = Self {
            items: Vec::new(),
            dirty: false,
        };
        data.define(base::SHARED_FLAGS, MetaValue::Byte(0));
        data.define(base::AIR_SUPPLY, MetaValue::Int(300));
        data.define(base::CUSTOM_NAME_VISIBLE, MetaValue::Boolean(false));
        data.define(base::SILENT, MetaValue::Boolean(false));
        data.define(base::NO_GRAVITY, MetaValue::Boolean(false));
        data.define(base::POSE, MetaValue::Pose(0));
        data.define(base::TICKS_FROZEN, MetaValue::Int(0));
        data
    }
}

impl Metadata {
    /// Defines an accessor with its default value; redefining an index replaces it.
    pub fn define(&mut self, index: u8, default: MetaValue) {
        let item = Item {
            index,
            value: default.clone(),
            default,
            dirty: false,
        };
        match self.items.binary_search_by_key(&index, |i| i.index) {
            Ok(at) => self.items[at] = item,
            Err(at) => self.items.insert(at, item),
        }
    }

    pub fn get(&self, index: u8) -> Option<&MetaValue> {
        self.items
            .binary_search_by_key(&index, |i| i.index)
            .ok()
            .map(|at| &self.items[at].value)
    }

    /// Sets a defined value; a changed value becomes dirty. Undefined indices are ignored.
    pub fn set(&mut self, index: u8, value: MetaValue) {
        if let Ok(at) = self.items.binary_search_by_key(&index, |i| i.index) {
            let item = &mut self.items[at];
            if item.value != value {
                item.value = value;
                item.dirty = true;
                self.dirty = true;
            }
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Values changed since the last call, clearing their dirty bits (vanilla `packDirty`).
    pub fn pack_dirty(&mut self) -> Option<Vec<MetaEntry>> {
        if !self.dirty {
            return None;
        }
        self.dirty = false;
        let out = self
            .items
            .iter_mut()
            .filter_map(|i| std::mem::take(&mut i.dirty).then_some(&*i))
            .map(|i| MetaEntry {
                index: i.index,
                value: i.value.clone(),
            })
            .collect();
        Some(out)
    }

    /// Values that differ from their defaults, sent when a player starts tracking the entity.
    pub fn non_default(&self) -> Vec<MetaEntry> {
        self.items
            .iter()
            .filter(|i| i.value != i.default)
            .map(|i| MetaEntry {
                index: i.index,
                value: i.value.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_protocol::packets::play::ItemStack;

    #[test]
    fn only_changed_values_are_packed() {
        let mut data = Metadata::default();
        data.define(item::ITEM, MetaValue::ItemStack(ItemStack::default()));
        assert!(data.non_default().is_empty());
        assert_eq!(data.pack_dirty(), None);
        let stone = MetaValue::ItemStack(ItemStack {
            count: 1,
            item_id: 1,
        });
        data.set(item::ITEM, stone.clone());
        data.set(base::NO_GRAVITY, MetaValue::Boolean(false));
        let packed = data.pack_dirty().unwrap();
        assert_eq!(packed.len(), 1);
        assert_eq!(packed[0].index, item::ITEM);
        assert_eq!(data.pack_dirty(), None);
        // Setting the same value again does not dirty it.
        data.set(item::ITEM, stone);
        assert!(!data.is_dirty());
        assert_eq!(data.non_default().len(), 1);
    }
}
