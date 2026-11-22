use ferrite_nbt::{Compound, List, Tag, binary};
#[test]
fn binary_roundtrip_named_and_network() {
    let mut map = Compound::new();
    map.insert("a\0??".into(), Tag::String("\0??".into()));
    map.insert("list".into(), Tag::List(List::Int(vec![1, -2])));
    let tag = Tag::Compound(map);
    let bytes = binary::write_named("root", &tag).unwrap();
    assert_eq!(
        binary::read_named(&bytes).unwrap(),
        ("root".into(), tag.clone())
    );
    let bytes = binary::write_network(&tag).unwrap();
    assert_eq!(binary::read_network(&bytes).unwrap(), tag);
}
#[test]
fn malformed_input_does_not_panic() {
    let mut seed = 1u32;
    for len in 0..256 {
        let mut bytes = vec![0; len];
        for b in &mut bytes {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            *b = (seed >> 24) as u8;
        }
        let _ = binary::read_named(&bytes);
    }
    assert!(binary::read_named(&[10, 0, 0, 7, 0, 1, b'a', 0x7f, 0xff, 0xff, 0xff]).is_err());
}

#[test]
fn compression_and_all_types_roundtrip() {
    let mut root = Compound::new();
    for (name, tag) in [
        ("byte", Tag::Byte(-1)),
        ("short", Tag::Short(-300)),
        ("int", Tag::Int(-12345)),
        ("long", Tag::Long(-123456789)),
        ("float", Tag::Float(1.25)),
        ("double", Tag::Double(-2.5)),
        ("bytes", Tag::ByteArray(vec![-1, 0, 1])),
        ("ints", Tag::IntArray(vec![-2, 4])),
        ("longs", Tag::LongArray(vec![-2, 4])),
        ("empty", Tag::List(List::String(vec![]))),
        ("nested", Tag::List(List::Compound(vec![Compound::new()]))),
    ] {
        root.insert(name.into(), tag);
    }
    let tag = Tag::Compound(root);
    for compression in [
        binary::Compression::None,
        binary::Compression::Gzip,
        binary::Compression::Zlib,
    ] {
        let data = binary::write_compressed("", &tag, compression).unwrap();
        assert_eq!(
            binary::read_compressed(&data, compression).unwrap(),
            (String::new(), tag.clone())
        );
    }
}
#[test]
fn reject_invalid_utf8_and_lengths() {
    for input in [
        &[8, 0, 0, 0, 1, 0][..],
        &[9, 0, 0, 0, 0, 0, 1][..],
        &[11, 0, 0, 0, 0, 0, 2, 0][..],
    ] {
        assert!(binary::read_named(input).is_err());
    }
}

#[test]
fn snbt_roundtrip() {
    let source = r#"{name:"hello \"world\"",byte:1b,short:-2s,int:3,long:4L,float:1.5f,double:2.5d,bytes:[B;1b,-2b],ints:[I;1,2],longs:[L;1L,2L],list:[{x:1},{x:2}]}"#;
    let tag = ferrite_nbt::snbt::parse(source).unwrap();
    let formatted = ferrite_nbt::snbt::format(&tag);
    assert_eq!(ferrite_nbt::snbt::parse(&formatted).unwrap(), tag);
}

#[test]
fn rejects_large_list_even_when_input_is_long_enough() {
    let mut data = vec![9, 0, 0, 1];
    data.extend_from_slice(&1_000_001i32.to_be_bytes());
    data.resize(data.len() + 1_000_001, 0);
    assert!(binary::read_named(&data).is_err());
}

