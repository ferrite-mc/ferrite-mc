//! The cartography table's view of the saved-map store: lookups for the result preview and the
//! creation of the new map when a zoomed-out or locked result is taken.

use std::sync::Arc;

use ferrite_inventory::{MapAccess, MapFinishError, MapInfo};
use ferrite_item::{
    ItemStack, MapPostProcessing, map_id, map_post_processing, set_map_id, set_map_post_processing,
};

use super::{
    map::{MapStore, store_for},
    region::Player,
};

/// [`MapAccess`] over a shared [`MapStore`].
pub struct StoreMaps(pub Arc<MapStore>);

impl MapAccess for StoreMaps {
    fn info(&self, id: i32) -> Option<MapInfo> {
        match self.0.get(id) {
            Ok(Some(map)) => Some(MapInfo {
                scale: map.scale,
                locked: map.locked,
            }),
            Ok(None) => None,
            Err(error) => {
                tracing::warn!("cartography table cannot read map {id}: {error}");
                None
            }
        }
    }

    fn finish(&self, stack: &mut ItemStack) -> Result<Option<i32>, MapFinishError> {
        let Some(work) = map_post_processing(stack) else {
            return Ok(None);
        };
        let source = map_id(stack).ok_or(MapFinishError)?;
        let map = match self.0.get(source) {
            Ok(Some(map)) => map,
            Ok(None) => return Err(MapFinishError),
            Err(error) => {
                tracing::warn!("cartography table cannot read map {source}: {error}");
                return Err(MapFinishError);
            }
        };
        let derived = match work {
            MapPostProcessing::Scale => map.scaled(),
            MapPostProcessing::Lock => map.locked_copy(),
        };
        let id = self.0.allocate(derived).map_err(|error| {
            tracing::warn!("cartography table cannot create a map: {error}");
            MapFinishError
        })?;
        set_map_id(stack, id);
        set_map_post_processing(stack, None);
        Ok(Some(id))
    }

    fn discard(&self, id: i32) {
        self.0.release(id);
    }
}

impl Player {
    /// The saved-map store of the world this player is in; `None` for a world with no
    /// directory (the in-memory test world), where cartography tables offer no results.
    pub(super) fn map_access(&self) -> Option<StoreMaps> {
        let root = self.source.as_ref()?.path()?;
        match store_for(root, ferrite_command::gamerule::DATA_VERSION) {
            Ok(store) => Some(StoreMaps(store)),
            Err(error) => {
                tracing::warn!("saved maps are unavailable: {error}");
                None
            }
        }
    }
}

impl Player {
    /// Sends the saved data of the filled maps in the player's hands (vanilla `MapItem.update`
    /// sending `ClientboundMapItemData`). A map is sent when first held and again whenever its
    /// saved data was replaced. Banner and player markers are not sent yet.
    pub(super) fn send_held_maps(&mut self) {
        // Read the ids by reference: this runs every tick for every player, and cloning the
        // stacks (components included) just to look at one component allocated for nothing.
        let held = [
            map_id(self.inventory.get(self.inventory.selected_slot())),
            map_id(self.inventory.get(ferrite_inventory::OFFHAND)),
        ];
        for id in held.into_iter().flatten() {
            let Some(access) = self.map_access() else {
                return;
            };
            let Ok(Some(map)) = access.0.get(id) else {
                continue;
            };
            let version = Arc::as_ptr(&map) as usize;
            if self.maps_sent.get(&id) == Some(&version) {
                continue;
            }
            self.maps_sent.insert(id, version);
            super::region::send(self, &map_packet(id, &map));
        }
    }
}

/// The full-map packet for `map`.
pub(super) fn map_packet(
    id: i32,
    map: &super::map::SavedMap,
) -> ferrite_protocol::packets::play::MapItemData {
    ferrite_protocol::packets::play::MapItemData {
        map_id: ferrite_protocol::VarInt(id),
        scale: map.scale,
        locked: map.locked,
        decorations: Some(Vec::new()),
        width: 128,
        height: 128,
        x: 0,
        z: 0,
        colors: map.colors.clone(),
    }
}

#[cfg(test)]
mod tests {
    use ferrite_inventory::{ClickInput, Clicker, Menu, NoCrafting, PlayerInventory};

    use super::*;
    use crate::game::map::SavedMap;

