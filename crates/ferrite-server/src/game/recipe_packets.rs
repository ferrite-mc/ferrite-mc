//! The recipe book: `update_recipes` (136), `recipe_book_settings` (77), `recipe_book_add` (75)
//! and `recipe_book_remove` (76).
//!
//! Every recipe display is pre-encoded with the server's own network codecs by
//! `tools/extractor/RecipeExtractor.java` (it boots the real data-pack loader on the user's server
//! jar), written to `vanilla/<ver>/generated/ferrite/recipes.json`. This module only frames the
//! packets and tracks which recipes each player has unlocked (`/recipe give|take`, and play:
//! see `recipe_unlock.rs`) and which of them still carry the "new" highlight until the client
//! sends `recipe_book_seen_recipe` (`ServerRecipeBook.highlight`).

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
pub const PLACE_GHOST_RECIPE: i32 = 64;
const FLAG_NOTIFICATION: u8 = 1;
const FLAG_HIGHLIGHT: u8 = 2;

#[derive(Debug, Clone)]
pub struct Display {
    pub index: i32,
    /// `RecipeBookAddPacket.Entry` bytes with the flags byte last.
    pub wire: Vec<u8>,
    /// The bare `RecipeDisplay`, for `place_ghost_recipe`; empty when the data predates it.
    pub display: Vec<u8>,
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
                    display: decode_hex(e["display"].as_str().unwrap_or_default()),
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

/// Vanilla smithing trim entries encoded at runtime (used whenever `recipes.json` lacks them
/// or when looking up a runtime-assigned display id).
#[derive(Default)]
struct RuntimeTrims {
    by_id: BTreeMap<String, Recipe>,
    by_display: HashMap<i32, (String, Vec<u8>)>,
}

fn build_runtime_trims(book: &Book) -> RuntimeTrims {
    let mut next = book
        .recipes
        .values()
        .flat_map(|recipe| recipe.displays.iter().map(|display| display.index))
        .max()
        .unwrap_or(0)
        + 1;
    let mut assigned = HashMap::new();
    let encoded = super::crafting::smithing_trim_book_entries(&mut |id| {
        let index = next;
        next += 1;
        assigned.insert(id.to_owned(), index);
        index
    });
    let mut trims = RuntimeTrims::default();
    for (id, entry) in encoded {
        let Some(&index) = assigned.get(&id) else {
            continue;
        };
        trims
            .by_display
            .insert(index, (id.clone(), entry.display.clone()));
        trims.by_id.insert(
            id,
            Recipe {
                notify: false,
                displays: vec![Display {
                    index,
                    wire: entry.wire,
                    display: entry.display,
                }],
            },
        );
    }
    trims
}

fn runtime_trims() -> &'static RuntimeTrims {
    static TRIMS: OnceLock<RuntimeTrims> = OnceLock::new();
    TRIMS.get_or_init(|| build_runtime_trims(book()))
}

/// The recipe id a recipe-book click (`place_recipe`) names by display id.
pub fn recipe_of_display(index: i32) -> Option<String> {
    book()
        .recipes
        .iter()
        .find(|(_, recipe)| recipe.displays.iter().any(|d| d.index == index))
        .map(|(id, _)| id.clone())
        .or_else(|| {
            runtime_trims()
                .by_display
                .get(&index)
                .map(|(id, _)| id.clone())
        })
}

/// `place_ghost_recipe`: shows the recipe in the crafting grid when the ingredients are missing.
pub fn ghost_packet(window: i32, index: i32) -> Option<Vec<u8>> {
    let display = book()
        .recipes
        .values()
        .flat_map(|recipe| &recipe.displays)
        .find(|display| display.index == index)
        .map(|display| display.display.as_slice())
        .or_else(|| {
            runtime_trims()
                .by_display
                .get(&index)
                .map(|(_, d)| d.as_slice())
        })?;
    if display.is_empty() {
        return None;
    }
    Some(packet(PLACE_GHOST_RECIPE, |out| {
        VarInt(window).encode(out);
        out.extend_from_slice(display);
    }))
}

/// Whether the recipe a `place_recipe` click names is one the player has unlocked.
pub fn display_is_unlocked(unlocked: &BTreeSet<String>, index: i32) -> bool {
    recipe_of_display(index).is_some_and(|id| unlocked.contains(&id))
}

