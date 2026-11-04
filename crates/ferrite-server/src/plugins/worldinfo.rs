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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
}
