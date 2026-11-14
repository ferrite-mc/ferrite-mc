//! Recipe types and the 26.3 JSON parser.

use crate::tags::ItemTags;
use crate::{Error, Result};
use serde_json::{Map, Value};
use std::borrow::Cow;
use std::collections::HashMap;

/// Normalizes an item or tag name to its namespaced form.
pub(crate) fn normalize_id(name: &str) -> Cow<'_, str> {
    if name.contains(':') {
        Cow::Borrowed(name)
    } else {
        Cow::Owned(format!("minecraft:{name}"))
    }
}

/// Normalizes a tag reference, accepting an optional leading `#`.
pub(crate) fn normalize_tag(name: &str) -> Cow<'_, str> {
    normalize_id(name.strip_prefix('#').unwrap_or(name))
}

/// One alternative in an ingredient: a concrete item or an item tag.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IngredientEntry {
    Item(String),
    Tag(String),
}

impl IngredientEntry {
    /// Whether an item satisfies this alternative.
    pub fn matches(&self, item: &str, tags: &ItemTags) -> bool {
        match self {
            Self::Item(id) => normalize_id(item).as_ref() == id,
            Self::Tag(tag) => tags.contains(tag, item),
        }
    }
}

/// A recipe ingredient: one or more alternative items/tags.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Ingredient {
    pub options: Vec<IngredientEntry>,
}

impl Ingredient {
    /// Whether `item` is one of this ingredient's alternatives.
    pub fn matches(&self, item: &str, tags: &ItemTags) -> bool {
        self.options.iter().any(|option| option.matches(item, tags))
    }

    /// Every concrete item this ingredient can accept, sorted and deduplicated.
    pub fn resolved_items(&self, tags: &ItemTags) -> Vec<String> {
        let mut items = Vec::new();
        for option in &self.options {
            match option {
                IngredientEntry::Item(item) => items.push(item.clone()),
                IngredientEntry::Tag(tag) => {
                    if let Some(entries) = tags.items(tag) {
                        items.extend(entries.iter().cloned());
                    }
                }
            }
        }
        items.sort_unstable();
        items.dedup();
        items
    }

    /// Whether this ingredient has no alternatives at all.
    pub fn is_empty(&self) -> bool {
        self.options.is_empty()
    }

    pub(crate) fn from_value(value: &Value, recipe: &str) -> Result<Self> {
        match value {
            Value::String(value) => Ok(Self {
                options: vec![entry_from_string(value)],
            }),
            Value::Array(values) => {
                let mut ingredient = Self::default();
                for value in values {
                    let nested = Self::from_value(value, recipe)?;
                    for option in nested.options {
                        if !ingredient.options.contains(&option) {
                            ingredient.options.push(option);
                        }
                    }
                }
                Ok(ingredient)
            }
            Value::Object(object) => {
                if let Some(item) = object.get("item").and_then(Value::as_str) {
                    Ok(Self {
                        options: vec![IngredientEntry::Item(normalize_id(item).into_owned())],
                    })
                } else if let Some(tag) = object.get("tag").and_then(Value::as_str) {
                    Ok(Self {
                        options: vec![IngredientEntry::Tag(normalize_tag(tag).into_owned())],
                    })
                } else {
                    Err(Error::recipe(
                        recipe,
                        "an ingredient object needs an `item` or `tag` string",
                    ))
                }
            }
            _ => Err(Error::recipe(
                recipe,
                "an ingredient must be a string, a list or an item/tag object",
            )),
        }
    }
}

impl From<&str> for Ingredient {
    fn from(value: &str) -> Self {
        Self {
            options: vec![entry_from_string(value)],
        }
    }
}

impl From<String> for Ingredient {
    fn from(value: String) -> Self {
        Self::from(value.as_str())
    }
}

fn entry_from_string(value: &str) -> IngredientEntry {
    match value.strip_prefix('#') {
        Some(tag) => IngredientEntry::Tag(normalize_tag(tag).into_owned()),
        None => IngredientEntry::Item(normalize_id(value).into_owned()),
    }
}

/// A recipe result or item reference.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemStack {
    pub item: String,
    pub count: u32,
    /// Raw 26.3 data component patch, if the recipe result carries one.
    pub components: Option<Value>,
}

