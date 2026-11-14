//! Indexed matching for crafting, cooking and stonecutting.

use crate::book::RecipeBook;
use crate::recipe::{
    CookingKind, CraftingShaped, CraftingShapeless, CraftingTransmute, Recipe, RecipeKind,
};
use crate::tags::ItemTags;

impl RecipeBook {
    /// Matches a crafting grid.
    ///
    /// `grid` is row-major with `width` columns; only the first
    /// `width * height` entries are read. Use [`RecipeBook::match_crafting_2x2`]
    /// or [`RecipeBook::match_crafting_3x3`] for the usual square grids.
    ///
    /// Shaped recipes are tried in every position and in mirrored form, as
    /// vanilla allows. Shapeless and transmute recipes compare the non-empty
    /// items as a multiset, with tag alternatives resolved through item tags.
    /// An empty grid never matches.
    pub fn match_crafting(
        &self,
        grid: &[Option<&str>],
        width: usize,
        height: usize,
    ) -> Option<&Recipe> {
        self.match_crafting_where(grid, width, height, &|_| true)
    }

    /// [`RecipeBook::match_crafting`] limited to the recipes `allow` accepts, for a caller that
    /// hides some of them (a plugin that removed a recipe).
    pub fn match_crafting_where(
        &self,
        grid: &[Option<&str>],
        width: usize,
        height: usize,
        allow: &dyn Fn(&Recipe) -> bool,
    ) -> Option<&Recipe> {
        let items = grid_items(grid, width, height)?;
        let mut candidates = Vec::new();
        for item in &items {
            if let Some(recipes) = self.crafting_candidates(item) {
                candidates.extend_from_slice(recipes);
            }
        }
        candidates.sort_unstable();
        candidates.dedup();

        for index in candidates {
            let recipe = &self.recipes()[index];
            if allow(recipe) && recipe_matches(recipe, grid, width, height, &items, self.tags()) {
                return Some(recipe);
            }
        }
        None
    }

    /// Matches a 2x2 crafting grid.
    pub fn match_crafting_2x2(&self, grid: &[Option<&str>; 4]) -> Option<&Recipe> {
        self.match_crafting(grid, 2, 2)
    }

    /// Matches a 3x3 crafting grid.
    pub fn match_crafting_3x3(&self, grid: &[Option<&str>; 9]) -> Option<&Recipe> {
        self.match_crafting(grid, 3, 3)
    }

    /// Matches a furnace-family input.
    pub fn match_cooking(&self, kind: CookingKind, item: &str) -> Option<&Recipe> {
        self.match_cooking_where(kind, item, &|_| true)
    }

    /// [`RecipeBook::match_cooking`] limited to the recipes `allow` accepts (a plugin removed
    /// the others).
    pub fn match_cooking_where(
        &self,
        kind: CookingKind,
        item: &str,
        allow: &dyn Fn(&Recipe) -> bool,
    ) -> Option<&Recipe> {
        let candidates = self.cooking_candidates(item)?;
        for index in candidates {
            let recipe = &self.recipes()[*index];
            if let RecipeKind::Cooking(cooking) = &recipe.kind
                && cooking.kind == kind
                && cooking.ingredient.matches(item, self.tags())
                && allow(recipe)
            {
                return Some(recipe);
            }
        }
        None
    }

    /// Every stonecutter recipe accepting `item`, in recipe-id order.
    pub fn stonecutting_options(&self, item: &str) -> Vec<&Recipe> {
        self.stonecutting_options_where(item, &|_| true)
    }

    /// [`RecipeBook::stonecutting_options`] limited to the recipes `allow` accepts.
    pub fn stonecutting_options_where(
        &self,
        item: &str,
        allow: &dyn Fn(&Recipe) -> bool,
    ) -> Vec<&Recipe> {
        let Some(candidates) = self.stonecutting_candidates(item) else {
            return Vec::new();
        };
        let mut recipes = Vec::with_capacity(candidates.len());
        for index in candidates {
            let recipe = &self.recipes()[*index];
            if let RecipeKind::Stonecutting(stonecutting) = &recipe.kind
                && stonecutting.ingredient.matches(item, self.tags())
                && allow(recipe)
            {
                recipes.push(recipe);
            }
        }
        recipes
    }
}

