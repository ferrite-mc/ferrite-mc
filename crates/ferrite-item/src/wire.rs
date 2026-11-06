//! The 26.3 network `Slot` codec (protocol 777).
//!
//! A slot on the wire is a VarInt count followed, when the count is positive, by the item
//! protocol id and the component patch:
//!
//! ```text
//! count (VarInt; <= 0 means empty, nothing follows)
//! item id (VarInt)
//! n_added (VarInt), n_removed (VarInt)
//! n_added   x (component type id (VarInt), component payload)
//! n_removed x (component type id (VarInt))
//! ```
//!
//! Every one of the 122 component types is parsed structurally, so a payload whose value has
//! no typed [`Component`] variant, or whose value uses a shape the typed variant cannot hold,
//! is kept as [`Component::Raw`] and re-encodes byte for byte. Typed payloads re-encode
//! canonically (component entries in id order, enchantments in name order); see
//! [`ItemStack::encode_network`].
//!
//! # Layout sources
//!
//! The layouts follow `vanilla/docs/Slot_data_notes.md` except where the 26.3 server classes in
//! `vanilla/26.3/lib/server-26.3.jar` disagree; those points are noted on the individual
//! layout functions below. Notably:
//!
//! * `attribute_modifiers` entries end with the `ItemAttributeModifiers.Display` discriminator
//!   (`0` default, `1` hidden, `2` override text + component), and slots use the 11-value
//!   `EquipmentSlotGroup` ids.
//! * `use_remainder`, `charged_projectiles`, `bundle_contents`, `container` and
//!   `sulfur_cube_content` carry `ItemStackTemplate`s (item id, count, patch), not `Slot`s.
//! * `trim`, `provides_trim_material`, `instrument`, `jukebox_playable` and `painting/variant`
//!   use `Holder` codecs with inline data; the inline shapes come from the server classes.
//! * The notes' unverified `TrimMaterial` is the 26.3 shape (palette id and description), and
//!   the hashed slot's CRC32C values are treated as opaque.
//!
//! Hashed slots (serverbound `Click Container`) are exposed as [`HashedSlot`], with the hashes
//! kept exactly as received.

use ferrite_nbt::Tag;
use ferrite_text::Component as Text;

use crate::component::{
    AttributeModifier, AttributeOperation, Component, ComponentPatch, CustomModelData,
    Enchantments, EquipmentSlot, Food, Rarity, Tool, ToolRule,
};
use crate::component_ids as ids;
use crate::{COMPONENT_COUNT, ItemStack, MAX_COUNT, item_count};

