//! Data component types, typed component values and their NBT forms.
//!
//! A component value is typed when the crate knows the 26.3 NBT shape of its component type.
//! Decoding is deliberately tolerant of the shapes vanilla produces (missing default fields,
//! an int where a float is expected) and falls back to [`Component::Opaque`] for anything else,
//! so a stack never loses a value it does not understand. The network codec in [`crate::wire`]
//! has the same policy with [`Component::Raw`], which keeps a received payload verbatim.

use std::collections::BTreeMap;

use ferrite_nbt::{Compound, List, Tag};
use ferrite_text::Component as Text;

use crate::Error;
use crate::component_ids as ids;
use crate::stack::ItemStack;

/// Number of entries of the `minecraft:data_component_type` registry.
pub const COMPONENT_COUNT: usize = ids::COUNT;

/// One entry of the `minecraft:data_component_type` registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ComponentType {
    /// Vanilla name, e.g. `"minecraft:damage"`.
    pub name: &'static str,
    /// Protocol id in the registry.
    pub id: u16,
}

/// Strips an optional `minecraft:` namespace so lookups accept both spellings.
fn key(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

/// The component type with the given name, with or without the `minecraft:` prefix.
pub fn component_type(name: &str) -> Option<ComponentType> {
    let wanted = key(name);
    let index = ids::TYPES
        .binary_search_by(|&(candidate, _)| key(candidate).cmp(wanted))
        .ok()?;
    let (name, id) = ids::TYPES.get(index).copied()?;
    Some(ComponentType { name, id })
}

/// Protocol id of a component type name, with or without the `minecraft:` prefix.
pub fn component_id(name: &str) -> Option<u16> {
    component_type(name).map(|component| component.id)
}

/// Name of a component type id.
pub fn component_name(id: u16) -> Option<&'static str> {
    ids::NAMES.get(usize::from(id)).copied()
}

/// Every registered component type, sorted by name.
pub fn component_types() -> impl Iterator<Item = ComponentType> + 'static {
    ids::TYPES
        .iter()
        .map(|&(name, id)| ComponentType { name, id })
}

/// A registry holder set: either a `#namespace:tag` reference or an explicit list of ids.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HolderSet {
    Tag(String),
    Ids(Vec<String>),
}

impl HolderSet {
    /// Encodes this holder set in the 26.3 NBT shape (`"#tag"` or list of ids).
    pub fn to_nbt(&self) -> Tag {
        match self {
            Self::Tag(tag) => {
                let normalized = tag.strip_prefix('#').unwrap_or(tag);
                Tag::String(format!("#{normalized}"))
            }
            Self::Ids(ids) => Tag::List(if ids.is_empty() {
                List::End
            } else {
                List::String(ids.clone())
            }),
        }
    }

    /// Decodes a holder set from the 26.3 NBT shape (`"#tag"`, `"id"`, or `["id", ...]`).
    pub fn from_nbt(tag: &Tag) -> Option<Self> {
        match tag {
            Tag::String(value) => {
                if let Some(tag_name) = value.strip_prefix('#') {
                    Some(Self::Tag(tag_name.to_owned()))
                } else {
                    Some(Self::Ids(vec![value.clone()]))
                }
            }
            Tag::List(List::End) => Some(Self::Ids(Vec::new())),
            Tag::List(List::String(values)) => Some(Self::Ids(values.clone())),
            _ => None,
        }
    }
}

/// A single block predicate in `minecraft:can_break` or `minecraft:can_place_on`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockPredicate {
    pub blocks: Option<HolderSet>,
}

impl BlockPredicate {
    /// Encodes this predicate in the 26.3 NBT shape.
    pub fn to_nbt(&self) -> Compound {
        let mut compound = Compound::new();
        if let Some(blocks) = &self.blocks {
            compound.insert("blocks".to_owned(), blocks.to_nbt());
        }
        compound
    }

    /// Decodes a block predicate when it only uses the modeled `blocks` field.
    pub fn from_nbt(tag: &Tag) -> Option<Self> {
        let Tag::Compound(compound) = tag else {
            return None;
        };
        for key in compound.keys() {
            if key != "blocks" {
                return None;
            }
        }
        let blocks = match compound.get("blocks") {
            Some(raw) => Some(HolderSet::from_nbt(raw)?),
            None => None,
        };
        Some(Self { blocks })
    }
}

/// Vanilla rarity, which colors the item name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Rarity {
    Common,
    Uncommon,
    Rare,
    Epic,
}

impl Rarity {
    /// Every rarity, in vanilla order.
    pub const ALL: [Self; 4] = [Self::Common, Self::Uncommon, Self::Rare, Self::Epic];

    /// The name vanilla uses, e.g. `"uncommon"`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Common => "common",
            Self::Uncommon => "uncommon",
            Self::Rare => "rare",
            Self::Epic => "epic",
        }
    }

    /// Parses a rarity name, with or without the `minecraft:` prefix.
    pub fn from_name(name: &str) -> Option<Self> {
        match key(name) {
            "common" => Some(Self::Common),
            "uncommon" => Some(Self::Uncommon),
            "rare" => Some(Self::Rare),
            "epic" => Some(Self::Epic),
            _ => None,
        }
    }
}

/// How an attribute modifier applies its amount.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AttributeOperation {
    AddValue,
    AddMultipliedBase,
    AddMultipliedTotal,
}

impl AttributeOperation {
    /// Every operation, in the order of vanilla's protocol ids.
    pub const ALL: [Self; 3] = [
        Self::AddValue,
        Self::AddMultipliedBase,
        Self::AddMultipliedTotal,
    ];

    /// The name vanilla uses, e.g. `"add_multiplied_base"`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::AddValue => "add_value",
            Self::AddMultipliedBase => "add_multiplied_base",
            Self::AddMultipliedTotal => "add_multiplied_total",
        }
    }

    /// Parses an operation name, with or without the `minecraft:` prefix.
    pub fn from_name(name: &str) -> Option<Self> {
        match key(name) {
            "add_value" => Some(Self::AddValue),
            "add_multiplied_base" => Some(Self::AddMultipliedBase),
            "add_multiplied_total" => Some(Self::AddMultipliedTotal),
            _ => None,
        }
    }
}

/// The equipment slot an attribute modifier applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EquipmentSlot {
    Any,
    Mainhand,
    Offhand,
    Hand,
    Feet,
    Legs,
    Chest,
    Head,
    Armor,
    Body,
    Saddle,
}

impl EquipmentSlot {
    /// Every slot, in the order of vanilla's `EquipmentSlotGroup` ids.
    pub const ALL: [Self; 11] = [
        Self::Any,
        Self::Mainhand,
        Self::Offhand,
        Self::Hand,
        Self::Feet,
        Self::Legs,
        Self::Chest,
        Self::Head,
        Self::Armor,
        Self::Body,
        Self::Saddle,
    ];

    /// The name vanilla uses, e.g. `"mainhand"`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Mainhand => "mainhand",
            Self::Offhand => "offhand",
            Self::Hand => "hand",
            Self::Feet => "feet",
            Self::Legs => "legs",
            Self::Chest => "chest",
            Self::Head => "head",
            Self::Armor => "armor",
            Self::Body => "body",
            Self::Saddle => "saddle",
        }
    }

    /// Parses a slot name, with or without the `minecraft:` prefix.
    pub fn from_name(name: &str) -> Option<Self> {
        match key(name) {
            "any" => Some(Self::Any),
            "mainhand" => Some(Self::Mainhand),
            "offhand" => Some(Self::Offhand),
            "hand" => Some(Self::Hand),
            "feet" => Some(Self::Feet),
            "legs" => Some(Self::Legs),
            "chest" => Some(Self::Chest),
            "head" => Some(Self::Head),
            "armor" => Some(Self::Armor),
            "body" => Some(Self::Body),
            "saddle" => Some(Self::Saddle),
            _ => None,
        }
    }
}

/// One entry of the `minecraft:attribute_modifiers` component.
#[derive(Clone, Debug, PartialEq)]
pub struct AttributeModifier {
    /// Attribute id, e.g. `"minecraft:attack_damage"`.
    pub attribute: String,
    /// Modifier id, e.g. `"minecraft:base_attack_damage"`.
    pub id: String,
    pub amount: f64,
    pub operation: AttributeOperation,
    pub slot: EquipmentSlot,
}

impl AttributeModifier {
    fn to_compound(&self) -> Compound {
        let mut compound = Compound::new();
        compound.insert("type".to_owned(), Tag::String(self.attribute.clone()));
        compound.insert("amount".to_owned(), Tag::Double(self.amount));
        compound.insert("id".to_owned(), Tag::String(self.id.clone()));
        compound.insert(
            "operation".to_owned(),
            Tag::String(self.operation.name().to_owned()),
        );
        compound.insert("slot".to_owned(), Tag::String(self.slot.name().to_owned()));
        compound
    }

    fn from_compound(compound: &Compound) -> Option<Self> {
        if compound.keys().any(|key| {
            !matches!(
                key.as_str(),
                "type" | "amount" | "id" | "operation" | "slot"
            )
        }) {
            return None;
        }
        Some(Self {
            attribute: as_str(compound.get("type")?)?.to_owned(),
            id: as_str(compound.get("id")?)?.to_owned(),
            amount: as_f64(compound.get("amount")?)?,
            operation: AttributeOperation::from_name(as_str(compound.get("operation")?)?)?,
            slot: EquipmentSlot::from_name(as_str(compound.get("slot")?)?)?,
        })
    }
}

/// The `minecraft:enchantments` and `minecraft:stored_enchantments` value: a map from
/// enchantment id to level.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Enchantments {
    /// Enchantment id (`"minecraft:sharpness"`) to level, sorted by id.
    pub levels: BTreeMap<String, i32>,
}

