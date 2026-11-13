//! Loot conditions: random chances, tool/block predicates, boolean operators.
//!
//! 26.3 writes a condition as either an object with a `type` or a plain string
//! naming a predicate file (`data/<namespace>/predicate/**`). String
//! references are resolved while loading. Conditions this crate cannot
//! evaluate (entity properties, location checks, damage source properties) are
//! retained opaquely and behave as satisfied.

use crate::{Definitions, LootContext, Rng, normalize_id};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// A condition attached to a pool, entry, function or another condition.
#[derive(Debug, Clone)]
pub enum Condition {
    /// Succeeds with the given probability.
    RandomChance {
        /// Probability in `[0, 1]`.
        chance: f32,
    },
    /// Succeeds unless an explosion destroys the drop.
    SurvivesExplosion,
    /// Checks the tool used against an item predicate.
    MatchTool(ToolPredicate),
    /// Checks the broken block id and block state properties.
    BlockStateProperty(BlockPredicate),
    /// Checks an entity role (`this`, `attacker`, `direct_attacker`) against a predicate. Always
    /// satisfied while the context carries no entity facts at all.
    EntityProperties {
        /// Entity role the predicate applies to (`this`, `attacker`, ...).
        entity: Option<String>,
        /// Raw predicate JSON.
        predicate: Value,
        /// Registry data for tag lookups.
        data: std::sync::Arc<crate::data::GameData>,
    },
    /// Checks the damage source against a predicate. Always satisfied while the context carries
    /// no damage facts.
    DamageSourceProperties {
        /// Raw predicate JSON.
        predicate: Value,
        /// Registry data for tag lookups.
        data: std::sync::Arc<crate::data::GameData>,
    },
    /// Chance by enchantment level of the attacker (`random_chance_with_enchanted_bonus`).
    RandomChanceWithEnchantedBonus {
        /// Chance without the enchantment.
        unenchanted: f32,
        /// `(base, per_level_above_first)` of the linear chance with the enchantment.
        enchanted: (f32, f32),
        /// Enchantment whose level selects the chance (looting).
        enchantment: String,
    },
    /// Checks the biome and block at the origin plus an offset.
    LocationCheck {
        /// Offset from the origin.
        offset: [i32; 3],
        /// Raw `predicate` (`biomes`, `block`).
        predicate: Value,
        /// Registry data for tags.
        data: std::sync::Arc<crate::data::GameData>,
    },
    /// Negates another condition.
    Inverted(Box<Condition>),
    /// Succeeds when any child does.
    AnyOf(Vec<Condition>),
    /// Succeeds when every child does.
    AllOf(Vec<Condition>),
    /// Checks [`LootContext::killed_by_player`].
    KilledByPlayer,
    /// Draws from a chance list indexed by enchantment level.
    TableBonus {
        /// Chances per level; the last entry applies to higher levels.
        chances: Vec<f32>,
        /// Enchantment whose level selects the chance.
        enchantment: String,
    },
    /// A condition type this crate keeps opaque; always satisfied.
    Opaque {
        /// Vanilla condition type (or predicate id when unresolved).
        kind: String,
    },
}

