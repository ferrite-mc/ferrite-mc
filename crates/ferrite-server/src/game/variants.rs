//! Picking a cat or wolf variant (vanilla `VariantUtils.selectVariantToSpawn`): every variant of
//! the registry has `spawn_conditions`, the candidates whose condition holds are narrowed to the
//! highest priority, and one of those is chosen at random.
//!
//! Conditions this server can evaluate: `minecraft:biome` (ids and `#tags`) and
//! `minecraft:moon_brightness`. `minecraft:structure` (cats in swamp huts and witch huts spawn
//! all black) needs a structure lookup at the animal's position that is not available here, so
//! that condition never holds.

use std::path::Path;
use std::sync::OnceLock;

use ferrite_entity::mob::MobKind;

/// One `spawn_conditions` entry.
#[derive(Debug, Clone)]
struct Entry {
    priority: i32,
    condition: Option<Condition>,
}

#[derive(Debug, Clone)]
enum Condition {
    Biome(Vec<String>),
    Moon {
        min: Option<f64>,
        max: Option<f64>,
    },
    /// A condition type this server cannot evaluate; it never holds.
    Unsupported,
}

impl Condition {
    fn holds(&self, biome: Option<&str>, moon: f64) -> bool {
        match self {
            Self::Biome(wanted) => biome.is_some_and(|biome| {
                wanted.iter().any(|entry| match entry.strip_prefix('#') {
                    Some(tag) => ferrite_data::tags::contains(
                        "worldgen/biome",
                        tag.strip_prefix("minecraft:").unwrap_or(tag),
                        biome,
                    ),
                    None => entry == biome,
                })
            }),
            Self::Moon { min, max } => {
                min.is_none_or(|min| moon >= min) && max.is_none_or(|max| moon <= max)
            }
            Self::Unsupported => false,
        }
    }
}

fn parse(json: &serde_json::Value) -> Vec<Entry> {
    json["spawn_conditions"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|entry| Entry {
            priority: entry["priority"]
                .as_i64()
                .and_then(|p| i32::try_from(p).ok())
                .unwrap_or(0),
            condition: entry
                .get("condition")
                .map(|condition| match condition["type"].as_str() {
                    Some("minecraft:biome") => Condition::Biome(match &condition["biomes"] {
                        serde_json::Value::String(one) => vec![one.clone()],
                        serde_json::Value::Array(list) => list
                            .iter()
                            .filter_map(|b| b.as_str().map(str::to_owned))
                            .collect(),
                        _ => Vec::new(),
                    }),
                    Some("minecraft:moon_brightness") => Condition::Moon {
                        min: condition["range"]["min"].as_f64(),
                        max: condition["range"]["max"].as_f64(),
                    },
                    _ => Condition::Unsupported,
                }),
        })
        .collect()
}

/// The conditions of every variant of `kind`, in [`ferrite_entity::variant`] order.
fn table(kind: MobKind) -> &'static Vec<Vec<Entry>> {
    static CATS: OnceLock<Vec<Vec<Entry>>> = OnceLock::new();
    static WOLVES: OnceLock<Vec<Vec<Entry>>> = OnceLock::new();
    let (cell, dir) = match kind {
        MobKind::Cat => (&CATS, "cat_variant"),
        _ => (&WOLVES, "wolf_variant"),
    };
    cell.get_or_init(|| {
        let relative = Path::new("vanilla/26.3/generated/data/minecraft").join(dir);
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dir = [relative.clone(), root.join(&relative)]
            .into_iter()
            .find(|d| d.is_dir());
        ferrite_entity::variant::names(kind)
            .unwrap_or_default()
            .iter()
            .map(|name| {
                let file = name.strip_prefix("minecraft:").unwrap_or(name);
                dir.as_ref()
                    .and_then(|dir| std::fs::read_to_string(dir.join(format!("{file}.json"))).ok())
                    .and_then(|text| serde_json::from_str(&text).ok())
                    .map(|json| parse(&json))
                    .unwrap_or_default()
            })
            .collect()
    })
}

