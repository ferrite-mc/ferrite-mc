use ferrite_nbt::{Compound, List, Tag, binary};
use ferrite_world::{
    BlockEntity, BlockInfo, BlockStateRegistry, LightData, LightSection, RegionFile,
    RuntimeRegistry, apply_light_to_nbt, chunk_from_nbt, chunk_to_nbt, light_from_nbt,
};

/// The fixture world is generated locally and gitignored, so CI skips these tests.
fn fixture(full: bool) -> Option<(Tag, RuntimeRegistry)> {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3");
    let path = base.join("fixture-world/world/dimensions/minecraft/overworld/region/r.0.0.mca");
    if !path.exists() {
        eprintln!("SKIP fixture-world test: {} absent", path.display());
        return None;
    }
    let registry = RuntimeRegistry::load(base.join("generated")).unwrap();
    let region = RegionFile::open(path, 0, 0).unwrap();
    for z in 0..32 {
        for x in 0..32 {
            if let Some(bytes) = region.read_chunk(x, z).unwrap() {
                let (_, tag) = binary::read_named(&bytes).unwrap();
                if let Tag::Compound(root) = &tag
                    && (root.get("Status") == Some(&Tag::String("minecraft:full".into()))) == full
                {
                    return Some((tag, registry));
                }
            }
        }
    }
    panic!("fixture lacks requested chunk status");
}

#[test]
fn full_chunk_preserves_entities_biomes_light_and_unknown_metadata() {
    let Some((mut input, registry)) = fixture(true) else {
        return;
    };
    let Tag::Compound(root) = &mut input else {
        unreachable!()
    };
    root.insert(
        "ferrite_test_unknown".into(),
        Tag::String("preserved".into()),
    );
    let Tag::List(List::Compound(sections)) = root.get_mut("sections").unwrap() else {
        panic!()
    };
    sections[0].insert("ferrite_test_section".into(), Tag::Int(73));
    sections[0].insert("SkyLight".into(), Tag::ByteArray(vec![17; 2048]));
    sections[0].insert("BlockLight".into(), Tag::ByteArray(vec![34; 2048]));
    let originals = sections.clone();
    let mut chunk = chunk_from_nbt(&input, -64, 384, &registry).unwrap();
    let water = ferrite_data::blocks::default_state("water").unwrap() as u32;
    let slab = ferrite_data::blocks::state_id(
        "stone_slab",
        &[("type", "bottom"), ("waterlogged", "true")],
    )
    .unwrap() as u32;
    let lava = ferrite_data::blocks::default_state("lava").unwrap() as u32;
    let grass = ferrite_data::blocks::default_state("grass_block").unwrap() as u32;
    chunk.set_block(0, 300, 0, lava, &registry).unwrap();
    chunk.set_block(1, 300, 0, grass, &registry).unwrap();
    assert!(chunk.sections()[22].is_randomly_ticking());
    assert!(chunk.sections()[22].randomly_ticking_block_count() > 0);
    assert!(chunk.sections()[22].randomly_ticking_fluid_count() > 0);
    chunk.set_block(0, 319, 0, water, &registry).unwrap();
    chunk.set_block(1, 319, 0, slab, &registry).unwrap();
    assert_eq!(chunk.sections().last().unwrap().fluid_count(), 2);
    let mut data = Compound::new();
    data.insert("CustomName".into(), Tag::String("example".into()));
    chunk.block_entities.insert(
        (1, 64, 2),
        BlockEntity {
            type_id: registry.entity_id("minecraft:chest").unwrap(),
            data,
        },
    );
    let encoded = chunk_to_nbt(&chunk, &registry).unwrap();
    let Tag::Compound(root) = &encoded else {
        panic!()
    };
    assert_eq!(
        root.get("ferrite_test_unknown"),
        Some(&Tag::String("preserved".into()))
    );
    let Tag::List(List::Compound(sections)) = root.get("sections").unwrap() else {
        panic!()
    };
    for old in originals {
        let new = sections
            .iter()
            .find(|s| s.get("Y") == old.get("Y"))
            .unwrap();
        for key in ["SkyLight", "BlockLight", "ferrite_test_section"] {
            assert_eq!(new.get(key), old.get(key));
        }
    }
    let restored = chunk_from_nbt(&encoded, -64, 384, &registry).unwrap();
    assert_eq!(restored.sections().last().unwrap().fluid_count(), 2);
    assert_eq!(
        restored.block_entities[&(1, 64, 2)].data.get("CustomName"),
        Some(&Tag::String("example".into()))
    );
    for (a, b) in chunk.sections().iter().zip(restored.sections()) {
        for i in 0..64 {
            assert_eq!(a.biomes().get(i), b.biomes().get(i));
        }
        assert_eq!(a.fluid_count(), b.fluid_count());
        assert_eq!(
            b.block_count(),
            (0..4096)
                .filter(|&i| !registry.flags(b.blocks().get(i).unwrap()).air)
                .count() as u16
        );
        let random_blocks = (0..4096)
            .filter(|&i| registry.randomly_ticking(b.blocks().get(i).unwrap()))
            .count() as u16;
        let random_fluids = (0..4096)
            .filter(|&i| registry.fluid_randomly_ticking(b.blocks().get(i).unwrap()))
            .count() as u16;
        assert_eq!(b.randomly_ticking_block_count(), random_blocks);
        assert_eq!(b.randomly_ticking_fluid_count(), random_fluids);
    }
}