impl ItemStack {
    pub fn new(item: impl AsRef<str>) -> Self {
        Self {
            item: normalize_id(item.as_ref()).into_owned(),
            count: 1,
            components: None,
        }
    }

    #[must_use]
    pub fn with_count(mut self, count: u32) -> Self {
        self.count = count;
        self
    }

    #[must_use]
    pub fn with_components(mut self, components: Value) -> Self {
        self.components = Some(components);
        self
    }
}

/// The furnace-family recipe kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CookingKind {
    Smelting,
    Blasting,
    Smoking,
    CampfireCooking,
}

impl CookingKind {
    pub const ALL: [Self; 4] = [
        Self::Smelting,
        Self::Blasting,
        Self::Smoking,
        Self::CampfireCooking,
    ];

    /// The full vanilla type name.
    pub const fn type_name(self) -> &'static str {
        match self {
            Self::Smelting => "minecraft:smelting",
            Self::Blasting => "minecraft:blasting",
            Self::Smoking => "minecraft:smoking",
            Self::CampfireCooking => "minecraft:campfire_cooking",
        }
    }

    pub fn from_type_name(name: &str) -> Option<Self> {
        match name.rsplit(':').next().unwrap_or(name) {
            "smelting" => Some(Self::Smelting),
            "blasting" => Some(Self::Blasting),
            "smoking" => Some(Self::Smoking),
            "campfire_cooking" => Some(Self::CampfireCooking),
            _ => None,
        }
    }
}

/// A shaped crafting recipe with its resolved pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct CraftingShaped {
    /// Pattern rows padded to [`CraftingShaped::width`]; `None` is a space.
    pub pattern: Vec<Vec<Option<Ingredient>>>,
    pub width: usize,
    pub height: usize,
    /// Horizontally mirrored pattern, used in addition to the normal one.
    pub mirrored_pattern: Vec<Vec<Option<Ingredient>>>,
    pub result: ItemStack,
    pub show_notification: bool,
}

/// A shapeless crafting recipe.
#[derive(Debug, Clone, PartialEq)]
pub struct CraftingShapeless {
    pub ingredients: Vec<Ingredient>,
    pub result: ItemStack,
    pub show_notification: bool,
}

/// The number of material items a transmute recipe accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaterialCount {
    pub min: u32,
    pub max: u32,
}

/// A crafting transmute recipe (dye a bundle, clone a map, ...).
#[derive(Debug, Clone, PartialEq)]
pub struct CraftingTransmute {
    pub input: Ingredient,
    pub material: Ingredient,
    /// Empty for recipes whose result is computed (for example map cloning).
    pub result: Option<ItemStack>,
    pub add_material_count_to_result: bool,
    pub material_count: Option<MaterialCount>,
}

/// A furnace-family recipe.
#[derive(Debug, Clone, PartialEq)]
pub struct CookingRecipe {
    pub kind: CookingKind,
    pub ingredient: Ingredient,
    pub result: ItemStack,
    pub experience: f32,
    pub cooking_time: u32,
}

/// A stonecutter recipe.
#[derive(Debug, Clone, PartialEq)]
pub struct StonecuttingRecipe {
    pub ingredient: Ingredient,
    pub result: ItemStack,
}

/// A smithing table transformation recipe.
#[derive(Debug, Clone, PartialEq)]
pub struct SmithingTransform {
    pub template: Ingredient,
    pub base: Ingredient,
    pub addition: Ingredient,
    pub result: ItemStack,
}

/// A smithing table trim recipe.
#[derive(Debug, Clone, PartialEq)]
pub struct SmithingTrim {
    pub template: Ingredient,
    pub base: Ingredient,
    pub addition: Ingredient,
    /// Trim pattern id, for example `minecraft:bolt`.
    pub pattern: Option<String>,
}

/// A recipe whose logic is not implemented but which is still listed.
#[derive(Debug, Clone, PartialEq)]
pub struct SpecialRecipe {
    /// Full 26.3 type name, for example `minecraft:crafting_special_repairitem`.
    pub type_name: String,
    pub result: Option<ItemStack>,
    /// Raw recipe JSON, preserved for callers that understand a special type.
    pub data: Value,
}

/// The specific recipe implementation.
#[derive(Debug, Clone, PartialEq)]
pub enum RecipeKind {
    CraftingShaped(CraftingShaped),
    CraftingShapeless(CraftingShapeless),
    CraftingTransmute(CraftingTransmute),
    Cooking(CookingRecipe),
    Stonecutting(StonecuttingRecipe),
    SmithingTransform(SmithingTransform),
    SmithingTrim(SmithingTrim),
    Special(SpecialRecipe),
}

