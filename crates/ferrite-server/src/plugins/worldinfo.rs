//! Read-only world clock, weather and difficulty for plugins, taken from the same state the
//! `/time`, `/weather` and `/difficulty` commands use.

use ferrite_plugin::HostCall;
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sky {
    Clear,
    Rain,
    Thunder,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldInfo {
    /// Ticks of the overworld clock, not wrapped to a day.
    pub clock_time: i64,
    /// Total world ticks.
    pub game_time: i64,
    /// 0 peaceful, 1 easy, 2 normal, 3 hard.
    pub difficulty: i32,
    pub sky: Sky,
    /// The duration the last `/weather` command asked for, in ticks.
    pub weather_duration: i32,
    pub border: Border,
    /// The world seed.
    pub seed: i64,
}

/// The world border (`/worldborder`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    pub center_x: f64,
    pub center_z: f64,
    pub diameter: f64,
    pub warning_distance: i32,
    pub warning_ticks: i32,
    pub damage_amount: f32,
    pub damage_buffer: f32,
}

type Source = Arc<dyn Fn() -> WorldInfo + Send + Sync>;

fn slot() -> &'static Mutex<Option<Source>> {
    static SLOT: OnceLock<Mutex<Option<Source>>> = OnceLock::new();
    SLOT.get_or_init(Default::default)
}

/// Called by the game loop with a reader of its command state (the latest world wins).
pub fn register(source: Source) {
    *slot().lock().unwrap() = Some(source);
}

fn difficulty_name(value: i32) -> &'static str {
    match value {
        0 => "PEACEFUL",
        1 => "EASY",
        3 => "HARD",
        _ => "NORMAL",
    }
}

fn answer_from(info: &WorldInfo, method: &str) -> Option<String> {
    let storm = info.sky != Sky::Clear;
    Some(match method {
        "getFullTime()" => info.clock_time.to_string(),
        "getTime()" => info.clock_time.rem_euclid(24_000).to_string(),
        "getGameTime()" => info.game_time.to_string(),
        "getDifficulty()" => difficulty_name(info.difficulty).to_owned(),
        "hasStorm()" => storm.to_string(),
        "isThundering()" => (info.sky == Sky::Thunder).to_string(),
        "isClearWeather()" => (!storm).to_string(),
        "getWeatherDuration()" | "getThunderDuration()" if storm => {
            info.weather_duration.to_string()
        }
        "getWeatherDuration()" | "getThunderDuration()" => "0".to_owned(),
        "getClearWeatherDuration()" if !storm => info.weather_duration.to_string(),
        "getClearWeatherDuration()" => "0".to_owned(),
        // Vanilla `DimensionType.moonPhase`: eight phases of one day each, 0 is the full moon.
        "borderSize()" => info.border.diameter.to_string(),
        "borderCenterX()" => info.border.center_x.to_string(),
        "borderCenterZ()" => info.border.center_z.to_string(),
        "borderWarningDistance()" => info.border.warning_distance.to_string(),
        "borderWarningTicks()" => info.border.warning_ticks.to_string(),
        "borderDamageAmount()" => info.border.damage_amount.to_string(),
        "borderDamageBuffer()" => info.border.damage_buffer.to_string(),
        "getSeed()" => info.seed.to_string(),
        "getMoonPhase()" => (info.clock_time.div_euclid(24_000).rem_euclid(8)).to_string(),
        _ => return None,
    })
}

/// The reply to a read-only world query on the default world, or `None` for another call.
pub fn answer(call: &HostCall) -> Option<String> {
    if call.owner != "World"
        || !call.args.is_empty()
        || uuid::Uuid::parse_str(&call.subject).ok() != Some(ferrite_plugin::default_world_id())
    {
        return None;
    }
    let source = slot().lock().unwrap().clone()?;
    answer_from(&source(), &call.method)
}

