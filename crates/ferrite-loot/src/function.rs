//! Loot functions attached to pools, entries and whole tables.
//!
//! Every function the shipped 26.3 loot tables use is evaluated. A function type outside that
//! set stays [`FunctionKind::Opaque`] and passes the stack through; [`crate::LootTables::unsupported`]
//! lists them.

use crate::condition::{self, Condition};
use crate::data::GameData;
use crate::number::{IntRange, NumberProvider};
use crate::{Definitions, LootContext, LootStack, Rng, normalize_id};
use ferrite_nbt::Tag;
use serde_json::Value;
use std::sync::Arc;

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
    /// Adds `round(level * count)` items for the attacker's enchantment level (looting).
    EnchantedCountIncrease {
        /// Enchantment whose level scales the bonus.
        enchantment: String,
        /// Float provider multiplied by the level.
        count: NumberProvider,
        /// Stack size cap applied after the bonus; `0` means none.
        limit: i32,
    },
    /// Replaces the stack with its smelting result.
    FurnaceSmelt {
        /// Multiply the result count by the input count.
        use_input_count: bool,
        /// Smelting recipes.
        data: Arc<GameData>,
    },
    /// Sets `minecraft:potion_contents.potion`.
    SetPotion {
        /// Potion id.
        id: String,
    },
    /// One random enchantment.
    EnchantRandomly {
        /// Candidate ids; `None` means every enchantment.
        options: Option<Vec<String>>,
        /// Only enchantments the item supports (books take any).
        only_compatible: bool,
        /// Enchantment registry.
        data: Arc<GameData>,
    },
    /// Enchants as an enchanting table would at some levels.
    EnchantWithLevels {
        /// Level provider.
        levels: NumberProvider,
        /// Candidate ids; `None` means every enchantment.
        options: Option<Vec<String>>,
        /// Enchantment registry.
        data: Arc<GameData>,
    },
    /// Sets the durability as a fraction of the maximum.
    SetDamage {
        /// Remaining-durability fraction removed, as a float provider.
        damage: NumberProvider,
        /// Adds to the current damage fraction instead of replacing it.
        add: bool,
    },
    /// Points a map at the closest structure when the host can find one.
    ExplorationMap {
        /// Structure ids of the destination tag.
        destinations: Vec<String>,
        /// Marker shown on the map (`minecraft:red_x`, `minecraft:mansion`, ...).
        decoration: String,
        /// Zoom level of the map.
        zoom: i32,
        /// Skip chunks that were already generated.
        skip_existing_chunks: bool,
    },
    /// Runs `on_fail` for stacks that do not match the item filter.
    Filtered {
        /// The raw `item_filter`.
        filter: Value,
        /// Functions run on matching stacks.
        on_pass: Vec<Function>,
        /// Functions run on failing stacks.
        on_fail: Vec<Function>,
        /// Registry data for item tags in the filter.
        data: Arc<GameData>,
    },
    /// Copies block state properties of the broken block into `minecraft:block_state`.
    CopyState {
        /// Block the properties belong to.
        block: String,
        /// Property names to copy.
        properties: Vec<String>,
    },
    /// Sets the custom or item name.
    SetName {
        /// The text component as NBT.
        name: Tag,
        /// Component written (`minecraft:custom_name` or `minecraft:item_name`).
        component: String,
    },
    /// Sets enchantment levels, optionally adding to existing ones.
    SetEnchantments {
        /// Level provider per enchantment id.
        enchantments: Vec<(String, NumberProvider)>,
        /// Adds to existing levels.
        add: bool,
    },
    /// Sets raw components.
    SetComponents {
        /// Component name and value.
        components: Vec<(String, Tag)>,
    },
    /// Adds one random effect to a suspicious stew.
    SetStewEffect {
        /// Effect id and duration (seconds) provider.
        effects: Vec<(String, NumberProvider)>,
    },
    /// A random instrument for goat horns.
    SetInstrument {
        /// Instrument ids.
        options: Vec<String>,
    },
    /// Sets the ominous bottle amplifier.
    SetOminousBottleAmplifier {
        /// Amplifier provider.
        amplifier: NumberProvider,
    },
    /// Copies an entity's custom name.
    CopyName {
        /// Entity role (`this`, `attacker`, `direct_attacker`).
        source: String,
    },
    /// Adds (or sets) attribute modifiers.
    SetAttributes {
        /// The modifiers to add.
        modifiers: Vec<AttributeModifier>,
        /// Replaces the item's existing modifiers instead of extending them.
        replace: bool,
    },
    /// Sets banner patterns.
    SetBannerPattern {
        /// `(pattern id, dye color)` layers.
        patterns: Vec<(String, String)>,
        /// Appends to the existing layers instead of replacing them.
        append: bool,
    },
    /// Sets the title, author and generation of a written book.
    SetBookCover {
        /// New title, if any.
        title: Option<String>,
        /// New author, if any.
        author: Option<String>,
        /// New generation, if any.
        generation: Option<i32>,
    },
    /// Sets the loot table of a container item.
    SetLootTable {
        /// Loot table id.
        table: String,
        /// Seed; `0` means random at open time.
        seed: i64,
    },
    /// Shows or hides parts of the tooltip.
    ToggleTooltips {
        /// Component id and whether it is shown.
        toggles: Vec<(String, bool)>,
    },
    /// A random potion.
    SetRandomPotion {
        /// Candidate ids; `None` means every potion.
        options: Option<Vec<String>>,
    },
    /// Updates fields of a firework star.
    SetFireworkExplosion {
        /// Shape (`small_ball`, `star`, ...).
        shape: Option<String>,
        /// Colors as RGB ints.
        colors: Option<Vec<i32>>,
        /// Fade colors as RGB ints.
        fade_colors: Option<Vec<i32>>,
        /// Trail flag.
        trail: Option<bool>,
        /// Twinkle flag.
        twinkle: Option<bool>,
    },
    /// Removes the stack.
    Discard,
    /// A function this crate passes through untouched, retaining its JSON.
    Opaque {
        /// Vanilla function type.
        kind: String,
        /// Raw function JSON.
        value: Value,
    },
}

