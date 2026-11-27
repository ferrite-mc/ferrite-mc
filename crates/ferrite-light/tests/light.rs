use ferrite_light::{LightBlockView, LightEngine, LightKind};
use std::collections::HashMap;
#[derive(Default)]
struct World(HashMap<(i32, i32, i32), u16>);
impl LightBlockView for World {
    fn state(&self, x: i32, y: i32, z: i32) -> u16 {
        *self.0.get(&(x, y, z)).unwrap_or(&0)
    }
}
fn state(name: &str) -> u16 {
    ferrite_data::blocks::default_state(name).unwrap()
}
#[test]
fn recentered_reused_engine_matches_a_fresh_engine() {
    let mut world = World::default();
    for (x, z, name) in [
        (0, 0, "torch"),
        (5, 8, "torch"),
        (-3, 2, "stone"),
        (20, -20, "water"),
        (-17, 18, "oak_slab"),
    ] {
        world.0.insert((x, 8, z), state(name));
    }
    let mut reused = LightEngine::new(0, 0, 0, 1, true);
    reused.relight(&world);
    for (center_x, center_z) in [(1, 2), (-2, 1), (0, 0), (2, -1)] {
        reused.recenter(center_x, center_z);
        reused.relight(&world);
        let mut fresh = LightEngine::new(center_x, center_z, 0, 1, true);
        fresh.relight(&world);
        for local_z in 0..16 {
            for local_x in 0..16 {
                for y in 0..32 {
                    let x = center_x * 16 + local_x;
                    let z = center_z * 16 + local_z;
                    for kind in [LightKind::Block, LightKind::Sky] {
                        assert_eq!(
                            reused.get(kind, x, y, z),
                            fresh.get(kind, x, y, z),
                            "{kind:?} at {x},{y},{z} after recenter {center_x},{center_z}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn torch_propagates_and_removal_clears() {
    let mut world = World::default();
    world.0.insert((0, 8, 0), state("torch"));
    let mut light = LightEngine::new(0, 0, 0, 0, false);
    light.relight(&world);
    assert_eq!(light.get(LightKind::Block, 0, 8, 0), Some(14));
    assert_eq!(light.get(LightKind::Block, 3, 8, 0), Some(11));
    assert_eq!(light.get(LightKind::Block, 1, 9, 1), Some(11));
    world.0.remove(&(0, 8, 0));
    light.block_changed(&world, 0, 8, 0);
    assert_eq!(light.get(LightKind::Block, 3, 8, 0), Some(0));
}
#[test]
fn sky_water_overhang_and_column_change() {
    let mut world = World::default();
    world.0.insert((0, 15, 0), state("water"));
    for x in -2..=2 {
        for z in -2..=2 {
            world.0.insert((x, 10, z), state("stone"));
        }
    }
    let mut light = LightEngine::new(0, 0, 0, 0, true);
    light.relight(&world);
    assert_eq!(light.get(LightKind::Sky, 10, 10, 10), Some(15));
    assert_eq!(light.get(LightKind::Sky, 0, 15, 0), Some(14));
    assert!(light.get(LightKind::Sky, 0, 9, 0).unwrap() < 15);
    world.0.insert((10, 15, 10), state("stone"));
    light.block_changed(&world, 10, 15, 10);
    assert_eq!(light.get(LightKind::Sky, 10, 15, 10), Some(0));
    assert_eq!(light.get(LightKind::Sky, 10, 14, 10), Some(14));
}
#[test]
fn torch_crosses_chunk_boundary() {
    let mut world = World::default();
    world.0.insert((15, 8, 0), state("torch"));
    let mut light = LightEngine::new(0, 0, 0, 0, false);
    light.relight(&world);
    assert_eq!(light.get(LightKind::Block, 16, 8, 0), Some(13));
}

#[test]
fn incremental_matches_relight_with_competing_sources_and_obstacles() {
    let mut world = World::default();
    let mut incremental = LightEngine::new(0, 0, 0, 0, false);
    incremental.relight(&world);
    for (x, y, z, name) in [
        (0, 8, 0, "torch"),
        (5, 8, 0, "torch"),
        (2, 8, 0, "stone"),
        (0, 8, 0, "air"),
        (2, 8, 0, "air"),
        (5, 8, 0, "air"),
    ] {
        world.0.insert((x, y, z), state(name));
        incremental.block_changed(&world, x, y, z);
        let mut expected = LightEngine::new(0, 0, 0, 0, false);
        expected.relight(&world);
        for px in -10..=15 {
            for pz in -10..=10 {
                for py in 0..16 {
                    assert_eq!(
                        incremental.get(LightKind::Block, px, py, pz),
                        expected.get(LightKind::Block, px, py, pz),
                        "after {name} at {x},{y},{z}: {px},{py},{pz}"
                    );
                }
            }
        }
    }
}

// Regression: the incremental sky repair must stay bit-identical to a full relight.
// It now resumes the affected column at the changed layer instead of the build
// limit, including a change in the topmost layer that has no layer above it.
#[test]
fn incremental_sky_matches_relight_for_changes_at_every_layer() {
    let mut world = World::default();
    for x in -4..=15 {
        for z in -4..=8 {
            world.0.insert((x, 10, z), state("stone"));
        }
    }
    let mut incremental = LightEngine::new(0, 0, 0, 1, true);
    incremental.relight(&world);
    for (x, y, z, name) in [
        (3, 20, 3, "stone"),
        (3, 20, 3, "air"),
        (7, 12, 2, "oak_slab"),
        (7, 12, 2, "air"),
        (1, 30, 1, "water"),
        (1, 30, 1, "air"),
        (5, 11, 5, "stone"),
        (2, 31, 2, "stone"),
    ] {
        world.0.insert((x, y, z), state(name));
        incremental.block_changed(&world, x, y, z);
        let mut expected = LightEngine::new(0, 0, 0, 1, true);
        expected.relight(&world);
        for py in 0..32 {
            for pz in -8..=8 {
                for px in -8..=8 {
                    assert_eq!(
                        incremental.get(LightKind::Sky, px, py, pz),
                        expected.get(LightKind::Sky, px, py, pz),
                        "after {name} at {x},{y},{z}: sky at {px},{py},{pz}"
                    );
                }
            }
        }
    }
}

#[test]
fn storage_compaction_and_unloaded_edges() {
    use ferrite_light::NibbleArray;
    let mut array = NibbleArray::Zero;
    array.set(17, 15);
    assert_eq!(array.get(17), Some(15));
    assert_eq!(array.get(16), Some(0));
    array.set(17, 0);
    array.compact();
    assert!(matches!(array, NibbleArray::Zero));
    struct OneChunk;
    impl LightBlockView for OneChunk {
        fn state(&self, x: i32, y: i32, z: i32) -> u16 {
            if (x, y, z) == (15, 8, 0) {
                state("torch")
            } else {
                0
            }
        }
        fn is_chunk_loaded(&self, x: i32, z: i32) -> bool {
            (x, z) == (0, 0)
        }
    }
    let mut engine = LightEngine::new(0, 0, 0, 0, false);
    engine.relight(&OneChunk);
    assert_eq!(engine.get(LightKind::Block, 16, 8, 0), None);
    assert!(engine.pending_edges().contains(&(1, 0)));
}

#[test]
fn documented_vanilla_properties() {
    use ferrite_light::vanilla_properties;
    assert_eq!(vanilla_properties(state("torch")).emission, 14);
    assert_eq!(vanilla_properties(state("water")).opacity, 1);
    assert_eq!(vanilla_properties(state("stone")).opacity, 15);
    assert_eq!(vanilla_properties(state("oak_stairs")).opacity, 0);
}

#[test]
fn skylight_incremental_matches_relight_after_column_changes() {
    let mut world = World::default();
    let mut actual = LightEngine::new(0, 0, 0, 1, true);
    actual.relight(&world);
    for (x, y, z, name) in [
        (0, 31, 0, "stone"),
        (1, 30, 0, "stone"),
        (0, 31, 0, "water"),
        (1, 30, 0, "air"),
        (0, 31, 0, "air"),
    ] {
        world.0.insert((x, y, z), state(name));
        actual.block_changed(&world, x, y, z);
        let mut full = LightEngine::new(0, 0, 0, 1, true);
        full.relight(&world);
        for x in -3..=3 {
            for z in -3..=3 {
                for y in 0..32 {
                    assert_eq!(
                        actual.get(LightKind::Sky, x, y, z),
                        full.get(LightKind::Sky, x, y, z),
                        "{x} {y} {z} after {name}"
                    );
                }
            }
        }
    }
}

#[test]
fn emitting_source_does_not_bypass_target_full_face() {
    let mut world = World::default();
    let slab = ferrite_data::blocks::state_id("oak_slab", &[("type", "bottom")]).unwrap();
    // Enclose the target to rule out light arriving around its full bottom face.
    world.0.insert((0, 7, 0), state("torch"));
    world.0.insert((0, 8, 0), slab);
    for (x, y, z) in [(1, 8, 0), (-1, 8, 0), (0, 8, 1), (0, 8, -1), (0, 9, 0)] {
        world.0.insert((x, y, z), state("stone"));
    }
    let mut engine = LightEngine::new(0, 0, 0, 0, false);
    engine.relight(&world);
    assert_eq!(engine.get(LightKind::Block, 0, 8, 0), Some(0));
}

#[test]
fn stair_faces_follow_half_facing_and_corner_shape() {
    use ferrite_light::vanilla_properties;
    let id = ferrite_data::blocks::state_id(
        "oak_stairs",
        &[
            ("facing", "north"),
            ("half", "bottom"),
            ("shape", "outer_left"),
            ("waterlogged", "false"),
        ],
    )
    .unwrap();
    let p = vanilla_properties(id);
    assert_eq!(p.opacity, 0);
    // Sample the exact face boxes at each quadrant center.
    let masks: Vec<u8> = p
        .faces
        .iter()
        .enumerate()
        .map(|(direction, &id)| {
            let (u, v) = match direction {
                0 | 1 => (2, 1),
                2 | 3 => (0, 2),
                _ => (0, 1),
            };
            let boxes = ferrite_data::block_props::shape_by_id(id);
            let mut mask = 0;
            for y in 0..2 {
                for x in 0..2 {
                    let a = (x as f64 + 0.5) / 2.0;
                    let b = (y as f64 + 0.5) / 2.0;
                    if boxes
                        .iter()
                        .any(|r| r[u] <= a && a < r[u + 3] && r[v] <= b && b < r[v + 3])
                    {
                        mask |= 1 << (y * 2 + x);
                    }
                }
            }
            mask
        })
        .collect();
    assert_eq!(masks, [7, 3, 15, 1, 7, 3]);
}

#[test]
fn incremental_changes_match_rebuild_over_mixed_sequence() {
    let mut world = World::default();
    let mut actual = LightEngine::new(0, 0, 0, 1, true);
    actual.relight(&world);
    let mut seed = 42u32;
    for _ in 0..40 {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let x = (seed % 7) as i32 - 3;
        let z = ((seed >> 8) % 7) as i32 - 3;
        let y = 25 + ((seed >> 16) % 7) as i32;
        let name = ["air", "stone", "torch", "water"][(seed >> 24) as usize % 4];
        world.0.insert((x, y, z), state(name));
        actual.block_changed(&world, x, y, z);
        let mut full = LightEngine::new(0, 0, 0, 1, true);
        full.relight(&world);
        for x in -6..=6 {
            for z in -6..=6 {
                for y in 20..32 {
                    for kind in [LightKind::Block, LightKind::Sky] {
                        assert_eq!(
                            actual.get(kind, x, y, z),
                            full.get(kind, x, y, z),
                            "{kind:?} at {x},{y},{z} after {name}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn common_state_dependent_light_properties() {
    use ferrite_light::vanilla_properties;
    let get = |name, props: &[(&str, &str)]| {
        vanilla_properties(ferrite_data::blocks::state_id(name, props).unwrap())
    };
    assert_eq!(get("redstone_ore", &[("lit", "true")]).emission, 9);
    assert_eq!(get("redstone_ore", &[("lit", "false")]).emission, 0);
    assert_eq!(get("cave_vines", &[("berries", "true")]).emission, 14);
    assert_eq!(
        get("sea_pickle", &[("waterlogged", "true"), ("pickles", "3")]).emission,
        12
    );
    assert_eq!(get("oak_door", &[]).opacity, 0);
    assert_eq!(get("bubble_column", &[]).opacity, 1);
    assert_eq!(get("end_rod", &[]).emission, 14);
}

#[test]
fn calibrated_sensors_vault_and_empty_lichen_have_stateful_emission() {
    use ferrite_light::vanilla_properties;
    let p = |name, props: &[(&str, &str)]| {
        vanilla_properties(ferrite_data::blocks::state_id(name, props).unwrap())
    };
    assert_eq!(p("sculk_sensor", &[]).emission, 1);
    assert_eq!(p("vault", &[("vault_state", "active")]).emission, 12);
    assert_eq!(
        p(
            "trial_spawner",
            &[("trial_spawner_state", "waiting_for_players")]
        )
        .emission,
        4
    );
    assert_eq!(
        p(
            "glow_lichen",
            &[
                ("down", "false"),
                ("up", "false"),
                ("north", "false"),
                ("south", "false"),
                ("east", "false"),
                ("west", "false")
            ]
        )
        .emission,
        0
    );
}

#[test]
fn attenuated_sky_fades_below_a_wide_water_layer() {
    let mut world = World::default();
    for x in -16..32 {
        for z in -16..32 {
            world.0.insert((x, 15, z), state("water"));
        }
    }
    let mut light = LightEngine::new(0, 0, 0, 0, true);
    light.relight(&world);
    assert_eq!(light.get(LightKind::Sky, 0, 15, 0), Some(14));
    assert_eq!(light.get(LightKind::Sky, 0, 14, 0), Some(13));
}
