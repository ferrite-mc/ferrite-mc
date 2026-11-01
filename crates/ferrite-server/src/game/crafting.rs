//! Crafting recipes for container menus, matched with `ferrite-recipe`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use ferrite_inventory::{CookingOutput, Crafter, FurnaceKind, NoCrafting};
use ferrite_item::{Component, ItemStack, PotionContents, component_ids};
use ferrite_recipe::{CookingKind, RecipeBook, RecipeKind};

/// Vanilla `CraftingMenu.slotChangedCraftingGrid`: the grid's result from the recipe book.
///
/// Special recipes (`crafting_special_*`, such as firework rockets or banner copying) have no
/// fixed result in `ferrite-recipe` and craft nothing. No 26.3 crafting result carries a
/// component patch, so a result is its item and count.
struct RecipeCrafter(RecipeBook);

impl RecipeCrafter {
    /// Vanilla `RecipeManager.getRecipeFor(RecipeType, input)` for one furnace family, with the
    /// 26.3 recipe's result, cooking time and experience.
    fn cooking(&self, kind: FurnaceKind, input: &ItemStack) -> Option<CookingOutput> {
        let cooking_kind = match kind {
            FurnaceKind::Furnace => CookingKind::Smelting,
            FurnaceKind::BlastFurnace => CookingKind::Blasting,
            FurnaceKind::Smoker => CookingKind::Smoking,
        };
        let recipe = self.0.match_cooking(cooking_kind, input.name()?)?;
        let RecipeKind::Cooking(cooking) = &recipe.kind else {
            return None;
        };
        let result = ItemStack::from_name(&cooking.result.item)?
            .copy_with_count(i32::try_from(cooking.result.count).ok()?);
        Some(CookingOutput {
            result,
            cook_time: i32::try_from(cooking.cooking_time).ok()?,
            experience: cooking.experience,
        })
    }
}

impl Crafter for RecipeCrafter {
    fn craft(&self, grid: &[ItemStack], width: usize) -> ItemStack {
        if width == 0 || !grid.len().is_multiple_of(width) {
            return ItemStack::empty();
        }
        let names: Vec<Option<&str>> = grid
            .iter()
            .map(|stack| if stack.is_empty() { None } else { stack.name() })
            .collect();
        self.0
            .match_crafting(&names, width, grid.len() / width)
            .and_then(|recipe| recipe.result())
            .and_then(|result| {
                let count = i32::try_from(result.count).ok()?;
                Some(ItemStack::from_name(&result.item)?.copy_with_count(count))
            })
            .unwrap_or_default()
    }

    fn cook(&self, kind: FurnaceKind, input: &ItemStack) -> Option<CookingOutput> {
        self.cooking(kind, input)
    }

    fn brew(&self, ingredient: &ItemStack, bottle: &ItemStack) -> Option<ItemStack> {
        if ingredient.is_empty() || bottle.is_empty() {
            return None;
        }
        let contents = match bottle.effective_component(component_ids::POTION_CONTENTS) {
            Some(Component::PotionContents(contents)) => contents,
            _ => return None,
        };
        let brewed = ferrite_recipe::brewing::mix(
            ingredient.name()?,
            bottle.name()?,
            contents.potion.as_deref(),
        )?;
        let mut out = bottle.clone();
        if let Some(item) = brewed.item {
            out.item = ItemStack::from_name(item)?.item;
        }
        let potion = brewed
            .potion
            .map_or_else(|| contents.potion.clone(), |potion| Some(potion.to_owned()));
        out.components.insert(
            component_ids::POTION_CONTENTS,
            Component::PotionContents(PotionContents { potion, ..contents }),
        );
        Some(out)
    }

    fn is_brewing_ingredient(&self, item: &ItemStack) -> bool {
        item.name()
            .is_some_and(ferrite_recipe::brewing::is_ingredient)
    }

    fn burn_time(&self, item: &ItemStack) -> i32 {
        item.name().map_or(0, fuel_burn_time)
    }

    fn stonecutting(&self, input: &ItemStack) -> Vec<ItemStack> {
        let Some(name) = input.name() else {
            return Vec::new();
        };
        self.0
            .stonecutting_options(name)
            .into_iter()
            .filter_map(|recipe| recipe.result())
            .filter_map(|result| {
                let count = i32::try_from(result.count).ok()?;
                ItemStack::from_name(&result.item).map(|stack| stack.copy_with_count(count))
            })
            .collect()
    }

    fn smithing(
        &self,
        template: &ItemStack,
        base: &ItemStack,
        addition: &ItemStack,
    ) -> Option<ItemStack> {
        let (Some(template), Some(base), Some(addition)) =
            (template.name(), base.name(), addition.name())
        else {
            return None;
        };
        let tags = self.0.tags();
        for recipe in self.0.recipes() {
            let RecipeKind::SmithingTransform(transform) = &recipe.kind else {
                continue;
            };
            if !transform.template.matches(template, tags)
                || !transform.base.matches(base, tags)
                || !transform.addition.matches(addition, tags)
            {
                continue;
            }
            let count = i32::try_from(transform.result.count).ok()?;
            return ItemStack::from_name(&transform.result.item)
                .map(|stack| stack.copy_with_count(count));
        }
        None
    }
}

