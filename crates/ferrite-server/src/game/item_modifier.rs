//! Data-pack item modifiers (`data/<namespace>/item_modifier/**/*.json`), as `/item modify` and
//! `/item ... from ... <modifier>` apply them to item stacks.
//!
//! Vanilla ships no item modifiers, so everything here is exercised with fixtures. A modifier is one
//! function object or a list of them (a `minecraft:sequence` nests more). Supported functions:
//! `set_count`, `limit_count`, `set_damage`, `set_components`, `set_name`, `set_lore` (replace),
//! `set_enchantments`. Conditions are limited to `minecraft:random_chance` with a constant chance. Any
//! other function or condition makes the whole modifier fail to load with a reason that names it, so
//! nothing is silently skipped.

use ferrite_item::{Component, ItemStack, component_ids as ids};
use ferrite_nbt::{Compound, Tag};
use serde_json::Value;

/// A number provider: a constant, or an inclusive uniform range.
#[derive(Debug, Clone, PartialEq)]
enum Number {
    Constant(f64),
    Uniform(f64, f64),
}

impl Number {
    fn parse(value: Option<&Value>, what: &str) -> Result<Self, String> {
        match value {
            Some(Value::Number(number)) => number
                .as_f64()
                .map(Self::Constant)
                .ok_or_else(|| format!("{what} is not a number")),
            Some(Value::Object(object)) => {
                let kind = object.get("type").and_then(Value::as_str);
                if kind.is_some_and(|kind| kind != "minecraft:uniform" && kind != "uniform")
                    && !(kind.is_none() && object.contains_key("min"))
                {
                    return Err(format!("{what}: unsupported number provider {kind:?}"));
                }
                let bound = |key: &str| {
                    object
                        .get(key)
                        .and_then(Value::as_f64)
                        .ok_or_else(|| format!("{what} needs a numeric '{key}'"))
                };
                let (min, max) = (bound("min")?, bound("max")?);
                if min > max {
                    return Err(format!("{what} range has min above max"));
                }
                Ok(Self::Uniform(min, max))
            }
            _ => Err(format!("{what} is missing")),
        }
    }

