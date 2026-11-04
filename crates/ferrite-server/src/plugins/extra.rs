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
