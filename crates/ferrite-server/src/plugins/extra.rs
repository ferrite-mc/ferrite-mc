//! Host handlers for Bukkit methods that vanilla already exposes as console commands.
//!
//! Each arm validates every value it formats into a command line: numbers must be finite,
//! keys must pass [`is_resource_key`](super::is_resource_key), and the target is always the
//! calling player's UUID, so a plugin cannot smuggle a second argument or another selector.

use super::{HostAction, is_resource_key};
use ferrite_plugin::{HOST_NULL, HostCall};
use uuid::Uuid;

fn number(text: &str) -> Option<f64> {
    text.parse::<f64>().ok().filter(|n| n.is_finite())
}

fn entity(text: &str) -> Option<Uuid> {
    Uuid::parse_str(text).ok()
}

/// `minecraft:` keys arrive either as a key or as an enum name (`FLAME`); both become a key.
fn key(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    is_resource_key(&lower).then_some(lower)
}

fn category(text: &str) -> Option<&'static str> {
    Some(match text.to_ascii_lowercase().as_str() {
        "master" => "master",
        "music" => "music",
        "records" => "record",
        "weather" => "weather",
        "blocks" => "block",
        "hostile" => "hostile",
        "neutral" => "neutral",
        "players" => "player",
        "ambient" => "ambient",
        "voice" => "voice",
        "ui" | "master_ui" => "master",
        _ => return None,
    })
}

/// The sound of a note block instrument.
fn note_sound(instrument: &str) -> Option<&'static str> {
    Some(match instrument {
        "PIANO" => "minecraft:block.note_block.harp",
        "BASS_DRUM" => "minecraft:block.note_block.basedrum",
        "SNARE_DRUM" => "minecraft:block.note_block.snare",
        "STICKS" => "minecraft:block.note_block.hat",
        "BASS_GUITAR" => "minecraft:block.note_block.bass",
        "FLUTE" => "minecraft:block.note_block.flute",
        "BELL" => "minecraft:block.note_block.bell",
        "GUITAR" => "minecraft:block.note_block.guitar",
        "CHIME" => "minecraft:block.note_block.chime",
        "XYLOPHONE" => "minecraft:block.note_block.xylophone",
        "IRON_XYLOPHONE" => "minecraft:block.note_block.iron_xylophone",
        "COW_BELL" => "minecraft:block.note_block.cow_bell",
        "DIDGERIDOO" => "minecraft:block.note_block.didgeridoo",
        "BIT" => "minecraft:block.note_block.bit",
        "BANJO" => "minecraft:block.note_block.banjo",
        "PLING" => "minecraft:block.note_block.pling",
        _ => return None,
    })
}

fn console(line: String, reply: &str) -> Option<HostAction> {
    Some(HostAction::Console {
        line,
        reply: reply.to_owned(),
    })
}

fn tellraw_text(text: &str) -> String {
    serde_json::json!({ "text": text }).to_string()
}