    fn eval(&self, rng: &mut ferrite_entity::Rng) -> f64 {
        match *self {
            Self::Constant(value) => value,
            Self::Uniform(min, max) => {
                let span = (max - min).floor() as i32 + 1;
                min + f64::from(rng.next_int(span.max(1)))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Step {
    SetCount {
        count: Number,
        add: bool,
    },
    LimitCount {
        min: Option<i32>,
        max: Option<i32>,
    },
    SetDamage {
        damage: Number,
        add: bool,
    },
    SetComponents(Vec<(u16, Tag)>),
    SetName(Tag),
    SetLore(Tag),
    SetEnchantments {
        levels: Vec<(String, Number)>,
        add: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
struct Function {
    chance: Option<f64>,
    step: Step,
}

/// A parsed item modifier.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemModifier {
    functions: Vec<Function>,
}

fn tag_of(value: &Value) -> Result<Tag, String> {
    ferrite_nbt::snbt::parse(&value.to_string()).map_err(|error| format!("invalid value: {error}"))
}

fn id_of(text: &str) -> String {
    if text.contains(':') {
        text.to_owned()
    } else {
        format!("minecraft:{text}")
    }
}

fn parse_into(value: &Value, out: &mut Vec<Function>) -> Result<(), String> {
    if let Value::Array(list) = value {
        return list.iter().try_for_each(|entry| parse_into(entry, out));
    }
    let object = value
        .as_object()
        .ok_or("an item modifier is a function or a list of functions")?;
    let mut chance = None;
    if let Some(conditions) = object.get("conditions") {
        for condition in conditions.as_array().ok_or("conditions must be a list")? {
            let kind = condition
                .get("condition")
                .and_then(Value::as_str)
                .map(id_of)
                .unwrap_or_default();
            if kind != "minecraft:random_chance" {
                return Err(format!("unsupported item modifier condition '{kind}'"));
            }
            let Some(Value::Number(number)) = condition.get("chance") else {
                return Err("random_chance needs a constant 'chance'".into());
            };
            chance = Some(number.as_f64().unwrap_or(1.0));
        }
    }
    let kind = object
        .get("function")
        .and_then(Value::as_str)
        .map(id_of)
        .ok_or("a function object needs a 'function' type")?;
    let step = match kind.as_str() {
        "minecraft:sequence" => {
            let inner = object
                .get("functions")
                .ok_or("sequence needs 'functions'")?;
            let mut nested = Vec::new();
            parse_into(inner, &mut nested)?;
            // A sequence's own chance gates every nested function.
            for mut function in nested {
                function.chance = match (chance, function.chance) {
                    (Some(a), Some(b)) => Some(a * b),
                    (a, b) => a.or(b),
                };
                out.push(function);
            }
            return Ok(());
        }
        "minecraft:set_count" => Step::SetCount {
            count: Number::parse(object.get("count"), "set_count count")?,
            add: object.get("add").and_then(Value::as_bool).unwrap_or(false),
        },
        "minecraft:limit_count" => {
            let limit = object.get("limit");
            let bound = |key: &str| limit.and_then(|l| l.get(key)).and_then(Value::as_i64);
            let (min, max) = match limit {
                Some(Value::Number(number)) => (None, number.as_i64()),
                _ => (bound("min"), bound("max")),
            };
            if min.is_none() && max.is_none() {
                return Err("limit_count needs a 'limit' with min or max".into());
            }
            Step::LimitCount {
                min: min.map(|v| v as i32),
                max: max.map(|v| v as i32),
            }
        }
        "minecraft:set_damage" => Step::SetDamage {
            damage: Number::parse(object.get("damage"), "set_damage damage")?,
            add: object.get("add").and_then(Value::as_bool).unwrap_or(false),
        },
        "minecraft:set_components" => {
            let components = object
                .get("components")
                .and_then(Value::as_object)
                .ok_or("set_components needs 'components'")?;
            let mut set = Vec::new();
            for (name, value) in components {
                let id = ferrite_item::component_id(name)
                    .ok_or_else(|| format!("unknown item component '{name}'"))?;
                set.push((id, tag_of(value)?));
            }
            Step::SetComponents(set)
        }
        "minecraft:set_name" => {
            if object.get("target").is_some_and(|t| t != "custom_name") {
                return Err("set_name only supports the custom_name target".into());
            }
            Step::SetName(tag_of(object.get("name").ok_or("set_name needs 'name'")?)?)
        }
        "minecraft:set_lore" => {
            if object
                .get("mode")
                .and_then(Value::as_str)
                .is_some_and(|m| m != "replace_all")
            {
                return Err("set_lore only supports mode replace_all".into());
            }
            Step::SetLore(tag_of(object.get("lore").ok_or("set_lore needs 'lore'")?)?)
        }
        "minecraft:set_enchantments" => {
            let enchantments = object
                .get("enchantments")
                .and_then(Value::as_object)
                .ok_or("set_enchantments needs 'enchantments'")?;
            let mut levels = Vec::new();
            for (name, level) in enchantments {
                levels.push((
                    id_of(name),
                    Number::parse(Some(level), "enchantment level")?,
                ));
            }
            Step::SetEnchantments {
                levels,
                add: object.get("add").and_then(Value::as_bool).unwrap_or(false),
            }
        }
        other => return Err(format!("unsupported item modifier function '{other}'")),
    };
    out.push(Function { chance, step });
    Ok(())
}

impl ItemModifier {
    /// Parses a modifier from its JSON text; the error names the unsupported or invalid part.
    pub fn parse(text: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
        let mut functions = Vec::new();
        parse_into(&value, &mut functions)?;
        Ok(Self { functions })
    }

    /// Applies every function in order. An empty result (a count driven to zero) is an empty stack.
    pub fn apply(&self, mut stack: ItemStack, rng: &mut ferrite_entity::Rng) -> ItemStack {
        for function in &self.functions {
            if stack.is_empty() {
                break;
            }
            if let Some(chance) = function.chance
                && f64::from(rng.next_int(1_000_000)) / 1_000_000.0 >= chance
            {
                continue;
            }
            match &function.step {
                Step::SetCount { count, add } => {
                    let value = count.eval(rng).floor() as i32;
                    stack.count = if *add { stack.count + value } else { value };
                    stack.count = stack.count.clamp(0, stack.max_stack_size().max(1));
                }
                Step::LimitCount { min, max } => {
                    if let Some(max) = max {
                        stack.count = stack.count.min(*max);
                    }
                    if let Some(min) = min {
                        stack.count = stack.count.max(*min);
                    }
                }
                Step::SetDamage { damage, add } => {
                    let max = ferrite_item::item_defaults(stack.item)
                        .and_then(|d| d.max_damage)
                        .unwrap_or(0);
                    if max > 0 {
                        // The provider is the fraction of durability left, like vanilla.
                        let left = f64::from(max - stack.damage()) / f64::from(max);
                        let fraction =
                            (if *add { left } else { 0.0 } + damage.eval(rng)).clamp(0.0, 1.0);
                        let used = ((1.0 - fraction) * f64::from(max)).floor() as i32;
                        stack
                            .components
                            .insert(ids::DAMAGE, Component::Damage(used));
                    }
                }
                Step::SetComponents(set) => {
                    for (id, tag) in set {
                        stack.components.insert(*id, Component::from_nbt(*id, tag));
                    }
                }
                Step::SetName(tag) => {
                    stack
                        .components
                        .insert(ids::CUSTOM_NAME, Component::from_nbt(ids::CUSTOM_NAME, tag));
                }
                Step::SetLore(tag) => {
                    stack
                        .components
                        .insert(ids::LORE, Component::from_nbt(ids::LORE, tag));
                }
                Step::SetEnchantments { levels, add } => {
                    let mut compound = Compound::new();
                    if *add
                        && let Some(existing @ Component::Enchantments(_)) =
                            stack.effective_component(ids::ENCHANTMENTS)
                        && let Ok(Tag::Compound(current)) = existing.to_nbt()
                    {
                        compound = current;
                    }
                    for (name, level) in levels {
                        let value = level.eval(rng).floor() as i32;
                        let previous = match compound.get(name) {
                            Some(Tag::Int(previous)) if *add => *previous,
                            _ => 0,
                        };
                        let total = (previous + value).clamp(0, 255);
                        if total > 0 {
                            compound.insert(name.clone(), Tag::Int(total));
                        } else {
                            compound.shift_remove(name);
                        }
                    }
                    stack.components.insert(
                        ids::ENCHANTMENTS,
                        Component::from_nbt(ids::ENCHANTMENTS, &Tag::Compound(compound)),
                    );
                }
            }
        }
        if stack.count <= 0 {
            ItemStack::empty()
        } else {
            stack
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stone(count: i32) -> ItemStack {
        ItemStack::with_count(ferrite_item::item_id("minecraft:stone").unwrap(), count)
    }

    fn pickaxe() -> ItemStack {
        ItemStack::with_count(
            ferrite_item::item_id("minecraft:diamond_pickaxe").unwrap(),
            1,
        )
    }

    fn run(json: &str, stack: ItemStack) -> ItemStack {
        ItemModifier::parse(json)
            .unwrap()
            .apply(stack, &mut ferrite_entity::Rng::new(7))
    }

    #[test]
    fn set_count_sets_adds_clamps_and_empties() {
        assert_eq!(
            run(r#"{"function":"minecraft:set_count","count":5}"#, stone(1)).count,
            5
        );
        assert_eq!(
            run(r#"{"function":"set_count","count":3,"add":true}"#, stone(4)).count,
            7
        );
        assert_eq!(
            run(r#"{"function":"set_count","count":500}"#, stone(1)).count,
            64,
            "max stack"
        );
        assert!(run(r#"{"function":"set_count","count":0}"#, stone(9)).is_empty());
        let ranged = run(
            r#"{"function":"set_count","count":{"type":"minecraft:uniform","min":2,"max":4}}"#,
            stone(1),
        );
        assert!((2..=4).contains(&ranged.count));
    }

    #[test]
    fn limit_count_bounds_the_stack() {
        assert_eq!(
            run(
                r#"{"function":"limit_count","limit":{"max":10}}"#,
                stone(40)
            )
            .count,
            10
        );
        assert_eq!(
            run(r#"{"function":"limit_count","limit":{"min":3}}"#, stone(1)).count,
            3
        );
    }

    #[test]
    fn components_name_lore_and_enchantments_are_applied_in_list_order() {
        let modifier = r#"[
            {"function":"set_components","components":{"minecraft:damage":7}},
            {"function":"set_name","name":"Fancy"},
            {"function":"set_enchantments","enchantments":{"minecraft:sharpness":3}},
            {"function":"set_enchantments","enchantments":{"minecraft:sharpness":2,"unbreaking":1},"add":true}
        ]"#;
        let out = run(modifier, pickaxe());
        assert_eq!(out.damage(), 7);
        assert!(out.components.get(ids::CUSTOM_NAME).is_some());
        let Some(Component::Enchantments(levels)) = out.components.get(ids::ENCHANTMENTS) else {
            panic!("enchantments set");
        };
        assert_eq!(levels.level("minecraft:sharpness"), Some(5));
        assert_eq!(levels.level("minecraft:unbreaking"), Some(1));
    }

    #[test]
    fn set_damage_is_a_fraction_of_durability_left() {
        let max = ferrite_item::item_defaults(pickaxe().item)
            .unwrap()
            .max_damage
            .unwrap();
        assert_eq!(
            run(r#"{"function":"set_damage","damage":0.5}"#, pickaxe()).damage(),
            max / 2
        );
        assert_eq!(
            run(r#"{"function":"set_damage","damage":1.0}"#, pickaxe()).damage(),
            0
        );
        // Non-damageable items are left alone.
        assert_eq!(
            run(r#"{"function":"set_damage","damage":0.5}"#, stone(2)),
            stone(2)
        );
    }

    #[test]
    fn chance_and_sequence_gate_functions() {
        let never = r#"{"function":"set_count","count":9,"conditions":[{"condition":"minecraft:random_chance","chance":0}]}"#;
        assert_eq!(run(never, stone(1)).count, 1);
        let always = r#"{"function":"sequence","conditions":[{"condition":"random_chance","chance":1}],"functions":[{"function":"set_count","count":2},{"function":"set_count","count":3,"add":true}]}"#;
        assert_eq!(run(always, stone(1)).count, 5);
    }

    #[test]
    fn unsupported_or_invalid_parts_fail_loading_with_their_name() {
        for (json, needle) in [
            (
                r#"{"function":"minecraft:enchant_randomly"}"#,
                "enchant_randomly",
            ),
            (
                r#"{"function":"set_count","count":1,"conditions":[{"condition":"minecraft:weather_check"}]}"#,
                "weather_check",
            ),
            (
                r#"{"function":"set_components","components":{"minecraft:nope":1}}"#,
                "minecraft:nope",
            ),
            (r#"{"function":"set_count"}"#, "count"),
            (r#"{"count":1}"#, "function"),
            (r#"[1]"#, "function"),
            ("not json", "expected"),
        ] {
            let error = ItemModifier::parse(json).unwrap_err();
            assert!(error.contains(needle), "{json}: {error}");
        }
    }
}
