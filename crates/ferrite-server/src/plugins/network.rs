//! Packets a plugin sends to one player: fake block changes, plugin messages, transfers,
//! resource packs and the compass target.
//!
//! Everything arrives as text from plugin code, so each value is validated before it can become
//! a packet: keys and hosts are character-checked, sizes are capped, and coordinates must be
//! finite. A call that fails validation is left unhandled, so the plugin gets the usual
//! "not implemented" error rather than a silent no-op.

use base64::Engine as _;
use ferrite_plugin::{HostCall, PlayerCommand, PluginPacket};
use uuid::Uuid;

const PART: char = '\u{1d}';
/// Vanilla limit for a clientbound custom payload.
const MAX_PAYLOAD: usize = 1_048_576;

fn block_pos(location: &str) -> Option<[i32; 3]> {
    let mut parts = location.split(PART);
    parts.next()?;
    let mut axis = || -> Option<i32> {
        let value: f64 = parts.next()?.parse().ok()?;
        (value.is_finite() && value.abs() < 3.0e7).then(|| value.floor() as i32)
    };
    Some([axis()?, axis()?, axis()?])
}

/// `STONE`, `minecraft:stone` or `stone` to the block name the state table uses.
fn block_name(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let name = lower.strip_prefix("minecraft:").unwrap_or(&lower);
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    valid.then(|| name.to_owned())
}

/// Legacy `BungeeCord` and namespaced `ns:path` channels; anything else is not a channel.
fn channel(text: &str) -> Option<String> {
    if text == "BungeeCord" {
        return Some("bungeecord:main".into());
    }
    let (namespace, path) = text.split_once(':')?;
    let part = |s: &str, extra: &[char]| {
        !s.is_empty()
            && s.chars().all(|c| {
                c.is_ascii_lowercase()
                    || c.is_ascii_digit()
                    || c == '_'
                    || c == '-'
                    || c == '.'
                    || extra.contains(&c)
            })
    };
    (part(namespace, &[]) && part(path, &['/'])).then(|| text.to_owned())
}

fn hostname(text: &str) -> Option<&str> {
    let ok = !text.is_empty()
        && text.len() <= 255
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'));
    ok.then_some(text)
}

fn url(text: &str) -> Option<&str> {
    let ok = (text.starts_with("http://") || text.starts_with("https://"))
        && text.len() <= 32_767
        && !text.chars().any(|c| c.is_control() || c.is_whitespace());
    ok.then_some(text)
}

