//! Network slot tests: structural coverage of every component type, typed decodes, byte-exact
//! vectors, nested templates, the hashed slot and a fuzz pass.

use ferrite_nbt::{Compound, Tag};
use ferrite_text::Component as Text;

use super::*;
use crate::component::{
    AttributeModifier, AttributeOperation, Component, CustomModelData, Enchantments, EquipmentSlot,
    Food, Rarity, Tool, ToolRule, TooltipDisplay,
};
use crate::component_ids as ids;

// ---------------------------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------------------------

fn cat(parts: impl IntoIterator<Item = Vec<u8>>) -> Vec<u8> {
    parts.into_iter().flatten().collect()
}

fn vi(value: i32) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(value, &mut out);
    out
}

fn string(value: &str) -> Vec<u8> {
    let mut out = Vec::new();
    write_string(value, &mut out).expect("test strings fit");
    out
}

fn i32b(value: i32) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

fn f32b(value: f32) -> Vec<u8> {
    value.to_bits().to_be_bytes().to_vec()
}

fn f64b(value: f64) -> Vec<u8> {
    value.to_bits().to_be_bytes().to_vec()
}

fn nbt(tag: &Tag) -> Vec<u8> {
    nbt_bytes(tag).expect("test tags encode")
}

fn empty_nbt() -> Vec<u8> {
    nbt(&Tag::Compound(Compound::new()))
}

fn text(value: &str) -> Vec<u8> {
    nbt(&Text::text(value).to_nbt())
}

/// A present `Holder<SoundEvent>` with inline data.
fn sound_inline() -> Vec<u8> {
    cat([string("minecraft:x"), vec![0]])
}

fn sound_or() -> Vec<u8> {
    cat([vec![0], sound_inline()])
}

fn id_or_inline(payload: Vec<u8>) -> Vec<u8> {
    cat([vec![0], payload])
}

fn id_set() -> Vec<u8> {
    cat([vec![0], string("minecraft:x")])
}

/// An `ItemStackTemplate` with one stone and no components.
fn template() -> Vec<u8> {
    cat([vi(1), vi(1), vi(0), vi(0)])
}

fn enchantment_sample() -> Vec<u8> {
    let sharpness =
        registry_ids::enchantment_id("minecraft:sharpness").expect("sharpness is registered");
    cat([vi(1), vi(sharpness), vi(5)])
}

fn trim_material_inline() -> Vec<u8> {
    cat([string("minecraft:trim_material"), text("m")])
}

fn trim_pattern_inline() -> Vec<u8> {
    cat([string("minecraft:trim_pattern"), text("p"), vec![1]])
}

fn instrument_inline() -> Vec<u8> {
    cat([sound_or(), f32b(1.0), f32b(16.0), vi(1), text("i")])
}

/// A full network slot for one component on a stone stack.
fn slot(component: u16, payload: Vec<u8>) -> Vec<u8> {
    cat([
        vec![1, 1], // count 1, minecraft:stone
        vec![1, 0], // one added component, no removed ones
        vi(i32::from(component)),
        payload,
    ])
}

fn stack_with_raw(id: u16, payload: Vec<u8>) -> ItemStack {
    let mut stack = ItemStack::new(1);
    stack.components.insert(id, Component::Raw(payload));
    stack
}

// ---------------------------------------------------------------------------------------------
// Structural coverage
// ---------------------------------------------------------------------------------------------

