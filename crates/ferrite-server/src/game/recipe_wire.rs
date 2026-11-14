//! The recipe book entries of recipes added at runtime, in the 26.3 network format.
//!
//! Vanilla recipes ship pre-encoded from `vanilla/<ver>/generated/ferrite/recipes.json`
//! (see `recipe_packets`). A recipe a plugin adds has no such entry, so it is encoded here:
//! `RecipeDisplayEntry` (id, display, group, category, crafting requirements) for shaped,
//! shapeless, furnace-family, stonecutter and smithing-transform recipes, plus the two parts of
//! `update_recipes` that depend on the recipe set (the item sets the furnace and smithing slots
//! accept, and the stonecutter's option list). The tests encode every vanilla recipe of these
//! kinds and compare the bytes with the data the real server wrote.

use std::collections::BTreeSet;

use ferrite_item::{ItemStack, item_id};
use ferrite_protocol::{Decode, Encode, VarInt};
use ferrite_recipe::{
    CookingKind, Ingredient, IngredientEntry, ItemTags, Recipe, RecipeBook, RecipeKind,
};

/// `SlotDisplay` registry ids.
const SLOT_EMPTY: i32 = 0;
const SLOT_ANY_FUEL: i32 = 1;
const SLOT_ITEM: i32 = 4;
const SLOT_ITEM_STACK: i32 = 5;
const SLOT_HOLDER_SET: i32 = 6;

/// `RecipeDisplay` registry ids.
const DISPLAY_SHAPELESS: i32 = 0;
const DISPLAY_SHAPED: i32 = 1;
const DISPLAY_FURNACE: i32 = 2;
const DISPLAY_STONECUTTER: i32 = 3;
const DISPLAY_SMITHING: i32 = 4;

/// `RecipeBookCategory` registry ids.
const CATEGORY_CRAFTING_BUILDING: i32 = 0;
const CATEGORY_CRAFTING_REDSTONE: i32 = 1;
const CATEGORY_CRAFTING_EQUIPMENT: i32 = 2;
const CATEGORY_CRAFTING_MISC: i32 = 3;
const CATEGORY_FURNACE_FOOD: i32 = 4;
const CATEGORY_FURNACE_BLOCKS: i32 = 5;
const CATEGORY_FURNACE_MISC: i32 = 6;
const CATEGORY_BLAST_BLOCKS: i32 = 7;
const CATEGORY_BLAST_MISC: i32 = 8;
const CATEGORY_SMOKER_FOOD: i32 = 9;
const CATEGORY_STONECUTTER: i32 = 10;
const CATEGORY_SMITHING: i32 = 11;
const CATEGORY_CAMPFIRE: i32 = 12;

/// One encoded recipe book entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encoded {
    /// `RecipeBookAddPacket.Entry` bytes with the flags byte (0) last.
    pub wire: Vec<u8>,
    /// The bare `RecipeDisplay`, for `place_ghost_recipe`.
    pub display: Vec<u8>,
}

fn varint(out: &mut Vec<u8>, value: i32) {
    VarInt(value).encode(out);
}

fn string(out: &mut Vec<u8>, text: &str) {
    varint(out, text.len() as i32);
    out.extend_from_slice(text.as_bytes());
}

/// A `HolderSet<Item>`: `0` and a tag key, or `n + 1` and `n` item ids. `None` when an item has
/// no registry id.
fn holder_set(ingredient: &Ingredient, tags: &ItemTags) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    if let [IngredientEntry::Tag(tag)] = ingredient.options.as_slice() {
        varint(&mut out, 0);
        string(&mut out, tag);
        return Some(out);
    }
    let items = declared_items(ingredient, tags);
    varint(&mut out, items.len() as i32 + 1);
    for item in items {
        varint(&mut out, i32::from(item_id(&item)?));
    }
    Some(out)
}

/// The items of an ingredient in the order the recipe lists them (a tag's items sorted), each once.
fn declared_items(ingredient: &Ingredient, tags: &ItemTags) -> Vec<String> {
    let mut items: Vec<String> = Vec::new();
    for option in &ingredient.options {
        let next = match option {
            IngredientEntry::Item(item) => vec![item.clone()],
            IngredientEntry::Tag(tag) => {
                let mut members: Vec<String> =
                    tags.items(tag).into_iter().flatten().cloned().collect();
                members.sort_unstable();
                members
            }
        };
        for item in next {
            if !items.contains(&item) {
                items.push(item);
            }
        }
    }
    items
}

