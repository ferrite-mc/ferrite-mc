use ferrite_worldgen::{WorldgenData, random_spread_candidate};

#[test]
fn loads_vanilla_structure_sets() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3/generated");
    let data = WorldgenData::load_strict(path).unwrap();
    let village = data
        .structure_sets
        .get(&"minecraft:villages".parse().unwrap())
        .unwrap();
    assert_eq!(village["placement"]["spacing"], 34);
    assert_eq!(village["placement"]["separation"], 8);
    assert_eq!(village["structures"].as_array().unwrap().len(), 5);
    assert_eq!(
        data.structure_set_candidate(&"minecraft:villages".parse().unwrap(), 123456789, -1, -1),
        Some((-22, -22))
    );
}

#[test]
fn pillager_outpost_excludes_nearby_village_candidates() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3/generated");
    let data = WorldgenData::load(path).unwrap();
    let outpost = "minecraft:pillager_outposts".parse().unwrap();
    let villages = "minecraft:villages".parse().unwrap();
    assert_eq!(
        data.structure_set_exclusion_allows(&outpost, 123456789, -22, -22),
        Some(false)
    );
    assert_eq!(
        data.structure_set_exclusion_allows(&villages, 123456789, -22, -22),
        Some(true)
    );
    assert_eq!(
        data.structure_set_exclusion_allows(&outpost, 123456789, 0, 0),
        Some(true)
    );
}

#[test]
fn village_candidate_matches_java_random_for_positive_and_negative_regions() {
    for (input, expected) in [
        ((0, 0), (21, 23)),
        ((-1, -1), (-22, -22)),
        ((34, 68), (56, 84)),
        ((-100, 45), (-83, 57)),
    ] {
        assert_eq!(
            random_spread_candidate(123456789, input.0, input.1, 34, 8, 10387312, false),
            Some(expected)
        );
    }
}

#[test]
fn triangular_candidate_matches_java_random() {
    for (input, expected) in [
        ((0, 0), (22, 5)),
        ((-1, -1), (-22, -11)),
        ((34, 68), (53, 71)),
        ((-100, 45), (-81, 54)),
    ] {
        assert_eq!(
            random_spread_candidate(123456789, input.0, input.1, 34, 8, 10387312, true),
            Some(expected)
        );
    }
}

#[test]
fn random_spread_rejects_invalid_spacing() {
    assert_eq!(random_spread_candidate(0, 0, 0, 0, 0, 0, false), None);
    assert_eq!(random_spread_candidate(0, 0, 0, 8, 8, 0, false), None);
}

#[test]
fn frequency_reducers_match_java_random_vectors() {
    let mut data = WorldgenData::default();
    let id: ferrite_worldgen::ResourceId = "minecraft:test".parse().unwrap();
    for (method, expected) in [
        ("default", [false, false, true, true]),
        ("legacy_type_1", [true, true, false, false]),
        ("legacy_type_2", [false, false, true, true]),
        ("legacy_type_3", [false, true, false, false]),
    ] {
        data.structure_sets.insert(id.clone(), serde_json::json!({"placement": {"frequency": 0.2, "frequency_reduction_method": method, "salt": 10387312}}));
        for ((x, z), result) in [(0, 0), (123, -77), (-100, 45), (300, 400)]
            .into_iter()
            .zip(expected)
        {
            assert_eq!(
                data.structure_set_frequency_allows(&id, 123456789, x, z),
                Some(result),
                "{method} at ({x},{z})"
            );
        }
    }
}
