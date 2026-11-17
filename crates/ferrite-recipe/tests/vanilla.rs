//! Integration tests against the real Minecraft 26.3 recipe data.

use ferrite_recipe::{CookingKind, RecipeBook, RecipeKind};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::OnceLock;

fn book() -> &'static RecipeBook {
    static BOOK: OnceLock<RecipeBook> = OnceLock::new();
    BOOK.get_or_init(|| {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../vanilla/26.3/generated/data/minecraft");
        assert!(
            dir.is_dir(),
            "vanilla data is missing at {}; run `python tools/fetch_vanilla.py`",
            dir.display()
        );
        RecipeBook::load(&dir).expect("load vanilla recipes")
    })
}

/// Runs a grid through the matcher and returns `(recipe id, result item, count)`.
fn craft<'a>(book: &'a RecipeBook, grid: [Option<&str>; 9]) -> Option<(&'a str, &'a str, u32)> {
    let recipe = book.match_crafting_3x3(&grid)?;
    let result = recipe.result().expect("crafting result");
    Some((recipe.id.as_str(), result.item.as_str(), result.count))
}

#[test]
fn every_vanilla_recipe_type_has_the_expected_count() {
    let book = book();
    assert_eq!(book.len(), 1_763, "top-level recipes in 26.3");
    assert_eq!(book.tags().len(), 236, "item tags in 26.3");

    let expected = BTreeMap::from([
        ("minecraft:blasting", 25),
        ("minecraft:campfire_cooking", 9),
        ("minecraft:crafting_decorated_pot", 1),
        ("minecraft:crafting_dye", 6),
        ("minecraft:crafting_imbue", 1),
        ("minecraft:crafting_shaped", 827),
        ("minecraft:crafting_shapeless", 375),
        ("minecraft:crafting_special_bannerduplicate", 16),
        ("minecraft:crafting_special_bookcloning", 1),
        ("minecraft:crafting_special_firework_rocket", 1),
        ("minecraft:crafting_special_firework_star", 1),
        ("minecraft:crafting_special_firework_star_fade", 1),
        ("minecraft:crafting_special_mapextending", 1),
        ("minecraft:crafting_special_repairitem", 1),
        ("minecraft:crafting_special_shielddecoration", 1),
        ("minecraft:crafting_transmute", 33),
        ("minecraft:smelting", 73),
        ("minecraft:smithing_transform", 12),
        ("minecraft:smithing_trim", 18),
        ("minecraft:smoking", 9),
        ("minecraft:stonecutting", 351),
    ]);
    assert_eq!(book.counts_by_type(), expected);
    // The per-type counts must account for every loaded recipe.
    assert_eq!(expected.values().sum::<usize>(), book.len());

    // 26.3 keeps `crafting_special_*` plus the newer `crafting_dye`,
    // `crafting_imbue` and `crafting_decorated_pot` types as specials.
    let specials = book
        .recipes()
        .iter()
        .filter(|recipe| recipe.is_special())
        .count();
    assert_eq!(specials, 31, "code-backed special recipes");

    let crafting = book
        .recipes()
        .iter()
        .filter(|recipe| {
            matches!(
                recipe.kind,
                RecipeKind::CraftingShaped(_)
                    | RecipeKind::CraftingShapeless(_)
                    | RecipeKind::CraftingTransmute(_)
            )
        })
        .count();
    assert_eq!(crafting, 1_235);
}

#[test]
fn planks_from_a_log() {
    let book = book();
    let mut grid = [None; 9];
    grid[4] = Some("minecraft:acacia_log");
    assert_eq!(
        craft(book, grid),
        Some(("minecraft:acacia_planks", "minecraft:acacia_planks", 4))
    );

    // Tag lookups accept an item with its `minecraft:` namespace omitted.
    let mut grid = [None; 9];
    grid[0] = Some("spruce_log");
    assert_eq!(
        craft(book, grid),
        Some(("minecraft:spruce_planks", "minecraft:spruce_planks", 4))
    );
}

#[test]
fn sticks_from_two_planks_anywhere_in_a_3x3() {
    let book = book();
    // Right column, bottom two rows: a shaped recipe must slide anywhere.
    let mut grid = [None; 9];
    grid[5] = Some("minecraft:acacia_planks");
    grid[8] = Some("minecraft:acacia_planks");
    assert_eq!(
        craft(book, grid),
        Some(("minecraft:stick", "minecraft:stick", 4))
    );

    // Middle column, top two rows.
    let mut grid = [None; 9];
    grid[1] = Some("minecraft:oak_planks");
    grid[4] = Some("minecraft:oak_planks");
    assert_eq!(
        craft(book, grid),
        Some(("minecraft:stick", "minecraft:stick", 4))
    );

    // Horizontal planks are not sticks: they are a pressure plate instead.
    let mut grid = [None; 9];
    grid[0] = Some("minecraft:oak_planks");
    grid[1] = Some("minecraft:oak_planks");
    assert_eq!(
        craft(book, grid),
        Some((
            "minecraft:oak_pressure_plate",
            "minecraft:oak_pressure_plate",
            1
        ))
    );
}

