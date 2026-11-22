use ferrite_world::{BlockFlags, BlockInfo, Chunk, HeightmapKind};
struct TestBlocks;
impl BlockInfo for TestBlocks {
    fn flags(&self, id: u32) -> BlockFlags {
        match id {
            0 => BlockFlags {
                air: true,
                ..Default::default()
            },
            1 => BlockFlags {
                motion_blocking: true,
                ..Default::default()
            },
            2 => BlockFlags {
                fluid: true,
                ..Default::default()
            },
            3 => BlockFlags {
                motion_blocking: true,
                leaves: true,
                ..Default::default()
            },
            _ => Default::default(),
        }
    }
}
#[test]
fn chunk_heights_and_non_air_counts_update() {
    let mut c = Chunk::new(-64, 384, 0, 0, 15, 6).unwrap();
    assert_eq!(c.sections().len(), 24);
    c.set_block(0, -64, 0, 1, &TestBlocks).unwrap();
    c.set_block(0, 10, 0, 2, &TestBlocks).unwrap();
    c.set_block(0, 20, 0, 3, &TestBlocks).unwrap();
    assert_eq!(c.height(HeightmapKind::WorldSurface, 0, 0), Some(85));
    assert_eq!(
        c.height(HeightmapKind::MotionBlockingNoLeaves, 0, 0),
        Some(75)
    );
    assert_eq!(c.height(HeightmapKind::OceanFloor, 0, 0), Some(85));
    c.set_block(0, 20, 0, 0, &TestBlocks).unwrap();
    assert_eq!(c.height(HeightmapKind::OceanFloor, 0, 0), Some(1));
    assert_eq!(c.sections()[0].block_count(), 1);
    assert_eq!(c.sections()[5].block_count(), 0);
}

#[test]
fn full_chunk_network_encoding_includes_light_masks_and_data() {
    use ferrite_world::{LightData, LightSection, encode_chunk_packet};
    let c = Chunk::new(0, 16, 0, 0, 15, 6).unwrap();
    let light = LightData {
        sky: vec![
            LightSection::Empty,
            LightSection::Data(Box::new([0x21; 2048])),
            LightSection::Empty,
        ],
        block: vec![LightSection::Empty; 3],
    };
    let encoded = encode_chunk_packet(2, -3, &c, &light).unwrap();
    assert_eq!(&encoded[..8], &[0, 0, 0, 2, 255, 255, 255, 253]);
    assert!(encoded.windows(2048).any(|a| a == [0x21; 2048]));
    assert!(encode_chunk_packet(0, 0, &c, &LightData::default()).is_err());
}

#[test]
fn network_section_counts_water_and_waterlogged_blocks() {
    use ferrite_world::ChunkSection;
    struct Vanilla;
    impl BlockInfo for Vanilla {
        fn flags(&self, id: u32) -> BlockFlags {
            let p = ferrite_data::block_props::get(id as u16);
            BlockFlags {
                air: p.is_air,
                fluid: p.fluid.is_some(),
                ..Default::default()
            }
        }
    }
    let air = ferrite_data::blocks::default_state("air").unwrap() as u32;
    let water = ferrite_data::blocks::default_state("water").unwrap() as u32;
    let slab = ferrite_data::blocks::state_id(
        "stone_slab",
        &[("type", "bottom"), ("waterlogged", "true")],
    )
    .unwrap() as u32;
    let mut s = ChunkSection::new(air, 0, 16, 6).unwrap();
    s.set_block(0, water, &Vanilla).unwrap();
    s.set_block(1, slab, &Vanilla).unwrap();
    let bytes = s.encode_network();
    assert_eq!(&bytes[..4], &[0, 2, 0, 2]);
    let mut expected = vec![0, 2, 0, 2];
    expected.extend(s.blocks().encode_network());
    expected.extend(s.biomes().encode_network());
    assert_eq!(bytes, expected);
    s.set_block(0, air, &Vanilla).unwrap();
    assert_eq!(&s.encode_network()[..4], &[0, 1, 0, 1]);
}