/// `SlotDisplay` of an ingredient; an absent smithing slot is the empty display.
fn ingredient_display(ingredient: &Ingredient, tags: &ItemTags) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    if ingredient.is_empty() {
        varint(&mut out, SLOT_EMPTY);
        return Some(out);
    }
    varint(&mut out, SLOT_HOLDER_SET);
    out.extend(holder_set(ingredient, tags)?);
    Some(out)
}

fn item_display(item: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    varint(&mut out, SLOT_ITEM);
    varint(&mut out, i32::from(item_id(item)?));
    Some(out)
}

/// `SlotDisplay.ItemStackSlotDisplay`: an `ItemStackTemplate` (item, count, component patch),
/// which is the stack's network form with the count and item id swapped.
fn stack_display(stack: &ItemStack) -> Option<Vec<u8>> {
    let network = stack.encode_network().ok()?;
    let mut rest = &network[..];
    let count = VarInt::decode(&mut rest).ok()?;
    let item = VarInt::decode(&mut rest).ok()?;
    let mut out = Vec::new();
    varint(&mut out, SLOT_ITEM_STACK);
    varint(&mut out, item.0);
    varint(&mut out, count.0);
    out.extend_from_slice(rest);
    Some(out)
}

fn category(recipe: &Recipe) -> i32 {
    let kind = recipe.category.as_deref();
    match &recipe.kind {
        RecipeKind::CraftingShaped(_) | RecipeKind::CraftingShapeless(_) => match kind {
            Some("building") => CATEGORY_CRAFTING_BUILDING,
            Some("redstone") => CATEGORY_CRAFTING_REDSTONE,
            Some("equipment") => CATEGORY_CRAFTING_EQUIPMENT,
            _ => CATEGORY_CRAFTING_MISC,
        },
        RecipeKind::Cooking(cooking) => match cooking.kind {
            CookingKind::Smelting => match kind {
                Some("food") => CATEGORY_FURNACE_FOOD,
                Some("blocks") => CATEGORY_FURNACE_BLOCKS,
                _ => CATEGORY_FURNACE_MISC,
            },
            CookingKind::Blasting => match kind {
                Some("blocks") => CATEGORY_BLAST_BLOCKS,
                _ => CATEGORY_BLAST_MISC,
            },
            CookingKind::Smoking => CATEGORY_SMOKER_FOOD,
            CookingKind::CampfireCooking => CATEGORY_CAMPFIRE,
        },
        RecipeKind::Stonecutting(_) => CATEGORY_STONECUTTER,
        _ => CATEGORY_SMITHING,
    }
}

