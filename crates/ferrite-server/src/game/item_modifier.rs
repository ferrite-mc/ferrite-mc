//! Data-pack item modifiers (`data/<namespace>/item_modifier/**/*.json`), as `/item modify` and
//! `/item ... from ... <modifier>` apply them to item stacks.
//!
//! Vanilla ships no item modifiers (the generated data has no `item_modifier` directory), so
//! everything here is exercised with fixtures. A modifier is one function object or a list of
//! them (a `minecraft:sequence` nests more). In 26.3 the function and condition type is the
//! `"type"` key (older data used `"function"`/`"condition"`, still accepted).
//!
//! Supported functions: `set_count`, `limit_count`, `set_damage`, `set_components`, `set_name`
//! (`custom_name` and `item_name`), `set_lore` (all four list modes), `set_enchantments`,
//! `set_item`, `set_potion`, `set_custom_data`, `set_ominous_bottle_amplifier`, `discard`,
//! `explosion_decay` (a no-op: the command context has no explosion) and `sequence`.
//! Conditions: `random_chance` with a constant chance, and `inverted`/`all_of`/`any_of` over
//! those. Any other function or condition makes the whole modifier fail to load with a reason
//! that names it, so nothing is silently skipped. See `docs/COMMANDS-STATUS.md` for why the
//! remaining vanilla functions are not supported.

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
                let span = (max - min).floor() + 1.0;
                if !span.is_finite() || span > f64::from(i32::MAX) {
                    return Err(format!(
                        "{what} uniform range exceeds the supported sampler span"
                    ));
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
    /// `set_name`: the text and whether it targets `item_name` instead of `custom_name`.
    SetName {
        name: Tag,
        item_name: bool,
    },
    SetLore {
        lines: Vec<Tag>,
        op: ListOp,
    },
    SetItem(u16),
    /// `set_potion`: the potion id (`minecraft:healing`).
    SetPotion(String),
    /// `set_custom_data`: merged into the stack's custom data.
    SetCustomData(Compound),
    SetOminousBottleAmplifier(Number),
    /// `discard` (and nothing else of the filter machinery): the stack disappears.
    Discard,
    /// `explosion_decay` needs an explosion radius, which the command context does not have.
    ExplosionDecay,
    SetEnchantments {
        levels: Vec<(String, Number)>,
        add: bool,
    },
    /// Every function the loot crate evaluates (enchanting, smelting, attributes, ...), run
    /// through `ferrite_loot` with the stack converted to its NBT components and back.
    Loot(LootStep),
}

/// A `ferrite_loot` function; two steps are equal when they print the same.
#[derive(Debug, Clone)]
struct LootStep(ferrite_loot::Function);

impl PartialEq for LootStep {
    fn eq(&self, other: &Self) -> bool {
        format!("{:?}", self.0) == format!("{:?}", other.0)
    }
}

/// The item modifier's loot context: vanilla `LootContextParamSets.COMMAND` gives the origin
/// and `this` entity, and nothing about a kill.
fn loot_context() -> ferrite_loot::LootContext {
    ferrite_loot::LootContext {
        this_entity: Some(ferrite_loot::EntityFacts::default()),
        host: Some(super::drops::default_host()),
        ..ferrite_loot::LootContext::default()
    }
}

fn to_loot(stack: &ItemStack) -> Option<ferrite_loot::LootStack> {
    let mut out = ferrite_loot::LootStack::new(ferrite_item::item_name(stack.item)?);
    out.count = stack.count;
    for (id, component) in stack.components.added() {
        if let (Some(name), Ok(tag)) = (ferrite_item::component_name(id), component.to_nbt()) {
            out.components.insert(name.to_owned(), tag);
        }
    }
    Some(out)
}

fn from_loot(stack: ferrite_loot::LootStack, previous: &ItemStack) -> ItemStack {
    let Some(item) = ferrite_item::item_id(&stack.item) else {
        return ItemStack::empty();
    };
    let mut out = ItemStack::with_count(item, stack.count);
    // A function that swaps the item (smelting, an enchanted book) starts from its defaults.
    if item == previous.item {
        out.components = previous.components.clone();
    }
    for (name, tag) in &stack.components {
        if let Some(id) = ferrite_item::component_id(name) {
            out.components.insert(id, Component::from_nbt(id, tag));
        }
    }
    out
}