/// Vanilla `MoonPhase` brightness for a day time (`DimensionType.MOON_BRIGHTNESS_PER_PHASE`).
pub fn moon_brightness(day_time: i64) -> f64 {
    const PER_PHASE: [f64; 8] = [1.0, 0.75, 0.5, 0.25, 0.0, 0.25, 0.5, 0.75];
    let phase = day_time.div_euclid(24_000).rem_euclid(8);
    PER_PHASE[usize::try_from(phase).unwrap_or(0)]
}

/// The variant index a new cat or wolf gets in `biome` under `moon` brightness, or `None` for a
/// kind without variants (or when the variant data is missing).
pub fn select(
    kind: MobKind,
    biome: Option<&str>,
    moon: f64,
    rng: &mut ferrite_entity::Rng,
) -> Option<u8> {
    let mut best: Vec<u8> = Vec::new();
    let mut best_priority = i32::MIN;
    for (index, entries) in table(kind).iter().enumerate() {
        // A variant takes part with the highest priority among its conditions that hold.
        let priority = entries
            .iter()
            .filter(|entry| {
                entry
                    .condition
                    .as_ref()
                    .is_none_or(|condition| condition.holds(biome, moon))
            })
            .map(|entry| entry.priority)
            .max();
        let (Some(priority), Ok(index)) = (priority, u8::try_from(index)) else {
            continue;
        };
        match priority.cmp(&best_priority) {
            std::cmp::Ordering::Greater => {
                best_priority = priority;
                best = vec![index];
            }
            std::cmp::Ordering::Equal => best.push(index),
            std::cmp::Ordering::Less => {}
        }
    }
    let pick = usize::try_from(rng.next_int(i32::try_from(best.len()).ok()?)).ok()?;
    best.get(pick).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_entity::Rng;
    use ferrite_entity::variant::{index, name};

    fn wolf(biome: &str) -> &'static str {
        let mut rng = Rng::new(7);
        let picked = select(MobKind::Wolf, Some(biome), 0.0, &mut rng).unwrap();
        name(MobKind::Wolf, picked).unwrap()
    }

    #[test]
    fn a_wolf_takes_the_variant_of_its_biome() {
        if table(MobKind::Wolf).iter().all(Vec::is_empty) {
            return;
        }
        assert_eq!(wolf("minecraft:snowy_taiga"), "minecraft:ashen");
        assert_eq!(wolf("minecraft:old_growth_pine_taiga"), "minecraft:black");
        assert_eq!(
            wolf("minecraft:old_growth_spruce_taiga"),
            "minecraft:chestnut"
        );
        assert_eq!(wolf("minecraft:forest"), "minecraft:woods");
        assert_eq!(wolf("minecraft:grove"), "minecraft:snowy");
        // Tags: `#minecraft:is_jungle`, `is_savanna` and `is_badlands`.
        assert_eq!(wolf("minecraft:bamboo_jungle"), "minecraft:rusty");
        assert_eq!(wolf("minecraft:savanna"), "minecraft:spotted");
        assert_eq!(wolf("minecraft:badlands"), "minecraft:striped");
        // No biome condition holds: the priority 0 default.
        assert_eq!(wolf("minecraft:plains"), "minecraft:pale");
    }

    #[test]
    fn cats_pick_any_unconditioned_variant_and_all_black_needs_a_full_moon() {
        if table(MobKind::Cat).iter().all(Vec::is_empty) {
            return;
        }
        let black = index(MobKind::Cat, "all_black");
        let mut rng = Rng::new(11);
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..400 {
            seen.insert(select(MobKind::Cat, Some("minecraft:plains"), 0.0, &mut rng).unwrap());
        }
        assert!(!seen.contains(&black.unwrap()), "new moon: never all black");
        assert_eq!(seen.len(), 10, "the ten ordinary coats all turn up");
        let mut full = std::collections::BTreeSet::new();
        for _ in 0..400 {
            full.insert(select(MobKind::Cat, Some("minecraft:plains"), 1.0, &mut rng).unwrap());
        }
        assert!(full.contains(&black.unwrap()), "full moon: all black joins");
    }

    #[test]
    fn the_moon_brightness_follows_the_eight_phases() {
        assert_eq!(moon_brightness(0), 1.0);
        assert_eq!(moon_brightness(24_000 * 4), 0.0);
        assert_eq!(moon_brightness(24_000 * 8), 1.0);
        assert_eq!(moon_brightness(24_000 * 3 + 5), 0.25);
    }
}
