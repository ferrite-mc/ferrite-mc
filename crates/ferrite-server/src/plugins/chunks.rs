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
}