impl Condition {
    /// Evaluates the condition, consuming randomness where vanilla would.
    pub(crate) fn test(&self, context: &LootContext, rng: &mut impl Rng) -> bool {
        match self {
            Self::RandomChance { chance } => rng.next_float() < *chance,
            Self::SurvivesExplosion => match context.explosion_radius {
                Some(radius) if radius > 0.0 => rng.next_float() < 1.0 / radius,
                _ => true,
            },
            Self::MatchTool(predicate) => predicate.test(context),
            Self::BlockStateProperty(predicate) => predicate.test(context),
            Self::EntityProperties {
                entity,
                predicate,
                data,
            } => {
                if context.this_entity.is_none() && context.damage_source.is_none() {
                    return true;
                }
                let facts = match entity.as_deref().unwrap_or("this") {
                    "this" => context.this_entity.as_ref(),
                    "attacker" => context
                        .damage_source
                        .as_ref()
                        .and_then(|damage| damage.source_entity.as_ref()),
                    "direct_attacker" => context
                        .damage_source
                        .as_ref()
                        .and_then(|damage| damage.direct_entity.as_ref()),
                    _ => None,
                };
                facts.is_some_and(|facts| crate::entity::entity_matches(predicate, facts, data))
            }
            Self::RandomChanceWithEnchantedBonus {
                unenchanted,
                enchanted,
                enchantment,
            } => {
                let level = context.enchantment_level(enchantment);
                let chance = if level > 0 {
                    enchanted.0 + enchanted.1 * (level - 1) as f32
                } else {
                    *unenchanted
                };
                rng.next_float() < chance
            }
            Self::LocationCheck {
                offset,
                predicate,
                data,
            } => {
                let (Some(origin), Some(host)) = (context.origin, context.host.as_deref()) else {
                    return true;
                };
                let pos = [
                    origin[0].floor() as i32 + offset[0],
                    origin[1].floor() as i32 + offset[1],
                    origin[2].floor() as i32 + offset[2],
                ];
                location_matches(predicate, pos, host, data)
            }
            Self::DamageSourceProperties { predicate, data } => match &context.damage_source {
                None => context.this_entity.is_none(),
                Some(damage) => crate::entity::damage_matches(predicate, damage, data),
            },
            Self::Opaque { .. } => true,
            Self::Inverted(condition) => !condition.test(context, rng),
            Self::AnyOf(conditions) => conditions
                .iter()
                .any(|condition| condition.test(context, rng)),
            Self::AllOf(conditions) => conditions
                .iter()
                .all(|condition| condition.test(context, rng)),
            Self::KilledByPlayer => context.killed_by_player,
            Self::TableBonus {
                chances,
                enchantment,
            } => {
                let Some(&fallback) = chances.last() else {
                    return false;
                };
                let level = context.enchantment_level(enchantment).max(0) as usize;
                let chance = chances.get(level).copied().unwrap_or(fallback);
                rng.next_float() < chance
            }
        }
    }
}

/// An item predicate from a `match_tool` condition.
#[derive(Debug, Clone, Default)]
pub struct ToolPredicate {
    /// Accepted tool items; tags are resolved to their members while loading.
    pub items: Vec<String>,
    /// Enchantment requirements, all of which must hold.
    pub enchantments: Vec<EnchantmentPredicate>,
}

impl ToolPredicate {
    fn test(&self, context: &LootContext) -> bool {
        let Some(tool) = &context.tool else {
            return false;
        };
        let tool = normalize_id(tool);
        if !self.items.is_empty() && !self.items.iter().any(|item| normalize_id(item) == tool) {
            return false;
        }
        self.enchantments.iter().all(|predicate| {
            predicate.enchantments.iter().any(|enchantment| {
                let level = context.tool_enchantment_level(enchantment);
                level >= predicate.min_level
                    && predicate
                        .max_level
                        .is_none_or(|max_level| level <= max_level)
            })
        })
    }
}

/// One enchantment requirement from a tool predicate.
#[derive(Debug, Clone, Default)]
pub struct EnchantmentPredicate {
    /// Enchantment ids; any one of them may satisfy the predicate.
    pub enchantments: Vec<String>,
    /// Minimum level (inclusive).
    pub min_level: i32,
    /// Maximum level (inclusive), if bounded.
    pub max_level: Option<i32>,
}

/// A block id and block state property predicate.
#[derive(Debug, Clone, Default)]
pub struct BlockPredicate {
    /// Accepted block ids. Empty means any block.
    pub blocks: Vec<String>,
    /// Required block state properties.
    pub properties: BTreeMap<String, String>,
}

impl BlockPredicate {
    fn test(&self, context: &LootContext) -> bool {
        if !self.blocks.is_empty() {
            let Some(block) = &context.block else {
                return false;
            };
            let block = normalize_id(block);
            if !self
                .blocks
                .iter()
                .any(|candidate| normalize_id(candidate) == block)
            {
                return false;
            }
        }
        self.properties
            .iter()
            .all(|(name, value)| context.block_state.get(name) == Some(value))
    }
}

/// Parses a `condition`/`conditions` field, which may be one condition or an
/// array (all of which must pass).
pub(crate) fn parse_list(value: &Value, definitions: &Definitions) -> Vec<Condition> {
    match value {
        Value::Array(values) => values
            .iter()
            .filter_map(|value| parse(value, definitions))
            .collect(),
        value => parse(value, definitions).into_iter().collect(),
    }
}

/// Parses one condition value, resolving string predicate references.
pub(crate) fn parse(value: &Value, definitions: &Definitions) -> Option<Condition> {
    parse_with(value, definitions, &mut Vec::new())
}

fn parse_with(
    value: &Value,
    definitions: &Definitions,
    resolving: &mut Vec<String>,
) -> Option<Condition> {
    match value {
        Value::String(name) => Some(resolve_reference(name, definitions, resolving)),
        Value::Array(terms) => Some(Condition::AllOf(
            terms
                .iter()
                .filter_map(|term| parse_with(term, definitions, resolving))
                .collect(),
        )),
        Value::Object(object) => Some(parse_object(object, definitions, resolving)),
        _ => None,
    }
}

