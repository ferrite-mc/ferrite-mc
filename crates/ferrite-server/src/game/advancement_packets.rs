//! `update_advancements` (133): the advancement tabs and the toast when something is granted.
//!
//! The wire form of every displayed advancement (title, icon, frame, layout position) comes from
//! `vanilla/<ver>/generated/ferrite/advancements.json`, written by
//! `tools/extractor/AdvancementExtractor.java` with the server's own network codec, so nothing
//! here re-implements component or item-stack encoding. This module frames the packet and decides
//! what each player may see, as `PlayerAdvancements` does: a root, an advancement whose parent is
//! the root or done, or one that is itself done (hidden ones only when done).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::OnceLock;

use ferrite_protocol::{Encode, VarInt};
use uuid::Uuid;

use super::commands::OnlinePlayer;

/// Packet id of `update_advancements` in `packets.json` for 26.3.
pub const UPDATE_ADVANCEMENTS: i32 = 133;

#[derive(Debug, Clone)]
pub struct Entry {
    pub id: String,
    pub parent: Option<String>,
    pub hidden: bool,
    pub frame: String,
    pub announce: bool,
    pub requirements: Vec<Vec<String>>,
    pub wire: Vec<u8>,
}

fn decode_hex(text: &str) -> Vec<u8> {
    (0..text.len() / 2)
        .filter_map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok())
        .collect()
}

pub fn parse(json: &str) -> Vec<Entry> {
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str(json) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            Some(Entry {
                id: item["id"].as_str()?.to_owned(),
                parent: item["parent"].as_str().map(str::to_owned),
                hidden: item["hidden"].as_bool().unwrap_or(false),
                frame: item["frame"].as_str().unwrap_or("task").to_owned(),
                announce: item["announce"].as_bool().unwrap_or(false),
                requirements: item["requirements"]
                    .as_array()?
                    .iter()
                    .map(|group| {
                        group
                            .as_array()
                            .map(|g| {
                                g.iter()
                                    .filter_map(|c| c.as_str().map(str::to_owned))
                                    .collect()
                            })
                            .unwrap_or_default()
                    })
                    .collect(),
                wire: decode_hex(item["wire"].as_str()?),
            })
        })
        .collect()
}

fn entries() -> &'static BTreeMap<String, Entry> {
    static ALL: OnceLock<BTreeMap<String, Entry>> = OnceLock::new();
    ALL.get_or_init(|| {
        let relative = Path::new("vanilla/26.3/generated/ferrite/advancements.json");
        [
            relative.to_path_buf(),
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(relative),
        ]
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .map(|text| {
            parse(&text)
                .into_iter()
                .map(|e| (e.id.clone(), e))
                .collect()
        })
        .unwrap_or_default()
    })
}

pub fn is_done(entry: &Entry, owned: &BTreeSet<String>) -> bool {
    !entry.requirements.is_empty()
        && entry.requirements.iter().all(|group| {
            group
                .iter()
                .any(|c| owned.contains(&format!("{}#{c}", entry.id)))
        })
}

fn visible_set(all: &BTreeMap<String, Entry>, owned: &BTreeSet<String>) -> BTreeSet<String> {
    all.values()
        .filter(|entry| {
            let done = is_done(entry, owned);
            if done {
                return true;
            }
            if entry.hidden {
                return false;
            }
            match entry.parent.as_deref().and_then(|p| all.get(p)) {
                None => entry.parent.is_none(),
                Some(parent) => parent.parent.is_none() || is_done(parent, owned),
            }
        })
        .map(|entry| entry.id.clone())
        .collect()
}

fn put_string(out: &mut Vec<u8>, text: &str) {
    VarInt(text.len() as i32).encode(out);
    out.extend_from_slice(text.as_bytes());
}

/// Frames an `update_advancements` packet (with its packet id) from already chosen parts.
pub fn frame(
    reset: bool,
    added: &[&Entry],
    removed: &[String],
    progress: &[(&Entry, &BTreeSet<String>)],
    show: bool,
    now_millis: i64,
) -> Vec<u8> {
    let mut out = Vec::new();
    VarInt(UPDATE_ADVANCEMENTS).encode(&mut out);
    reset.encode(&mut out);
    VarInt(added.len() as i32).encode(&mut out);
    for entry in added {
        out.extend_from_slice(&entry.wire);
    }
    VarInt(removed.len() as i32).encode(&mut out);
    for id in removed {
        put_string(&mut out, id);
    }
    VarInt(progress.len() as i32).encode(&mut out);
    for (entry, owned) in progress {
        put_string(&mut out, &entry.id);
        let criteria: Vec<&String> = entry.requirements.iter().flatten().collect();
        VarInt(criteria.len() as i32).encode(&mut out);
        for name in criteria {
            put_string(&mut out, name);
            if owned.contains(&format!("{}#{name}", entry.id)) {
                true.encode(&mut out);
                out.extend_from_slice(&now_millis.to_be_bytes());
            } else {
                false.encode(&mut out);
            }
        }
    }
    show.encode(&mut out);
    out
}