/// The vanilla command behind `call`, or `None` when it has no command equivalent.
pub fn extra_action(call: &HostCall) -> Option<HostAction> {
    let arg = |index: usize| {
        call.args
            .get(index)
            .map(String::as_str)
            .filter(|a| *a != HOST_NULL)
    };
    let f = |index: usize| arg(index).and_then(number);
    let player = Uuid::parse_str(&call.subject).ok();
    match (call.owner.as_str(), call.method.as_str()) {
        ("Player", "kill(DamageSource)") => console(format!("kill {}", player?), ""),
        ("Player", "teleport(Entity)" | "teleport(Entity,TeleportCause)") => {
            let target = entity(arg(0)?)?;
            console(format!("tp {} {target}", player?), "true")
        }
        ("Player", "lookAt(double,double,double,LookAnchor)") => console(
            format!(
                "tp {0} ~ ~ ~ facing {1} {2} {3}",
                player?,
                f(0)?,
                f(1)?,
                f(2)?
            ),
            "",
        ),
        ("Player", "setRotation(float,float)") => {
            console(format!("tp {} ~ ~ ~ {} {}", player?, f(0)?, f(1)?), "")
        }
        ("Player", "hideTitle()") => console(format!("title {} clear", player?), ""),
        ("Player", "setTitleTimes(int,int,int)") => {
            let t = |i| arg(i)?.parse::<u32>().ok().filter(|v| *v <= 1_000_000);
            console(
                format!("title {} times {} {} {}", player?, t(0)?, t(1)?, t(2)?),
                "",
            )
        }
        ("Player", "sendRawMessage(String)" | "sendRawMessage(UUID,String)") => {
            let text = arg(call.args.len().checked_sub(1)?)?;
            console(format!("tellraw {} {}", player?, tellraw_text(text)), "")
        }
        ("Player", "playNote(Location,Instrument,int)") => {
            let (x, y, z) = location_xyz(arg(0)?)?;
            let sound = note_sound(arg(1)?)?;
            let note: i32 = arg(2)?.parse().ok().filter(|n| (0..=24).contains(n))?;
            // Vanilla `NoteBlock`: the pitch doubles every twelve semitones around note 12.
            let pitch = 2f64.powf(f64::from(note - 12) / 12.0);
            console(
                format!("playsound {sound} record {} {x} {y} {z} 3 {pitch}", player?),
                "",
            )
        }
        ("Player", "stopSound(SoundCategory)") => {
            console(format!("stopsound {} {}", player?, category(arg(0)?)?), "")
        }
        ("Player", "stopSound(String,SoundCategory)") => console(
            format!(
                "stopsound {} {} {}",
                player?,
                category(arg(1)?)?,
                key(arg(0)?)?
            ),
            "",
        ),
        (
            "Player",
            "playSound(Entity,Sound,SoundCategory,float,float)"
            | "playSound(Entity,String,SoundCategory,float,float)"
            | "playSound(Entity,Sound,SoundCategory,float,float,long)"
            | "playSound(Entity,String,SoundCategory,float,float,long)",
        ) => {
            let at = entity(arg(0)?)?;
            let sound = key(arg(1)?)?;
            let volume = f(3)?.clamp(0.0, 3.4e38);
            let pitch = f(4)?.clamp(0.0, 2.0);
            console(
                format!(
                    "execute at {at} run playsound {sound} {} {} ~ ~ ~ {volume} {pitch}",
                    category(arg(2)?)?,
                    player?
                ),
                "",
            )
        }
        ("Player", method)
            if method.starts_with("spawnParticle(Particle,double,double,double,int,") =>
        {
            // Every overload shares `particle, x, y, z, count, offsetX, offsetY, offsetZ, extra`;
            // particles that need data (dust, blocks, items) have no command form.
            if method.contains("Object") && arg(11).is_some() {
                return None;
            }
            let count = arg(4)?.parse::<u32>().ok().filter(|c| *c <= 100_000)?;
            console(
                format!(
                    "particle {} {} {} {} {} {} {} {} {count} normal {}",
                    key(arg(0)?)?,
                    f(1)?,
                    f(2)?,
                    f(3)?,
                    f(5)?,
                    f(6)?,
                    f(7)?,
                    f(8).unwrap_or(0.0),
                    player?
                ),
                "",
            )
        }
        ("Player", "setPlayerWeather(WeatherType)") => None,
        ("World", method) => world_action(call, method),
        ("Server", "banProfile(String,String)") => {
            let name = player_name(arg(0)?)?;
            let reason: String = arg(1)
                .unwrap_or("")
                .chars()
                .filter(|c| !c.is_control())
                .take(256)
                .collect();
            let line = if reason.is_empty() {
                format!("ban {name}")
            } else {
                format!("ban {name} {reason}")
            };
            console(line, "")
        }
        ("Server", "pardonProfile(String)") => {
            console(format!("pardon {}", player_name(arg(0)?)?), "")
        }
        _ => None,
    }
}

/// A Mojang account name: 1 to 16 letters, digits or underscores, so it cannot add a second command argument.
fn player_name(text: &str) -> Option<&str> {
    let ok = !text.is_empty()
        && text.len() <= 16
        && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    ok.then_some(text)
}

/// The `x y z` of an encoded `Location` (`world, x, y, z, yaw, pitch`).
fn location_xyz(encoded: &str) -> Option<(f64, f64, f64)> {
    let mut parts = encoded.split(ferrite_plugin::HOST_ITEM_SEPARATOR);
    parts.next()?;
    let mut axis = || number(parts.next()?).filter(|v| v.abs() < 3.0e7);
    Some((axis()?, axis()?, axis()?))
}

