//! The recipe book: `update_recipes` (136), `recipe_book_settings` (77), `recipe_book_add` (75)
//! and `recipe_book_remove` (76).
//!
//! Every recipe display is pre-encoded with the server's own network codecs by
//! `tools/extractor/RecipeExtractor.java` (it boots the real data-pack loader on the user's server
//! jar), written to `vanilla/<ver>/generated/ferrite/recipes.json`. This module only frames the
//! packets and tracks which recipes each player has unlocked (`/recipe give|take`). Recipes are
//! not unlocked by gameplay yet, and the client's recipe-book setting and "seen" packets are
//! not handled.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::OnceLock;

use ferrite_protocol::{Encode, VarInt};
use uuid::Uuid;

use super::commands::OnlinePlayer;

pub const UPDATE_RECIPES: i32 = 136;
pub const RECIPE_BOOK_SETTINGS: i32 = 77;
pub const RECIPE_BOOK_ADD: i32 = 75;
pub const RECIPE_BOOK_REMOVE: i32 = 76;
const FLAG_NOTIFICATION: u8 = 1;

#[derive(Debug, Clone)]
pub struct Display {
    pub index: i32,
    /// `RecipeBookAddPacket.Entry` bytes with the flags byte last.
    pub wire: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Recipe {
    pub notify: bool,
    pub displays: Vec<Display>,
}

#[derive(Debug, Default)]
pub struct Book {
    pub recipes: BTreeMap<String, Recipe>,
    /// Body of `update_recipes` (without the packet id).
    pub update: Vec<u8>,
}

fn decode_hex(text: &str) -> Vec<u8> {
    (0..text.len() / 2)
        .filter_map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok())
        .collect()
}

pub fn parse(json: &str) -> Book {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(json) else {
        return Book::default();
    };
    let mut book = Book {
        update: decode_hex(root["update_recipes"].as_str().unwrap_or_default()),
        ..Book::default()
    };
    for item in root["recipes"].as_array().into_iter().flatten() {
        let Some(id) = item["id"].as_str() else {
            continue;
        };
        let displays = item["entries"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|e| {
                Some(Display {
                    index: i32::try_from(e["index"].as_i64()?).ok()?,
                    wire: decode_hex(e["wire"].as_str()?),
                })
            })
            .collect();
        book.recipes.insert(
            id.to_owned(),
            Recipe {
                notify: item["notify"].as_bool().unwrap_or(false),
                displays,
            },
        );
    }
    book
}

fn book() -> &'static Book {
    static ALL: OnceLock<Book> = OnceLock::new();
    ALL.get_or_init(|| {
        let relative = Path::new("vanilla/26.3/generated/ferrite/recipes.json");
        [
            relative.to_path_buf(),
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(relative),
        ]
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .map(|text| parse(&text))
        .unwrap_or_default()
    })
}

fn packet(id: i32, body: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let mut out = Vec::new();
    VarInt(id).encode(&mut out);
    body(&mut out);
    out
}

/// `recipe_book_add`: `entries` are (recipe, notify) pairs; `replace` drops what the client had.
pub fn add_packet(book: &Book, entries: &[(&str, bool)], replace: bool) -> Vec<u8> {
    let mut wires: Vec<Vec<u8>> = Vec::new();
    for (id, notify) in entries {
        let Some(recipe) = book.recipes.get(*id) else {
            continue;
        };
        for display in &recipe.displays {
            let mut wire = display.wire.clone();
            if let (true, true, Some(flags)) = (*notify, recipe.notify, wire.last_mut()) {
                *flags |= FLAG_NOTIFICATION;
            }
            wires.push(wire);
        }
    }
    packet(RECIPE_BOOK_ADD, |out| {
        VarInt(wires.len() as i32).encode(out);
        for wire in &wires {
            out.extend_from_slice(wire);
        }
        replace.encode(out);
    })
}

pub fn remove_packet(book: &Book, ids: &[&str]) -> Vec<u8> {
    let indexes: Vec<i32> = ids
        .iter()
        .filter_map(|id| book.recipes.get(*id))
        .flat_map(|r| r.displays.iter().map(|d| d.index))
        .collect();
    packet(RECIPE_BOOK_REMOVE, |out| {
        VarInt(indexes.len() as i32).encode(out);
        for index in indexes {
            VarInt(index).encode(out);
        }
    })
}

/// Everything closed and unfiltered: the four recipe books, `[open, filtering]` each.
pub fn settings_packet() -> Vec<u8> {
    packet(RECIPE_BOOK_SETTINGS, |out| out.extend_from_slice(&[0; 8]))
}

fn file(dir: &Path, uuid: &Uuid) -> std::path::PathBuf {
    dir.join(format!("{uuid}.recipes.json"))
}

