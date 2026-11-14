//! `Player#spawnParticle` with particle data: the option bytes of
//! `ParticleTypes.STREAM_CODEC` for every particle type that carries any.
//!
//! The Java side validates the Bukkit data object and reduces it to a short text spec (see
//! `PlayerMisc.particleSpec`); this module checks it again and writes the bytes the 26.3
//! codec of that particle type expects:
//!
//! | particle | codec | spec |
//! |---|---|---|
//! | `block`, `block_marker`, `falling_dust`, `dust_pillar`, `block_crumble` | block state id (VarInt) | `block:<state id>` |
//! | `dust` | RGB int, scale float | `dust:<rgb>:<scale>` |
//! | `dust_color_transition` | RGB int, RGB int, scale float | `transition:<from>:<to>:<scale>` |
//! | `entity_effect`, `tinted_leaves`, `flash` | ARGB int | `color:<argb>` |
//! | `effect`, `instant_effect` | ARGB int, power float | `spell:<argb>:<power>` |
//! | `dragon_breath`, `sculk_charge` | float | `float:<value>` |
//! | `shriek` | delay VarInt | `int:<ticks>` |
//! | `item` | item id, count, empty component patch | `item:<key>:<count>` |
//! | `trail` | target 3 doubles, ARGB int, duration VarInt | `trail:<x>:<y>:<z>:<argb>:<ticks>` |
//! | `vibration` | position source, arrival VarInt | `vibration:block:<x>:<y>:<z>:<ticks>` or `vibration:entity:<id>:<yoffset>:<ticks>` |
//! | `geyser`, `geyser_poof`, `geyser_plume` | water blocks int | `geyser:<n>` |
//! | `geyser_base` | water blocks int, impulse float | `geyserbase:<n>:<impulse>` |

use ferrite_plugin::{HostCall, PlayerCommand, PluginPacket};
use uuid::Uuid;

const NULL_ARG: &str = "\u{1b}null";
const METHOD: &str = "spawnParticleData(Particle,double,double,double,int,double,double,double,double,double,double,String,boolean,RandomizationType)";

/// What a particle type's options look like on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layout {
    None,
    Block,
    Dust,
    Transition,
    Color,
    Spell,
    Float,
    Delay,
    Item,
    Trail,
    Vibration,
    Geyser,
    GeyserBase,
}

fn layout(name: &str) -> Option<Layout> {
    Some(match name {
        "block" | "block_marker" | "falling_dust" | "dust_pillar" | "block_crumble" => {
            Layout::Block
        }
        "dust" => Layout::Dust,
        "dust_color_transition" => Layout::Transition,
        "entity_effect" | "tinted_leaves" | "flash" => Layout::Color,
        "effect" | "instant_effect" => Layout::Spell,
        "dragon_breath" | "sculk_charge" => Layout::Float,
        "shriek" => Layout::Delay,
        "item" => Layout::Item,
        "trail" => Layout::Trail,
        "vibration" => Layout::Vibration,
        "geyser" | "geyser_poof" | "geyser_plume" => Layout::Geyser,
        "geyser_base" => Layout::GeyserBase,
        _ => return registered(name).then_some(Layout::None),
    })
}

fn registered(name: &str) -> bool {
    ferrite_data::registries::id("minecraft:particle_type", &format!("minecraft:{name}")).is_some()
}

fn varint(out: &mut Vec<u8>, value: i32) {
    let mut v = value as u32;
    loop {
        if v & !0x7f == 0 {
            out.push(v as u8);
            return;
        }
        out.push((v & 0x7f) as u8 | 0x80);
        v >>= 7;
    }
}