/// A valid payload for every component type, matching the layout the parser expects.
fn sample_payload(id: u16) -> Vec<u8> {
    match id {
        ids::CUSTOM_DATA => nbt(&Tag::Compound({
            let mut compound = Compound::new();
            compound.insert("a".to_owned(), Tag::Int(1));
            compound
        })),
        ids::MAX_STACK_SIZE => vi(16),
        ids::MAX_DAMAGE => vi(1561),
        ids::DAMAGE => vi(7),
        ids::UNBREAKABLE => vec![],
        ids::USE_EFFECTS => cat([vec![1, 0], f32b(0.5)]),
        ids::CUSTOM_NAME => text("hi"),
        ids::MINIMUM_ATTACK_CHARGE => f32b(1.0),
        ids::DAMAGE_TYPE => vi(1),
        ids::ITEM_NAME => text("hi"),
        ids::ITEM_MODEL => string("minecraft:stone"),
        ids::LORE => cat([vi(2), text("a"), text("b")]),
        ids::RARITY => vi(3),
        ids::ENCHANTMENTS => enchantment_sample(),
        ids::CAN_PLACE_ON | ids::CAN_BREAK => vec![1, 0, 0, 0, 0, 0],
        ids::ATTRIBUTE_MODIFIERS => cat([
            vi(1),
            vi(0),
            string("minecraft:x"),
            f64b(1.5),
            vi(1),
            vi(2),
            vi(0),
        ]),
        ids::CUSTOM_MODEL_DATA => vec![0, 0, 0, 0],
        ids::TOOLTIP_DISPLAY => cat([vec![1], vi(1), vi(3)]),
        ids::REPAIR_COST => vi(3),
        ids::CREATIVE_SLOT_LOCK => vec![],
        ids::ENCHANTMENT_GLINT_OVERRIDE => vec![1],
        ids::INTANGIBLE_PROJECTILE => vec![],
        ids::FOOD => cat([vi(4), f32b(2.4), vec![1]]),
        ids::CONSUMABLE => cat([f32b(1.6), vi(0), sound_or(), vec![0], vi(1), vec![2]]),
        ids::USE_REMAINDER => template(),
        ids::USE_COOLDOWN => cat([f32b(1.0), vec![1], string("minecraft:x")]),
        ids::DAMAGE_RESISTANT => id_set(),
        ids::TOOL => cat([
            vi(1),
            vi(0),
            string("minecraft:mineable/pickaxe"),
            vec![1],
            f32b(8.0),
            vec![0],
            f32b(1.0),
            vi(1),
            vec![1],
        ]),
        ids::WEAPON => cat([vi(1), f32b(0.5)]),
        ids::ATTACK_RANGE => cat([
            f32b(0.0),
            f32b(3.0),
            f32b(0.0),
            f32b(3.0),
            f32b(0.0),
            f32b(1.0),
        ]),
        ids::ENCHANTABLE => vi(15),
        ids::EQUIPPABLE => cat([vi(0), sound_or(), vec![0, 0, 0, 0, 0, 0, 0, 0], vec![1]]),
        ids::REPAIRABLE => id_set(),
        ids::GLIDER => vec![],
        ids::TOOLTIP_STYLE => string("minecraft:x"),
        ids::DEATH_PROTECTION => cat([vi(1), vec![2]]),
        ids::BLOCKS_ATTACKS => cat([
            f32b(90.0),
            f32b(1.0),
            vi(1),
            f32b(0.0),
            vec![0],
            f32b(0.0),
            f32b(1.0),
            f32b(0.0),
            f32b(0.0),
            f32b(0.0),
            vec![0, 0, 0],
        ]),
        ids::PIERCING_WEAPON => vec![1, 0, 0, 0],
        ids::KINETIC_WEAPON => cat([
            vi(1),
            vi(2),
            vec![1],
            vi(3),
            f32b(1.0),
            f32b(2.0),
            vec![0, 0],
            f32b(1.0),
            f32b(0.0),
            vec![0, 0],
        ]),
        ids::ATTACK_ANIMATION | ids::INTERACT_ANIMATION => cat([vi(1), vi(2)]),
        ids::ADDITIONAL_TRADE_COST => vi(1),
        ids::BLOCK_TRANSFORMER => vi(0),
        ids::VILLAGER_FOOD => vi(4),
        ids::STORED_ENCHANTMENTS => enchantment_sample(),
        ids::DYE => vi(1),
        ids::DYED_COLOR => i32b(0x00ff_00ff),
        ids::MAP_ID => vi(1),
        ids::MAP_DECORATIONS => empty_nbt(),
        ids::MAP_POST_PROCESSING => vi(1),
        ids::CHARGED_PROJECTILES | ids::BUNDLE_CONTENTS => cat([vi(1), template()]),
        ids::POTION_CONTENTS => cat([
            vec![1],
            vi(1),
            vec![1],
            i32b(0x1122_3344),
            vi(1),
            vi(1),
            vi(0),
            vi(200),
            vec![1],
            vec![0],
            vec![1],
            vec![0],
            vec![1],
            string("potion"),
        ]),
        ids::POTION_DURATION_SCALE => f32b(1.0),
        ids::SUSPICIOUS_STEW_EFFECTS => cat([vi(1), vi(1), vi(200)]),
        ids::WRITABLE_BOOK_CONTENT => cat([vi(1), string("page"), vec![1], string("filtered")]),
        ids::WRITTEN_BOOK_CONTENT => cat([
            string("T"),
            vec![0],
            string("A"),
            vi(1),
            vi(1),
            text("page"),
            vec![0],
            vec![1],
        ]),
        ids::TRIM => cat([
            id_or_inline(trim_material_inline()),
            id_or_inline(trim_pattern_inline()),
        ]),
        ids::DEBUG_STICK_STATE => empty_nbt(),
        ids::ENTITY_DATA => cat([vi(0), empty_nbt()]),
        ids::BUCKET_ENTITY_DATA => empty_nbt(),
        ids::BLOCK_ENTITY_DATA => cat([vi(0), empty_nbt()]),
        ids::INSTRUMENT => id_or_inline(instrument_inline()),
        ids::PROVIDES_TRIM_MATERIAL => id_or_inline(trim_material_inline()),
        ids::OMINOUS_BOTTLE_AMPLIFIER => vi(1),
        ids::JUKEBOX_PLAYABLE => id_or_inline(cat([sound_or(), text("song"), f32b(10.0), vi(1)])),
        ids::PROVIDES_BANNER_PATTERNS => id_set(),
        ids::RECIPES => empty_nbt(),
        ids::LODESTONE_TRACKER => {
            cat([vec![1], string("minecraft:overworld"), vec![0; 8], vec![0]])
        }
        ids::FIREWORK_EXPLOSION => cat([vi(0), vi(1), i32b(0x00ff_0000), vi(0), vec![1], vec![0]]),
        ids::FIREWORKS => cat([
            vi(1),
            vi(1),
            vi(0),
            vi(1),
            i32b(0x00ff_0000),
            vi(0),
            vec![1],
            vec![0],
        ]),
        ids::PROFILE => cat([
            vec![1],
            vec![0; 16],
            string("Steve"),
            vi(0),
            vec![0, 0, 0, 0],
        ]),
        ids::NOTE_BLOCK_SOUND => string("minecraft:x"),
        ids::BANNER_PATTERNS => cat([
            vi(1),
            id_or_inline(cat([string("minecraft:x"), string("key")])),
            vi(1),
        ]),
        ids::BASE_COLOR => vi(1),
        ids::POT_DECORATIONS => cat([vi(1), vi(1)]),
        ids::CONTAINER => cat([vi(2), vec![0], vec![1], template()]),
        ids::BLOCK_STATE => cat([vi(1), string("a"), string("b")]),
        ids::BEES => cat([vi(1), vi(0), empty_nbt(), vi(20), vi(0)]),
        ids::SULFUR_CUBE_CONTENT => template(),
        ids::LOCK => nbt(&Tag::String("key".to_owned())),
        ids::CONTAINER_LOOT => empty_nbt(),
        ids::BREAK_SOUND => sound_or(),
        ids::COMPOSTABLE => cat([vec![1], i32b(1)]),
        ids::COOKING_FUEL | ids::BREWING_FUEL => cat([vec![1], i32b(200), vec![1], f32b(1.0)]),
        ids::MOB_VISIBILITY => cat([id_set(), f32b(1.0)]),
        ids::PAINTING_VARIANT => {
            id_or_inline(cat([vi(1), vi(2), string("minecraft:x"), vec![0, 0]]))
        }
        ids::SIGN_TEXT_FRONT | ids::SIGN_TEXT_BACK => cat([
            text("a"),
            text("b"),
            text("c"),
            text("d"),
            vec![0],
            vi(0),
            vec![0],
        ]),
        ids::WAXED => vec![],
        // Every remaining component is a single VarInt: the animal/plant variant and dye
        // components, plus cushion color and pottery pattern.
        _ => vi(0),
    }
}