/// Errors produced by the network slot codec.
#[derive(Debug, thiserror::Error)]
pub enum WireError {
    /// The buffer ended in the middle of a value.
    #[error("unexpected end of item stack data")]
    UnexpectedEof,
    /// A VarInt used more than five bytes.
    #[error("VarInt is longer than five bytes")]
    VarIntTooLong,
    /// A boolean byte was neither 0 nor 1.
    #[error("invalid boolean byte {0:#04x}")]
    InvalidBoolean(u8),
    /// A string was not valid UTF-8.
    #[error("string is not valid UTF-8")]
    InvalidString,
    /// A string exceeded its protocol length limit.
    #[error("string length {0} exceeds the limit of {1}")]
    StringTooLong(usize, usize),
    /// A prefixed array had a negative length.
    #[error("negative array length {0}")]
    NegativeLength(i32),
    /// A prefixed array was longer than the layout allows.
    #[error("array length {0} exceeds the limit of {1}")]
    ArrayTooLong(usize, usize),
    /// Extra bytes remained after decoding a slot.
    #[error("{0} trailing bytes after the item stack")]
    TrailingBytes(usize),
    /// An item protocol id is not in the registry.
    #[error("invalid item protocol id {0}")]
    InvalidItemId(i32),
    /// A component type id is not in the registry.
    #[error("invalid data component type id {0}")]
    InvalidComponentId(i32),
    /// A slot count is not valid.
    #[error("invalid slot count {0}")]
    InvalidCount(i32),
    /// An NBT payload of a component did not decode.
    #[error("invalid component NBT")]
    Nbt(#[source] ferrite_nbt::binary::Error),
    /// A text component of a payload did not decode.
    #[error("invalid text component")]
    Text(#[source] ferrite_text::Error),
    /// An NBT tag was nested deeper than the limit.
    #[error("NBT nested deeper than {0} levels")]
    NbtDepth(usize),
    /// An NBT tag type is not known.
    #[error("unknown NBT tag type {0}")]
    NbtTag(u8),
    /// A component value has no network form ([`Component::Opaque`] is save-format only).
    #[error("component {0} has no network form")]
    NotRepresentable(&'static str),
    /// A name could not be resolved to a registry id.
    #[error("unknown {registry} entry {name:?}")]
    UnknownRegistry {
        registry: &'static str,
        name: String,
    },
    /// A typed component was stored under another component type's id.
    #[error("component {kind} cannot be stored under data component type id {id}")]
    ComponentIdMismatch { kind: &'static str, id: u16 },
    /// A nested component payload exceeded the nesting limit.
    #[error("component payloads are nested too deeply")]
    Depth,
    /// A component payload did not fit its layout.
    #[error("invalid payload for component {0}")]
    InvalidPayload(&'static str),
    /// A component type id has no layout in this crate yet.
    #[error("unsupported data component type id {0}")]
    UnsupportedComponent(u16),
}

/// Maximum nesting of component payloads (a container of a container of ...).
const MAX_DEPTH: u8 = 32;
/// Maximum entries of a component list.
const MAX_LIST: usize = 4096;
/// Maximum entries of a hashed-slot hash list.
const MAX_HASHES: usize = 4096;

/// A hashed slot, as sent in the serverbound `Click Container` packet.
///
/// The component hashes use an undocumented CRC32C variant; this crate keeps them exactly as
/// received instead of computing them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HashedSlot {
    /// Item protocol id.
    pub item: u16,
    /// Stack count.
    pub count: i32,
    /// Added components with their opaque hashes.
    pub added: Vec<(u16, i32)>,
    /// Removed component type ids.
    pub removed: Vec<u16>,
}

impl HashedSlot {
    /// Encodes a present hashed slot, including its leading `has_item` boolean.
    pub fn encode_network(&self) -> Result<Vec<u8>, WireError> {
        let mut out = vec![1];
        write_varint(i32::from(self.item), &mut out);
        write_varint(self.count, &mut out);
        write_varint(len_i32(self.added.len())?, &mut out);
        write_varint(len_i32(self.removed.len())?, &mut out);
        for (id, hash) in &self.added {
            write_varint(i32::from(*id), &mut out);
            out.extend_from_slice(&hash.to_be_bytes());
        }
        for id in &self.removed {
            write_varint(i32::from(*id), &mut out);
        }
        Ok(out)
    }

    /// Decodes a hashed slot, returning `None` for an absent one.
    pub fn decode_network(bytes: &[u8]) -> Result<Option<Self>, WireError> {
        let mut reader = Reader::new(bytes);
        let slot = decode_hashed_slot(&mut reader)?;
        if reader.remaining() != 0 {
            return Err(WireError::TrailingBytes(reader.remaining()));
        }
        Ok(slot)
    }
}

/// Encodes the absent hashed slot (`has_item = false`).
pub fn encode_empty_hashed_slot() -> Vec<u8> {
    vec![0]
}

fn decode_hashed_slot(reader: &mut Reader<'_>) -> Result<Option<HashedSlot>, WireError> {
    if !reader.bool()? {
        return Ok(None);
    }
    decode_hashed_body(reader).map(Some)
}

/// A present hashed slot after its `has_item` boolean.
fn decode_hashed_body(reader: &mut Reader<'_>) -> Result<HashedSlot, WireError> {
    let item = reader.item_id()?;
    let count = reader.count()?;
    let added_len = reader.count_len(MAX_HASHES)?;
    let removed_len = reader.count_len(MAX_HASHES)?;
    let mut added = Vec::with_capacity(added_len.min(256));
    for _ in 0..added_len {
        let id = reader.component_id()?;
        added.push((id, reader.i32()?));
    }
    let mut removed = Vec::with_capacity(removed_len.min(256));
    for _ in 0..removed_len {
        removed.push(reader.component_id()?);
    }
    Ok(HashedSlot {
        item,
        count,
        added,
        removed,
    })
}

impl ItemStack {
    /// Encodes the stack as a network slot.
    ///
    /// An empty stack is a single zero byte. Added components are written in component type id
    /// order, and typed components use their canonical payload, so re-encoding a stack decoded
    /// from this crate's output is byte exact. Payloads received for components without a typed
    /// variant are kept verbatim as [`Component::Raw`].
    pub fn encode_network(&self) -> Result<Vec<u8>, WireError> {
        let mut out = Vec::new();
        self.encode_network_into(&mut out)?;
        Ok(out)
    }

    /// Appends the stack's network slot to `out`.
    pub fn encode_network_into(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        if self.is_empty() {
            write_varint(0, out);
            return Ok(());
        }
        if !(1..=MAX_COUNT).contains(&self.count) {
            return Err(WireError::InvalidCount(self.count));
        }
        if usize::from(self.item) >= item_count() {
            return Err(WireError::InvalidItemId(i32::from(self.item)));
        }
        let mut added = Vec::new();
        for (id, component) in self.components.added() {
            if let Some(expected) = component.type_id()
                && expected != id
            {
                return Err(WireError::ComponentIdMismatch {
                    kind: component.kind(),
                    id,
                });
            }
            added.push((id, encode_component(component)?));
        }
        let removed: Vec<u16> = self.components.removed().collect();

        write_varint(self.count, out);
        write_varint(i32::from(self.item), out);
        write_varint(len_i32(added.len())?, out);
        write_varint(len_i32(removed.len())?, out);
        for (id, payload) in added {
            write_varint(i32::from(id), out);
            out.extend_from_slice(&payload);
        }
        for id in removed {
            write_varint(i32::from(id), out);
        }
        Ok(())
    }

    /// Decodes a network slot, requiring the buffer to end exactly at the slot.
    pub fn decode_network(bytes: &[u8]) -> Result<Self, WireError> {
        let (stack, consumed) = Self::decode_network_prefix(bytes)?;
        if consumed != bytes.len() {
            return Err(WireError::TrailingBytes(bytes.len() - consumed));
        }
        Ok(stack)
    }

    /// Decodes one network slot from the start of `bytes`, returning it and its length.
    pub fn decode_network_prefix(bytes: &[u8]) -> Result<(Self, usize), WireError> {
        let mut reader = Reader::new(bytes);
        let stack = decode_slot(&mut reader)?;
        Ok((stack, reader.pos))
    }
}

fn decode_slot(reader: &mut Reader<'_>) -> Result<ItemStack, WireError> {
    let count = reader.varint()?;
    if count <= 0 {
        return Ok(ItemStack::empty());
    }
    if count > MAX_COUNT {
        return Err(WireError::InvalidCount(count));
    }
    let item = reader.item_id()?;
    let components = decode_patch(reader, 0)?;
    Ok(ItemStack {
        item,
        count,
        components,
    })
}

fn decode_patch(reader: &mut Reader<'_>, depth: u8) -> Result<ComponentPatch, WireError> {
    if depth > MAX_DEPTH {
        return Err(WireError::Depth);
    }
    let added = reader.count_len(MAX_LIST)?;
    let removed = reader.count_len(MAX_LIST)?;
    let mut patch = ComponentPatch::new();
    for _ in 0..added {
        let id = reader.component_id()?;
        let payload = reader.component_payload(id, depth)?;
        patch.insert(id, decode_component_payload(id, payload)?);
    }
    for _ in 0..removed {
        patch.remove(reader.component_id()?);
    }
    Ok(patch)
}

/// Decodes a payload into a typed component, falling back to [`Component::Raw`].
///
/// A typed value is only accepted when re-encoding it reproduces the received payload exactly.
/// Values that a typed variant would normalize (an unusual text shape, a non-canonical list
/// order) therefore stay raw and still round-trip byte for byte.
fn decode_component_payload(id: u16, payload: &[u8]) -> Result<Component, WireError> {
    if let Some(component) = decode_typed(id, payload)
        && encode_component(&component).is_ok_and(|encoded| encoded == payload)
    {
        return Ok(component);
    }
    Ok(Component::Raw(payload.to_vec()))
}

// ---------------------------------------------------------------------------------------------
// Typed payloads
// ---------------------------------------------------------------------------------------------

/// Tries to decode a payload into one of the crate's typed components.
///
/// Returns `None` when the value cannot be represented exactly (unknown registry entries, a
/// display override, a multi-block tool set, ...), which makes the caller keep the bytes raw.
fn decode_typed(id: u16, payload: &[u8]) -> Option<Component> {
    let mut reader = Reader::new(payload);
    let component = match id {
        ids::CUSTOM_NAME => Component::CustomName(reader.text().ok()?),
        ids::ITEM_NAME => Component::ItemName(reader.text().ok()?),
        ids::LORE => Component::Lore(reader.text_list(MAX_LIST).ok()?),
        ids::DAMAGE => Component::Damage(reader.varint().ok()?),
        ids::MAX_DAMAGE => Component::MaxDamage(reader.varint().ok()?),
        ids::MAX_STACK_SIZE => Component::MaxStackSize(reader.varint().ok()?),
        ids::UNBREAKABLE => Component::Unbreakable,
        ids::ENCHANTMENTS => Component::Enchantments(decode_enchantments(&mut reader)?),
        ids::STORED_ENCHANTMENTS => {
            Component::StoredEnchantments(decode_enchantments(&mut reader)?)
        }
        ids::CUSTOM_MODEL_DATA => Component::CustomModelData(CustomModelData {
            floats: reader.prefixed(MAX_LIST, Reader::f32).ok()?,
            flags: reader.prefixed(MAX_LIST, Reader::bool).ok()?,
            strings: reader
                .prefixed(MAX_LIST, |reader| reader.string(32767).map(str::to_owned))
                .ok()?,
            colors: reader.prefixed(MAX_LIST, Reader::i32).ok()?,
        }),
        ids::DYED_COLOR => Component::DyedColor(reader.i32().ok()?),
        ids::RARITY => {
            Component::Rarity(*Rarity::ALL.get(usize::try_from(reader.varint().ok()?).ok()?)?)
        }
        ids::REPAIR_COST => Component::RepairCost(reader.varint().ok()?),
        ids::CUSTOM_DATA => match reader.nbt_tag().ok()? {
            Tag::Compound(compound) => Component::CustomData(compound),
            _ => return None,
        },
        ids::ATTRIBUTE_MODIFIERS => {
            Component::AttributeModifiers(decode_attribute_modifiers(&mut reader)?)
        }
        ids::FOOD => Component::Food(Food {
            nutrition: reader.varint().ok()?,
            saturation: reader.f32().ok()?,
            can_always_eat: reader.bool().ok()?,
        }),
        ids::POTION_CONTENTS => {
            let potion = reader
                .optional(Reader::varint)
                .ok()?
                .and_then(|id| u16::try_from(id).ok())
                .and_then(registry_ids::potion_name)
                .map(str::to_owned);
            let custom_color = reader.optional(Reader::i32).ok()?;
            let count = reader.count_len(MAX_LIST).ok()?;
            let mut custom_effects = Vec::new();
            for _ in 0..count {
                custom_effects.push(decode_potion_effect(&mut reader)?);
            }
            let custom_name = reader
                .optional(|reader| reader.string(32767).map(str::to_owned))
                .ok()?;
            Component::PotionContents(crate::component::PotionContents {
                potion,
                custom_color,
                custom_effects,
                custom_name,
            })
        }
        ids::TOOL => Component::Tool(decode_tool(&mut reader)?),
        _ => return None,
    };
    (reader.remaining() == 0).then_some(component)
}

/// One entry of `PotionContents.custom_effects`. A hidden (nested) effect has no NBT form, so
/// it makes the whole component stay [`Component::Raw`].
fn decode_potion_effect(reader: &mut Reader<'_>) -> Option<crate::component::PotionEffect> {
    let effect =
        registry_ids::mob_effect_name(u16::try_from(reader.varint().ok()?).ok()?)?.to_owned();
    let amplifier = reader.varint().ok()?;
    let duration = reader.varint().ok()?;
    let ambient = reader.bool().ok()?;
    let visible = reader.bool().ok()?;
    let show_icon = reader.bool().ok()?;
    if reader.bool().ok()? {
        return None;
    }
    Some(crate::component::PotionEffect {
        effect,
        amplifier,
        duration,
        ambient,
        visible,
        show_icon,
    })
}

fn decode_enchantments(reader: &mut Reader<'_>) -> Option<Enchantments> {
    let count = reader.count_len(MAX_LIST).ok()?;
    let mut enchantments = Enchantments::new();
    let mut previous: Option<&'static str> = None;
    for _ in 0..count {
        let enchantment = registry_ids::enchantment_name(reader.varint().ok()?)?;
        let level = reader.varint().ok()?;
        if previous.is_some_and(|previous| previous >= enchantment) {
            return None; // duplicates or unsorted entries would not re-encode identically
        }
        previous = Some(enchantment);
        enchantments.set(enchantment, level);
    }
    Some(enchantments)
}

fn decode_attribute_modifiers(reader: &mut Reader<'_>) -> Option<Vec<AttributeModifier>> {
    let count = reader.count_len(MAX_LIST).ok()?;
    let mut modifiers = Vec::with_capacity(count.min(256));
    for _ in 0..count {
        let attribute = registry_ids::attribute_name(u16::try_from(reader.varint().ok()?).ok()?)?;
        let id = reader.string(32767).ok()?.to_owned();
        let amount = reader.f64().ok()?;
        let operation =
            *AttributeOperation::ALL.get(usize::try_from(reader.varint().ok()?).ok()?)?;
        let slot = *EquipmentSlot::ALL.get(usize::try_from(reader.varint().ok()?).ok()?)?;
        // The display discriminator is 0 (default), 1 (hidden) or 2 (override text); only the
        // default form fits the typed modifier.
        if reader.varint().ok()? != 0 {
            return None;
        }
        modifiers.push(AttributeModifier {
            attribute: attribute.to_owned(),
            id,
            amount,
            operation,
            slot,
        });
    }
    Some(modifiers)
}

fn decode_tool(reader: &mut Reader<'_>) -> Option<Tool> {
    let count = reader.count_len(MAX_LIST).ok()?;
    let mut rules = Vec::with_capacity(count.min(256));
    for _ in 0..count {
        rules.push(ToolRule {
            blocks: read_id_set_string(reader).ok()?,
            speed: reader.optional(Reader::f32).ok()?,
            correct_for_drops: reader.optional(Reader::bool).ok()?,
        });
    }
    Some(Tool {
        rules,
        default_mining_speed: reader.f32().ok()?,
        damage_per_block: reader.varint().ok()?,
        can_destroy_blocks_in_creative: reader.bool().ok()?,
    })
}

fn encode_component(component: &Component) -> Result<Vec<u8>, WireError> {
    let mut out = Vec::new();
    match component {
        Component::CustomName(text) | Component::ItemName(text) => {
            out.extend_from_slice(&nbt_bytes(&text.to_nbt())?);
        }
        Component::Lore(lore) => {
            write_count(lore.len(), &mut out)?;
            for text in lore {
                out.extend_from_slice(&nbt_bytes(&text.to_nbt())?);
            }
        }
        Component::Damage(value)
        | Component::MaxDamage(value)
        | Component::MaxStackSize(value)
        | Component::RepairCost(value) => write_varint(*value, &mut out),
        Component::Unbreakable => {}
        Component::Enchantments(enchantments) | Component::StoredEnchantments(enchantments) => {
            write_count(enchantments.levels.len(), &mut out)?;
            for (enchantment, level) in &enchantments.levels {
                let id = registry_ids::enchantment_id(enchantment).ok_or_else(|| {
                    WireError::UnknownRegistry {
                        registry: "enchantment",
                        name: enchantment.clone(),
                    }
                })?;
                write_varint(id, &mut out);
                write_varint(*level, &mut out);
            }
        }
        Component::CustomModelData(data) => {
            write_count(data.floats.len(), &mut out)?;
            for value in &data.floats {
                out.extend_from_slice(&value.to_be_bytes());
            }
            write_count(data.flags.len(), &mut out)?;
            for value in &data.flags {
                out.push(u8::from(*value));
            }
            write_count(data.strings.len(), &mut out)?;
            for value in &data.strings {
                write_string(value, &mut out)?;
            }
            write_count(data.colors.len(), &mut out)?;
            for value in &data.colors {
                out.extend_from_slice(&value.to_be_bytes());
            }
        }
        Component::DyedColor(value) => out.extend_from_slice(&value.to_be_bytes()),
        Component::Rarity(rarity) => {
            let id = Rarity::ALL
                .iter()
                .position(|candidate| candidate == rarity)
                .expect("every Rarity is in Rarity::ALL");
            write_varint(len_i32(id)?, &mut out);
        }
        Component::CustomData(compound) => {
            out.extend_from_slice(&nbt_bytes(&Tag::Compound(compound.clone()))?);
        }
        Component::AttributeModifiers(modifiers) => {
            write_count(modifiers.len(), &mut out)?;
            for modifier in modifiers {
                let attribute =
                    registry_ids::attribute_id(&modifier.attribute).ok_or_else(|| {
                        WireError::UnknownRegistry {
                            registry: "attribute",
                            name: modifier.attribute.clone(),
                        }
                    })?;
                write_varint(i32::from(attribute), &mut out);
                write_string(&modifier.id, &mut out)?;
                out.extend_from_slice(&modifier.amount.to_be_bytes());
                write_varint(operation_id(modifier.operation), &mut out);
                write_varint(slot_id(modifier.slot), &mut out);
                write_varint(0, &mut out); // Display: default
            }
        }
        Component::Food(food) => {
            write_varint(food.nutrition, &mut out);
            out.extend_from_slice(&food.saturation.to_be_bytes());
            out.push(u8::from(food.can_always_eat));
        }
        Component::Tool(tool) => {
            write_count(tool.rules.len(), &mut out)?;
            for rule in &tool.rules {
                write_id_set(&rule.blocks, &mut out)?;
                write_optional(rule.speed, &mut out, |value, out| {
                    out.extend_from_slice(&value.to_be_bytes());
                });
                write_optional(rule.correct_for_drops, &mut out, |value, out| {
                    out.push(u8::from(value));
                });
            }
            out.extend_from_slice(&tool.default_mining_speed.to_be_bytes());
            write_varint(tool.damage_per_block, &mut out);
            out.push(u8::from(tool.can_destroy_blocks_in_creative));
        }
        Component::PotionContents(contents) => {
            match &contents.potion {
                Some(potion) => {
                    out.push(1);
                    let id = registry_ids::potion_id(potion).ok_or_else(|| {
                        WireError::UnknownRegistry {
                            registry: "potion",
                            name: potion.clone(),
                        }
                    })?;
                    write_varint(i32::from(id), &mut out);
                }
                None => out.push(0),
            }
            write_optional(contents.custom_color, &mut out, |color, out| {
                out.extend_from_slice(&color.to_be_bytes());
            });
            write_count(contents.custom_effects.len(), &mut out)?;
            for effect in &contents.custom_effects {
                let id = registry_ids::mob_effect_id(&effect.effect).ok_or_else(|| {
                    WireError::UnknownRegistry {
                        registry: "mob_effect",
                        name: effect.effect.clone(),
                    }
                })?;
                write_varint(i32::from(id), &mut out);
                write_varint(effect.amplifier, &mut out);
                write_varint(effect.duration, &mut out);
                out.push(u8::from(effect.ambient));
                out.push(u8::from(effect.visible));
                out.push(u8::from(effect.show_icon));
                out.push(0); // no hidden effect
            }
            match &contents.custom_name {
                Some(name) => {
                    out.push(1);
                    write_string(name, &mut out)?;
                }
                None => out.push(0),
            }
        }
        Component::Trim(trim) => {
            // A holder reference is the registry id plus one; zero would start an inline value.
            for (registry, name, id) in [
                (
                    "trim_material",
                    &trim.material,
                    registry_ids::trim_material_id(&trim.material),
                ),
                (
                    "trim_pattern",
                    &trim.pattern,
                    registry_ids::trim_pattern_id(&trim.pattern),
                ),
            ] {
                let id = id.ok_or_else(|| WireError::UnknownRegistry {
                    registry,
                    name: name.clone(),
                })?;
                write_varint(id + 1, &mut out);
            }
        }
        Component::LodestoneTracker(tracker) => {
            match &tracker.target {
                Some((dimension, [x, y, z])) => {
                    out.push(1);
                    write_string(dimension, &mut out)?;
                    // `BlockPos.asLong`: 26 bits of x, 26 of z, 12 of y.
                    let packed = ((i64::from(*x) & 0x3FF_FFFF) << 38)
                        | ((i64::from(*z) & 0x3FF_FFFF) << 12)
                        | (i64::from(*y) & 0xFFF);
                    out.extend_from_slice(&packed.to_be_bytes());
                }
                None => out.push(0),
            }
            out.push(u8::from(tracker.tracked));
        }
        Component::Opaque(_) => return Err(WireError::NotRepresentable(component.kind())),
        Component::Raw(bytes) => out.extend_from_slice(bytes),
    }
    Ok(out)
}

fn operation_id(operation: AttributeOperation) -> i32 {
    i32::try_from(
        AttributeOperation::ALL
            .iter()
            .position(|candidate| *candidate == operation)
            .expect("every operation is in AttributeOperation::ALL"),
    )
    .expect("the operation list fits in i32")
}

fn slot_id(slot: EquipmentSlot) -> i32 {
    i32::try_from(
        EquipmentSlot::ALL
            .iter()
            .position(|candidate| *candidate == slot)
            .expect("every slot is in EquipmentSlot::ALL"),
    )
    .expect("the slot list fits in i32")
}

// ---------------------------------------------------------------------------------------------
// Structural payload parser: every component type
// ---------------------------------------------------------------------------------------------

/// Skips one component payload for its type id.
fn skip_component_payload(id: u16, reader: &mut Reader<'_>, depth: u8) -> Result<(), WireError> {
    if depth > MAX_DEPTH {
        return Err(WireError::Depth);
    }
    match id {
        // NBT values.
        ids::CUSTOM_DATA
        | ids::MAP_DECORATIONS
        | ids::DEBUG_STICK_STATE
        | ids::BUCKET_ENTITY_DATA
        | ids::RECIPES
        | ids::LOCK
        | ids::CONTAINER_LOOT => reader.skip_nbt()?,

        // Simple VarInts.
        ids::MAX_STACK_SIZE
        | ids::MAX_DAMAGE
        | ids::DAMAGE
        | ids::DAMAGE_TYPE
        | ids::ENCHANTABLE
        | ids::REPAIR_COST
        | ids::OMINOUS_BOTTLE_AMPLIFIER
        | ids::ADDITIONAL_TRADE_COST
        | ids::MAP_ID
        | ids::MAP_POST_PROCESSING
        | ids::VILLAGER_FOOD
        | ids::BLOCK_TRANSFORMER
        | ids::PROVIDES_POTTERY_PATTERN
        | ids::DYE
        | ids::BASE_COLOR
        | ids::CUSHION_COLOR
        | ids::VILLAGER_VARIANT
        | ids::WOLF_VARIANT
        | ids::WOLF_SOUND_VARIANT
        | ids::WOLF_COLLAR
        | ids::FOX_VARIANT
        | ids::SALMON_SIZE
        | ids::PARROT_VARIANT
        | ids::TROPICAL_FISH_PATTERN
        | ids::TROPICAL_FISH_BASE_COLOR
        | ids::TROPICAL_FISH_PATTERN_COLOR
        | ids::MOOSHROOM_VARIANT
        | ids::RABBIT_VARIANT
        | ids::PIG_VARIANT
        | ids::PIG_SOUND_VARIANT
        | ids::COW_VARIANT
        | ids::COW_SOUND_VARIANT
        | ids::CHICKEN_VARIANT
        | ids::CHICKEN_SOUND_VARIANT
        | ids::ZOMBIE_NAUTILUS_VARIANT
        | ids::FROG_VARIANT
        | ids::HORSE_VARIANT
        | ids::LLAMA_VARIANT
        | ids::AXOLOTL_VARIANT
        | ids::CAT_VARIANT
        | ids::CAT_SOUND_VARIANT
        | ids::CAT_COLLAR
        | ids::SHEEP_COLOR
        | ids::SHULKER_COLOR => {
            reader.varint()?;
        }

        // Randomly ticked-looking singles.
        ids::MINIMUM_ATTACK_CHARGE | ids::POTION_DURATION_SCALE => {
            reader.f32()?;
        }
        ids::DYED_COLOR => {
            reader.i32()?;
        }
        ids::ENCHANTMENT_GLINT_OVERRIDE => {
            reader.bool()?;
        }

        // Unit components with no payload.
        ids::UNBREAKABLE
        | ids::CREATIVE_SLOT_LOCK
        | ids::INTANGIBLE_PROJECTILE
        | ids::GLIDER
        | ids::WAXED => {}

        ids::ATTACK_ANIMATION | ids::INTERACT_ANIMATION => {
            reader.varint()?;
            reader.varint()?;
        }
        ids::USE_EFFECTS => {
            reader.bool()?;
            reader.bool()?;
            reader.f32()?;
        }
        ids::FOOD => {
            reader.varint()?;
            reader.f32()?;
            reader.bool()?;
        }
        ids::WEAPON => {
            reader.varint()?;
            reader.f32()?;
        }
        ids::ATTACK_RANGE => {
            for _ in 0..6 {
                reader.f32()?;
            }
        }
        ids::RARITY => {
            reader.varint()?;
        }
        ids::CUSTOM_NAME | ids::ITEM_NAME => {
            reader.skip_nbt()?;
        }
        ids::LORE => {
            reader.prefixed(MAX_LIST, Reader::skip_nbt)?;
        }
        ids::ITEM_MODEL | ids::TOOLTIP_STYLE | ids::NOTE_BLOCK_SOUND => {
            reader.id()?;
        }
        ids::ENCHANTMENTS | ids::STORED_ENCHANTMENTS => {
            reader.prefixed(MAX_LIST, |reader| {
                reader.varint()?;
                reader.varint()?;
                Ok(())
            })?;
        }
        ids::CUSTOM_MODEL_DATA => {
            reader.prefixed(MAX_LIST, Reader::f32)?;
            reader.prefixed(MAX_LIST, Reader::bool)?;
            reader.prefixed(MAX_LIST, |reader| reader.id())?;
            reader.prefixed(MAX_LIST, Reader::i32)?;
        }
        ids::TOOLTIP_DISPLAY => {
            reader.bool()?;
            reader.prefixed(MAX_LIST, Reader::varint)?;
        }
        ids::CAN_PLACE_ON | ids::CAN_BREAK => {
            reader.prefixed(MAX_LIST, |reader| block_predicate(reader, depth))?;
        }
        ids::ATTRIBUTE_MODIFIERS => {
            reader.prefixed(MAX_LIST, |reader| {
                reader.varint()?; // attribute id
                reader.id()?; // modifier id
                reader.f64()?;
                reader.varint()?; // operation
                reader.varint()?; // slot group
                match reader.varint()? {
                    0 | 1 => Ok(()),
                    2 => {
                        reader.skip_nbt()?;
                        Ok(())
                    }
                    _ => Err(WireError::InvalidPayload("attribute modifier display")),
                }
            })?;
        }
        ids::TOOL => {
            reader.prefixed(MAX_LIST, |reader| {
                reader.id_set()?;
                if reader.bool()? {
                    reader.f32()?;
                }
                if reader.bool()? {
                    reader.bool()?;
                }
                Ok(())
            })?;
            reader.f32()?; // default mining speed
            reader.varint()?; // damage per block
            reader.bool()?; // can destroy blocks in creative
        }
        ids::CONSUMABLE => {
            reader.f32()?;
            reader.varint()?; // animation
            reader.id_or(sound_event)?;
            reader.bool()?;
            reader.prefixed(MAX_LIST, |reader| consume_effect(reader, depth))?;
        }
        ids::USE_REMAINDER | ids::SULFUR_CUBE_CONTENT => {
            item_stack_template(reader, depth)?;
        }
        ids::USE_COOLDOWN => {
            reader.f32()?;
            reader.optional(Reader::id)?;
        }
        ids::DAMAGE_RESISTANT | ids::REPAIRABLE | ids::PROVIDES_BANNER_PATTERNS => {
            reader.id_set()?;
        }
        ids::MOB_VISIBILITY => {
            reader.id_set()?;
            reader.f32()?;
        }
        ids::DEATH_PROTECTION => {
            reader.prefixed(MAX_LIST, |reader| consume_effect(reader, depth))?;
        }
        ids::EQUIPPABLE => {
            reader.varint()?; // equipment slot
            reader.id_or(sound_event)?;
            reader.optional(Reader::id)?; // equipment asset
            reader.optional(Reader::id)?; // camera overlay
            reader.optional(|reader| reader.id_set())?; // allowed entities
            for _ in 0..5 {
                reader.bool()?;
            }
            reader.id_or(sound_event)?; // shearing sound
        }
        ids::BLOCKS_ATTACKS => {
            reader.f32()?;
            reader.f32()?;
            reader.prefixed(MAX_LIST, |reader| {
                reader.f32()?; // angle
                reader.optional(|reader| reader.id_set())?; // damage types
                reader.f32()?; // base
                reader.f32()?; // factor
                Ok(())
            })?;
            reader.f32()?;
            reader.f32()?;
            reader.f32()?;
            reader.optional(|reader| reader.id_set())?;
            reader.optional(|reader| reader.id_or(sound_event))?;
            reader.optional(|reader| reader.id_or(sound_event))?;
        }
        ids::PIERCING_WEAPON => {
            reader.bool()?;
            reader.bool()?;
            reader.optional(|reader| reader.id_or(sound_event))?;
            reader.optional(|reader| reader.id_or(sound_event))?;
        }
        ids::KINETIC_WEAPON => {
            reader.varint()?;
            reader.varint()?;
            for _ in 0..3 {
                if reader.bool()? {
                    kinetic_condition(reader)?;
                }
            }
            reader.f32()?;
            reader.f32()?;
            reader.optional(|reader| reader.id_or(sound_event))?;
            reader.optional(|reader| reader.id_or(sound_event))?;
        }
        ids::CHARGED_PROJECTILES | ids::BUNDLE_CONTENTS => {
            reader.prefixed(MAX_LIST, |reader| item_stack_template(reader, depth))?;
        }
        ids::CONTAINER => {
            reader.prefixed(256, |reader| {
                reader.optional(|reader| item_stack_template(reader, depth))?;
                Ok(())
            })?;
        }
        ids::POTION_CONTENTS => {
            reader.optional(Reader::varint)?; // potion id
            reader.optional(Reader::i32)?; // custom color
            reader.prefixed(MAX_LIST, |reader| potion_effect(reader, depth))?;
            reader.optional(|reader| reader.string(32767))?;
        }
        ids::SUSPICIOUS_STEW_EFFECTS => {
            reader.prefixed(MAX_LIST, |reader| {
                reader.varint()?;
                reader.varint()?;
                Ok(())
            })?;
        }
        ids::WRITABLE_BOOK_CONTENT => {
            reader.prefixed(100, |reader| {
                reader.string(1024)?;
                reader.optional(|reader| reader.string(1024).map(drop))?;
                Ok(())
            })?;
        }
        ids::WRITTEN_BOOK_CONTENT => {
            reader.string(32)?;
            reader.optional(|reader| reader.string(32).map(drop))?;
            reader.string(32767)?;
            reader.varint()?;
            reader.prefixed(100, |reader| {
                reader.skip_nbt()?;
                reader.optional(|reader| reader.skip_nbt())?;
                Ok(())
            })?;
            reader.bool()?;
        }
        ids::TRIM => {
            reader.id_or(trim_material)?;
            reader.id_or(trim_pattern)?;
        }
        ids::PROVIDES_TRIM_MATERIAL => {
            reader.id_or(trim_material)?;
        }
        ids::INSTRUMENT => {
            reader.id_or(instrument)?;
        }
        ids::JUKEBOX_PLAYABLE => {
            reader.id_or(jukebox_song)?;
        }
        ids::PAINTING_VARIANT => {
            reader.id_or(painting_variant)?;
        }
        ids::ENTITY_DATA | ids::BLOCK_ENTITY_DATA => {
            reader.varint()?;
            reader.skip_nbt()?;
        }
        ids::LODESTONE_TRACKER => {
            if reader.bool()? {
                reader.id()?;
                reader.i64()?;
            }
            reader.bool()?;
        }
        ids::FIREWORK_EXPLOSION => {
            firework_explosion(reader)?;
        }
        ids::FIREWORKS => {
            reader.varint()?;
            reader.prefixed(MAX_LIST, firework_explosion)?;
        }
        ids::PROFILE => {
            resolvable_profile(reader)?;
        }
        ids::BANNER_PATTERNS => {
            reader.prefixed(MAX_LIST, |reader| {
                reader.id_or(banner_pattern)?;
                reader.varint()?; // dye
                Ok(())
            })?;
        }
        ids::POT_DECORATIONS => {
            reader.prefixed(4, Reader::varint)?;
        }
        ids::BLOCK_STATE => {
            reader.prefixed(MAX_LIST, |reader| {
                reader.string(32767)?;
                reader.string(32767)?;
                Ok(())
            })?;
        }
        ids::BEES => {
            reader.prefixed(MAX_LIST, |reader| {
                reader.varint()?; // entity type
                reader.skip_nbt()?;
                reader.varint()?;
                reader.varint()?;
                Ok(())
            })?;
        }
        ids::BREAK_SOUND => {
            reader.id_or(sound_event)?;
        }
        ids::COMPOSTABLE => {
            resolvable_int(reader)?;
        }
        ids::COOKING_FUEL | ids::BREWING_FUEL => {
            resolvable_int(reader)?;
            resolvable_float(reader)?;
        }
        ids::SIGN_TEXT_FRONT | ids::SIGN_TEXT_BACK => {
            sign_text(reader)?;
        }
        _ => return Err(WireError::UnsupportedComponent(id)),
    }
    Ok(())
}

fn block_predicate(reader: &mut Reader<'_>, depth: u8) -> Result<(), WireError> {
    reader.optional(|reader| reader.id_set())?;
    reader.optional(|reader| {
        reader.prefixed(MAX_LIST, |reader| {
            reader.string(32767)?; // property name
            if reader.bool()? {
                reader.string(32767)?; // exact value
            } else {
                reader.optional(|reader| reader.string(32767).map(drop))?;
                reader.optional(|reader| reader.string(32767).map(drop))?;
            }
            Ok(())
        })
    })?;
    reader.optional(|reader| reader.skip_nbt())?;
    reader.prefixed(MAX_LIST, |reader| {
        let id = reader.component_id()?;
        reader.skip_component_payload(id, depth + 1)
    })?;
    reader.prefixed(64, |reader| {
        reader.varint()?;
        reader.skip_nbt()
    })?;
    Ok(())
}

fn consume_effect(reader: &mut Reader<'_>, depth: u8) -> Result<(), WireError> {
    match reader.varint()? {
        0 => {
            reader.prefixed(MAX_LIST, |reader| potion_effect(reader, depth))?;
            reader.f32()?;
        }
        1 => reader.id_set()?,
        2 => {}
        3 => {
            reader.f32()?;
        }
        4 => reader.id_or(sound_event)?,
        _ => return Err(WireError::InvalidPayload("consume effect")),
    }
    Ok(())
}

fn potion_effect(reader: &mut Reader<'_>, depth: u8) -> Result<(), WireError> {
    if depth > MAX_DEPTH {
        return Err(WireError::Depth);
    }
    reader.varint()?; // effect id
    potion_effect_details(reader, depth)
}

fn potion_effect_details(reader: &mut Reader<'_>, depth: u8) -> Result<(), WireError> {
    if depth > MAX_DEPTH {
        return Err(WireError::Depth);
    }
    reader.varint()?; // amplifier
    reader.varint()?; // duration
    reader.bool()?; // ambient
    reader.bool()?; // show particles
    reader.bool()?; // show icon
    if reader.bool()? {
        potion_effect_details(reader, depth + 1)?;
    }
    Ok(())
}

fn kinetic_condition(reader: &mut Reader<'_>) -> Result<(), WireError> {
    reader.varint()?;
    reader.f32()?;
    reader.f32()?;
    Ok(())
}

fn firework_explosion(reader: &mut Reader<'_>) -> Result<(), WireError> {
    reader.varint()?;
    reader.prefixed(MAX_LIST, Reader::i32)?;
    reader.prefixed(MAX_LIST, Reader::i32)?;
    reader.bool()?;
    reader.bool()?;
    Ok(())
}

fn item_stack_template(reader: &mut Reader<'_>, depth: u8) -> Result<(), WireError> {
    reader.item_id()?;
    reader.count()?;
    let added = reader.count_len(MAX_LIST)?;
    let removed = reader.count_len(MAX_LIST)?;
    for _ in 0..added {
        let id = reader.component_id()?;
        reader.skip_component_payload(id, depth + 1)?;
    }
    for _ in 0..removed {
        reader.component_id()?;
    }
    Ok(())
}

fn sound_event(reader: &mut Reader<'_>) -> Result<(), WireError> {
    reader.id()?;
    if reader.bool()? {
        reader.f32()?;
    }
    Ok(())
}

fn trim_material(reader: &mut Reader<'_>) -> Result<(), WireError> {
    reader.id()?; // palette id
    reader.skip_nbt()?; // description
    Ok(())
}

fn trim_pattern(reader: &mut Reader<'_>) -> Result<(), WireError> {
    reader.id()?; // asset id
    reader.skip_nbt()?; // description
    reader.bool()?; // decal
    Ok(())
}

fn instrument(reader: &mut Reader<'_>) -> Result<(), WireError> {
    reader.id_or(sound_event)?;
    reader.f32()?; // use duration
    reader.f32()?; // range
    reader.varint()?; // durability damage
    reader.skip_nbt()?; // description
    Ok(())
}

fn jukebox_song(reader: &mut Reader<'_>) -> Result<(), WireError> {
    reader.id_or(sound_event)?;
    reader.skip_nbt()?; // description
    reader.f32()?; // length
    reader.varint()?; // comparator output
    Ok(())
}

fn banner_pattern(reader: &mut Reader<'_>) -> Result<(), WireError> {
    reader.id()?; // asset id
    reader.string(32767)?; // translation key
    Ok(())
}

fn painting_variant(reader: &mut Reader<'_>) -> Result<(), WireError> {
    reader.varint()?; // width
    reader.varint()?; // height
    reader.id()?; // asset id
    reader.optional(|reader| reader.skip_nbt())?; // title
    reader.optional(|reader| reader.skip_nbt())?; // author
    Ok(())
}

fn resolvable_int(reader: &mut Reader<'_>) -> Result<(), WireError> {
    if reader.bool()? {
        reader.i32()?;
    } else {
        reader.varint()?;
    }
    Ok(())
}

fn resolvable_float(reader: &mut Reader<'_>) -> Result<(), WireError> {
    if reader.bool()? {
        reader.f32()?;
    } else {
        reader.varint()?;
    }
    Ok(())
}

fn resolvable_profile(reader: &mut Reader<'_>) -> Result<(), WireError> {
    if reader.bool()? {
        reader.uuid()?;
        reader.string(16)?;
        profile_properties(reader)?;
    } else {
        reader.optional(|reader| reader.string(16).map(drop))?;
        reader.optional(|reader| reader.uuid())?;
        profile_properties(reader)?;
    }
    reader.optional(Reader::id)?; // body
    reader.optional(Reader::id)?; // cape
    reader.optional(Reader::id)?; // elytra
    reader.optional(Reader::varint)?; // model
    Ok(())
}

fn profile_properties(reader: &mut Reader<'_>) -> Result<(), WireError> {
    reader.prefixed(16, |reader| {
        reader.string(64)?;
        reader.string(32767)?;
        reader.optional(|reader| reader.string(1024).map(drop))?;
        Ok(())
    })?;
    Ok(())
}

fn sign_text(reader: &mut Reader<'_>) -> Result<(), WireError> {
    for _ in 0..4 {
        reader.skip_nbt()?;
    }
    if reader.bool()? {
        for _ in 0..4 {
            reader.skip_nbt()?;
        }
    }
    reader.varint()?; // dye color
    reader.bool()?; // glowing
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Registry ids used by typed payloads
// ---------------------------------------------------------------------------------------------

/// Registry ids the typed network components need: attributes, blocks and enchantments.
pub mod registry_ids {
    include!(concat!(env!("OUT_DIR"), "/wire_registries.rs"));

    fn key(name: &str) -> &str {
        name.strip_prefix("minecraft:").unwrap_or(name)
    }

    fn lookup(entries: &'static [(&'static str, u16)], name: &str) -> Option<u16> {
        let wanted = key(name);
        let index = entries
            .binary_search_by(|(candidate, _)| key(candidate).cmp(wanted))
            .ok()?;
        entries.get(index).map(|(_, id)| *id)
    }

    /// Protocol id of an attribute, with or without the `minecraft:` prefix.
    pub fn attribute_id(name: &str) -> Option<u16> {
        lookup(ATTRIBUTES, name)
    }

    /// Attribute name of a protocol id.
    pub fn attribute_name(id: u16) -> Option<&'static str> {
        ATTRIBUTE_NAMES.get(usize::from(id)).copied()
    }

    /// Protocol id of a block, with or without the `minecraft:` prefix.
    pub fn block_id(name: &str) -> Option<u16> {
        lookup(BLOCKS, name)
    }

    /// Block name of a protocol id.
    pub fn block_name(id: u16) -> Option<&'static str> {
        BLOCK_NAMES.get(usize::from(id)).copied()
    }

    /// Registry id of an enchantment, with or without the `minecraft:` prefix.
    pub fn enchantment_id(name: &str) -> Option<i32> {
        let wanted = key(name);
        let index = ENCHANTMENTS
            .binary_search_by(|candidate| key(candidate).cmp(wanted))
            .ok()?;
        i32::try_from(index).ok()
    }

    /// Registry id of a trim material, with or without the `minecraft:` prefix.
    pub fn trim_material_id(name: &str) -> Option<i32> {
        let wanted = key(name);
        let index = TRIM_MATERIALS
            .binary_search_by(|candidate| key(candidate).cmp(wanted))
            .ok()?;
        i32::try_from(index).ok()
    }

    /// Registry id of a jukebox song (`minecraft:13`), with or without the `minecraft:` prefix.
    pub fn jukebox_song_id(name: &str) -> Option<i32> {
        let wanted = key(name);
        let index = JUKEBOX_SONGS
            .binary_search_by(|candidate| key(candidate).cmp(wanted))
            .ok()?;
        i32::try_from(index).ok()
    }

    /// Registry id of a trim pattern, with or without the `minecraft:` prefix.
    pub fn trim_pattern_id(name: &str) -> Option<i32> {
        let wanted = key(name);
        let index = TRIM_PATTERNS
            .binary_search_by(|candidate| key(candidate).cmp(wanted))
            .ok()?;
        i32::try_from(index).ok()
    }

    /// Enchantment name of a registry id.
    pub fn enchantment_name(id: i32) -> Option<&'static str> {
        usize::try_from(id)
            .ok()
            .and_then(|index| ENCHANTMENTS.get(index))
            .copied()
    }