fn finite(text: &str) -> Option<f64> {
    text.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// The option bytes for `particle` from a Java-side `spec`; `None` when the spec is missing,
/// malformed, or does not belong to this particle type.
pub fn options(particle: &str, spec: &str) -> Option<Vec<u8>> {
    let layout = layout(particle)?;
    let spec = if spec == NULL_ARG { "" } else { spec };
    let mut parts = spec.split(':');
    let kind = parts.next().unwrap_or("");
    let rest: Vec<&str> = parts.collect();
    let mut out = Vec::new();
    let int = |text: &str| text.parse::<i32>().ok();
    let float = |text: &str| finite(text).map(|v| v as f32);
    match (layout, kind, rest.as_slice()) {
        (Layout::None, "", []) => {}
        (Layout::Block, "block", [state]) => {
            let state = u16::try_from(state.parse::<u32>().ok()?).ok()?;
            ferrite_data::blocks::state(state)?;
            varint(&mut out, i32::from(state));
        }
        (Layout::Dust, "dust", [rgb, scale]) => {
            out.extend(int(rgb)?.to_be_bytes());
            out.extend(float(scale)?.clamp(0.01, 4.0).to_be_bytes());
        }
        (Layout::Transition, "transition", [from, to, scale]) => {
            out.extend(int(from)?.to_be_bytes());
            out.extend(int(to)?.to_be_bytes());
            out.extend(float(scale)?.clamp(0.01, 4.0).to_be_bytes());
        }
        (Layout::Color, "color", [argb]) => out.extend(int(argb)?.to_be_bytes()),
        (Layout::Spell, "spell", [argb, power]) => {
            out.extend(int(argb)?.to_be_bytes());
            out.extend(float(power)?.to_be_bytes());
        }
        (Layout::Float, "float", [value]) => out.extend(float(value)?.to_be_bytes()),
        (Layout::Delay, "int", [delay]) => varint(&mut out, int(delay).filter(|d| *d >= 0)?),
        (Layout::Item, "item", rest) if rest.len() >= 2 => {
            // The key may carry a namespace, so the count is the last part.
            let (count, key) = rest.split_last()?;
            let key = key.join(":");
            let key = if key.contains(':') {
                key
            } else {
                format!("minecraft:{key}")
            };
            let id = ferrite_data::registries::id("minecraft:item", &key)?;
            let count = int(count).filter(|c| (1..=99).contains(c))?;
            // `ItemStackTemplate.STREAM_CODEC`: item, count, component patch. The particle
            // only shows the model, so the patch is empty (nothing added, nothing removed).
            varint(&mut out, id);
            varint(&mut out, count);
            varint(&mut out, 0);
            varint(&mut out, 0);
        }
        (Layout::Trail, "trail", [x, y, z, argb, ticks]) => {
            for axis in [x, y, z] {
                out.extend(finite(axis)?.to_be_bytes());
            }
            out.extend(int(argb)?.to_be_bytes());
            varint(&mut out, int(ticks).filter(|t| *t >= 0)?);
        }
        (Layout::Vibration, "vibration", ["block", x, y, z, ticks]) => {
            varint(&mut out, 0);
            let pos = ferrite_protocol::types::BlockPos::new(int(x)?, int(y)?, int(z)?);
            out.extend(pos.pack().to_be_bytes());
            varint(&mut out, int(ticks).filter(|t| *t >= 0)?);
        }
        (Layout::Vibration, "vibration", ["entity", id, y_offset, ticks]) => {
            varint(&mut out, 1);
            let entity = super::facts::entity_id(Uuid::parse_str(id).ok()?)?;
            varint(&mut out, entity);
            out.extend(float(y_offset)?.to_be_bytes());
            varint(&mut out, int(ticks).filter(|t| *t >= 0)?);
        }
        (Layout::Geyser, "geyser", [water]) => out.extend(int(water)?.to_be_bytes()),
        (Layout::GeyserBase, "geyserbase", [water, impulse]) => {
            out.extend(int(water)?.to_be_bytes());
            out.extend(float(impulse)?.to_be_bytes());
        }
        _ => return None,
    }
    Some(out)
}

/// `spawnParticleData(Particle,x,y,z,count,offX,offY,offZ,speedX,speedY,speedZ,spec,force,
/// RandomizationType)`: the 26.3 `ClientboundLevelParticlesPacket` for one player.
pub fn particle_command(call: &HostCall) -> Option<PlayerCommand> {
    if call.owner != "Player" || call.method != METHOD {
        return None;
    }
    let player = Uuid::parse_str(&call.subject).ok()?;
    let arg = |i: usize| call.args.get(i).map(String::as_str);
    let key = arg(0)?.to_ascii_lowercase();
    let name = key.strip_prefix("minecraft:").unwrap_or(&key);
    if !name.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
        return None;
    }
    let id = ferrite_data::registries::id("minecraft:particle_type", &format!("minecraft:{name}"))?;
    let data = options(name, arg(11)?)?;
    let position = |i: usize| finite(arg(i)?).filter(|v| v.abs() < 3.0e7);
    let small = |i: usize| finite(arg(i)?).map(|v| v as f32);
    let count: i32 = arg(4)?.parse().ok().filter(|c| (0..=100_000).contains(c))?;
    let randomization = match arg(13)? {
        "DEFAULT" => 0,
        "ALTERNATIVE" => 1,
        "ALTERNATIVE_WITH_SPEED" => 2,
        _ => return None,
    };
    let force = arg(12)? == "true";
    Some(PlayerCommand::Packet {
        player,
        packet: PluginPacket::Particle {
            particle: id,
            options: data,
            force,
            pos: [position(1)?, position(2)?, position(3)?],
            spread: [small(5)?, small(6)?, small(7)?],
            speed: [small(8)?, small(9)?, small(10)?],
            count,
            randomization,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYER: &str = "00000000-0000-0000-0000-00000000000d";

    fn call(particle: &str, spec: &str, count: &str, randomization: &str) -> HostCall {
        let args = [
            particle,
            "1.5",
            "64",
            "-2.5",
            count,
            "0.1",
            "0.2",
            "0.3",
            "0.5",
            "0.5",
            "0.5",
            spec,
            "true",
            randomization,
        ];
        HostCall {
            owner: "Player".into(),
            subject: PLAYER.into(),
            method: METHOD.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    fn packet(call: HostCall) -> Option<PluginPacket> {
        match particle_command(&call)? {
            PlayerCommand::Packet { packet, .. } => Some(packet),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn data_free_particles_have_no_option_bytes() {
        assert_eq!(options("flame", ""), Some(vec![]));
        assert_eq!(options("flame", NULL_ARG), Some(vec![]));
        // Data a particle does not take is refused, as Bukkit refuses the wrong data class.
        assert_eq!(options("flame", "color:-1"), None);
        assert_eq!(options("not_a_particle", ""), None);
    }

    #[test]
    fn every_data_particle_needs_its_own_data() {
        assert_eq!(options("dust", ""), None);
        assert_eq!(options("dust", "color:5"), None);
        let dust = options("dust", "dust:16711680:1.5").unwrap();
        assert_eq!(dust[..4], 0x00ff_0000_i32.to_be_bytes());
        assert_eq!(dust[4..], 1.5_f32.to_be_bytes());
        assert_eq!(options("dust", "dust:1:NaN"), None);
        // Dust scale is limited to 0.01..4 like `ScalableParticleOptionsBase`.
        let big = options("dust", "dust:1:99").unwrap();
        assert_eq!(big[4..], 4.0_f32.to_be_bytes());
        assert_eq!(
            options("dust_color_transition", "transition:1:2:1")
                .unwrap()
                .len(),
            12
        );
        assert_eq!(
            options("entity_effect", "color:-65536").unwrap(),
            (-65536_i32).to_be_bytes()
        );
        assert_eq!(options("tinted_leaves", "color:1").unwrap().len(), 4);
        assert_eq!(options("effect", "spell:-1:0.5").unwrap().len(), 8);
        assert_eq!(
            options("sculk_charge", "float:1.0").unwrap(),
            1.0_f32.to_be_bytes()
        );
        assert_eq!(options("dragon_breath", "float:0.5").unwrap().len(), 4);
        assert_eq!(options("shriek", "int:300").unwrap(), vec![0xac, 0x02]);
        assert_eq!(options("shriek", "int:-1"), None);
    }

    #[test]
    fn block_and_item_particles_use_registry_ids() {
        let stone = ferrite_data::blocks::default_state("minecraft:stone").unwrap();
        for name in [
            "block",
            "falling_dust",
            "block_marker",
            "dust_pillar",
            "block_crumble",
        ] {
            let bytes = options(name, &format!("block:{stone}")).unwrap();
            let mut expected = Vec::new();
            varint(&mut expected, i32::from(stone));
            assert_eq!(bytes, expected, "{name}");
        }
        assert_eq!(options("block", "block:65535"), None);
        let item = ferrite_data::registries::id("minecraft:item", "minecraft:diamond").unwrap();
        let mut expected = Vec::new();
        varint(&mut expected, item);
        varint(&mut expected, 3);
        expected.extend([0, 0]);
        assert_eq!(
            options("item", "item:minecraft:diamond:3").unwrap(),
            expected
        );
        assert_eq!(options("item", "item:diamond:3").unwrap(), expected);
        assert_eq!(options("item", "item:minecraft:nope:1"), None);
        assert_eq!(options("item", "item:diamond:0"), None);
    }

    #[test]
    fn trail_vibration_and_geyser_follow_their_codecs() {
        let trail = options("trail", "trail:1:2:3:-1:40").unwrap();
        assert_eq!(trail.len(), 24 + 4 + 1);
        assert_eq!(trail[..8], 1.0_f64.to_be_bytes());
        let block = options("vibration", "vibration:block:1:64:-3:20").unwrap();
        assert_eq!(block[0], 0);
        assert_eq!(block.len(), 1 + 8 + 1);
        assert_eq!(
            block[1..9],
            ferrite_protocol::types::BlockPos::new(1, 64, -3)
                .pack()
                .to_be_bytes()
        );
        // An entity source needs an entity the host knows.
        assert_eq!(
            options("vibration", &format!("vibration:entity:{PLAYER}:1.5:20")),
            None
        );
        assert_eq!(options("geyser", "geyser:3").unwrap(), 3_i32.to_be_bytes());
        assert_eq!(options("geyser_base", "geyserbase:3:0.5").unwrap().len(), 8);
        assert_eq!(options("geyser", "geyserbase:3:0.5"), None);
    }

    #[test]
    fn the_packet_carries_position_spread_speeds_count_and_randomization() {
        let Some(PluginPacket::Particle {
            particle,
            options,
            force,
            pos,
            spread,
            speed,
            count,
            randomization,
        }) = packet(call(
            "minecraft:dust",
            "dust:255:1",
            "7",
            "ALTERNATIVE_WITH_SPEED",
        ))
        else {
            panic!("expected a particle packet");
        };
        assert_eq!(
            particle,
            ferrite_data::registries::id("minecraft:particle_type", "minecraft:dust").unwrap()
        );
        assert_eq!(options.len(), 8);
        assert!(force);
        assert_eq!(pos, [1.5, 64.0, -2.5]);
        assert_eq!(spread, [0.1_f32, 0.2, 0.3]);
        assert_eq!(speed, [0.5_f32; 3]);
        assert_eq!((count, randomization), (7, 2));
        // Bad input is left unhandled instead of reaching a client.
        assert!(packet(call("minecraft:dust", "dust:255:1", "-1", "DEFAULT")).is_none());
        assert!(packet(call("minecraft:dust", "dust:255:1", "1", "WEIRD")).is_none());
        assert!(packet(call("minecraft:dust", "", "1", "DEFAULT")).is_none());
        assert!(packet(call("minecraft:nope", "", "1", "DEFAULT")).is_none());
    }
}