#[test]
fn crafting_table_in_a_2x2() {
    let book = book();
    let grid = [
        Some("minecraft:oak_planks"),
        Some("minecraft:birch_planks"),
        Some("minecraft:spruce_planks"),
        Some("minecraft:jungle_planks"),
    ];
    let recipe = book.match_crafting_2x2(&grid).expect("2x2 crafting table");
    assert_eq!(recipe.id, "minecraft:crafting_table");
    let result = recipe.result().expect("result");
    assert_eq!(result.item, "minecraft:crafting_table");
    assert_eq!(result.count, 1);

    // Three planks are not a crafting table.
    let grid = [
        Some("minecraft:oak_planks"),
        Some("minecraft:oak_planks"),
        Some("minecraft:oak_planks"),
        None,
    ];
    assert_ne!(
        book.match_crafting_2x2(&grid)
            .map(|recipe| recipe.id.as_str()),
        Some("minecraft:crafting_table")
    );
}

#[test]
fn mirrored_shaped_recipes_match() {
    let book = book();

    // oak_stairs is `#  / ## / ###`; the mirrored form is right-aligned.
    let mirrored = [
        None,
        None,
        Some("minecraft:oak_planks"),
        None,
        Some("minecraft:oak_planks"),
        Some("minecraft:oak_planks"),
        Some("minecraft:oak_planks"),
        Some("minecraft:oak_planks"),
        Some("minecraft:oak_planks"),
    ];
    assert_eq!(
        craft(book, mirrored),
        Some(("minecraft:oak_stairs", "minecraft:oak_stairs", 4))
    );

    // The normal orientation still matches too.
    let normal = [
        Some("minecraft:oak_planks"),
        None,
        None,
        Some("minecraft:oak_planks"),
        Some("minecraft:oak_planks"),
        None,
        Some("minecraft:oak_planks"),
        Some("minecraft:oak_planks"),
        Some("minecraft:oak_planks"),
    ];
    assert_eq!(
        craft(book, normal),
        Some(("minecraft:oak_stairs", "minecraft:oak_stairs", 4))
    );

    // wooden_hoe is `XX / .# / .#`; mirrored it is `XX / #. / #.`.
    let mut mirrored_hoe = [None; 9];
    mirrored_hoe[0] = Some("minecraft:oak_planks");
    mirrored_hoe[1] = Some("minecraft:oak_planks");
    mirrored_hoe[3] = Some("minecraft:stick");
    mirrored_hoe[6] = Some("minecraft:stick");
    assert_eq!(
        craft(book, mirrored_hoe),
        Some(("minecraft:wooden_hoe", "minecraft:wooden_hoe", 1))
    );
}

#[test]
fn flint_and_steel_is_shapeless() {
    let book = book();
    for (flint, steel) in [(0, 1), (8, 4), (2, 6)] {
        let mut grid = [None; 9];
        grid[flint] = Some("minecraft:flint");
        grid[steel] = Some("minecraft:iron_ingot");
        assert_eq!(
            craft(book, grid),
            Some(("minecraft:flint_and_steel", "minecraft:flint_and_steel", 1)),
            "flint at {flint}, iron at {steel}"
        );
    }

    // Three items do not match the two-ingredient recipe.
    let mut grid = [None; 9];
    grid[0] = Some("minecraft:flint");
    grid[1] = Some("minecraft:iron_ingot");
    grid[2] = Some("minecraft:iron_ingot");
    assert!(
        craft(book, grid).is_none_or(|(id, _, _)| id != "minecraft:flint_and_steel"),
        "extra iron must not produce flint and steel"
    );
}