/// Encodes a recipe a plugin added (or any vanilla recipe of the same kinds) as a recipe book
/// entry. `result` is the stack the recipe makes, with the plugin's metadata. `None` for kinds the
/// client's book has no plain display for (smithing trims, transmute, special recipes) or when an
/// item has no registry id.
pub fn encode_entry(
    recipe: &Recipe,
    tags: &ItemTags,
    index: i32,
    group: Option<i32>,
    result: &ItemStack,
) -> Option<Encoded> {
    let mut display = Vec::new();
    let mut requirements: Vec<&Ingredient> = Vec::new();
    match &recipe.kind {
        RecipeKind::CraftingShapeless(shapeless) => {
            varint(&mut display, DISPLAY_SHAPELESS);
            varint(&mut display, shapeless.ingredients.len() as i32);
            for ingredient in &shapeless.ingredients {
                display.extend(ingredient_display(ingredient, tags)?);
                requirements.push(ingredient);
            }
            display.extend(stack_display(result)?);
            display.extend(item_display("crafting_table")?);
        }
        RecipeKind::CraftingShaped(shaped) => {
            // `ShapedRecipePattern` shrinks the pattern: blank outer rows and columns are gone.
            let rows = shaped.pattern.iter().enumerate().filter(|(_, row)| row.iter().any(Option::is_some));
            let (top, bottom) = (
                rows.clone().map(|(i, _)| i).min()?,
                rows.map(|(i, _)| i).max()?,
            );
            let columns: Vec<usize> = shaped
                .pattern
                .iter()
                .flat_map(|row| row.iter().enumerate().filter(|(_, cell)| cell.is_some()).map(|(i, _)| i))
                .collect();
            let (left, right) = (*columns.iter().min()?, *columns.iter().max()?);
            let (width, height) = (right - left + 1, bottom - top + 1);
            varint(&mut display, DISPLAY_SHAPED);
            varint(&mut display, width as i32);
            varint(&mut display, height as i32);
            varint(&mut display, (width * height) as i32);
            for row in &shaped.pattern[top..=bottom] {
                for column in left..=right {
                    match row.get(column).and_then(Option::as_ref) {
                        Some(ingredient) => {
                            display.extend(ingredient_display(ingredient, tags)?);
                            requirements.push(ingredient);
                        }
                        None => varint(&mut display, SLOT_EMPTY),
                    }
                }
            }
            display.extend(stack_display(result)?);
            display.extend(item_display("crafting_table")?);
        }
        RecipeKind::Cooking(cooking) => {
            varint(&mut display, DISPLAY_FURNACE);
            display.extend(ingredient_display(&cooking.ingredient, tags)?);
            varint(&mut display, SLOT_ANY_FUEL);
            display.extend(stack_display(result)?);
            display.extend(item_display(match cooking.kind {
                CookingKind::Smelting => "furnace",
                CookingKind::Blasting => "blast_furnace",
                CookingKind::Smoking => "smoker",
                CookingKind::CampfireCooking => "campfire",
            })?);
            varint(&mut display, i32::try_from(cooking.cooking_time).ok()?);
            display.extend(cooking.experience.to_be_bytes());
            requirements.push(&cooking.ingredient);
        }
        RecipeKind::Stonecutting(cutting) => {
            varint(&mut display, DISPLAY_STONECUTTER);
            display.extend(ingredient_display(&cutting.ingredient, tags)?);
            display.extend(stack_display(result)?);
            display.extend(item_display("stonecutter")?);
            requirements.push(&cutting.ingredient);
        }
        RecipeKind::SmithingTransform(smithing) => {
            varint(&mut display, DISPLAY_SMITHING);
            display.extend(ingredient_display(&smithing.template, tags)?);
            display.extend(ingredient_display(&smithing.base, tags)?);
            display.extend(ingredient_display(&smithing.addition, tags)?);
            display.extend(stack_display(result)?);
            display.extend(item_display("smithing_table")?);
            requirements.extend(
                [&smithing.template, &smithing.base, &smithing.addition]
                    .into_iter()
                    .filter(|ingredient| !ingredient.is_empty()),
            );
        }
        _ => return None,
    }
    let mut wire = Vec::new();
    varint(&mut wire, index);
    wire.extend_from_slice(&display);
    varint(&mut wire, group.map_or(0, |group| group + 1));
    varint(&mut wire, category(recipe));
    wire.push(1);
    varint(&mut wire, requirements.len() as i32);
    for ingredient in requirements {
        wire.extend(holder_set(ingredient, tags)?);
    }
    wire.push(0);
    Some(Encoded { wire, display })
}

/// The `update_recipes` body for a recipe set: the property sets (which items each furnace and
/// smithing slot accepts) and the stonecutter's options. `base` is the body of the vanilla
/// packet; its brewing sets, which no plugin recipe changes, and the order of its property sets
/// are kept.
///
/// `visible` says which recipes of `book` still exist; `added` are a plugin's recipes.
pub fn update_body(
    base: &[u8],
    book: &RecipeBook,
    visible: &dyn Fn(&Recipe) -> bool,
    added: &[Recipe],
    result_of: &dyn Fn(&Recipe) -> Option<ItemStack>,
) -> Option<Vec<u8>> {
    let mut rest = base;
    let sets = VarInt::decode(&mut rest).ok()?.0;
    let tags = book.tags();
    let recipes: Vec<&Recipe> = book
        .recipes()
        .iter()
        .filter(|recipe| visible(recipe))
        .chain(added)
        .collect();
    let mut out = Vec::new();
    varint(&mut out, sets);
    for _ in 0..sets {
        let len = VarInt::decode(&mut rest).ok()?.0 as usize;
        let key = std::str::from_utf8(rest.get(..len)?).ok()?.to_owned();
        rest = &rest[len..];
        let count = VarInt::decode(&mut rest).ok()?.0;
        let mut kept = Vec::new();
        for _ in 0..count {
            kept.push(VarInt::decode(&mut rest).ok()?.0);
        }
        let ids = match key.as_str() {
            "minecraft:furnace_input" => cooking_inputs(&recipes, tags, CookingKind::Smelting),
            "minecraft:blast_furnace_input" => cooking_inputs(&recipes, tags, CookingKind::Blasting),
            "minecraft:smoker_input" => cooking_inputs(&recipes, tags, CookingKind::Smoking),
            "minecraft:campfire_input" => {
                cooking_inputs(&recipes, tags, CookingKind::CampfireCooking)
            }
            "minecraft:smithing_template" => smithing_slot(&recipes, tags, 0),
            "minecraft:smithing_base" => smithing_slot(&recipes, tags, 1),
            "minecraft:smithing_addition" => smithing_slot(&recipes, tags, 2),
            // Brewing is not a recipe a plugin changes.
            _ => kept.iter().copied().collect(),
        };
        string(&mut out, &key);
        varint(&mut out, ids.len() as i32);
        // The vanilla order (a hash set's) is kept for the ids that stay, new ones follow.
        let mut ordered: Vec<i32> = kept.iter().copied().filter(|id| ids.contains(id)).collect();
        ordered.extend(ids.iter().copied().filter(|id| !kept.contains(id)));
        for id in ordered {
            varint(&mut out, id);
        }
    }
    let options: Vec<&&Recipe> = recipes
        .iter()
        .filter(|recipe| matches!(recipe.kind, RecipeKind::Stonecutting(_)))
        .collect();
    let mut entries = Vec::new();
    for recipe in options {
        let RecipeKind::Stonecutting(cutting) = &recipe.kind else {
            continue;
        };
        let Some(result) = result_of(recipe) else {
            continue;
        };
        let mut entry = holder_set(&cutting.ingredient, tags)?;
        entry.extend(stack_display(&result)?);
        entries.push(entry);
    }
    varint(&mut out, entries.len() as i32);
    for entry in entries {
        out.extend(entry);
    }
    Some(out)
}

