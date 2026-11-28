//! Tests for the block physics extracted from the vanilla server by `tools/extractor`.

use ferrite_data::block_props::{self, FluidKind, NoteBlockInstrument};
use ferrite_data::blocks;

/// The default state and its physical properties for a block name.
fn default(name: &str) -> (u16, &'static block_props::BlockProps) {
    let state = blocks::default_state(name).unwrap_or_else(|| panic!("{name} is not a block"));
    (state, block_props::get(state))
}

#[test]
fn air_is_air() {
    let (_, air) = default("air");
    assert!(air.is_air);
    assert!(!air.blocks_motion);
    assert_eq!(air.light_opacity, 0);
    assert!(air.empty_collision);
    assert_eq!(
        block_props::collision_shape(blocks::default_state("air").unwrap()).len(),
        0
    );
}

#[test]
fn stone_is_opaque_and_blocks_motion() {
    let (state, stone) = default("stone");
    assert_eq!(stone.light_opacity, 15);
    assert!(stone.blocks_motion);
    assert_eq!(
        stone.heightmaps
            & (block_props::WORLD_SURFACE
                | block_props::OCEAN_FLOOR
                | block_props::MOTION_BLOCKING
                | block_props::MOTION_BLOCKING_NO_LEAVES),
        0b1111,
        "stone counts for every heightmap, got {:#06b}",
        stone.heightmaps
    );
    assert!(stone.full_collision);
    assert_eq!(
        block_props::collision_shape(state),
        &[[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]]
    );
}

#[test]
fn glass_is_transparent_but_still_blocks_motion() {
    let (_, glass) = default("glass");
    assert_eq!(glass.light_opacity, 0);
    assert!(!glass.can_occlude);
    assert!(glass.propagates_skylight_down);
    assert!(glass.blocks_motion);
}

#[test]
fn water_is_a_full_fluid_without_ocean_floor() {
    let (state, water) = default("water");
    assert_eq!(water.fluid, Some((FluidKind::Water, 8, false)));
    assert!(!water.blocks_motion);
    assert_ne!(water.heightmaps & block_props::MOTION_BLOCKING, 0);
    assert_eq!(water.heightmaps & block_props::OCEAN_FLOOR, 0);
    assert!(block_props::collision_shape(state).is_empty());
}

#[test]
fn oak_leaves_are_excluded_from_the_no_leaves_heightmap() {
    let (_, leaves) = default("oak_leaves");
    assert_ne!(leaves.heightmaps & block_props::MOTION_BLOCKING, 0);
    assert_eq!(
        leaves.heightmaps & block_props::MOTION_BLOCKING_NO_LEAVES,
        0
    );
    assert!(!leaves.can_occlude, "leaves do not occlude");
}

#[test]
fn light_sources() {
    assert_eq!(default("torch").1.light_emission, 14);
    assert_eq!(default("glowstone").1.light_emission, 15);
    assert_eq!(default("stone").1.light_emission, 0);
}

#[test]
fn bottom_slab_has_a_half_height_collision_box() {
    let (state, slab) = default("stone_slab");
    assert!(!slab.full_collision);
    assert!(!slab.empty_collision);
    assert_eq!(
        block_props::collision_shape(state),
        &[[0.0, 0.0, 0.0, 1.0, 0.5, 1.0]]
    );
    assert_eq!(
        block_props::occlusion_shape(state),
        &[[0.0, 0.0, 0.0, 1.0, 0.5, 1.0]]
    );
}

#[test]
fn block_info_matches_the_block_table() {
    let stone = block_props::block_info("stone").expect("stone has block info");
    assert_eq!(stone.name, "minecraft:stone");
    assert_eq!(Some(stone.default_state), blocks::default_state("stone"));
    assert_eq!(stone.item, Some("minecraft:stone"));
    assert_eq!(stone.explosion_resistance, 6.0);

    let water = block_props::block_info("minecraft:water").expect("water has block info");
    assert_eq!(water.item, None);
    assert!(block_props::block_info("not_a_block").is_none());
}

#[test]
fn every_state_has_consistent_shapes() {
    assert_eq!(block_props::STATE_COUNT, blocks::STATE_COUNT);
    for id in 0..block_props::STATE_COUNT as u16 {
        let props = block_props::get(id);
        let collision = block_props::collision_shape(id);
        assert_eq!(
            props.empty_collision,
            collision.is_empty(),
            "state {id} empty_collision disagrees with its shape"
        );
        assert_eq!(
            props.full_collision,
            collision == [[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]],
            "state {id} full_collision disagrees with its shape"
        );
        assert!(
            block_props::occlusion_shape(id).len() <= 8,
            "state {id} has an implausible occlusion shape"
        );
    }
}

#[test]
fn slab_face_shapes_follow_vanilla() {
    let state = blocks::default_state("stone_slab").unwrap();
    assert_eq!(
        block_props::face_occlusion(state, 2),
        &[[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]]
    );
    assert!(block_props::face_occlusion(state, 3).is_empty());
    assert_eq!(
        block_props::face_occlusion(state, 0),
        &[[0.0, 0.0, 0.0, 1.0, 0.5, 1.0]]
    );
}

#[test]
fn note_block_instruments_follow_the_block_under_the_note() {
    // `NoteBlockInstrument` is a block property, so every state of a block shares it.
    assert_eq!(default("stone").1.instrument, NoteBlockInstrument::Basedrum);
    assert_eq!(default("sand").1.instrument, NoteBlockInstrument::Snare);
    assert_eq!(default("glass").1.instrument, NoteBlockInstrument::Hat);
    assert_eq!(
        default("oak_planks").1.instrument,
        NoteBlockInstrument::Bass
    );
    assert_eq!(
        default("gold_block").1.instrument,
        NoteBlockInstrument::Bell
    );
    assert_eq!(
        default("packed_ice").1.instrument,
        NoteBlockInstrument::Chime
    );
    assert_eq!(
        default("white_wool").1.instrument,
        NoteBlockInstrument::Guitar
    );
    assert_eq!(
        default("copper_block").1.instrument,
        NoteBlockInstrument::Trumpet
    );
    assert_eq!(
        default("zombie_head").1.instrument,
        NoteBlockInstrument::Zombie
    );
    assert_eq!(
        default("player_head").1.instrument,
        NoteBlockInstrument::CustomHead
    );
    // A note block is a wooden block, so a note block on a note block sounds like bass.
    assert_eq!(
        default("note_block").1.instrument,
        NoteBlockInstrument::Bass
    );

    assert!(NoteBlockInstrument::Bell.is_tunable());
    assert!(NoteBlockInstrument::Bell.sound().is_some());
    assert!(!NoteBlockInstrument::Zombie.is_tunable());
    assert!(NoteBlockInstrument::Zombie.works_above_note_block());
    assert_eq!(NoteBlockInstrument::CustomHead.sound(), None);
    assert_eq!(
        NoteBlockInstrument::from_name("trumpet_weathered"),
        Some(NoteBlockInstrument::TrumpetWeathered)
    );
    assert_eq!(NoteBlockInstrument::from_name("not_an_instrument"), None);
}

#[test]
fn flowing_fluid_retains_its_vanilla_id() {
    let state = blocks::state_id("water", &[("level", "1")]).unwrap();
    assert_eq!(
        block_props::get(state).fluid.unwrap().0,
        FluidKind::FlowingWater
    );
}
