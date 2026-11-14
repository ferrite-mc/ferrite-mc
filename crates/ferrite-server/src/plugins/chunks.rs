//! Which chunks the server keeps loaded (a player, the spawn or a plugin holds them), for
//! `World#isChunkLoaded`, `World#getLoadedChunks`, `World#loadChunk`, `World#unloadChunk`,
//! `World#isChunkInUse` and the plugin chunk tickets/forceload calls.
//!
//! The game loop marks a chunk when its first player holder arrives and unmarks it when the last
//! one leaves. A plugin load or ticket also keeps the chunk in this set, and only the matching
//! unload/removal drops it, so a plugin-funded chunk is never evicted just because no player is
//! near it. Every world is tracked; runtime worlds also have a real chunk source the view loads.

use ferrite_plugin::{HOST_ITEM_SEPARATOR, HostCall};
use std::{
    collections::{BTreeSet, HashMap},
    path::PathBuf,
    sync::{Mutex, OnceLock},
};
use uuid::Uuid;

fn loaded() -> &'static Mutex<BTreeSet<(i32, i32)>> {
    static LOADED: OnceLock<Mutex<BTreeSet<(i32, i32)>>> = OnceLock::new();
    LOADED.get_or_init(Default::default)
}

/// Chunks a plugin loaded, force-loaded or tickets, per world. The game loop never clears these;
/// only `World#unloadChunk` and the matching ticket/forceload removal do.
fn plugin_chunks() -> &'static Mutex<HashMap<Uuid, BTreeSet<(i32, i32)>>> {
    static PLUGIN: OnceLock<Mutex<HashMap<Uuid, BTreeSet<(i32, i32)>>>> = OnceLock::new();
    PLUGIN.get_or_init(Default::default)
}

fn paths() -> &'static Mutex<HashMap<Uuid, PathBuf>> {
    static PATHS: OnceLock<Mutex<HashMap<Uuid, PathBuf>>> = OnceLock::new();
    PATHS.get_or_init(Default::default)
}

fn world_dir() -> &'static Mutex<Option<PathBuf>> {
    static DIR: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    DIR.get_or_init(Default::default)
}

/// The world directory chunks are saved in (`None` for the in-memory world).
pub fn set_world_dir(path: Option<PathBuf>) {
    *world_dir().lock().unwrap() = path.clone();
    match path {
        Some(path) => {
            set_world_path(ferrite_plugin::default_world_id(), path);
        }
        None => {
            remove_world_path(ferrite_plugin::default_world_id());
        }
    }
}

/// The directory a Bukkit world saves into. The overworld is the world root; the vanilla nether
/// and end live in `DIM-1`/`DIM1` under it. Runtime worlds are registered when they are opened.
pub fn set_world_path(world: Uuid, path: PathBuf) {
    paths().lock().unwrap().insert(world, path);
}

pub fn remove_world_path(world: Uuid) {
    paths().lock().unwrap().remove(&world);
}

fn path_of(world: Uuid) -> Option<PathBuf> {
    paths().lock().unwrap().get(&world).cloned().or_else(|| {
        (world == ferrite_plugin::default_world_id())
            .then(|| world_dir().lock().unwrap().clone())
            .flatten()
    })
}

/// Whether the overworld chunk is saved on disk (vanilla `World#isChunkGenerated`).
fn saved_on_disk(x: i32, z: i32) -> bool {
    let Some(dir) = world_dir().lock().unwrap().clone() else {
        return false;
    };
    let (rx, rz) = (x.div_euclid(32), z.div_euclid(32));
    let file = dir
        .join(crate::game::Dimension::Overworld.region_dir())
        .join(format!("r.{rx}.{rz}.mca"));
    file.is_file()
        && ferrite_world::RegionFile::open(&file, rx, rz)
            .and_then(|region| region.read_chunk(x, z))
            .is_ok_and(|chunk| chunk.is_some())
}

/// The game loop's player/spawn hold on a chunk.
pub fn set_loaded(x: i32, z: i32, is_loaded: bool) {
    let mut set = loaded().lock().unwrap();
    if is_loaded {
        set.insert((x, z));
    } else {
        set.remove(&(x, z));
    }
}