    /// Protocol id of a potion, with or without the `minecraft:` prefix.
    pub fn potion_id(name: &str) -> Option<u16> {
        lookup(POTIONS, name)
    }

    /// Potion name of a protocol id.
    pub fn potion_name(id: u16) -> Option<&'static str> {
        POTION_NAMES.get(usize::from(id)).copied()
    }

    /// Protocol id of a mob effect, with or without the `minecraft:` prefix.
    pub fn mob_effect_id(name: &str) -> Option<u16> {
        lookup(MOB_EFFECTS, name)
    }

    /// Mob effect name of a protocol id.
    pub fn mob_effect_name(id: u16) -> Option<&'static str> {
        MOB_EFFECT_NAMES.get(usize::from(id)).copied()
    }
}

// ---------------------------------------------------------------------------------------------
// IdSet helpers
// ---------------------------------------------------------------------------------------------

/// Reads an `IdSet` that the typed tool component can hold: a tag or a single block id.
fn read_id_set_string(reader: &mut Reader<'_>) -> Result<String, WireError> {
    match reader.varint()? {
        0 => Ok(format!("#{}", reader.id()?)),
        2 => {
            let id = reader.varint()?;
            let id = u16::try_from(id).map_err(|_| WireError::InvalidPayload("block id"))?;
            registry_ids::block_name(id)
                .map(str::to_owned)
                .ok_or(WireError::InvalidPayload("block id"))
        }
        kind if kind > 0 => Err(WireError::InvalidPayload("multi-block tool set")),
        kind => Err(WireError::NegativeLength(kind)),
    }
}

