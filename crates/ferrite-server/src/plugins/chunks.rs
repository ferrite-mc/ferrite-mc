//! Which overworld chunks the server keeps loaded (some player or the spawn holds them), for
//! `World#isChunkLoaded` and `World#getLoadedChunks`.
//!
//! The game loop marks a chunk when its first holder arrives and unmarks it when the last one
//! leaves. Plugins only read.

use ferrite_plugin::{HOST_ITEM_SEPARATOR, HostCall};
use std::{
    collections::BTreeSet,
    sync::{Mutex, OnceLock},
};

fn loaded() -> &'static Mutex<BTreeSet<(i32, i32)>> {
    static LOADED: OnceLock<Mutex<BTreeSet<(i32, i32)>>> = OnceLock::new();
    LOADED.get_or_init(Default::default)
}

fn world_dir() -> &'static Mutex<Option<std::path::PathBuf>> {
    static DIR: OnceLock<Mutex<Option<std::path::PathBuf>>> = OnceLock::new();
    DIR.get_or_init(Default::default)
}

/// The world directory chunks are saved in (`None` for the in-memory world).
pub fn set_world_dir(path: Option<std::path::PathBuf>) {
    *world_dir().lock().unwrap() = path;
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

pub fn set_loaded(x: i32, z: i32, is_loaded: bool) {
    let mut set = loaded().lock().unwrap();
    if is_loaded {
        set.insert((x, z));
    } else {
        set.remove(&(x, z));
    }
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

/// The reply to a chunk state query about the default world, or `None` for anything else.
pub fn answer(call: &HostCall) -> Option<String> {
    if call.owner != "World" || !is_default_world(&call.subject) {
        return None;
    }
    match (call.method.as_str(), call.args.as_slice()) {
        ("isChunkLoaded(int,int)", [x, z]) => {
            let key = (x.parse().ok()?, z.parse().ok()?);
            Some(loaded().lock().unwrap().contains(&key).to_string())
        }
        ("isChunkGenerated(int,int)", [x, z]) => {
            let key = (x.parse().ok()?, z.parse().ok()?);
            let loaded = loaded().lock().unwrap().contains(&key);
            Some((loaded || saved_on_disk(key.0, key.1)).to_string())
        }
        // `World#getWorldPath`: the directory the world saves into, empty for the in-memory world.
        ("getWorldPath()", []) => Some(
            world_dir()
                .lock()
                .unwrap()
                .as_ref()
                .map(|dir| {
                    std::path::absolute(dir)
                        .unwrap_or_else(|_| dir.clone())
                        .to_string_lossy()
                        .into_owned()
                })
                .unwrap_or_default(),
        ),
        ("getLoadedChunkCoordinates()", []) => Some(
            loaded()
                .lock()
                .unwrap()
                .iter()
                .map(|(x, z)| format!("{x},{z}"))
                .collect::<Vec<_>>()
                .join(&HOST_ITEM_SEPARATOR.to_string()),
        ),
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
}
