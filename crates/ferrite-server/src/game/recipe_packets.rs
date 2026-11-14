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

/// What the client's recipe book knows of the recipes plugins added: the display ids handed out
/// (stable per recipe id and never reused, so a removed recipe's id is not recycled) and the bare
/// displays of the recipes that exist now.
#[derive(Default)]
struct PluginDisplays {
    next: i32,
    index: HashMap<String, i32>,
    current: HashMap<i32, (String, Vec<u8>)>,
}

fn plugin_displays() -> std::sync::MutexGuard<'static, PluginDisplays> {
    static DISPLAYS: OnceLock<std::sync::Mutex<PluginDisplays>> = OnceLock::new();
    DISPLAYS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The recipe display id of a plugin recipe, allocated above every vanilla one on first use.
pub fn plugin_display_index(id: &str) -> i32 {
    let mut displays = plugin_displays();
    if let Some(index) = displays.index.get(id) {
        return *index;
    }
    if displays.next == 0 {
        displays.next = book()
            .recipes
            .values()
            .flat_map(|recipe| recipe.displays.iter().map(|display| display.index))
            .max()
            .unwrap_or(0)
            + 1;
    }
    let index = displays.next;
    displays.next += 1;
    displays.index.insert(id.to_owned(), index);
    index
}

/// The recipe id a recipe-book click (`place_recipe`) names by display id.
pub fn recipe_of_display(index: i32) -> Option<String> {
    if let Some((id, _)) = plugin_displays().current.get(&index) {
        return Some(id.clone());
    }
    book()
        .recipes
        .iter()
        .find(|(_, recipe)| recipe.displays.iter().any(|d| d.index == index))
        .map(|(id, _)| id.clone())
}

/// `place_ghost_recipe`: shows the recipe in the crafting grid when the ingredients are missing.
pub fn ghost_packet(window: i32, index: i32) -> Option<Vec<u8>> {
    let plugin = plugin_displays().current.get(&index).map(|(_, d)| d.clone());
    let display = match plugin {
        Some(display) => display,
        None => book()
            .recipes
            .values()
            .flat_map(|recipe| &recipe.displays)
            .find(|display| display.index == index)
            .map(|display| display.display.clone())?,
    };
    if display.is_empty() {
        return None;
    }
    Some(packet(PLACE_GHOST_RECIPE, |out| {
        VarInt(window).encode(out);
        out.extend_from_slice(&display);
    }))
}

/// Whether the recipe a `place_recipe` click names is one the player has unlocked. The recipes a
/// plugin added are in every player's book.
pub fn display_is_unlocked(unlocked: &BTreeSet<String>, index: i32) -> bool {
    plugin_displays().current.contains_key(&index)
        || recipe_of_display(index).is_some_and(|id| unlocked.contains(&id))
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
#[cfg(test)]
pub fn add_packet(book: &Book, entries: &[(&str, bool)], replace: bool) -> Vec<u8> {
    let entries: Vec<(&str, bool, bool)> = entries.iter().map(|(id, n)| (*id, *n, false)).collect();
    add_packet_highlighted(book, &entries, replace)
}

/// Like [`add_packet`] with `(recipe, notify, highlight)` entries; a highlighted recipe shows
/// the "new" marker in the client's book until the client reports it seen.
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
    /// Whether this sync also carries what plugins changed (`Server#addRecipe` and friends).
    /// The game loop's does; a plain `default()` leaves the vanilla book alone.
    plugins: bool,
    /// The recipe-set revision each player's client was last told about.
    sent_revision: HashMap<Uuid, u64>,
    view: Option<super::crafting::PluginBookView>,
    /// Every plugin recipe this sync has encoded, removed ones included: telling a client to
    /// forget a recipe needs the display ids it was given.
    known: BTreeMap<String, Recipe>,
}

impl RecipeSync {
    /// A sync that also shows plugin recipes in the book and drops the vanilla ones a plugin removed.
    pub fn with_plugin_recipes() -> Self {
        Self {
            plugins: true,
            ..Self::default()
        }
    }

    /// Re-encodes what plugins changed when the recipe set has a new revision.
    fn refresh_view(&mut self, base_update: &[u8]) {
        let revision = super::crafting::plugin_recipe_revision();
        if self.view.as_ref().is_some_and(|view| view.revision == revision) {
            return;
        }
        let Some(view) = super::crafting::plugin_book_view(base_update, &mut |id| {
            plugin_display_index(id)
        }) else {
            return;
        };
        let mut current = HashMap::new();
        for (id, encoded) in &view.entries {
            let index = plugin_display_index(id);
            current.insert(index, (id.clone(), encoded.display.clone()));
            self.known.insert(
                id.clone(),
                Recipe {
                    notify: false,
                    displays: vec![Display {
                        index,
                        wire: encoded.wire.clone(),
                        display: encoded.display.clone(),
                    }],
                },
            );
        }
        plugin_displays().current = current;
        self.view = Some(view);
    }

