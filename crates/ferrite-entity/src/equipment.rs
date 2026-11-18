//! Generic entity equipment state and NBT persistence.
//!
//! Covers the eight vanilla `EquipmentSlot` positions (main hand, off hand, feet, legs,
//! chest, head, body, saddle) and per-slot drop chances for any entity or player. Empty slots
//! are `None` (wire representation omits them or encodes [`ItemStack::empty`]).

use ferrite_item::ItemStack;
use ferrite_nbt::{Compound, Tag};

/// Number of vanilla entity equipment slots (6 standard + body + saddle).
pub const SLOT_COUNT: usize = 8;
/// Vanilla default mob equipment drop chance (`0.085f`).
pub const DEFAULT_DROP_CHANCE: f32 = 0.085;

/// Vanilla `EquipmentSlot` positions in wire order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum EquipmentSlot {
    MainHand = 0,
    OffHand = 1,
    Feet = 2,
    Legs = 3,
    Chest = 4,
    Head = 5,
    Body = 6,
    Saddle = 7,
}

impl EquipmentSlot {
    /// All eight slots in vanilla wire order.
    pub const ALL: [Self; SLOT_COUNT] = [
        Self::MainHand,
        Self::OffHand,
        Self::Feet,
        Self::Legs,
        Self::Chest,
        Self::Head,
        Self::Body,
        Self::Saddle,
    ];

    /// Parses a vanilla wire slot index (`0..=7`).
    pub fn from_index(index: u8) -> Option<Self> {
        match index {
            0 => Some(Self::MainHand),
            1 => Some(Self::OffHand),
            2 => Some(Self::Feet),
            3 => Some(Self::Legs),
            4 => Some(Self::Chest),
            5 => Some(Self::Head),
            6 => Some(Self::Body),
            7 => Some(Self::Saddle),
            _ => None,
        }
    }

    /// Parses a Bukkit/vanilla slot name case-insensitively (`HAND`/`MAIN_HAND`, `OFF_HAND`,
    /// `FEET`, `LEGS`, `CHEST`, `HEAD`, `BODY`, `SADDLE`).
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_uppercase().as_str() {
            "HAND" | "MAIN_HAND" | "MAINHAND" => Some(Self::MainHand),
            "OFF_HAND" | "OFFHAND" => Some(Self::OffHand),
            "FEET" | "BOOTS" => Some(Self::Feet),
            "LEGS" | "LEGGINGS" => Some(Self::Legs),
            "CHEST" | "CHESTPLATE" => Some(Self::Chest),
            "HEAD" | "HELMET" => Some(Self::Head),
            "BODY" => Some(Self::Body),
            "SADDLE" => Some(Self::Saddle),
            _ => None,
        }
    }

    /// Dense array index (`0..8`).
    pub const fn as_index(self) -> usize {
        self as usize
    }

    /// Wire slot id used by `ClientboundSetEquipmentPacket`.
    pub const fn as_wire_slot(self) -> u8 {
        self as u8
    }

    /// Canonical uppercase slot name matching Bukkit's `EquipmentSlot`.
    pub const fn as_name(self) -> &'static str {
        match self {
            Self::MainHand => "HAND",
            Self::OffHand => "OFF_HAND",
            Self::Feet => "FEET",
            Self::Legs => "LEGS",
            Self::Chest => "CHEST",
            Self::Head => "HEAD",
            Self::Body => "BODY",
            Self::Saddle => "SADDLE",
        }
    }

    /// Vanilla 26.3 NBT compound key inside `equipment` and `drop_chances`.
    pub const fn nbt_key(self) -> &'static str {
        match self {
            Self::MainHand => "mainhand",
            Self::OffHand => "offhand",
            Self::Feet => "feet",
            Self::Legs => "legs",
            Self::Chest => "chest",
            Self::Head => "head",
            Self::Body => "body",
            Self::Saddle => "saddle",
        }
    }
}

/// Eight-slot entity equipment component with per-slot drop chances, cloned across region
/// split/merge and persisted as NBT.
#[derive(Debug, Clone)]
pub struct Equipment {
    slots: [Option<ItemStack>; SLOT_COUNT],
    drop_chances: [f32; SLOT_COUNT],
    dirty: u8,
}