/// [`RecipeBook::match_cooking`] over a plain list of recipes (added at runtime), first match wins.
pub fn match_cooking_in<'a>(
    recipes: &'a [Recipe],
    tags: &ItemTags,
    kind: CookingKind,
    item: &str,
) -> Option<&'a Recipe> {
    recipes.iter().find(|recipe| {
        matches!(&recipe.kind, RecipeKind::Cooking(cooking)
            if cooking.kind == kind && cooking.ingredient.matches(item, tags))
    })
}

/// [`RecipeBook::stonecutting_options`] over a plain list of recipes, in list order.
pub fn stonecutting_in<'a>(recipes: &'a [Recipe], tags: &ItemTags, item: &str) -> Vec<&'a Recipe> {
    recipes
        .iter()
        .filter(|recipe| {
            matches!(&recipe.kind, RecipeKind::Stonecutting(cutting)
                if cutting.ingredient.matches(item, tags))
        })
        .collect()
}

impl CraftingShaped {
    pub(crate) fn matches(
        &self,
        grid: &[Option<&str>],
        width: usize,
        height: usize,
        tags: &ItemTags,
    ) -> bool {
        if width < self.width || height < self.height {
            return false;
        }
        if fits_pattern(
            &self.pattern,
            self.width,
            self.height,
            grid,
            width,
            height,
            tags,
        ) {
            return true;
        }
        self.pattern != self.mirrored_pattern
            && fits_pattern(
                &self.mirrored_pattern,
                self.width,
                self.height,
                grid,
                width,
                height,
                tags,
            )
    }
}

fn fits_pattern(
    pattern: &[Vec<Option<crate::recipe::Ingredient>>],
    pattern_width: usize,
    pattern_height: usize,
    grid: &[Option<&str>],
    width: usize,
    height: usize,
    tags: &ItemTags,
) -> bool {
    for offset_y in 0..=height - pattern_height {
        for offset_x in 0..=width - pattern_width {
            if matches_at(
                pattern,
                pattern_width,
                pattern_height,
                offset_x,
                offset_y,
                grid,
                width,
                height,
                tags,
            ) {
                return true;
            }
        }
    }
    false
}

#[allow(clippy::too_many_arguments)]
fn matches_at(
    pattern: &[Vec<Option<crate::recipe::Ingredient>>],
    pattern_width: usize,
    pattern_height: usize,
    offset_x: usize,
    offset_y: usize,
    grid: &[Option<&str>],
    width: usize,
    height: usize,
    tags: &ItemTags,
) -> bool {
    for y in 0..height {
        for x in 0..width {
            let item = grid[y * width + x];
            let inside = x >= offset_x
                && x < offset_x + pattern_width
                && y >= offset_y
                && y < offset_y + pattern_height;
            if inside {
                let cell = &pattern[y - offset_y][x - offset_x];
                match (cell, item) {
                    (Some(ingredient), Some(item)) => {
                        if !ingredient.matches(item, tags) {
                            return false;
                        }
                    }
                    (None, None) => {}
                    _ => return false,
                }
            } else if item.is_some() {
                return false;
            }
        }
    }
    true
}

impl CraftingShapeless {
    pub(crate) fn matches(&self, items: &[&str], tags: &ItemTags) -> bool {
        if self.ingredients.len() != items.len() || items.is_empty() {
            return false;
        }
        let mut matched_item = vec![usize::MAX; items.len()];
        for ingredient in 0..self.ingredients.len() {
            let mut visited = vec![false; items.len()];
            if !augment(
                ingredient,
                &self.ingredients,
                items,
                tags,
                &mut visited,
                &mut matched_item,
            ) {
                return false;
            }
        }
        true
    }
}

fn augment(
    ingredient: usize,
    ingredients: &[crate::recipe::Ingredient],
    items: &[&str],
    tags: &ItemTags,
    visited: &mut [bool],
    matched_item: &mut [usize],
) -> bool {
    for item in 0..items.len() {
        if visited[item] || !ingredients[ingredient].matches(items[item], tags) {
            continue;
        }
        visited[item] = true;
        let previous = matched_item[item];
        if previous == usize::MAX
            || augment(previous, ingredients, items, tags, visited, matched_item)
        {
            matched_item[item] = ingredient;
            return true;
        }
    }
    false
}