fn cooking_inputs(recipes: &[&Recipe], tags: &ItemTags, kind: CookingKind) -> BTreeSet<i32> {
    recipes
        .iter()
        .filter_map(|recipe| match &recipe.kind {
            RecipeKind::Cooking(cooking) if cooking.kind == kind => Some(&cooking.ingredient),
            _ => None,
        })
        .flat_map(|ingredient| ingredient.resolved_items(tags))
        .filter_map(|item| item_id(&item).map(i32::from))
        .collect()
}

fn smithing_slot(
    recipes: &[&Recipe],
    tags: &ItemTags,
    slot: usize,
) -> BTreeSet<i32> {
    recipes
        .iter()
        .filter_map(|recipe| match &recipe.kind {
            RecipeKind::SmithingTransform(s) => Some((&s.template, &s.base, &s.addition)),
            RecipeKind::SmithingTrim(s) => Some((&s.template, &s.base, &s.addition)),
            _ => None,
        })
        .flat_map(|parts| [parts.0, parts.1, parts.2][slot].resolved_items(tags))
        .filter_map(|item| item_id(&item).map(i32::from))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::recipe_packets;
    use std::path::Path;

    fn data() -> Option<(RecipeBook, String)> {
        let root = [
            Path::new("vanilla/26.3/generated"),
            Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../vanilla/26.3/generated")),
        ]
        .into_iter()
        .find(|dir| dir.join("ferrite/recipes.json").is_file())?;
        let book = RecipeBook::load(root.join("data/minecraft")).ok()?;
        let json = std::fs::read_to_string(root.join("ferrite/recipes.json")).ok()?;
        Some((book, json))
    }

    fn stack_of(recipe: &Recipe) -> ItemStack {
        let result = recipe.result().expect("a result");
        ItemStack::from_name(&result.item)
            .unwrap()
            .copy_with_count(i32::try_from(result.count).unwrap())
    }

    /// The group id the extractor wrote after the display, so the comparison can feed it back.
    fn group_of(entry: &recipe_packets::Display) -> Option<i32> {
        let mut rest = &entry.wire[..];
        VarInt::decode(&mut rest).ok()?;
        let mut rest = &rest[entry.display.len()..];
        match VarInt::decode(&mut rest).ok()?.0 {
            0 => None,
            group => Some(group - 1),
        }
    }

    #[test]
    fn every_vanilla_recipe_of_a_supported_kind_encodes_to_the_bytes_the_server_wrote() {
        let Some((book, json)) = data() else {
            return;
        };
        let vanilla = recipe_packets::parse(&json);
        let mut checked = std::collections::BTreeMap::<&str, usize>::new();
        let mut skipped = 0;
        for recipe in book.recipes() {
            let Some(entry) = vanilla.recipes.get(&recipe.id).and_then(|r| r.displays.first())
            else {
                continue;
            };
            // A result with a component patch (suspicious stew) is dressed by the caller, not by
            // this test's plain stack.
            if recipe.result().is_some_and(|result| result.components.is_some()) {
                continue;
            }
            let kind = match &recipe.kind {
                RecipeKind::CraftingShaped(_) => "shaped",
                RecipeKind::CraftingShapeless(_) => "shapeless",
                RecipeKind::Cooking(_) => "cooking",
                RecipeKind::Stonecutting(_) => "stonecutting",
                RecipeKind::SmithingTransform(_) => "smithing",
                _ => continue,
            };
            let encoded = encode_entry(recipe, book.tags(), entry.index, group_of(entry), &stack_of(recipe))
                .unwrap_or_else(|| panic!("{} did not encode", recipe.id));
            // A set mixing items and tags lists the tag's members in the tag file's order, which
            // the loaded tag table (a hash set) no longer has; the client treats it as a set.
            if mixes_tags(recipe) {
                assert_eq!(encoded.display.len(), entry.display.len(), "{}", recipe.id);
                skipped += 1;
                continue;
            }
            assert_eq!(
                hex(&encoded.display),
                hex(&entry.display),
                "display of {}",
                recipe.id
            );
            assert_eq!(hex(&encoded.wire), hex(&entry.wire), "entry of {}", recipe.id);
            *checked.entry(kind).or_default() += 1;
        }
        assert!(skipped * 20 < checked.values().sum::<usize>(), "{skipped} recipes skipped");
        // Every supported kind was really compared.
        for kind in ["shaped", "shapeless", "cooking", "stonecutting", "smithing"] {
            assert!(checked.get(kind).copied().unwrap_or(0) > 0, "no {kind} recipe was compared");
        }
    }

    fn mixes_tags(recipe: &Recipe) -> bool {
        let mixed = |ingredient: &Ingredient| {
            ingredient.options.len() > 1
                && ingredient.options.iter().any(|o| matches!(o, IngredientEntry::Tag(_)))
        };
        match &recipe.kind {
            RecipeKind::CraftingShaped(r) => r.pattern.iter().flatten().flatten().any(mixed),
            RecipeKind::CraftingShapeless(r) => r.ingredients.iter().any(mixed),
            RecipeKind::Cooking(r) => mixed(&r.ingredient),
            RecipeKind::Stonecutting(r) => mixed(&r.ingredient),
            RecipeKind::SmithingTransform(r) => mixed(&r.template) || mixed(&r.base) || mixed(&r.addition),
            _ => false,
        }
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn the_update_body_of_the_unchanged_vanilla_set_is_the_vanilla_body() {
        let Some((book, json)) = data() else {
            return;
        };
        let base = hex_to_bytes(
            serde_json::from_str::<serde_json::Value>(&json).unwrap()["update_recipes"]
                .as_str()
                .unwrap(),
        );
        let body = update_body(&base, &book, &|_| true, &[], &|recipe| Some(stack_of(recipe)))
            .expect("the body encodes");
        // The item sets are equal as sets and the stonecutter options are the same list.
        assert_eq!(sets_of(&body), sets_of(&base));
        assert_eq!(hex(&stonecutter_of(&body)), hex(&stonecutter_of(&base)));
    }

    fn hex_to_bytes(text: &str) -> Vec<u8> {
        (0..text.len() / 2)
            .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).unwrap())
            .collect()
    }

    /// The property sets of an `update_recipes` body, ids sorted, and the offset where the
    /// stonecutter list starts.
    fn parse_sets(mut rest: &[u8]) -> (std::collections::BTreeMap<String, Vec<i32>>, &[u8]) {
        let mut out = std::collections::BTreeMap::new();
        let sets = VarInt::decode(&mut rest).unwrap().0;
        for _ in 0..sets {
            let len = VarInt::decode(&mut rest).unwrap().0 as usize;
            let key = String::from_utf8(rest[..len].to_vec()).unwrap();
            rest = &rest[len..];
            let count = VarInt::decode(&mut rest).unwrap().0;
            let mut ids: Vec<i32> = (0..count).map(|_| VarInt::decode(&mut rest).unwrap().0).collect();
            ids.sort_unstable();
            out.insert(key, ids);
        }
        (out, rest)
    }

    fn sets_of(body: &[u8]) -> std::collections::BTreeMap<String, Vec<i32>> {
        parse_sets(body).0
    }

    fn stonecutter_of(body: &[u8]) -> Vec<u8> {
        parse_sets(body).1.to_vec()
    }
}