impl Enchantments {
    /// An empty enchantment map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether no enchantment is present.
    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }

    /// The level of an enchantment, with or without the `minecraft:` prefix.
    pub fn level(&self, enchantment: &str) -> Option<i32> {
        if let Some(level) = self.levels.get(enchantment) {
            return Some(*level);
        }
        let wanted = key(enchantment);
        self.levels
            .iter()
            .find(|(id, _)| key(id) == wanted)
            .map(|(_, level)| *level)
    }

    /// Sets the level of an enchantment, replacing any existing entry.
    pub fn set(&mut self, enchantment: impl Into<String>, level: i32) {
        self.levels.insert(enchantment.into(), level);
    }

    fn to_tag(&self) -> Tag {
        Tag::Compound(
            self.levels
                .iter()
                .map(|(enchantment, level)| (enchantment.clone(), Tag::Int(*level)))
                .collect(),
        )
    }

    fn from_tag(tag: &Tag) -> Option<Self> {
        let Tag::Compound(levels) = tag else {
            return None;
        };
        let mut enchantments = Self::new();
        for (enchantment, level) in levels {
            enchantments.set(enchantment.clone(), as_int(level)?);
        }
        Some(enchantments)
    }
}

/// The `minecraft:custom_model_data` value, which item models can match on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CustomModelData {
    pub floats: Vec<f32>,
    pub flags: Vec<bool>,
    pub strings: Vec<String>,
    pub colors: Vec<i32>,
}

impl CustomModelData {
    fn to_tag(&self) -> Tag {
        let mut compound = Compound::new();
        compound.insert(
            "floats".to_owned(),
            Tag::List(List::Float(self.floats.clone())),
        );
        compound.insert(
            "flags".to_owned(),
            Tag::List(List::Byte(
                self.flags.iter().map(|flag| i8::from(*flag)).collect(),
            )),
        );
        compound.insert(
            "strings".to_owned(),
            Tag::List(List::String(self.strings.clone())),
        );
        compound.insert(
            "colors".to_owned(),
            Tag::List(List::Int(self.colors.clone())),
        );
        Tag::Compound(compound)
    }

    fn from_tag(tag: &Tag) -> Option<Self> {
        let Tag::Compound(fields) = tag else {
            return None;
        };
        if fields
            .keys()
            .any(|key| !matches!(key.as_str(), "floats" | "flags" | "strings" | "colors"))
        {
            return None;
        }
        Some(Self {
            floats: match fields.get("floats") {
                Some(tag) => float_list(tag)?,
                None => Vec::new(),
            },
            flags: match fields.get("flags") {
                Some(tag) => flag_list(tag)?,
                None => Vec::new(),
            },
            strings: match fields.get("strings") {
                Some(tag) => string_list(tag)?,
                None => Vec::new(),
            },
            colors: match fields.get("colors") {
                Some(tag) => int_list(tag)?,
                None => Vec::new(),
            },
        })
    }
}

/// The `minecraft:food` value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Food {
    /// Hunger points restored.
    pub nutrition: i32,
    /// Saturation restored, in saturation points.
    pub saturation: f32,
    /// Whether the item can be eaten with a full hunger bar.
    pub can_always_eat: bool,
}

impl Food {
    /// A food component that cannot be eaten when full.
    pub const fn new(nutrition: i32, saturation: f32) -> Self {
        Self {
            nutrition,
            saturation,
            can_always_eat: false,
        }
    }

    fn to_compound(self) -> Compound {
        let mut compound = Compound::new();
        compound.insert("nutrition".to_owned(), Tag::Int(self.nutrition));
        compound.insert("saturation".to_owned(), Tag::Float(self.saturation));
        if self.can_always_eat {
            compound.insert("can_always_eat".to_owned(), Tag::Byte(1));
        }
        compound
    }

    fn from_tag(tag: &Tag) -> Option<Self> {
        let Tag::Compound(fields) = tag else {
            return None;
        };
        if fields
            .keys()
            .any(|key| !matches!(key.as_str(), "nutrition" | "saturation" | "can_always_eat"))
        {
            return None;
        }
        Some(Self {
            nutrition: as_int(fields.get("nutrition")?)?,
            saturation: match fields.get("saturation") {
                Some(tag) => as_f32(tag)?,
                None => 0.0,
            },
            can_always_eat: match fields.get("can_always_eat") {
                Some(tag) => as_bool(tag)?,
                None => false,
            },
        })
    }
}

/// One `minecraft:potion_contents` custom effect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PotionEffect {
    /// `minecraft:mob_effect` registry name.
    pub effect: String,
    pub amplifier: i32,
    pub duration: i32,
    pub ambient: bool,
    /// `show_particles`.
    pub visible: bool,
    pub show_icon: bool,
}

/// The `minecraft:potion_contents` value: a base potion plus custom effects.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PotionContents {
    /// `minecraft:potion` registry name; `None` means water/empty.
    pub potion: Option<String>,
    pub custom_color: Option<i32>,
    pub custom_effects: Vec<PotionEffect>,
    pub custom_name: Option<String>,
}

impl PotionContents {
    fn to_compound(&self) -> Compound {
        let mut compound = Compound::new();
        if let Some(potion) = &self.potion {
            compound.insert("potion".to_owned(), Tag::String(potion.clone()));
        }
        if let Some(color) = self.custom_color {
            compound.insert("custom_color".to_owned(), Tag::Int(color));
        }
        if !self.custom_effects.is_empty() {
            compound.insert(
                "custom_effects".to_owned(),
                Tag::List(List::Compound(
                    self.custom_effects
                        .iter()
                        .map(PotionEffect::to_compound)
                        .collect(),
                )),
            );
        }
        if let Some(name) = &self.custom_name {
            compound.insert("custom_name".to_owned(), Tag::String(name.clone()));
        }
        compound
    }

    fn from_tag(tag: &Tag) -> Option<Self> {
        let Tag::Compound(fields) = tag else {
            return None;
        };
        let potion = match fields.get("potion") {
            Some(Tag::String(potion)) => Some(potion.clone()),
            Some(_) => return None,
            None => None,
        };
        let custom_color = match fields.get("custom_color") {
            Some(tag) => Some(as_int(tag)?),
            None => None,
        };
        let custom_effects = match fields.get("custom_effects") {
            Some(Tag::List(List::Compound(effects))) => effects
                .iter()
                .map(PotionEffect::from_compound)
                .collect::<Option<Vec<_>>>()?,
            Some(_) => return None,
            None => Vec::new(),
        };
        let custom_name = match fields.get("custom_name") {
            Some(Tag::String(name)) => Some(name.clone()),
            Some(_) => return None,
            None => None,
        };
        Some(Self {
            potion,
            custom_color,
            custom_effects,
            custom_name,
        })
    }
}

impl PotionEffect {
    fn to_compound(&self) -> Compound {
        let mut compound = Compound::new();
        compound.insert("id".to_owned(), Tag::String(self.effect.clone()));
        compound.insert("amplifier".to_owned(), Tag::Int(self.amplifier));
        compound.insert("duration".to_owned(), Tag::Int(self.duration));
        compound.insert("ambient".to_owned(), Tag::Byte(i8::from(self.ambient)));
        compound.insert(
            "show_particles".to_owned(),
            Tag::Byte(i8::from(self.visible)),
        );
        compound.insert("show_icon".to_owned(), Tag::Byte(i8::from(self.show_icon)));
        compound
    }

    fn from_compound(fields: &Compound) -> Option<Self> {
        Some(Self {
            effect: match fields.get("id")? {
                Tag::String(effect) => effect.clone(),
                _ => return None,
            },
            amplifier: fields.get("amplifier").map_or(Some(0), as_int)?,
            duration: fields.get("duration").map_or(Some(0), as_int)?,
            ambient: fields.get("ambient").map_or(Some(false), as_bool)?,
            visible: fields.get("show_particles").map_or(Some(true), as_bool)?,
            show_icon: fields.get("show_icon").map_or(Some(true), as_bool)?,
        })
    }
}

/// One rule of the `minecraft:tool` component.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolRule {
    /// A block id or a `#`-prefixed block tag.
    pub blocks: String,
    /// Mining speed for these blocks; `None` keeps the default speed.
    pub speed: Option<f32>,
    /// Whether these blocks drop their items; `None` keeps the item's behavior.
    pub correct_for_drops: Option<bool>,
}

impl ToolRule {
    /// A rule for the given blocks and tag, without speed or drop overrides.
    pub fn new(blocks: impl Into<String>) -> Self {
        Self {
            blocks: blocks.into(),
            speed: None,
            correct_for_drops: None,
        }
    }

    fn to_compound(&self) -> Compound {
        let mut compound = Compound::new();
        compound.insert("blocks".to_owned(), Tag::String(self.blocks.clone()));
        if let Some(speed) = self.speed {
            compound.insert("speed".to_owned(), Tag::Float(speed));
        }
        if let Some(correct_for_drops) = self.correct_for_drops {
            compound.insert(
                "correct_for_drops".to_owned(),
                Tag::Byte(i8::from(correct_for_drops)),
            );
        }
        compound
    }

    fn from_compound(compound: &Compound) -> Option<Self> {
        if compound
            .keys()
            .any(|key| !matches!(key.as_str(), "blocks" | "speed" | "correct_for_drops"))
        {
            return None;
        }
        Some(Self {
            blocks: as_str(compound.get("blocks")?)?.to_owned(),
            speed: match compound.get("speed") {
                Some(tag) => Some(as_f32(tag)?),
                None => None,
            },
            correct_for_drops: match compound.get("correct_for_drops") {
                Some(tag) => Some(as_bool(tag)?),
                None => None,
            },
        })
    }
}

/// The `minecraft:trim` value: which material and pattern were smithed onto the armor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trim {
    /// Trim material id, for example `minecraft:amethyst`.
    pub material: String,
    /// Trim pattern id, for example `minecraft:bolt`.
    pub pattern: String,
}

impl Trim {
    fn to_compound(&self) -> Compound {
        let mut compound = Compound::new();
        compound.insert("material".into(), Tag::String(self.material.clone()));
        compound.insert("pattern".into(), Tag::String(self.pattern.clone()));
        compound
    }