impl CraftingTransmute {
    pub(crate) fn matches(&self, items: &[&str], tags: &ItemTags) -> bool {
        let (min, max) = match self.material_count {
            Some(count) => (count.min as usize, count.max as usize),
            None => (1, 1),
        };
        if items.len() < 1 + min || items.len() > 1 + max {
            return false;
        }

        for input in 0..items.len() {
            if !self.input.matches(items[input], tags) {
                continue;
            }
            if items
                .iter()
                .enumerate()
                .all(|(index, item)| index == input || self.material.matches(item, tags))
            {
                return true;
            }
        }
        false
    }
}

/// The non-empty items of a grid, or `None` for an empty grid or a size vanilla has no table for.
fn grid_items<'a>(grid: &[Option<&'a str>], width: usize, height: usize) -> Option<Vec<&'a str>> {
    if width == 0 || height == 0 || width > 3 || height > 3 {
        return None;
    }
    let cells = width.checked_mul(height)?;
    if grid.len() < cells {
        return None;
    }
    let mut items = Vec::with_capacity(cells);
    for y in 0..height {
        for x in 0..width {
            if let Some(item) = grid[y * width + x] {
                items.push(item);
            }
        }
    }
    (!items.is_empty()).then_some(items)
}

fn recipe_matches(
    recipe: &Recipe,
    grid: &[Option<&str>],
    width: usize,
    height: usize,
    items: &[&str],
    tags: &ItemTags,
) -> bool {
    match &recipe.kind {
        RecipeKind::CraftingShaped(shaped) => shaped.matches(grid, width, height, tags),
        RecipeKind::CraftingShapeless(shapeless) => shapeless.matches(items, tags),
        RecipeKind::CraftingTransmute(transmute) => transmute.matches(items, tags),
        _ => false,
    }
}