/// Vanilla `FuelValues.burnDuration` for the 26.3 fuels this server knows (the generated data
/// pack does not ship the `furnace_fuel` values, so the common fuels are listed here; anything
/// absent is not a fuel).
fn fuel_burn_time(name: &str) -> i32 {
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    match name {
        "coal" | "charcoal" => 1600,
        "coal_block" => 16000,
        "lava_bucket" => 20000,
        "blaze_rod" => 2400,
        "dried_kelp_block" => 4000,
        "magma_cream" => 800,
        "bamboo" | "stick" | "bowl" => 100,
        "sapling" => 100,
        _ if name.ends_with("_sapling") => 100,
        _ if name.ends_with("_planks")
            || name.ends_with("_log")
            || name.ends_with("_wood")
            || name.ends_with("_fence")
            || name.ends_with("_fence_gate")
            || name.ends_with("_stairs")
            || name == "crafting_table"
            || name == "chest"
            || name == "trapped_chest"
            || name == "bookshelf"
            || name == "ladder"
            || name == "note_block"
            || name == "jukebox"
            || name == "cartography_table"
            || name == "fletching_table"
            || name == "smithing_table"
            || name == "loom"
            || name == "barrel" =>
        {
            300
        }
        _ if name.ends_with("_slab") => 150,
        _ if name.ends_with("_wool") => 100,
        _ if name.ends_with("_carpet") => 67,
        _ if name.ends_with("_door") => 200,
        _ if name.ends_with("_boat") || name.ends_with("_sign") => 200,
        _ if name.ends_with("_wooden_sword")
            || name.ends_with("_wooden_pickaxe")
            || name.ends_with("_wooden_axe")
            || name.ends_with("_wooden_shovel")
            || name.ends_with("_wooden_hoe") =>
        {
            200
        }
        _ => 0,
    }
}

/// The generated vanilla data pack: `vanilla/26.3/generated` under the working directory, or
/// the repository checkout this binary was built from.
fn data_dir() -> Option<PathBuf> {
    let relative = Path::new("vanilla/26.3/generated/data/minecraft");
    [
        relative.to_path_buf(),
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(relative),
    ]
    .into_iter()
    .find(|dir| dir.join("recipe").is_dir())
}

fn load() -> Option<RecipeCrafter> {
    let Some(dir) = data_dir() else {
        tracing::warn!("no vanilla recipe data found; crafting is disabled");
        return None;
    };
    match RecipeBook::load(&dir) {
        Ok(book) => {
            tracing::info!(recipes = book.len(), "loaded crafting recipes");
            Some(RecipeCrafter(book))
        }
        Err(error) => {
            tracing::error!(%error, "failed to load recipes; crafting is disabled");
            None
        }
    }
}

static BOOK: OnceLock<Option<RecipeCrafter>> = OnceLock::new();

/// Loads the recipe book now, so the first crafting click does not stall a region tick.
pub fn preload() {
    BOOK.get_or_init(load);
}

/// The crafter every menu uses.
pub fn crafter() -> &'static dyn Crafter {
    match BOOK.get_or_init(load) {
        Some(book) => book,
        None => &NoCrafting,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(name: &str, count: i32) -> ItemStack {
        ItemStack::from_name(name).unwrap().copy_with_count(count)
    }

    #[test]
    fn crafts_planks_sticks_and_tables_in_both_grids() {
        let crafter = crafter();
        let empty = ItemStack::empty;
        // A log anywhere in the 2x2 grid makes four planks.
        let grid = [empty(), empty(), stack("oak_log", 3), empty()];
        assert_eq!(crafter.craft(&grid, 2), stack("oak_planks", 4));
        // Two planks stacked vertically make four sticks, in any column of the 3x3 grid.
        let mut grid = vec![empty(); 9];
        grid[2] = stack("oak_planks", 1);
        grid[5] = stack("oak_planks", 1);
        assert_eq!(crafter.craft(&grid, 3), stack("stick", 4));
        let grid: [ItemStack; 4] = std::array::from_fn(|_| stack("birch_planks", 1));
        assert_eq!(crafter.craft(&grid, 2), stack("crafting_table", 1));
        // Side by side they are a pressure plate; dirt and stone are no recipe.
        let grid = [
            stack("oak_planks", 1),
            stack("oak_planks", 1),
            empty(),
            empty(),
        ];
        assert_eq!(crafter.craft(&grid, 2), stack("oak_pressure_plate", 1));
        let grid = [stack("dirt", 1), stack("stone", 1), empty(), empty()];
        assert_eq!(crafter.craft(&grid, 2), empty());
        // A three-wide recipe (a chest) needs the 3x3 grid.
        let mut grid: Vec<ItemStack> = (0..9).map(|_| stack("oak_planks", 1)).collect();
        grid[4] = empty();
        assert_eq!(crafter.craft(&grid, 3), stack("chest", 1));
        assert_eq!(crafter.craft(&[], 2), empty());
    }
}