#[test]
fn modern_snbt_literals() {
    use ferrite_nbt::snbt::parse;
    assert_eq!(parse("0xFFub").unwrap(), Tag::Byte(-1));
    assert_eq!(parse("0b1111_1111b").unwrap(), Tag::Byte(-1));
    assert_eq!(parse("240ub").unwrap(), Tag::Byte(-16));
    assert_eq!(parse("0xAB").unwrap(), Tag::Int(171));
    assert_eq!(parse("0b").unwrap(), Tag::Byte(0));
    assert_eq!(parse("1_2.3_4_5f").unwrap(), Tag::Float(12.345));
    assert_eq!(
        parse(r#""\u2603\n\U0001F600""#).unwrap(),
        Tag::String("\u{2603}\n\u{1f600}".into())
    );
    assert_eq!(parse("[B;1,2,]").unwrap(), Tag::ByteArray(vec![1, 2]));
    assert_eq!(parse("[I;1b,2s,3,]").unwrap(), Tag::IntArray(vec![1, 2, 3]));
    let mixed = parse("['a',{b:3},1]").unwrap();
    let Tag::List(List::Compound(items)) = mixed else {
        panic!("expected wrapper compounds")
    };
    assert_eq!(items[0].get(""), Some(&Tag::String("a".into())));
    assert_eq!(items[1].get("b"), Some(&Tag::Int(3)));
    assert_eq!(items[2].get(""), Some(&Tag::Int(1)));
    assert!(parse("240sb").is_err());
    assert!(parse("1e1000").is_err());
    assert!(parse("[1,,]").is_err());
}

#[test]
fn snbt_operations_named_escapes_and_invalid_numbers() {
    use ferrite_nbt::snbt::parse;
    assert_eq!(
        parse(r#""\N{SNOWMAN}\x21""#).unwrap(),
        Tag::String("\u{2603}!".into())
    );
    assert_eq!(parse("bool(3)").unwrap(), Tag::Byte(1));
    assert_eq!(
        parse(r#"uuid("00112233-4455-6677-8899-aabbccddeeff")"#).unwrap(),
        Tag::IntArray(vec![
            0x00112233,
            0x44556677,
            0x8899aabbu32 as i32,
            0xccddeeffu32 as i32
        ])
    );
    for s in ["0x", "012", "-", "0x_ff", "1e999", "[B;256]"] {
        assert!(parse(s).is_err(), "{s}");
    }
}

#[cfg(feature = "serde")]
#[test]
fn serde_struct_roundtrip() {
    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Example {
        name: String,
        health: i32,
        active: bool,
        values: Vec<i64>,
        missing: Option<i32>,
    }
    let expected = Example {
        name: "Steve".into(),
        health: 20,
        active: true,
        values: vec![1, 2],
        missing: None,
    };
    let tag = ferrite_nbt::serde::to_tag(&expected).unwrap();
    let bytes = binary::write_named("", &tag).unwrap();
    let actual = ferrite_nbt::serde::from_tag(binary::read_named(&bytes).unwrap().1).unwrap();
    assert_eq!(expected, actual);
}

#[cfg(feature = "serde")]
#[test]
fn serde_unsigned_and_enums_roundtrip() {
    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    enum Mode {
        Idle,
        Active(i32),
    }
    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Value {
        value: u8,
        mode: Mode,
    }
    for mode in [Mode::Idle, Mode::Active(42)] {
        let expected = Value { value: 255, mode };
        assert_eq!(
            ferrite_nbt::serde::from_tag::<Value>(ferrite_nbt::serde::to_tag(&expected).unwrap())
                .unwrap(),
            expected
        );
    }
}

#[cfg(feature = "serde")]
#[test]
fn serde_char_and_optional_roundtrip() {
    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Value {
        character: char,
        optional: Option<i32>,
    }
    for optional in [None, Some(4)] {
        let expected = Value {
            character: 'Z',
            optional,
        };
        assert_eq!(
            ferrite_nbt::serde::from_tag::<Value>(ferrite_nbt::serde::to_tag(&expected).unwrap())
                .unwrap(),
            expected
        );
    }
}

#[test]
fn snbt_surrogate_pairs() {
    assert_eq!(
        ferrite_nbt::snbt::parse(r#""\uD83D\uDE00""#).unwrap(),
        Tag::String("\u{1f600}".into())
    );
    assert!(ferrite_nbt::snbt::parse(r#""\uD800x""#).is_err());
}

#[test]
fn write_network_compound_into_skipping_matches_filtered_compound() {
    let mut full = ferrite_nbt::Compound::new();
    full.insert("x".into(), Tag::Int(1));
    full.insert("y".into(), Tag::Int(64));
    full.insert("z".into(), Tag::Int(-2));
    full.insert("CustomName".into(), Tag::String("Test".into()));

    let mut filtered = full.clone();
    filtered.shift_remove("x");
    filtered.shift_remove("y");
    filtered.shift_remove("z");
    let expected = binary::write_network(&Tag::Compound(filtered)).unwrap();

    let mut actual = vec![0x5A];
    binary::write_network_compound_into_skipping(&mut actual, &full, &["x", "y", "z"]).unwrap();
    assert_eq!(actual[0], 0x5A);
    assert_eq!(&actual[1..], expected.as_slice());
}