/// One loaded recipe.
#[derive(Debug, Clone, PartialEq)]
pub struct Recipe {
    /// Namespaced recipe id derived from the file name.
    pub id: String,
    pub category: Option<String>,
    pub group: Option<String>,
    pub kind: RecipeKind,
}

impl Recipe {
    /// The full 26.3 type name.
    pub fn type_name(&self) -> &str {
        match &self.kind {
            RecipeKind::CraftingShaped(_) => "minecraft:crafting_shaped",
            RecipeKind::CraftingShapeless(_) => "minecraft:crafting_shapeless",
            RecipeKind::CraftingTransmute(_) => "minecraft:crafting_transmute",
            RecipeKind::Cooking(recipe) => recipe.kind.type_name(),
            RecipeKind::Stonecutting(_) => "minecraft:stonecutting",
            RecipeKind::SmithingTransform(_) => "minecraft:smithing_transform",
            RecipeKind::SmithingTrim(_) => "minecraft:smithing_trim",
            RecipeKind::Special(recipe) => &recipe.type_name,
        }
    }

    /// The result, if the recipe has a static one.
    pub fn result(&self) -> Option<&ItemStack> {
        match &self.kind {
            RecipeKind::CraftingShaped(recipe) => Some(&recipe.result),
            RecipeKind::CraftingShapeless(recipe) => Some(&recipe.result),
            RecipeKind::CraftingTransmute(recipe) => recipe.result.as_ref(),
            RecipeKind::Cooking(recipe) => Some(&recipe.result),
            RecipeKind::Stonecutting(recipe) => Some(&recipe.result),
            RecipeKind::SmithingTransform(recipe) => Some(&recipe.result),
            RecipeKind::SmithingTrim(_) => None,
            RecipeKind::Special(recipe) => recipe.result.as_ref(),
        }
    }

    /// Whether this is an unimplemented special recipe.
    pub fn is_special(&self) -> bool {
        matches!(self.kind, RecipeKind::Special(_))
    }
}

/// Parses one recipe in the vanilla data pack JSON format, e.g. a recipe a plugin adds at runtime.
pub fn parse_recipe_json(id: &str, json: &str) -> Result<Recipe> {
    let value: Value =
        serde_json::from_str(json).map_err(|error| Error::recipe(id, error.to_string()))?;
    parse_recipe(id, &value)
}

pub(crate) fn parse_recipe(id: &str, value: &Value) -> Result<Recipe> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::recipe(id, "a recipe must be a JSON object"))?;
    let type_name = required_str(object, "type", id)?;
    let short = type_name.rsplit(':').next().unwrap_or(type_name);
    let category = optional_str(object, "category", id)?;
    let group = optional_str(object, "group", id)?;

    let kind = match short {
        "crafting_shaped" => RecipeKind::CraftingShaped(parse_shaped(object, id)?),
        "crafting_shapeless" => RecipeKind::CraftingShapeless(parse_shapeless(object, id)?),
        "crafting_transmute" => RecipeKind::CraftingTransmute(parse_transmute(object, id)?),
        "smelting" => RecipeKind::Cooking(parse_cooking(CookingKind::Smelting, object, id)?),
        "blasting" => RecipeKind::Cooking(parse_cooking(CookingKind::Blasting, object, id)?),
        "smoking" => RecipeKind::Cooking(parse_cooking(CookingKind::Smoking, object, id)?),
        "campfire_cooking" => {
            RecipeKind::Cooking(parse_cooking(CookingKind::CampfireCooking, object, id)?)
        }
        "stonecutting" => RecipeKind::Stonecutting(StonecuttingRecipe {
            ingredient: ingredient_field(object, "ingredient", id)?,
            result: required_result(object, id)?,
        }),
        "smithing_transform" => RecipeKind::SmithingTransform(SmithingTransform {
            template: optional_ingredient_field(object, "template", id)?,
            base: ingredient_field(object, "base", id)?,
            addition: optional_ingredient_field(object, "addition", id)?,
            result: required_result(object, id)?,
        }),
        "smithing_trim" => RecipeKind::SmithingTrim(SmithingTrim {
            template: optional_ingredient_field(object, "template", id)?,
            base: ingredient_field(object, "base", id)?,
            addition: optional_ingredient_field(object, "addition", id)?,
            pattern: optional_str(object, "pattern", id)?,
        }),
        _ => RecipeKind::Special(SpecialRecipe {
            type_name: type_name.to_owned(),
            result: optional_result(object.get("result"), id)?,
            data: value.clone(),
        }),
    };

    Ok(Recipe {
        id: id.to_owned(),
        category,
        group,
        kind,
    })
}

