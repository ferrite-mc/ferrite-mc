//! Loot functions attached to pools, entries and whole tables.
//!
//! `copy_components` copies block entity components onto the stack. Functions that only touch
//! other item metadata (names, enchantments, potions, ...) are kept opaque: the item id,
//! count and components pass through unchanged.

use crate::condition::{self, Condition};
use crate::number::{IntRange, NumberProvider};
use crate::{Definitions, LootContext, LootStack, Rng, normalize_id};
use serde_json::Value;

/// One loot function and the conditions guarding it.
#[derive(Debug, Clone)]
pub struct Function {
    /// Conditions that must hold before the function runs.
    pub conditions: Vec<Condition>,
    /// What the function does to a stack.
    pub kind: FunctionKind,
}

/// The effect of a loot function on an `(item id, count)` stack.
#[derive(Debug, Clone)]
pub enum FunctionKind {
    /// Replaces or adds to the stack count.
    SetCount {
        /// Count provider (`constant`, `uniform` or `binomial`).
        count: NumberProvider,
        /// Whether `count` is added to the current count instead of replacing it.
        add: bool,
    },
    /// Scales the count by an enchantment level.
    ApplyBonus {
        /// Enchantment supplying the level (`minecraft:fortune` or `minecraft:looting`).
        enchantment: String,
        /// Vanilla bonus formula.
        formula: BonusFormula,
    },
    /// Drops individual items when an explosion destroys the block.
    ExplosionDecay,
    /// Clamps the count to a range.
    LimitCount {
        /// Inclusive bounds applied to the count.
        limit: IntRange,
    },
    /// Copies components from the block entity that was broken into the stack.
    CopyComponents {
        /// Where the components come from.
        source: ComponentSource,
        /// Components to copy; `None` copies every component not in `exclude`.
        include: Option<Vec<String>>,
        /// Components never copied.
        exclude: Vec<String>,
    },
    /// A function this crate passes through untouched, retaining its JSON.
    Opaque {
        /// Vanilla function type.
        kind: String,
        /// Raw function JSON.
        value: Value,
    },
}

/// The component source of `copy_components`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentSource {
    /// The block entity of the broken block ([`LootContext::block_entity_components`]).
    BlockEntity,
    /// An entity source (`this`, `attacker`, ...) this crate has no model of; copies nothing.
    Unsupported,
}

/// The `apply_bonus` formulas vanilla 26.3 writes.
#[derive(Debug, Clone, PartialEq)]
pub enum BonusFormula {
    /// Fortune ore drops: `1..=level + 1` times the base count, with the first
    /// multiplier twice as likely as the others.
    OreDrops,
    /// Adds `0..=bonus_multiplier * level` to the count.
    UniformBonusCount {
        /// Multiplier per enchantment level.
        bonus_multiplier: i32,
    },
    /// Adds one success per `extra + level` binomial trials.
    BinomialWithBonusCount {
        /// Trials beyond the enchantment level.
        extra: i32,
        /// Success probability in `[0, 1]`.
        probability: f32,
    },
    /// A formula this crate does not implement; leaves the count unchanged.
    Unknown,
}

/// Parses a `modifier`/`functions` field, which may be one function or an
/// array of them.
pub(crate) fn parse_functions(value: &Value, definitions: &Definitions) -> Vec<Function> {
    match value {
        Value::Array(functions) => functions
            .iter()
            .filter_map(|function| parse(function, definitions))
            .collect(),
        function => parse(function, definitions).into_iter().collect(),
    }
}

fn parse(value: &Value, definitions: &Definitions) -> Option<Function> {
    let object = value.as_object()?;
    let conditions = ["condition", "conditions"]
        .iter()
        .filter_map(|key| object.get(*key))
        .flat_map(|value| condition::parse_list(value, definitions))
        .collect();
    let kind_name = object.get("type").and_then(Value::as_str).unwrap_or("");
    let kind = match kind_name {
        "minecraft:set_count" => FunctionKind::SetCount {
            count: NumberProvider::parse(object.get("count")),
            add: object.get("add").and_then(Value::as_bool).unwrap_or(false),
        },
        "minecraft:apply_bonus" => FunctionKind::ApplyBonus {
            enchantment: object
                .get("enchantment")
                .and_then(Value::as_str)
                .unwrap_or("minecraft:fortune")
                .to_owned(),
            formula: parse_formula(object),
        },
        "minecraft:explosion_decay" => FunctionKind::ExplosionDecay,
        "minecraft:limit_count" => FunctionKind::LimitCount {
            limit: IntRange::parse(object.get("limit")),
        },
        "minecraft:copy_components" => FunctionKind::CopyComponents {
            source: match object.get("source").and_then(Value::as_str) {
                Some("block_entity") => ComponentSource::BlockEntity,
                _ => ComponentSource::Unsupported,
            },
            include: object.get("include").map(component_names),
            exclude: object
                .get("exclude")
                .map(component_names)
                .unwrap_or_default(),
        },
        other => FunctionKind::Opaque {
            kind: other.to_owned(),
            value: value.clone(),
        },
    };
    Some(Function { conditions, kind })
}

/// The component names of an `include`/`exclude` list, namespaced.
fn component_names(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|names| {
            names
                .iter()
                .filter_map(Value::as_str)
                .map(normalize_id)
                .collect()
        })
        .unwrap_or_default()
}

