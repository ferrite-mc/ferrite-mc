//! The locator bar: every player is a waypoint transmitter and every other player in the same
//! dimension receives it, the way `ServerWaypointManager` connects them. The kind of connection
//! follows vanilla: beyond 332 blocks an azimuth, inside the receiver's view distance an exact
//! position, otherwise the chunk. `/waypoint modify` styles and colors are applied to the icon.

use std::collections::{BTreeMap, HashMap};

use ferrite_protocol::packets::play::{TrackedWaypoint, WaypointData};
use ferrite_protocol::{Encode, Packet, VarInt};
use uuid::Uuid;

use super::commands::OnlinePlayer;
use super::commands_more::WaypointStyle;

const FAR: f64 = 332.0;
/// Vanilla re-sends an azimuth only after it moved by half a degree.
const AZIMUTH_STEP: f32 = 0.008_726_646;
const DEFAULT_STYLE: &str = "minecraft:default";

#[derive(Debug, Clone, PartialEq)]
struct Sent {
    data: WaypointData,
    style: String,
    color: Option<u32>,
}

#[derive(Default)]
pub struct Locator {
    sent: HashMap<(Uuid, Uuid), Sent>,
    /// `(waypoint_transmit_range, waypoint_receive_range)` per player; a player missing here has
    /// no limit.
    pub ranges: HashMap<Uuid, (f64, f64)>,
}

/// Players the locator bar hides: wearing a carved pumpkin or a mob head, sneaking (vanilla's
/// `waypoint_transmit_range_crouch` modifier on `ServerPlayer`) or under the invisibility effect
/// (`effect.waypoint_transmit_range_hide`); each is a -1 `add_multiplied_total` modifier on
/// `waypoint_transmit_range`. The region threads refresh it on `Command::ScanInventory`.
pub static HEAD_HIDDEN: std::sync::Mutex<std::collections::BTreeSet<Uuid>> =
    std::sync::Mutex::new(std::collections::BTreeSet::new());

/// Items whose `waypoint_transmit_range_hide` modifier sits on the head slot (26.3 item data).
pub const HIDING_HEAD_ITEMS: [&str; 8] = [
    "minecraft:carved_pumpkin",
    "minecraft:creeper_head",
    "minecraft:dragon_head",
    "minecraft:piglin_head",
    "minecraft:player_head",
    "minecraft:skeleton_skull",
    "minecraft:wither_skeleton_skull",
    "minecraft:zombie_head",
];

/// Whether a player transmits nothing: a hiding head item, sneaking or invisibility.
pub fn hides(head_item: Option<&str>, sneaking: bool, invisible: bool) -> bool {
    sneaking || invisible || head_item.is_some_and(|name| HIDING_HEAD_ITEMS.contains(&name))
}

/// The `(transmit, receive)` ranges of one player: the attribute with its stored modifiers, plus
/// the head item's hide modifier (`add_multiplied_total` of -1) when it is worn.
pub fn player_ranges(
    attributes: &BTreeMap<(Uuid, String), super::attributes::Stored>,
    uuid: Uuid,
    hidden: bool,
) -> (f64, f64) {
    let value = |path: &str, hide: bool| {
        let limits = super::attributes::default_for(path, true).expect("known attribute");
        let mut stored = attributes
            .get(&(uuid, path.to_owned()))
            .cloned()
            .unwrap_or_default();
        if hide {
            stored.modifiers.insert(
                "minecraft:waypoint_transmit_range_hide".to_owned(),
                (-1.0, super::attributes::Operation::AddMultipliedTotal),
            );
        }
        stored.value(limits, limits.default)
    };
    (
        value("waypoint_transmit_range", hidden),
        value("waypoint_receive_range", false),
    )
}

/// A transmitter that is not a player: a mob (or any living entity) whose
/// `waypoint_transmit_range` attribute was raised above its default of 0 with `/attribute`
/// (`LivingEntity.isTransmittingWaypoint`).
#[derive(Debug, Clone, PartialEq)]
pub struct Extra {
    pub uuid: Uuid,
    pub pos: [f64; 3],
    pub dimension: super::Dimension,
    pub range: f64,
}

