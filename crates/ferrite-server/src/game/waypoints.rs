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

/// Players wearing a carved pumpkin or a mob head, which hide them from the locator bar. The
/// region threads refresh it on `Command::ScanInventory`.
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

fn desired(receiver: &OnlinePlayer, source: &OnlinePlayer, range: f64) -> Option<WaypointData> {
    if receiver.dimension != source.dimension {
        return None;
    }
    let d: Vec<f64> = (0..3).map(|i| receiver.pos[i] - source.pos[i]).collect();
    let distance = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if distance > range {
        return None;
    }
    if distance > FAR {
        return Some(WaypointData::Azimuth(azimuth(receiver.pos, source.pos)));
    }
    let block = [
        source.pos[0].floor() as i32,
        source.pos[1].floor() as i32,
        source.pos[2].floor() as i32,
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
    ) -> Vec<(Uuid, Vec<u8>)> {
        let mut out = Vec::new();
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
            for (sid, source) in online {
                if rid == sid || spectators.contains(sid) {
                    continue;
                }
                let range = match (self.ranges.get(sid), self.ranges.get(rid)) {
                    (Some((transmit, _)), Some((_, receive))) => transmit.min(*receive),
                    _ => f64::INFINITY,
                };
                let Some(data) = desired(receiver, source, range) else {
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

        let first = locator.sync(&online, &styles, &Default::default());
        assert_eq!(first.len(), 2);
        let seen = decode(&first.iter().find(|(r, _)| *r == a).unwrap().1);
        assert_eq!((seen.operation.0, seen.uuid), (0, b));
        assert_eq!(seen.data, WaypointData::Position([40, 70, -5]));
        assert!(
            locator
                .sync(&online, &styles, &Default::default())
                .is_empty()
        );

        online.get_mut(&b).unwrap().pos = [41.0, 70.0, -5.0];
        let moved = locator.sync(&online, &styles, &Default::default());
        assert_eq!(decode(&moved[0].1).operation.0, 2);

        online.get_mut(&b).unwrap().pos = [900.0, 70.0, 0.0];
        let far = locator.sync(&online, &styles, &Default::default());
        let ops: Vec<i32> = far.iter().map(|(_, p)| decode(p).operation.0).collect();
        assert_eq!(ops.iter().filter(|op| **op == 1).count(), 2);
        assert!(
            far.iter()
                .any(|(_, p)| matches!(decode(p).data, WaypointData::Azimuth(_)))
        );

        online.get_mut(&b).unwrap().dimension = Dimension::Nether;
        let apart = locator.sync(&online, &styles, &Default::default());
        assert!(apart.iter().all(|(_, p)| decode(p).operation.0 == 1));
        assert!(
            locator
                .sync(&online, &styles, &Default::default())
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
        let sent = locator.sync(&online, &BTreeMap::new(), &Default::default());
        assert!(sent.iter().all(|(r, _)| *r != a), "a sees nobody");
        assert_eq!(sent.len(), 1, "b still sees a");
        // A carved pumpkin on b zeroes its range through the hide modifier.
        let (transmit, _) = player_ranges(&BTreeMap::new(), b, true);
        assert_eq!(transmit, 0.0);
        assert_eq!(player_ranges(&BTreeMap::new(), b, false), (6e7, 6e7));
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
        let sent = locator.sync(&online, &styles, &Default::default());
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
        assert_eq!(locator.sync(&online, &styles, &none).len(), 2);
        let spectators = std::collections::BTreeSet::from([b]);
        let after = locator.sync(&online, &styles, &spectators);
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