/// Matches a crafting grid against a plain list of recipes, in order: the first match wins.
/// Used for recipes added at runtime, which are not part of a loaded [`RecipeBook`].
pub fn match_crafting_in<'a>(
    recipes: &'a [Recipe],
    tags: &ItemTags,
    grid: &[Option<&str>],
    width: usize,
    height: usize,
) -> Option<&'a Recipe> {
    let items = grid_items(grid, width, height)?;
    recipes
        .iter()
        .find(|recipe| recipe_matches(recipe, grid, width, height, &items, tags))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_recipe_json;

    const SHAPED: &str = r#"{
        "type": "minecraft:crafting_shaped",
        "pattern": ["SS", "S "],
        "key": {"S": "minecraft:stone"},
        "result": {"id": "minecraft:diamond", "count": 2}
    }"#;
    const SHAPELESS: &str = r#"{
        "type": "minecraft:crafting_shapeless",
        "ingredients": ["minecraft:dirt", "minecraft:sand"],
        "result": {"id": "minecraft:clay_ball", "count": 1}
    }"#;

    #[test]
    fn runtime_recipes_match_in_order_and_in_mirrored_and_shifted_positions() {
        let recipes = [
            parse_recipe_json("plugin:corner", SHAPED).unwrap(),
            parse_recipe_json("plugin:mix", SHAPELESS).unwrap(),
        ];
        let tags = ItemTags::default();
        let stone = Some("minecraft:stone");
        // The shaped pattern in a 3x3 grid, shifted down one row.
        let grid = [None, None, None, stone, stone, None, stone, None, None];
        assert_eq!(
            match_crafting_in(&recipes, &tags, &grid, 3, 3).map(|r| r.id.as_str()),
            Some("plugin:corner")
        );
        // Mirrored, as vanilla allows.
        let mirrored = [stone, stone, None, stone];
        assert_eq!(
            match_crafting_in(&recipes, &tags, &mirrored, 2, 2).map(|r| r.id.as_str()),
            Some("plugin:corner")
        );
        let mix = [Some("minecraft:sand"), None, Some("minecraft:dirt"), None];
        assert_eq!(
            match_crafting_in(&recipes, &tags, &mix, 2, 2).map(|r| r.id.as_str()),
            Some("plugin:mix")
        );
        // An empty grid, an extra item and an unsupported grid size match nothing.
        assert!(match_crafting_in(&recipes, &tags, &[None; 4], 2, 2).is_none());
        let extra = [
            Some("minecraft:sand"),
            Some("minecraft:dirt"),
            Some("minecraft:dirt"),
            None,
        ];
        assert!(match_crafting_in(&recipes, &tags, &extra, 2, 2).is_none());
        assert!(match_crafting_in(&recipes, &tags, &[stone; 16], 4, 4).is_none());
    }

    #[test]
    fn runtime_cooking_recipes_match_by_kind_and_stonecutting_lists_every_option() {
        let smelt = |id: &str, kind: &str, item: &str| {
            parse_recipe_json(
                id,
                &format!(
                    r#"{{"type":"minecraft:{kind}","ingredient":"minecraft:{item}","result":{{"id":"minecraft:glass"}},"experience":0.5,"cookingtime":40}}"#
                ),
            )
            .unwrap()
        };
        let recipes = [
            smelt("plugin:a", "smelting", "gravel"),
            smelt("plugin:b", "smoking", "gravel"),
            smelt("plugin:c", "campfire_cooking", "gravel"),
            smelt("plugin:d", "blasting", "gravel"),
        ];
        let tags = ItemTags::default();
        for (kind, id) in [
            (CookingKind::Smelting, "plugin:a"),
            (CookingKind::Smoking, "plugin:b"),
            (CookingKind::CampfireCooking, "plugin:c"),
            (CookingKind::Blasting, "plugin:d"),
        ] {
            let found = match_cooking_in(&recipes, &tags, kind, "minecraft:gravel");
            assert_eq!(found.map(|r| r.id.as_str()), Some(id));
        }
        assert!(
            match_cooking_in(&recipes, &tags, CookingKind::Smelting, "minecraft:sand").is_none()
        );

        let cut = |id: &str, item: &str| {
            parse_recipe_json(
                id,
                &format!(
                    r#"{{"type":"minecraft:stonecutting","ingredient":"minecraft:{item}","result":{{"id":"minecraft:stick","count":2}}}}"#
                ),
            )
            .unwrap()
        };
        let cuts = [
            cut("plugin:x", "stone"),
            cut("plugin:y", "dirt"),
            cut("plugin:z", "stone"),
        ];
        let ids: Vec<_> = stonecutting_in(&cuts, &tags, "minecraft:stone")
            .iter()
            .map(|r| r.id.as_str())
            .collect();
        assert_eq!(ids, ["plugin:x", "plugin:z"]);
        assert!(stonecutting_in(&cuts, &tags, "minecraft:sand").is_empty());
    }

    #[test]
    fn an_ingredient_may_be_a_list_of_items_and_a_pattern_may_have_gaps() {
        // The shape the Java API sends for a `MaterialChoice` of several materials.
        let recipe = parse_recipe_json(
            "plugin:either",
            r#"{
                "type": "minecraft:crafting_shaped",
                "pattern": ["A B"],
                "key": {"A": "minecraft:dirt", "B": ["minecraft:sand", "minecraft:gravel"]},
                "result": {"id": "minecraft:clay_ball", "count": 1}
            }"#,
        )
        .unwrap();
        let recipes = [recipe];
        let tags = ItemTags::default();
        let dirt = Some("minecraft:dirt");
        for second in ["minecraft:sand", "minecraft:gravel"] {
            let grid = [dirt, None, Some(second)];
            assert!(
                match_crafting_in(&recipes, &tags, &grid, 3, 1).is_some(),
                "{second}"
            );
        }
        let grid = [dirt, None, Some("minecraft:stone")];
        assert!(match_crafting_in(&recipes, &tags, &grid, 3, 1).is_none());
        // The gap matters: dirt and sand side by side is a different shape.
        let grid = [dirt, Some("minecraft:sand"), None];
        assert!(match_crafting_in(&recipes, &tags, &grid, 3, 1).is_none());
    }

    #[test]
    fn a_runtime_recipe_in_bad_json_or_of_an_unknown_shape_is_an_error() {
        assert!(parse_recipe_json("plugin:bad", "{").is_err());
        assert!(
            parse_recipe_json("plugin:bad", r#"{"type":"minecraft:crafting_shaped"}"#).is_err()
        );
        assert!(parse_recipe_json("plugin:bad", "[]").is_err());
    }
}
