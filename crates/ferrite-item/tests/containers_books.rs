//! `minecraft:container`, `minecraft:writable_book_content` and
//! `minecraft:written_book_content` as typed components: each has an NBT save form and a
//! network form, so a stack that went through the save format can still be sent to a client
//! and a stack that came from a client can still be saved.

use ferrite_item::{
    Component, Filterable, ItemStack, WritableBookContent, WrittenBookContent, component_ids as ids,
};
use ferrite_nbt::{Compound, List, Tag};
use ferrite_text::Component as Text;

fn stone(count: i32) -> ItemStack {
    ItemStack::from_name("stone")
        .unwrap()
        .copy_with_count(count)
}

fn named_sword() -> ItemStack {
    let mut sword = ItemStack::from_name("diamond_sword").unwrap();
    sword.components.insert(
        ids::CUSTOM_NAME,
        Component::CustomName(Text::text("Édge ⚔")),
    );
    sword
}

fn shulker(contents: Vec<ItemStack>) -> ItemStack {
    let mut shulker = ItemStack::from_name("red_shulker_box").unwrap();
    shulker
        .components
        .insert(ids::CONTAINER, Component::Container(contents));
    shulker
}

/// Saves and reloads a stack through the NBT item format.
fn through_nbt(stack: &ItemStack) -> ItemStack {
    ItemStack::from_nbt(&stack.to_nbt().expect("save")).expect("load")
}

/// Sends a stack over the wire and reads it back.
fn through_wire(stack: &ItemStack) -> ItemStack {
    let bytes = stack.encode_network().expect("encode");
    ItemStack::decode_network(&bytes).expect("decode")
}

#[test]
fn a_container_survives_the_wire() {
    let stack = shulker(vec![
        stone(3),
        ItemStack::empty(),
        named_sword(),
        ItemStack::empty(),
    ]);
    let back = through_wire(&stack);
    assert_eq!(back, stack);
    assert!(matches!(
        back.components.get(ids::CONTAINER),
        Some(Component::Container(_))
    ));
}

#[test]
fn a_container_survives_the_save_format_with_sparse_slots() {
    let mut contents = vec![ItemStack::empty(); 27];
    contents[0] = stone(64);
    contents[13] = named_sword();
    contents[26] = stone(1);
    let stack = shulker(contents);
    let tag = stack.to_nbt().unwrap();
    // The save format lists only the occupied slots.
    let Some(Tag::Compound(components)) = tag.get("components") else {
        panic!("components compound");
    };
    let Some(Tag::List(List::Compound(slots))) = components.get("minecraft:container") else {
        panic!("container list");
    };
    let numbers: Vec<_> = slots
        .iter()
        .map(|entry| entry.get("slot").cloned())
        .collect();
    assert_eq!(numbers, [Tag::Int(0), Tag::Int(13), Tag::Int(26)].map(Some));
    assert_eq!(through_nbt(&stack), stack);
}

#[test]
fn a_saved_container_can_be_sent_to_a_client_and_back() {
    // The regression: an item loaded from the save format used to carry an opaque container,
    // which has no network form, so sending it panicked or stripped the contents.
    let stack = through_nbt(&shulker(vec![stone(5), named_sword()]));
    let sent = stack
        .encode_network()
        .expect("a saved container is sendable");
    let from_client = ItemStack::decode_network(&sent).unwrap();
    assert_eq!(from_client, stack);
    // And a container received from a client can be saved.
    let saved = through_nbt(&from_client);
    assert_eq!(saved, stack);
}

#[test]
fn nested_containers_round_trip_both_ways() {
    let inner = shulker(vec![stone(2)]);
    let mut bundle = ItemStack::from_name("chest").unwrap();
    bundle
        .components
        .insert(ids::CONTAINER, Component::Container(vec![inner]));
    assert_eq!(through_nbt(&bundle), bundle);
    assert_eq!(through_wire(&bundle), bundle);
}