/// The non-player transmitters among the entities the command view knows. Players (`is_player`)
/// are the locator's own sources; an entity without a stored attribute has the vanilla default
/// of 0 and does not transmit.
pub fn non_player_transmitters(
    attributes: &BTreeMap<(Uuid, String), super::attributes::Stored>,
    entities: &super::entity_index::EntityIndex,
    is_player: impl Fn(&Uuid) -> bool,
) -> Vec<Extra> {
    let limits = super::attributes::range_of("waypoint_transmit_range").expect("known attribute");
    attributes
        .iter()
        .filter(|((_, path), _)| path == "waypoint_transmit_range")
        .filter(|((uuid, _), _)| !is_player(uuid))
        .filter_map(|((uuid, _), stored)| {
            let entity = entities.get(*uuid)?;
            let default =
                super::attributes::default_for_kind("waypoint_transmit_range", entity.kind)
                    .map_or(0.0, |range| range.default);
            let range = stored.value(limits, default);
            (range > 0.0).then_some(Extra {
                uuid: *uuid,
                pos: entity.pos,
                dimension: entity.dimension,
                range,
            })
        })
        .collect()
}

const NAMED: [(&str, u32); 16] = [
    ("black", 0x000000),
    ("dark_blue", 0x0000AA),
    ("dark_green", 0x00AA00),
    ("dark_aqua", 0x00AAAA),
    ("dark_red", 0xAA0000),
    ("dark_purple", 0xAA00AA),
    ("gold", 0xFFAA00),
    ("gray", 0xAAAAAA),
    ("dark_gray", 0x555555),
    ("blue", 0x5555FF),
    ("green", 0x55FF55),
    ("aqua", 0x55FFFF),
    ("red", 0xFF5555),
    ("light_purple", 0xFF55FF),
    ("yellow", 0xFFFF55),
    ("white", 0xFFFFFF),
];

fn color_of(text: &str) -> Option<u32> {
    if let Some((_, rgb)) = NAMED.iter().find(|(name, _)| *name == text) {
        return Some(*rgb);
    }
    u32::from_str_radix(text.trim_start_matches('#'), 16)
        .ok()
        .filter(|rgb| *rgb <= 0xFF_FFFF)
}

/// The azimuth vanilla sends: the receiver's offset from the source rotated a quarter turn.
fn azimuth(receiver: [f64; 3], source: [f64; 3]) -> f32 {
    let (dx, dz) = (receiver[0] - source[0], receiver[2] - source[2]);
    f64::atan2(dx, -dz) as f32
}

fn desired(
    receiver: &OnlinePlayer,
    source: ([f64; 3], super::Dimension),
    range: f64,
) -> Option<WaypointData> {
    let (source_pos, source_dimension) = source;
    if receiver.dimension != source_dimension {
        return None;
    }
    let d: Vec<f64> = (0..3).map(|i| receiver.pos[i] - source_pos[i]).collect();
    let distance = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    // `doesSourceIgnoreReceiver`: ignored once the distance reaches the range.
    if distance >= range {
        return None;
    }
    if distance > FAR {
        return Some(WaypointData::Azimuth(azimuth(receiver.pos, source_pos)));
    }
    let block = [
        source_pos[0].floor() as i32,
        source_pos[1].floor() as i32,
        source_pos[2].floor() as i32,
    ];
    let chunk = [block[0] >> 4, block[2] >> 4];
    let home = [
        (receiver.pos[0].floor() as i32) >> 4,
        (receiver.pos[2].floor() as i32) >> 4,
    ];
    if (chunk[0] - home[0]).abs() <= receiver.view && (chunk[1] - home[1]).abs() <= receiver.view {
        Some(WaypointData::Position(block))
    } else {
        Some(WaypointData::Chunk(chunk))
    }
}

fn frame(packet: &TrackedWaypoint) -> Vec<u8> {
    let mut bytes = Vec::new();
    VarInt(TrackedWaypoint::ID).encode(&mut bytes);
    packet.encode(&mut bytes);
    bytes
}

