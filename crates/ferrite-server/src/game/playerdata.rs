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

/// What vanilla keeps in a player's data file besides position and inventory: `Health`,
/// `AbsorptionAmount`, `foodLevel`, `foodSaturationLevel`, `foodExhaustionLevel`, `XpLevel`,
/// `XpP` and `XpTotal`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Vitals {
    pub health: f32,
    #[serde(default)]
    pub absorption: f32,
    pub food: i32,
    pub saturation: f32,
    #[serde(default)]
    pub exhaustion: f32,
    pub xp_level: i32,
    pub xp_progress: f32,
    pub xp_total: i32,
}

impl Vitals {
    /// The values clamped to what the game allows, or `None` for a save that is dead (health
    /// zero or less) or not a number: a dead save is not brought back as a corpse.
    pub fn sanitized(&self, max_health: f32) -> Option<Self> {
        if !self.health.is_finite() || self.health <= 0.0 || !max_health.is_finite() {
            return None;
        }
        let food = self.food.clamp(0, 20);
        let finite = |value: f32, max: f32| {
            if value.is_finite() {
                value.clamp(0.0, max)
            } else {
                0.0
            }
        };
        Some(Self {
            health: self.health.min(max_health),
            absorption: finite(self.absorption, 1024.0),
            food,
            saturation: finite(self.saturation, food as f32),
            exhaustion: finite(self.exhaustion, 40.0),
            xp_level: self.xp_level.clamp(0, 21_863),
            xp_progress: finite(self.xp_progress, 1.0),
            xp_total: self.xp_total.max(0),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    /// Health, hunger and experience; absent in older saves, which keep the defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vitals: Option<Vitals>,
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub gamemode: i32,
    /// The End credits were already seen, so a first trip home does not replay them.
    #[serde(default)]
    pub seen_credits: bool,
    /// Vanilla `XpSeed`: the enchanting table's offers stay put until the player enchants.
    #[serde(default)]
    pub enchant_seed: Option<i32>,
    /// The dimension the player logged out in, as a dimension key. Older saves default to
    /// the overworld.
    #[serde(default = "default_dimension")]
    pub dimension: String,
    /// The plugin runtime world the player logged out in, so a rejoin lands there again. Absent
    /// for the vanilla dimensions and in older saves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world: Option<Uuid>,
    /// `(inventory slot, network slot bytes as hex)` for non-empty slots, in vanilla `Inventory`
    /// numbering. The network form keeps every component, including ones the NBT save format
    /// cannot re-encode for the network.
    #[serde(default)]
    pub items: Vec<(u8, String)>,
    /// The selected hotbar slot.
    #[serde(default)]
    pub selected: u8,
    /// The entity the player was riding when they left (vanilla keeps it as `RootVehicle`). The
    /// vehicle stays in its chunk; a rejoin seats the player on it again if it still exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vehicle: Option<Uuid>,
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

/// Whether the End credits must not be shown: either this world disables them, or this player
/// already saw them.
pub fn credits_seen(saved: Option<&Saved>, disabled: bool) -> bool {
    disabled || saved.is_some_and(|saved| saved.seen_credits)
}

impl Saved {
    pub fn capture(
        pos: [f64; 3],
        yaw: f32,
        pitch: f32,
        gamemode: i32,
        dimension: super::Dimension,
        seen_credits: bool,
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
            seen_credits,
            enchant_seed: None,
            dimension: dimension.key().to_owned(),
            world: None,
            vehicle: None,
            items,
            selected: inventory.selected_slot() as u8,
            inventory: Vec::new(),
            vitals: None,
        }
    }

    /// Records health, hunger and experience.
    #[must_use]
    pub fn with_vitals(mut self, vitals: Vitals) -> Self {
        self.vitals = Some(vitals);
        self
    }

    /// Records the plugin runtime world the player is in, so a rejoin restores it.
    #[must_use]
    pub fn in_world(mut self, world: Option<Uuid>) -> Self {
        self.world = world;
        self
    }

    /// Records the player's enchantment seed.
    #[must_use]
    pub fn with_enchant_seed(mut self, seed: i32) -> Self {
        self.enchant_seed = Some(seed);
        self
    }

    /// Records the vehicle the player is riding, so a rejoin seats them on it again.
    #[must_use]
    pub fn riding(mut self, vehicle: Option<Uuid>) -> Self {
        self.vehicle = vehicle;
        self
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

/// When the player's data file was first created, which is when they first joined.
pub fn first_played(dir: &Path, uuid: &Uuid) -> Option<std::time::SystemTime> {
    let meta = std::fs::metadata(path_in(dir, uuid)).ok()?;
    meta.created().or_else(|_| meta.modified()).ok()
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
    fn first_played_is_the_creation_time_of_the_saved_file() {
        let dir = temp_dir("first-played");
        let uuid = Uuid::from_u128(8);
        assert_eq!(first_played(&dir, &uuid), None);
        let saved = Saved::capture(
            [0.0, 64.0, 0.0],
            0.0,
            0.0,
            0,
            crate::game::Dimension::Overworld,
            false,
            &PlayerInventory::new(),
        );
        save_to(&dir, &uuid, &saved).unwrap();
        let first = first_played(&dir, &uuid).unwrap();
        let age = std::time::SystemTime::now()
            .duration_since(first)
            .unwrap_or_default();
        assert!(age < std::time::Duration::from_secs(60));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// `Player#loadData` and joining restore health, hunger and experience; older saves and
    /// dead or corrupt ones leave the defaults alone.
    #[test]
    fn vitals_round_trip_and_bad_ones_are_not_applied() {
        let dir = temp_dir("vitals");
        let uuid = Uuid::from_u128(12);
        let base = Saved::capture(
            [0.0, 64.0, 0.0],
            0.0,
            0.0,
            0,
            crate::game::Dimension::Overworld,
            false,
            &PlayerInventory::new(),
        );
        assert_eq!(base.vitals, None);
        let vitals = Vitals {
            health: 7.5,
            absorption: 2.0,
            food: 9,
            saturation: 3.0,
            exhaustion: 1.5,
            xp_level: 30,
            xp_progress: 0.25,
            xp_total: 1395,
        };
        save_to(&dir, &uuid, &base.clone().with_vitals(vitals.clone())).unwrap();
        let loaded = load_from(&dir, &uuid).unwrap().vitals.unwrap();
        assert_eq!(loaded, vitals);
        assert_eq!(loaded.sanitized(20.0), Some(vitals.clone()));
        // Too much of everything is clamped; the health cannot exceed the maximum.
        let wild = Vitals {
            health: 50.0,
            food: 99,
            saturation: 99.0,
            exhaustion: 99.0,
            xp_progress: 4.0,
            xp_level: -3,
            xp_total: -1,
            ..vitals.clone()
        };
        let fixed = wild.sanitized(20.0).unwrap();
        assert_eq!(
            (fixed.health, fixed.food, fixed.saturation),
            (20.0, 20, 20.0)
        );
        assert_eq!((fixed.exhaustion, fixed.xp_progress), (40.0, 1.0));
        assert_eq!((fixed.xp_level, fixed.xp_total), (0, 0));
        for health in [0.0, -1.0, f32::NAN] {
            assert_eq!(
                Vitals {
                    health,
                    ..vitals.clone()
                }
                .sanitized(20.0),
                None,
                "{health}"
            );
        }
        // An older save has no vitals key at all.
        let json = serde_json::to_value(&base).unwrap();
        assert!(json.get("vitals").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn enchant_seed_round_trips_and_older_saves_have_none() {
        let dir = temp_dir("enchant-seed");
        let uuid = Uuid::from_u128(11);
        let base = Saved::capture(
            [0.0, 64.0, 0.0],
            0.0,
            0.0,
            0,
            crate::game::Dimension::Overworld,
            false,
            &PlayerInventory::new(),
        );
        save_to(&dir, &uuid, &base.clone().with_enchant_seed(-12345)).unwrap();
        assert_eq!(load_from(&dir, &uuid).unwrap().enchant_seed, Some(-12345));
        let mut json: serde_json::Value = serde_json::to_value(&base).unwrap();
        json.as_object_mut().unwrap().remove("enchant_seed");
        let older: Saved = serde_json::from_value(json).unwrap();
        assert_eq!(older.enchant_seed, None);
        let _ = std::fs::remove_dir_all(dir);
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
            true,
            &inventory,
        )
        .in_world(Some(Uuid::from_u128(99)))
        .riding(Some(Uuid::from_u128(0x7e1d)));
        save_to(&dir, &uuid, &saved).unwrap();
        let loaded = load_from(&dir, &uuid).unwrap();
        assert_eq!(loaded, saved);
        assert_eq!(loaded.vehicle, Some(Uuid::from_u128(0x7e1d)));
        assert!(loaded.seen_credits);
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
        assert!(!saved.seen_credits, "older saves have not seen the credits");
        assert!(credits_seen(Some(&saved), true));
        assert!(!credits_seen(Some(&saved), false));
        assert!(credits_seen(None, true));
        let inventory = saved.inventory();
        assert_eq!(inventory.get(0).count, 5);
        assert_eq!(inventory.get(ferrite_inventory::HEAD).count, 1);
        assert_eq!(inventory.get(ferrite_inventory::OFFHAND).count, 2);
        let filled = inventory.slots().iter().filter(|s| !s.is_empty()).count();
        assert_eq!(filled, 3);
    }
}