/// A plugin held this chunk with a ticket or a force-load. The chunk is loaded through the live
/// world view so its data is readable immediately, and it stays in the loaded set until released.
pub fn hold(world: Uuid, x: i32, z: i32, held: bool) {
    if held {
        plugin_chunks()
            .lock()
            .unwrap()
            .entry(world)
            .or_default()
            .insert((x, z));
        if let Some(view) = ferrite_plugin::world_view() {
            view.load_chunk(world, x, z);
        }
    } else if let Some(chunks) = plugin_chunks().lock().unwrap().get_mut(&world) {
        chunks.remove(&(x, z));
    }
}

/// Records a chunk a plugin loaded with `World#loadChunk` without holding a ticket. It stays
/// visible to `getLoadedChunks` until it is unloaded again.
pub fn mark_loaded(world: Uuid, x: i32, z: i32) {
    plugin_chunks()
        .lock()
        .unwrap()
        .entry(world)
        .or_default()
        .insert((x, z));
}

fn plugin_has(world: Uuid, x: i32, z: i32) -> bool {
    plugin_chunks()
        .lock()
        .unwrap()
        .get(&world)
        .is_some_and(|chunks| chunks.contains(&(x, z)))
}

fn plugin_coordinates(world: Uuid) -> Vec<(i32, i32)> {
    plugin_chunks()
        .lock()
        .unwrap()
        .get(&world)
        .map(|chunks| chunks.iter().copied().collect())
        .unwrap_or_default()
}

fn is_default_world(subject: &str) -> bool {
    uuid::Uuid::parse_str(subject).ok() == Some(ferrite_plugin::default_world_id())
}

/// `World#refreshChunk(x, z)` on the default world.
pub fn refresh_command(call: &HostCall) -> Option<ferrite_plugin::PlayerCommand> {
    if call.owner != "World"
        || !is_default_world(&call.subject)
        || call.method != "refreshChunk(int,int)"
    {
        return None;
    }
    Some(ferrite_plugin::PlayerCommand::RefreshChunk {
        x: call.args.first()?.parse().ok()?,
        z: call.args.get(1)?.parse().ok()?,
    })
}

fn coordinates(world: Uuid, is_default: bool) -> Vec<(i32, i32)> {
    let mut out: BTreeSet<(i32, i32)> = plugin_coordinates(world).into_iter().collect();
    if is_default {
        out.extend(loaded().lock().unwrap().iter().copied());
    } else if let Some(view) = ferrite_plugin::world_view()
        && let Some(chunks) = view.loaded_chunks(world)
    {
        out.extend(chunks);
    }
    out.into_iter().collect()
}

fn is_loaded(world: Uuid, x: i32, z: i32, is_default: bool) -> bool {
    plugin_has(world, x, z)
        || (is_default && loaded().lock().unwrap().contains(&(x, z)))
        || (!is_default
            && ferrite_plugin::world_view()
                .and_then(|view| view.chunk_loaded(world, x, z))
                .unwrap_or(false))
}