/// The SHA-1 of a resource pack as the 40 hex digits the client expects; empty means "unknown".
fn pack_hash(base64_bytes: &str) -> Option<String> {
    if base64_bytes.is_empty() || base64_bytes == ferrite_plugin::HOST_NULL {
        return Some(String::new());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64_bytes)
        .ok()?;
    (bytes.len() == 20).then(|| bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn resource_pack(player: Uuid, call: &HostCall) -> Option<PlayerCommand> {
    let types: Vec<&str> = call
        .method
        .split_once('(')?
        .1
        .trim_end_matches(')')
        .split(',')
        .filter(|t| !t.is_empty())
        .collect();
    if types.len() != call.args.len() {
        return None;
    }
    let (mut id, mut link, mut hash, mut required) = (None, None, String::new(), false);
    for (kind, value) in types.iter().zip(&call.args) {
        match *kind {
            "UUID" => id = Some(Uuid::parse_str(value).ok()?),
            "String" if link.is_none() => link = Some(url(value)?.to_owned()),
            "byte[]" => hash = pack_hash(value)?,
            "boolean" => required = value == "true",
            // A prompt (String or Component) is not sent yet; the pack is offered without one.
            "String" | "Component" => {}
            _ => return None,
        }
    }
    let url = link?;
    let id = id.unwrap_or_else(|| Uuid::new_v3(&Uuid::NAMESPACE_URL, url.as_bytes()));
    Some(PlayerCommand::Packet {
        player,
        packet: PluginPacket::ResourcePackPush {
            id,
            url,
            hash,
            required,
        },
    })
}

/// The vanilla entity-event status of a Bukkit `EntityEffect`, for the effects whose status is
/// certain; others are left unhandled.
fn entity_status(effect: &str) -> Option<u8> {
    Some(match effect {
        "HURT" => 2,
        "DEATH" => 3,
        "SHIELD_BLOCK" => 29,
        "SHIELD_BREAK" => 30,
        "TOTEM_RESURRECT" => 35,
        "HURT_DROWN" => 36,
        "HURT_EXPLOSION" => 37,
        "TELEPORT_ENDER" => 46,
        _ => return None,
    })
}

/// The packet command behind `call`, or `None` when it is another call or a value is invalid.
pub fn network_command(call: &HostCall) -> Option<PlayerCommand> {
    if call.owner != "Player" {
        return None;
    }
    let player = Uuid::parse_str(&call.subject).ok()?;
    let arg = |i: usize| call.args.get(i).map(String::as_str);
    if let Some(hidden) = match call.method.as_str() {
        "hidePlayer(Player)" | "hidePlayer(Plugin,Player)" | "hideEntity(Plugin,Entity)" => {
            Some(true)
        }
        "showPlayer(Player)" | "showPlayer(Plugin,Player)" | "showEntity(Plugin,Entity)" => {
            Some(false)
        }
        _ => None,
    } {
        let target = Uuid::parse_str(call.args.last()?).ok()?;
        return (target != player).then_some(PlayerCommand::Visibility {
            viewer: player,
            target,
            hidden,
        });
    }
    let packet = match call.method.as_str() {
        "sendBlockChange(Location,BlockData)" | "sendBlockChange(Location,Material,byte)" => {
            let pos = block_pos(arg(0)?)?;
            let state = ferrite_data::blocks::default_state(&block_name(arg(1)?)?)?;
            PluginPacket::BlockChange {
                pos,
                state: u32::from(state),
            }
        }
        "sendPluginMessage(Plugin,String,byte[])" => {
            let data = base64::engine::general_purpose::STANDARD
                .decode(arg(2)?)
                .ok()?;
            if data.len() > MAX_PAYLOAD {
                return None;
            }
            PluginPacket::PluginMessage {
                channel: channel(arg(1)?)?,
                data,
            }
        }
        "transfer(String,int)" => {
            let port: u16 = arg(1)?.parse().ok().filter(|p| *p != 0)?;
            PluginPacket::Transfer {
                host: hostname(arg(0)?)?.to_owned(),
                port,
            }
        }
        // `Animate` actions: 0 swing main hand, 3 swing off hand.
        "swingMainHand()" => PluginPacket::Animation { action: 0 },
        "swingOffHand()" => PluginPacket::Animation { action: 3 },
        "swingHand(EquipmentSlot)" => match arg(0)? {
            "HAND" => PluginPacket::Animation { action: 0 },
            "OFF_HAND" => PluginPacket::Animation { action: 3 },
            _ => return None,
        },
        "updateInventory()" => PluginPacket::ResyncInventory,
        "sendOpLevel(byte)" => {
            let level: u8 = arg(0)?.parse().ok().filter(|level| *level <= 4)?;
            PluginPacket::EntityEvent {
                entity_id: super::facts::entity_id(player)?,
                status: 24 + level,
            }
        }
        "playEffect(EntityEffect)" => PluginPacket::EntityEvent {
            entity_id: super::facts::entity_id(player)?,
            status: entity_status(arg(0)?)?,
        },
        "sendEntityEffect(EntityEffect,Entity)" => PluginPacket::EntityEvent {
            entity_id: super::facts::entity_id(Uuid::parse_str(arg(1)?).ok()?)?,
            status: entity_status(arg(0)?)?,
        },
        "playLevelEvent(Location,int,int)" => PluginPacket::LevelEvent {
            event: arg(1)?.parse().ok().filter(|event| *event >= 0)?,
            pos: block_pos(arg(0)?)?,
            data: arg(2)?.parse().ok()?,
        },
        "playHurtAnimation(float)" => {
            let yaw: f32 = arg(0)?.parse().ok().filter(|yaw: &f32| yaw.is_finite())?;
            PluginPacket::HurtAnimation { yaw }
        }
        "setCompassTarget(Location)" => PluginPacket::CompassTarget {
            pos: block_pos(arg(0)?)?,
        },
        "removeResourcePack(UUID)" => PluginPacket::ResourcePackPop {
            id: Some(Uuid::parse_str(arg(0)?).ok()?),
        },
        "removeResourcePacks()" | "clearResourcePacks()" => {
            PluginPacket::ResourcePackPop { id: None }
        }
        method
            if method.starts_with("setResourcePack(") || method.starts_with("addResourcePack(") =>
        {
            return resource_pack(player, call);
        }
        _ => return None,
    };
    Some(PlayerCommand::Packet { player, packet })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYER: &str = "00000000-0000-0000-0000-00000000000d";

    fn call(method: &str, args: &[&str]) -> HostCall {
        HostCall {
            owner: "Player".into(),
            subject: PLAYER.into(),
            method: method.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    fn packet(call: HostCall) -> Option<PluginPacket> {
        match network_command(&call)? {
            PlayerCommand::Packet { packet, .. } => Some(packet),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn hide_and_show_become_visibility_commands() {
        let other = "00000000-0000-0000-0000-0000000000aa";
        let command = |method: &str, args: &[&str]| network_command(&call(method, args));
        assert!(matches!(
            command("hidePlayer(Player)", &[other]),
            Some(PlayerCommand::Visibility { hidden: true, target, .. }) if target == Uuid::parse_str(other).unwrap()
        ));
        assert!(matches!(
            command("showEntity(Plugin,Entity)", &["plugin", other]),
            Some(PlayerCommand::Visibility { hidden: false, .. })
        ));
        // A player cannot hide themselves, and a bad UUID is no command.
        assert!(command("hidePlayer(Player)", &[PLAYER]).is_none());
        assert!(command("hidePlayer(Player)", &["nope"]).is_none());
    }

    fn location(x: &str, y: &str, z: &str) -> String {
        format!("world{PART}{x}{PART}{y}{PART}{z}{PART}0{PART}0")
    }

    #[test]
    fn block_changes_use_the_default_state_of_the_block() {
        let stone = ferrite_data::blocks::default_state("stone").unwrap();
        for (method, args) in [
            (
                "sendBlockChange(Location,BlockData)",
                vec![location("10.7", "64", "-3.2"), "minecraft:stone".to_owned()],
            ),
            (
                "sendBlockChange(Location,Material,byte)",
                vec![
                    location("10.7", "64", "-3.2"),
                    "STONE".to_owned(),
                    "0".to_owned(),
                ],
            ),
        ] {
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            assert_eq!(
                packet(call(method, &refs)),
                Some(PluginPacket::BlockChange {
                    pos: [10, 64, -4],
                    state: u32::from(stone)
                }),
                "{method}"
            );
        }
    }

    #[test]
    fn block_changes_reject_unknown_blocks_and_bad_coordinates() {
        assert_eq!(
            packet(call(
                "sendBlockChange(Location,BlockData)",
                &[&location("0", "0", "0"), "minecraft:not_a_block"]
            )),
            None
        );
        assert_eq!(
            packet(call(
                "sendBlockChange(Location,BlockData)",
                &[&location("NaN", "0", "0"), "stone"]
            )),
            None
        );
        assert_eq!(
            packet(call(
                "sendBlockChange(Location,BlockData)",
                &[&location("1e9", "0", "0"), "stone"]
            )),
            None
        );
        assert_eq!(
            packet(call(
                "sendBlockChange(Location,BlockData)",
                &[&location("0", "0", "0"), "stone; op"]
            )),
            None
        );
    }

    #[test]
    fn plugin_messages_carry_a_channel_and_bytes() {
        let data = base64::engine::general_purpose::STANDARD.encode([1u8, 2, 3]);
        assert_eq!(
            packet(call(
                "sendPluginMessage(Plugin,String,byte[])",
                &["Plug", "BungeeCord", &data]
            )),
            Some(PluginPacket::PluginMessage {
                channel: "bungeecord:main".into(),
                data: vec![1, 2, 3]
            })
        );
        assert_eq!(
            packet(call(
                "sendPluginMessage(Plugin,String,byte[])",
                &["Plug", "my:chan", &data]
            )),
            Some(PluginPacket::PluginMessage {
                channel: "my:chan".into(),
                data: vec![1, 2, 3]
            })
        );
        for bad in ["", "nochannel", "Bad:Chan", "a:b c", ":x", "x:"] {
            assert_eq!(
                packet(call(
                    "sendPluginMessage(Plugin,String,byte[])",
                    &["Plug", bad, &data]
                )),
                None,
                "{bad:?}"
            );
        }
        assert_eq!(
            packet(call(
                "sendPluginMessage(Plugin,String,byte[])",
                &["Plug", "a:b", "!!!"]
            )),
            None
        );
    }

    #[test]
    fn transfers_validate_host_and_port() {
        assert_eq!(
            packet(call("transfer(String,int)", &["play.example.com", "25566"])),
            Some(PluginPacket::Transfer {
                host: "play.example.com".into(),
                port: 25566
            })
        );
        for (host, port) in [
            ("", "25565"),
            ("a b", "25565"),
            ("host", "0"),
            ("host", "70000"),
            ("host", "-1"),
            ("ho\nst", "1"),
        ] {
            assert_eq!(
                packet(call("transfer(String,int)", &[host, port])),
                None,
                "{host:?} {port:?}"
            );
        }
    }

    #[test]
    fn resource_packs_push_with_a_hex_hash_and_pop_by_id() {
        let hash = base64::engine::general_purpose::STANDARD.encode([0xabu8; 20]);
        let Some(PluginPacket::ResourcePackPush {
            url,
            hash,
            required,
            ..
        }) = packet(call(
            "setResourcePack(String,byte[],String,boolean)",
            &[
                "https://example.com/pack.zip",
                &hash,
                "Please accept",
                "true",
            ],
        ))
        else {
            panic!("expected a push");
        };
        assert_eq!(
            (url.as_str(), hash.as_str(), required),
            (
                "https://example.com/pack.zip",
                "abababababababababababababababababababab",
                true
            )
        );
        let id = "00000000-0000-0000-0000-0000000000aa";
        assert_eq!(
            packet(call("removeResourcePack(UUID)", &[id])),
            Some(PluginPacket::ResourcePackPop {
                id: Some(Uuid::parse_str(id).unwrap())
            })
        );
        assert_eq!(
            packet(call("removeResourcePacks()", &[])),
            Some(PluginPacket::ResourcePackPop { id: None })
        );
        for bad in [
            "ftp://x/pack.zip",
            "https://x/pa ck.zip",
            "javascript:alert(1)",
        ] {
            assert_eq!(
                packet(call(
                    "setResourcePack(String,byte[],String,boolean)",
                    &[bad, "", "", "false"]
                )),
                None,
                "{bad}"
            );
        }
        let short = base64::engine::general_purpose::STANDARD.encode([1u8; 3]);
        assert_eq!(
            packet(call(
                "setResourcePack(String,byte[],String,boolean)",
                &["https://x/p.zip", &short, "", "false"]
            )),
            None
        );
    }

    #[test]
    fn effects_resync_and_world_events_become_packets() {
        assert_eq!(
            packet(call("updateInventory()", &[])),
            Some(PluginPacket::ResyncInventory)
        );
        assert_eq!(packet(call("sendOpLevel(byte)", &["9"])), None);
        assert_eq!(
            packet(call("playEffect(EntityEffect)", &["WOLF_HEARTS"])),
            None
        );
        assert_eq!(
            packet(call(
                "playLevelEvent(Location,int,int)",
                &[&location("1", "64", "2"), "1000", "0"]
            )),
            Some(PluginPacket::LevelEvent {
                event: 1000,
                pos: [1, 64, 2],
                data: 0
            })
        );
        assert_eq!(
            packet(call(
                "playLevelEvent(Location,int,int)",
                &[&location("1", "64", "2"), "-1", "0"]
            )),
            None
        );
    }

    #[test]
    fn swings_and_hurt_tilts_are_animations() {
        assert_eq!(
            packet(call("swingMainHand()", &[])),
            Some(PluginPacket::Animation { action: 0 })
        );
        assert_eq!(
            packet(call("swingOffHand()", &[])),
            Some(PluginPacket::Animation { action: 3 })
        );
        assert_eq!(
            packet(call("swingHand(EquipmentSlot)", &["OFF_HAND"])),
            Some(PluginPacket::Animation { action: 3 })
        );
        assert_eq!(packet(call("swingHand(EquipmentSlot)", &["HEAD"])), None);
        assert_eq!(
            packet(call("playHurtAnimation(float)", &["90"])),
            Some(PluginPacket::HurtAnimation { yaw: 90.0 })
        );
        assert_eq!(packet(call("playHurtAnimation(float)", &["NaN"])), None);
    }

    #[test]
    fn compass_target_points_at_the_block() {
        assert_eq!(
            packet(call(
                "setCompassTarget(Location)",
                &[&location("5.5", "70", "-9.5")]
            )),
            Some(PluginPacket::CompassTarget { pos: [5, 70, -10] })
        );
    }
}