/// One `set_attributes` modifier.
#[derive(Debug, Clone)]
pub struct AttributeModifier {
    /// Attribute id (`minecraft:attack_damage`).
    pub attribute: String,
    /// Modifier id.
    pub id: String,
    /// Amount provider.
    pub amount: NumberProvider,
    /// `add_value`, `add_multiplied_base` or `add_multiplied_total`.
    pub operation: String,
    /// Slots to choose from at random (`mainhand`, `any`, ...).
    pub slots: Vec<String>,
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

pub(crate) fn parse(value: &Value, definitions: &Definitions) -> Option<Function> {
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
        "minecraft:enchanted_count_increase" => FunctionKind::EnchantedCountIncrease {
            enchantment: object
                .get("enchantment")
                .and_then(Value::as_str)
                .unwrap_or("minecraft:looting")
                .to_owned(),
            count: NumberProvider::parse(object.get("count")),
            limit: object.get("limit").and_then(Value::as_i64).unwrap_or(0) as i32,
        },
        "minecraft:furnace_smelt" => FunctionKind::FurnaceSmelt {
            use_input_count: object
                .get("use_input_count")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            data: definitions.data.clone(),
        },
        "minecraft:set_potion" => FunctionKind::SetPotion {
            id: normalize_id(object.get("id").and_then(Value::as_str).unwrap_or("")),
        },
        "minecraft:enchant_randomly" => FunctionKind::EnchantRandomly {
            options: object
                .get("options")
                .map(|options| definitions.data.resolve_ids("enchantment", options)),
            only_compatible: object
                .get("only_compatible")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            data: definitions.data.clone(),
        },
        "minecraft:enchant_with_levels" => FunctionKind::EnchantWithLevels {
            levels: NumberProvider::parse(object.get("levels")),
            options: object
                .get("options")
                .map(|options| definitions.data.resolve_ids("enchantment", options)),
            data: definitions.data.clone(),
        },
        "minecraft:set_damage" => FunctionKind::SetDamage {
            damage: NumberProvider::parse(object.get("damage")),
            add: object.get("add").and_then(Value::as_bool).unwrap_or(false),
        },
        "minecraft:exploration_map" => FunctionKind::ExplorationMap {
            destinations: definitions.data.resolve_ids(
                "worldgen/structure",
                object
                    .get("destination")
                    .unwrap_or(&Value::String("#minecraft:on_treasure_maps".to_owned())),
            ),
            decoration: object
                .get("decoration")
                .and_then(Value::as_str)
                .unwrap_or("minecraft:mansion")
                .to_owned(),
            zoom: object.get("zoom").and_then(Value::as_i64).unwrap_or(2) as i32,
            skip_existing_chunks: object
                .get("skip_existing_chunks")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        },
        "minecraft:filtered" if filter_supported(object.get("item_filter")) => {
            FunctionKind::Filtered {
                filter: object.get("item_filter").cloned().unwrap_or(Value::Null),
                on_pass: object
                    .get("on_pass")
                    .map(|value| parse_functions(value, definitions))
                    .unwrap_or_default(),
                on_fail: object
                    .get("on_fail")
                    .map(|value| parse_functions(value, definitions))
                    .unwrap_or_default(),
                data: definitions.data.clone(),
            }
        }
        "minecraft:copy_state" => FunctionKind::CopyState {
            block: normalize_id(object.get("block").and_then(Value::as_str).unwrap_or("")),
            properties: object
                .get("properties")
                .and_then(Value::as_array)
                .map(|names| {
                    names
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
        },
        "minecraft:set_name" if object.get("name").and_then(json_to_tag).is_some() => {
            FunctionKind::SetName {
                name: object
                    .get("name")
                    .and_then(json_to_tag)
                    .unwrap_or(Tag::Byte(0)),
                component: match object.get("target").and_then(Value::as_str) {
                    Some("item_name") => "minecraft:item_name".to_owned(),
                    _ => "minecraft:custom_name".to_owned(),
                },
            }
        }
        "minecraft:set_enchantments" => FunctionKind::SetEnchantments {
            enchantments: object
                .get("enchantments")
                .and_then(Value::as_object)
                .map(|levels| {
                    levels
                        .iter()
                        .map(|(id, level)| (normalize_id(id), NumberProvider::parse(Some(level))))
                        .collect()
                })
                .unwrap_or_default(),
            add: object.get("add").and_then(Value::as_bool).unwrap_or(false),
        },
        "minecraft:set_components"
            if object
                .get("components")
                .and_then(Value::as_object)
                .is_some_and(|map| map.values().all(|value| json_to_tag(value).is_some())) =>
        {
            FunctionKind::SetComponents {
                components: object
                    .get("components")
                    .and_then(Value::as_object)
                    .map(|map| {
                        map.iter()
                            .filter_map(|(name, value)| {
                                json_to_tag(value).map(|tag| (normalize_id(name), tag))
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            }
        }
        "minecraft:set_stew_effect" => FunctionKind::SetStewEffect {
            effects: object
                .get("effects")
                .and_then(Value::as_array)
                .map(|effects| {
                    effects
                        .iter()
                        .map(|effect| {
                            (
                                normalize_id(
                                    effect.get("type").and_then(Value::as_str).unwrap_or(""),
                                ),
                                NumberProvider::parse(effect.get("duration")),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default(),
        },
        "minecraft:set_instrument" => FunctionKind::SetInstrument {
            options: object
                .get("options")
                .map(|options| definitions.data.resolve_ids("instrument", options))
                .unwrap_or_default(),
        },
        "minecraft:set_ominous_bottle_amplifier" => FunctionKind::SetOminousBottleAmplifier {
            amplifier: NumberProvider::parse(object.get("amplifier")),
        },
        "minecraft:copy_name" => FunctionKind::CopyName {
            source: object
                .get("source")
                .and_then(Value::as_str)
                .unwrap_or("this")
                .to_owned(),
        },
        "minecraft:set_attributes" => FunctionKind::SetAttributes {
            modifiers: object
                .get("modifiers")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .map(|modifier| AttributeModifier {
                            attribute: normalize_id(
                                modifier
                                    .get("attribute")
                                    .and_then(Value::as_str)
                                    .unwrap_or(""),
                            ),
                            id: normalize_id(
                                modifier.get("id").and_then(Value::as_str).unwrap_or(""),
                            ),
                            amount: NumberProvider::parse(modifier.get("amount")),
                            operation: modifier
                                .get("operation")
                                .and_then(Value::as_str)
                                .unwrap_or("add_value")
                                .to_owned(),
                            slots: match modifier.get("slot") {
                                Some(Value::String(slot)) => vec![slot.clone()],
                                Some(Value::Array(slots)) => slots
                                    .iter()
                                    .filter_map(Value::as_str)
                                    .map(str::to_owned)
                                    .collect(),
                                _ => Vec::new(),
                            },
                        })
                        .collect()
                })
                .unwrap_or_default(),
            replace: object
                .get("replace")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        },
        "minecraft:set_banner_pattern" => FunctionKind::SetBannerPattern {
            patterns: object
                .get("patterns")
                .and_then(Value::as_array)
                .map(|layers| {
                    layers
                        .iter()
                        .map(|layer| {
                            (
                                normalize_id(
                                    layer.get("pattern").and_then(Value::as_str).unwrap_or(""),
                                ),
                                layer
                                    .get("color")
                                    .and_then(Value::as_str)
                                    .unwrap_or("white")
                                    .to_owned(),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default(),
            append: object
                .get("append")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        },
        "minecraft:set_book_cover" => FunctionKind::SetBookCover {
            // A filterable string is `"text"` or `{"raw": "text", "filtered": "..."}`.
            title: object.get("title").and_then(|title| {
                title
                    .as_str()
                    .or_else(|| title.get("raw").and_then(Value::as_str))
                    .map(str::to_owned)
            }),
            author: object
                .get("author")
                .and_then(Value::as_str)
                .map(str::to_owned),
            generation: object
                .get("generation")
                .and_then(Value::as_i64)
                .map(|generation| generation as i32),
        },
        "minecraft:set_loot_table" => FunctionKind::SetLootTable {
            table: normalize_id(
                object
                    .get("loot_table_id")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            ),
            seed: object.get("seed").and_then(Value::as_i64).unwrap_or(0),
        },
        "minecraft:toggle_tooltips" => FunctionKind::ToggleTooltips {
            toggles: object
                .get("toggles")
                .and_then(Value::as_object)
                .map(|map| {
                    map.iter()
                        .filter_map(|(id, shown)| Some((normalize_id(id), shown.as_bool()?)))
                        .collect()
                })
                .unwrap_or_default(),
        },
        "minecraft:set_random_potion" => FunctionKind::SetRandomPotion {
            options: object
                .get("options")
                .map(|options| definitions.data.resolve_ids("potion", options)),
        },
        "minecraft:set_firework_explosion" => {
            let colors = |key: &str| {
                object.get(key).and_then(Value::as_array).map(|list| {
                    list.iter()
                        .filter_map(Value::as_i64)
                        .map(|color| color as i32)
                        .collect()
                })
            };
            FunctionKind::SetFireworkExplosion {
                shape: object
                    .get("shape")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                colors: colors("colors"),
                fade_colors: colors("fade_colors"),
                trail: object.get("trail").and_then(Value::as_bool),
                twinkle: object.get("twinkle").and_then(Value::as_bool),
            }
        }
        "minecraft:discard" => FunctionKind::Discard,
        other => FunctionKind::Opaque {
            kind: other.to_owned(),
            value: value.clone(),
        },
    };
    Some(Function { conditions, kind })
}

/// Whether a stew effect is instantaneous (its duration is not scaled to ticks).
fn instantaneous(effect: &str) -> bool {
    matches!(
        effect,
        "minecraft:instant_health" | "minecraft:instant_damage" | "minecraft:saturation"
    )
}

/// Whether `filtered` can evaluate this `item_filter`: `items`, `count` and
/// `predicates` that only test for the presence of a component.
fn filter_supported(filter: Option<&Value>) -> bool {
    let Some(object) = filter.and_then(Value::as_object) else {
        return false;
    };
    object.iter().all(|(key, value)| match key.as_str() {
        "items" | "count" => true,
        "predicates" => value.as_object().is_some_and(|map| {
            map.values()
                .all(|v| v.as_object().is_some_and(|o| o.is_empty()))
        }),
        _ => false,
    })
}

fn filter_matches(filter: &Value, stack: &LootStack, data: &GameData) -> bool {
    let Some(object) = filter.as_object() else {
        return false;
    };
    object.iter().all(|(key, value)| match key.as_str() {
        "items" => data
            .resolve_ids("item", value)
            .contains(&normalize_id(&stack.item)),
        "count" => IntRange::parse(Some(value)).clamp(stack.count) == stack.count,
        "predicates" => value.as_object().is_some_and(|map| {
            map.keys()
                .all(|component| stack.components.contains_key(&normalize_id(component)))
        }),
        _ => false,
    })
}

/// JSON to NBT, with integers as `Int` (or `Long` when large), fractions as `Double`, booleans
/// as `Byte` and arrays of one element kind as typed lists. `None` for shapes NBT cannot hold
/// (null, mixed arrays).
pub(crate) fn json_to_tag(value: &Value) -> Option<Tag> {
    Some(match value {
        Value::String(text) => Tag::String(text.clone()),
        Value::Bool(flag) => Tag::Byte(i8::from(*flag)),
        Value::Number(number) => match number.as_i64() {
            Some(int) => i32::try_from(int).map_or(Tag::Long(int), Tag::Int),
            None => Tag::Double(number.as_f64()?),
        },
        Value::Object(map) => {
            let mut compound = ferrite_nbt::Compound::new();
            for (key, value) in map {
                compound.insert(key.clone(), json_to_tag(value)?);
            }
            Tag::Compound(compound)
        }
        Value::Array(values) => {
            let tags: Vec<Tag> = values.iter().map(json_to_tag).collect::<Option<_>>()?;
            Tag::List(match tags.first() {
                None => ferrite_nbt::List::End,
                Some(Tag::String(_)) => ferrite_nbt::List::String(
                    tags.into_iter()
                        .map(|tag| match tag {
                            Tag::String(text) => Some(text),
                            _ => None,
                        })
                        .collect::<Option<_>>()?,
                ),
                Some(Tag::Int(_)) => ferrite_nbt::List::Int(
                    tags.into_iter()
                        .map(|tag| match tag {
                            Tag::Int(int) => Some(int),
                            _ => None,
                        })
                        .collect::<Option<_>>()?,
                ),
                Some(Tag::Compound(_)) => ferrite_nbt::List::Compound(
                    tags.into_iter()
                        .map(|tag| match tag {
                            Tag::Compound(compound) => Some(compound),
                            _ => None,
                        })
                        .collect::<Option<_>>()?,
                ),
                Some(_) => return None,
            })
        }
        Value::Null => return None,
    })
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

impl Function {
    /// Applies the function to `stack` if its conditions hold. A count driven to zero or below
    /// means the stack is gone.
    pub fn apply_to(&self, stack: &mut LootStack, context: &LootContext, rng: &mut impl Rng) {
        apply(std::slice::from_ref(self), stack, context, rng);
    }
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
            Self::EnchantedCountIncrease {
                enchantment,
                count,
                limit,
            } => {
                let level = context.enchantment_level(enchantment);
                let attacker_level = context
                    .damage_source
                    .as_ref()
                    .and_then(|damage| damage.source_entity.as_ref())
                    .and_then(|entity| entity.mainhand.as_ref())
                    .map_or(0, |(_, enchants)| {
                        enchants
                            .iter()
                            .find(|(id, _)| normalize_id(id) == normalize_id(enchantment))
                            .map_or(0, |(_, level)| *level)
                    });
                let level = level.max(attacker_level);
                if level <= 0 {
                    return;
                }
                let bonus = count.get_float(rng) * level as f32;
                // Math.round: floor(x + 0.5)
                stack.count = stack.count.saturating_add((bonus + 0.5).floor() as i32);
                if *limit > 0 {
                    stack.count = stack.count.min(*limit);
                }
            }
            Self::FurnaceSmelt {
                use_input_count,
                data,
            } => {
                let Some((result, result_count)) = data.smelt(&stack.item) else {
                    return;
                };
                let input = if *use_input_count { stack.count } else { 1 };
                let max = context
                    .host
                    .as_ref()
                    .map_or(64, |host| host.max_stack_size(result));
                stack.count = input.saturating_mul(result_count).min(max);
                stack.item = result.to_owned();
                stack.components.clear();
            }
            Self::SetPotion { id } => {
                let mut contents = match stack.components.remove("minecraft:potion_contents") {
                    Some(Tag::Compound(contents)) => contents,
                    _ => ferrite_nbt::Compound::new(),
                };
                contents.insert("potion".to_owned(), Tag::String(id.clone()));
                stack.components.insert(
                    "minecraft:potion_contents".to_owned(),
                    Tag::Compound(contents),
                );
            }
            Self::EnchantRandomly {
                options,
                only_compatible,
                data,
            } => crate::enchant::randomly(stack, options, *only_compatible, data, rng),
            Self::EnchantWithLevels {
                levels,
                options,
                data,
            } => {
                let levels = levels.get_int(rng);
                crate::enchant::with_levels(
                    stack,
                    levels,
                    options,
                    data,
                    context.host.as_deref(),
                    rng,
                );
            }
            Self::SetDamage { damage, add } => {
                let Some(max) = context
                    .host
                    .as_ref()
                    .and_then(|host| host.max_damage(&stack.item))
                    .filter(|max| *max > 0)
                else {
                    return;
                };
                let current = match stack.components.get("minecraft:damage") {
                    Some(Tag::Int(damage)) => *damage,
                    _ => 0,
                };
                let base = if *add {
                    1.0 - current as f32 / max as f32
                } else {
                    0.0
                };
                let remaining = (base + damage.get_float(rng)).clamp(0.0, 1.0);
                let value = ((1.0 - remaining) * max as f32).floor() as i32;
                stack
                    .components
                    .insert("minecraft:damage".to_owned(), Tag::Int(value));
            }
            Self::ExplorationMap {
                destinations,
                decoration,
                zoom,
                skip_existing_chunks,
            } => {
                // 26.3 checks only for an empty stack and the origin, not for the item.
                let Some(origin) = context.origin else {
                    return;
                };
                let Some(components) = context.host.as_ref().and_then(|host| {
                    host.exploration_map(
                        origin,
                        destinations,
                        decoration,
                        *zoom,
                        *skip_existing_chunks,
                    )
                }) else {
                    return;
                };
                for (name, value) in components {
                    stack.components.insert(name, value);
                }
            }
            Self::Filtered {
                filter,
                on_pass,
                on_fail,
                data,
            } => {
                let functions = if filter_matches(filter, stack, data) {
                    on_pass
                } else {
                    on_fail
                };
                apply(functions, stack, context, rng);
            }
            Self::CopyState { block, properties } => {
                if context.block.as_deref().map(normalize_id).as_deref() != Some(block.as_str()) {
                    return;
                }
                let mut state = match stack.components.remove("minecraft:block_state") {
                    Some(Tag::Compound(state)) => state,
                    _ => ferrite_nbt::Compound::new(),
                };
                for property in properties {
                    if let Some(value) = context.block_state.get(property) {
                        state.insert(property.clone(), Tag::String(value.clone()));
                    }
                }
                stack
                    .components
                    .insert("minecraft:block_state".to_owned(), Tag::Compound(state));
            }
            Self::SetName { name, component } => {
                stack.components.insert(component.clone(), name.clone());
            }
            Self::SetEnchantments { enchantments, add } => {
                let name = if stack.item == "minecraft:enchanted_book" {
                    "minecraft:stored_enchantments"
                } else {
                    "minecraft:enchantments"
                };
                let mut levels = match stack.components.remove(name) {
                    Some(Tag::Compound(levels)) => levels,
                    _ => ferrite_nbt::Compound::new(),
                };
                for (id, provider) in enchantments {
                    let level = provider.get_int(rng);
                    let existing = match levels.get(id) {
                        Some(Tag::Int(existing)) if *add => *existing,
                        _ => 0,
                    };
                    let level = existing.saturating_add(level).clamp(0, 255);
                    if level == 0 {
                        levels.shift_remove(id);
                    } else {
                        levels.insert(id.clone(), Tag::Int(level));
                    }
                }
                stack
                    .components
                    .insert(name.to_owned(), Tag::Compound(levels));
            }
            Self::SetComponents { components } => {
                for (name, value) in components {
                    stack.components.insert(name.clone(), value.clone());
                }
            }
            Self::SetStewEffect { effects } => {
                if stack.item != "minecraft:suspicious_stew" || effects.is_empty() {
                    return;
                }
                let index = rng.next_int(i32::try_from(effects.len()).unwrap_or(i32::MAX));
                let (id, duration) = &effects[index as usize];
                let seconds = duration.get_int(rng);
                // Instant effects keep the plain number; others are written in ticks.
                let ticks = if instantaneous(id) {
                    seconds
                } else {
                    seconds.saturating_mul(20)
                };
                let mut entry = ferrite_nbt::Compound::new();
                entry.insert("id".to_owned(), Tag::String(id.clone()));
                entry.insert("duration".to_owned(), Tag::Int(ticks));
                let mut list = match stack.components.remove("minecraft:suspicious_stew_effects") {
                    Some(Tag::List(ferrite_nbt::List::Compound(list))) => list,
                    _ => Vec::new(),
                };
                list.push(entry);
                stack.components.insert(
                    "minecraft:suspicious_stew_effects".to_owned(),
                    Tag::List(ferrite_nbt::List::Compound(list)),
                );
            }
            Self::SetInstrument { options } => {
                if options.is_empty() {
                    return;
                }
                let index = rng.next_int(i32::try_from(options.len()).unwrap_or(i32::MAX));
                stack.components.insert(
                    "minecraft:instrument".to_owned(),
                    Tag::String(options[index as usize].clone()),
                );
            }
            Self::SetOminousBottleAmplifier { amplifier } => {
                let level = amplifier.get_int(rng);
                stack.components.insert(
                    "minecraft:ominous_bottle_amplifier".to_owned(),
                    Tag::Int(level),
                );
            }
            Self::CopyName { source } => {
                let entity = match source.as_str() {
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
                if let Some(name) = entity.and_then(|entity| entity.custom_name.clone()) {
                    stack
                        .components
                        .insert("minecraft:custom_name".to_owned(), Tag::String(name));
                }
            }
            Self::SetAttributes { modifiers, replace } => {
                let mut list = if *replace {
                    Vec::new()
                } else {
                    let existing = stack
                        .components
                        .get("minecraft:attribute_modifiers")
                        .cloned()
                        .or_else(|| {
                            context.host.as_ref().and_then(|host| {
                                host.default_component(&stack.item, "minecraft:attribute_modifiers")
                            })
                        });
                    match existing {
                        Some(Tag::List(ferrite_nbt::List::Compound(list))) => list,
                        _ => Vec::new(),
                    }
                };
                for modifier in modifiers {
                    if modifier.slots.is_empty() {
                        continue;
                    }
                    let slot = &modifier.slots[rng
                        .next_int(i32::try_from(modifier.slots.len()).unwrap_or(i32::MAX))
                        as usize];
                    let mut entry = ferrite_nbt::Compound::new();
                    entry.insert("type".to_owned(), Tag::String(modifier.attribute.clone()));
                    entry.insert("id".to_owned(), Tag::String(modifier.id.clone()));
                    entry.insert(
                        "amount".to_owned(),
                        Tag::Double(f64::from(modifier.amount.get_float(rng))),
                    );
                    entry.insert(
                        "operation".to_owned(),
                        Tag::String(modifier.operation.clone()),
                    );
                    entry.insert("slot".to_owned(), Tag::String(slot.clone()));
                    list.push(entry);
                }
                stack.components.insert(
                    "minecraft:attribute_modifiers".to_owned(),
                    Tag::List(ferrite_nbt::List::Compound(list)),
                );
            }
            Self::SetBannerPattern { patterns, append } => {
                let mut layers = if *append {
                    match stack.components.remove("minecraft:banner_patterns") {
                        Some(Tag::List(ferrite_nbt::List::Compound(list))) => list,
                        _ => Vec::new(),
                    }
                } else {
                    Vec::new()
                };
                for (pattern, color) in patterns {
                    let mut layer = ferrite_nbt::Compound::new();
                    layer.insert("pattern".to_owned(), Tag::String(pattern.clone()));
                    layer.insert("color".to_owned(), Tag::String(color.clone()));
                    layers.push(layer);
                }
                stack.components.insert(
                    "minecraft:banner_patterns".to_owned(),
                    Tag::List(ferrite_nbt::List::Compound(layers)),
                );
            }
            Self::SetBookCover {
                title,
                author,
                generation,
            } => {
                let mut book = match stack.components.remove("minecraft:written_book_content") {
                    Some(Tag::Compound(book)) => book,
                    _ => {
                        let mut empty = ferrite_nbt::Compound::new();
                        let mut title = ferrite_nbt::Compound::new();
                        title.insert("raw".to_owned(), Tag::String(String::new()));
                        empty.insert("title".to_owned(), Tag::Compound(title));
                        empty.insert("author".to_owned(), Tag::String(String::new()));
                        empty
                    }
                };
                if let Some(title) = title {
                    let mut raw = ferrite_nbt::Compound::new();
                    raw.insert("raw".to_owned(), Tag::String(title.clone()));
                    book.insert("title".to_owned(), Tag::Compound(raw));
                }
                if let Some(author) = author {
                    book.insert("author".to_owned(), Tag::String(author.clone()));
                }
                if let Some(generation) = generation {
                    book.insert("generation".to_owned(), Tag::Int(*generation));
                }
                stack.components.insert(
                    "minecraft:written_book_content".to_owned(),
                    Tag::Compound(book),
                );
            }
            Self::SetLootTable { table, seed } => {
                let mut loot = ferrite_nbt::Compound::new();
                loot.insert("loot_table".to_owned(), Tag::String(table.clone()));
                if *seed != 0 {
                    loot.insert("seed".to_owned(), Tag::Long(*seed));
                }
                stack
                    .components
                    .insert("minecraft:container_loot".to_owned(), Tag::Compound(loot));
            }
            Self::ToggleTooltips { toggles } => {
                let mut display = match stack.components.remove("minecraft:tooltip_display") {
                    Some(Tag::Compound(display)) => display,
                    _ => ferrite_nbt::Compound::new(),
                };
                let mut hidden = match display.shift_remove("hidden_components") {
                    Some(Tag::List(ferrite_nbt::List::String(list))) => list,
                    _ => Vec::new(),
                };
                for (component, shown) in toggles {
                    hidden.retain(|id| id != component);
                    if !shown {
                        hidden.push(component.clone());
                    }
                }
                if !hidden.is_empty() {
                    display.insert(
                        "hidden_components".to_owned(),
                        Tag::List(ferrite_nbt::List::String(hidden)),
                    );
                }
                stack.components.insert(
                    "minecraft:tooltip_display".to_owned(),
                    Tag::Compound(display),
                );
            }
            Self::SetRandomPotion { options } => {
                let candidates = match options {
                    Some(ids) => ids.clone(),
                    None => context
                        .host
                        .as_ref()
                        .map(|host| host.registry_ids("minecraft:potion"))
                        .unwrap_or_default(),
                };
                if candidates.is_empty() {
                    return;
                }
                let index = rng.next_int(i32::try_from(candidates.len()).unwrap_or(i32::MAX));
                let mut contents = match stack.components.remove("minecraft:potion_contents") {
                    Some(Tag::Compound(contents)) => contents,
                    _ => ferrite_nbt::Compound::new(),
                };
                contents.insert(
                    "potion".to_owned(),
                    Tag::String(candidates[index as usize].clone()),
                );
                stack.components.insert(
                    "minecraft:potion_contents".to_owned(),
                    Tag::Compound(contents),
                );
            }
            Self::SetFireworkExplosion {
                shape,
                colors,
                fade_colors,
                trail,
                twinkle,
            } => {
                let mut star = match stack.components.remove("minecraft:firework_explosion") {
                    Some(Tag::Compound(star)) => star,
                    _ => ferrite_nbt::Compound::new(),
                };
                if let Some(shape) = shape {
                    star.insert("shape".to_owned(), Tag::String(shape.clone()));
                }
                if let Some(colors) = colors {
                    star.insert("colors".to_owned(), Tag::IntArray(colors.clone()));
                }
                if let Some(fade) = fade_colors {
                    star.insert("fade_colors".to_owned(), Tag::IntArray(fade.clone()));
                }
                if let Some(trail) = trail {
                    star.insert("has_trail".to_owned(), Tag::Byte(i8::from(*trail)));
                }
                if let Some(twinkle) = twinkle {
                    star.insert("has_twinkle".to_owned(), Tag::Byte(i8::from(*twinkle)));
                }
                stack.components.insert(
                    "minecraft:firework_explosion".to_owned(),
                    Tag::Compound(star),
                );
            }
            Self::Discard => stack.count = 0,
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