/// Applies every function whose conditions hold to one stack.
pub(crate) fn apply(
    functions: &[Function],
    stack: &mut LootStack,
    context: &LootContext,
    rng: &mut impl Rng,
) {
    for function in functions {
        if function
            .conditions
            .iter()
            .all(|condition| condition.test(context, rng))
        {
            function.kind.apply(stack, context, rng);
            stack.count = stack.count.max(0);
        }
    }
}

impl FunctionKind {
    fn apply(&self, stack: &mut LootStack, context: &LootContext, rng: &mut impl Rng) {
        match self {
            Self::SetCount { count, add } => {
                let count = count.get_int(rng);
                stack.count = if *add {
                    stack.count.saturating_add(count)
                } else {
                    count
                };
            }
            Self::ApplyBonus {
                enchantment,
                formula,
            } => {
                let level = context.enchantment_level(enchantment).max(0);
                stack.count = formula.apply(stack.count, level, rng);
            }
            Self::ExplosionDecay => {
                let Some(radius) = context.explosion_radius else {
                    return;
                };
                if radius <= 0.0 {
                    return;
                }
                let keep = 1.0 / radius;
                if keep >= 1.0 {
                    return;
                }
                let mut kept = 0;
                for _ in 0..stack.count.max(0) {
                    if rng.next_float() < keep {
                        kept += 1;
                    }
                }
                stack.count = kept;
            }
            Self::LimitCount { limit } => {
                stack.count = limit.clamp(stack.count);
            }
            Self::CopyComponents {
                source,
                include,
                exclude,
            } => {
                if *source != ComponentSource::BlockEntity {
                    return;
                }
                for (name, value) in &context.block_entity_components {
                    let wanted = include
                        .as_ref()
                        .is_none_or(|include| include.contains(name));
                    if wanted && !exclude.contains(name) {
                        stack.components.insert(name.clone(), value.clone());
                    }
                }
            }
            Self::Opaque { .. } => {}
        }
    }
}

impl BonusFormula {
    fn apply(&self, base: i32, level: i32, rng: &mut impl Rng) -> i32 {
        match self {
            Self::OreDrops => {
                if level <= 0 {
                    return base;
                }
                let roll = rng.next_int(level + 2) - 1;
                base.saturating_mul(roll.max(0) + 1)
            }
            Self::UniformBonusCount { bonus_multiplier } => {
                let span = bonus_multiplier.saturating_mul(level).saturating_add(1);
                if span <= 0 {
                    return base;
                }
                base.saturating_add(rng.next_int(span))
            }
            Self::BinomialWithBonusCount { extra, probability } => {
                let trials = extra.saturating_add(level).max(0);
                let probability = probability.clamp(0.0, 1.0);
                let mut successes = 0;
                for _ in 0..trials {
                    if rng.next_float() < probability {
                        successes += 1;
                    }
                }
                base.saturating_add(successes)
            }
            Self::Unknown => base,
        }
    }
}

fn parse_formula(object: &serde_json::Map<String, Value>) -> BonusFormula {
    let parameters = object.get("parameters").and_then(Value::as_object);
    let parameter = |name: &str| parameters.and_then(|parameters| parameters.get(name));
    match object.get("formula").and_then(Value::as_str).unwrap_or("") {
        "minecraft:ore_drops" => BonusFormula::OreDrops,
        "minecraft:uniform_bonus_count" => BonusFormula::UniformBonusCount {
            bonus_multiplier: parameter("bonusMultiplier")
                .and_then(Value::as_i64)
                .unwrap_or(1) as i32,
        },
        "minecraft:binomial_with_bonus_count" => BonusFormula::BinomialWithBonusCount {
            extra: parameter("extra").and_then(Value::as_i64).unwrap_or(0) as i32,
            probability: parameter("probability")
                .and_then(Value::as_f64)
                .unwrap_or(0.0) as f32,
        },
        _ => BonusFormula::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(u64);

    impl Rng for Fixed {
        fn next_int(&mut self, bound: i32) -> i32 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
            ((self.0 >> 33) % bound.max(1) as u64) as i32
        }

        fn next_float(&mut self) -> f32 {
            self.next_int(1 << 24) as f32 / (1 << 24) as f32
        }
    }

    #[test]
    fn ore_drops_without_fortune_keeps_the_base() {
        assert_eq!(BonusFormula::OreDrops.apply(1, 0, &mut Fixed(1)), 1);
    }

    #[test]
    fn ore_drops_with_fortune_scales_the_base() {
        let mut rng = Fixed(2);
        for _ in 0..100 {
            let value = BonusFormula::OreDrops.apply(1, 3, &mut rng);
            assert!((1..=4).contains(&value));
        }
    }

    #[test]
    fn uniform_bonus_stays_within_the_multiplier() {
        let mut rng = Fixed(3);
        for _ in 0..100 {
            let value = BonusFormula::UniformBonusCount {
                bonus_multiplier: 2,
            }
            .apply(1, 3, &mut rng);
            assert!((1..=7).contains(&value));
        }
    }

    #[test]
    fn opaque_functions_pass_the_stack_through() {
        let function = Function {
            conditions: Vec::new(),
            kind: FunctionKind::Opaque {
                kind: "minecraft:set_name".to_owned(),
                value: serde_json::json!({"name": "test"}),
            },
        };
        let mut stack = LootStack {
            count: 3,
            ..LootStack::new("minecraft:stick")
        };
        apply(
            std::slice::from_ref(&function),
            &mut stack,
            &LootContext::default(),
            &mut Fixed(4),
        );
        assert_eq!(
            stack,
            LootStack {
                count: 3,
                ..LootStack::new("minecraft:stick")
            }
        );
    }
}
