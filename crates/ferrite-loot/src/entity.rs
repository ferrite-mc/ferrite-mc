//! Entity and damage source facts, and the `entity_properties` / `damage_source_properties`
//! predicates evaluated against them.
//!
//! The caller describes the entities involved ([`EntityFacts`]); the predicate keys the shipped
//! loot tables use are evaluated: `minecraft:entity_type`, `minecraft:flags`,
//! `minecraft:equipment` (main hand enchantments), `minecraft:components`,
//! `minecraft:type_specific/*` and `minecraft:vehicle`. A key this module does not know makes
//! the predicate fail instead of passing silently.

use crate::data::GameData;
use serde_json::Value;
use std::collections::BTreeMap;

/// What the loot rolling needs to know about one entity.
#[derive(Debug, Clone, Default)]
pub struct EntityFacts {
    /// Namespaced entity type (`minecraft:sheep`).
    pub entity_type: String,
    /// Whether the entity is burning (`flags.is_on_fire`).
    pub on_fire: bool,
    /// Whether the entity is a baby (`flags.is_baby`).
    pub baby: bool,
    /// The held main-hand item and its enchantments, if the entity holds something.
    pub mainhand: Option<(String, BTreeMap<String, i32>)>,
    /// The entity's custom name as plain text (`copy_name`).
    pub custom_name: Option<String>,
    /// What the entity rides.
    pub vehicle: Option<Box<EntityFacts>>,
    /// Further facts keyed `components/<component>` (value as text, for example
    /// `components/minecraft:sheep/color` = `white`) or `type_specific/<kind>/<field>`
    /// (`type_specific/sheep/sheared` = `false`, `type_specific/cube_mob/size` = `2`).
    pub properties: BTreeMap<String, String>,
}

/// The damage that killed an entity.
#[derive(Debug, Clone, Default)]
pub struct DamageFacts {
    /// Namespaced damage type (`minecraft:player_attack`).
    pub damage_type: String,
    /// The entity that caused the damage (vanilla `attacker`).
    pub source_entity: Option<EntityFacts>,
    /// The entity that dealt it directly, such as a projectile (vanilla `direct_attacker`).
    pub direct_entity: Option<EntityFacts>,
}

fn same_id(a: &str, b: &str) -> bool {
    a.trim_start_matches("minecraft:") == b.trim_start_matches("minecraft:")
}

/// Whether `facts` satisfies the entity predicate `predicate`.
pub(crate) fn entity_matches(predicate: &Value, facts: &EntityFacts, data: &GameData) -> bool {
    let Some(object) = predicate.as_object() else {
        return false;
    };
    object.iter().all(|(key, expected)| match key.as_str() {
        "minecraft:entity_type" => expected.as_str().is_some_and(|wanted| {
            wanted.strip_prefix('#').map_or_else(
                || same_id(wanted, &facts.entity_type),
                |tag| data.in_tag("entity_type", tag, &facts.entity_type),
            )
        }),
        "minecraft:flags" => expected.as_object().is_some_and(|flags| {
            flags.iter().all(|(flag, wanted)| {
                let actual = match flag.as_str() {
                    "is_on_fire" => facts.on_fire,
                    "is_baby" => facts.baby,
                    other => facts
                        .properties
                        .get(&format!("flags/{other}"))
                        .is_some_and(|value| value == "true"),
                };
                wanted.as_bool() == Some(actual)
            })
        }),
        "minecraft:equipment" => expected.as_object().is_some_and(|slots| {
            slots.iter().all(|(slot, item)| {
                slot == "mainhand"
                    && facts
                        .mainhand
                        .as_ref()
                        .is_some_and(|(id, enchants)| item_matches(item, id, enchants, data))
            })
        }),
        "minecraft:components" => expected.as_object().is_some_and(|components| {
            components.iter().all(|(name, wanted)| {
                facts
                    .properties
                    .get(&format!("components/{name}"))
                    .is_some_and(|actual| value_matches(wanted, actual))
            })
        }),
        "minecraft:vehicle" => facts
            .vehicle
            .as_deref()
            .is_some_and(|vehicle| entity_matches(expected, vehicle, data)),
        other => other
            .strip_prefix("minecraft:type_specific/")
            .is_some_and(|kind| {
                expected.as_object().is_some_and(|fields| {
                    fields.iter().all(|(field, wanted)| {
                        facts
                            .properties
                            .get(&format!("type_specific/{kind}/{field}"))
                            .is_some_and(|actual| value_matches(wanted, actual))
                    })
                })
            }),
    })
}

/// A value predicate leaf: bool, number, `{min, max}` range or an id/string.
fn value_matches(wanted: &Value, actual: &str) -> bool {
    match wanted {
        Value::Bool(flag) => actual == flag.to_string(),
        Value::Number(number) => actual
            .parse::<f64>()
            .is_ok_and(|actual| Some(actual) == number.as_f64()),
        Value::String(text) => same_id(text, actual),
        Value::Object(range) => actual.parse::<f64>().is_ok_and(|actual| {
            range
                .get("min")
                .and_then(Value::as_f64)
                .is_none_or(|min| actual >= min)
                && range
                    .get("max")
                    .and_then(Value::as_f64)
                    .is_none_or(|max| actual <= max)
        }),
        _ => false,
    }
}

/// An item predicate over the held item: `predicates.minecraft:enchantments` entries.
fn item_matches(
    predicate: &Value,
    item: &str,
    enchantments: &BTreeMap<String, i32>,
    data: &GameData,
) -> bool {
    let Some(object) = predicate.as_object() else {
        return false;
    };
    object.iter().all(|(key, value)| match key.as_str() {
        "items" => data
            .resolve_ids("item", value)
            .iter()
            .any(|id| same_id(id, item)),
        "predicates" => value.as_object().is_some_and(|predicates| {
            predicates.iter().all(|(name, wanted)| {
                name == "minecraft:enchantments"
                    && wanted.as_array().is_some_and(|entries| {
                        entries
                            .iter()
                            .all(|entry| enchantment_entry_matches(entry, enchantments, data))
                    })
            })
        }),
        _ => false,
    })
}

fn enchantment_entry_matches(
    entry: &Value,
    enchantments: &BTreeMap<String, i32>,
    data: &GameData,
) -> bool {
    let ids = entry
        .get("enchantments")
        .map(|value| data.resolve_ids("enchantment", value));
    enchantments.iter().any(|(id, level)| {
        ids.as_ref()
            .is_none_or(|ids| ids.iter().any(|candidate| same_id(candidate, id)))
            && entry
                .get("levels")
                .is_none_or(|range| value_matches(range, &level.to_string()))
    })
}

/// Whether `facts` satisfies a `damage_source_properties` predicate.
pub(crate) fn damage_matches(predicate: &Value, facts: &DamageFacts, data: &GameData) -> bool {
    let Some(object) = predicate.as_object() else {
        return false;
    };
    object.iter().all(|(key, expected)| match key.as_str() {
        "tags" => expected.as_array().is_some_and(|tags| {
            tags.iter().all(|entry| {
                let Some(id) = entry.get("id").and_then(Value::as_str) else {
                    return false;
                };
                let wanted = entry
                    .get("expected")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                let tag = id.trim_start_matches('#');
                data.in_tag("damage_type", tag, &facts.damage_type) == wanted
            })
        }),
        "source_entity" => facts
            .source_entity
            .as_ref()
            .is_some_and(|entity| entity_matches(expected, entity, data)),
        "direct_entity" => facts
            .direct_entity
            .as_ref()
            .is_some_and(|entity| entity_matches(expected, entity, data)),
        _ => false,
    })
}