fn parse_object(
    object: &Map<String, Value>,
    definitions: &Definitions,
    resolving: &mut Vec<String>,
) -> Condition {
    let kind = object.get("type").and_then(Value::as_str).unwrap_or("");
    match kind {
        "minecraft:random_chance" => Condition::RandomChance {
            chance: float(object.get("chance")) as f32,
        },
        "minecraft:survives_explosion" => Condition::SurvivesExplosion,
        "minecraft:match_tool" => {
            Condition::MatchTool(parse_tool(object.get("predicate"), definitions))
        }
        "minecraft:block_state_property" | "minecraft:match_block" => {
            Condition::BlockStateProperty(parse_block(object))
        }
        "minecraft:entity_properties" => Condition::EntityProperties {
            entity: object
                .get("entity")
                .and_then(Value::as_str)
                .map(str::to_owned),
            predicate: object.get("predicate").cloned().unwrap_or(Value::Null),
            data: definitions.data.clone(),
        },
        "minecraft:random_chance_with_enchanted_bonus" => {
            let linear = object.get("enchanted_chance");
            Condition::RandomChanceWithEnchantedBonus {
                unenchanted: float(object.get("unenchanted_chance")) as f32,
                enchanted: (
                    float(linear.and_then(|v| v.get("base").or_else(|| v.get("value")))) as f32,
                    float(linear.and_then(|v| v.get("per_level_above_first"))) as f32,
                ),
                enchantment: object
                    .get("enchantment")
                    .and_then(Value::as_str)
                    .unwrap_or("minecraft:looting")
                    .to_owned(),
            }
        }
        "minecraft:location_check" => {
            let offset = |key: &str| object.get(key).and_then(Value::as_i64).unwrap_or(0) as i32;
            Condition::LocationCheck {
                offset: [offset("offsetX"), offset("offsetY"), offset("offsetZ")],
                predicate: object.get("predicate").cloned().unwrap_or(Value::Null),
                data: definitions.data.clone(),
            }
        }
        "minecraft:damage_source_properties" => Condition::DamageSourceProperties {
            predicate: object.get("predicate").cloned().unwrap_or(Value::Null),
            data: definitions.data.clone(),
        },
        "minecraft:inverted" => Condition::Inverted(Box::new(
            object
                .get("term")
                .and_then(|term| parse_with(term, definitions, resolving))
                .unwrap_or(Condition::Opaque {
                    kind: "minecraft:inverted".to_owned(),
                }),
        )),
        "minecraft:any_of" => Condition::AnyOf(parse_terms(object, definitions, resolving)),
        "minecraft:all_of" => Condition::AllOf(parse_terms(object, definitions, resolving)),
        "minecraft:killed_by_player" => Condition::KilledByPlayer,
        "minecraft:table_bonus" => Condition::TableBonus {
            chances: floats(object.get("chances")),
            enchantment: object
                .get("enchantment")
                .and_then(Value::as_str)
                .unwrap_or("minecraft:fortune")
                .to_owned(),
        },
        other => Condition::Opaque {
            kind: other.to_owned(),
        },
    }
}

fn parse_terms(
    object: &Map<String, Value>,
    definitions: &Definitions,
    resolving: &mut Vec<String>,
) -> Vec<Condition> {
    object
        .get("terms")
        .and_then(Value::as_array)
        .map(|terms| {
            terms
                .iter()
                .filter_map(|term| parse_with(term, definitions, resolving))
                .collect()
        })
        .unwrap_or_default()
}

fn resolve_reference(
    name: &str,
    definitions: &Definitions,
    resolving: &mut Vec<String>,
) -> Condition {
    let id = definitions.qualify(name);
    if resolving.contains(&id) {
        return Condition::Opaque {
            kind: format!("cycle:{id}"),
        };
    }
    let Some(value) = definitions.predicates.get(&id) else {
        return Condition::Opaque { kind: id };
    };
    resolving.push(id);
    let condition = parse_with(value, definitions, resolving).unwrap_or(Condition::Opaque {
        kind: "minecraft:unknown".to_owned(),
    });
    resolving.pop();
    condition
}

