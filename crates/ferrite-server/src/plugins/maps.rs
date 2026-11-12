//! Host operations behind Bukkit's `Server#createMap` and `MapView` setters: allocate a saved
//! map in the shared store and keep its settings in step. Both are answered synchronously by the
//! store's own lock, never by a region.
//!
//! * `Server.allocateMap(String,int,int,int,boolean,boolean)` — dimension key, Bukkit scale
//!   ordinal (0 closest .. 4 farthest), centre x, centre z, tracking, unlimited tracking.
//!   Replies with the new map id.
//! * `Server.updateMap(int,String,int,int,int,boolean,boolean,boolean)` — id, dimension key,
//!   scale, centre x, centre z, tracking, unlimited tracking, locked. Replies with an empty
//!   string. Pixels, banners and frames of the saved map are kept.

use std::sync::Arc;

use ferrite_plugin::HostCall;

use crate::game::map::{MAP_COLORS, MAX_SCALE, MapStore, SavedMap};

/// The reply for a map host call on the primary store, or `None` for any other call.
pub fn answer(call: &HostCall) -> Option<String> {
    if call.owner != "Server" || !matches!(call.name(), "allocateMap" | "updateMap") {
        return None;
    }
    let store = crate::game::primary_map_store()?;
    answer_in(&store, call)
}

fn answer_in(store: &Arc<MapStore>, call: &HostCall) -> Option<String> {
    let flag = |text: &String| match text.as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    };
    let scale = |text: &String| text.parse::<u8>().ok().filter(|s| *s <= MAX_SCALE);
    match call.method.as_str() {
        "allocateMap(String,int,int,int,boolean,boolean)" => {
            let [dimension, scale_text, x, z, tracking, unlimited] = call.args.as_slice() else {
                return None;
            };
            let mut map = SavedMap::fresh(0.0, 0.0, scale(scale_text)?, true, false, dimension);
            // Bukkit sets the centre exactly; only vanilla map items snap it to the grid.
            map.x_center = x.parse().ok()?;
            map.z_center = z.parse().ok()?;
            map.tracking_position = flag(tracking)?;
            map.unlimited_tracking = flag(unlimited)?;
            debug_assert_eq!(map.colors.len(), MAP_COLORS);
            store.allocate(map).ok().map(|id| id.to_string())
        }
        "updateMap(int,String,int,int,int,boolean,boolean,boolean)" => {
            let [id, dimension, scale_text, x, z, tracking, unlimited, locked] =
                call.args.as_slice()
            else {
                return None;
            };
            let id: i32 = id.parse().ok()?;
            let mut map = (*store.get(id).ok()??).clone();
            map.dimension = dimension.clone();
            map.scale = scale(scale_text)?;
            map.x_center = x.parse().ok()?;
            map.z_center = z.parse().ok()?;
            map.tracking_position = flag(tracking)?;
            map.unlimited_tracking = flag(unlimited)?;
            map.locked = flag(locked)?;
            store.save(id, map).ok()?;
            Some(String::new())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(method: &str, args: &[&str]) -> HostCall {
        HostCall {
            owner: "Server".into(),
            subject: "server".into(),
            method: method.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    fn store() -> (tempfile::TempDir, Arc<MapStore>) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(MapStore::open(dir.path(), 4000).unwrap());
        (dir, store)
    }

    const ALLOCATE: &str = "allocateMap(String,int,int,int,boolean,boolean)";
    const UPDATE: &str = "updateMap(int,String,int,int,int,boolean,boolean,boolean)";

    #[test]
    fn allocating_creates_a_persisted_map_with_the_requested_settings() {
        let (dir, store) = store();
        let reply = answer_in(
            &store,
            &call(
                ALLOCATE,
                &["ferrite:arena", "3", "-70", "200", "true", "false"],
            ),
        );
        assert_eq!(reply.as_deref(), Some("0"));
        let reopened = MapStore::open(dir.path(), 4000).unwrap();
        let map = reopened.get(0).unwrap().unwrap();
        assert_eq!(map.dimension, "ferrite:arena");
        assert_eq!((map.scale, map.x_center, map.z_center), (3, -70, 200));
        assert!(map.tracking_position && !map.unlimited_tracking && !map.locked);
        let second = answer_in(
            &store,
            &call(
                ALLOCATE,
                &["minecraft:overworld", "0", "0", "0", "true", "true"],
            ),
        );
        assert_eq!(second.as_deref(), Some("1"));
    }

    #[test]
    fn updating_changes_settings_and_keeps_the_pixels() {
        let (_dir, store) = store();
        let mut map = SavedMap::fresh(0.0, 0.0, 0, true, false, "minecraft:overworld");
        map.colors[7] = 12;
        let id = store.allocate(map).unwrap();
        let reply = answer_in(
            &store,
            &call(
                UPDATE,
                &[
                    &id.to_string(),
                    "minecraft:the_end",
                    "4",
                    "10",
                    "20",
                    "false",
                    "true",
                    "true",
                ],
            ),
        );
        assert_eq!(reply.as_deref(), Some(""));
        let saved = store.get(id).unwrap().unwrap();
        assert_eq!(saved.dimension, "minecraft:the_end");
        assert_eq!((saved.scale, saved.x_center, saved.z_center), (4, 10, 20));
        assert!(!saved.tracking_position && saved.unlimited_tracking && saved.locked);
        assert_eq!(saved.colors[7], 12);
    }

    #[test]
    fn bad_arguments_and_unknown_maps_are_not_answered() {
        let (_dir, store) = store();
        for args in [
            ["d", "9", "0", "0", "true", "false"],
            ["d", "x", "0", "0", "true", "false"],
            ["d", "1", "q", "0", "true", "false"],
            ["d", "1", "0", "0", "maybe", "false"],
        ] {
            assert_eq!(answer_in(&store, &call(ALLOCATE, &args)), None, "{args:?}");
        }
        assert_eq!(answer_in(&store, &call(ALLOCATE, &["d", "1"])), None);
        assert_eq!(store.last_id(), -1, "nothing was created");
        let unknown = call(UPDATE, &["5", "d", "1", "0", "0", "true", "false", "false"]);
        assert_eq!(answer_in(&store, &unknown), None);
        let other = call("getMap(int)", &["1"]);
        assert_eq!(answer(&other), None);
    }
}