    fn from_tag(tag: &Tag) -> Option<Self> {
        let Tag::Compound(compound) = tag else {
            return None;
        };
        Some(Self {
            material: as_str(compound.get("material")?)?.to_owned(),
            pattern: as_str(compound.get("pattern")?)?.to_owned(),
        })
    }
}

/// The `minecraft:lodestone_tracker` value: where a compass points.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LodestoneTracker {
    /// The dimension id and block position of the lodestone, if the compass is bound to one.
    pub target: Option<(String, [i32; 3])>,
    /// Whether the compass stops pointing when the lodestone is gone.
    pub tracked: bool,
}

impl LodestoneTracker {
    fn to_compound(&self) -> Compound {
        let mut compound = Compound::new();
        if let Some((dimension, pos)) = &self.target {
            let mut target = Compound::new();
            target.insert("pos".into(), Tag::IntArray(pos.to_vec()));
            target.insert("dimension".into(), Tag::String(dimension.clone()));
            compound.insert("target".into(), Tag::Compound(target));
        }
        compound.insert("tracked".into(), Tag::Byte(i8::from(self.tracked)));
        compound
    }

    fn from_tag(tag: &Tag) -> Option<Self> {
        let Tag::Compound(compound) = tag else {
            return None;
        };
        let target = match compound.get("target") {
            Some(Tag::Compound(target)) => {
                let Some(Tag::IntArray(pos)) = target.get("pos") else {
                    return None;
                };
                Some((
                    as_str(target.get("dimension")?)?.to_owned(),
                    <[i32; 3]>::try_from(pos.as_slice()).ok()?,
                ))
            }
            Some(_) => return None,
            None => None,
        };
        let tracked = match compound.get("tracked") {
            Some(tag) => as_bool(tag)?,
            None => true,
        };
        Some(Self { target, tracked })
    }
}

/// The `minecraft:tool` value.
#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    /// Per-block-group mining rules, in priority order.
    pub rules: Vec<ToolRule>,
    /// Mining speed for blocks no rule covers.
    pub default_mining_speed: f32,
    /// Durability lost per block mined.
    pub damage_per_block: i32,
    /// Whether creative mode can break blocks this tool cannot harvest.
    pub can_destroy_blocks_in_creative: bool,
}

impl Default for Tool {
    fn default() -> Self {
        Self {
            rules: Vec::new(),
            default_mining_speed: 1.0,
            damage_per_block: 0,
            can_destroy_blocks_in_creative: false,
        }
    }
}

impl Tool {
    fn to_compound(&self) -> Compound {
        let mut compound = Compound::new();
        compound.insert(
            "rules".to_owned(),
            Tag::List(if self.rules.is_empty() {
                List::End
            } else {
                List::Compound(self.rules.iter().map(ToolRule::to_compound).collect())
            }),
        );
        if self.default_mining_speed != 1.0 {
            compound.insert(
                "default_mining_speed".to_owned(),
                Tag::Float(self.default_mining_speed),
            );
        }
        if self.damage_per_block != 0 {
            compound.insert(
                "damage_per_block".to_owned(),
                Tag::Int(self.damage_per_block),
            );
        }
        if self.can_destroy_blocks_in_creative {
            compound.insert("can_destroy_blocks_in_creative".to_owned(), Tag::Byte(1));
        }
        compound
    }

    fn from_tag(tag: &Tag) -> Option<Self> {
        let Tag::Compound(fields) = tag else {
            return None;
        };
        if fields.keys().any(|key| {
            !matches!(
                key.as_str(),
                "rules"
                    | "default_mining_speed"
                    | "damage_per_block"
                    | "can_destroy_blocks_in_creative"
            )
        }) {
            return None;
        }
        let rules = match fields.get("rules") {
            Some(Tag::List(List::End)) | None => Vec::new(),
            Some(Tag::List(List::Compound(values))) => values
                .iter()
                .map(ToolRule::from_compound)
                .collect::<Option<Vec<_>>>()?,
            Some(_) => return None,
        };
        Some(Self {
            rules,
            default_mining_speed: match fields.get("default_mining_speed") {
                Some(tag) => as_f32(tag)?,
                None => 1.0,
            },
            damage_per_block: match fields.get("damage_per_block") {
                Some(tag) => as_int(tag)?,
                None => 0,
            },
            can_destroy_blocks_in_creative: match fields.get("can_destroy_blocks_in_creative") {
                Some(tag) => as_bool(tag)?,
                None => false,
            },
        })
    }
}

/// The `minecraft:tooltip_display` value: whether the whole tooltip is hidden and which
/// components are left out of the tooltip.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TooltipDisplay {
    /// Hide the tooltip entirely.
    pub hide_tooltip: bool,
    /// Component type ids hidden from the tooltip, in display order.
    pub hidden_components: Vec<u16>,
}

impl TooltipDisplay {
    fn to_tag(&self) -> Result<Tag, Error> {
        let mut compound = Compound::new();
        if self.hide_tooltip {
            compound.insert("hide_tooltip".to_owned(), Tag::Byte(1));
        }
        if !self.hidden_components.is_empty() {
            let mut names = Vec::with_capacity(self.hidden_components.len());
            for id in &self.hidden_components {
                names.push(
                    component_name(*id)
                        .ok_or(Error::UnknownComponentId(*id))?
                        .to_owned(),
                );
            }
            compound.insert(
                "hidden_components".to_owned(),
                Tag::List(List::String(names)),
            );
        }
        Ok(Tag::Compound(compound))
    }

    fn from_tag(tag: &Tag) -> Option<Self> {
        let Tag::Compound(fields) = tag else {
            return None;
        };
        if fields
            .keys()
            .any(|key| !matches!(key.as_str(), "hide_tooltip" | "hidden_components"))
        {
            return None;
        }
        let hide_tooltip = match fields.get("hide_tooltip") {
            Some(tag) => as_bool(tag)?,
            None => false,
        };
        let hidden_components = match fields.get("hidden_components") {
            Some(Tag::List(List::End)) | None => Vec::new(),
            Some(Tag::List(List::String(names))) => names
                .iter()
                .map(|name| component_id(name))
                .collect::<Option<Vec<_>>>()?,
            Some(Tag::List(List::Int(ids))) => ids
                .iter()
                .map(|id| {
                    u16::try_from(*id)
                        .ok()
                        .filter(|id| *id < COMPONENT_COUNT as u16)
                })
                .collect::<Option<Vec<_>>>()?,
            Some(_) => return None,
        };
        Some(Self {
            hide_tooltip,
            hidden_components,
        })
    }
}

/// Vanilla's limit on `minecraft:container` slots (`ItemContainerContents.MAX_SIZE`).
pub const CONTAINER_MAX_SLOTS: usize = 256;

/// Vanilla `BundleContents.MAX_WEIGHT`: a bundle holds one stack's worth of weight.
pub const BUNDLE_MAX_WEIGHT: i32 = 64;
/// The weight an empty bundle itself takes inside another bundle (vanilla
/// `BundleContents.getWeight` starts a nested bundle at 4).
pub const BUNDLE_INNER_WEIGHT: i32 = 4;

/// The `minecraft:bundle_contents` value: the stacks inside a bundle.
///
/// A bundle has no slots; every item takes `64 / max_stack_size` of its 64 weight, so a
/// 64-stackable item weighs 1, a 16-stackable item 4 and an unstackable item 64. A nested
/// bundle weighs 4 plus whatever it holds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BundleContents {
    /// The stored stacks, in insertion order.
    pub items: Vec<ItemStack>,
}

impl BundleContents {
    /// An empty bundle.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the bundle holds nothing.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The weight one of `stack`'s items takes in a bundle.
    pub fn item_weight(stack: &ItemStack) -> i32 {
        if stack.name() == Some("minecraft:bundle") {
            return match stack.effective_component(ids::BUNDLE_CONTENTS) {
                Some(Component::BundleContents(contents)) => {
                    BUNDLE_INNER_WEIGHT + contents.weight()
                }
                _ => BUNDLE_INNER_WEIGHT,
            };
        }
        (BUNDLE_MAX_WEIGHT / stack.max_stack_size().max(1)).max(1)
    }

    /// The used weight, 0 to [`BUNDLE_MAX_WEIGHT`].
    pub fn weight(&self) -> i32 {
        self.items
            .iter()
            .map(|stack| Self::item_weight(stack).saturating_mul(stack.count.max(0)))
            .sum()
    }

    /// Whether `stack` fits at all (at least one item). Shulker boxes never do.
    pub fn can_insert(&self, stack: &ItemStack) -> bool {
        !stack.is_empty()
            && !stack
                .name()
                .is_some_and(|name| name.ends_with("shulker_box"))
            && self.weight() + Self::item_weight(stack) <= BUNDLE_MAX_WEIGHT
    }

    /// Vanilla `BundleContents.Mutable.tryInsert`: tops up matching stacks first, then adds a
    /// new stack while weight remains. Returns what did not fit; the whole stack when the
    /// bundle is full or the item may not be stored.
    pub fn insert(&mut self, mut stack: ItemStack) -> ItemStack {
        if !self.can_insert(&stack) {
            return stack;
        }
        let unit = Self::item_weight(&stack);
        let mut room = BUNDLE_MAX_WEIGHT - self.weight();
        for existing in &mut self.items {
            if stack.is_empty() || room < unit {
                break;
            }
            if existing.is_same_item_same_components(&stack) {
                let stack_room = (existing.max_stack_size() - existing.count).max(0);
                let moved = (room / unit).min(stack_room).min(stack.count);
                if moved > 0 {
                    existing.count += moved;
                    stack.count -= moved;
                    room -= moved * unit;
                }
            }
        }
        if !stack.is_empty() && room >= unit {
            let moved = (room / unit).min(stack.count);
            if moved > 0 {
                self.items.push(stack.copy_with_count(moved));
                stack.count -= moved;
            }
        }
        if stack.count <= 0 {
            ItemStack::empty()
        } else {
            stack
        }
    }