    pub fn sync(
        &mut self,
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
        self.sent_revision.retain(|uuid, _| online.contains_key(uuid));
        let book = book();
        if book.recipes.is_empty() {
            return Vec::new();
        }
        if self.plugins {
            self.refresh_view(&book.update);
        }
        let Self {
            sent,
            sent_revision,
            view,
            known,
            ..
        } = self;
        let revision = view.as_ref().map_or(0, |view| view.revision);
        let no_hidden = std::collections::HashSet::new();
        let hidden = view.as_ref().map_or(&no_hidden, |view| &view.hidden);
        let plugin_ids: Vec<&str> = view
            .iter()
            .flat_map(|view| view.entries.iter().map(|(id, _)| id.as_str()))
            .collect();
        // A changed recipe set (a plugin recipe, a hidden vanilla one) comes with its own
        // `update_recipes`; before any change the client gets the vanilla body.
        let update_body = view
            .as_ref()
            .filter(|view| view.revision > 0)
            .and_then(|view| view.update.as_deref())
            .unwrap_or(&book.update);
        let find = |id: &str| book.recipes.get(id).or_else(|| known.get(id));
        let empty = BTreeSet::new();
        let mut out = Vec::new();
        for uuid in online.keys() {
            let owned_set = owned.get(uuid).unwrap_or(&empty);
            let mut have: BTreeSet<String> = owned_set
                .iter()
                .filter(|id| !hidden.contains(*id))
                .cloned()
                .collect();
            have.extend(plugin_ids.iter().map(|id| (*id).to_owned()));
            let previous = sent.get(uuid);
            if previous.is_some_and(|old| *old == have)
                && sent_revision.get(uuid).copied().unwrap_or(0) == revision
            {
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
                    out.push((*uuid, packet(UPDATE_RECIPES, |o| o.extend_from_slice(update_body))));
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
                    if sent_revision.get(uuid).copied().unwrap_or(0) != revision {
                        out.push((
                            *uuid,
                            packet(UPDATE_RECIPES, |o| o.extend_from_slice(update_body)),
                        ));
                    }
                    // A recipe a plugin replaced under the same id keeps its display id: tell the
                    // client to forget the old entry before the new one arrives.
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
            sent.insert(*uuid, have);
            sent_revision.insert(*uuid, revision);
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
        let id = <VarInt as ferrite_protocol::Decode>::decode(&mut rest).unwrap().0;
        (id, rest)
    }

    /// The display ids a `recipe_book_add` or `recipe_book_remove` body lists.
    fn add_indexes(body: &[u8]) -> Vec<i32> {
        let mut rest = body;
        let count = <VarInt as ferrite_protocol::Decode>::decode(&mut rest).unwrap().0;
        // Only the first index of each entry is read; entries are told apart by walking the
        // plugin displays this test knows, so decode just the leading varint of the first one.
        assert!(count >= 1);
        vec![<VarInt as ferrite_protocol::Decode>::decode(&mut rest).unwrap().0]
    }

    #[test]
    fn a_plugin_recipe_reaches_the_clients_book_and_leaves_it_when_removed() {
        if book().recipes.is_empty() {
            return;
        }
        let _guard = crate::game::crafting_overlay_lock();
        crate::game::reset_recipes();
        let (uuid, online) = one_player();
        let mut sync = RecipeSync::with_plugin_recipes();
        let owned = BTreeMap::new();
        let setup = sync.sync(&online, &owned, &BTreeMap::new(), None);
        assert_eq!(setup.len(), 3, "update_recipes, settings and the (empty) book");
        let plain_update = setup[0].1.clone();

        // A furnace recipe a plugin adds: the client is sent a new `update_recipes` (the furnace
        // input set and nothing else changes shape) and a `recipe_book_add` with its display.
        let json = r#"{"type":"minecraft:smelting","ingredient":"minecraft:gravel","result":{"id":"minecraft:glass"},"experience":1.0,"cookingtime":50}"#;
        assert_eq!(crate::game::add_plugin_recipe("plugin:book_glass", json, None, 0), Ok(true));
        let added = sync.sync(&online, &owned, &BTreeMap::new(), None);
        let ids: Vec<i32> = added.iter().map(|(_, bytes)| split(bytes).0).collect();
        assert_eq!(ids, [UPDATE_RECIPES, RECIPE_BOOK_ADD], "{ids:?}");
        assert_ne!(added[0].1, plain_update, "gravel is now a furnace input");
        let index = plugin_display_index("plugin:book_glass");
        assert!(index > book().recipes.values().flat_map(|r| r.displays.iter().map(|d| d.index)).max().unwrap());
        assert_eq!(add_indexes(split(&added[1].1).1), [index]);
        assert_eq!(recipe_of_display(index).as_deref(), Some("plugin:book_glass"));
        // The client can place it: a plugin recipe is in every player's book.
        assert!(display_is_unlocked(&BTreeSet::new(), index));
        assert!(ghost_packet(3, index).is_some());
        assert!(sync.sync(&online, &owned, &BTreeMap::new(), None).is_empty(), "nothing changed");

        // A vanilla recipe a plugin removes leaves the book of a player who had it.
        let stick = "minecraft:stick".to_owned();
        let owned = BTreeMap::from([(uuid, BTreeSet::from([stick.clone()]))]);
        let with_stick = sync.sync(&online, &owned, &BTreeMap::new(), None);
        assert_eq!(with_stick.len(), 1);
        assert!(crate::game::remove_recipe("minecraft:stick"));
        let hidden = sync.sync(&online, &owned, &BTreeMap::new(), None);
        let kinds: Vec<i32> = hidden.iter().map(|(_, bytes)| split(bytes).0).collect();
        assert_eq!(kinds, [UPDATE_RECIPES, RECIPE_BOOK_REMOVE], "{kinds:?}");

        // Removing the plugin recipe sends its display id back in a `recipe_book_remove`.
        assert!(crate::game::remove_recipe("plugin:book_glass"));
        let gone = sync.sync(&online, &owned, &BTreeMap::new(), None);
        let removal = gone.iter().find(|(_, bytes)| split(bytes).0 == RECIPE_BOOK_REMOVE).expect("a removal");
        let body = split(&removal.1).1;
        assert_eq!(body[0], 1, "one display");
        assert_eq!(<VarInt as ferrite_protocol::Decode>::decode(&mut &body[1..]).unwrap().0, index);
        assert!(recipe_of_display(index).is_none(), "the id no longer names a recipe");
        assert!(!display_is_unlocked(&BTreeSet::new(), index));

        // A reset brings the vanilla recipe back into the book and the vanilla update body back.
        crate::game::reset_recipes();
        let reset = sync.sync(&online, &owned, &BTreeMap::new(), None);
        let kinds: Vec<i32> = reset.iter().map(|(_, bytes)| split(bytes).0).collect();
        assert!(kinds.contains(&RECIPE_BOOK_ADD), "{kinds:?}");
        assert_eq!(reset[0].1, plain_update);
    }

    #[test]
    fn a_player_who_joins_later_is_sent_the_plugin_recipes_with_the_first_book() {
        if book().recipes.is_empty() {
            return;
        }
        let _guard = crate::game::crafting_overlay_lock();
        crate::game::reset_recipes();
        let json = r#"{"type":"minecraft:stonecutting","ingredient":"minecraft:dirt","result":{"id":"minecraft:stick","count":2}}"#;
        assert_eq!(crate::game::add_plugin_recipe("plugin:book_cut", json, None, 0), Ok(true));
        let (_, online) = one_player();
        let mut sync = RecipeSync::with_plugin_recipes();
        let first = sync.sync(&online, &BTreeMap::new(), &BTreeMap::new(), None);
        let kinds: Vec<i32> = first.iter().map(|(_, bytes)| split(bytes).0).collect();
        assert_eq!(kinds, [UPDATE_RECIPES, RECIPE_BOOK_SETTINGS, RECIPE_BOOK_ADD]);
        let index = plugin_display_index("plugin:book_cut");
        assert_eq!(add_indexes(split(&first[2].1).1), [index]);
        // A plain sync (not the game loop's) leaves the vanilla book alone.
        let mut plain = RecipeSync::default();
        let first = plain.sync(&online, &BTreeMap::new(), &BTreeMap::new(), None);
        assert_eq!(first[0].1.len(), packet(UPDATE_RECIPES, |o| o.extend_from_slice(&book().update)).len());
        crate::game::reset_recipes();
    }
}