impl Default for Equipment {
    fn default() -> Self {
        Self {
            slots: std::array::from_fn(|_| None),
            drop_chances: [DEFAULT_DROP_CHANCE; SLOT_COUNT],
            dirty: 0,
        }
    }
}

impl PartialEq for Equipment {
    fn eq(&self, other: &Self) -> bool {
        self.slots == other.slots && self.drop_chances == other.drop_chances
    }
}

impl Equipment {
    /// Empty equipment across all six slots.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the item in `slot`, or `None` when empty.
    pub fn get(&self, slot: EquipmentSlot) -> Option<&ItemStack> {
        self.slots[slot.as_index()].as_ref()
    }

    /// Replaces `slot`, returning the previous stack. Passing an empty [`ItemStack`] clears it.
    pub fn set(&mut self, slot: EquipmentSlot, item: Option<ItemStack>) -> Option<ItemStack> {
        let normalized = item.filter(|stack| !stack.is_empty());
        let idx = slot.as_index();
        if self.slots[idx] != normalized {
            self.dirty |= 1 << idx;
        }
        std::mem::replace(&mut self.slots[idx], normalized)
    }

    /// Clears every slot.
    pub fn clear(&mut self) {
        for (idx, slot) in self.slots.iter_mut().enumerate() {
            if slot.is_some() {
                self.dirty |= 1 << idx;
                *slot = None;
            }
        }
    }

    /// Whether any slot has changed since the last [`Self::take_dirty_wire_slots`] or
    /// [`Self::clear_dirty`].
    pub fn is_dirty(&self) -> bool {
        self.dirty != 0
    }

    /// Clears the dirty bitmask without emitting packets (used after restoring from save NBT).
    pub fn clear_dirty(&mut self) {
        self.dirty = 0;
    }

    /// Drains changed slots as `(wire_slot, ItemStack)` pairs for `SetEquipment`. Cleared slots
    /// emit [`ItemStack::empty`] so clients remove the old item.
    pub fn take_dirty_wire_slots(&mut self) -> Vec<(u8, ItemStack)> {
        if self.dirty == 0 {
            return Vec::new();
        }
        let mask = self.dirty;
        self.dirty = 0;
        let mut out = Vec::new();
        for slot in EquipmentSlot::ALL {
            let idx = slot.as_index();
            if (mask & (1 << idx)) != 0 {
                out.push((
                    slot.as_wire_slot(),
                    self.slots[idx].clone().unwrap_or_default(),
                ));
            }
        }
        out
    }