/// The `worldborder` command for a valid border write on the default world.
pub fn border_line(call: &HostCall) -> Option<String> {
    if call.owner != "World"
        || uuid::Uuid::parse_str(&call.subject).ok() != Some(ferrite_plugin::default_world_id())
    {
        return None;
    }
    let number = |i: usize| -> Option<f64> {
        call.args
            .get(i)?
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
    };
    Some(match call.method.as_str() {
        "borderSet(double,long)" => {
            let size = number(0).filter(|s| (1.0..=59_999_968.0).contains(s))?;
            let seconds = (number(1)?.max(0.0) / 20.0).round() as i64;
            if seconds == 0 {
                format!("worldborder set {size}")
            } else {
                format!("worldborder set {size} {seconds}")
            }
        }
        "borderCenter(double,double)" => {
            let (x, z) = (number(0)?, number(1)?);
            if x.abs() > 29_999_984.0 || z.abs() > 29_999_984.0 {
                return None;
            }
            format!("worldborder center {x} {z}")
        }
        "borderDamageAmount(double)" => format!(
            "worldborder damage amount {}",
            number(0).filter(|v| *v >= 0.0)?
        ),
        "borderDamageBuffer(double)" => format!(
            "worldborder damage buffer {}",
            number(0).filter(|v| *v >= 0.0)?
        ),
        "borderWarningDistance(int)" => format!(
            "worldborder warning distance {}",
            number(0).filter(|v| *v >= 0.0)? as i64
        ),
        "borderWarningTime(int)" => format!(
            "worldborder warning time {}",
            (number(0).filter(|v| *v >= 0.0)? / 20.0).round() as i64
        ),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(sky: Sky) -> WorldInfo {
        WorldInfo {
            clock_time: 24_000 * 9 + 6_000,
            game_time: 500_000,
            difficulty: 3,
            sky,
            weather_duration: 6_000,
            border: Border {
                center_x: 1.5,
                center_z: -2.0,
                diameter: 100.0,
                warning_distance: 5,
                warning_ticks: 300,
                damage_amount: 0.2,
                damage_buffer: 5.0,
            },
            seed: -42,
        }
    }

    #[test]
    fn clock_difficulty_and_moon_follow_the_state() {
        let i = info(Sky::Clear);
        let get = |m: &str| answer_from(&i, m);
        assert_eq!(get("getTime()").as_deref(), Some("6000"));
        assert_eq!(get("getFullTime()").as_deref(), Some("222000"));
        assert_eq!(get("getGameTime()").as_deref(), Some("500000"));
        assert_eq!(get("getDifficulty()").as_deref(), Some("HARD"));
        assert_eq!(get("getMoonPhase()").as_deref(), Some("1"));
        assert_eq!(get("getPing()"), None);
    }

    #[test]
    fn weather_queries_agree_with_each_other() {
        let clear = info(Sky::Clear);
        assert_eq!(answer_from(&clear, "hasStorm()").as_deref(), Some("false"));
        assert_eq!(
            answer_from(&clear, "isClearWeather()").as_deref(),
            Some("true")
        );
        assert_eq!(
            answer_from(&clear, "getWeatherDuration()").as_deref(),
            Some("0")
        );
        let thunder = info(Sky::Thunder);
        assert_eq!(answer_from(&thunder, "hasStorm()").as_deref(), Some("true"));
        assert_eq!(
            answer_from(&thunder, "isThundering()").as_deref(),
            Some("true")
        );
        assert_eq!(
            answer_from(&thunder, "isClearWeather()").as_deref(),
            Some("false")
        );
        assert_eq!(
            answer_from(&info(Sky::Rain), "isThundering()").as_deref(),
            Some("false")
        );
    }

    #[test]
    fn border_and_seed_are_read_and_writes_are_validated() {
        let i = info(Sky::Clear);
        assert_eq!(answer_from(&i, "borderSize()").as_deref(), Some("100"));
        assert_eq!(answer_from(&i, "borderCenterX()").as_deref(), Some("1.5"));
        assert_eq!(
            answer_from(&i, "borderWarningTicks()").as_deref(),
            Some("300")
        );
        assert_eq!(answer_from(&i, "getSeed()").as_deref(), Some("-42"));
        let call = |method: &str, args: &[&str]| HostCall {
            owner: "World".into(),
            subject: ferrite_plugin::default_world_id().to_string(),
            method: method.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        };
        assert_eq!(
            border_line(&call("borderSet(double,long)", &["50", "0"])).as_deref(),
            Some("worldborder set 50")
        );
        assert_eq!(
            border_line(&call("borderSet(double,long)", &["50", "200"])).as_deref(),
            Some("worldborder set 50 10")
        );
        assert_eq!(
            border_line(&call("borderSet(double,long)", &["0", "0"])),
            None
        );
        assert_eq!(
            border_line(&call("borderCenter(double,double)", &["1", "2"])).as_deref(),
            Some("worldborder center 1 2")
        );
        assert_eq!(
            border_line(&call("borderCenter(double,double)", &["1e9", "2"])),
            None
        );
        assert_eq!(
            border_line(&call("borderWarningTime(int)", &["100"])).as_deref(),
            Some("worldborder warning time 5")
        );
    }
}