/// Unlocked recipe ids saved for a player; empty when none.
pub fn load(dir: &Path, uuid: &Uuid) -> BTreeSet<String> {
    std::fs::read_to_string(file(dir, uuid))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(dir: &Path, uuid: &Uuid, owned: &BTreeSet<String>) {
    if let Ok(text) = serde_json::to_string(owned) {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(file(dir, uuid), text);
    }
}

#[derive(Default)]
pub struct RecipeSync {
    sent: HashMap<Uuid, BTreeSet<String>>,
}

impl RecipeSync {
    pub fn sync(
        &mut self,
        online: &HashMap<Uuid, OnlinePlayer>,
        owned: &BTreeMap<Uuid, BTreeSet<String>>,
        persist: Option<&Path>,
    ) -> Vec<(Uuid, Vec<u8>)> {
        self.sent.retain(|uuid, _| online.contains_key(uuid));
        let book = book();
        if book.recipes.is_empty() {
            return Vec::new();
        }
        let empty = BTreeSet::new();
        let mut out = Vec::new();
        for uuid in online.keys() {
            let have = owned.get(uuid).unwrap_or(&empty);
            let previous = self.sent.get(uuid);
            if previous.is_some_and(|old| old == have) {
                continue;
            }
            match previous {
                None => {
                    out.push((
                        *uuid,
                        packet(UPDATE_RECIPES, |o| o.extend_from_slice(&book.update)),
                    ));
                    out.push((*uuid, settings_packet()));
                    let all: Vec<(&str, bool)> =
                        have.iter().map(|id| (id.as_str(), false)).collect();
                    out.push((*uuid, add_packet(book, &all, true)));
                }
                Some(old) => {
                    if let Some(dir) = persist {
                        save(dir, uuid, have);
                    }
                    let added: Vec<(&str, bool)> =
                        have.difference(old).map(|id| (id.as_str(), true)).collect();
                    let removed: Vec<&str> = old.difference(have).map(String::as_str).collect();
                    if !added.is_empty() {
                        out.push((*uuid, add_packet(book, &added, false)));
                    }
                    if !removed.is_empty() {
                        out.push((*uuid, remove_packet(book, &removed)));
                    }
                }
            }
            self.sent.insert(*uuid, have.clone());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Book {
        parse(
            r#"{"recipes":[
              {"id":"t:a","notify":true,"entries":[{"index":3,"wire":"aabb00"}]},
              {"id":"t:b","notify":false,"entries":[{"index":300,"wire":"cc00"},{"index":301,"wire":"dd00"}]}
            ],"update_recipes":"0102"}"#,
        )
    }

    #[test]
    fn add_sets_the_notification_flag_only_when_asked_and_allowed() {
        let book = sample();
        // 75, count 1, entry with flags 0|1, replace=false
        assert_eq!(
            add_packet(&book, &[("t:a", true)], false),
            [75, 1, 0xaa, 0xbb, 0x01, 0]
        );
        assert_eq!(
            add_packet(&book, &[("t:a", false)], true),
            [75, 1, 0xaa, 0xbb, 0x00, 1]
        );
        // `t:b` has no toast, two displays, unknown ids are skipped.
        assert_eq!(
            add_packet(&book, &[("t:b", true), ("t:none", true)], false),
            [75, 2, 0xcc, 0x00, 0xdd, 0x00, 0]
        );
    }

    #[test]
    fn remove_and_settings_match_the_bytes_the_vanilla_codec_writes() {
        let book = sample();
        // Vanilla writes ids 3 and 300 as `02 03 ac 02`.
        assert_eq!(remove_packet(&book, &["t:a", "t:zzz"]), [76, 1, 3]);
        assert_eq!(
            remove_packet(&book, &["t:a", "t:b"]),
            [76, 3, 3, 0xac, 0x02, 0xad, 0x02]
        );
        assert_eq!(settings_packet(), [77, 0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn saved_recipes_round_trip() {
        let dir = std::env::temp_dir().join(format!("ferrite-rec-{}", std::process::id()));
        let uuid = Uuid::from_u128(5);
        assert!(load(&dir, &uuid).is_empty());
        let owned = BTreeSet::from(["minecraft:stick".to_owned()]);
        save(&dir, &uuid, &owned);
        assert_eq!(load(&dir, &uuid), owned);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sync_sends_setup_first_then_additions_and_removals() {
        let uuid = Uuid::from_u128(1);
        let (outbound, _) = ferrite_net::game::OutboundSender::channel(1 << 10);
        let mut online = HashMap::new();
        online.insert(
            uuid,
            OnlinePlayer {
                name: String::new(),
                entity: 0,
                outbound,
                pos: [0.0; 3],
                yaw: 0.0,
                pitch: 0.0,
                dimension: crate::game::Dimension::Overworld,
                view: 10,
            },
        );
        let mut sync = RecipeSync::default();
        let mut owned = BTreeMap::new();
        let loaded = !book().recipes.is_empty();
        let first = sync.sync(&online, &owned, None);
        assert_eq!(first.len(), if loaded { 3 } else { 0 });
        assert!(sync.sync(&online, &owned, None).is_empty());
        if let Some(id) = book().recipes.keys().next() {
            owned.insert(uuid, BTreeSet::from([id.clone()]));
            assert_eq!(sync.sync(&online, &owned, None).len(), 1);
            owned.clear();
            assert_eq!(sync.sync(&online, &owned, None).len(), 1);
        }
    }
}