fn light_pattern(section: usize) -> LightSection {
    match section % 4 {
        0 => LightSection::Unknown,
        1 => LightSection::Empty,
        _ => {
            let mut data = Box::new([0u8; 2048]);
            for (index, byte) in data.iter_mut().enumerate() {
                *byte = (index as u8).wrapping_mul(37).wrapping_add(section as u8);
            }
            LightSection::Data(data)
        }
    }
}

#[test]
fn light_arrays_roundtrip_through_chunk_nbt() {
    const SECTIONS: usize = 26;
    const MIN_SECTION: i32 = -4;
    let mut root = Compound::new();
    root.insert("DataVersion".into(), Tag::Int(ferrite_world::DATA_VERSION));
    // A stored section the writer has to merge into instead of appending a duplicate.
    let mut existing = Compound::new();
    existing.insert("Y".into(), Tag::Byte(0));
    existing.insert("SkyLight".into(), Tag::ByteArray(vec![99; 2048]));
    existing.insert("BlockLight".into(), Tag::ByteArray(vec![-99; 2048]));
    root.insert("sections".into(), Tag::List(List::Compound(vec![existing])));
    let mut tag = Tag::Compound(root);

    let light = LightData {
        sky: (0..SECTIONS).map(light_pattern).collect(),
        block: (0..SECTIONS)
            .map(|index| light_pattern(index + 2))
            .collect(),
    };
    apply_light_to_nbt(&mut tag, &light, MIN_SECTION).unwrap();

    // Go through the real binary codec, not just the in-memory tag.
    let bytes = binary::write_named("", &tag).unwrap();
    let (_, decoded) = binary::read_named(&bytes).unwrap();
    let restored = light_from_nbt(&decoded, MIN_SECTION, SECTIONS).unwrap();
    assert_eq!(restored.sky, light.sky);
    assert_eq!(restored.block, light.block);

    // The sentinel sections below and above the column exist only because light needs them.
    let Tag::Compound(root) = &decoded else {
        panic!("chunk root")
    };
    let Tag::List(List::Compound(sections)) = root.get("sections").unwrap() else {
        panic!("sections")
    };
    for y in [
        MIN_SECTION - 1,
        MIN_SECTION,
        MIN_SECTION + SECTIONS as i32 - 2,
    ] {
        assert!(
            sections
                .iter()
                .any(|section| section.get("Y") == Some(&Tag::Byte(y as i8))),
            "missing light section at Y {y}"
        );
    }
}

#[test]
fn unknown_light_removes_stale_stored_arrays() {
    let mut root = Compound::new();
    let mut section = Compound::new();
    section.insert("Y".into(), Tag::Byte(2));
    section.insert("SkyLight".into(), Tag::ByteArray(vec![1; 2048]));
    section.insert("BlockLight".into(), Tag::ByteArray(vec![2; 2048]));
    root.insert("sections".into(), Tag::List(List::Compound(vec![section])));
    let mut tag = Tag::Compound(root);

    // Y=2 is index 7 when the column starts at section -4; the other slots are unknown.
    let light = LightData {
        sky: vec![LightSection::Unknown; 8],
        block: vec![LightSection::Unknown; 8],
    };
    apply_light_to_nbt(&mut tag, &light, -4).unwrap();
    let Tag::Compound(root) = &tag else {
        panic!("chunk root")
    };
    let Tag::List(List::Compound(sections)) = root.get("sections").unwrap() else {
        panic!("sections")
    };
    let stored = sections
        .iter()
        .find(|section| section.get("Y") == Some(&Tag::Byte(2)))
        .unwrap();
    assert!(stored.get("SkyLight").is_none());
    assert!(stored.get("BlockLight").is_none());
}

#[test]
fn stored_light_arrays_must_be_2048_bytes() {
    let mut section = Compound::new();
    section.insert("Y".into(), Tag::Byte(0));
    section.insert("SkyLight".into(), Tag::ByteArray(vec![1; 7]));
    let mut root = Compound::new();
    root.insert("sections".into(), Tag::List(List::Compound(vec![section])));
    assert!(light_from_nbt(&Tag::Compound(root), -4, 26).is_err());
}

#[test]
fn proto_chunk_from_fixture_roundtrips_status_and_blocks() {
    let Some((input, registry)) = fixture(false) else {
        return;
    };
    let Tag::Compound(root) = &input else {
        panic!()
    };
    let status = root.get("Status").cloned();
    let chunk = chunk_from_nbt(&input, -64, 384, &registry).unwrap();
    let encoded = chunk_to_nbt(&chunk, &registry).unwrap();
    let Tag::Compound(root) = &encoded else {
        panic!()
    };
    assert_eq!(root.get("Status"), status.as_ref());
    let restored = chunk_from_nbt(&encoded, -64, 384, &registry).unwrap();
    for (a, b) in chunk.sections().iter().zip(restored.sections()) {
        for i in 0..4096 {
            assert_eq!(a.blocks().get(i), b.blocks().get(i));
        }
        for i in 0..64 {
            assert_eq!(a.biomes().get(i), b.biomes().get(i));
        }
    }
}