/// World effects that vanilla exposes as commands. Only the default world has a console
/// target, so a call on another world is left unhandled.
fn world_action(call: &HostCall, method: &str) -> Option<HostAction> {
    if Uuid::parse_str(&call.subject).ok()? != ferrite_plugin::default_world_id() {
        return None;
    }
    let arg = |index: usize| {
        call.args
            .get(index)
            .map(String::as_str)
            .filter(|a| *a != HOST_NULL)
    };
    let f = |index: usize| arg(index).and_then(number);
    match method {
        "strikeLightning(Location)" => {
            let (x, y, z) = location_xyz(arg(0)?)?;
            console(format!("summon lightning_bolt {x} {y} {z}"), "")
        }
        "playSound(Location,Sound,float,float)"
        | "playSound(Location,String,float,float)"
        | "playSound(Location,Sound,SoundCategory,float,float)"
        | "playSound(Location,String,SoundCategory,float,float)"
        | "playSound(Location,Sound,float,float,long)"
        | "playSound(Location,String,float,float,long)"
        | "playSound(Location,Sound,SoundCategory,float,float,long)"
        | "playSound(Location,String,SoundCategory,float,float,long)" => {
            let (x, y, z) = location_xyz(arg(0)?)?;
            let sound = key(arg(1)?)?;
            let (cat, rest) = if method.contains("SoundCategory") {
                (category(arg(2)?)?, 3)
            } else {
                ("master", 2)
            };
            let volume = f(rest)?.clamp(0.0, 3.4e38);
            let pitch = f(rest + 1)?.clamp(0.0, 2.0);
            console(
                format!("playsound {sound} {cat} @a {x} {y} {z} {volume} {pitch}"),
                "",
            )
        }
        "spawnParticle(Particle,Location,int)"
        | "spawnParticle(Particle,Location,int,double)"
        | "spawnParticle(Particle,Location,int,double,double,double)"
        | "spawnParticle(Particle,Location,int,double,double,double,double)" => {
            let (x, y, z) = location_xyz(arg(1)?)?;
            let count = arg(2)?.parse::<u32>().ok().filter(|c| *c <= 100_000)?;
            let offsets = if method.ends_with("double,double,double)")
                || method.ends_with("double,double,double,double)")
            {
                (f(3)?, f(4)?, f(5)?)
            } else {
                (0.0, 0.0, 0.0)
            };
            let speed = if method.ends_with("double,double,double,double)") {
                f(6)?
            } else if method.ends_with("int,double)") {
                f(3)?
            } else {
                0.0
            };
            console(
                format!(
                    "particle {} {x} {y} {z} {} {} {} {speed} {count} normal @a",
                    key(arg(0)?)?,
                    offsets.0,
                    offsets.1,
                    offsets.2
                ),
                "",
            )
        }
        // The coordinate forms: `particle, x, y, z, count` plus optionally the three offsets and a speed.
        // Overloads taking particle data are left unhandled, as for players.
        m if m.starts_with("spawnParticle(Particle,double,double,double,int")
            && !m.contains("Object") =>
        {
            let count = arg(4)?.parse::<u32>().ok().filter(|c| *c <= 100_000)?;
            let offsets = match call.args.len() {
                5 => (0.0, 0.0, 0.0),
                8 | 9 => (f(5)?, f(6)?, f(7)?),
                _ => return None,
            };
            let speed = if call.args.len() == 9 { f(8)? } else { 0.0 };
            console(
                format!(
                    "particle {} {} {} {} {} {} {} {speed} {count} normal @a",
                    key(arg(0)?)?,
                    f(1).filter(|v| v.abs() < 3.0e7)?,
                    f(2).filter(|v| v.abs() < 3.0e7)?,
                    f(3).filter(|v| v.abs() < 3.0e7)?,
                    offsets.0,
                    offsets.1,
                    offsets.2
                ),
                "",
            )
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(method: &str, args: &[&str]) -> HostCall {
        HostCall {
            owner: "Player".into(),
            subject: "00000000-0000-0000-0000-000000000007".into(),
            method: method.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    fn line(call: HostCall) -> Option<String> {
        match extra_action(&call)? {
            HostAction::Console { line, .. } => Some(line),
            HostAction::Reply(_) => None,
        }
    }

    const ME: &str = "00000000-0000-0000-0000-000000000007";

    #[test]
    fn commands_target_only_the_calling_player() {
        assert_eq!(
            line(call("kill(DamageSource)", &["x"])),
            Some(format!("kill {ME}"))
        );
        assert_eq!(
            line(call("hideTitle()", &[])),
            Some(format!("title {ME} clear"))
        );
        assert_eq!(
            line(call("setTitleTimes(int,int,int)", &["10", "70", "20"])),
            Some(format!("title {ME} times 10 70 20"))
        );
        assert_eq!(
            line(call(
                "stopSound(String,SoundCategory)",
                &["minecraft:block.stone.hit", "BLOCKS"]
            )),
            Some(format!("stopsound {ME} block minecraft:block.stone.hit"))
        );
    }

    #[test]
    fn notes_play_the_instrument_at_the_right_pitch() {
        let at = format!("world{P}1{P}64{P}2{P}0{P}0", P = '\u{1d}');
        assert_eq!(
            line(call(
                "playNote(Location,Instrument,int)",
                &[&at, "PLING", "24"]
            )),
            Some(format!(
                "playsound minecraft:block.note_block.pling record {ME} 1 64 2 3 2"
            ))
        );
        assert_eq!(
            line(call(
                "playNote(Location,Instrument,int)",
                &[&at, "KAZOO", "1"]
            )),
            None
        );
        assert_eq!(
            line(call(
                "playNote(Location,Instrument,int)",
                &[&at, "PIANO", "25"]
            )),
            None
        );
    }

    #[test]
    fn untrusted_values_are_rejected() {
        assert_eq!(
            line(call("stopSound(String,SoundCategory)", &["a b", "BLOCKS"])),
            None
        );
        assert_eq!(line(call("setRotation(float,float)", &["NaN", "0"])), None);
        assert_eq!(line(call("teleport(Entity)", &["@a"])), None);
        assert_eq!(
            line(call("setTitleTimes(int,int,int)", &["-1", "1", "1"])),
            None
        );
    }

    #[test]
    fn profile_bans_only_accept_account_names() {
        let server = |method: &str, args: &[&str]| HostCall {
            owner: "Server".into(),
            subject: "server".into(),
            method: method.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        };
        assert_eq!(
            line(server(
                "banProfile(String,String)",
                &["Alex_1", "griefing\nop"]
            ))
            .as_deref(),
            Some("ban Alex_1 griefingop")
        );
        assert_eq!(
            line(server("banProfile(String,String)", &["Alex", ""])).as_deref(),
            Some("ban Alex")
        );
        assert_eq!(
            line(server("banProfile(String,String)", &["a b", ""])),
            None
        );
        assert_eq!(line(server("banProfile(String,String)", &["@a", ""])), None);
        assert_eq!(
            line(server("pardonProfile(String)", &["Alex"])).as_deref(),
            Some("pardon Alex")
        );
    }

    #[test]
    fn world_effects_target_the_default_world_only() {
        let default = ferrite_plugin::default_world_id().to_string();
        let at = format!(
            "world{0}1.5{0}64{0}-2{0}0{0}0",
            ferrite_plugin::HOST_ITEM_SEPARATOR
        );
        let world = |subject: &str, method: &str, args: &[&str]| HostCall {
            owner: "World".into(),
            subject: subject.into(),
            method: method.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        };
        assert_eq!(
            line(world(&default, "strikeLightning(Location)", &[&at])).as_deref(),
            Some("summon lightning_bolt 1.5 64 -2")
        );
        assert_eq!(
            line(world(
                &default,
                "playSound(Location,String,SoundCategory,float,float)",
                &[
                    &at,
                    "minecraft:entity.player.levelup",
                    "PLAYERS",
                    "1",
                    "1.5"
                ]
            ))
            .as_deref(),
            Some("playsound minecraft:entity.player.levelup player @a 1.5 64 -2 1 1.5")
        );
        assert_eq!(
            line(world(
                &default,
                "spawnParticle(Particle,Location,int)",
                &[&at, "minecraft:flame", "x"]
            )),
            None
        );
        assert_eq!(
            line(world(
                "00000000-0000-0000-0000-0000000000fe",
                "strikeLightning(Location)",
                &[&at]
            )),
            None
        );
    }

    #[test]
    fn coordinate_particles_use_the_same_command_as_location_particles() {
        let default = ferrite_plugin::default_world_id().to_string();
        let world = |subject: &str, method: &str, args: &[&str]| HostCall {
            owner: "World".into(),
            subject: subject.into(),
            method: method.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        };
        let args = [
            "minecraft:flame",
            "1",
            "64.5",
            "-2",
            "7",
            "0.1",
            "0.2",
            "0.3",
            "0.05",
        ];
        assert_eq!(
            line(world(
                &default,
                "spawnParticle(Particle,double,double,double,int,double,double,double,double)",
                &args
            ))
            .as_deref(),
            Some("particle minecraft:flame 1 64.5 -2 0.1 0.2 0.3 0.05 7 normal @a")
        );
        assert_eq!(
            line(world(
                &default,
                "spawnParticle(Particle,double,double,double,int)",
                &args[..5]
            ))
            .as_deref(),
            Some("particle minecraft:flame 1 64.5 -2 0 0 0 0 7 normal @a")
        );
        assert_eq!(
            line(world(
                &default,
                "spawnParticle(Particle,double,double,double,int,Object)",
                &args[..6]
            )),
            None
        );
    }

    #[test]
    fn particles_and_positional_sounds_format_all_arguments() {
        let at = "00000000-0000-0000-0000-000000000009";
        assert_eq!(
            line(call(
                "playSound(Entity,Sound,SoundCategory,float,float)",
                &[at, "minecraft:entity.arrow.hit", "PLAYERS", "1.0", "0.5"]
            )),
            Some(format!(
                "execute at {at} run playsound minecraft:entity.arrow.hit player {ME} ~ ~ ~ 1 0.5"
            ))
        );
        assert_eq!(
            line(call(
                "spawnParticle(Particle,double,double,double,int,double,double,double,double)",
                &["FLAME", "1", "2", "3", "4", "0.1", "0.2", "0.3", "0.5"]
            )),
            Some(format!(
                "particle flame 1 2 3 0.1 0.2 0.3 0.5 4 normal {ME}"
            ))
        );
    }
}
