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

    /// Bit masks inside [`SHARED_FLAGS`].
    pub const FLAG_ON_FIRE: u8 = 0x01;
    pub const FLAG_CROUCHING: u8 = 0x02;
    pub const FLAG_SPRINTING: u8 = 0x08;
    pub const FLAG_SWIMMING: u8 = 0x10;
    pub const FLAG_INVISIBLE: u8 = 0x20;
    pub const FLAG_GLOWING: u8 = 0x40;
    pub const FLAG_FALL_FLYING: u8 = 0x80;
}

/// Accessor indices of living entities (vanilla `LivingEntity`).
pub mod living {
    pub const LIVING_FLAGS: u8 = 8;
    pub const HEALTH: u8 = 9;
    pub const EFFECT_AMBIENCE: u8 = 11;
    pub const ARROW_COUNT: u8 = 12;
    pub const STINGER_COUNT: u8 = 13;
    pub const SLEEPING_POS: u8 = 14;
}

/// Accessor indices of mobs (vanilla `Mob`).
pub mod mob {
    pub const MOB_FLAGS: u8 = 15;
    pub const FLAG_NO_AI: u8 = 0x01;
    pub const FLAG_LEFT_HANDED: u8 = 0x02;
    pub const FLAG_AGGRESSIVE: u8 = 0x04;
}

/// Accessor indices of players (vanilla `Player`).
pub mod player {
    pub const ABSORPTION: u8 = 15;
    pub const SCORE: u8 = 16;
    pub const SKIN_CUSTOMIZATION: u8 = 17;
    pub const MAIN_HAND: u8 = 18;
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

    /// Defines the standard `LivingEntity` metadata indices with `max_health`.
    pub fn define_living(&mut self, max_health: f32) {
        self.define(living::LIVING_FLAGS, MetaValue::Byte(0));
        self.define(living::HEALTH, MetaValue::Float(max_health));
        self.define(living::EFFECT_AMBIENCE, MetaValue::Boolean(false));
        self.define(living::ARROW_COUNT, MetaValue::Int(0));
        self.define(living::STINGER_COUNT, MetaValue::Int(0));
        self.define(living::SLEEPING_POS, MetaValue::OptionalBlockPos(None));
    }

    /// Defines `LivingEntity`, `Mob`, and representative 26.3 per-type defaults for `kind`.
    pub fn define_mob(&mut self, kind: crate::mob::MobKind) {
        self.define_living(kind.max_health());
        self.define(mob::MOB_FLAGS, MetaValue::Byte(0));
        match kind {
            crate::mob::MobKind::Creeper => {
                self.define(16, MetaValue::Int(-1));
                self.define(17, MetaValue::Boolean(false));
                self.define(18, MetaValue::Boolean(false));
            }
            crate::mob::MobKind::Enderman => {
                self.define(16, MetaValue::OptionalBlockState(None));
                self.define(17, MetaValue::Boolean(false));
                self.define(18, MetaValue::Boolean(false));
            }
            crate::mob::MobKind::Sheep => {
                self.define(16, MetaValue::Boolean(false));
                self.define(17, MetaValue::Byte(0));
            }
            crate::mob::MobKind::Wolf | crate::mob::MobKind::Cat => {
                self.define(16, MetaValue::Boolean(false));
                self.define(17, MetaValue::Byte(0));
                self.define(18, MetaValue::OptionalUuid(None));
            }
            crate::mob::MobKind::Villager | crate::mob::MobKind::WanderingTrader => {
                self.define(16, MetaValue::Boolean(false));
                self.define(17, MetaValue::Int(0));
                self.define(
                    18,
                    MetaValue::VillagerData {
                        kind: 2,
                        profession: 0,
                        level: 1,
                    },
                );
            }
            crate::mob::MobKind::Cow | crate::mob::MobKind::Pig | crate::mob::MobKind::Chicken => {
                self.define(16, MetaValue::Boolean(false));
            }
            _ => {}
        }
    }

    /// Builds default 26.3 metadata for an online player.
    pub fn for_player() -> Self {
        let mut meta = Self::default();
        meta.define_living(20.0);
        meta.define(player::ABSORPTION, MetaValue::Float(0.0));
        meta.define(player::SCORE, MetaValue::Int(0));
        meta.define(player::SKIN_CUSTOMIZATION, MetaValue::Byte(0));
        meta.define(player::MAIN_HAND, MetaValue::Byte(1));
        meta
    }

    /// Updates one bit in [`base::SHARED_FLAGS`].
    pub fn set_shared_flag(&mut self, bit: u8, enabled: bool) {
        let current = match self.get(base::SHARED_FLAGS) {
            Some(MetaValue::Byte(flags)) => *flags as u8,
            _ => 0,
        };
        let next = if enabled {
            current | bit
        } else {
            current & !bit
        };
        self.set(base::SHARED_FLAGS, MetaValue::Byte(next as i8));
    }

    /// Reads one bit in [`base::SHARED_FLAGS`].
    pub fn shared_flag(&self, bit: u8) -> bool {
        match self.get(base::SHARED_FLAGS) {
            Some(MetaValue::Byte(flags)) => ((*flags as u8) & bit) != 0,
            _ => false,
        }
    }

    /// Updates [`base::POSE`].
    pub fn set_pose(&mut self, pose_id: i32) {
        self.set(base::POSE, MetaValue::Pose(pose_id));
    }

    /// Reads [`base::POSE`], defaulting to `0` (STANDING).
    pub fn pose(&self) -> i32 {
        match self.get(base::POSE) {
            Some(MetaValue::Pose(pose_id)) => *pose_id,
            _ => 0,
        }
    }

    /// Updates [`base::CUSTOM_NAME_VISIBLE`].
    pub fn set_custom_name_visible(&mut self, visible: bool) {
        self.set(base::CUSTOM_NAME_VISIBLE, MetaValue::Boolean(visible));
    }

    /// Updates [`living::HEALTH`] when defined.
    pub fn set_health(&mut self, health: f32) {
        self.set(living::HEALTH, MetaValue::Float(health));
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

    #[test]
    fn living_mob_and_player_defaults_define_expected_indices() {
        let mut creeper = Metadata::default();
        creeper.define_mob(crate::mob::MobKind::Creeper);
        assert_eq!(creeper.get(living::HEALTH), Some(&MetaValue::Float(20.0)));
        assert_eq!(creeper.get(mob::MOB_FLAGS), Some(&MetaValue::Byte(0)));
        assert_eq!(creeper.get(16), Some(&MetaValue::Int(-1)));

        let mut sheep = Metadata::default();
        sheep.define_mob(crate::mob::MobKind::Sheep);
        assert_eq!(sheep.get(living::HEALTH), Some(&MetaValue::Float(8.0)));
        assert_eq!(sheep.get(17), Some(&MetaValue::Byte(0)));

        let mut player = Metadata::for_player();
        assert_eq!(player.get(player::MAIN_HAND), Some(&MetaValue::Byte(1)));
        player.set_shared_flag(base::FLAG_GLOWING, true);
        assert!(player.shared_flag(base::FLAG_GLOWING));
        player.set_pose(5);
        assert_eq!(player.pose(), 5);
        player.set_custom_name_visible(true);
        player.set_health(15.5);
        let dirty = player.pack_dirty().unwrap();
        assert_eq!(dirty.len(), 4);
    }
}