    /// Removes up to `amount` items from the stack at `index` (vanilla
    /// `BundleContents.Mutable.removeOne` generalized). Returns `None` for a bad index or an
    /// empty stack.
    pub fn extract(&mut self, index: usize, amount: i32) -> Option<ItemStack> {
        let stack = self.items.get_mut(index)?;
        let taken = stack.split(amount)?;
        if stack.is_empty() {
            self.items.remove(index);
        }
        Some(taken)
    }

    /// The save form: one item stack compound per stored stack.
    fn to_tag(&self) -> Result<Tag, Error> {
        let mut entries = Vec::with_capacity(self.items.len());
        for stack in &self.items {
            if stack.is_empty() {
                continue;
            }
            entries.push(stack.to_nbt()?);
        }
        Ok(Tag::List(if entries.is_empty() {
            List::End
        } else {
            List::Compound(entries)
        }))
    }

    /// Reads the save form. Any unreadable or empty entry makes the value stay opaque.
    fn from_tag(tag: &Tag) -> Option<Self> {
        let entries = match tag {
            Tag::List(List::End) => return Some(Self::new()),
            Tag::List(List::Compound(entries)) => entries,
            _ => return None,
        };
        let mut items = Vec::with_capacity(entries.len());
        for entry in entries {
            let stack = ItemStack::from_nbt(entry)
                .ok()
                .filter(|stack| !stack.is_empty())?;
            items.push(stack);
        }
        Some(Self { items })
    }
}

/// Vanilla's limit on the pages of a book.
pub const BOOK_MAX_PAGES: usize = 100;

/// A value with an optional chat-filtered variant (vanilla `Filterable`): the text a player
/// typed, and what a client with chat filtering sees instead.
#[derive(Clone, Debug, PartialEq)]
pub struct Filterable<T> {
    /// The unfiltered value.
    pub raw: T,
    /// The filtered value, when the server filtered it.
    pub filtered: Option<T>,
}

impl<T> Filterable<T> {
    /// A value that was not filtered.
    pub fn plain(raw: T) -> Self {
        Self {
            raw,
            filtered: None,
        }
    }

    /// The save form: the bare value, or `{raw, filtered}` when a filtered variant exists.
    fn to_tag(&self, convert: impl Fn(&T) -> Tag) -> Tag {
        match &self.filtered {
            None => convert(&self.raw),
            Some(filtered) => {
                let mut compound = Compound::new();
                compound.insert("raw".into(), convert(&self.raw));
                compound.insert("filtered".into(), convert(filtered));
                Tag::Compound(compound)
            }
        }
    }

    /// The `{raw, filtered}` form, used when a list mixes both shapes.
    fn to_full_tag(&self, convert: impl Fn(&T) -> Tag) -> Tag {
        let mut compound = Compound::new();
        compound.insert("raw".into(), convert(&self.raw));
        if let Some(filtered) = &self.filtered {
            compound.insert("filtered".into(), convert(filtered));
        }
        Tag::Compound(compound)
    }

    /// Reads either save form. A compound with a `raw` key is the full form, like vanilla's
    /// codec, which tries it before the bare value.
    fn from_tag(tag: &Tag, convert: impl Fn(&Tag) -> Option<T>) -> Option<Self> {
        if let Tag::Compound(compound) = tag
            && let Some(raw) = compound.get("raw")
        {
            return Some(Self {
                raw: convert(raw)?,
                filtered: match compound.get("filtered") {
                    Some(filtered) => Some(convert(filtered)?),
                    None => None,
                },
            });
        }
        convert(tag).map(Self::plain)
    }
}

/// `minecraft:writable_book_content`, the pages of a book and quill.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WritableBookContent {
    /// Page text, at most [`BOOK_MAX_PAGES`] pages of 1024 UTF-16 units each.
    pub pages: Vec<Filterable<String>>,
}

/// `minecraft:written_book_content`, a signed book.
#[derive(Clone, Debug, PartialEq)]
pub struct WrittenBookContent {
    /// The title, at most 32 UTF-16 units.
    pub title: Filterable<String>,
    /// The author's name.
    pub author: String,
    /// Copy generation: 0 original, 1 copy of the original, 2 copy of a copy, 3 tattered.
    pub generation: i32,
    /// Page text, at most [`BOOK_MAX_PAGES`] pages.
    pub pages: Vec<Filterable<Text>>,
    /// Whether the pages' selectors and scores were already resolved.
    pub resolved: bool,
}