fn file(dir: &Path, uuid: &Uuid) -> std::path::PathBuf {
    dir.join(format!("{uuid}.advancements.json"))
}

/// Obtained criteria (`<advancement>#<criterion>`) saved for a player; empty when none.
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
pub struct AdvancementSync {
    /// Advancements players finished since the last drain, for announcements and rewards.
    pub completed: Vec<(Uuid, String)>,
    sent: HashMap<Uuid, (BTreeSet<String>, BTreeSet<String>)>,
}

impl AdvancementSync {
    /// One packet per player whose advancements changed since the last call.
    pub fn sync(
        &mut self,
        online: &HashMap<Uuid, OnlinePlayer>,
        owned: &BTreeMap<Uuid, BTreeSet<String>>,
        persist: Option<&Path>,
    ) -> Vec<(Uuid, Vec<u8>)> {
        self.sent.retain(|uuid, _| online.contains_key(uuid));
        let all = entries();
        if all.is_empty() {
            return Vec::new();
        }
        let empty = BTreeSet::new();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64);
        let mut out = Vec::new();
        for uuid in online.keys() {
            let have = owned.get(uuid).unwrap_or(&empty);
            let previous = self.sent.get(uuid);
            if previous.is_some_and(|(old, _)| old == have) {
                continue;
            }
            let reset = previous.is_none();
            let visible = visible_set(all, have);
            let (old_owned, old_visible) = previous.cloned().unwrap_or_default();
            let added: Vec<&Entry> = visible
                .difference(&old_visible)
                .filter_map(|id| all.get(id))
                .collect();
            let removed: Vec<String> = old_visible.difference(&visible).cloned().collect();
            let progress: Vec<(&Entry, &BTreeSet<String>)> = visible
                .iter()
                .filter_map(|id| all.get(id))
                .filter(|entry| {
                    let prefix = format!("{}#", entry.id);
                    let mine: BTreeSet<&String> =
                        have.iter().filter(|c| c.starts_with(&prefix)).collect();
                    let theirs: BTreeSet<&String> = old_owned
                        .iter()
                        .filter(|c| c.starts_with(&prefix))
                        .collect();
                    reset || !old_visible.contains(&entry.id) || mine != theirs
                })
                .map(|entry| (entry, have))
                .collect();
            if !reset {
                for entry in all.values() {
                    if is_done(entry, have) && !is_done(entry, &old_owned) {
                        self.completed.push((*uuid, entry.id.clone()));
                    }
                }
            }
            out.push((*uuid, frame(reset, &added, &removed, &progress, true, now)));
            if let (false, Some(dir)) = (reset, persist) {
                save(dir, uuid, have);
            }
            self.sent.insert(*uuid, (have.clone(), visible));
        }
        out
    }
}