impl Locator {
    /// Brings every receiver in line with the players around it; returns the packets to send.
    pub fn sync(
        &mut self,
        online: &HashMap<Uuid, OnlinePlayer>,
        styles: &BTreeMap<Uuid, WaypointStyle>,
        spectators: &std::collections::BTreeSet<Uuid>,
        extra: &[Extra],
    ) -> Vec<(Uuid, Vec<u8>)> {
        let mut out = Vec::new();
        // Every transmitter: the players that are not spectators, then the non-player ones.
        // The fourth field is the transmit range, `None` for no limit.
        let sources: Vec<(Uuid, [f64; 3], super::Dimension, Option<f64>)> = online
            .iter()
            .filter(|(sid, _)| !spectators.contains(sid))
            .map(|(sid, p)| {
                (
                    *sid,
                    p.pos,
                    p.dimension,
                    self.ranges.get(sid).map(|(transmit, _)| *transmit),
                )
            })
            .chain(
                extra
                    .iter()
                    .map(|e| (e.uuid, e.pos, e.dimension, Some(e.range))),
            )
            .collect();
        let packet = |op, uuid, style: &str, color, data| {
            frame(&TrackedWaypoint {
                operation: VarInt(op),
                uuid,
                style: style.to_owned(),
                color,
                data,
            })
        };
        let mut keep = std::collections::HashSet::new();
        for (rid, receiver) in online {
            for (sid, source_pos, source_dimension, transmit) in &sources {
                let sid = sid;
                if rid == sid {
                    continue;
                }
                // `WaypointTransmitter.doesSourceIgnoreReceiver`: a spectator receiver is never
                // ignored, whatever the ranges say.
                let range = if spectators.contains(rid) {
                    f64::INFINITY
                } else {
                    let receive = self.ranges.get(rid).map(|(_, receive)| *receive);
                    transmit
                        .unwrap_or(f64::INFINITY)
                        .min(receive.unwrap_or(f64::INFINITY))
                };
                let Some(data) = desired(receiver, (*source_pos, *source_dimension), range) else {
                    continue;
                };
                keep.insert((*rid, *sid));
                let custom = styles.get(sid);
                let style = custom
                    .and_then(|s| s.style.clone())
                    .unwrap_or_else(|| DEFAULT_STYLE.to_owned());
                let color = custom.and_then(|s| s.color.as_deref()).and_then(color_of);
                let now = Sent { data, style, color };
                let op = match self.sent.get(&(*rid, *sid)) {
                    None => 0,
                    Some(old) if old == &now => continue,
                    Some(old) => match (&old.data, &now.data) {
                        (WaypointData::Azimuth(a), WaypointData::Azimuth(b))
                            if (a - b).abs() <= AZIMUTH_STEP
                                && old.style == now.style
                                && old.color == now.color =>
                        {
                            continue;
                        }
                        (a, b) if std::mem::discriminant(a) == std::mem::discriminant(b) => 2,
                        _ => {
                            out.push((
                                *rid,
                                packet(1, *sid, DEFAULT_STYLE, None, WaypointData::Empty),
                            ));
                            0
                        }
                    },
                };
                out.push((
                    *rid,
                    packet(op, *sid, &now.style, now.color, now.data.clone()),
                ));
                self.sent.insert((*rid, *sid), now);
            }
        }
        self.sent.retain(|(rid, sid), _| {
            if keep.contains(&(*rid, *sid)) {
                return true;
            }
            if online.contains_key(rid) {
                out.push((
                    *rid,
                    packet(1, *sid, DEFAULT_STYLE, None, WaypointData::Empty),
                ));
            }
            false
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Dimension;
    use ferrite_protocol::Decode;

    fn player(pos: [f64; 3], dimension: Dimension) -> OnlinePlayer {
        let (outbound, _) = ferrite_net::game::OutboundSender::channel(1 << 10);
        OnlinePlayer {
            name: String::new(),
            entity: 0,
            outbound,
            pos,
            yaw: 0.0,
            pitch: 0.0,
            dimension,
            view: 10,
        }
    }

    fn decode(bytes: &[u8]) -> TrackedWaypoint {
        let mut buf = bytes;
        assert_eq!(VarInt::decode(&mut buf).unwrap().0, TrackedWaypoint::ID);
        TrackedWaypoint::decode(&mut buf).unwrap()
    }

    #[test]
    fn locator_tracks_updates_changes_kind_and_untracks() {
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let mut online = HashMap::new();
        online.insert(a, player([0.0, 64.0, 0.0], Dimension::Overworld));
        online.insert(b, player([40.0, 70.0, -5.0], Dimension::Overworld));
        let mut locator = Locator::default();
        let styles = BTreeMap::new();

        let first = locator.sync(&online, &styles, &Default::default(), &[]);
        assert_eq!(first.len(), 2);
        let seen = decode(&first.iter().find(|(r, _)| *r == a).unwrap().1);
        assert_eq!((seen.operation.0, seen.uuid), (0, b));
        assert_eq!(seen.data, WaypointData::Position([40, 70, -5]));
        assert!(
            locator
                .sync(&online, &styles, &Default::default(), &[])
                .is_empty()
        );

        online.get_mut(&b).unwrap().pos = [41.0, 70.0, -5.0];
        let moved = locator.sync(&online, &styles, &Default::default(), &[]);
        assert_eq!(decode(&moved[0].1).operation.0, 2);

        online.get_mut(&b).unwrap().pos = [900.0, 70.0, 0.0];
        let far = locator.sync(&online, &styles, &Default::default(), &[]);
        let ops: Vec<i32> = far.iter().map(|(_, p)| decode(p).operation.0).collect();
        assert_eq!(ops.iter().filter(|op| **op == 1).count(), 2);
        assert!(
            far.iter()
                .any(|(_, p)| matches!(decode(p).data, WaypointData::Azimuth(_)))
        );

        online.get_mut(&b).unwrap().dimension = Dimension::Nether;
        let apart = locator.sync(&online, &styles, &Default::default(), &[]);
        assert!(apart.iter().all(|(_, p)| decode(p).operation.0 == 1));
        assert!(
            locator
                .sync(&online, &styles, &Default::default(), &[])
                .is_empty()
        );
    }

    #[test]
    fn transmit_and_receive_ranges_limit_the_locator() {
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let mut online = HashMap::new();
        online.insert(a, player([0.0; 3], Dimension::Overworld));
        online.insert(b, player([50.0, 0.0, 0.0], Dimension::Overworld));
        let mut locator = Locator::default();
        // b transmits 10 blocks only: a is out of reach.
        locator.ranges.insert(a, (6e7, 6e7));
        locator.ranges.insert(b, (10.0, 6e7));
        let sent = locator.sync(&online, &BTreeMap::new(), &Default::default(), &[]);
        assert!(sent.iter().all(|(r, _)| *r != a), "a sees nobody");
        assert_eq!(sent.len(), 1, "b still sees a");
        // A carved pumpkin on b zeroes its range through the hide modifier.
        let (transmit, _) = player_ranges(&BTreeMap::new(), b, true);
        assert_eq!(transmit, 0.0);
        assert_eq!(player_ranges(&BTreeMap::new(), b, false), (6e7, 6e7));
    }

    #[test]
    fn sneaking_invisibility_and_hiding_heads_hide_a_transmitter() {
        assert!(!hides(None, false, false));
        assert!(!hides(Some("minecraft:diamond_helmet"), false, false));
        assert!(hides(Some("minecraft:carved_pumpkin"), false, false));
        assert!(hides(Some("minecraft:zombie_head"), false, false));
        assert!(hides(None, true, false), "sneaking");
        assert!(hides(None, false, true), "invisibility effect");
        let b = Uuid::from_u128(2);
        assert_eq!(
            player_ranges(&BTreeMap::new(), b, hides(None, true, false)).0,
            0.0
        );
        // A range of exactly zero reaches nobody, even someone standing in the same spot.
        let (a, b) = (Uuid::from_u128(1), b);
        let mut online = HashMap::new();
        online.insert(a, player([0.0; 3], Dimension::Overworld));
        online.insert(b, player([0.0; 3], Dimension::Overworld));
        let mut locator = Locator::default();
        locator.ranges.insert(a, (6e7, 6e7));
        locator.ranges.insert(b, (0.0, 6e7));
        let sent = locator.sync(&online, &BTreeMap::new(), &Default::default(), &[]);
        assert!(
            sent.iter().all(|(r, _)| *r != a),
            "hidden b is not shown to a"
        );
    }

    #[test]
    fn a_mob_with_a_transmit_range_shows_up_on_the_locator_bar() {
        let a = Uuid::from_u128(1);
        let mob = Uuid::from_u128(0x30b);
        let mut online = HashMap::new();
        online.insert(a, player([0.0; 3], Dimension::Overworld));
        let mut locator = Locator::default();
        locator.ranges.insert(a, (6e7, 6e7));
        let extra = [Extra {
            uuid: mob,
            pos: [30.0, 64.0, 0.0],
            dimension: Dimension::Overworld,
            range: 100.0,
        }];
        let sent = locator.sync(&online, &BTreeMap::new(), &Default::default(), &extra);
        assert_eq!(sent.len(), 1);
        let packet = decode(&sent[0].1);
        assert_eq!((packet.operation.0, packet.uuid), (0, mob));
        assert_eq!(packet.data, WaypointData::Position([30, 64, 0]));
        // Out of its own range, or in another dimension, it is removed again.
        let far = [Extra {
            pos: [500.0, 64.0, 0.0],
            ..extra[0].clone()
        }];
        let gone = locator.sync(&online, &BTreeMap::new(), &Default::default(), &far);
        assert_eq!(decode(&gone[0].1).operation.0, 1);
        let other = [Extra {
            dimension: Dimension::Nether,
            ..extra[0].clone()
        }];
        assert!(
            locator
                .sync(&online, &BTreeMap::new(), &Default::default(), &other)
                .is_empty()
        );
    }

    #[test]
    fn only_entities_with_a_raised_transmit_range_transmit() {
        let mob = Uuid::from_u128(0x30b);
        let (idle, steve) = (Uuid::from_u128(0x30c), Uuid::from_u128(1));
        let mut index = super::super::entity_index::EntityIndex::default();
        let zombie = ferrite_data::entity_props::by_name("minecraft:zombie")
            .unwrap()
            .id;
        for uuid in [mob, idle, steve] {
            index.insert(super::super::entity_index::CommandEntity {
                uuid,
                network_id: 1,
                kind: zombie,
                pos: [1.0, 2.0, 3.0],
                yaw: 0.0,
                pitch: 0.0,
                dimension: Dimension::Overworld,
                tags: Default::default(),
                custom_name: None,
            });
        }
        let mut attributes = BTreeMap::new();
        let set = |base| super::super::attributes::Stored {
            base: Some(base),
            ..Default::default()
        };
        attributes.insert((mob, "waypoint_transmit_range".to_owned()), set(80.0));
        attributes.insert((idle, "waypoint_transmit_range".to_owned()), set(0.0));
        attributes.insert((steve, "waypoint_transmit_range".to_owned()), set(80.0));
        attributes.insert((mob, "armor".to_owned()), set(5.0));
        let found = non_player_transmitters(&attributes, &index, |uuid| *uuid == steve);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(
            (found[0].uuid, found[0].range, found[0].pos),
            (mob, 80.0, [1.0, 2.0, 3.0])
        );
    }

    #[test]
    fn a_spectator_receiver_ignores_the_ranges() {
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let mut online = HashMap::new();
        online.insert(a, player([0.0; 3], Dimension::Overworld));
        online.insert(b, player([50.0, 0.0, 0.0], Dimension::Overworld));
        let mut locator = Locator::default();
        locator.ranges.insert(a, (6e7, 6e7));
        locator.ranges.insert(b, (10.0, 6e7));
        let spectators = std::collections::BTreeSet::from([a]);
        let sent = locator.sync(&online, &BTreeMap::new(), &spectators, &[]);
        assert!(
            sent.iter().any(|(r, _)| *r == a),
            "spectator a sees out-of-range b"
        );
    }

    #[test]
    fn waypoint_modify_style_and_color_reach_the_icon() {
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let mut online = HashMap::new();
        online.insert(a, player([0.0; 3], Dimension::Overworld));
        online.insert(b, player([5.0, 0.0, 0.0], Dimension::Overworld));
        let mut locator = Locator::default();
        let mut styles = BTreeMap::new();
        styles.insert(
            b,
            WaypointStyle {
                color: Some("#ff8800".into()),
                style: Some("minecraft:bowtie".into()),
            },
        );
        let sent = locator.sync(&online, &styles, &Default::default(), &[]);
        let packet = decode(&sent.iter().find(|(r, _)| *r == a).unwrap().1);
        assert_eq!(packet.style, "minecraft:bowtie");
        assert_eq!(packet.color, Some(0xff8800));
        assert_eq!(color_of("red"), Some(0xFF5555));
        assert_eq!(color_of("nope"), None);
    }

    #[test]
    fn spectators_do_not_transmit_and_are_removed_when_they_become_one() {
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let mut online = HashMap::new();
        online.insert(a, player([0.0; 3], Dimension::Overworld));
        online.insert(b, player([5.0, 0.0, 0.0], Dimension::Overworld));
        let mut locator = Locator::default();
        let styles = BTreeMap::new();
        let none = std::collections::BTreeSet::new();
        assert_eq!(locator.sync(&online, &styles, &none, &[]).len(), 2);
        let spectators = std::collections::BTreeSet::from([b]);
        let after = locator.sync(&online, &styles, &spectators, &[]);
        assert_eq!(after.len(), 1, "only a is told that b is gone");
        assert_eq!(after[0].0, a);
        assert_eq!(decode(&after[0].1).operation.0, 1);
    }

    #[test]
    fn azimuth_matches_the_quarter_turn_formula() {
        assert!((azimuth([10.0, 0.0, 0.0], [0.0; 3]) - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        assert!((azimuth([0.0, 0.0, 10.0], [0.0; 3]) - std::f32::consts::PI).abs() < 1e-6);
        assert!(azimuth([0.0, 0.0, -10.0], [0.0; 3]).abs() < 1e-6);
    }
}