#[test]
fn smelting_raw_iron() {
    let book = book();
    let recipe = book
        .match_cooking(CookingKind::Smelting, "minecraft:raw_iron")
        .expect("raw iron smelting recipe");
    assert_eq!(recipe.id, "minecraft:iron_ingot_from_smelting_raw_iron");
    let RecipeKind::Cooking(cooking) = &recipe.kind else {
        panic!("expected a cooking recipe, got {:?}", recipe.kind);
    };
    assert_eq!(cooking.kind, CookingKind::Smelting);
    assert_eq!(cooking.cooking_time, 200);
    assert!((cooking.experience - 0.7).abs() < f32::EPSILON);
    assert_eq!(cooking.result.item, "minecraft:iron_ingot");
    assert_eq!(cooking.result.count, 1);

    // Raw iron also blasts, but it is never smoked.
    let recipe = book
        .match_cooking(CookingKind::Blasting, "minecraft:raw_iron")
        .expect("raw iron blasting recipe");
    assert_eq!(recipe.id, "minecraft:iron_ingot_from_blasting_raw_iron");
    let RecipeKind::Cooking(cooking) = &recipe.kind else {
        panic!("expected a cooking recipe");
    };
    assert_eq!(cooking.kind, CookingKind::Blasting);
    assert_eq!(cooking.cooking_time, 200);
    assert!((cooking.experience - 0.7).abs() < f32::EPSILON);
    assert_eq!(cooking.result.item, "minecraft:iron_ingot");

    assert!(
        book.match_cooking(CookingKind::Smoking, "minecraft:raw_iron")
            .is_none()
    );
    assert!(
        book.match_cooking(CookingKind::Smelting, "minecraft:sand")
            .is_some_and(|recipe| recipe.id == "minecraft:glass")
    );
    assert!(
        book.match_cooking(CookingKind::Blasting, "minecraft:sand")
            .is_none()
    );
    assert!(
        book.match_cooking(CookingKind::Smelting, "minecraft:dirt")
            .is_none()
    );

    // Another furnace family member: raw copper blasts into a copper ingot.
    let recipe = book
        .match_cooking(CookingKind::Blasting, "minecraft:raw_copper")
        .expect("raw copper blasting recipe");
    let RecipeKind::Cooking(cooking) = &recipe.kind else {
        panic!("expected a cooking recipe");
    };
    assert_eq!(cooking.result.item, "minecraft:copper_ingot");
    assert_eq!(cooking.cooking_time, 200);
}

#[test]
fn stonecutting_stone_options() {
    let book = book();
    let options = book.stonecutting_options("minecraft:stone");
    assert_eq!(options.len(), 11, "stonecutter recipes accepting stone");

    let ids = options
        .iter()
        .map(|recipe| recipe.id.as_str())
        .collect::<Vec<_>>();
    assert!(ids.contains(&"minecraft:stone_stairs_from_stone_stonecutting"));
    assert!(ids.contains(&"minecraft:stone_slab_from_stone_stonecutting"));
    assert!(ids.contains(&"minecraft:cobblestone_from_stone_stonecutting"));

    let stairs = book
        .recipe("stone_stairs_from_stone_stonecutting")
        .expect("stone stairs recipe");
    let RecipeKind::Stonecutting(stonecutting) = &stairs.kind else {
        panic!("expected a stonecutting recipe");
    };
    assert_eq!(stonecutting.result.item, "minecraft:stone_stairs");
    assert_eq!(stonecutting.result.count, 1);

    let slab = book
        .recipe("stone_slab_from_stone_stonecutting")
        .expect("stone slab recipe");
    let RecipeKind::Stonecutting(stonecutting) = &slab.kind else {
        panic!("expected a stonecutting recipe");
    };
    assert_eq!(stonecutting.result.item, "minecraft:stone_slab");
    assert_eq!(stonecutting.result.count, 2);

    // Options come back in recipe-id order and every input is accepted.
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(
        book.stonecutting_options("minecraft:diamond_pickaxe")
            .is_empty()
    );
}

#[test]
fn grids_that_match_nothing_return_none() {
    let book = book();

    assert!(book.match_crafting_3x3(&[None; 9]).is_none());
    assert!(book.match_crafting_2x2(&[None; 4]).is_none());

    // Nine dirt blocks: no recipe accepts dirt at all.
    let dirt = [Some("minecraft:dirt"); 9];
    assert!(craft(book, dirt).is_none());

    // A chest-shaped ring of planks with one dirt in place of a plank.
    let mut grid = [Some("minecraft:oak_planks"); 9];
    grid[4] = None;
    grid[0] = Some("minecraft:dirt");
    assert!(craft(book, grid).is_none());

    // A single stick alone is not a recipe.
    let mut grid = [None; 9];
    grid[4] = Some("minecraft:stick");
    assert!(craft(book, grid).is_none());

    // A 1x1 grid is too small for the crafting table.
    assert!(
        book.match_crafting(&[Some("minecraft:oak_planks")], 1, 1)
            .is_none_or(|recipe| recipe.id != "minecraft:crafting_table")
    );
}