/// Builds a homogeneous NBT list from tags, or `None` for the kinds this does not need.
fn homogeneous_list(tags: &[Tag]) -> Option<Tag> {
    if let Some(strings) = tags
        .iter()
        .map(|tag| match tag {
            Tag::String(value) => Some(value.clone()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
    {
        return Some(Tag::List(List::String(strings)));
    }
    tags.iter()
        .map(|tag| match tag {
            Tag::Compound(value) => Some(value.clone()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
        .map(|compounds| Tag::List(List::Compound(compounds)))
}

/// The elements of a list tag as individual tags, for the list kinds pages can use.
fn list_elements(tag: &Tag) -> Option<Vec<Tag>> {
    match tag {
        Tag::List(List::End) => Some(Vec::new()),
        Tag::List(List::String(values)) => Some(values.iter().cloned().map(Tag::String).collect()),
        Tag::List(List::Compound(values)) => {
            Some(values.iter().cloned().map(Tag::Compound).collect())
        }
        _ => None,
    }
}

/// Writes pages: the bare shape when no page needs the full form and the elements agree on a
/// list kind, otherwise every page in its `{raw, filtered}` form.
fn pages_to_tag<T>(pages: &[Filterable<T>], convert: impl Fn(&T) -> Tag) -> Tag {
    let bare: Vec<Tag> = pages.iter().map(|page| page.to_tag(&convert)).collect();
    if pages.iter().all(|page| page.filtered.is_none())
        && bare
            .iter()
            .all(|tag| !matches!(tag, Tag::Compound(c) if c.contains_key("raw")))
        && let Some(list) = homogeneous_list(&bare)
    {
        return list;
    }
    let full: Vec<Compound> = pages
        .iter()
        .filter_map(|page| match page.to_full_tag(&convert) {
            Tag::Compound(compound) => Some(compound),
            _ => None,
        })
        .collect();
    Tag::List(List::Compound(full))
}

fn pages_from_tag<T>(tag: &Tag, convert: impl Fn(&Tag) -> Option<T>) -> Option<Vec<Filterable<T>>> {
    let elements = list_elements(tag)?;
    if elements.len() > BOOK_MAX_PAGES {
        return None;
    }
    elements
        .iter()
        .map(|element| Filterable::from_tag(element, &convert))
        .collect()
}

fn string_of(tag: &Tag) -> Option<String> {
    match tag {
        Tag::String(value) => Some(value.clone()),
        _ => None,
    }
}

impl WritableBookContent {
    fn to_tag(&self) -> Tag {
        let mut compound = Compound::new();
        if !self.pages.is_empty() {
            compound.insert(
                "pages".into(),
                pages_to_tag(&self.pages, |page| Tag::String(page.clone())),
            );
        }
        Tag::Compound(compound)
    }

    fn from_tag(tag: &Tag) -> Option<Self> {
        let Tag::Compound(compound) = tag else {
            return None;
        };
        let pages = match compound.get("pages") {
            Some(pages) => pages_from_tag(pages, string_of)?,
            None => Vec::new(),
        };
        Some(Self { pages })
    }
}

impl WrittenBookContent {
    fn to_tag(&self) -> Tag {
        let mut compound = Compound::new();
        compound.insert(
            "title".into(),
            self.title.to_tag(|t| Tag::String(t.clone())),
        );
        compound.insert("author".into(), Tag::String(self.author.clone()));
        if self.generation != 0 {
            compound.insert("generation".into(), Tag::Int(self.generation));
        }
        if !self.pages.is_empty() {
            compound.insert("pages".into(), pages_to_tag(&self.pages, Text::to_nbt));
        }
        if self.resolved {
            compound.insert("resolved".into(), Tag::Byte(1));
        }
        Tag::Compound(compound)
    }

    fn from_tag(tag: &Tag) -> Option<Self> {
        let Tag::Compound(compound) = tag else {
            return None;
        };
        let generation = match compound.get("generation") {
            None => 0,
            Some(tag) => as_int(tag).filter(|g| (0..=3).contains(g))?,
        };
        let resolved = match compound.get("resolved") {
            None => false,
            Some(Tag::Byte(value)) => *value != 0,
            Some(_) => return None,
        };
        let pages = match compound.get("pages") {
            Some(pages) => pages_from_tag(pages, |tag| Text::from_nbt(tag).ok())?,
            None => Vec::new(),
        };
        Some(Self {
            title: Filterable::from_tag(compound.get("title")?, string_of)?,
            author: string_of(compound.get("author")?)?,
            generation,
            pages,
            resolved,
        })
    }
}

/// The save form of `minecraft:container`: the occupied slots as `{slot, item}`.
fn decode_block_predicates(tag: &Tag) -> Option<Vec<BlockPredicate>> {
    match tag {
        Tag::List(List::End) => Some(Vec::new()),
        Tag::List(List::Compound(entries)) => entries
            .iter()
            .map(|entry| BlockPredicate::from_nbt(&Tag::Compound(entry.clone())))
            .collect(),
        Tag::Compound(_) => BlockPredicate::from_nbt(tag).map(|pred| vec![pred]),
        _ => None,
    }
}

fn container_to_tag(slots: &[ItemStack]) -> Result<Tag, Error> {
    let mut entries = Vec::new();
    for (slot, stack) in slots.iter().enumerate() {
        if stack.is_empty() {
            continue;
        }
        let mut entry = Compound::new();
        entry.insert("slot".into(), Tag::Int(slot as i32));
        entry.insert("item".into(), Tag::Compound(stack.to_nbt()?));
        entries.push(entry);
    }
    Ok(Tag::List(if entries.is_empty() {
        List::End
    } else {
        List::Compound(entries)
    }))
}

/// Reads the save form of `minecraft:container` into a dense slot list ending at the last
/// occupied slot. A slot outside `0..256`, a repeated slot, a missing field or an unreadable
/// item makes the whole value invalid (`None`), so it stays opaque instead of losing items.
fn container_from_tag(tag: &Tag) -> Option<Vec<ItemStack>> {
    let entries = match tag {
        Tag::List(List::End) => return Some(Vec::new()),
        Tag::List(List::Compound(entries)) => entries,
        _ => return None,
    };
    let mut slots: Vec<ItemStack> = Vec::new();
    for entry in entries {
        let slot = usize::try_from(as_int(entry.get("slot")?)?)
            .ok()
            .filter(|slot| *slot < CONTAINER_MAX_SLOTS)?;
        let Some(Tag::Compound(item)) = entry.get("item") else {
            return None;
        };
        let stack = ItemStack::from_nbt(item).ok().filter(|s| !s.is_empty())?;
        if slots.len() <= slot {
            slots.resize(slot + 1, ItemStack::empty());
        }
        if !slots[slot].is_empty() {
            return None;
        }
        slots[slot] = stack;
    }
    Some(slots)
}

/// A typed data component value.
#[derive(Clone, Debug, PartialEq)]
pub enum Component {
    /// `minecraft:custom_name`, the stack's display name.
    CustomName(Text),
    /// `minecraft:item_name`, the item's own name.
    ItemName(Text),
    /// `minecraft:lore`.
    Lore(Vec<Text>),
    /// `minecraft:damage`, durability already used.
    Damage(i32),
    /// `minecraft:max_damage`, total durability.
    MaxDamage(i32),
    /// `minecraft:max_stack_size`, the stack limit of this stack.
    MaxStackSize(i32),
    /// `minecraft:unbreakable`; its presence makes the item unbreakable.
    Unbreakable,
    /// `minecraft:enchantment_glint_override`; forces enchantment glint on or off.
    EnchantmentGlintOverride(bool),
    /// `minecraft:glider`; enables elytra-style gliding when equipped.
    Glider,
    /// `minecraft:damage_resistant`; damage types this item is immune to when in fire/etc.
    DamageResistant(HolderSet),
    /// `minecraft:use_remainder`; item left behind after consuming/using this item.
    UseRemainder(Box<ItemStack>),
    /// `minecraft:repairable`; items that can repair this item on an anvil.
    Repairable(HolderSet),
    /// `minecraft:can_break`; blocks this item can break in adventure mode.
    CanBreak(Vec<BlockPredicate>),
    /// `minecraft:can_place_on`; blocks this item can be placed on in adventure mode.
    CanPlaceOn(Vec<BlockPredicate>),
    /// `minecraft:enchantments`.
    Enchantments(Enchantments),
    /// `minecraft:stored_enchantments`.
    StoredEnchantments(Enchantments),
    /// `minecraft:custom_model_data`.
    CustomModelData(CustomModelData),
    /// `minecraft:dyed_color`, packed RGB.
    DyedColor(i32),
    /// `minecraft:rarity`.
    Rarity(Rarity),
    /// `minecraft:repair_cost`.
    RepairCost(i32),
    /// `minecraft:map_id`, the id of the saved map a filled map shows (`data/map_<id>.dat`).
    MapId(i32),
    /// `minecraft:custom_data`, opaque to the server.
    CustomData(Compound),
    /// `minecraft:attribute_modifiers`.
    AttributeModifiers(Vec<AttributeModifier>),
    /// `minecraft:food`.
    Food(Food),
    /// `minecraft:potion_contents`.
    PotionContents(PotionContents),
    /// `minecraft:tool`.
    Tool(Tool),
    /// `minecraft:trim`, an armor trim from a smithing table.
    Trim(Trim),
    /// `minecraft:lodestone_tracker`, a compass bound to a lodestone.
    LodestoneTracker(LodestoneTracker),
    /// `minecraft:tooltip_display`, which hides tooltip lines.
    TooltipDisplay(TooltipDisplay),
    /// `minecraft:container`, the stacks inside a shulker box, chest or barrel item. The list is
    /// dense (empty slots are empty stacks) and ends at the last occupied slot when read from
    /// the save format; the network form keeps whatever length it arrived with.
    Container(Vec<ItemStack>),
    /// `minecraft:bundle_contents`, the stacks inside a bundle.
    BundleContents(BundleContents),
    /// `minecraft:writable_book_content`, the pages of a book and quill.
    WritableBookContent(WritableBookContent),
    /// `minecraft:written_book_content`, a signed book.
    WrittenBookContent(WrittenBookContent),
    /// A component without a typed variant, or a value whose NBT shape did not match its typed
    /// variant. Kept verbatim.
    Opaque(Tag),
    /// A component payload kept exactly as it appeared on the wire, produced by
    /// [`crate::wire`] for components without a typed variant. It has no NBT form; re-encoding
    /// it to the network writes the bytes verbatim.
    Raw(Vec<u8>),
}

impl Component {
    /// The component type this value belongs to, or `None` for [`Component::Opaque`], which
    /// takes its type id from the patch entry that stores it.
    pub fn type_id(&self) -> Option<u16> {
        Some(match self {
            Self::CustomName(_) => ids::CUSTOM_NAME,
            Self::ItemName(_) => ids::ITEM_NAME,
            Self::Lore(_) => ids::LORE,
            Self::Damage(_) => ids::DAMAGE,
            Self::MaxDamage(_) => ids::MAX_DAMAGE,
            Self::MaxStackSize(_) => ids::MAX_STACK_SIZE,
            Self::Unbreakable => ids::UNBREAKABLE,
            Self::EnchantmentGlintOverride(_) => ids::ENCHANTMENT_GLINT_OVERRIDE,
            Self::Glider => ids::GLIDER,
            Self::DamageResistant(_) => ids::DAMAGE_RESISTANT,
            Self::UseRemainder(_) => ids::USE_REMAINDER,
            Self::Repairable(_) => ids::REPAIRABLE,
            Self::CanBreak(_) => ids::CAN_BREAK,
            Self::CanPlaceOn(_) => ids::CAN_PLACE_ON,
            Self::Enchantments(_) => ids::ENCHANTMENTS,
            Self::StoredEnchantments(_) => ids::STORED_ENCHANTMENTS,
            Self::CustomModelData(_) => ids::CUSTOM_MODEL_DATA,
            Self::DyedColor(_) => ids::DYED_COLOR,
            Self::Rarity(_) => ids::RARITY,
            Self::RepairCost(_) => ids::REPAIR_COST,
            Self::MapId(_) => ids::MAP_ID,
            Self::CustomData(_) => ids::CUSTOM_DATA,
            Self::AttributeModifiers(_) => ids::ATTRIBUTE_MODIFIERS,
            Self::Food(_) => ids::FOOD,
            Self::PotionContents(_) => ids::POTION_CONTENTS,
            Self::Tool(_) => ids::TOOL,
            Self::Trim(_) => ids::TRIM,
            Self::LodestoneTracker(_) => ids::LODESTONE_TRACKER,
            Self::TooltipDisplay(_) => ids::TOOLTIP_DISPLAY,
            Self::Container(_) => ids::CONTAINER,
            Self::BundleContents(_) => ids::BUNDLE_CONTENTS,
            Self::WritableBookContent(_) => ids::WRITABLE_BOOK_CONTENT,
            Self::WrittenBookContent(_) => ids::WRITTEN_BOOK_CONTENT,
            Self::Opaque(_) | Self::Raw(_) => return None,
        })
    }

    /// The component name without its namespace, e.g. `"custom_name"`; useful for diagnostics.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::CustomName(_) => "custom_name",
            Self::ItemName(_) => "item_name",
            Self::Lore(_) => "lore",
            Self::Damage(_) => "damage",
            Self::MaxDamage(_) => "max_damage",
            Self::MaxStackSize(_) => "max_stack_size",
            Self::Unbreakable => "unbreakable",
            Self::EnchantmentGlintOverride(_) => "enchantment_glint_override",
            Self::Glider => "glider",
            Self::DamageResistant(_) => "damage_resistant",
            Self::UseRemainder(_) => "use_remainder",
            Self::Repairable(_) => "repairable",
            Self::CanBreak(_) => "can_break",
            Self::CanPlaceOn(_) => "can_place_on",
            Self::Enchantments(_) => "enchantments",
            Self::StoredEnchantments(_) => "stored_enchantments",
            Self::CustomModelData(_) => "custom_model_data",
            Self::DyedColor(_) => "dyed_color",
            Self::Rarity(_) => "rarity",
            Self::RepairCost(_) => "repair_cost",
            Self::MapId(_) => "map_id",
            Self::CustomData(_) => "custom_data",
            Self::AttributeModifiers(_) => "attribute_modifiers",
            Self::Food(_) => "food",
            Self::PotionContents(_) => "potion_contents",
            Self::Tool(_) => "tool",
            Self::Trim(_) => "trim",
            Self::LodestoneTracker(_) => "lodestone_tracker",
            Self::TooltipDisplay(_) => "tooltip_display",
            Self::Container(_) => "container",
            Self::BundleContents(_) => "bundle_contents",
            Self::WritableBookContent(_) => "writable_book_content",
            Self::WrittenBookContent(_) => "written_book_content",
            Self::Opaque(_) => "opaque",
            Self::Raw(_) => "raw",
        }
    }

    /// Encodes the value as NBT, in the shape the 26.3 item save format uses.
    ///
    /// # Errors
    ///
    /// [`Component::Raw`] only exists in the network form, so it cannot be saved as NBT.
    pub fn to_nbt(&self) -> Result<Tag, Error> {
        Ok(match self {
            Self::CustomName(text) | Self::ItemName(text) => text.to_nbt(),
            Self::Lore(lore) => text_list(lore),
            Self::Damage(value)
            | Self::MaxDamage(value)
            | Self::MaxStackSize(value)
            | Self::RepairCost(value)
            | Self::MapId(value)
            | Self::DyedColor(value) => Tag::Int(*value),
            Self::Unbreakable => Tag::Compound(Compound::new()),
            Self::Glider => Tag::Compound(Compound::new()),
            Self::DamageResistant(types) => {
                let mut compound = Compound::new();
                compound.insert("types".to_owned(), types.to_nbt());
                Tag::Compound(compound)
            }
            Self::UseRemainder(stack) => Tag::Compound(stack.to_nbt()?),
            Self::Repairable(items) => {
                let mut compound = Compound::new();
                compound.insert("items".to_owned(), items.to_nbt());
                Tag::Compound(compound)
            }
            Self::CanBreak(predicates) | Self::CanPlaceOn(predicates) => {
                Tag::List(if predicates.is_empty() {
                    List::End
                } else {
                    List::Compound(predicates.iter().map(BlockPredicate::to_nbt).collect())
                })
            }
            Self::EnchantmentGlintOverride(value) => Tag::Byte(i8::from(*value)),
            Self::Enchantments(enchantments) | Self::StoredEnchantments(enchantments) => {
                enchantments.to_tag()
            }
            Self::CustomModelData(data) => data.to_tag(),
            Self::Rarity(rarity) => Tag::String(rarity.name().to_owned()),
            Self::CustomData(data) => Tag::Compound(data.clone()),
            Self::AttributeModifiers(modifiers) => Tag::List(if modifiers.is_empty() {
                List::End
            } else {
                List::Compound(
                    modifiers
                        .iter()
                        .map(AttributeModifier::to_compound)
                        .collect(),
                )
            }),
            Self::Food(food) => Tag::Compound(food.to_compound()),
            Self::PotionContents(contents) => Tag::Compound(contents.to_compound()),
            Self::Tool(tool) => Tag::Compound(tool.to_compound()),
            Self::Trim(trim) => Tag::Compound(trim.to_compound()),
            Self::LodestoneTracker(tracker) => Tag::Compound(tracker.to_compound()),
            Self::TooltipDisplay(display) => display.to_tag()?,
            Self::Container(slots) => container_to_tag(slots)?,
            Self::BundleContents(contents) => contents.to_tag()?,
            Self::WritableBookContent(book) => book.to_tag(),
            Self::WrittenBookContent(book) => book.to_tag(),
            Self::Opaque(tag) => tag.clone(),
            Self::Raw(_) => return Err(Error::NoNbtForm("raw")),
        })
    }

    /// Decodes a component value of type `id`.
    ///
    /// Values that do not match the typed NBT shape are returned as [`Component::Opaque`]
    /// instead of failing, so unknown data survives a load/save cycle.
    pub fn from_nbt(id: u16, tag: &Tag) -> Self {
        let typed = match id {
            ids::CUSTOM_NAME => Text::from_nbt(tag).ok().map(Self::CustomName),
            ids::ITEM_NAME => Text::from_nbt(tag).ok().map(Self::ItemName),
            ids::LORE => decode_text_list(tag).map(Self::Lore),
            ids::DAMAGE => as_int(tag).map(Self::Damage),
            ids::MAX_DAMAGE => as_int(tag).map(Self::MaxDamage),
            ids::MAX_STACK_SIZE => as_int(tag).map(Self::MaxStackSize),
            ids::UNBREAKABLE => match tag {
                Tag::Compound(compound) if compound.is_empty() => Some(Self::Unbreakable),
                _ => None,
            },
            ids::GLIDER => match tag {
                Tag::Compound(compound) if compound.is_empty() => Some(Self::Glider),
                _ => None,
            },
            ids::DAMAGE_RESISTANT => match tag {
                Tag::Compound(compound) => compound
                    .get("types")
                    .and_then(HolderSet::from_nbt)
                    .map(Self::DamageResistant),
                _ => None,
            },
            ids::USE_REMAINDER => match tag {
                Tag::Compound(compound) => ItemStack::from_nbt(compound)
                    .ok()
                    .map(|stack| Self::UseRemainder(Box::new(stack))),
                _ => None,
            },
            ids::REPAIRABLE => match tag {
                Tag::Compound(compound) => compound
                    .get("items")
                    .and_then(HolderSet::from_nbt)
                    .map(Self::Repairable),
                _ => None,
            },
            ids::CAN_BREAK => decode_block_predicates(tag).map(Self::CanBreak),
            ids::CAN_PLACE_ON => decode_block_predicates(tag).map(Self::CanPlaceOn),
            ids::ENCHANTMENT_GLINT_OVERRIDE => match tag {
                Tag::Byte(value) => Some(Self::EnchantmentGlintOverride(*value != 0)),
                _ => None,
            },
            ids::ENCHANTMENTS => Enchantments::from_tag(tag).map(Self::Enchantments),
            ids::STORED_ENCHANTMENTS => Enchantments::from_tag(tag).map(Self::StoredEnchantments),
            ids::CUSTOM_MODEL_DATA => CustomModelData::from_tag(tag).map(Self::CustomModelData),
            ids::DYED_COLOR => as_int(tag).map(Self::DyedColor),
            ids::RARITY => as_str(tag).and_then(Rarity::from_name).map(Self::Rarity),
            ids::REPAIR_COST => as_int(tag).map(Self::RepairCost),
            ids::MAP_ID => as_int(tag).map(Self::MapId),
            ids::CUSTOM_DATA => match tag {
                Tag::Compound(compound) => Some(Self::CustomData(compound.clone())),
                _ => None,
            },
            ids::ATTRIBUTE_MODIFIERS => match tag {
                Tag::List(List::End) => Some(Self::AttributeModifiers(Vec::new())),
                Tag::List(List::Compound(values)) => values
                    .iter()
                    .map(AttributeModifier::from_compound)
                    .collect::<Option<Vec<_>>>()
                    .map(Self::AttributeModifiers),
                _ => None,
            },
            ids::FOOD => Food::from_tag(tag).map(Self::Food),
            ids::POTION_CONTENTS => PotionContents::from_tag(tag).map(Self::PotionContents),
            ids::TOOL => Tool::from_tag(tag).map(Self::Tool),
            ids::TRIM => Trim::from_tag(tag).map(Self::Trim),
            ids::LODESTONE_TRACKER => LodestoneTracker::from_tag(tag).map(Self::LodestoneTracker),
            ids::TOOLTIP_DISPLAY => TooltipDisplay::from_tag(tag).map(Self::TooltipDisplay),
            ids::CONTAINER => container_from_tag(tag).map(Self::Container),
            ids::BUNDLE_CONTENTS => BundleContents::from_tag(tag).map(Self::BundleContents),
            ids::WRITABLE_BOOK_CONTENT => {
                WritableBookContent::from_tag(tag).map(Self::WritableBookContent)
            }
            ids::WRITTEN_BOOK_CONTENT => {
                WrittenBookContent::from_tag(tag).map(Self::WrittenBookContent)
            }
            _ => None,
        };
        typed.unwrap_or_else(|| Self::Opaque(tag.clone()))
    }
}

/// A stack's changes to an item's default components.
///
/// Each entry either adds or overrides a component, or removes the component type from the
/// item's defaults. An empty patch means "the item's defaults". Entries are keyed by component
/// type id and iterate in id order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComponentPatch {
    values: BTreeMap<u16, Option<Component>>,
}

impl ComponentPatch {
    /// An empty patch.
    pub const fn new() -> Self {
        Self {
            values: BTreeMap::new(),
        }
    }

    /// Whether the patch has no entries.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Number of entries, added and removed alike.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// The value the patch adds or overrides for a component type.
    pub fn get(&self, id: u16) -> Option<&Component> {
        self.values.get(&id).and_then(Option::as_ref)
    }

    /// Whether the patch removes a component type from the item's defaults.
    pub fn is_removed(&self, id: u16) -> bool {
        matches!(self.values.get(&id), Some(None))
    }

    /// Whether the patch has an entry (added or removed) for a component type.
    pub fn contains(&self, id: u16) -> bool {
        self.values.contains_key(&id)
    }

    /// Adds or overrides a component, returning the value it replaced.
    ///
    /// Unlike [`ComponentPatch::insert_opaque`], a typed component is expected under its own
    /// type id; [`ComponentPatch::to_nbt`] rejects a mismatch.
    pub fn insert(&mut self, id: u16, component: Component) -> Option<Component> {
        self.values.insert(id, Some(component)).flatten()
    }

    /// Adds or overrides a component with raw NBT.
    pub fn insert_opaque(&mut self, id: u16, tag: Tag) -> Option<Component> {
        self.insert(id, Component::Opaque(tag))
    }

    /// Removes a component type from the item's defaults, returning the value it replaced.
    pub fn remove(&mut self, id: u16) -> Option<Component> {
        self.values.insert(id, None).flatten()
    }

    /// Forgets the patch entry for a component type, restoring the item's default.
    pub fn unset(&mut self, id: u16) -> Option<Component> {
        self.values.remove(&id).flatten()
    }

    /// The components the patch adds or overrides, in id order.
    pub fn added(&self) -> impl Iterator<Item = (u16, &Component)> + '_ {
        self.values
            .iter()
            .filter_map(|(id, value)| value.as_ref().map(|component| (*id, component)))
    }

    /// The component types the patch removes, in id order.
    pub fn removed(&self) -> impl Iterator<Item = u16> + '_ {
        self.values
            .iter()
            .filter_map(|(id, value)| value.is_none().then_some(*id))
    }

    /// Encodes the patch as the `components` compound of the item save format: added components
    /// under their name, removed ones under a `!`-prefixed name and an empty compound.
    pub fn to_nbt(&self) -> Result<Compound, Error> {
        let mut out = Compound::new();
        for (id, value) in &self.values {
            let name = component_name(*id).ok_or(Error::UnknownComponentId(*id))?;
            match value {
                Some(component) => {
                    if let Some(expected) = component.type_id()
                        && expected != *id
                    {
                        return Err(Error::ComponentIdMismatch {
                            kind: component.kind(),
                            id: *id,
                        });
                    }
                    out.insert(name.to_owned(), component.to_nbt()?);
                }
                None => {
                    out.insert(format!("!{name}"), Tag::Compound(Compound::new()));
                }
            }
        }
        Ok(out)
    }

    /// Decodes the `components` compound of the item save format.
    ///
    /// `!`-prefixed keys mark removed components; every other key decodes with
    /// [`Component::from_nbt`].
    pub fn from_nbt(components: &Compound) -> Result<Self, Error> {
        let mut patch = Self::new();
        for (raw_name, value) in components {
            let (name, removed) = match raw_name.strip_prefix('!') {
                Some(name) => (name, true),
                None => (raw_name.as_str(), false),
            };
            let id = component_id(name).ok_or_else(|| Error::UnknownComponent(name.to_owned()))?;
            if removed {
                patch.values.insert(id, None);
            } else {
                patch
                    .values
                    .insert(id, Some(Component::from_nbt(id, value)));
            }
        }
        Ok(patch)
    }
}

// ---------------------------------------------------------------------------------------------
// NBT helpers
// ---------------------------------------------------------------------------------------------

fn as_int(tag: &Tag) -> Option<i32> {
    match tag {
        Tag::Byte(value) => Some(i32::from(*value)),
        Tag::Short(value) => Some(i32::from(*value)),
        Tag::Int(value) => Some(*value),
        Tag::Long(value) => i32::try_from(*value).ok(),
        _ => None,
    }
}

fn as_f64(tag: &Tag) -> Option<f64> {
    match tag {
        Tag::Float(value) => Some(f64::from(*value)),
        Tag::Double(value) => Some(*value),
        Tag::Byte(value) => Some(f64::from(*value)),
        Tag::Short(value) => Some(f64::from(*value)),
        Tag::Int(value) => Some(f64::from(*value)),
        Tag::Long(value) => Some(*value as f64),
        _ => None,
    }
}

fn as_f32(tag: &Tag) -> Option<f32> {
    match tag {
        Tag::Float(value) => Some(*value),
        Tag::Double(value) => Some(*value as f32),
        Tag::Int(value) => Some(*value as f32),
        Tag::Long(value) => Some(*value as f32),
        _ => None,
    }
}

fn as_bool(tag: &Tag) -> Option<bool> {
    match tag {
        Tag::Byte(value) => Some(*value != 0),
        _ => None,
    }
}

fn as_str(tag: &Tag) -> Option<&str> {
    match tag {
        Tag::String(value) => Some(value),
        _ => None,
    }
}

fn float_list(tag: &Tag) -> Option<Vec<f32>> {
    match tag {
        Tag::List(List::End) => Some(Vec::new()),
        Tag::List(List::Float(values)) => Some(values.clone()),
        _ => None,
    }
}

fn flag_list(tag: &Tag) -> Option<Vec<bool>> {
    match tag {
        Tag::List(List::End) => Some(Vec::new()),
        Tag::List(List::Byte(values)) => Some(values.iter().map(|value| *value != 0).collect()),
        _ => None,
    }
}

fn string_list(tag: &Tag) -> Option<Vec<String>> {
    match tag {
        Tag::List(List::End) => Some(Vec::new()),
        Tag::List(List::String(values)) => Some(values.clone()),
        _ => None,
    }
}

fn int_list(tag: &Tag) -> Option<Vec<i32>> {
    match tag {
        Tag::List(List::End) => Some(Vec::new()),
        Tag::List(List::Int(values)) => Some(values.clone()),
        _ => None,
    }
}

/// Encodes a list of text components: a string list when every component is plain text,
/// otherwise a compound list.
fn text_list(lore: &[Text]) -> Tag {
    if lore.is_empty() {
        return Tag::List(List::End);
    }
    let mut plain = Vec::with_capacity(lore.len());
    let mut all_plain = true;
    for text in lore {
        match text.to_nbt() {
            Tag::String(value) => plain.push(value),
            Tag::Compound(_) => all_plain = false,
            _ => unreachable!("a text component encodes to a string or a compound"),
        }
    }
    if all_plain {
        Tag::List(List::String(plain))
    } else {
        Tag::List(List::Compound(lore.iter().map(text_compound).collect()))
    }
}

/// Decodes a list of text components, either form vanilla writes.
fn decode_text_list(tag: &Tag) -> Option<Vec<Text>> {
    match tag {
        Tag::List(List::End) => Some(Vec::new()),
        Tag::List(List::String(values)) => Some(
            values
                .iter()
                .map(|value| Text::text(value.clone()))
                .collect(),
        ),
        Tag::List(List::Compound(values)) => values
            .iter()
            .map(|value| Text::from_nbt(&Tag::Compound(value.clone())).ok())
            .collect(),
        _ => None,
    }
}

fn text_compound(text: &Text) -> Compound {
    match text.to_nbt() {
        Tag::Compound(compound) => compound,
        Tag::String(value) => {
            let mut compound = Compound::new();
            compound.insert("text".to_owned(), Tag::String(value));
            compound
        }
        _ => unreachable!("a text component encodes to a string or a compound"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_enchantments() -> Enchantments {
        let mut enchantments = Enchantments::new();
        enchantments.set("minecraft:sharpness", 5);
        enchantments.set("minecraft:unbreaking", 3);
        enchantments
    }

    #[test]
    fn component_type_lookup_accepts_both_spellings() {
        assert_eq!(COMPONENT_COUNT, 122);
        assert_eq!(component_id("custom_data"), Some(0));
        assert_eq!(component_id("minecraft:damage"), Some(3));
        assert_eq!(component_name(0), Some("minecraft:custom_data"));
        assert_eq!(component_name(3), Some("minecraft:damage"));
        assert_eq!(component_id("not_a_component"), None);
        assert_eq!(component_name(u16::MAX), None);
        assert_eq!(
            component_type("damage"),
            Some(ComponentType {
                name: "minecraft:damage",
                id: 3
            })
        );
        assert_eq!(component_types().count(), COMPONENT_COUNT);
        let mut ids: Vec<u16> = component_types().map(|component| component.id).collect();
        ids.sort_unstable();
        assert_eq!(
            ids,
            (0..COMPONENT_COUNT as u16).collect::<Vec<_>>(),
            "the generated tables cover every id exactly once"
        );
        let names: Vec<&str> = component_types().map(|component| component.name).collect();
        assert!(
            names.windows(2).all(|pair| pair[0] < pair[1]),
            "component types are sorted by name"
        );
    }

    #[test]
    fn typed_components_round_trip() {
        let mut model_data = CustomModelData::default();
        model_data.floats.push(1.5);
        model_data.flags.push(true);
        model_data.strings.push("variant".to_owned());
        model_data.colors.push(0x00ff_00ff);

        let mut tool = Tool::default();
        tool.rules.push(ToolRule {
            blocks: "minecraft:cobweb".to_owned(),
            speed: Some(15.0),
            correct_for_drops: Some(true),
        });
        tool.rules.push(ToolRule {
            blocks: "#minecraft:sword_efficient".to_owned(),
            speed: Some(1.5),
            correct_for_drops: None,
        });
        tool.default_mining_speed = 1.2;
        tool.damage_per_block = 2;
        tool.can_destroy_blocks_in_creative = true;

        let mut custom_data = Compound::new();
        custom_data.insert("owner".to_owned(), Tag::String("Steve".to_owned()));
        custom_data.insert("level".to_owned(), Tag::Int(12));

        let cases = [
            (
                ids::CUSTOM_NAME,
                Component::CustomName(Text::text("Sword").bold(true)),
            ),
            (
                ids::ITEM_NAME,
                Component::ItemName(Text::translatable("item.minecraft.diamond_sword")),
            ),
            (
                ids::LORE,
                Component::Lore(vec![
                    Text::text("plain"),
                    Text::text("styled").italic(true),
                    Text::translatable("item.minecraft.stone")
                        .color(ferrite_text::NamedColor::Gray),
                ]),
            ),
            (ids::DAMAGE, Component::Damage(7)),
            (ids::MAX_DAMAGE, Component::MaxDamage(1561)),
            (ids::MAX_STACK_SIZE, Component::MaxStackSize(16)),
            (ids::UNBREAKABLE, Component::Unbreakable),
            (ids::GLIDER, Component::Glider),
            (
                ids::DAMAGE_RESISTANT,
                Component::DamageResistant(HolderSet::Tag("minecraft:is_fire".to_owned())),
            ),
            (
                ids::USE_REMAINDER,
                Component::UseRemainder(Box::new(ItemStack::from_name("minecraft:bowl").unwrap())),
            ),
            (
                ids::REPAIRABLE,
                Component::Repairable(HolderSet::Ids(vec!["minecraft:iron_ingot".to_owned()])),
            ),
            (
                ids::CAN_BREAK,
                Component::CanBreak(vec![BlockPredicate {
                    blocks: Some(HolderSet::Ids(vec!["minecraft:stone".to_owned()])),
                }]),
            ),
            (
                ids::CAN_PLACE_ON,
                Component::CanPlaceOn(vec![BlockPredicate {
                    blocks: Some(HolderSet::Tag("minecraft:mineable/pickaxe".to_owned())),
                }]),
            ),
            (
                ids::ENCHANTMENT_GLINT_OVERRIDE,
                Component::EnchantmentGlintOverride(true),
            ),
            (
                ids::ENCHANTMENT_GLINT_OVERRIDE,
                Component::EnchantmentGlintOverride(false),
            ),
            (
                ids::ENCHANTMENTS,
                Component::Enchantments(sample_enchantments()),
            ),
            (
                ids::STORED_ENCHANTMENTS,
                Component::StoredEnchantments(sample_enchantments()),
            ),
            (
                ids::CUSTOM_MODEL_DATA,
                Component::CustomModelData(model_data),
            ),
            (ids::DYED_COLOR, Component::DyedColor(0x0012_3456)),
            (ids::RARITY, Component::Rarity(Rarity::Epic)),
            (ids::REPAIR_COST, Component::RepairCost(4)),
            (ids::MAP_ID, Component::MapId(12)),
            (ids::CUSTOM_DATA, Component::CustomData(custom_data)),
            (
                ids::ATTRIBUTE_MODIFIERS,
                Component::AttributeModifiers(vec![AttributeModifier {
                    attribute: "minecraft:attack_damage".to_owned(),
                    id: "minecraft:base_attack_damage".to_owned(),
                    amount: 6.0,
                    operation: AttributeOperation::AddValue,
                    slot: EquipmentSlot::Mainhand,
                }]),
            ),
            (
                ids::FOOD,
                Component::Food(Food {
                    nutrition: 4,
                    saturation: 2.4,
                    can_always_eat: true,
                }),
            ),
            (ids::TOOL, Component::Tool(tool)),
            (
                ids::LODESTONE_TRACKER,
                Component::LodestoneTracker(LodestoneTracker {
                    target: Some(("minecraft:the_nether".to_owned(), [-12, 70, 305])),
                    tracked: false,
                }),
            ),
            (
                ids::TRIM,
                Component::Trim(Trim {
                    material: "minecraft:amethyst".to_owned(),
                    pattern: "minecraft:bolt".to_owned(),
                }),
            ),
            (
                ids::TOOLTIP_DISPLAY,
                Component::TooltipDisplay(TooltipDisplay {
                    hide_tooltip: true,
                    hidden_components: vec![ids::ENCHANTMENTS, ids::ATTRIBUTE_MODIFIERS],
                }),
            ),
            (
                ids::BUNDLE_CONTENTS,
                Component::BundleContents(BundleContents {
                    items: vec![
                        ItemStack::from_name("minecraft:stone")
                            .unwrap()
                            .copy_with_count(32),
                        ItemStack::from_name("minecraft:ender_pearl").unwrap(),
                    ],
                }),
            ),
        ];

        for (id, component) in cases {
            assert_eq!(component.type_id(), Some(id), "{component:?}");
            let tag = component
                .to_nbt()
                .expect("typed components have an NBT form");
            assert_eq!(
                Component::from_nbt(id, &tag),
                component,
                "{tag:?} did not round-trip"
            );
        }
    }

    #[test]
    fn bundle_weight_follows_vanilla_stack_sizes() {
        let stone = ItemStack::from_name("minecraft:stone").unwrap();
        let pearl = ItemStack::from_name("minecraft:ender_pearl").unwrap();
        let sword = ItemStack::from_name("minecraft:diamond_sword").unwrap();
        assert_eq!(BundleContents::item_weight(&stone), 1);
        assert_eq!(BundleContents::item_weight(&pearl), 4);
        assert_eq!(BundleContents::item_weight(&sword), 64);

        let mut bundle = BundleContents::new();
        assert_eq!(bundle.weight(), 0);
        assert!(bundle.insert(stone.copy_with_count(48)).is_empty());
        assert_eq!(bundle.weight(), 48);
        // 48 + 4 pearls = 64, so exactly four fit. The rest is rejected.
        let leftover = bundle.insert(pearl.copy_with_count(8));
        assert_eq!(leftover.count, 4);
        assert_eq!(leftover.name(), Some("minecraft:ender_pearl"));
        assert_eq!(bundle.weight(), BUNDLE_MAX_WEIGHT);
        assert!(!bundle.can_insert(&stone));
        assert_eq!(bundle.insert(stone.copy_with_count(1)).count, 1);

        // A full bundle refuses an unstackable item outright.
        let mut full = BundleContents::new();
        full.insert(sword.clone());
        assert_eq!(full.weight(), 64);
        assert_eq!(full.insert(sword.clone()), sword);

        // A nested bundle weighs four plus its contents.
        let mut inner = ItemStack::from_name("minecraft:bundle").unwrap();
        inner.components.insert(
            ids::BUNDLE_CONTENTS,
            Component::BundleContents(BundleContents {
                items: vec![stone.copy_with_count(8)],
            }),
        );
        assert_eq!(BundleContents::item_weight(&inner), 12);
        // Shulker boxes never go inside a bundle.
        let shulker = ItemStack::from_name("minecraft:red_shulker_box").unwrap();
        assert!(!BundleContents::new().can_insert(&shulker));
        assert_eq!(BundleContents::new().insert(shulker.clone()), shulker);
    }

    #[test]
    fn bundle_insert_merges_and_extract_removes_empty_stacks() {
        let stone = ItemStack::from_name("minecraft:stone").unwrap();
        let mut bundle = BundleContents::new();
        bundle.insert(stone.copy_with_count(30));
        bundle.insert(stone.copy_with_count(20));
        assert_eq!(bundle.items.len(), 1, "matching stacks merge");
        assert_eq!(bundle.items[0].count, 50);
        assert_eq!(bundle.extract(0, 10).map(|stack| stack.count), Some(10));
        assert_eq!(bundle.items[0].count, 40);
        assert_eq!(bundle.extract(1, 1), None);
        assert_eq!(bundle.extract(0, 40).map(|stack| stack.count), Some(40));
        assert!(bundle.items.is_empty(), "an emptied stack is removed");
    }

    #[test]
    fn bundle_contents_round_trip_through_nbt() {
        let mut bundle = BundleContents::new();
        bundle.insert(
            ItemStack::from_name("minecraft:stone")
                .unwrap()
                .copy_with_count(12),
        );
        bundle.insert(ItemStack::from_name("minecraft:diamond_sword").unwrap());
        let component = Component::BundleContents(bundle);
        let tag = component.to_nbt().expect("bundle contents encode");
        assert!(matches!(tag, Tag::List(List::Compound(_))));
        assert_eq!(Component::from_nbt(ids::BUNDLE_CONTENTS, &tag), component);
        // An empty bundle is the empty list and stays typed.
        let empty = Component::BundleContents(BundleContents::new());
        let tag = empty.to_nbt().unwrap();
        assert_eq!(Component::from_nbt(ids::BUNDLE_CONTENTS, &tag), empty);
    }

    #[test]
    fn values_of_another_shape_stay_opaque() {
        let damage = Tag::String("not an int".to_owned());
        assert_eq!(
            Component::from_nbt(ids::DAMAGE, &damage),
            Component::Opaque(damage.clone())
        );
        assert_eq!(
            Component::from_nbt(u16::MAX, &damage),
            Component::Opaque(damage.clone())
        );

        let unbreakable = Tag::Compound(Compound::new());
        assert_eq!(
            Component::from_nbt(ids::UNBREAKABLE, &unbreakable),
            Component::Unbreakable
        );

        let mut with_tooltip = Compound::new();
        with_tooltip.insert("show_in_tooltip".to_owned(), Tag::Byte(1));
        assert_eq!(
            Component::from_nbt(ids::UNBREAKABLE, &Tag::Compound(with_tooltip.clone())),
            Component::Opaque(Tag::Compound(with_tooltip))
        );
    }

    #[test]
    fn patch_encodes_added_and_removed_components() {
        let mut custom_data = Compound::new();
        custom_data.insert("shop".to_owned(), Tag::String("spawn".to_owned()));
        custom_data.insert("price".to_owned(), Tag::Int(5));

        let mut patch = ComponentPatch::new();
        patch.insert(ids::CUSTOM_DATA, Component::CustomData(custom_data.clone()));
        patch.insert(ids::DAMAGE, Component::Damage(3));
        patch.remove(ids::FOOD);

        let compound = patch.to_nbt().expect("patch encodes");
        assert_eq!(
            compound.get("minecraft:custom_data"),
            Some(&Tag::Compound(custom_data))
        );
        assert_eq!(compound.get("minecraft:damage"), Some(&Tag::Int(3)));
        assert_eq!(
            compound.get("!minecraft:food"),
            Some(&Tag::Compound(Compound::new()))
        );
        assert_eq!(ComponentPatch::from_nbt(&compound).unwrap(), patch);
        assert_eq!(patch.added().count(), 2);
        assert_eq!(patch.removed().collect::<Vec<_>>(), vec![ids::FOOD]);
        assert!(patch.is_removed(ids::FOOD));
        assert!(!patch.contains(ids::MAX_DAMAGE));
        assert_eq!(patch.len(), 3);
    }

    #[test]
    fn patch_rejects_a_mismatched_component_id() {
        let mut patch = ComponentPatch::new();
        patch.insert(ids::MAX_DAMAGE, Component::Damage(1));
        assert!(matches!(
            patch.to_nbt(),
            Err(Error::ComponentIdMismatch {
                kind: "damage",
                id
            }) if id == ids::MAX_DAMAGE
        ));
    }

    #[test]
    fn patch_rejects_unknown_component_names() {
        let mut components = Compound::new();
        components.insert("minecraft:not_a_component".to_owned(), Tag::Int(1));
        assert!(matches!(
            ComponentPatch::from_nbt(&components),
            Err(Error::UnknownComponent(name)) if name == "minecraft:not_a_component"
        ));
    }

    #[test]
    fn patch_unset_restores_the_default() {
        let mut patch = ComponentPatch::new();
        patch.insert(ids::DAMAGE, Component::Damage(2));
        assert_eq!(patch.unset(ids::DAMAGE), Some(Component::Damage(2)));
        assert!(patch.is_empty());

        patch.remove(ids::MAX_DAMAGE);
        assert!(patch.is_removed(ids::MAX_DAMAGE));
        assert_eq!(patch.remove(ids::MAX_DAMAGE), None);
        assert_eq!(patch.unset(ids::MAX_DAMAGE), None);
        assert!(patch.is_empty());
    }
}