    /// Whether all six slots are empty.
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(Option::is_none)
    }

    /// Iterates over occupied `(slot, item)` pairs in wire order.
    pub fn iter(&self) -> impl Iterator<Item = (EquipmentSlot, &ItemStack)> + '_ {
        EquipmentSlot::ALL
            .into_iter()
            .filter_map(|slot| self.get(slot).map(|item| (slot, item)))
    }

    /// Converts occupied slots into `(wire_slot, ItemStack)` pairs for `SetEquipment`.
    pub fn to_wire_slots(&self) -> Vec<(u8, ItemStack)> {
        self.iter()
            .map(|(slot, item)| (slot.as_wire_slot(), item.clone()))
            .collect()
    }

    /// Serializes occupied slots to vanilla 26.3 `equipment` NBT compound.
    pub fn to_nbt(&self) -> Compound {
        let mut compound = Compound::new();
        for (slot, item) in self.iter() {
            if let Ok(item_nbt) = item.to_nbt() {
                compound.insert(slot.nbt_key().into(), Tag::Compound(item_nbt));
            }
        }
        compound
    }

    /// Returns the drop chance for `slot` (defaults to [`DEFAULT_DROP_CHANCE`]).
    pub fn drop_chance(&self, slot: EquipmentSlot) -> f32 {
        self.drop_chances[slot.as_index()]
    }

    /// Sets the drop chance for `slot` (finite values only).
    pub fn set_drop_chance(&mut self, slot: EquipmentSlot, chance: f32) {
        if chance.is_finite() {
            self.drop_chances[slot.as_index()] = chance;
        }
    }

    /// Iterates over all `(slot, drop_chance)` pairs.
    pub fn drop_chances_iter(&self) -> impl Iterator<Item = (EquipmentSlot, f32)> + '_ {
        EquipmentSlot::ALL
            .into_iter()
            .map(|slot| (slot, self.drop_chances[slot.as_index()]))
    }

    /// Serializes non-default drop chances to vanilla 26.3 `drop_chances` NBT compound.
    pub fn drop_chances_to_nbt(&self) -> Option<Compound> {
        let mut compound = Compound::new();
        for slot in EquipmentSlot::ALL {
            let chance = self.drop_chances[slot.as_index()];
            if (chance - DEFAULT_DROP_CHANCE).abs() > f32::EPSILON {
                compound.insert(slot.nbt_key().into(), Tag::Float(chance));
            }
        }
        (!compound.is_empty()).then_some(compound)
    }

    /// Restores drop chances from a `drop_chances` NBT compound.
    pub fn drop_chances_from_nbt(&mut self, compound: &Compound) {
        for slot in EquipmentSlot::ALL {
            if let Some(Tag::Float(chance)) = compound.get(slot.nbt_key())
                && chance.is_finite()
            {
                self.drop_chances[slot.as_index()] = *chance;
            }
        }
    }

    /// Restores equipment from an `equipment` NBT compound; malformed slot entries are skipped.
    pub fn from_nbt(compound: &Compound) -> Self {
        let mut equipment = Self::new();
        for slot in EquipmentSlot::ALL {
            if let Some(Tag::Compound(item_tag)) = compound.get(slot.nbt_key())
                && let Ok(stack) = ItemStack::from_nbt(item_tag)
            {
                equipment.set(slot, Some(stack));
            }
        }
        equipment
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_protocol::VarInt;
    use ferrite_protocol::codec::{Decode, Encode};
    use ferrite_protocol::packets::play::SetEquipment;

    #[test]
    fn slot_name_and_index_round_trip() {
        for slot in EquipmentSlot::ALL {
            assert_eq!(EquipmentSlot::from_index(slot.as_wire_slot()), Some(slot));
            assert_eq!(EquipmentSlot::from_name(slot.as_name()), Some(slot));
        }
        assert_eq!(
            EquipmentSlot::from_name("MAIN_HAND"),
            Some(EquipmentSlot::MainHand)
        );
        assert_eq!(EquipmentSlot::from_index(99), None);
        assert_eq!(EquipmentSlot::from_name("TAIL"), None);
    }

    #[test]
    fn set_get_and_wire_packet_encode() {
        let mut eq = Equipment::new();
        assert!(eq.is_empty());

        let sword = ItemStack::from_name("minecraft:iron_sword").unwrap();
        let helm = ItemStack::from_name("minecraft:iron_helmet").unwrap();
        assert_eq!(eq.set(EquipmentSlot::MainHand, Some(sword.clone())), None);
        assert_eq!(eq.set(EquipmentSlot::Head, Some(helm.clone())), None);
        assert!(!eq.is_empty());
        assert_eq!(eq.get(EquipmentSlot::MainHand), Some(&sword));
        assert_eq!(eq.get(EquipmentSlot::Head), Some(&helm));

        let packet = SetEquipment {
            entity_id: VarInt(42),
            slots: eq.to_wire_slots(),
        };
        let mut wire = Vec::new();
        packet.encode(&mut wire);
        let decoded = SetEquipment::<ItemStack>::decode(&mut wire.as_slice()).unwrap();
        assert_eq!(decoded, packet);

        eq.set(EquipmentSlot::MainHand, Some(ItemStack::empty()));
        assert_eq!(eq.get(EquipmentSlot::MainHand), None);
    }

    #[test]
    fn nbt_persistence_round_trips() {
        let mut eq = Equipment::new();
        let bow = ItemStack::from_name("minecraft:bow").unwrap();
        let boots = ItemStack::from_name("minecraft:leather_boots").unwrap();
        eq.set(EquipmentSlot::MainHand, Some(bow));
        eq.set(EquipmentSlot::Feet, Some(boots));

        let nbt = eq.to_nbt();
        let restored = Equipment::from_nbt(&nbt);
        assert_eq!(restored, eq);
    }
}