/// A function condition.
#[derive(Debug, Clone, PartialEq)]
enum Cond {
    /// `minecraft:random_chance` with a constant chance.
    Chance(f64),
    Inverted(Box<Cond>),
    AllOf(Vec<Cond>),
    AnyOf(Vec<Cond>),
}

impl Cond {
    fn test(&self, rng: &mut ferrite_entity::Rng) -> bool {
        match self {
            Self::Chance(chance) => f64::from(rng.next_int(1_000_000)) / 1_000_000.0 < *chance,
            Self::Inverted(inner) => !inner.test(rng),
            Self::AllOf(terms) => terms.iter().all(|term| term.test(rng)),
            Self::AnyOf(terms) => terms.iter().any(|term| term.test(rng)),
        }
    }

    fn parse(value: &Value) -> Result<Self, String> {
        let kind = value
            .get("type")
            .or_else(|| value.get("condition"))
            .and_then(Value::as_str)
            .map(id_of)
            .ok_or("a condition needs a 'type'")?;
        let terms = |key: &str| -> Result<Vec<Cond>, String> {
            value
                .get(key)
                .and_then(Value::as_array)
                .ok_or_else(|| format!("{kind} needs '{key}'"))?
                .iter()
                .map(Cond::parse)
                .collect()
        };
        match kind.as_str() {
            "minecraft:random_chance" => {
                let chance = match value.get("chance") {
                    Some(Value::Number(number)) => number.as_f64(),
                    Some(Value::Object(object)) => object.get("value").and_then(Value::as_f64),
                    _ => None,
                };
                chance
                    .map(Self::Chance)
                    .ok_or_else(|| "random_chance needs a constant 'chance'".to_owned())
            }
            "minecraft:inverted" => Ok(Self::Inverted(Box::new(Self::parse(
                value.get("term").ok_or("inverted needs 'term'")?,
            )?))),
            "minecraft:all_of" => Ok(Self::AllOf(terms("terms")?)),
            "minecraft:any_of" => Ok(Self::AnyOf(terms("terms")?)),
            other => Err(format!("unsupported item modifier condition '{other}'")),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Function {
    conditions: Vec<Cond>,
    step: Step,
}

/// `ListOperation` of `set_lore`.
#[derive(Debug, Clone, PartialEq)]
enum ListOp {
    ReplaceAll,
    ReplaceSection { offset: usize, size: Option<usize> },
    Insert { offset: usize },
    Append,
}

impl ListOp {
    fn parse(object: &serde_json::Map<String, Value>) -> Result<Self, String> {
        let offset = |what: &str| -> Result<usize, String> {
            match object.get("offset") {
                None => Ok(0),
                Some(value) => value
                    .as_u64()
                    .map(|n| n as usize)
                    .ok_or_else(|| format!("{what} offset must be a non-negative integer")),
            }
        };
        match object.get("mode").and_then(Value::as_str) {
            None | Some("replace_all") => Ok(Self::ReplaceAll),
            Some("replace_section") => Ok(Self::ReplaceSection {
                offset: offset("replace_section")?,
                size: object
                    .get("size")
                    .and_then(Value::as_u64)
                    .map(|n| n as usize),
            }),
            Some("insert") => Ok(Self::Insert {
                offset: offset("insert")?,
            }),
            Some("append") => Ok(Self::Append),
            Some(other) => Err(format!("unknown set_lore mode '{other}'")),
        }
    }

    /// Applies the operation to `current`, or `None` when vanilla's bounds checks reject it.
    fn apply(&self, current: Vec<Tag>, lines: Vec<Tag>) -> Option<Vec<Tag>> {
        const LIMIT: usize = 256;
        let result = match self {
            Self::ReplaceAll => lines,
            Self::Append => current.into_iter().chain(lines).collect(),
            Self::Insert { offset } => {
                if *offset > current.len() {
                    return None;
                }
                let mut out = current;
                out.splice(*offset..*offset, lines);
                out
            }
            Self::ReplaceSection { offset, size } => {
                if *offset > current.len() {
                    return None;
                }
                let end = (*offset + size.unwrap_or(lines.len())).min(current.len());
                let mut out = current;
                out.splice(*offset..end, lines);
                out
            }
        };
        (result.len() <= LIMIT).then_some(result)
    }
}

/// Lore lines as one homogeneous NBT list (strings stay strings unless a compound is present).
fn lore_list(lines: Vec<Tag>) -> Tag {
    if lines.iter().any(|line| matches!(line, Tag::Compound(_))) {
        let compounds = lines
            .into_iter()
            .map(|line| match line {
                Tag::Compound(compound) => compound,
                Tag::String(text) => {
                    let mut compound = Compound::new();
                    compound.insert("text".to_owned(), Tag::String(text));
                    compound
                }
                _ => Compound::new(),
            })
            .collect();
        Tag::List(ferrite_nbt::List::Compound(compounds))
    } else {
        let strings = lines
            .into_iter()
            .filter_map(|line| match line {
                Tag::String(text) => Some(text),
                _ => None,
            })
            .collect::<Vec<_>>();
        if strings.is_empty() {
            Tag::List(ferrite_nbt::List::End)
        } else {
            Tag::List(ferrite_nbt::List::String(strings))
        }
    }
}

fn lore_lines(tag: Option<Tag>) -> Vec<Tag> {
    match tag {
        Some(Tag::List(ferrite_nbt::List::String(lines))) => {
            lines.into_iter().map(Tag::String).collect()
        }
        Some(Tag::List(ferrite_nbt::List::Compound(lines))) => {
            lines.into_iter().map(Tag::Compound).collect()
        }
        _ => Vec::new(),
    }
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

/// Function types this module evaluates itself; every other one goes to `ferrite_loot`.
const NATIVE: [&str; 14] = [
    "minecraft:sequence",
    "minecraft:set_count",
    "minecraft:limit_count",
    "minecraft:set_damage",
    "minecraft:set_components",
    "minecraft:set_name",
    "minecraft:set_lore",
    "minecraft:set_item",
    "minecraft:set_potion",
    "minecraft:set_custom_data",
    "minecraft:set_ominous_bottle_amplifier",
    "minecraft:discard",
    "minecraft:explosion_decay",
    "minecraft:set_enchantments",
];

/// Rewrites the pre-26 `function`/`condition` keys of a (nested) function to `type`, the key the
/// loot crate reads.
fn with_type_keys(value: &Value) -> Value {
    match value {
        Value::Array(list) => Value::Array(list.iter().map(with_type_keys).collect()),
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (key, inner) in map {
                let legacy = matches!(key.as_str(), "function" | "condition")
                    && !map.contains_key("type")
                    && inner.is_string();
                if legacy {
                    out.insert(
                        "type".to_owned(),
                        Value::String(id_of(inner.as_str().unwrap_or(""))),
                    );
                } else {
                    out.insert(key.clone(), with_type_keys(inner));
                }
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

fn parse_into(value: &Value, out: &mut Vec<Function>) -> Result<(), String> {
    if let Value::Array(list) = value {
        return list.iter().try_for_each(|entry| parse_into(entry, out));
    }
    let object = value
        .as_object()
        .ok_or("an item modifier is a function or a list of functions")?;
    let native_kind = object
        .get("type")
        .or_else(|| object.get("function"))
        .and_then(Value::as_str)
        .map(id_of);
    if let Some(kind) = native_kind.as_deref()
        && !NATIVE.contains(&kind)
    {
        let tables = super::drops::tables()
            .ok_or_else(|| format!("'{kind}' needs the vanilla loot data, which is not loaded"))?;
        let function = tables
            .parse_function(&with_type_keys(&Value::Object(object.clone())))
            .map_err(|error| format!("unsupported item modifier function: {error}"))?;
        out.push(Function {
            conditions: Vec::new(),
            step: Step::Loot(LootStep(function)),
        });
        return Ok(());
    }
    let mut conditions = Vec::new();
    for key in ["conditions", "condition"] {
        match object.get(key) {
            None => {}
            Some(Value::Array(list)) => {
                for condition in list {
                    conditions.push(Cond::parse(condition)?);
                }
            }
            Some(single @ Value::Object(_)) => conditions.push(Cond::parse(single)?),
            Some(_) => return Err(format!("'{key}' must be a condition or a list of them")),
        }
    }
    let kind = object
        .get("type")
        .or_else(|| object.get("function"))
        .and_then(Value::as_str)
        .map(id_of)
        .ok_or("a function object needs a 'type'")?;
    let step = match kind.as_str() {
        "minecraft:sequence" => {
            let inner = object
                .get("functions")
                .ok_or("sequence needs 'functions'")?;
            let mut nested = Vec::new();
            parse_into(inner, &mut nested)?;
            // A sequence's own conditions gate every nested function.
            for mut function in nested {
                let mut all = conditions.clone();
                all.append(&mut function.conditions);
                function.conditions = all;
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
            if object.get("entity").is_some() {
                return Err("set_name with an 'entity' (name resolution) is not supported".into());
            }
            let item_name = match object.get("target").and_then(Value::as_str) {
                None | Some("custom_name") => false,
                Some("item_name") => true,
                Some(other) => return Err(format!("unknown set_name target '{other}'")),
            };
            Step::SetName {
                name: tag_of(object.get("name").ok_or("set_name needs 'name'")?)?,
                item_name,
            }
        }
        "minecraft:set_lore" => {
            if object.get("entity").is_some() {
                return Err("set_lore with an 'entity' (name resolution) is not supported".into());
            }
            let lore = object.get("lore").ok_or("set_lore needs 'lore'")?;
            let lines = match lore {
                Value::Array(list) => list.iter().map(tag_of).collect::<Result<Vec<_>, _>>()?,
                single => vec![tag_of(single)?],
            };
            Step::SetLore {
                lines,
                op: ListOp::parse(object)?,
            }
        }
        "minecraft:set_item" => {
            let name = object
                .get("item")
                .and_then(Value::as_str)
                .ok_or("set_item needs 'item'")?;
            Step::SetItem(
                ferrite_item::item_id(&id_of(name))
                    .ok_or_else(|| format!("unknown item '{name}'"))?,
            )
        }
        "minecraft:set_potion" => Step::SetPotion(id_of(
            object
                .get("id")
                .and_then(Value::as_str)
                .ok_or("set_potion needs 'id'")?,
        )),
        "minecraft:set_custom_data" => {
            let tag = tag_of(object.get("tag").ok_or("set_custom_data needs 'tag'")?)?;
            let Tag::Compound(compound) = tag else {
                return Err("set_custom_data 'tag' must be an object".into());
            };
            Step::SetCustomData(compound)
        }
        "minecraft:set_ominous_bottle_amplifier" => Step::SetOminousBottleAmplifier(Number::parse(
            object.get("amplifier"),
            "set_ominous_bottle_amplifier amplifier",
        )?),
        "minecraft:discard" => Step::Discard,
        "minecraft:explosion_decay" => Step::ExplosionDecay,
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
    out.push(Function { conditions, step });
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
            if !function.conditions.iter().all(|cond| cond.test(rng)) {
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
                Step::SetName { name, item_name } => {
                    let id = if *item_name {
                        ids::ITEM_NAME
                    } else {
                        ids::CUSTOM_NAME
                    };
                    stack.components.insert(id, Component::from_nbt(id, name));
                }
                Step::SetLore { lines, op } => {
                    let current = lore_lines(
                        stack
                            .effective_component(ids::LORE)
                            .and_then(|lore| lore.to_nbt().ok()),
                    );
                    // A section outside the existing lines makes the function a no-op, like a
                    // rejected `ListOperation` in vanilla.
                    if let Some(next) = op.apply(current, lines.clone()) {
                        stack
                            .components
                            .insert(ids::LORE, Component::from_nbt(ids::LORE, &lore_list(next)));
                    }
                }
                Step::SetItem(item) => stack.item = *item,
                Step::SetPotion(potion) => {
                    let mut contents = match stack
                        .effective_component(ids::POTION_CONTENTS)
                        .and_then(|c| c.to_nbt().ok())
                    {
                        Some(Tag::Compound(existing)) => existing,
                        _ => Compound::new(),
                    };
                    contents.insert("potion".to_owned(), Tag::String(potion.clone()));
                    stack.components.insert(
                        ids::POTION_CONTENTS,
                        Component::from_nbt(ids::POTION_CONTENTS, &Tag::Compound(contents)),
                    );
                }
                Step::SetCustomData(tag) => {
                    let mut data = match stack
                        .effective_component(ids::CUSTOM_DATA)
                        .and_then(|c| c.to_nbt().ok())
                    {
                        Some(Tag::Compound(existing)) => existing,
                        _ => Compound::new(),
                    };
                    for (key, value) in tag.iter() {
                        data.insert(key.clone(), value.clone());
                    }
                    stack.components.insert(
                        ids::CUSTOM_DATA,
                        Component::from_nbt(ids::CUSTOM_DATA, &Tag::Compound(data)),
                    );
                }
                Step::SetOminousBottleAmplifier(amplifier) => {
                    let value = amplifier.eval(rng).floor() as i32;
                    if let Some(id) =
                        ferrite_item::component_id("minecraft:ominous_bottle_amplifier")
                    {
                        stack
                            .components
                            .insert(id, Component::from_nbt(id, &Tag::Int(value.clamp(0, 4))));
                    }
                }
                Step::Discard => stack = ItemStack::empty(),
                Step::ExplosionDecay => {}
                Step::Loot(step) => {
                    if let Some(mut rolled) = to_loot(&stack) {
                        step.0.apply_to(
                            &mut rolled,
                            &loot_context(),
                            &mut super::drops::LootRng(rng),
                        );
                        stack = if rolled.count > 0 {
                            from_loot(rolled, &stack)
                        } else {
                            ItemStack::empty()
                        };
                    }
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
    fn uniform_modifier_rejects_ranges_that_overflow_the_sampler() {
        for field in ["count", "damage"] {
            let function = if field == "count" {
                "set_count"
            } else {
                "set_damage"
            };
            for (min, max) in [(0.0, 2147483647.0), (-1e100, 1e100)] {
                let definition = serde_json::json!({
                    "function": function,
                    (field): {"type": "uniform", "min": min, "max": max}
                });
                assert!(ItemModifier::parse(&definition.to_string()).is_err());
            }
        }
        let widest = serde_json::json!({"type": "uniform", "min": 0, "max": 2147483646});
        let provider = Number::parse(Some(&widest), "count").unwrap();
        let sample = provider.eval(&mut ferrite_entity::Rng::new(7));
        assert!((0.0..=2147483646.0).contains(&sample));
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
    fn the_26_3_type_key_is_what_data_packs_use() {
        // 26.3 loot functions and conditions are keyed by "type" (see the blaze loot table).
        let modifier = r#"[
            {"type":"minecraft:set_count","count":5},
            {"type":"minecraft:set_count","count":2,"add":true,
             "conditions":[{"type":"minecraft:random_chance","chance":1}]}
        ]"#;
        assert_eq!(run(modifier, stone(1)).count, 7);
        // The legacy keys keep working.
        assert_eq!(
            run(r#"{"function":"set_count","count":4}"#, stone(1)).count,
            4
        );
    }

    #[test]
    fn inverted_all_of_and_any_of_combine_conditions() {
        let gated = |condition: &str| {
            let json = format!(r#"{{"type":"set_count","count":9,"conditions":[{condition}]}}"#);
            run(&json, stone(1)).count
        };
        let always = r#"{"type":"random_chance","chance":1}"#;
        let never = r#"{"type":"random_chance","chance":0}"#;
        assert_eq!(
            gated(&format!(r#"{{"type":"inverted","term":{never}}}"#)),
            9
        );
        assert_eq!(
            gated(&format!(r#"{{"type":"inverted","term":{always}}}"#)),
            1
        );
        assert_eq!(
            gated(&format!(
                r#"{{"type":"all_of","terms":[{always},{never}]}}"#
            )),
            1
        );
        assert_eq!(
            gated(&format!(
                r#"{{"type":"any_of","terms":[{never},{always}]}}"#
            )),
            9
        );
    }

    #[test]
    fn set_item_potion_custom_data_and_discard() {
        let out = run(
            r#"{"type":"set_item","item":"minecraft:diamond"}"#,
            stone(3),
        );
        assert_eq!(ferrite_item::item_name(out.item), Some("minecraft:diamond"));
        assert_eq!(out.count, 3, "set_item keeps the count");
        assert!(ItemModifier::parse(r#"{"type":"set_item","item":"minecraft:nope"}"#).is_err());

        let potion = ferrite_item::ItemStack::with_count(
            ferrite_item::item_id("minecraft:potion").unwrap(),
            1,
        );
        let out = run(r#"{"type":"set_potion","id":"minecraft:slowness"}"#, potion);
        let contents = out
            .components
            .get(ids::POTION_CONTENTS)
            .and_then(|c| c.to_nbt().ok())
            .expect("potion contents");
        let Tag::Compound(contents) = contents else {
            panic!("compound");
        };
        assert_eq!(
            contents.get("potion"),
            Some(&Tag::String("minecraft:slowness".into()))
        );

        let out = run(
            r#"[{"type":"set_custom_data","tag":{"a":1}},{"type":"set_custom_data","tag":{"b":"x"}}]"#,
            stone(1),
        );
        let Some(Tag::Compound(data)) = out
            .components
            .get(ids::CUSTOM_DATA)
            .and_then(|c| c.to_nbt().ok())
        else {
            panic!("custom data");
        };
        assert!(
            data.get("a").is_some() && data.get("b").is_some(),
            "merged: {data:?}"
        );

        assert!(run(r#"{"type":"discard"}"#, stone(5)).is_empty());
        // `explosion_decay` has no explosion to decay in the command context.
        assert_eq!(run(r#"{"type":"explosion_decay"}"#, stone(5)).count, 5);
    }

    #[test]
    fn set_name_targets_and_lore_list_operations() {
        let out = run(
            r#"{"type":"set_name","name":"Plain","target":"item_name"}"#,
            stone(1),
        );
        assert!(out.components.get(ids::ITEM_NAME).is_some());
        assert!(out.components.get(ids::CUSTOM_NAME).is_none());
        let lore = |json: &str, stack: ItemStack| -> Vec<String> {
            let out = run(json, stack);
            match out.components.get(ids::LORE).and_then(|c| c.to_nbt().ok()) {
                Some(Tag::List(ferrite_nbt::List::String(lines))) => lines,
                other => panic!("lore list expected, got {other:?}"),
            }
        };
        let base = run(r#"{"type":"set_lore","lore":["a","b","c"]}"#, stone(1));
        let with = |json: &str| lore(json, base.clone());
        assert_eq!(
            with(r#"{"type":"set_lore","lore":["x"],"mode":"append"}"#),
            ["a", "b", "c", "x"]
        );
        assert_eq!(
            with(r#"{"type":"set_lore","lore":["x"],"mode":"insert","offset":1}"#),
            ["a", "x", "b", "c"]
        );
        assert_eq!(
            with(
                r#"{"type":"set_lore","lore":["x"],"mode":"replace_section","offset":1,"size":2}"#
            ),
            ["a", "x"]
        );
        assert_eq!(
            with(r#"{"type":"set_lore","lore":["x","y"],"mode":"replace_all"}"#),
            ["x", "y"]
        );
        // An insert past the end is rejected by vanilla's ListOperation: the lore stays.
        assert_eq!(
            with(r#"{"type":"set_lore","lore":["x"],"mode":"insert","offset":9}"#),
            ["a", "b", "c"]
        );
        assert!(
            ItemModifier::parse(r#"{"type":"set_lore","lore":["x"],"mode":"sideways"}"#).is_err()
        );
    }

    #[test]
    fn unsupported_or_invalid_parts_fail_loading_with_their_name() {
        for (json, needle) in [
            (
                r#"{"function":"minecraft:set_contents","component":"minecraft:container","entries":[]}"#,
                "set_contents",
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
            (r#"{"count":1}"#, "type"),
            (r#"[1]"#, "function"),
            ("not json", "expected"),
        ] {
            let error = ItemModifier::parse(json).unwrap_err();
            assert!(error.contains(needle), "{json}: {error}");
        }
    }

    fn item(name: &str, count: i32) -> ItemStack {
        ItemStack::with_count(ferrite_item::item_id(name).unwrap(), count)
    }

    fn component(stack: &ItemStack, name: &str) -> Option<Tag> {
        let id = ferrite_item::component_id(name)?;
        stack.effective_component(id).and_then(|c| c.to_nbt().ok())
    }

    /// The rest of the loot functions run through the loot crate (needs the vanilla data).
    #[test]
    fn loot_functions_run_in_item_modifiers() {
        if super::super::drops::tables().is_none() {
            return;
        }
        // enchant_with_levels: an iron pickaxe (enchantability 14) gets real enchantments.
        let pick = run(
            r##"{"type":"minecraft:enchant_with_levels","levels":30,"options":"#minecraft:on_random_loot"}"##,
            item("minecraft:iron_pickaxe", 1),
        );
        assert!(matches!(
            component(&pick, "minecraft:enchantments"),
            Some(Tag::Compound(levels)) if !levels.is_empty()
        ));
        // ... and a book becomes an enchanted book with stored enchantments.
        let book = run(
            r##"{"type":"minecraft:enchant_with_levels","levels":30,"options":"#minecraft:on_random_loot"}"##,
            item("minecraft:book", 1),
        );
        assert_eq!(
            book.item,
            ferrite_item::item_id("minecraft:enchanted_book").unwrap()
        );
        assert!(component(&book, "minecraft:stored_enchantments").is_some());
        // enchant_randomly respects what the item supports.
        let rod = run(
            r#"{"type":"minecraft:enchant_randomly","options":["minecraft:sharpness","minecraft:unbreaking"]}"#,
            item("minecraft:fishing_rod", 1),
        );
        let Some(Tag::Compound(levels)) = component(&rod, "minecraft:enchantments") else {
            panic!("no enchantments on the rod");
        };
        assert_eq!(levels.keys().collect::<Vec<_>>(), ["minecraft:unbreaking"]);
        // furnace_smelt keeps the count of the input stack.
        let cooked = run(
            r#"{"type":"minecraft:furnace_smelt"}"#,
            item("minecraft:beef", 3),
        );
        assert_eq!(
            (cooked.item, cooked.count),
            (ferrite_item::item_id("minecraft:cooked_beef").unwrap(), 3)
        );
        // set_stew_effect only touches suspicious stew.
        let stew = run(
            r#"{"type":"minecraft:set_stew_effect","effects":[{"type":"minecraft:blindness","duration":5}]}"#,
            item("minecraft:suspicious_stew", 1),
        );
        assert!(component(&stew, "minecraft:suspicious_stew_effects").is_some());
        let bowl = run(
            r#"{"type":"minecraft:set_stew_effect","effects":[{"type":"minecraft:blindness","duration":5}]}"#,
            item("minecraft:stone", 1),
        );
        assert!(component(&bowl, "minecraft:suspicious_stew_effects").is_none());
        // set_instrument picks one of the tag's horns.
        let horn = run(
            r##"{"type":"minecraft:set_instrument","options":"#minecraft:regular_goat_horns"}"##,
            item("minecraft:goat_horn", 1),
        );
        assert!(matches!(
            component(&horn, "minecraft:instrument"),
            Some(Tag::String(_))
        ));
        // A condition of a delegated function is evaluated by the loot crate.
        let never = run(
            r#"{"type":"minecraft:furnace_smelt","conditions":[{"condition":"minecraft:random_chance","chance":0.0}]}"#,
            item("minecraft:beef", 1),
        );
        assert_eq!(never.item, ferrite_item::item_id("minecraft:beef").unwrap());
        // enchanted_count_increase needs an attacker, which item modifiers do not have.
        let same = run(
            r#"{"type":"minecraft:enchanted_count_increase","count":2,"enchantment":"minecraft:looting"}"#,
            item("minecraft:stone", 2),
        );
        assert_eq!(same.count, 2);
    }
}