/// The reply to a chunk or world-path query, or `None` for anything else.
pub fn answer(call: &HostCall) -> Option<String> {
    if call.owner != "World" {
        return None;
    }
    let world = uuid::Uuid::parse_str(&call.subject).ok()?;
    let is_default = world == ferrite_plugin::default_world_id();
    match (call.method.as_str(), call.args.as_slice()) {
        // The default world's loaded set is tracked here; other worlds answer from their source.
        ("isChunkLoaded(int,int)", [x, z]) => {
            let (x, z) = (x.parse().ok()?, z.parse().ok()?);
            Some(is_loaded(world, x, z, is_default).to_string())
        }
        ("isChunkInUse(int,int)", [x, z]) => {
            let (x, z) = (x.parse().ok()?, z.parse().ok()?);
            Some(is_loaded(world, x, z, is_default).to_string())
        }
        ("isChunkGenerated(int,int)", [x, z]) if is_default => {
            let (x, z) = (x.parse().ok()?, z.parse().ok()?);
            let generated = is_loaded(world, x, z, true) || saved_on_disk(x, z);
            Some(generated.to_string())
        }
        // `World#loadChunk(x, z)`: reading the chunk through the live view generates it when it
        // does not exist and makes it resident. Remembering it keeps `getLoadedChunks` honest.
        ("loadChunk(int,int)", [x, z]) => {
            let (x, z) = (x.parse().ok()?, z.parse().ok()?);
            let loaded = ferrite_plugin::world_view()
                .and_then(|view| view.load_chunk(world, x, z))
                .unwrap_or(false);
            if loaded {
                mark_loaded(world, x, z);
            }
            Some(loaded.to_string())
        }
        // `World#unloadChunk` / `unloadChunkRequest`: drop the plugin's hold. A chunk a player (or
        // the spawn) still holds cannot be unloaded; a source that keeps the column resident
        // cannot be forcibly evicted through this API either, so both answer `false`.
        ("unloadChunk(int,int)", [x, z]) => {
            let (x, z) = (x.parse().ok()?, z.parse().ok()?);
            let plugin_held = plugin_has(world, x, z);
            if let Some(chunks) = plugin_chunks().lock().unwrap().get_mut(&world) {
                chunks.remove(&(x, z));
            }
            let player_held = is_default && loaded().lock().unwrap().contains(&(x, z));
            let resident = !is_default
                && ferrite_plugin::world_view()
                    .and_then(|view| view.chunk_loaded(world, x, z))
                    .unwrap_or(false);
            Some((plugin_held && !player_held && !resident).to_string())
        }
        // A plugin ticket, force-load or its removal. `held` is the final union of all of that
        // world's plugin holds, computed by Java, so a chunk held by two plugins is not released
        // when one of them removes its ticket.
        ("holdChunk(int,int,boolean)", [x, z, held]) => {
            let (x, z) = (x.parse().ok()?, z.parse().ok()?);
            let held = match held.as_str() {
                "true" => true,
                "false" => false,
                _ => return None,
            };
            if held {
                hold(world, x, z, true);
            } else if let Some(chunks) = plugin_chunks().lock().unwrap().get_mut(&world) {
                chunks.remove(&(x, z));
            }
            Some("true".to_owned())
        }
        // `World#getWorldPath`: the directory the world saves into, empty for an in-memory world.
        ("getWorldPath()", []) => Some(
            path_of(world)
                .map(|dir| {
                    std::path::absolute(&dir)
                        .unwrap_or(dir)
                        .to_string_lossy()
                        .into_owned()
                })
                .unwrap_or_default(),
        ),
        ("getLoadedChunkCoordinates()", []) => Some(
            coordinates(world, is_default)
                .iter()
                .map(|(x, z)| format!("{x},{z}"))
                .collect::<Vec<_>>()
                .join(&HOST_ITEM_SEPARATOR.to_string()),
        ),
        ("getChunkCount()", []) => Some(coordinates(world, is_default).len().to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(method: &str, args: &[&str]) -> HostCall {
        HostCall {
            owner: "World".into(),
            subject: ferrite_plugin::default_world_id().to_string(),
            method: method.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    #[test]
    fn refreshing_a_chunk_becomes_a_command() {
        assert_eq!(
            refresh_command(&call("refreshChunk(int,int)", &["3", "-4"])),
            Some(ferrite_plugin::PlayerCommand::RefreshChunk { x: 3, z: -4 })
        );
        assert_eq!(
            refresh_command(&call("refreshChunk(int,int)", &["x", "1"])),
            None
        );
        assert_eq!(
            refresh_command(&call("isChunkLoaded(int,int)", &["3", "4"])),
            None
        );
    }

    #[test]
    fn marked_chunks_are_loaded_until_unmarked() {
        set_loaded(9001, -9001, true);
        assert_eq!(
            answer(&call("isChunkLoaded(int,int)", &["9001", "-9001"])).as_deref(),
            Some("true")
        );
        assert!(
            answer(&call("getLoadedChunkCoordinates()", &[]))
                .unwrap()
                .contains("9001,-9001")
        );
        set_loaded(9001, -9001, false);
        assert_eq!(
            answer(&call("isChunkLoaded(int,int)", &["9001", "-9001"])).as_deref(),
            Some("false")
        );
        assert_eq!(answer(&call("isChunkLoaded(int,int)", &["x", "1"])), None);
    }

    #[test]
    fn a_plugin_hold_survives_the_game_loop_and_is_released_by_unload() {
        let world = Uuid::from_u128(0x9003);
        let mut at = call("holdChunk(int,int,boolean)", &["12", "-34", "true"]);
        at.subject = world.to_string();
        assert_eq!(answer(&at).as_deref(), Some("true"));
        let mut loaded = call("isChunkLoaded(int,int)", &["12", "-34"]);
        loaded.subject = world.to_string();
        assert_eq!(answer(&loaded).as_deref(), Some("true"));
        // A world with no source still remembers the plugin's hold and its coordinates.
        let mut list = call("getLoadedChunkCoordinates()", &[]);
        list.subject = world.to_string();
        assert_eq!(answer(&list).as_deref(), Some("12,-34"));
        let mut unload = call("unloadChunk(int,int)", &["12", "-34"]);
        unload.subject = world.to_string();
        assert_eq!(answer(&unload).as_deref(), Some("true"));
        assert_eq!(answer(&loaded).as_deref(), Some("false"));
    }

    #[test]
    fn generated_means_loaded_or_saved_in_a_region_file() {
        set_loaded(9002, 9002, true);
        assert_eq!(
            answer(&call("isChunkGenerated(int,int)", &["9002", "9002"])).as_deref(),
            Some("true")
        );
        set_loaded(9002, 9002, false);
        // No world directory: an unloaded chunk is not generated.
        assert_eq!(
            answer(&call("isChunkGenerated(int,int)", &["9002", "9002"])).as_deref(),
            Some("false")
        );
        // A saved chunk counts even when unloaded.
        let dir = tempfile::tempdir().unwrap();
        let region = dir
            .path()
            .join(crate::game::Dimension::Overworld.region_dir());
        std::fs::create_dir_all(&region).unwrap();
        let file = ferrite_world::RegionFile::open(region.join("r.0.0.mca"), 0, 0).unwrap();
        file.write_chunk(3, 4, b"x", ferrite_world::RegionCompression::None)
            .unwrap();
        assert_eq!(answer(&call("getWorldPath()", &[])).as_deref(), Some(""));
        set_world_dir(Some(dir.path().to_path_buf()));
        assert!(saved_on_disk(3, 4));
        assert!(!saved_on_disk(5, 5));
        assert_eq!(
            answer(&call("getWorldPath()", &[])).as_deref(),
            Some(dir.path().to_str().unwrap())
        );
        set_world_dir(None);
    }

    #[test]
    fn every_world_has_its_own_path() {
        let dir = tempfile::tempdir().unwrap();
        set_world_dir(Some(dir.path().to_path_buf()));
        let nether = Uuid::from_u128(0x9004);
        set_world_path(nether, dir.path().join("DIM-1"));
        let mut call = call("getWorldPath()", &[]);
        call.subject = nether.to_string();
        assert_eq!(
            answer(&call).as_deref(),
            Some(dir.path().join("DIM-1").to_str().unwrap())
        );
        remove_world_path(nether);
        assert_eq!(answer(&call).as_deref(), Some(""));
        set_world_dir(None);
    }

    #[test]
    fn chunk_count_is_the_union_of_player_and_plugin_chunks() {
        let world = Uuid::from_u128(0x9005);
        set_world_dir(None);
        set_loaded(9100, 9100, true);
        hold(world, 9200, 9200, true);
        let mut all = call("getChunkCount()", &[]);
        all.subject = world.to_string();
        assert_eq!(answer(&all).as_deref(), Some("1"));
        assert_eq!(answer(&call("getChunkCount()", &[])).as_deref(), Some("1"));
        hold(world, 9200, 9200, false);
        set_loaded(9100, 9100, false);
    }
}