#[test]
fn a_malformed_saved_container_is_kept_verbatim_not_dropped() {
    let mut bad_slot = Compound::new();
    bad_slot.insert("slot".into(), Tag::Int(300));
    bad_slot.insert("item".into(), Tag::Compound(stone(1).to_nbt().unwrap()));
    let mut duplicate = Compound::new();
    duplicate.insert("slot".into(), Tag::Int(1));
    duplicate.insert("item".into(), Tag::Compound(stone(1).to_nbt().unwrap()));
    for malformed in [
        Tag::String("not a list".into()),
        Tag::List(List::Compound(vec![bad_slot])),
        Tag::List(List::Compound(vec![duplicate.clone(), duplicate])),
        Tag::List(List::Compound(vec![Compound::new()])),
    ] {
        let mut components = Compound::new();
        components.insert("minecraft:container".into(), malformed.clone());
        let mut root = Compound::new();
        root.insert("id".into(), Tag::String("minecraft:red_shulker_box".into()));
        root.insert("count".into(), Tag::Int(1));
        root.insert("components".into(), Tag::Compound(components));
        let stack = ItemStack::from_nbt(&root).unwrap();
        assert!(
            matches!(
                stack.components.get(ids::CONTAINER),
                Some(Component::Opaque(_))
            ),
            "{malformed:?} must stay opaque"
        );
        // Saved again it is exactly what was read.
        assert_eq!(stack.to_nbt().unwrap(), root);
    }
}

#[test]
fn truncated_or_oversized_wire_containers_are_errors_not_panics() {
    let stack = shulker(vec![stone(3), named_sword()]);
    let bytes = stack.encode_network().unwrap();
    for cut in 0..bytes.len() {
        let _ = ItemStack::decode_network(&bytes[..cut]);
    }
    // 257 entries is over vanilla's 256 limit: refused on the way out...
    let too_many = shulker(vec![ItemStack::empty(); 257]);
    assert!(too_many.encode_network().is_err());
    // ...and on the way in (the same stack with the length patched past the limit).
    let ok = shulker(vec![ItemStack::empty(); 256]);
    let mut bytes = ok.encode_network().unwrap();
    // slot: count, item, added=1, removed=0, component id, then the list length (256 = 0x80 0x02).
    let at = bytes
        .windows(2)
        .position(|pair| pair == [0x80, 0x02])
        .expect("the 256 length prefix");
    bytes[at] = 0x81; // 257
    assert!(ItemStack::decode_network(&bytes).is_err());
}

fn book_pages() -> Vec<Filterable<String>> {
    vec![
        Filterable {
            raw: "Seite 1: Grüße 日本語 😀".into(),
            filtered: None,
        },
        Filterable {
            raw: "raw \u{a7}c page".into(),
            filtered: Some("filtered page".into()),
        },
    ]
}

#[test]
fn a_writable_book_keeps_its_pages_everywhere() {
    let mut book = ItemStack::from_name("writable_book").unwrap();
    book.components.insert(
        ids::WRITABLE_BOOK_CONTENT,
        Component::WritableBookContent(WritableBookContent {
            pages: book_pages(),
        }),
    );
    assert_eq!(through_nbt(&book), book);
    assert_eq!(through_wire(&book), book);
    assert_eq!(through_nbt(&through_wire(&book)), book);
}

#[test]
fn a_written_book_keeps_title_author_generation_pages_and_resolved() {
    let mut book = ItemStack::from_name("written_book").unwrap();
    book.components.insert(
        ids::WRITTEN_BOOK_CONTENT,
        Component::WrittenBookContent(WrittenBookContent {
            title: Filterable {
                raw: "Das Buch ✨".into(),
                filtered: Some("Buch".into()),
            },
            author: "Álvaro".into(),
            generation: 2,
            pages: vec![
                Filterable {
                    raw: Text::text("erste Seite 😀"),
                    filtered: None,
                },
                Filterable {
                    raw: Text::text("zweite"),
                    filtered: Some(Text::text("2.")),
                },
            ],
            resolved: true,
        }),
    );
    assert_eq!(through_nbt(&book), book);
    assert_eq!(through_wire(&book), book);
    assert_eq!(through_nbt(&through_wire(&book)), book);
}

#[test]
fn an_empty_book_and_default_fields_round_trip() {
    let mut book = ItemStack::from_name("written_book").unwrap();
    book.components.insert(
        ids::WRITTEN_BOOK_CONTENT,
        Component::WrittenBookContent(WrittenBookContent {
            title: Filterable {
                raw: String::new(),
                filtered: None,
            },
            author: String::new(),
            generation: 0,
            pages: Vec::new(),
            resolved: false,
        }),
    );
    assert_eq!(through_nbt(&book), book);
    assert_eq!(through_wire(&book), book);
}