/// The chat line for a finished advancement, e.g. `Alice has made the advancement [Stone Age]`.
pub fn announcement(player: &str, id: &str) -> Option<String> {
    let entry = entries().get(id)?;
    if !entry.announce {
        return None;
    }
    let path = id
        .strip_prefix("minecraft:")
        .unwrap_or(id)
        .replace('/', ".");
    let title = super::feedback::render(&format!("advancements.{path}.title"), &[]);
    let key = match entry.frame.as_str() {
        "goal" => "chat.type.advancement.goal",
        "challenge" => "chat.type.advancement.challenge",
        _ => "chat.type.advancement.task",
    };
    Some(super::feedback::render(
        key,
        &[player, &format!("[{title}]")],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, parent: Option<&str>, hidden: bool, groups: &[&[&str]]) -> Entry {
        Entry {
            id: id.into(),
            parent: parent.map(str::to_owned),
            hidden,
            frame: "task".into(),
            announce: false,
            requirements: groups
                .iter()
                .map(|g| g.iter().map(|c| (*c).to_owned()).collect())
                .collect(),
            wire: vec![0xAA, 0xBB],
        }
    }

    fn tree() -> BTreeMap<String, Entry> {
        [
            entry("t:root", None, false, &[&["x"]]),
            entry("t:child", Some("t:root"), false, &[&["a"]]),
            entry("t:grand", Some("t:child"), false, &[&["b"]]),
            entry("t:secret", Some("t:root"), true, &[&["s"]]),
        ]
        .into_iter()
        .map(|e| (e.id.clone(), e))
        .collect()
    }

    #[test]
    fn frame_matches_the_bytes_the_vanilla_packet_codec_writes() {
        // Captured from ClientboundUpdateAdvancementsPacket.STREAM_CODEC with progress for
        // requirements [[a],[b,c]] where only `a` was obtained at 1793922735815 ms.
        let e = entry("ferrite:t", None, false, &[&["a"], &["b", "c"]]);
        let owned: BTreeSet<String> = ["ferrite:t#a".to_owned()].into();
        let mut bytes = frame(
            true,
            &[],
            &["ferrite:gone".into()],
            &[(&e, &owned)],
            true,
            1_793_922_735_815,
        );
        assert_eq!(bytes.drain(..2).collect::<Vec<_>>(), [0x85, 0x01]);
        let expected = "0100010c666572726974653a676f6e650109666572726974653a7403016101000001a1ae209ec701620001630001";
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        // The vanilla capture lists criteria a, b, c; ours flattens the same requirements.
        assert_eq!(hex[..], expected[..]);
    }

    #[test]
    fn visibility_follows_parent_progress_and_hidden_flag() {
        let all = tree();
        let none = BTreeSet::new();
        let ids = |s: BTreeSet<String>| s.into_iter().collect::<Vec<_>>();
        assert_eq!(ids(visible_set(&all, &none)), ["t:child", "t:root"]);
        let child: BTreeSet<String> = ["t:child#a".to_owned()].into();
        assert_eq!(
            ids(visible_set(&all, &child)),
            ["t:child", "t:grand", "t:root"]
        );
        let secret: BTreeSet<String> = ["t:secret#s".to_owned()].into();
        assert!(visible_set(&all, &secret).contains("t:secret"));
    }

    #[test]
    fn saved_progress_round_trips() {
        let dir = std::env::temp_dir().join(format!("ferrite-adv-{}", std::process::id()));
        let uuid = Uuid::from_u128(7);
        assert!(load(&dir, &uuid).is_empty());
        let owned: BTreeSet<String> = ["a:b#c".to_owned()].into();
        save(&dir, &uuid, &owned);
        assert_eq!(load(&dir, &uuid), owned);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sync_sends_a_reset_first_then_only_changes() {
        let uuid = Uuid::from_u128(9);
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
        let mut sync = AdvancementSync::default();
        let mut owned = BTreeMap::new();
        // Without extracted data there is nothing to send, and nothing may be sent twice.
        let first = sync.sync(&online, &owned, None);
        assert_eq!(first.len(), usize::from(!entries().is_empty()));
        assert!(sync.sync(&online, &owned, None).is_empty());
        owned.insert(
            uuid,
            ["minecraft:story/root#crafting_table".to_owned()].into(),
        );
        let changed = sync.sync(&online, &owned, None);
        assert_eq!(changed.len(), usize::from(!entries().is_empty()));
        assert!(sync.sync(&online, &owned, None).is_empty());
    }

    #[test]
    fn announcements_use_the_frame_and_the_lang_title() {
        // Needs the extracted data; without it nothing is announced.
        match announcement("Alice", "minecraft:story/mine_stone") {
            None => {
                assert!(entries().is_empty() || !entries()["minecraft:story/mine_stone"].announce)
            }
            Some(text) => {
                assert!(
                    text.starts_with("Alice has made the advancement ["),
                    "{text}"
                );
            }
        }
        assert_eq!(announcement("Alice", "t:unknown"), None);
    }

    #[test]
    fn parse_reads_the_extractor_output() {
        let parsed = parse(
            r#"[{"id":"minecraft:a/root","parent":null,"hidden":false,"requirements":[["x","y"]],"wire":"0a0b"}]"#,
        );
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].wire, vec![0x0a, 0x0b]);
        assert_eq!(
            parsed[0].requirements,
            vec![vec!["x".to_owned(), "y".to_owned()]]
        );
    }
}