fn parse_tool(value: Option<&Value>, definitions: &Definitions) -> ToolPredicate {
    let Some(object) = value.and_then(Value::as_object) else {
        return ToolPredicate::default();
    };

    let mut items = Vec::new();
    collect_item_refs(object.get("items"), definitions, &mut items);
    items.sort();
    items.dedup();

    let enchantments = object
        .get("predicates")
        .and_then(Value::as_object)
        .and_then(|predicates| predicates.get("minecraft:enchantments"))
        .and_then(Value::as_array)
        .map(|predicates| {
            predicates
                .iter()
                .filter_map(parse_enchantment_predicate)
                .collect()
        })
        .unwrap_or_default();

    ToolPredicate {
        items,
        enchantments,
    }
}

fn parse_enchantment_predicate(value: &Value) -> Option<EnchantmentPredicate> {
    let object = value.as_object()?;
    let enchantments = names(object.get("enchantments"));
    if enchantments.is_empty() {
        return None;
    }
    let (min_level, max_level) = match object.get("levels") {
        Some(Value::Number(number)) => {
            let level = number.as_i64().unwrap_or(0) as i32;
            (level, Some(level))
        }
        Some(Value::Object(levels)) => (
            levels
                .get("min")
                .and_then(Value::as_i64)
                .map_or(0, |level| level as i32),
            levels
                .get("max")
                .and_then(Value::as_i64)
                .map(|level| level as i32),
        ),
        _ => (0, None),
    };
    Some(EnchantmentPredicate {
        enchantments,
        min_level,
        max_level,
    })
}

fn parse_block(object: &Map<String, Value>) -> BlockPredicate {
    let mut blocks = Vec::new();
    if let Some(value) = object.get("blocks").or_else(|| object.get("block")) {
        match value {
            Value::String(block) => blocks.push(normalize_id(block)),
            Value::Array(values) => {
                for value in values {
                    if let Some(block) = value.as_str() {
                        blocks.push(normalize_id(block));
                    }
                }
            }
            _ => {}
        }
    }

    let properties = object
        .get("state")
        .or_else(|| object.get("properties"))
        .and_then(Value::as_object)
        .map(|properties| {
            properties
                .iter()
                .map(|(name, value)| (name.clone(), property_value(value)))
                .collect()
        })
        .unwrap_or_default();

    BlockPredicate { blocks, properties }
}

/// Collects item ids from a string, a `#tag`, or an array of those.
pub(crate) fn collect_item_refs(
    value: Option<&Value>,
    definitions: &Definitions,
    items: &mut Vec<String>,
) {
    match value {
        Some(Value::String(item)) => {
            if let Some(tag) = item.strip_prefix('#') {
                if let Some(members) = definitions.tags.items(tag) {
                    items.extend(members.iter().cloned());
                }
            } else {
                items.push(normalize_id(item));
            }
        }
        Some(Value::Array(values)) => {
            for value in values {
                collect_item_refs(Some(value), definitions, items);
            }
        }
        _ => {}
    }
}

fn names(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(name)) => vec![normalize_id(name)],
        Some(Value::Array(names)) => names
            .iter()
            .filter_map(Value::as_str)
            .map(normalize_id)
            .collect(),
        _ => Vec::new(),
    }
}

fn floats(value: Option<&Value>) -> Vec<f32> {
    value
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .map(|value| value.as_f64().unwrap_or(0.0) as f32)
                .collect()
        })
        .unwrap_or_default()
}

fn float(value: Option<&Value>) -> f64 {
    value.and_then(Value::as_f64).unwrap_or(0.0)
}

fn property_value(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        _ => String::new(),
    }
}

/// `location_check` predicate: `biomes` (ids and tags) and `block` (`blocks`, `state`).
fn location_matches(
    predicate: &Value,
    pos: [i32; 3],
    host: &dyn crate::LootHost,
    data: &crate::data::GameData,
) -> bool {
    let Some(object) = predicate.as_object() else {
        return false;
    };
    object.iter().all(|(key, wanted)| match key.as_str() {
        "biomes" => host.biome_at(pos).is_some_and(|biome| {
            data.resolve_ids("worldgen/biome", wanted)
                .contains(&normalize_id(&biome))
        }),
        "block" => host.block_at(pos).is_some_and(|(block, state)| {
            let blocks_ok = wanted.get("blocks").is_none_or(|blocks| {
                data.resolve_ids("block", blocks)
                    .contains(&normalize_id(&block))
            });
            let state_ok = wanted
                .get("state")
                .and_then(Value::as_object)
                .is_none_or(|props| {
                    props
                        .iter()
                        .all(|(name, value)| value.as_str() == state.get(name).map(String::as_str))
                });
            blocks_ok && state_ok
        }),
        _ => false,
    })
}