#[test]
fn section_random_tick_counts_follow_block_replacements() {
    use ferrite_world::ChunkSection;
    struct Ticking;
    impl BlockInfo for Ticking {
        fn flags(&self, id: u32) -> BlockFlags {
            BlockFlags {
                air: id == 0,
                fluid: id == 2,
                ..Default::default()
            }
        }
        fn randomly_ticking(&self, id: u32) -> bool {
            id == 1
        }
        fn fluid_randomly_ticking(&self, id: u32) -> bool {
            id == 2
        }
    }
    let mut s = ChunkSection::new(0, 0, 16, 6).unwrap();
    assert!(!s.is_randomly_ticking());
    s.set_block(0, 1, &Ticking).unwrap();
    s.set_block(1, 2, &Ticking).unwrap();
    assert_eq!(s.randomly_ticking_block_count(), 1);
    assert_eq!(s.randomly_ticking_fluid_count(), 1);
    s.set_block(0, 0, &Ticking).unwrap();
    assert!(s.is_randomly_ticking());
    s.set_block(1, 0, &Ticking).unwrap();
    assert!(!s.is_randomly_ticking());
}

#[test]
fn compute_heightmaps_from_u16_matches_recompute_heightmaps() {
    let info = TestBlocks;
    let mut chunk_fast = Chunk::new(0, 64, 0, 0, 15, 6).unwrap();
    let mut chunk_ref = Chunk::new(0, 64, 0, 0, 15, 6).unwrap();
    let mut flat = vec![0u16; 64 * 256];
    for col in 0..256usize {
        let x = (col % 16) as u8;
        let z = (col / 16) as u8;
        let y_idx = (col * 3 + 5) % 60;
        let state = match col % 4 {
            0 => 1u16, // solid stone
            1 => 2u16, // leaves
            2 => 3u16, // water
            _ => 0u16, // air
        };
        flat[y_idx * 256 + col] = state;
        chunk_ref
            .set_block(x, y_idx as i32, z, u32::from(state), &info)
            .unwrap();
    }
    for sy in 0..4 {
        chunk_fast.sections_mut()[sy]
            .set_blocks_u16(&flat[sy * 4096..(sy + 1) * 4096], &info)
            .unwrap();
    }
    chunk_fast.compute_heightmaps_from_u16(&flat, &info);
    chunk_ref.recompute_heightmaps(&info);
    for kind in HeightmapKind::ALL {
        assert_eq!(
            chunk_fast.heights(kind),
            chunk_ref.heights(kind),
            "{kind:?}"
        );
    }
}

#[test]
fn encode_chunk_packet_into_matches_block_entity_coordinate_stripping() {
    use ferrite_nbt::Tag;
    use ferrite_world::{
        BlockEntity, LightData, LightSection, encode_chunk_packet, encode_chunk_packet_into,
    };
    let mut chunk = Chunk::new(0, 16, 0, 0, 15, 6).unwrap();
    let mut data = ferrite_nbt::Compound::new();
    data.insert("x".into(), Tag::Int(1));
    data.insert("y".into(), Tag::Int(5));
    data.insert("z".into(), Tag::Int(2));
    data.insert("id".into(), Tag::String("minecraft:chest".into()));
    chunk
        .block_entities
        .insert((1, 5, 2), BlockEntity { type_id: 1, data });
    let light = LightData {
        sky: vec![LightSection::Empty; 3],
        block: vec![LightSection::Empty; 3],
    };
    let direct = encode_chunk_packet(0, 0, &chunk, &light).unwrap();
    let mut prefixed = vec![0xAA, 0xBB];
    encode_chunk_packet_into(0, 0, &chunk, &light, &mut prefixed).unwrap();
    assert_eq!(&prefixed[..2], &[0xAA, 0xBB]);
    assert_eq!(&prefixed[2..], direct.as_slice());
}
