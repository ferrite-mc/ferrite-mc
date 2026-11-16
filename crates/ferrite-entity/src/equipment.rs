//! Generic entity equipment state and NBT persistence.
//!
//! Covers the six standard vanilla `EquipmentSlot` positions (main hand, off hand, feet, legs,
//! chest, head) for any entity or player. Empty slots are `None` (wire representation omits them
//! or encodes [`ItemStack::empty`]).

use ferrite_item::ItemStack;
use ferrite_nbt::{Compound, Tag};

/// Number of standard vanilla equipment slots.
pub const SLOT_COUNT: usize = 6;

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
}

impl EquipmentSlot {
    /// All six slots in vanilla wire order.
    pub const ALL: [Self; SLOT_COUNT] = [
        Self::MainHand,
        Self::OffHand,
        Self::Feet,
        Self::Legs,
        Self::Chest,
        Self::Head,
    ];

    /// Parses a vanilla wire slot index (`0..=5`).
    pub fn from_index(index: u8) -> Option<Self> {
        match index {
            0 => Some(Self::MainHand),
            1 => Some(Self::OffHand),
            2 => Some(Self::Feet),
            3 => Some(Self::Legs),
            4 => Some(Self::Chest),
            5 => Some(Self::Head),
            _ => None,
        }
    }

    /// Parses a Bukkit/vanilla slot name case-insensitively (`HAND`/`MAIN_HAND`, `OFF_HAND`,
    /// `FEET`, `LEGS`, `CHEST`, `HEAD`).
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_uppercase().as_str() {
            "HAND" | "MAIN_HAND" | "MAINHAND" => Some(Self::MainHand),
            "OFF_HAND" | "OFFHAND" => Some(Self::OffHand),
            "FEET" | "BOOTS" => Some(Self::Feet),
            "LEGS" | "LEGGINGS" => Some(Self::Legs),
            "CHEST" | "CHESTPLATE" => Some(Self::Chest),
            "HEAD" | "HELMET" => Some(Self::Head),
            _ => None,
        }
    }

    /// Dense array index (`0..6`).
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
        }
    }

    /// Vanilla 26.3 NBT compound key inside `equipment`.
    pub const fn nbt_key(self) -> &'static str {
        match self {
            Self::MainHand => "mainhand",
            Self::OffHand => "offhand",
            Self::Feet => "feet",
            Self::Legs => "legs",
            Self::Chest => "chest",
            Self::Head => "head",
        }
    }
}

/// Six-slot entity equipment component, cloned across region split/merge and persisted as NBT.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Equipment {
    slots: [Option<ItemStack>; SLOT_COUNT],
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
        std::mem::replace(&mut self.slots[slot.as_index()], normalized)
    }

    /// Clears every slot.
    pub fn clear(&mut self) {
        self.slots = Default::default();
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
