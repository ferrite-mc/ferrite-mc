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
