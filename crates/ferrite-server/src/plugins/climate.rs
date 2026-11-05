//! `Biome` climate for `World#getTemperature` / `getHumidity`: the `temperature` and `downfall`
//! of a biome's vanilla data file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use ferrite_plugin::{HOST_ITEM_SEPARATOR, HostCall};

fn cache() -> &'static Mutex<HashMap<String, Option<(f64, f64)>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<(f64, f64)>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn read(name: &str) -> Option<(f64, f64)> {
    let id = name.strip_prefix("minecraft:").unwrap_or(name);
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '/')
    {
        return None;
    }
    let relative = format!("vanilla/26.3/generated/data/minecraft/worldgen/biome/{id}.json");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let text = [PathBuf::from(&relative), root.join(&relative)]
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some((
        json.get("temperature")?.as_f64()?,
        json.get("downfall")?.as_f64()?,
    ))
}

/// `biomeClimate(String)`: `temperature` and `humidity` of a biome key, joined by the item separator.
pub fn answer(call: &HostCall) -> Option<String> {
    if call.method != "biomeClimate(String)" {
        return None;
    }
    let name = call.args.first()?;
    let climate = *cache()
        .lock()
        .unwrap()
        .entry(name.clone())
        .or_insert_with(|| read(name));
    let (temperature, humidity) = climate?;
    Some(format!("{temperature}{HOST_ITEM_SEPARATOR}{humidity}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plains_is_mild_and_rain_free_names_are_refused() {
        let call = |name: &str| HostCall {
            owner: "World".into(),
            subject: "x".into(),
            method: "biomeClimate(String)".into(),
            args: vec![name.into()],
        };
        let Some(reply) = answer(&call("minecraft:plains")) else {
            eprintln!("skipping: vanilla data not generated");
            return;
        };
        let (t, h) = reply.split_once(HOST_ITEM_SEPARATOR).unwrap();
        assert_eq!(
            (t.parse::<f64>().unwrap(), h.parse::<f64>().unwrap()),
            (0.8, 0.4)
        );
        assert_eq!(answer(&call("minecraft:../secret")), None);
    }
}