fn parse_shaped(object: &Map<String, Value>, id: &str) -> Result<CraftingShaped> {
    let pattern = object
        .get("pattern")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::recipe(id, "`pattern` must be an array"))?;
    let rows = pattern
        .iter()
        .map(|row| {
            row.as_str()
                .ok_or_else(|| Error::recipe(id, "each `pattern` row must be a string"))
        })
        .collect::<Result<Vec<_>>>()?;
    if rows.is_empty() {
        return Err(Error::recipe(id, "`pattern` must not be empty"));
    }
    let height = rows.len();
    let width = rows
        .iter()
        .map(|row| row.chars().count())
        .max()
        .unwrap_or(0);
    if width == 0 || width > 3 || height > 3 {
        return Err(Error::recipe(
            id,
            format!("pattern {width}x{height} does not fit a 3x3 crafting grid"),
        ));
    }

    let key = object
        .get("key")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::recipe(id, "`key` must be an object"))?;
    let mut ingredients = HashMap::with_capacity(key.len());
    for (symbol, value) in key {
        let mut chars = symbol.chars();
        let symbol_char = chars
            .next()
            .ok_or_else(|| Error::recipe(id, "`key` contains an empty symbol"))?;
        if chars.next().is_some() {
            return Err(Error::recipe(
                id,
                format!("key symbol `{symbol}` must be one character"),
            ));
        }
        ingredients.insert(symbol_char, Ingredient::from_value(value, id)?);
    }

    let mut resolved = Vec::with_capacity(height);
    for row in &rows {
        let mut cells = Vec::with_capacity(width);
        for symbol in row.chars() {
            if symbol == ' ' {
                cells.push(None);
            } else {
                let ingredient = ingredients.get(&symbol).ok_or_else(|| {
                    Error::recipe(
                        id,
                        format!("pattern symbol `{symbol}` is missing from `key`"),
                    )
                })?;
                cells.push(Some(ingredient.clone()));
            }
        }
        while cells.len() < width {
            cells.push(None);
        }
        resolved.push(cells);
    }

    let mirrored_pattern = resolved
        .iter()
        .map(|row| row.iter().rev().cloned().collect())
        .collect();
    let show_notification = optional_bool(object, "show_notification", id)?.unwrap_or(true);

    Ok(CraftingShaped {
        pattern: resolved,
        width,
        height,
        mirrored_pattern,
        result: required_result(object, id)?,
        show_notification,
    })
}

fn parse_shapeless(object: &Map<String, Value>, id: &str) -> Result<CraftingShapeless> {
    let values = object
        .get("ingredients")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::recipe(id, "`ingredients` must be an array"))?;
    let ingredients = values
        .iter()
        .map(|value| Ingredient::from_value(value, id))
        .collect::<Result<Vec<_>>>()?;
    Ok(CraftingShapeless {
        ingredients,
        result: required_result(object, id)?,
        show_notification: optional_bool(object, "show_notification", id)?.unwrap_or(true),
    })
}

fn parse_transmute(object: &Map<String, Value>, id: &str) -> Result<CraftingTransmute> {
    let material_count = match object.get("material_count") {
        None => None,
        Some(value) => {
            let value = value
                .as_object()
                .ok_or_else(|| Error::recipe(id, "`material_count` must be an object"))?;
            let min = required_u32(value, "min", id)?;
            let max = required_u32(value, "max", id)?;
            if min == 0 || min > max {
                return Err(Error::recipe(id, "`material_count` needs 1 <= min <= max"));
            }
            Some(MaterialCount { min, max })
        }
    };
    Ok(CraftingTransmute {
        input: ingredient_field(object, "input", id)?,
        material: ingredient_field(object, "material", id)?,
        result: optional_result(object.get("result"), id)?,
        add_material_count_to_result: optional_bool(object, "add_material_count_to_result", id)?
            .unwrap_or(false),
        material_count,
    })
}

