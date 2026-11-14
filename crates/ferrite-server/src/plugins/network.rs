//! Packets a plugin sends to one player: fake block changes, plugin messages, transfers,
//! resource packs and the compass target.
//!
//! Everything arrives as text from plugin code, so each value is validated before it can become
//! a packet: keys and hosts are character-checked, sizes are capped, and coordinates must be
//! finite. A call that fails validation is left unhandled, so the plugin gets the usual
//! "not implemented" error rather than a silent no-op.

use base64::Engine as _;
use ferrite_plugin::{BossBarChange, HostCall, PlayerCommand, PluginPacket, ScoreboardStep};
use uuid::Uuid;

const PART: char = '\u{1d}';
const NULL_ARG: &str = "\u{1b}null";
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
    let mut prompt: Option<String> = None;
    for (kind, value) in types.iter().zip(&call.args) {
        match *kind {
            "UUID" => id = Some(Uuid::parse_str(value).ok()?),
            "String" if link.is_none() => link = Some(url(value)?.to_owned()),
            "byte[]" => hash = pack_hash(value)?,
            "boolean" => required = value == "true",
            // The prompt: a Component arrives as JSON, a String as legacy-formatted text
            // (`CraftChatMessage.fromStringOrNull`); null or empty means none.
            "Component" if value != NULL_ARG => {
                ferrite_text::Component::from_json(value).ok()?;
                prompt = Some(value.clone());
            }
            "String" if value != NULL_ARG && !value.is_empty() => {
                prompt = Some(ferrite_text::Component::from_legacy(value).to_json());
            }
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
            prompt,
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

/// An objective name: vanilla allows 16 characters from a small set.
fn sb_name(text: &str) -> Option<String> {
    (!text.is_empty()
        && text.chars().count() <= 16
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-.+".contains(c)))
    .then(|| text.to_owned())
}

/// A score holder: a player name or any text of at most 40 characters.
fn sb_owner(text: &str) -> Option<String> {
    (!text.is_empty() && text.chars().count() <= 40).then(|| text.to_owned())
}

/// A boss bar's fill, between empty and full.
fn boss_progress(text: &str) -> Option<f32> {
    text.parse::<f32>()
        .ok()
        .filter(|p| p.is_finite() && (0.0..=1.0).contains(p))
}

/// A colour or overlay ordinal, at most `max`.
fn boss_style(text: &str, max: i32) -> Option<i32> {
    text.parse::<i32>().ok().filter(|v| (0..=max).contains(v))
}

/// The darken / music / fog bits of a boss bar.
fn boss_flags(text: &str) -> Option<u8> {
    text.parse::<u8>().ok().filter(|f| *f <= 7)
}

/// The registry id of a `minecraft:` mob effect key.
fn mob_effect(key: &str) -> Option<i32> {
    ferrite_data::registries::id("minecraft:mob_effect", key)
}

/// The dye names a sign's text colour may have.
const SIGN_COLORS: [&str; 16] = [
    "white",
    "orange",
    "magenta",
    "light_blue",
    "yellow",
    "lime",
    "pink",
    "gray",
    "light_gray",
    "cyan",
    "purple",
    "blue",
    "brown",
    "green",
    "red",
    "black",
];

/// The wire slot of a Bukkit `EquipmentSlot`.
fn equipment_slot(name: &str) -> Option<u8> {
    Some(match name {
        "HAND" => 0,
        "OFF_HAND" => 1,
        "FEET" => 2,
        "LEGS" => 3,
        "CHEST" => 4,
        "HEAD" => 5,
        _ => return None,
    })
}

/// An encoded `ItemStack` (`MATERIAL<ITEM>amount`); `Some(None)` is an empty stack, `None` is invalid.
fn item_snapshot(text: &str) -> Option<Option<ferrite_plugin::ItemSnapshot>> {
    if text == NULL_ARG {
        return Some(None);
    }
    let (material, amount) = text.split_once(PART)?;
    let amount: u8 = amount.parse().ok().filter(|a| (1..=99).contains(a))?;
    if material == "AIR" {
        return Some(None);
    }
    let key = format!("minecraft:{}", material.to_ascii_lowercase());
    ferrite_item::item_id(&key)?;
    Some(Some(ferrite_plugin::ItemSnapshot {
        material: key,
        amount,
    }))
}

/// Offers separated by `\u{1e}`, each with its fields separated by `\u{1c}`:
/// `gives, wants, second wants or null, uses, max uses, xp, price multiplier`.
fn merchant_trades(text: &str) -> Option<Vec<ferrite_plugin::MerchantTrade>> {
    const OFFER: char = '\u{1e}';
    const FIELD: char = '\u{1c}';
    let mut trades = Vec::new();
    for offer in text.split(OFFER).filter(|o| !o.is_empty()) {
        let mut fields = offer.split(FIELD);
        let gives = item_snapshot(fields.next()?)??;
        let wants = item_snapshot(fields.next()?)??;
        let second_wants = item_snapshot(fields.next()?)?;
        let uses: i32 = fields.next()?.parse().ok().filter(|u| *u >= 0)?;
        let max_uses: i32 = fields.next()?.parse().ok().filter(|u| *u > 0)?;
        let xp: i32 = fields.next()?.parse().ok().filter(|x| *x >= 0)?;
        let price_multiplier: f32 = fields
            .next()?
            .parse()
            .ok()
            .filter(|m: &f32| m.is_finite() && (0.0..=100.0).contains(m))?;
        trades.push(ferrite_plugin::MerchantTrade {
            gives,
            wants,
            second_wants,
            uses,
            max_uses,
            xp,
            price_multiplier,
        });
        if trades.len() > 64 {
            return None;
        }
    }
    (!trades.is_empty()).then_some(trades)
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
    if call.method == "updateCommands()" {
        return Some(PlayerCommand::UpdateCommands { player });
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
        "swingHand(EquipmentSlot)" | "swingHand(EquipmentSlot,SwingAnimation)" => match arg(0)? {
            "HAND" => PluginPacket::Animation { action: 0 },
            "OFF_HAND" => PluginPacket::Animation { action: 3 },
            _ => return None,
        },
        "sendPotionEffectChange(LivingEntity,PotionEffect)" => {
            let mut parts = arg(1)?.split(PART);
            let effect = mob_effect(parts.next()?)?;
            let duration: i32 = parts.next()?.parse().ok().filter(|d: &i32| *d >= -1)?;
            let amplifier: i32 = parts
                .next()?
                .parse()
                .ok()
                .filter(|a: &i32| (0..=255).contains(a))?;
            let flag = |part: Option<&str>| part.map(|p| p == "true");
            let ambient = flag(parts.next())?;
            let particles = flag(parts.next())?;
            let icon = flag(parts.next())?;
            PluginPacket::PotionEffect {
                entity_id: super::facts::entity_id(Uuid::parse_str(arg(0)?).ok()?)?,
                effect,
                amplifier,
                duration,
                flags: u8::from(ambient) | (u8::from(particles) << 1) | (u8::from(icon) << 2),
            }
        }
        "sendPotionEffectChangeRemove(LivingEntity,PotionEffectType)" => {
            PluginPacket::PotionRemove {
                entity_id: super::facts::entity_id(Uuid::parse_str(arg(0)?).ok()?)?,
                effect: mob_effect(arg(1)?)?,
            }
        }
        "sendEquipmentChange(LivingEntity,EquipmentSlot,ItemStack)" => PluginPacket::Equipment {
            entity_id: super::facts::entity_id(Uuid::parse_str(arg(0)?).ok()?)?,
            slot: equipment_slot(arg(1)?)?,
            item: item_snapshot(arg(2)?)?,
        },
        "sendSignChange(Location,String[],DyeColor,boolean)" => {
            let mut lines: [String; 4] = Default::default();
            let mut given = arg(1)?.split(PART);
            for line in &mut lines {
                let text = given.next().unwrap_or("");
                if text != NULL_ARG {
                    *line = text.chars().filter(|c| !c.is_control()).take(384).collect();
                }
            }
            let color = arg(2)?.to_ascii_lowercase();
            if !SIGN_COLORS.contains(&color.as_str()) {
                return None;
            }
            PluginPacket::SignText {
                pos: block_pos(arg(0)?)?,
                lines,
                color,
                glowing: arg(3)? == "true",
            }
        }
        // Entries: `item,<slot>,<material>,<count>` and `furnace,<burn>,<burn total>,<cook>,<cook total>`.
        "sendTileSnapshot(Location,String,List)" => {
            super::tile_update::packet(block_pos(arg(0)?)?, arg(1)?, arg(2)?, PART)?
        }
        "sendLinks(ServerLinks)" => {
            let mut links = Vec::new();
            for entry in arg(0)?.split(PART).filter(|e| !e.is_empty()).take(100) {
                // `KNOWN:<index>` or `CUSTOM:<label>`, a tab, then the URL.
                let (head, url) = entry.split_once('\t')?;
                if !(url.starts_with("http://") || url.starts_with("https://")) || url.len() > 4096
                {
                    return None;
                }
                let (known, label) = match head.split_once(':')? {
                    ("KNOWN", index) => (
                        Some(index.parse::<i32>().ok().filter(|i| (0..=9).contains(i))?),
                        String::new(),
                    ),
                    ("CUSTOM", label) => (
                        None,
                        label
                            .chars()
                            .filter(|c| !c.is_control())
                            .take(256)
                            .collect(),
                    ),
                    _ => return None,
                };
                links.push((known, label, url.to_owned()));
            }
            PluginPacket::ServerLinks(links)
        }
        "updateInventory()" => PluginPacket::ResyncInventory,
        "wakeup(boolean)" => PluginPacket::WakeUp,
        "dropSelected(boolean)" => PluginPacket::DropSelected {
            all: arg(0)? == "true",
        },
        "dropStack(ItemStack)" => PluginPacket::DropStack {
            item: item_snapshot(arg(0)?)??,
        },
        "sbObjective(String,int,String,int)" => {
            PluginPacket::Scoreboard(ScoreboardStep::Objective {
                name: sb_name(arg(0)?)?,
                method: arg(1)?.parse::<u8>().ok().filter(|m| *m <= 2)?,
                display: arg(2)?.chars().take(256).collect(),
                hearts: arg(3)? == "1",
            })
        }
        "sbDisplay(int,String)" => PluginPacket::Scoreboard(ScoreboardStep::Display {
            slot: arg(0)?
                .parse::<i32>()
                .ok()
                .filter(|s| (0..=2).contains(s))?,
            name: if arg(1)?.is_empty() {
                String::new()
            } else {
                sb_name(arg(1)?)?
            },
        }),
        "sbScore(String,String,int)" => PluginPacket::Scoreboard(ScoreboardStep::Score {
            owner: sb_owner(arg(0)?)?,
            objective: sb_name(arg(1)?)?,
            score: arg(2)?.parse().ok()?,
        }),
        "sbReset(String,String)" => PluginPacket::Scoreboard(ScoreboardStep::Reset {
            owner: sb_owner(arg(0)?)?,
            objective: if arg(1)?.is_empty() {
                String::new()
            } else {
                sb_name(arg(1)?)?
            },
        }),
        "sbTeam(String,int,String,String)" => {
            let name = arg(0)?;
            if name.is_empty() || name.chars().count() > 16 || name.chars().any(char::is_control) {
                return None;
            }
            let method = arg(1)?.parse::<u8>().ok().filter(|m| *m <= 4)?;
            let params: Vec<String> = if method == 0 || method == 2 {
                let parts: Vec<&str> = arg(2)?.split(PART).collect();
                let [display, options, tag, collision, color, prefix, suffix] = parts[..] else {
                    return None;
                };
                options.parse::<u8>().ok().filter(|o| *o <= 3)?;
                tag.parse::<u8>().ok().filter(|v| *v <= 3)?;
                collision.parse::<u8>().ok().filter(|v| *v <= 3)?;
                color
                    .parse::<i32>()
                    .ok()
                    .filter(|c| (-1..=15).contains(c))?;
                [display, options, tag, collision, color, prefix, suffix]
                    .iter()
                    .map(|text| text.chars().filter(|c| !c.is_control()).take(256).collect())
                    .collect()
            } else {
                Vec::new()
            };
            let players: Vec<String> = arg(3)?
                .split(PART)
                .filter(|player| !player.is_empty())
                .map(sb_owner)
                .collect::<Option<_>>()?;
            PluginPacket::Scoreboard(ScoreboardStep::Team {
                name: name.to_owned(),
                method,
                params,
                players,
            })
        }
        "bossBarAdd(String,String,float,int,int,int)" => {
            PluginPacket::BossBar(BossBarChange::Add {
                id: Uuid::parse_str(arg(0)?).ok()?,
                name: arg(1)?.chars().take(256).collect(),
                progress: boss_progress(arg(2)?)?,
                color: boss_style(arg(3)?, 6)?,
                overlay: boss_style(arg(4)?, 4)?,
                flags: boss_flags(arg(5)?)?,
            })
        }
        "bossBarRemove(String)" => PluginPacket::BossBar(BossBarChange::Remove {
            id: Uuid::parse_str(arg(0)?).ok()?,
        }),
        "bossBarProgress(String,float)" => PluginPacket::BossBar(BossBarChange::Progress {
            id: Uuid::parse_str(arg(0)?).ok()?,
            progress: boss_progress(arg(1)?)?,
        }),
        "bossBarName(String,String)" => PluginPacket::BossBar(BossBarChange::Name {
            id: Uuid::parse_str(arg(0)?).ok()?,
            name: arg(1)?.chars().take(256).collect(),
        }),
        "bossBarStyle(String,int,int)" => PluginPacket::BossBar(BossBarChange::Style {
            id: Uuid::parse_str(arg(0)?).ok()?,
            color: boss_style(arg(1)?, 6)?,
            overlay: boss_style(arg(2)?, 4)?,
        }),
        "bossBarFlags(String,int)" => PluginPacket::BossBar(BossBarChange::Flags {
            id: Uuid::parse_str(arg(0)?).ok()?,
            flags: boss_flags(arg(1)?)?,
        }),
        "openMerchant(String,String)" => PluginPacket::OpenMerchant {
            title: arg(0)?
                .chars()
                .filter(|c| !c.is_control())
                .take(256)
                .collect(),
            trades: merchant_trades(arg(1)?)?,
        },
        "openSign(Location,Side)" => PluginPacket::OpenSign {
            pos: block_pos(arg(0)?)?,
            front: match arg(1)? {
                "FRONT" => true,
                "BACK" => false,
                _ => return None,
            },
        },
        "retrieveCookie(NamespacedKey)" => {
            let key = arg(0)?.to_ascii_lowercase();
            let valid = !key.is_empty()
                && key.len() <= 256
                && key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_-./:".contains(c));
            if !valid {
                return None;
            }
            PluginPacket::RetrieveCookie { key }
        }
        "storeCookie(NamespacedKey,String)" => {
            use base64::Engine;
            let key = arg(0)?.to_ascii_lowercase();
            let valid_key = !key.is_empty()
                && key.len() <= 256
                && key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_-./:".contains(c));
            // Vanilla limits a cookie to 5120 bytes.
            let payload = base64::engine::general_purpose::STANDARD
                .decode(arg(1)?)
                .ok()
                .filter(|bytes| bytes.len() <= 5120)?;
            if !valid_key {
                return None;
            }
            PluginPacket::StoreCookie { key, payload }
        }
        "setWindowProperty(int,int)" => PluginPacket::WindowProperty {
            id: arg(0)?.parse().ok().filter(|id| *id >= 0)?,
            value: arg(1)?.parse().ok()?,
        },
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
        "playPickupItemAnimation(Item,int)" => {
            let item = Uuid::parse_str(arg(0)?).ok()?;
            PluginPacket::PickupAnimation {
                item: super::facts::fact(item)?.entity_id,
                amount: arg(1)?.parse().ok().filter(|amount| *amount > 0)?,
            }
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
    fn fake_potion_effects_use_registry_ids_and_flags() {
        let player = Uuid::parse_str(PLAYER).unwrap();
        let effect = format!("minecraft:speed{PART}100{PART}2{PART}false{PART}true{PART}true");
        let send = || {
            packet(call(
                "sendPotionEffectChange(LivingEntity,PotionEffect)",
                &[PLAYER, &effect],
            ))
        };
        // Without a recorded player there is no entity id to address.
        assert!(send().is_none());
        super::super::facts::record(
            player,
            super::super::facts::PlayerFacts {
                entity_id: 41,
                joined: std::time::SystemTime::now(),
                address: None,
            },
        );
        assert_eq!(
            send(),
            Some(PluginPacket::PotionEffect {
                entity_id: 41,
                effect: 0,
                amplifier: 2,
                duration: 100,
                flags: 0b110
            })
        );
        assert_eq!(
            packet(call(
                "sendPotionEffectChangeRemove(LivingEntity,PotionEffectType)",
                &[PLAYER, "minecraft:speed"]
            )),
            Some(PluginPacket::PotionRemove {
                entity_id: 41,
                effect: 0
            })
        );
        assert!(
            packet(call(
                "sendPotionEffectChangeRemove(LivingEntity,PotionEffectType)",
                &[PLAYER, "minecraft:nope"]
            ))
            .is_none()
        );
        super::super::facts::forget(player);
    }

    #[test]
    fn equipment_items_and_slots_are_validated() {
        assert_eq!(equipment_slot("HEAD"), Some(5));
        assert_eq!(equipment_slot("BODY"), None);
        assert_eq!(
            item_snapshot(&format!("DIAMOND_HELMET{PART}1")),
            Some(Some(ferrite_plugin::ItemSnapshot {
                material: "minecraft:diamond_helmet".into(),
                amount: 1
            }))
        );
        assert_eq!(item_snapshot(&format!("AIR{PART}1")), Some(None));
        assert_eq!(item_snapshot(NULL_ARG), Some(None));
        assert_eq!(item_snapshot(&format!("NOT_AN_ITEM{PART}1")), None);
        assert_eq!(item_snapshot(&format!("STONE{PART}0")), None);
    }

    #[test]
    fn sign_text_keeps_four_clean_lines_and_a_known_colour() {
        let lines = ["one", "two\u{7}x", "\u{1b}null"].join(&PART.to_string());
        let sign = |color: &str| {
            packet(call(
                "sendSignChange(Location,String[],DyeColor,boolean)",
                &[&location("1", "64", "2"), &lines, color, "true"],
            ))
        };
        assert_eq!(
            sign("RED"),
            Some(PluginPacket::SignText {
                pos: [1, 64, 2],
                lines: ["one".into(), "twox".into(), String::new(), String::new()],
                color: "red".into(),
                glowing: true,
            })
        );
        assert_eq!(sign("plaid"), None);
    }

    #[test]
    fn a_tile_snapshot_becomes_a_packet_for_that_player_only() {
        let entries = ["item,2,minecraft:stone,5"].join(&PART.to_string());
        let command = network_command(&call(
            "sendTileSnapshot(Location,String,List)",
            &[&location("4", "65", "-3"), "minecraft:barrel", &entries],
        ));
        let Some(PlayerCommand::Packet { player, packet }) = command else {
            panic!("a packet command");
        };
        assert_eq!(player, Uuid::parse_str(PLAYER).unwrap());
        assert_eq!(
            packet,
            PluginPacket::TileUpdate {
                pos: [4, 65, -3],
                entity: "minecraft:barrel".into(),
                items: vec![(2, "minecraft:stone".into(), 5)],
                furnace: None,
                patterns: Vec::new(),
                profile: None,
                properties: Vec::new(),
                metas: Vec::new(),
                extra: ferrite_nbt::Compound::new(),
            }
        );
        let refused = |entity: &str| {
            network_command(&call(
                "sendTileSnapshot(Location,String,List)",
                &[&location("4", "65", "-3"), entity, ""],
            ))
        };
        assert_eq!(refused("minecraft:beacon"), None);
        assert_eq!(refused("minecraft:sign_but_not_here"), None);
    }

    #[test]
    fn server_links_are_parsed_and_checked() {
        let sep = PART.to_string();
        let good = [
            "KNOWN:6\thttps://example.org",
            "CUSTOM:Wiki\thttps://wiki.example.org",
        ]
        .join(&sep);
        assert_eq!(
            packet(call("sendLinks(ServerLinks)", &[&good])),
            Some(PluginPacket::ServerLinks(vec![
                (Some(6), String::new(), "https://example.org".into()),
                (None, "Wiki".into(), "https://wiki.example.org".into()),
            ]))
        );
        assert!(
            packet(call(
                "sendLinks(ServerLinks)",
                &["KNOWN:6\tjavascript:alert(1)"]
            ))
            .is_none()
        );
        assert!(
            packet(call(
                "sendLinks(ServerLinks)",
                &["KNOWN:99\thttps://example.org"]
            ))
            .is_none()
        );
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

    /// `addResourcePack`/`setResourcePack` carry a request id, hash, prompt and `required`.
    #[test]
    fn resource_pack_overloads_carry_id_prompt_and_required() {
        let id = "00000000-0000-0000-0000-0000000000bb";
        let hash = base64::engine::general_purpose::STANDARD.encode([0x01u8; 20]);
        let component = r#"{"text":"Pick me","color":"gold"}"#;
        for method in [
            "addResourcePack(UUID,String,byte[],String,boolean)",
            "setResourcePack(UUID,String,byte[],String,boolean)",
        ] {
            let Some(PluginPacket::ResourcePackPush {
                id: got,
                required,
                prompt,
                ..
            }) = packet(call(
                method,
                &[id, "https://example.com/p.zip", &hash, "\u{a7}6Pick me", "true"],
            ))
            else {
                panic!("{method}");
            };
            assert_eq!(got, Uuid::parse_str(id).unwrap());
            assert!(required);
            assert!(prompt.unwrap().contains("Pick me"), "{method}");
        }
        let Some(PluginPacket::ResourcePackPush { prompt, .. }) = packet(call(
            "setResourcePack(UUID,String,byte[],Component,boolean)",
            &[id, "https://example.com/p.zip", &hash, component, "false"],
        )) else {
            panic!("component overload");
        };
        assert_eq!(prompt.as_deref(), Some(component));
        // A null or empty prompt offers the pack without one.
        let Some(PluginPacket::ResourcePackPush { prompt, .. }) = packet(call(
            "setResourcePack(UUID,String,byte[],String,boolean)",
            &[id, "https://example.com/p.zip", &hash, NULL_ARG, "false"],
        )) else {
            panic!("null prompt");
        };
        assert_eq!(prompt, None);
        // A prompt that is not a component is rejected rather than shown as raw JSON.
        assert!(
            packet(call(
                "setResourcePack(UUID,String,byte[],Component,boolean)",
                &[id, "https://example.com/p.zip", &hash, "{not json", "false"],
            ))
            .is_none()
        );
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
        assert_eq!(
            packet(call("setWindowProperty(int,int)", &["2", "-5"])),
            Some(PluginPacket::WindowProperty { id: 2, value: -5 })
        );
        assert_eq!(
            packet(call("setWindowProperty(int,int)", &["-1", "5"])),
            None
        );
        assert_eq!(
            packet(call("wakeup(boolean)", &["true"])),
            Some(PluginPacket::WakeUp)
        );
        assert_eq!(
            packet(call("dropSelected(boolean)", &["true"])),
            Some(PluginPacket::DropSelected { all: true })
        );
        assert_eq!(
            packet(call("dropStack(ItemStack)", &[&format!("EMERALD{PART}4")])),
            Some(PluginPacket::DropStack {
                item: ferrite_plugin::ItemSnapshot {
                    material: "minecraft:emerald".into(),
                    amount: 4
                }
            })
        );
        assert_eq!(
            packet(call("dropStack(ItemStack)", &[&format!("AIR{PART}1")])),
            None
        );
        assert!(matches!(
            network_command(&call("updateCommands()", &[])),
            Some(PlayerCommand::UpdateCommands { .. })
        ));
        assert_eq!(
            packet(call(
                "sbObjective(String,int,String,int)",
                &["kills", "0", "Kills", "0"]
            )),
            Some(PluginPacket::Scoreboard(ScoreboardStep::Objective {
                name: "kills".into(),
                method: 0,
                display: "Kills".into(),
                hearts: false
            }))
        );
        assert_eq!(
            packet(call(
                "sbObjective(String,int,String,int)",
                &["has space", "0", "x", "0"]
            )),
            None
        );
        assert_eq!(packet(call("sbDisplay(int,String)", &["5", "kills"])), None);
        assert_eq!(
            packet(call("sbScore(String,String,int)", &["Alex", "kills", "-4"])),
            Some(PluginPacket::Scoreboard(ScoreboardStep::Score {
                owner: "Alex".into(),
                objective: "kills".into(),
                score: -4
            }))
        );
        let params = ["Red", "3", "1", "3", "12", "[R] ", ""].join(&PART.to_string());
        assert_eq!(
            packet(call(
                "sbTeam(String,int,String,String)",
                &["red", "0", &params, "Alex"]
            )),
            Some(PluginPacket::Scoreboard(ScoreboardStep::Team {
                name: "red".into(),
                method: 0,
                params: ["Red", "3", "1", "3", "12", "[R] ", ""]
                    .map(String::from)
                    .to_vec(),
                players: vec!["Alex".into()],
            }))
        );
        assert_eq!(
            packet(call(
                "sbTeam(String,int,String,String)",
                &["red", "1", "", ""]
            )),
            Some(PluginPacket::Scoreboard(ScoreboardStep::Team {
                name: "red".into(),
                method: 1,
                params: vec![],
                players: vec![],
            }))
        );
        let bad = ["Red", "9", "1", "3", "12", "", ""].join(&PART.to_string());
        assert_eq!(
            packet(call(
                "sbTeam(String,int,String,String)",
                &["red", "0", &bad, ""]
            )),
            None
        );
        let bar = Uuid::from_u128(9).to_string();
        assert_eq!(
            packet(call(
                "bossBarAdd(String,String,float,int,int,int)",
                &[&bar, "Boss", "0.5", "2", "1", "3"]
            )),
            Some(PluginPacket::BossBar(BossBarChange::Add {
                id: Uuid::from_u128(9),
                name: "Boss".into(),
                progress: 0.5,
                color: 2,
                overlay: 1,
                flags: 3,
            }))
        );
        assert_eq!(
            packet(call("bossBarProgress(String,float)", &[&bar, "1.5"])),
            None
        );
        assert_eq!(
            packet(call("bossBarStyle(String,int,int)", &[&bar, "7", "0"])),
            None
        );
        assert_eq!(
            packet(call("bossBarRemove(String)", &[&bar])),
            Some(PluginPacket::BossBar(BossBarChange::Remove {
                id: Uuid::from_u128(9)
            }))
        );
        assert_eq!(
            packet(call("retrieveCookie(NamespacedKey)", &["My:Key"])),
            Some(PluginPacket::RetrieveCookie {
                key: "my:key".into()
            })
        );
        assert_eq!(
            packet(call("retrieveCookie(NamespacedKey)", &["bad key"])),
            None
        );
        assert_eq!(
            packet(call(
                "storeCookie(NamespacedKey,String)",
                &["my:key", "AQID"]
            )),
            Some(PluginPacket::StoreCookie {
                key: "my:key".into(),
                payload: vec![1, 2, 3]
            })
        );
        assert_eq!(
            packet(call(
                "storeCookie(NamespacedKey,String)",
                &["bad key", "AQID"]
            )),
            None
        );
        assert_eq!(
            packet(call(
                "storeCookie(NamespacedKey,String)",
                &["my:key", &"AAAA".repeat(2000)]
            )),
            None
        );
        let at = format!("w{0}3.5{0}64{0}-2{0}0{0}0", PART);
        assert_eq!(
            packet(call("openSign(Location,Side)", &[&at, "BACK"])),
            Some(PluginPacket::OpenSign {
                pos: [3, 64, -2],
                front: false
            })
        );
        assert_eq!(packet(call("openSign(Location,Side)", &[&at, "TOP"])), None);
        let offer = |gives: &str, uses: &str| {
            [
                format!("{gives}{PART}1"),
                format!("EMERALD{PART}3"),
                NULL_ARG.to_owned(),
                uses.to_owned(),
                "8".to_owned(),
                "2".to_owned(),
                "0.05".to_owned(),
            ]
            .join("\u{1c}")
        };
        let two = format!("{}\u{1e}{}", offer("DIAMOND", "0"), offer("STICK", "1"));
        match packet(call("openMerchant(String,String)", &["Shop", &two])) {
            Some(PluginPacket::OpenMerchant { title, trades }) => {
                assert_eq!(title, "Shop");
                assert_eq!(trades.len(), 2);
                assert_eq!(trades[0].gives.material, "minecraft:diamond");
                assert_eq!(trades[0].wants.amount, 3);
                assert_eq!(trades[1].uses, 1);
                assert_eq!(trades[1].second_wants, None);
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(
            packet(call("openMerchant(String,String)", &["Shop", ""])),
            None
        );
        assert_eq!(
            packet(call(
                "openMerchant(String,String)",
                &["Shop", &offer("NOT_AN_ITEM", "0")]
            )),
            None
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
    fn pickup_animation_names_the_item_entity_and_a_positive_count() {
        let item = Uuid::from_u128(0xfeed);
        super::super::facts::publish_for_test(item, 5150);
        let args = |count: &str| [item.to_string(), count.to_owned()];
        let pickup = |count: &str| {
            let args = args(count);
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            packet(call("playPickupItemAnimation(Item,int)", &refs))
        };
        assert_eq!(
            pickup("3"),
            Some(PluginPacket::PickupAnimation {
                item: 5150,
                amount: 3
            })
        );
        assert_eq!(pickup("0"), None);
        assert_eq!(pickup("-1"), None);
        assert_eq!(pickup("many"), None);
        // An item the host never published has no network id to name.
        let unknown = Uuid::from_u128(0xdead).to_string();
        assert_eq!(
            packet(call("playPickupItemAnimation(Item,int)", &[&unknown, "1"])),
            None
        );
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
