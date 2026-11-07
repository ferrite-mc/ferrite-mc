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
            Self::Enchantments(_) => ids::ENCHANTMENTS,
            Self::StoredEnchantments(_) => ids::STORED_ENCHANTMENTS,
            Self::CustomModelData(_) => ids::CUSTOM_MODEL_DATA,
            Self::DyedColor(_) => ids::DYED_COLOR,
            Self::Rarity(_) => ids::RARITY,
            Self::RepairCost(_) => ids::REPAIR_COST,
            Self::CustomData(_) => ids::CUSTOM_DATA,
            Self::AttributeModifiers(_) => ids::ATTRIBUTE_MODIFIERS,
            Self::Food(_) => ids::FOOD,
            Self::PotionContents(_) => ids::POTION_CONTENTS,
            Self::Tool(_) => ids::TOOL,
            Self::Trim(_) => ids::TRIM,
            Self::LodestoneTracker(_) => ids::LODESTONE_TRACKER,
            Self::TooltipDisplay(_) => ids::TOOLTIP_DISPLAY,
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
            Self::Enchantments(_) => "enchantments",
            Self::StoredEnchantments(_) => "stored_enchantments",
            Self::CustomModelData(_) => "custom_model_data",
            Self::DyedColor(_) => "dyed_color",
            Self::Rarity(_) => "rarity",
            Self::RepairCost(_) => "repair_cost",
            Self::CustomData(_) => "custom_data",
            Self::AttributeModifiers(_) => "attribute_modifiers",
            Self::Food(_) => "food",
            Self::PotionContents(_) => "potion_contents",
            Self::Tool(_) => "tool",
            Self::Trim(_) => "trim",
            Self::LodestoneTracker(_) => "lodestone_tracker",
            Self::TooltipDisplay(_) => "tooltip_display",
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
            | Self::DyedColor(value) => Tag::Int(*value),
            Self::Unbreakable => Tag::Compound(Compound::new()),
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
            ids::ENCHANTMENTS => Enchantments::from_tag(tag).map(Self::Enchantments),
            ids::STORED_ENCHANTMENTS => Enchantments::from_tag(tag).map(Self::StoredEnchantments),
            ids::CUSTOM_MODEL_DATA => CustomModelData::from_tag(tag).map(Self::CustomModelData),
            ids::DYED_COLOR => as_int(tag).map(Self::DyedColor),
            ids::RARITY => as_str(tag).and_then(Rarity::from_name).map(Self::Rarity),
            ids::REPAIR_COST => as_int(tag).map(Self::RepairCost),
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