fn parse_cooking(
    kind: CookingKind,
    object: &Map<String, Value>,
    id: &str,
) -> Result<CookingRecipe> {
    let experience = optional_f32(object, "experience", id)?.unwrap_or(0.0);
    if !experience.is_finite() {
        return Err(Error::recipe(id, "`experience` must be finite"));
    }
    Ok(CookingRecipe {
        kind,
        ingredient: ingredient_field(object, "ingredient", id)?,
        result: required_result(object, id)?,
        experience,
        cooking_time: optional_u32(object, "cookingtime", id)?.unwrap_or(200),
    })
}

fn ingredient_field(object: &Map<String, Value>, key: &str, id: &str) -> Result<Ingredient> {
    let value = object
        .get(key)
        .ok_or_else(|| Error::recipe(id, format!("missing `{key}`")))?;
    Ingredient::from_value(value, id)
}

/// A smithing slot vanilla makes optional (`Optional<Ingredient>`): a missing key is an
/// [`Ingredient`] with no alternatives, which only an empty slot satisfies.
fn optional_ingredient_field(
    object: &Map<String, Value>,
    key: &str,
    id: &str,
) -> Result<Ingredient> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(Ingredient::default()),
        Some(value) => Ingredient::from_value(value, id),
    }
}

fn required_result(object: &Map<String, Value>, id: &str) -> Result<ItemStack> {
    let value = object
        .get("result")
        .ok_or_else(|| Error::recipe(id, "missing `result`"))?;
    parse_result(value, id)
}

fn parse_result(value: &Value, id: &str) -> Result<ItemStack> {
    match value {
        Value::String(item) => Ok(ItemStack::new(item)),
        Value::Object(object) => {
            let item = required_str(object, "id", id)?;
            let count = match object.get("count") {
                None => 1,
                Some(value) => value
                    .as_u64()
                    .and_then(|count| u32::try_from(count).ok())
                    .filter(|count| *count > 0)
                    .ok_or_else(|| {
                        Error::recipe(id, "result `count` must be a positive integer")
                    })?,
            };
            let components = match object.get("components") {
                None | Some(Value::Null) => None,
                Some(value @ Value::Object(_)) => Some(value.clone()),
                Some(_) => {
                    return Err(Error::recipe(id, "result `components` must be an object"));
                }
            };
            Ok(ItemStack {
                item: normalize_id(item).into_owned(),
                count,
                components,
            })
        }
        _ => Err(Error::recipe(
            id,
            "`result` must be an item id string or an object",
        )),
    }
}

fn optional_result(value: Option<&Value>, id: &str) -> Result<Option<ItemStack>> {
    match value {
        None => Ok(None),
        Some(Value::Object(object)) if object.is_empty() => Ok(None),
        Some(value) => parse_result(value, id).map(Some),
    }
}

fn required_str<'a>(object: &'a Map<String, Value>, key: &str, id: &str) -> Result<&'a str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::recipe(id, format!("missing or invalid `{key}` string")))
}

fn optional_str(object: &Map<String, Value>, key: &str, id: &str) -> Result<Option<String>> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .map(|value| Some(value.to_owned()))
            .ok_or_else(|| Error::recipe(id, format!("`{key}` must be a string"))),
    }
}

fn optional_bool(object: &Map<String, Value>, key: &str, id: &str) -> Result<Option<bool>> {
    match object.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_bool()
            .map(Some)
            .ok_or_else(|| Error::recipe(id, format!("`{key}` must be a boolean"))),
    }
}

fn optional_u32(object: &Map<String, Value>, key: &str, id: &str) -> Result<Option<u32>> {
    match object.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .map(Some)
            .ok_or_else(|| Error::recipe(id, format!("`{key}` must be a non-negative integer"))),
    }
}

fn required_u32(object: &Map<String, Value>, key: &str, id: &str) -> Result<u32> {
    optional_u32(object, key, id)?.ok_or_else(|| Error::recipe(id, format!("missing `{key}`")))
}

fn optional_f32(object: &Map<String, Value>, key: &str, id: &str) -> Result<Option<f32>> {
    match object.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_f64()
            .map(|value| Some(value as f32))
            .ok_or_else(|| Error::recipe(id, format!("`{key}` must be a number"))),
    }
}