#[test]
fn every_component_type_round_trips_through_a_slot() {
    for id in 0..u16::try_from(COMPONENT_COUNT).expect("component count fits in u16") {
        let payload = sample_payload(id);
        let stack = stack_with_raw(id, payload.clone());
        let bytes = stack.encode_network().expect("sample encodes");
        let decoded = ItemStack::decode_network(&bytes)
            .unwrap_or_else(|error| panic!("component {id} does not parse: {error}"));
        assert!(
            decoded.components.contains(id),
            "component {id} ({:?}) was dropped",
            crate::component_name(id)
        );
        assert_eq!(
            decoded.encode_network().expect("decoded stack encodes"),
            bytes,
            "component {id} ({:?}) does not round-trip byte for byte\npayload: {payload:02x?}",
            crate::component_name(id),
        );
        assert_eq!(decoded.item, 1);
        assert_eq!(decoded.count, 1);
    }
}

#[test]
fn typed_components_decode_from_their_network_payloads() {
    let sharpness =
        registry_ids::enchantment_id("minecraft:sharpness").expect("sharpness is registered");
    let cases: Vec<(u16, Vec<u8>, Component)> = vec![
        (
            ids::CUSTOM_NAME,
            text("hi"),
            Component::CustomName(Text::text("hi")),
        ),
        (
            ids::ITEM_NAME,
            text("hi"),
            Component::ItemName(Text::text("hi")),
        ),
        (
            ids::LORE,
            cat([vi(2), text("a"), text("b")]),
            Component::Lore(vec![Text::text("a"), Text::text("b")]),
        ),
        (ids::DAMAGE, vi(7), Component::Damage(7)),
        (ids::MAX_DAMAGE, vi(1561), Component::MaxDamage(1561)),
        (ids::MAX_STACK_SIZE, vi(16), Component::MaxStackSize(16)),
        (ids::UNBREAKABLE, vec![], Component::Unbreakable),
        (
            ids::ENCHANTMENTS,
            cat([vi(1), vi(sharpness), vi(5)]),
            Component::Enchantments({
                let mut enchantments = Enchantments::new();
                enchantments.set("minecraft:sharpness", 5);
                enchantments
            }),
        ),
        (
            ids::STORED_ENCHANTMENTS,
            cat([vi(1), vi(sharpness), vi(5)]),
            Component::StoredEnchantments({
                let mut enchantments = Enchantments::new();
                enchantments.set("minecraft:sharpness", 5);
                enchantments
            }),
        ),
        (
            ids::CUSTOM_MODEL_DATA,
            cat([
                vi(1),
                f32b(1.5),
                vi(1),
                vec![1],
                vi(1),
                string("a"),
                vi(1),
                i32b(7),
            ]),
            Component::CustomModelData(CustomModelData {
                floats: vec![1.5],
                flags: vec![true],
                strings: vec!["a".to_owned()],
                colors: vec![7],
            }),
        ),
        (
            ids::DYED_COLOR,
            i32b(0x00ff_00ff),
            Component::DyedColor(0x00ff_00ff),
        ),
        (ids::RARITY, vi(3), Component::Rarity(Rarity::Epic)),
        (ids::REPAIR_COST, vi(3), Component::RepairCost(3)),
        (
            ids::CUSTOM_DATA,
            nbt(&Tag::Compound({
                let mut compound = Compound::new();
                compound.insert("a".to_owned(), Tag::Int(1));
                compound
            })),
            Component::CustomData({
                let mut compound = Compound::new();
                compound.insert("a".to_owned(), Tag::Int(1));
                compound
            }),
        ),
        (
            ids::FOOD,
            cat([vi(4), f32b(2.4), vec![1]]),
            Component::Food(Food {
                nutrition: 4,
                saturation: 2.4,
                can_always_eat: true,
            }),
        ),
        (
            ids::TOOLTIP_DISPLAY,
            cat([
                vec![1],
                vi(2),
                vi(i32::from(ids::ENCHANTMENTS)),
                vi(i32::from(ids::ATTRIBUTE_MODIFIERS)),
            ]),
            Component::TooltipDisplay(TooltipDisplay {
                hide_tooltip: true,
                hidden_components: vec![ids::ENCHANTMENTS, ids::ATTRIBUTE_MODIFIERS],
            }),
        ),
    ];
    for (id, payload, expected) in cases {
        let decoded = ItemStack::decode_network(&slot(id, payload)).expect("sample decodes");
        assert_eq!(
            decoded.components.get(id),
            Some(&expected),
            "component {id} did not decode to its typed variant"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Byte-exact vectors
// ---------------------------------------------------------------------------------------------

#[test]
fn hand_written_component_vectors() {
    let vectors: &[(u16, Vec<u8>)] = &[
        (ids::DAMAGE, vec![0x05]),
        (ids::MAX_STACK_SIZE, vec![0x10]),
        (ids::MAX_DAMAGE, vec![0x99, 0x0c]), // 1561
        (ids::UNBREAKABLE, vec![]),
        (ids::REPAIR_COST, vec![0x03]),
        (ids::RARITY, vec![0x03]), // epic
        (ids::ENCHANTMENT_GLINT_OVERRIDE, vec![0x01]),
        (ids::DYED_COLOR, vec![0x00, 0xff, 0x00, 0xff]),
        (ids::CUSTOM_NAME, vec![0x08, 0x00, 0x02, b'H', b'i']),
        (
            ids::LORE,
            vec![0x02, 0x08, 0x00, 0x01, b'a', 0x08, 0x00, 0x01, b'b'],
        ),
        (
            ids::FOOD,
            vec![0x04, 0x40, 0x19, 0x99, 0x9a, 0x01], // nutrition 4, 2.4, always eat
        ),
        (
            ids::CUSTOM_DATA,
            vec![0x0a, 0x03, 0x00, 0x01, b'a', 0x00, 0x00, 0x00, 0x01, 0x00],
        ),
        (
            ids::CUSTOM_MODEL_DATA,
            vec![
                0x01, 0x3f, 0x80, 0x00, 0x00, // floats: 1.0
                0x01, 0x01, // flags: true
                0x01, 0x01, b'a', // strings: "a"
                0x01, 0x00, 0x00, 0x00, 0x07, // colors: 7
            ],
        ),
        (ids::TOOLTIP_DISPLAY, vec![0x01, 0x01, 0x03]), // hide tooltip, hide damage
        (
            ids::ATTRIBUTE_MODIFIERS,
            cat([
                vec![0x01], // one modifier
                vec![0x00], // attribute id 0
                string("minecraft:x"),
                f64b(1.5),
                vec![0x01], // add_multiplied_base
                vec![0x01], // mainhand
                vec![0x00], // display: default
            ]),
        ),
    ];
    for (id, payload) in vectors {
        let bytes = slot(*id, payload.clone());
        let decoded = ItemStack::decode_network(&bytes)
            .unwrap_or_else(|error| panic!("vector for component {id} does not parse: {error}"));
        assert_eq!(
            decoded.encode_network().expect("decoded stack encodes"),
            bytes,
            "component {id} vector is not byte exact"
        );
    }

    // The typed decode of the hand-written names and food values.
    let name =
        ItemStack::decode_network(&slot(ids::CUSTOM_NAME, vec![0x08, 0x00, 0x02, b'H', b'i']))
            .expect("name decodes");
    assert_eq!(
        name.components.get(ids::CUSTOM_NAME),
        Some(&Component::CustomName(Text::text("Hi")))
    );
    let food =
        ItemStack::decode_network(&slot(ids::FOOD, vec![0x04, 0x40, 0x19, 0x99, 0x9a, 0x01]))
            .expect("food decodes");
    assert_eq!(
        food.components.get(ids::FOOD),
        Some(&Component::Food(Food {
            nutrition: 4,
            saturation: 2.4,
            can_always_eat: true,
        }))
    );
}

#[test]
fn empty_slot_structure() {
    assert_eq!(ItemStack::empty().encode_network().unwrap(), [0]);
    assert!(HashedSlot::decode_network(&[0]).unwrap().is_none());
    assert_eq!(encode_empty_hashed_slot(), [0]);
}

#[test]
fn full_diamond_sword_round_trips() {
    let mut stack = ItemStack::from_name("minecraft:diamond_sword").expect("sword is registered");
    let mut enchantments = Enchantments::new();
    enchantments.set("minecraft:sharpness", 5);
    enchantments.set("minecraft:unbreaking", 3);
    stack.components.insert(
        ids::ENCHANTMENTS,
        Component::Enchantments(enchantments.clone()),
    );
    stack.components.insert(
        ids::CUSTOM_NAME,
        Component::CustomName(Text::text("Excalibur")),
    );
    stack.components.insert(ids::DAMAGE, Component::Damage(7));
    stack.components.insert(
        ids::CUSTOM_DATA,
        Component::CustomData({
            let mut compound = Compound::new();
            compound.insert("owner".to_owned(), Tag::String("ferrite".to_owned()));
            compound
        }),
    );

    let bytes = stack.encode_network().expect("sword encodes");
    let decoded = ItemStack::decode_network(&bytes).expect("sword decodes");
    assert_eq!(decoded, stack);
    assert_eq!(decoded.encode_network().expect("re-encodes"), bytes);
    assert_eq!(
        decoded.components.get(ids::ENCHANTMENTS),
        Some(&Component::Enchantments(enchantments))
    );
    assert_eq!(
        decoded.components.get(ids::CUSTOM_NAME),
        Some(&Component::CustomName(Text::text("Excalibur")))
    );
    assert_eq!(
        decoded.components.get(ids::DAMAGE),
        Some(&Component::Damage(7))
    );
}

#[test]
fn nested_templates_inside_bundle_and_container_round_trip() {
    // bundle_contents: two templates; the first carries a nested custom_name component.
    let bundle = cat([
        vi(2),
        vi(1),
        vi(2),
        vi(1),
        vi(0),
        vi(i32::from(ids::CUSTOM_NAME)),
        text("nested"),
        vi(55), // minecraft:dirt
        vi(1),
        vi(0),
        vi(0),
    ]);
    let bundle_stack = stack_with_raw(ids::BUNDLE_CONTENTS, bundle);
    let bundle_bytes = bundle_stack.encode_network().expect("bundle encodes");
    let bundle_decoded = ItemStack::decode_network(&bundle_bytes).expect("bundle decodes");
    assert_eq!(bundle_decoded, bundle_stack);
    assert_eq!(
        bundle_decoded.encode_network().expect("re-encodes"),
        bundle_bytes
    );

    // container: one empty optional and one template that itself contains a bundle_contents
    // component, so the parser must recurse through template -> component -> template.
    let inner = cat([vi(1), template()]);
    let container = cat([
        vi(2),
        vec![0],
        vec![1],
        vi(1),
        vi(1),
        vi(1),
        vi(0),
        vi(i32::from(ids::BUNDLE_CONTENTS)),
        inner,
    ]);
    let container_stack = stack_with_raw(ids::CONTAINER, container);
    let container_bytes = container_stack.encode_network().expect("container encodes");
    let container_decoded = ItemStack::decode_network(&container_bytes).expect("container decodes");
    // The container is a typed component now: an empty slot and a one-item template whose own
    // bundle contents stay raw. The bytes it re-encodes to are exactly the bytes received.
    let Some(Component::Container(slots)) = container_decoded.components.get(ids::CONTAINER) else {
        panic!("container decodes to the typed component");
    };
    assert_eq!(slots.len(), 2);
    assert!(slots[0].is_empty());
    assert_eq!(
        slots[1].components.get(ids::BUNDLE_CONTENTS),
        Some(&Component::Raw(vec![1, 1, 1, 0, 0]))
    );
    assert_eq!(
        container_decoded.encode_network().expect("re-encodes"),
        container_bytes
    );
}

#[test]
fn hashed_slot_round_trips_and_keeps_opaque_hashes() {
    let slot = HashedSlot {
        item: 1,
        count: 2,
        added: vec![(ids::DAMAGE, 0x1234_5678), (ids::ENCHANTMENTS, -1)],
        removed: vec![ids::FOOD],
    };
    let bytes = slot.encode_network().expect("hashed slot encodes");
    assert_eq!(bytes[0], 1);
    assert_eq!(HashedSlot::decode_network(&bytes).unwrap(), Some(slot));

    // An absent hashed slot is one zero byte.
    assert_eq!(HashedSlot::decode_network(&[0]).unwrap(), None);
    assert!(HashedSlot::decode_network(&[0, 1]).is_err());
}

#[test]
fn malformed_slots_are_errors_not_panics() {
    assert!(ItemStack::decode_network(&[]).is_err());
    assert!(ItemStack::decode_network(&[1]).is_err()); // no item id
    assert!(ItemStack::decode_network(&[1, 1]).is_err()); // no counts
    assert!(ItemStack::decode_network(&[1, 1, 0, 0, 9]).is_err()); // trailing byte
    assert!(ItemStack::decode_network(&[1, 0xff, 0x7f, 0, 0]).is_err()); // unknown item id
    assert!(ItemStack::decode_network(&[1, 1, 1, 0, 0x7f]).is_err()); // unknown component id
    assert!(ItemStack::decode_network(&[0, 1]).is_err()); // bytes after an empty slot
    assert_eq!(
        ItemStack::decode_network(&[0xff, 0xff, 0xff, 0xff, 0x0f]).unwrap(),
        ItemStack::empty()
    );
}

#[test]
fn truncated_payloads_never_panic() {
    let stack = stack_with_raw(ids::CUSTOM_DATA, sample_payload(ids::CUSTOM_DATA));
    let bytes = stack.encode_network().unwrap();
    for len in 0..bytes.len() {
        let _ = ItemStack::decode_network(&bytes[..len]);
    }
    let stack = stack_with_raw(ids::LORE, sample_payload(ids::LORE));
    let bytes = stack.encode_network().unwrap();
    for len in 0..bytes.len() {
        let _ = ItemStack::decode_network(&bytes[..len]);
    }
}

/// Deterministic random bytes: the decoders must return errors, never panic or allocate
/// unboundedly.
#[test]
fn random_bytes_never_panic() {
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..20_000 {
        let len = usize::try_from(next() % 80).expect("length fits") + 1;
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            bytes.push(next() as u8);
        }
        let _ = ItemStack::decode_network(&bytes);
        let _ = ItemStack::decode_network_prefix(&bytes);
        let _ = HashedSlot::decode_network(&bytes);
    }
}

#[test]
fn every_variant_id_is_a_single_varint() {
    for name in [
        "minecraft:villager/variant",
        "minecraft:wolf/collar",
        "minecraft:sheep/color",
        "minecraft:cushion/color",
        "minecraft:provides_pottery_pattern",
    ] {
        let id = crate::component_id(name).expect("variant component is registered");
        let stack = stack_with_raw(id, vec![0x05]);
        let bytes = stack.encode_network().unwrap();
        assert_eq!(ItemStack::decode_network(&bytes).unwrap(), stack);
    }
}

#[test]
fn typed_attribute_modifiers_and_tool_round_trip() {
    let modifiers = vec![AttributeModifier {
        attribute: "minecraft:attack_damage".to_owned(),
        id: "minecraft:base_attack_damage".to_owned(),
        amount: 2.0,
        operation: AttributeOperation::AddValue,
        slot: EquipmentSlot::Mainhand,
    }];
    let mut stack = ItemStack::new(1);
    stack.components.insert(
        ids::ATTRIBUTE_MODIFIERS,
        Component::AttributeModifiers(modifiers.clone()),
    );
    let bytes = stack.encode_network().unwrap();
    assert_eq!(
        ItemStack::decode_network(&bytes)
            .unwrap()
            .components
            .get(ids::ATTRIBUTE_MODIFIERS),
        Some(&Component::AttributeModifiers(modifiers))
    );

    let tool = Tool {
        rules: vec![ToolRule {
            blocks: "minecraft:stone".to_owned(),
            speed: Some(4.0),
            correct_for_drops: Some(true),
        }],
        default_mining_speed: 1.0,
        damage_per_block: 2,
        can_destroy_blocks_in_creative: true,
    };
    let mut stack = ItemStack::new(1);
    stack
        .components
        .insert(ids::TOOL, Component::Tool(tool.clone()));
    let bytes = stack.encode_network().unwrap();
    assert_eq!(
        ItemStack::decode_network(&bytes)
            .unwrap()
            .components
            .get(ids::TOOL),
        Some(&Component::Tool(tool))
    );
}

#[test]
fn potion_contents_round_trip() {
    let contents = crate::PotionContents {
        potion: Some("minecraft:swiftness".to_owned()),
        custom_color: Some(0x1122_3344),
        custom_effects: vec![crate::PotionEffect {
            effect: "minecraft:jump_boost".to_owned(),
            amplifier: 1,
            duration: 200,
            ambient: true,
            visible: false,
            show_icon: true,
        }],
        custom_name: Some("Fast".to_owned()),
    };
    let mut stack = ItemStack::new(1);
    stack.components.insert(
        ids::POTION_CONTENTS,
        Component::PotionContents(contents.clone()),
    );
    let bytes = stack.encode_network().unwrap();
    assert_eq!(
        ItemStack::decode_network(&bytes)
            .unwrap()
            .components
            .get(ids::POTION_CONTENTS),
        Some(&Component::PotionContents(contents))
    );
}

#[test]
fn opaque_components_cannot_be_encoded() {
    let mut stack = ItemStack::new(1);
    stack
        .components
        .insert(ids::DAMAGE, Component::Opaque(Tag::Int(3)));
    assert!(matches!(
        stack.encode_network(),
        Err(WireError::NotRepresentable("opaque"))
    ));
}

#[cfg(feature = "protocol")]
#[test]
fn protocol_traits_round_trip() {
    use ferrite_protocol::{Decode, Encode};

    let mut stack = ItemStack::from_name("minecraft:diamond_sword").unwrap();
    stack.components.insert(ids::DAMAGE, Component::Damage(3));
    let mut bytes = Vec::new();
    stack.encode(&mut bytes);
    let mut input = bytes.as_slice();
    assert_eq!(ItemStack::decode(&mut input).unwrap(), stack);
    assert!(input.is_empty());
}

#[cfg(feature = "protocol")]
#[test]
fn optional_hashed_slot_matches_the_wire_form() {
    use ferrite_protocol::{Decode, Encode};

    let slot = HashedSlot {
        item: 1,
        count: 5,
        added: vec![(ids::DAMAGE, -7)],
        removed: vec![ids::FOOD],
    };
    let mut bytes = Vec::new();
    Some(slot.clone()).encode(&mut bytes);
    assert_eq!(bytes, slot.encode_network().unwrap());
    let mut input = bytes.as_slice();
    assert_eq!(
        Option::<HashedSlot>::decode(&mut input).unwrap(),
        Some(slot)
    );
    assert!(input.is_empty());
    let mut bytes = Vec::new();
    None::<HashedSlot>.encode(&mut bytes);
    assert_eq!(bytes, encode_empty_hashed_slot());
}

#[test]
fn a_trim_is_sent_as_two_registry_references() {
    let trim = Component::Trim(crate::Trim {
        material: "minecraft:amethyst".to_owned(),
        pattern: "minecraft:bolt".to_owned(),
    });
    let material = registry_ids::trim_material_id("minecraft:amethyst").expect("registered");
    let pattern = registry_ids::trim_pattern_id("minecraft:bolt").expect("registered");
    let mut expected = Vec::new();
    write_varint(material + 1, &mut expected);
    write_varint(pattern + 1, &mut expected);
    assert_eq!(encode_component(&trim).unwrap(), expected);
    // Sorted file names are the registry ids: amethyst is the first material.
    assert_eq!(material, 0);
}

#[test]
fn an_unknown_trim_material_cannot_be_sent() {
    let trim = Component::Trim(crate::Trim {
        material: "minecraft:nothing".to_owned(),
        pattern: "minecraft:bolt".to_owned(),
    });
    assert!(matches!(
        encode_component(&trim),
        Err(WireError::UnknownRegistry { .. })
    ));
}

#[test]
fn a_lodestone_tracker_is_a_global_pos_and_a_flag() {
    let tracker = Component::LodestoneTracker(crate::LodestoneTracker {
        target: Some(("minecraft:overworld".to_owned(), [-3, 64, 7])),
        tracked: true,
    });
    let bytes = encode_component(&tracker).unwrap();
    let mut expected = vec![1];
    write_varint("minecraft:overworld".len() as i32, &mut expected);
    expected.extend_from_slice(b"minecraft:overworld");
    let packed: i64 = ((-3_i64 & 0x3FF_FFFF) << 38) | ((7 & 0x3FF_FFFF) << 12) | 64;
    expected.extend_from_slice(&packed.to_be_bytes());
    expected.push(1);
    assert_eq!(bytes, expected);
    let unbound = Component::LodestoneTracker(crate::LodestoneTracker {
        target: None,
        tracked: false,
    });
    assert_eq!(encode_component(&unbound).unwrap(), [0, 0]);
}