/// Pending per-player recipe book changes drained by the game loop into `GameState::more.recipes`.
static DISCOVERY: std::sync::Mutex<Vec<(Uuid, String, bool)>> = std::sync::Mutex::new(Vec::new());

/// Takes the queued book changes; the game loop applies them to `GameState::more.recipes`.
pub fn take_discovery() -> Vec<(Uuid, String, bool)> {
    std::mem::take(
        &mut *DISCOVERY
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    )
}

fn book() -> &'static Book {
    static ALL: OnceLock<Book> = OnceLock::new();
    ALL.get_or_init(|| {
        let path =
            ferrite_protocol::vanilla_generated_dir(Some(Path::new(env!("CARGO_MANIFEST_DIR"))))
                .join("ferrite/recipes.json");
        std::fs::read_to_string(path)
            .ok()
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
#[cfg(test)]
pub fn add_packet(book: &Book, entries: &[(&str, bool)], replace: bool) -> Vec<u8> {
    let entries: Vec<(&str, bool, bool)> = entries.iter().map(|(id, n)| (*id, *n, false)).collect();
    add_packet_highlighted(book, &entries, replace)
}

/// Like [`add_packet`] with `(recipe, notify, highlight)` entries; a highlighted recipe shows
/// the "new" marker in the client's book until the client reports it seen.
#[cfg(test)]
pub fn add_packet_highlighted(
    book: &Book,
    entries: &[(&str, bool, bool)],
    replace: bool,
) -> Vec<u8> {
    add_packet_in(&|id| book.recipes.get(id), entries, replace)
}

/// [`add_packet_highlighted`] over any way of finding a recipe by id.
fn add_packet_in<'a>(
    find: &dyn Fn(&str) -> Option<&'a Recipe>,
    entries: &[(&str, bool, bool)],
    replace: bool,
) -> Vec<u8> {
    let mut wires: Vec<Vec<u8>> = Vec::new();
    for (id, notify, highlight) in entries {
        let Some(recipe) = find(id) else {
            continue;
        };
        for display in &recipe.displays {
            let mut wire = display.wire.clone();
            if let Some(flags) = wire.last_mut() {
                if *notify && recipe.notify {
                    *flags |= FLAG_NOTIFICATION;
                }
                if *highlight {
                    *flags |= FLAG_HIGHLIGHT;
                }
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

#[cfg(test)]
pub fn remove_packet(book: &Book, ids: &[&str]) -> Vec<u8> {
    remove_packet_in(&|id| book.recipes.get(id), ids)
}

fn remove_packet_in<'a>(find: &dyn Fn(&str) -> Option<&'a Recipe>, ids: &[&str]) -> Vec<u8> {
    let indexes: Vec<i32> = ids
        .iter()
        .filter_map(|id| find(id))
        .flat_map(|r| r.displays.iter().map(|d| d.index))
        .collect();
    packet(RECIPE_BOOK_REMOVE, |out| {
        VarInt(indexes.len() as i32).encode(out);
        for index in indexes {
            VarInt(index).encode(out);
        }
    })
}

/// The four recipe books, `[open, filtering]` each (all closed and unfiltered by default).
pub fn settings_packet(flags: [u8; 8]) -> Vec<u8> {
    packet(RECIPE_BOOK_SETTINGS, |out| out.extend_from_slice(&flags))
}

/// Applies a client's `change_settings`; ignores book types the client cannot send.
pub fn change_settings(flags: &mut [u8; 8], book: i32, open: bool, filtering: bool) {
    if (0..4).contains(&book) {
        flags[book as usize * 2] = u8::from(open);
        flags[book as usize * 2 + 1] = u8::from(filtering);
    }
}

fn settings_file(dir: &Path, uuid: &Uuid) -> std::path::PathBuf {
    dir.join(format!("{uuid}.recipebook.json"))
}

pub fn load_settings(dir: &Path, uuid: &Uuid) -> [u8; 8] {
    std::fs::read_to_string(settings_file(dir, uuid))
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<u8>>(&text).ok())
        .and_then(|flags| <[u8; 8]>::try_from(flags).ok())
        .map(|flags| flags.map(|f| f.min(1)))
        .unwrap_or_default()
}

pub fn save_settings(dir: &Path, uuid: &Uuid, flags: &[u8; 8]) {
    if let Ok(text) = serde_json::to_string(&flags.to_vec()) {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(settings_file(dir, uuid), text);
    }
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

/// Recipes whose "new" highlight the client has not cleared yet, per player.
static HIGHLIGHT: std::sync::Mutex<BTreeMap<Uuid, BTreeSet<String>>> =
    std::sync::Mutex::new(BTreeMap::new());

fn highlights() -> std::sync::MutexGuard<'static, BTreeMap<Uuid, BTreeSet<String>>> {
    HIGHLIGHT.lock().unwrap_or_else(|e| e.into_inner())
}

fn highlight_file(dir: &Path, uuid: &Uuid) -> std::path::PathBuf {
    dir.join(format!("{uuid}.recipehighlight.json"))
}

fn save_highlight(dir: &Path, uuid: &Uuid) {
    let set = highlights().get(uuid).cloned().unwrap_or_default();
    if let Ok(text) = serde_json::to_string(&set) {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(highlight_file(dir, uuid), text);
    }
}

/// The recipes a player still has highlighted.
pub fn highlighted(uuid: &Uuid) -> BTreeSet<String> {
    highlights().get(uuid).cloned().unwrap_or_default()
}

/// The client reports the recipe with this display id seen (`recipe_book_seen_recipe`): its
/// highlight ends, as `ServerRecipeBook.removeHighlight`. Returns whether anything changed.
pub fn mark_seen(uuid: Uuid, display: i32, persist: Option<&Path>) -> bool {
    let Some(id) = recipe_of_display(display) else {
        return false;
    };
    let changed = highlights()
        .get_mut(&uuid)
        .is_some_and(|set| set.remove(&id));
    if changed && let Some(dir) = persist {
        save_highlight(dir, &uuid);
    }
    changed
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
        settings: &BTreeMap<Uuid, [u8; 8]>,
        persist: Option<&Path>,
    ) -> Vec<(Uuid, Vec<u8>)> {
        self.sync_in(book(), runtime_trims(), online, owned, settings, persist)
    }

    fn sync_in(
        &mut self,
        book: &Book,
        trims: &RuntimeTrims,
        online: &HashMap<Uuid, OnlinePlayer>,
        owned: &BTreeMap<Uuid, BTreeSet<String>>,
        settings: &BTreeMap<Uuid, [u8; 8]>,
        persist: Option<&Path>,
    ) -> Vec<(Uuid, Vec<u8>)> {
        // Only players this sync had sent a book to are forgotten, so another sync (a test, or a
        // second world) cannot drop their highlights.
        self.sent.retain(|uuid, _| {
            let keep = online.contains_key(uuid);
            if !keep {
                highlights().remove(uuid);
            }
            keep
        });
        if book.recipes.is_empty() {
            return Vec::new();
        }
        let find = |id: &str| book.recipes.get(id).or_else(|| trims.by_id.get(id));
        let empty = BTreeSet::new();
        let mut out = Vec::new();
        for uuid in online.keys() {
            let owned_set = owned.get(uuid).unwrap_or(&empty);
            // A recipe is in the book when the player unlocked it and it exists.
            let have: BTreeSet<String> = owned_set
                .iter()
                .filter(|id| book.recipes.contains_key(*id) || trims.by_id.contains_key(*id))
                .cloned()
                .collect();
            let previous = self.sent.get(uuid);
            if previous.is_some_and(|old| *old == have) {
                continue;
            }
            match previous {
                None => {
                    if let Some(dir) = persist {
                        let saved: BTreeSet<String> =
                            std::fs::read_to_string(highlight_file(dir, uuid))
                                .ok()
                                .and_then(|text| serde_json::from_str(&text).ok())
                                .unwrap_or_default();
                        highlights().insert(*uuid, saved);
                    }
                    out.push((
                        *uuid,
                        packet(UPDATE_RECIPES, |o| o.extend_from_slice(&book.update)),
                    ));
                    out.push((
                        *uuid,
                        settings_packet(settings.get(uuid).copied().unwrap_or_default()),
                    ));
                    let marked = highlighted(uuid);
                    let all: Vec<(&str, bool, bool)> = have
                        .iter()
                        .map(|id| (id.as_str(), false, marked.contains(id)))
                        .collect();
                    out.push((*uuid, add_packet_in(&find, &all, true)));
                }
                Some(old) => {
                    if let Some(dir) = persist {
                        save(dir, uuid, owned_set);
                    }
                    let removed: Vec<&str> = old.difference(&have).map(String::as_str).collect();
                    let added: Vec<(&str, bool, bool)> = have
                        .difference(old)
                        .map(|id| (id.as_str(), true, true))
                        .collect();
                    {
                        // `ServerRecipeBook.addRecipes` highlights what it adds, `remove` clears it.
                        let mut all = highlights();
                        let set = all.entry(*uuid).or_default();
                        set.extend(added.iter().map(|(id, _, _)| (*id).to_owned()));
                        for id in &removed {
                            set.remove(*id);
                        }
                    }
                    if let Some(dir) = persist {
                        save_highlight(dir, uuid);
                    }
                    if !removed.is_empty() {
                        out.push((*uuid, remove_packet_in(&find, &removed)));
                    }
                    if !added.is_empty() {
                        out.push((*uuid, add_packet_in(&find, &added, false)));
                    }
                }
            }
            self.sent.insert(*uuid, have);
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
    fn a_displays_bare_form_is_kept_for_the_ghost_recipe() {
        let book = parse(
            r#"{"recipes":[{"id":"t:a","notify":true,
               "entries":[{"index":3,"wire":"aabb00","display":"aabb"}]}],"update_recipes":""}"#,
        );
        assert_eq!(book.recipes["t:a"].displays[0].display, [0xaa, 0xbb]);
        // Without the field (older extractor output) the display is empty, so no ghost is sent.
        assert!(sample().recipes["t:a"].displays[0].display.is_empty());
    }

    #[test]
    fn only_unlocked_recipes_can_be_placed_and_real_recipes_have_a_ghost() {
        // Needs `vanilla/26.3/generated/ferrite/recipes.json`; skipped without it.
        let Some(id) = recipe_of_display(0) else {
            return;
        };
        let index = book().recipes[&id].displays[0].index;
        assert!(display_is_unlocked(&BTreeSet::from([id.clone()]), index));
        assert!(!display_is_unlocked(&BTreeSet::new(), index));
        let ghost = ghost_packet(5, index).expect("a real recipe has a display");
        assert_eq!(&ghost[..2], &[PLACE_GHOST_RECIPE as u8, 5]);
        assert!(ghost.len() > 2);
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
    fn highlighted_recipes_carry_flag_two_and_toast_flag_one() {
        let book = sample();
        // 75, one entry with flags 1|2 = 3, replace=false
        assert_eq!(
            add_packet_highlighted(&book, &[("t:a", true, true)], false),
            [75, 1, 0xaa, 0xbb, 0x03, 0]
        );
        assert_eq!(
            add_packet_highlighted(&book, &[("t:a", false, true)], true),
            [75, 1, 0xaa, 0xbb, 0x02, 1]
        );
        // `t:b` never toasts but can still be highlighted.
        assert_eq!(
            add_packet_highlighted(&book, &[("t:b", true, true)], false),
            [75, 2, 0xcc, 0x02, 0xdd, 0x02, 0]
        );
    }

    #[test]
    fn seen_recipes_lose_their_highlight_and_it_survives_a_reconnect() {
        let Some((id, index)) = book()
            .recipes
            .iter()
            .find_map(|(id, r)| Some((id.clone(), r.displays.first()?.index)))
        else {
            return; // needs the extracted recipe data
        };
        let dir = std::env::temp_dir().join(format!("ferrite-hl-{}", std::process::id()));
        let uuid = Uuid::from_u128(77);
        let (outbound, _) = ferrite_net::game::OutboundSender::channel(1 << 10);
        let online = HashMap::from([(
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
        )]);
        let mut sync = RecipeSync::default();
        let mut owned = BTreeMap::new();
        sync.sync(&online, &owned, &BTreeMap::new(), Some(&dir));
        owned.insert(uuid, BTreeSet::from([id.clone()]));
        sync.sync(&online, &owned, &BTreeMap::new(), Some(&dir));
        assert!(
            highlighted(&uuid).contains(&id),
            "a new recipe is highlighted"
        );
        // A reconnect re-sends the book with the highlight still set.
        highlights().remove(&uuid);
        let mut again = RecipeSync::default();
        let sent = again.sync(&online, &owned, &BTreeMap::new(), Some(&dir));
        assert!(
            highlighted(&uuid).contains(&id),
            "the highlight is read back from disk"
        );
        assert!(sent.len() == 3);
        assert!(mark_seen(uuid, index, Some(&dir)));
        assert!(highlighted(&uuid).is_empty());
        assert!(
            !mark_seen(uuid, index, Some(&dir)),
            "seen twice changes nothing"
        );
        highlights().remove(&uuid);
        let mut third = RecipeSync::default();
        third.sync(&online, &owned, &BTreeMap::new(), Some(&dir));
        assert!(
            highlighted(&uuid).is_empty(),
            "a cleared highlight stays cleared"
        );
        let _ = std::fs::remove_dir_all(&dir);
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
        assert_eq!(settings_packet([0; 8]), [77, 0, 0, 0, 0, 0, 0, 0, 0]);
        let mut flags = [0; 8];
        change_settings(&mut flags, 1, true, true);
        change_settings(&mut flags, 9, true, true);
        assert_eq!(settings_packet(flags), [77, 0, 0, 1, 1, 0, 0, 0, 0]);
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
        let first = sync.sync(&online, &owned, &BTreeMap::new(), None);
        assert_eq!(first.len(), if loaded { 3 } else { 0 });
        assert!(
            sync.sync(&online, &owned, &BTreeMap::new(), None)
                .is_empty()
        );
        if let Some(id) = book().recipes.keys().next() {
            owned.insert(uuid, BTreeSet::from([id.clone()]));
            assert_eq!(sync.sync(&online, &owned, &BTreeMap::new(), None).len(), 1);
            owned.clear();
            assert_eq!(sync.sync(&online, &owned, &BTreeMap::new(), None).len(), 1);
        }
    }

    fn one_player() -> (Uuid, HashMap<Uuid, OnlinePlayer>) {
        let uuid = Uuid::from_u128(2);
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
        (uuid, online)
    }

    /// A packet id and body of an outbound packet.
    fn split(bytes: &[u8]) -> (i32, &[u8]) {
        let mut rest = bytes;
        let id = <VarInt as ferrite_protocol::Decode>::decode(&mut rest)
            .unwrap()
            .0;
        (id, rest)
    }

    /// The display ids a `recipe_book_add` or `recipe_book_remove` body lists.
    fn add_indexes(body: &[u8]) -> Vec<i32> {
        let mut rest = body;
        let count = <VarInt as ferrite_protocol::Decode>::decode(&mut rest)
            .unwrap()
            .0;
        // Only the first index of each entry is read; decode just the leading varint of the first one.
        assert!(count >= 1);
        vec![
            <VarInt as ferrite_protocol::Decode>::decode(&mut rest)
                .unwrap()
                .0,
        ]
    }

    #[test]
    fn vanilla_smithing_trim_sync_encodes_and_delivers_unlocked_trims() {
        let trims = runtime_trims();
        if trims.by_id.is_empty() {
            return;
        }
        let trim_id = "minecraft:bolt_armor_trim_smithing_template_smithing_trim";
        let trim_recipe = trims
            .by_id
            .get(trim_id)
            .expect("bolt smithing trim is encoded at runtime");
        let runtime_index = trim_recipe.displays[0].index;
        assert!(
            !trim_recipe.displays[0].display.is_empty() && !trim_recipe.displays[0].wire.is_empty()
        );
        assert_eq!(recipe_of_display(runtime_index).as_deref(), Some(trim_id));
        assert!(ghost_packet(4, runtime_index).is_some());
        assert!(display_is_unlocked(
            &BTreeSet::from([trim_id.to_owned()]),
            runtime_index
        ));
        assert!(!display_is_unlocked(&BTreeSet::new(), runtime_index));

        // Even when the pre-extracted book lacks smithing trim entries, RecipeSync falls back to
        // runtime_trims() and sends RECIPE_BOOK_ADD / RECIPE_BOOK_REMOVE with the runtime display id.
        let (uuid, online) = one_player();
        let synthetic_book = sample();
        let mut sync = RecipeSync::default();
        let mut owned = BTreeMap::new();
        let setup = sync.sync_in(
            &synthetic_book,
            trims,
            &online,
            &owned,
            &BTreeMap::new(),
            None,
        );
        let setup_ids: Vec<i32> = setup.iter().map(|(_, bytes)| split(bytes).0).collect();
        assert_eq!(
            setup_ids,
            [UPDATE_RECIPES, RECIPE_BOOK_SETTINGS, RECIPE_BOOK_ADD]
        );

        owned.insert(uuid, BTreeSet::from([trim_id.to_owned()]));
        let added = sync.sync_in(
            &synthetic_book,
            trims,
            &online,
            &owned,
            &BTreeMap::new(),
            None,
        );
        assert_eq!(added.len(), 1);
        assert_eq!(split(&added[0].1).0, RECIPE_BOOK_ADD);
        assert_eq!(add_indexes(split(&added[0].1).1), [runtime_index]);

        owned.clear();
        let removed = sync.sync_in(
            &synthetic_book,
            trims,
            &online,
            &owned,
            &BTreeMap::new(),
            None,
        );
        assert_eq!(removed.len(), 1);
        assert_eq!(split(&removed[0].1).0, RECIPE_BOOK_REMOVE);
        assert_eq!(add_indexes(split(&removed[0].1).1), [runtime_index]);
        highlights().remove(&uuid);
    }

    #[test]
    fn join_recipe_sync_sends_update_settings_and_unlocked_recipes_including_trims() {
        if book().recipes.is_empty() {
            return;
        }
        let (uuid, online) = one_player();
        let stick = "minecraft:stick".to_owned();
        let trim = "minecraft:bolt_armor_trim_smithing_template_smithing_trim".to_owned();
        let owned = BTreeMap::from([(uuid, BTreeSet::from([stick, trim]))]);
        let mut flags = [0u8; 8];
        change_settings(&mut flags, 0, true, false);
        let settings = BTreeMap::from([(uuid, flags)]);

        let mut sync = RecipeSync::default();
        let packets = sync.sync(&online, &owned, &settings, None);
        let kinds: Vec<i32> = packets.iter().map(|(_, bytes)| split(bytes).0).collect();
        assert_eq!(
            kinds,
            [UPDATE_RECIPES, RECIPE_BOOK_SETTINGS, RECIPE_BOOK_ADD]
        );
        assert_eq!(split(&packets[0].1).1, book().update.as_slice());
        assert_eq!(split(&packets[1].1).1, &flags);
        let add_body = split(&packets[2].1).1;
        let mut rest = add_body;
        let count = <VarInt as ferrite_protocol::Decode>::decode(&mut rest)
            .unwrap()
            .0;
        assert!(count >= 2, "both unlocked recipes are sent on join");
        assert_eq!(
            add_body.last().copied(),
            Some(1),
            "initial join sends replace=true"
        );
        assert!(sync.sync(&online, &owned, &settings, None).is_empty());
        highlights().remove(&uuid);
    }

    #[test]
    fn recipe_settings_and_discovery_persistence_round_trip() {
        let dir = std::env::temp_dir().join(format!("ferrite-rec-persist-{}", std::process::id()));
        let uuid = Uuid::from_u128(99);
        let mut flags = [0u8; 8];
        change_settings(&mut flags, 2, true, true);
        save_settings(&dir, &uuid, &flags);
        assert_eq!(load_settings(&dir, &uuid), flags);

        if let Some(id) = book().recipes.keys().next().cloned() {
            let online = HashMap::from([(uuid, one_player().1.into_values().next().unwrap())]);
            let mut sync = RecipeSync::default();
            let mut owned = BTreeMap::new();
            sync.sync(&online, &owned, &BTreeMap::new(), Some(&dir));
            owned.insert(uuid, BTreeSet::from([id.clone()]));
            sync.sync(&online, &owned, &BTreeMap::new(), Some(&dir));
            assert_eq!(load(&dir, &uuid), BTreeSet::from([id]));
            highlights().remove(&uuid);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