    fn store() -> (tempfile::TempDir, Arc<MapStore>) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(MapStore::open(dir.path(), 4000).unwrap());
        (dir, store)
    }

    fn filled(id: i32) -> ItemStack {
        let mut stack = ItemStack::from_name("minecraft:filled_map").unwrap();
        set_map_id(&mut stack, id);
        stack
    }

    fn item(name: &str, count: i32) -> ItemStack {
        ItemStack::from_name(name).unwrap().copy_with_count(count)
    }

    /// A table with `map` and `addition` placed, taken with a plain click.
    fn take(
        store: &Arc<MapStore>,
        map: ItemStack,
        addition: ItemStack,
    ) -> (PlayerInventory, ItemStack) {
        let maps = StoreMaps(store.clone());
        let mut inventory = PlayerInventory::new();
        let mut menu = Menu::cartography(1);
        menu.set_slot_with_maps(&mut inventory, &NoCrafting, &maps, 0, map);
        menu.set_slot_with_maps(&mut inventory, &NoCrafting, &maps, 1, addition);
        let mut p = Clicker::new(&mut inventory, false, &NoCrafting).with_maps(&maps);
        menu.clicked(&mut p, 2, 0, ClickInput::Pickup);
        let taken = menu.carried().clone();
        (inventory, taken)
    }

    fn explored() -> SavedMap {
        let mut map = SavedMap::fresh(300.0, 300.0, 1, true, false, "ferrite:arena");
        map.colors[10] = 33;
        map
    }

    #[test]
    fn the_map_packet_carries_the_saved_scale_lock_and_pixels() {
        let mut map = explored();
        map.locked = true;
        let packet = map_packet(9, &map);
        assert_eq!(packet.map_id.0, 9);
        assert_eq!((packet.scale, packet.locked), (1, true));
        assert_eq!(
            (packet.width, packet.height, packet.x, packet.z),
            (128, 128, 0, 0)
        );
        assert_eq!(packet.colors.len(), 128 * 128);
        assert_eq!(packet.colors[10], 33);
    }

    #[test]
    fn zooming_out_creates_a_blank_larger_map_and_keeps_the_source() {
        let (_dir, store) = store();
        let source = store.allocate(explored()).unwrap();
        let (_, taken) = take(&store, filled(source), item("minecraft:paper", 1));
        let new = map_id(&taken).unwrap();
        assert_ne!(new, source);
        let zoomed = store.get(new).unwrap().unwrap();
        assert_eq!(zoomed.scale, 2);
        assert!(zoomed.colors.iter().all(|&c| c == 0), "zoom-out is blank");
        assert_eq!(zoomed.dimension, "ferrite:arena", "the world is kept");
        let original = store.get(source).unwrap().unwrap();
        assert_eq!(original.scale, 1);
        assert_eq!(original.colors[10], 33, "the source keeps its pixels");
        assert!(!original.locked);
    }

    #[test]
    fn locking_copies_the_pixels_into_a_new_locked_map() {
        let (_dir, store) = store();
        let source = store.allocate(explored()).unwrap();
        let (_, taken) = take(&store, filled(source), item("minecraft:glass_pane", 1));
        let new = map_id(&taken).unwrap();
        assert_ne!(new, source);
        let locked = store.get(new).unwrap().unwrap();
        assert!(locked.locked);
        assert_eq!(locked.colors[10], 33);
        assert_eq!(locked.scale, 1);
        assert!(!store.get(source).unwrap().unwrap().locked);
    }

    #[test]
    fn cloning_shares_the_id_and_creates_no_file() {
        let (dir, store) = store();
        let source = store.allocate(explored()).unwrap();
        let (_, taken) = take(&store, filled(source), item("minecraft:map", 1));
        assert_eq!(taken.count, 2);
        assert_eq!(map_id(&taken), Some(source));
        assert_eq!(store.last_id(), source);
        assert!(!dir.path().join("data/minecraft/maps/1.dat").exists());
    }

    #[test]
    fn a_new_map_survives_a_restart() {
        let (dir, store) = store();
        let source = store.allocate(explored()).unwrap();
        let (_, taken) = take(&store, filled(source), item("minecraft:paper", 1));
        let new = map_id(&taken).unwrap();
        drop(store);
        let reopened = Arc::new(MapStore::open(dir.path(), 4000).unwrap());
        assert_eq!(reopened.last_id(), new);
        assert_eq!(reopened.get(new).unwrap().unwrap().scale, 2);
        // The next allocation never reuses a persisted id.
        let (_, again) = take(&reopened, filled(source), item("minecraft:paper", 1));
        assert_eq!(map_id(&again), Some(new + 1));
    }

    #[test]
    fn a_damaged_source_map_gives_no_result_and_is_not_overwritten() {
        let (dir, store) = store();
        let source = store.allocate(explored()).unwrap();
        let path = dir.path().join(format!("data/minecraft/maps/{source}.dat"));
        std::fs::write(&path, b"broken").unwrap();
        let fresh = Arc::new(MapStore::open(dir.path(), 4000).unwrap());
        let (inventory, taken) = take(&fresh, filled(source), item("minecraft:paper", 1));
        assert!(taken.is_empty(), "no result for an unreadable map");
        assert_eq!(std::fs::read(&path).unwrap(), b"broken");
        assert_eq!(fresh.last_id(), source, "nothing was allocated");
        drop(inventory);
    }

    #[test]
    fn shift_click_with_no_room_releases_the_new_map() {
        let (_dir, store) = store();
        let source = store.allocate(explored()).unwrap();
        let maps = StoreMaps(store.clone());
        let mut inventory = PlayerInventory::new();
        for slot in 0..ferrite_inventory::SIZE {
            inventory.set(slot, item("minecraft:stick", 64));
        }
        let mut menu = Menu::cartography(1);
        menu.set_slot_with_maps(&mut inventory, &NoCrafting, &maps, 0, filled(source));
        menu.set_slot_with_maps(
            &mut inventory,
            &NoCrafting,
            &maps,
            1,
            item("minecraft:paper", 1),
        );
        let mut p = Clicker::new(&mut inventory, false, &NoCrafting).with_maps(&maps);
        menu.clicked(&mut p, 2, 0, ClickInput::QuickMove);
        assert_eq!(store.last_id(), source, "the id was handed back");
        assert!(!menu.slot(&inventory, 0).is_empty(), "the source map stays");
    }

    #[test]
    fn two_players_taking_at_once_get_distinct_maps() {
        let (_dir, store) = store();
        let source = store.allocate(explored()).unwrap();
        let handles: Vec<_> = (0..6)
            .map(|_| {
                let store = store.clone();
                std::thread::spawn(move || {
                    let (_, taken) = take(&store, filled(source), item("minecraft:glass_pane", 1));
                    map_id(&taken).unwrap()
                })
            })
            .collect();
        let mut ids: Vec<i32> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 6);
    }
}