/// Writes an `IdSet` for a typed tool rule: `#tag` becomes a tag set, anything else a single
/// block id.
fn write_id_set(blocks: &str, out: &mut Vec<u8>) -> Result<(), WireError> {
    if let Some(tag) = blocks.strip_prefix('#') {
        write_varint(0, out);
        write_string(tag, out)?;
    } else {
        let id = registry_ids::block_id(blocks).ok_or_else(|| WireError::UnknownRegistry {
            registry: "block",
            name: blocks.to_owned(),
        })?;
        write_varint(2, out); // one inline id
        write_varint(i32::from(id), out);
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Network reader and writer primitives
// ---------------------------------------------------------------------------------------------

/// A bounds-checked cursor over a packet body.
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], WireError> {
        let end = self.pos.checked_add(len).ok_or(WireError::UnexpectedEof)?;
        let slice = self
            .bytes
            .get(self.pos..end)
            .ok_or(WireError::UnexpectedEof)?;
        self.pos = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, WireError> {
        Ok(self.take(1)?[0])
    }

    fn bool(&mut self) -> Result<bool, WireError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(WireError::InvalidBoolean(other)),
        }
    }

    fn varint(&mut self) -> Result<i32, WireError> {
        let mut value = 0_u32;
        let mut shift = 0;
        loop {
            let byte = self.u8()?;
            value |= u32::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value as i32);
            }
            shift += 7;
            if shift >= 35 {
                return Err(WireError::VarIntTooLong);
            }
        }
    }

    fn count_len(&mut self, max: usize) -> Result<usize, WireError> {
        let count = self.varint()?;
        if count < 0 {
            return Err(WireError::NegativeLength(count));
        }
        let count = count as usize;
        if count > max {
            return Err(WireError::ArrayTooLong(count, max));
        }
        Ok(count)
    }

    fn i32(&mut self) -> Result<i32, WireError> {
        let bytes = self.take(4)?;
        Ok(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn i64(&mut self) -> Result<i64, WireError> {
        let bytes = self.take(8)?;
        Ok(i64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    fn f32(&mut self) -> Result<f32, WireError> {
        Ok(f32::from_bits(self.i32()? as u32))
    }

    fn f64(&mut self) -> Result<f64, WireError> {
        Ok(f64::from_bits(self.i64()? as u64))
    }

    fn uuid(&mut self) -> Result<(), WireError> {
        self.take(16)?;
        Ok(())
    }

    fn string(&mut self, max_chars: usize) -> Result<&'a str, WireError> {
        let len = self.varint()?;
        if len < 0 {
            return Err(WireError::NegativeLength(len));
        }
        let len = len as usize;
        if len > max_chars.saturating_mul(3) + 3 {
            return Err(WireError::StringTooLong(len, max_chars));
        }
        let bytes = self.take(len)?;
        let text = std::str::from_utf8(bytes).map_err(|_| WireError::InvalidString)?;
        if text.encode_utf16().count() > max_chars {
            return Err(WireError::StringTooLong(len, max_chars));
        }
        Ok(text)
    }

    fn id(&mut self) -> Result<&'a str, WireError> {
        self.string(32767)
    }

    fn item_id(&mut self) -> Result<u16, WireError> {
        let id = self.varint()?;
        if id < 0 || id as usize >= item_count() {
            return Err(WireError::InvalidItemId(id));
        }
        Ok(id as u16)
    }

    fn count(&mut self) -> Result<i32, WireError> {
        let count = self.varint()?;
        if !(0..=MAX_COUNT).contains(&count) {
            return Err(WireError::InvalidCount(count));
        }
        Ok(count)
    }

    fn component_id(&mut self) -> Result<u16, WireError> {
        let id = self.varint()?;
        if id < 0 || id as usize >= COMPONENT_COUNT {
            return Err(WireError::InvalidComponentId(id));
        }
        Ok(id as u16)
    }

    fn prefixed<T>(
        &mut self,
        max: usize,
        mut value: impl FnMut(&mut Self) -> Result<T, WireError>,
    ) -> Result<Vec<T>, WireError> {
        let count = self.count_len(max)?;
        let mut out = Vec::with_capacity(count.min(256));
        for _ in 0..count {
            out.push(value(self)?);
        }
        Ok(out)
    }

    fn optional<T>(
        &mut self,
        value: impl FnOnce(&mut Self) -> Result<T, WireError>,
    ) -> Result<Option<T>, WireError> {
        if self.bool()? {
            Ok(Some(value(self)?))
        } else {
            Ok(None)
        }
    }

    /// Reads an `IdSet`: a tag name (kind 0) or `kind - 1` registry ids.
    fn id_set(&mut self) -> Result<(), WireError> {
        let kind = self.varint()?;
        if kind == 0 {
            self.id()?;
        } else if kind > 0 {
            let count = usize::try_from(kind - 1)
                .map_err(|_| WireError::ArrayTooLong(usize::MAX, MAX_LIST))?;
            if count > MAX_LIST {
                return Err(WireError::ArrayTooLong(count, MAX_LIST));
            }
            for _ in 0..count {
                self.varint()?;
            }
        } else {
            return Err(WireError::NegativeLength(kind));
        }
        Ok(())
    }

    /// Reads an `IdOr`: kind 0 followed by inline data, or a registry id.
    fn id_or(
        &mut self,
        inline: impl FnOnce(&mut Self) -> Result<(), WireError>,
    ) -> Result<(), WireError> {
        let id = self.varint()?;
        if id == 0 {
            inline(self)?;
        } else if id < 0 {
            return Err(WireError::NegativeLength(id));
        }
        Ok(())
    }

    /// Reads a text component (network NBT).
    fn text(&mut self) -> Result<Text, WireError> {
        let tag = self.nbt_tag()?;
        Text::from_nbt(&tag).map_err(WireError::Text)
    }

    fn text_list(&mut self, max: usize) -> Result<Vec<Text>, WireError> {
        self.prefixed(max, Reader::text)
    }

    /// Skips a nameless-root NBT tag, returning the tag slice.
    fn nbt(&mut self) -> Result<&'a [u8], WireError> {
        let start = self.pos;
        self.skip_nbt()?;
        Ok(&self.bytes[start..self.pos])
    }

    fn nbt_tag(&mut self) -> Result<Tag, WireError> {
        let bytes = self.nbt()?;
        ferrite_nbt::binary::read_network(bytes).map_err(WireError::Nbt)
    }

    fn skip_nbt(&mut self) -> Result<(), WireError> {
        let tag = self.u8()?;
        if tag == 0 {
            return Err(WireError::NbtTag(0));
        }
        self.skip_nbt_payload(tag, 0)
    }

    fn skip_nbt_payload(&mut self, tag: u8, depth: usize) -> Result<(), WireError> {
        if depth > 512 {
            return Err(WireError::NbtDepth(512));
        }
        match tag {
            0 => Ok(()),
            1 => self.take(1).map(drop),
            2 => self.take(2).map(drop),
            3 => self.take(4).map(drop),
            4 => self.take(8).map(drop),
            5 => self.take(4).map(drop),
            6 => self.take(8).map(drop),
            7 | 11 => {
                let count = self.i32()?;
                let width = if tag == 7 { 1 } else { 4 };
                self.take_array(count, width)
            }
            12 => {
                let count = self.i32()?;
                self.take_array(count, 8)
            }
            8 => {
                let len = self.u16()?;
                self.take(usize::from(len)).map(drop)
            }
            9 => {
                let element = self.u8()?;
                let count = self.i32()?;
                let count = self.array_len(count)?;
                for _ in 0..count {
                    self.skip_nbt_payload(element, depth + 1)?;
                }
                Ok(())
            }
            10 => loop {
                let entry = self.u8()?;
                if entry == 0 {
                    return Ok(());
                }
                let name_len = self.u16()?;
                self.take(usize::from(name_len))?;
                self.skip_nbt_payload(entry, depth + 1)?;
            },
            other => Err(WireError::NbtTag(other)),
        }
    }

    fn u16(&mut self) -> Result<u16, WireError> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn array_len(&mut self, count: i32) -> Result<usize, WireError> {
        if count < 0 {
            return Err(WireError::NegativeLength(count));
        }
        Ok(count as usize)
    }

    fn take_array(&mut self, count: i32, width: usize) -> Result<(), WireError> {
        let count = self.array_len(count)?;
        let len = count.checked_mul(width).ok_or(WireError::UnexpectedEof)?;
        self.take(len).map(drop)
    }

    /// Advances past one component payload and returns its slice.
    fn component_payload(&mut self, id: u16, depth: u8) -> Result<&'a [u8], WireError> {
        let start = self.pos;
        skip_component_payload(id, self, depth)?;
        Ok(&self.bytes[start..self.pos])
    }

    fn skip_component_payload(&mut self, id: u16, depth: u8) -> Result<(), WireError> {
        skip_component_payload(id, self, depth)
    }
}

