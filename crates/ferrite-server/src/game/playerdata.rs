//! Per-player save data so a player who leaves resumes where they were instead of at world spawn.
//!
//! Stored as `<world>/playerdata/<uuid>.json` and written atomically (temp file + rename) when
//! the player leaves. The directory is owned by the world ([`super::WorldSettings::playerdata`]),
//! not by the process: a world without one keeps no player state at all, which is what test
//! worlds use. A process-wide directory made every same-named bot in an integration run resume
//! whichever world happened to save that name first. A vanilla NBT `.dat` compatible format can
//! replace this later without touching callers.

use std::path::{Path, PathBuf};

use ferrite_inventory::PlayerInventory;
use ferrite_item::ItemStack;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub gamemode: i32,
    /// The dimension the player logged out in, as a dimension key. Older saves default to
    /// the overworld.
    #[serde(default = "default_dimension")]
    pub dimension: String,
    /// `(inventory slot, network slot bytes as hex)` for non-empty slots, in vanilla `Inventory`
    /// numbering. The network form keeps every component, including ones the NBT save format
    /// cannot re-encode for the network.
    #[serde(default)]
    pub items: Vec<(u8, String)>,
    /// The selected hotbar slot.
    #[serde(default)]
    pub selected: u8,
    /// Older saves: `(window-0 menu slot, item id, count)` for non-empty slots.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inventory: Vec<(u8, i32, i32)>,
}

/// Window-0 menu slot to `Inventory` slot, for older saves.
fn legacy_slot(menu_slot: u8) -> Option<usize> {
    Some(match menu_slot {
        5 => ferrite_inventory::HEAD,
        6 => ferrite_inventory::CHEST,
        7 => ferrite_inventory::LEGS,
        8 => ferrite_inventory::FEET,
        9..=35 => usize::from(menu_slot),
        36..=44 => usize::from(menu_slot) - 36,
        45 => ferrite_inventory::OFFHAND,
        _ => return None,
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

fn default_dimension() -> String {
    super::Dimension::Overworld.key().to_owned()
}

impl Saved {
    pub fn capture(
        pos: [f64; 3],
        yaw: f32,
        pitch: f32,
        gamemode: i32,
        dimension: super::Dimension,
        inventory: &PlayerInventory,
    ) -> Self {
        let items = inventory
            .slots()
            .iter()
            .enumerate()
            .filter(|(_, stack)| !stack.is_empty())
            .filter_map(|(slot, stack)| match stack.encode_network() {
                Ok(bytes) => Some((slot as u8, hex(&bytes))),
                Err(error) => {
                    tracing::warn!(slot, %error, "not saving an unencodable item stack");
                    None
                }
            })
            .collect();
        Self {
            pos,
            yaw,
            pitch,
            gamemode,
            dimension: dimension.key().to_owned(),
            items,
            selected: inventory.selected_slot() as u8,
            inventory: Vec::new(),
        }
    }

    /// The saved dimension, or the overworld when an older save has none.
    pub fn dimension(&self) -> super::Dimension {
        super::Dimension::from_key(&self.dimension).unwrap_or_default()
    }

    /// Rebuilds the inventory, ignoring unknown slots and unreadable stacks.
    pub fn inventory(&self) -> PlayerInventory {
        let mut inventory = PlayerInventory::new();
        inventory.selected = usize::from(self.selected).min(ferrite_inventory::HOTBAR - 1);
        for &(menu_slot, item_id, count) in &self.inventory {
            if let (Some(slot), Ok(item)) = (legacy_slot(menu_slot), u16::try_from(item_id)) {
                inventory.set(slot, ItemStack::with_count(item, count));
            }
        }
        for (slot, bytes) in &self.items {
            let stack = unhex(bytes).and_then(|bytes| ItemStack::decode_network(&bytes).ok());
            match stack {
                Some(stack) if usize::from(*slot) < ferrite_inventory::SIZE => {
                    inventory.set(usize::from(*slot), stack)
                }
                _ => tracing::warn!(slot, "ignoring an unreadable saved item stack"),
            }
        }
        inventory
    }
}

fn path_in(dir: &Path, uuid: &Uuid) -> PathBuf {
    dir.join(format!("{uuid}.json"))
}

pub fn load_from(dir: &Path, uuid: &Uuid) -> Option<Saved> {
    let bytes = std::fs::read(path_in(dir, uuid)).ok()?;
    match serde_json::from_slice(&bytes) {
        Ok(saved) => Some(saved),
        Err(error) => {
            tracing::warn!(%uuid, %error, "ignoring unreadable player data");
            None
        }
    }
}

pub fn save_to(dir: &Path, uuid: &Uuid, saved: &Saved) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let target = path_in(dir, uuid);
    let temp = target.with_extension("json.tmp");
    std::fs::write(
        &temp,
        serde_json::to_vec_pretty(saved).map_err(std::io::Error::other)?,
    )?;
    std::fs::rename(temp, target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ferrite-playerdata-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn round_trips_position_mode_and_inventory() {
        let dir = temp_dir("roundtrip");
        let uuid = Uuid::from_u128(7);
        let mut inventory = PlayerInventory::new();
        inventory.selected = 4;
        inventory.set(
            0,
            ItemStack::from_name("stone").unwrap().copy_with_count(64),
        );
        let mut sword = ItemStack::from_name("diamond_sword").unwrap();
        sword.components.insert(
            ferrite_item::component_ids::DAMAGE,
            ferrite_item::Component::Damage(12),
        );
        inventory.set(ferrite_inventory::OFFHAND, sword.clone());
        let saved = Saved::capture(
            [12.5, 70.0, -3.25],
            90.0,
            -15.0,
            1,
            crate::game::Dimension::Nether,
            &inventory,
        );
        save_to(&dir, &uuid, &saved).unwrap();
        let loaded = load_from(&dir, &uuid).unwrap();
        assert_eq!(loaded, saved);
        assert_eq!(loaded.inventory(), inventory);
        assert_eq!(loaded.inventory().get(ferrite_inventory::OFFHAND), &sword);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_or_corrupt_data_is_none() {
        let dir = temp_dir("corrupt");
        let uuid = Uuid::from_u128(8);
        assert!(load_from(&dir, &uuid).is_none());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(path_in(&dir, &uuid), b"{ not json").unwrap();
        assert!(load_from(&dir, &uuid).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn older_saves_and_bad_slots_are_converted_or_ignored() {
        let saved: Saved = serde_json::from_str(
            r#"{"pos":[0,0,0],"yaw":0,"pitch":0,"gamemode":0,
                "inventory":[[200,1,1],[36,1,5],[5,1,1],[45,1,2]],
                "items":[[99,"0101"],[3,"zz"]]}"#,
        )
        .unwrap();
        let inventory = saved.inventory();
        assert_eq!(inventory.get(0).count, 5);
        assert_eq!(inventory.get(ferrite_inventory::HEAD).count, 1);
        assert_eq!(inventory.get(ferrite_inventory::OFFHAND).count, 2);
        let filled = inventory.slots().iter().filter(|s| !s.is_empty()).count();
        assert_eq!(filled, 3);
    }
}