fn write_varint(value: i32, out: &mut Vec<u8>) {
    let mut value = value as u32;
    while value & !0x7f != 0 {
        out.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn len_i32(len: usize) -> Result<i32, WireError> {
    i32::try_from(len).map_err(|_| WireError::ArrayTooLong(len, i32::MAX as usize))
}

fn write_count(len: usize, out: &mut Vec<u8>) -> Result<(), WireError> {
    write_varint(len_i32(len)?, out);
    Ok(())
}

fn write_string(value: &str, out: &mut Vec<u8>) -> Result<(), WireError> {
    write_count(value.len(), out)?;
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn write_optional<T>(value: Option<T>, out: &mut Vec<u8>, write: impl FnOnce(T, &mut Vec<u8>)) {
    match value {
        Some(value) => {
            out.push(1);
            write(value, out);
        }
        None => out.push(0),
    }
}

fn nbt_bytes(tag: &Tag) -> Result<Vec<u8>, WireError> {
    ferrite_nbt::binary::write_network(tag).map_err(WireError::Nbt)
}

/// [`ferrite_protocol::Encode`] and [`ferrite_protocol::Decode`] implementations.
///
/// The protocol crate cannot depend on this one (packets may hold item stacks, not the other
/// way around), so the impls live here behind the `protocol` feature. `Encode` is infallible by
/// design; it panics when the stack holds a component that has no network form (an
/// [`Component::Opaque`] loaded from the save format, or an unresolvable registry name).
#[cfg(feature = "protocol")]
mod protocol_impl {
    use ferrite_protocol::{Decode, DecodeError, Encode};

    use crate::{HashedSlot, ItemStack};

    impl Encode for ItemStack {
        fn encode(&self, out: &mut Vec<u8>) {
            self.encode_network_into(out)
                .expect("item stack cannot be encoded to the network form");
        }
    }

    impl Decode for ItemStack {
        fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
            let (stack, consumed) = ItemStack::decode_network_prefix(buf)
                .map_err(|_| DecodeError::Invalid("invalid item stack"))?;
            *buf = &buf[consumed..];
            Ok(stack)
        }
    }

    /// The body of a present `HashedStack`; `Option<HashedSlot>` adds vanilla's `has_item`
    /// boolean, so it is the full wire form.
    impl Encode for HashedSlot {
        fn encode(&self, out: &mut Vec<u8>) {
            let bytes = self
                .encode_network()
                .expect("hashed slot lists fit a VarInt");
            out.extend_from_slice(&bytes[1..]);
        }
    }

    impl Decode for HashedSlot {
        fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
            let mut reader = super::Reader::new(buf);
            let slot = super::decode_hashed_body(&mut reader)
                .map_err(|_| DecodeError::Invalid("invalid hashed item stack"))?;
            *buf = &buf[reader.pos..];
            Ok(slot)
        }
    }
}

#[cfg(test)]
mod tests;
