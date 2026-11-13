//! Crafting recipes for container menus, matched with `ferrite-recipe`.

use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use std::collections::{HashMap, HashSet};

use ferrite_inventory::{CookingOutput, Crafter, EnchantRules, FurnaceKind, NoCrafting};
use ferrite_item::{Component, ItemStack, PotionContents, Trim, component_ids};
use ferrite_recipe::{CookingKind, RecipeBook, RecipeKind};

/// Vanilla `CraftingMenu.slotChangedCraftingGrid`: the grid's result from the recipe book.
///
/// Special recipes (`crafting_special_*`, such as firework rockets or banner copying) have no
/// fixed result in `ferrite-recipe` and craft nothing. No 26.3 crafting result carries a
/// component patch, so a result is its item and count.
struct RecipeCrafter(RecipeBook);

/// Crafting recipes plugins changed at runtime (`Server#addRecipe`, `removeRecipe`,
/// `clearRecipes`, `resetRecipes`), applied on top of the vanilla book.
#[derive(Default)]
struct PluginRecipes {
    /// Added recipes, in the order they were added.
    added: Vec<ferrite_recipe::Recipe>,
    /// Ids of vanilla recipes that no longer craft.
    removed: HashSet<String>,
}

fn plugin_recipes() -> &'static RwLock<PluginRecipes> {
    static RECIPES: OnceLock<RwLock<PluginRecipes>> = OnceLock::new();
    RECIPES.get_or_init(Default::default)
}

/// A matched crafting recipe: its id and its fixed result.
struct Crafted {
    id: String,
    item: String,
    count: u32,
}

impl RecipeCrafter {
    /// The recipe a grid crafts: the first vanilla one a plugin has not removed, then the first
    /// plugin one. Special recipes without a fixed result match nothing.
    fn find(&self, names: &[Option<&str>], width: usize, height: usize) -> Option<Crafted> {
        let plugin = plugin_recipes().read().unwrap();
        let recipe = self
            .0
            .match_crafting_where(names, width, height, &|recipe| {
                !plugin.removed.contains(&recipe.id)
            })
            .or_else(|| {
                ferrite_recipe::match_crafting_in(
                    &plugin.added,
                    self.0.tags(),
                    names,
                    width,
                    height,
                )
            })?;
        let result = recipe.result()?;
        Some(Crafted {
            id: recipe.id.clone(),
            item: result.item.clone(),
            count: result.count,
        })
    }
}

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

/// One enchantment's anvil data from `data/minecraft/enchantment/*.json`.
struct EnchantEntry {
    max_level: i32,
    anvil_cost: i32,
    /// The `supported_items` reference: an item tag (`#...`) or one item id.
    supported: String,
    /// Enchantments in the `exclusive_set` tag, resolved.
    exclusive: HashSet<String>,
}

static ENCHANTMENTS: OnceLock<HashMap<String, EnchantEntry>> = OnceLock::new();

/// Loads every enchantment's maximum level, anvil weight, supported items and exclusive set.
fn enchantments() -> &'static HashMap<String, EnchantEntry> {
    ENCHANTMENTS.get_or_init(|| {
        let Some(dir) = data_dir() else {
            return HashMap::new();
        };
        let mut map = HashMap::new();
        let Ok(files) = std::fs::read_dir(dir.join("enchantment")) else {
            return map;
        };
        for file in files.flatten() {
            let path = file.path();
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            let int = |key: &str| json.get(key).and_then(serde_json::Value::as_i64);
            let (Some(max_level), Some(anvil_cost), Some(supported)) = (
                int("max_level"),
                int("anvil_cost"),
                json.get("supported_items")
                    .and_then(serde_json::Value::as_str),
            ) else {
                continue;
            };
            let exclusive = json
                .get("exclusive_set")
                .and_then(serde_json::Value::as_str)
                .map(|reference| enchantment_tag(&dir, reference))
                .unwrap_or_default();
            map.insert(
                format!("minecraft:{stem}"),
                EnchantEntry {
                    max_level: i32::try_from(max_level).unwrap_or(1),
                    anvil_cost: i32::try_from(anvil_cost).unwrap_or(1),
                    supported: supported.to_owned(),
                    exclusive,
                },
            );
        }
        map
    })
}

/// The enchantments in an enchantment tag reference such as `#minecraft:exclusive_set/damage`.
fn enchantment_tag(dir: &Path, reference: &str) -> HashSet<String> {
    let Some(tag) = reference.strip_prefix('#') else {
        return HashSet::from([reference.to_owned()]);
    };
    let path = dir.join("tags/enchantment").join(format!(
        "{}.json",
        tag.strip_prefix("minecraft:").unwrap_or(tag)
    ));
    let Some(json) = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
    else {
        return HashSet::new();
    };
    json.get("values")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .flat_map(|value| enchantment_tag(dir, value))
        .collect()
}

impl Crafter for RecipeCrafter {
    fn enchantment_rules(&self, enchantment: &str, item: &ItemStack) -> Option<EnchantRules> {
        let entry = enchantments().get(enchantment)?;
        let name = item.name()?;
        let supported = match entry.supported.strip_prefix('#') {
            Some(tag) => self.0.tags().contains(tag, name),
            None => entry.supported == name,
        };
        Some(EnchantRules {
            max_level: entry.max_level,
            anvil_cost: entry.anvil_cost,
            supported,
        })
    }

    fn enchantments_exclusive(&self, a: &str, b: &str) -> bool {
        enchantments()
            .get(a)
            .is_some_and(|entry| entry.exclusive.contains(b))
    }

    fn craft(&self, grid: &[ItemStack], width: usize) -> ItemStack {
        if width == 0 || !grid.len().is_multiple_of(width) {
            return ItemStack::empty();
        }
        let names: Vec<Option<&str>> = grid
            .iter()
            .map(|stack| if stack.is_empty() { None } else { stack.name() })
            .collect();
        self.find(&names, width, grid.len() / width)
            .and_then(|crafted| {
                let count = i32::try_from(crafted.count).ok()?;
                Some(ItemStack::from_name(&crafted.item)?.copy_with_count(count))
            })
            .unwrap_or_default()
    }

    fn recipe_id(&self, grid: &[ItemStack], width: usize) -> Option<String> {
        if width == 0 || !grid.len().is_multiple_of(width) {
            return None;
        }
        let names: Vec<Option<&str>> = grid
            .iter()
            .map(|stack| if stack.is_empty() { None } else { stack.name() })
            .collect();
        self.find(&names, width, grid.len() / width)
            .map(|crafted| crafted.id)
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
        self.smithing_match(template, base, addition)
            .map(|(_, result)| result)
    }

    fn recipe_layout(&self, id: &str, width: usize) -> Option<Vec<Option<Vec<String>>>> {
        let recipe = self.0.recipe(id)?;
        let tags = self.0.tags();
        let items = |ingredient: &ferrite_recipe::Ingredient| ingredient.resolved_items(tags);
        let mut cells = vec![None; width * width];
        match &recipe.kind {
            RecipeKind::CraftingShaped(shaped) => {
                if shaped.width > width || shaped.height > width {
                    return None;
                }
                for (row, line) in shaped.pattern.iter().enumerate() {
                    for (column, cell) in line.iter().enumerate() {
                        cells[row * width + column] = cell.as_ref().map(items);
                    }
                }
            }
            RecipeKind::CraftingShapeless(shapeless) => {
                if shapeless.ingredients.len() > cells.len() {
                    return None;
                }
                for (cell, ingredient) in cells.iter_mut().zip(&shapeless.ingredients) {
                    *cell = Some(items(ingredient));
                }
            }
            _ => return None,
        }
        Some(cells)
    }

    fn smithing_recipe_id(
        &self,
        template: &ItemStack,
        base: &ItemStack,
        addition: &ItemStack,
    ) -> Option<String> {
        self.smithing_match(template, base, addition)
            .map(|(id, _)| id)
    }
}

impl RecipeCrafter {
    /// The smithing recipe that applies, with its id and result.
    fn smithing_match(
        &self,
        template: &ItemStack,
        base: &ItemStack,
        addition: &ItemStack,
    ) -> Option<(String, ItemStack)> {
        let base_stack = base;
        let (Some(template), Some(base), Some(addition)) =
            (template.name(), base.name(), addition.name())
        else {
            return None;
        };
        let tags = self.0.tags();
        for recipe in self.0.recipes() {
            if let RecipeKind::SmithingTrim(trim) = &recipe.kind {
                if !trim.template.matches(template, tags)
                    || !trim.base.matches(base, tags)
                    || !trim.addition.matches(addition, tags)
                {
                    continue;
                }
                let (Some(material), Some(pattern)) =
                    (trim_material_of(addition), trim.pattern.as_deref())
                else {
                    continue;
                };
                let trim = Trim {
                    material: format!("minecraft:{material}"),
                    pattern: pattern.to_owned(),
                };
                // Vanilla `SmithingTrimRecipe`: the same trim again does nothing.
                if let Some(Component::Trim(old)) =
                    base_stack.effective_component(component_ids::TRIM)
                    && old == trim
                {
                    return None;
                }
                let mut out = base_stack.copy_with_count(1);
                out.components
                    .insert(component_ids::TRIM, Component::Trim(trim));
                return Some((recipe.id.clone(), out));
            }
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
                .map(|stack| (recipe.id.clone(), stack.copy_with_count(count)));
        }
        None
    }
}

/// The trim material an item provides (its `provides_trim_material` component in vanilla).
fn trim_material_of(item: &str) -> Option<&'static str> {
    Some(match item.strip_prefix("minecraft:").unwrap_or(item) {
        "amethyst_shard" => "amethyst",
        "copper_ingot" => "copper",
        "diamond" => "diamond",
        "emerald" => "emerald",
        "gold_ingot" => "gold",
        "iron_ingot" => "iron",
        "lapis_lazuli" => "lapis",
        "netherite_ingot" => "netherite",
        "quartz" => "quartz",
        "redstone" => "redstone",
        "resin_brick" => "resin",
        _ => return None,
    })
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

/// The recipe a plugin-supplied grid crafts: its id, result item key and count. Special recipes
/// without a fixed result match nothing.
pub fn match_for_plugin(
    names: &[Option<&str>],
    width: usize,
    height: usize,
) -> Option<(String, String, u32)> {
    let book = BOOK.get_or_init(load).as_ref()?;
    let crafted = book.find(names, width, height)?;
    Some((crafted.id, crafted.item, crafted.count))
}

/// `Server#addRecipe`: a shaped or shapeless crafting recipe in vanilla data pack JSON. `Ok(false)`
/// when the id is taken by a recipe that still crafts; `Err` for JSON that is not one of those
/// two types (cooking, smithing and the other kinds are not added at runtime).
pub fn add_plugin_recipe(id: &str, json: &str) -> Result<bool, String> {
    let Some(book) = BOOK.get_or_init(load).as_ref() else {
        return Err("crafting recipes are not loaded".into());
    };
    let recipe = ferrite_recipe::parse_recipe_json(id, json).map_err(|error| error.to_string())?;
    if !matches!(
        recipe.kind,
        RecipeKind::CraftingShaped(_) | RecipeKind::CraftingShapeless(_)
    ) {
        return Err(format!(
            "{} recipes cannot be added at runtime; only crafting_shaped and crafting_shapeless",
            recipe.type_name()
        ));
    }
    let mut plugin = plugin_recipes().write().unwrap();
    let taken = plugin.added.iter().any(|known| known.id == id)
        || (book.0.recipe(id).is_some() && !plugin.removed.contains(id));
    if taken {
        return Ok(false);
    }
    plugin.added.push(recipe);
    Ok(true)
}

/// `Server#removeRecipe`: a plugin recipe disappears, a vanilla one stops crafting. `false` when
/// nothing by that id crafts.
pub fn remove_recipe(id: &str) -> bool {
    let mut plugin = plugin_recipes().write().unwrap();
    if let Some(index) = plugin.added.iter().position(|known| known.id == id) {
        plugin.added.remove(index);
        return true;
    }
    let vanilla = BOOK
        .get_or_init(load)
        .as_ref()
        .is_some_and(|book| book.0.recipe(id).is_some());
    vanilla && plugin.removed.insert(id.to_owned())
}

/// `Server#clearRecipes`: no crafting recipe is left, vanilla ones included.
pub fn clear_recipes() {
    if let Some(book) = BOOK.get_or_init(load).as_ref() {
        clear_into(&mut plugin_recipes().write().unwrap(), &book.0);
    }
}

fn clear_into(plugin: &mut PluginRecipes, book: &RecipeBook) {
    plugin.added.clear();
    plugin.removed = book
        .recipes()
        .iter()
        .map(|recipe| recipe.id.clone())
        .collect();
}

/// `Server#resetRecipes`: vanilla recipes only, as loaded.
pub fn reset_recipes() {
    *plugin_recipes().write().unwrap() = PluginRecipes::default();
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
    fn vanilla_enchantment_rules_drive_the_anvil() {
        let crafter = crafter();
        let sword = stack("diamond_sword", 1);
        let sharpness = crafter
            .enchantment_rules("minecraft:sharpness", &sword)
            .expect("sharpness is a vanilla enchantment");
        assert_eq!(
            (
                sharpness.max_level,
                sharpness.anvil_cost,
                sharpness.supported
            ),
            (5, 1, true)
        );
        let protection = crafter
            .enchantment_rules("minecraft:protection", &sword)
            .unwrap();
        assert_eq!((protection.max_level, protection.supported), (4, false));
        assert!(
            crafter
                .enchantment_rules("minecraft:mending", &stack("bow", 1))
                .unwrap()
                .supported
        );
        assert!(
            crafter
                .enchantment_rules("minecraft:nonsense", &sword)
                .is_none()
        );
        assert!(crafter.enchantments_exclusive("minecraft:sharpness", "minecraft:smite"));
        assert!(!crafter.enchantments_exclusive("minecraft:sharpness", "minecraft:unbreaking"));

        // A sharpness V book on a sharpness IV sword in a real anvil menu.
        let mut inventory = ferrite_inventory::PlayerInventory::new();
        let mut menu = ferrite_inventory::Menu::anvil(1);
        let mut applied = ferrite_item::Enchantments::new();
        applied.set("minecraft:sharpness", 4);
        let mut sword = sword;
        sword.components.insert(
            component_ids::ENCHANTMENTS,
            Component::Enchantments(applied),
        );
        let mut stored = ferrite_item::Enchantments::new();
        stored.set("minecraft:sharpness", 4);
        stored.set("minecraft:smite", 2);
        let mut book = stack("enchanted_book", 1);
        book.components.insert(
            component_ids::STORED_ENCHANTMENTS,
            Component::StoredEnchantments(stored),
        );
        menu.set_slot(&mut inventory, crafter, 0, sword);
        menu.set_slot(&mut inventory, crafter, 1, book);
        let Some(Component::Enchantments(result)) = menu
            .result()
            .effective_component(component_ids::ENCHANTMENTS)
        else {
            panic!("the anvil produced no enchanted result");
        };
        assert_eq!(result.level("minecraft:sharpness"), Some(5));
        assert_eq!(
            result.level("minecraft:smite"),
            None,
            "smite excludes sharpness"
        );
        // sharpness V at weight 1 halved to max(1, 0) = 1 per level, plus smite's exclusion level.
        assert_eq!(menu.anvil_cost(), 5 + 1);
    }

    #[test]
    fn a_recipe_lays_out_on_the_grid_it_fits() {
        let crafter = crafter();
        // Sticks: two planks one above the other, in the top-left corner of either grid.
        let two = crafter
            .recipe_layout("minecraft:stick", 2)
            .expect("fits 2x2");
        assert_eq!(two.len(), 4);
        assert!(
            two[0]
                .as_ref()
                .is_some_and(|items| items.contains(&"minecraft:oak_planks".to_owned()))
        );
        assert!(two[2].is_some() && two[1].is_none() && two[3].is_none());
        // A crafting table fits 2x2; a chest (3x3 ring) does not.
        assert!(
            crafter
                .recipe_layout("minecraft:crafting_table", 2)
                .is_some()
        );
        assert!(crafter.recipe_layout("minecraft:chest", 2).is_none());
        assert!(crafter.recipe_layout("minecraft:chest", 3).is_some());
        assert!(
            crafter
                .recipe_layout("minecraft:no_such_recipe", 3)
                .is_none()
        );
    }

    #[test]
    fn a_smithing_table_trims_armor_and_refuses_the_same_trim_twice() {
        let crafter = crafter();
        let template = stack("bolt_armor_trim_smithing_template", 1);
        let armor = stack("iron_chestplate", 1);
        let trimmed = crafter
            .smithing(&template, &armor, &stack("amethyst_shard", 1))
            .expect("amethyst bolt trim");
        assert_eq!(trimmed.name(), armor.name());
        assert_eq!(
            crafter.smithing_recipe_id(&template, &armor, &stack("amethyst_shard", 1)),
            Some("minecraft:bolt_armor_trim_smithing_template_smithing_trim".to_owned())
        );
        assert_eq!(
            trimmed.effective_component(component_ids::TRIM),
            Some(Component::Trim(Trim {
                material: "minecraft:amethyst".to_owned(),
                pattern: "minecraft:bolt".to_owned(),
            }))
        );
        // The same trim again is no recipe; a different material replaces it.
        assert!(
            crafter
                .smithing(&template, &trimmed, &stack("amethyst_shard", 1))
                .is_none()
        );
        let copper = crafter
            .smithing(&template, &trimmed, &stack("copper_ingot", 1))
            .expect("copper bolt trim");
        assert!(matches!(
            copper.effective_component(component_ids::TRIM),
            Some(Component::Trim(trim)) if trim.material == "minecraft:copper"
        ));
        // Not armor, or not a trim material: nothing.
        assert!(
            crafter
                .smithing(&template, &stack("stick", 1), &stack("amethyst_shard", 1))
                .is_none()
        );
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

    /// The plugin recipe tests share one process-wide overlay, so they run one at a time.
    fn overlay_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    const DIAMOND_FROM_DIRT: &str = r#"{
        "type": "minecraft:crafting_shapeless",
        "ingredients": ["minecraft:dirt", "minecraft:dirt"],
        "result": {"id": "minecraft:diamond", "count": 3}
    }"#;

    #[test]
    fn a_plugin_recipe_crafts_until_it_is_removed() {
        let _guard = overlay_lock();
        let crafter = crafter();
        let grid = [
            stack("dirt", 1),
            stack("dirt", 1),
            ItemStack::empty(),
            ItemStack::empty(),
        ];
        assert_eq!(crafter.craft(&grid, 2), ItemStack::empty(), "no recipe yet");
        assert_eq!(
            add_plugin_recipe("plugin:two_dirt", DIAMOND_FROM_DIRT),
            Ok(true)
        );
        assert_eq!(crafter.craft(&grid, 2), stack("diamond", 3));
        assert_eq!(
            crafter.recipe_id(&grid, 2).as_deref(),
            Some("plugin:two_dirt")
        );
        // The id is taken now, by a plugin recipe and by a vanilla one.
        assert_eq!(
            add_plugin_recipe("plugin:two_dirt", DIAMOND_FROM_DIRT),
            Ok(false)
        );
        assert_eq!(
            add_plugin_recipe("minecraft:lead", DIAMOND_FROM_DIRT),
            Ok(false)
        );
        assert!(remove_recipe("plugin:two_dirt"));
        assert!(!remove_recipe("plugin:two_dirt"), "already gone");
        assert_eq!(crafter.craft(&grid, 2), ItemStack::empty());
    }

    #[test]
    fn removing_a_vanilla_recipe_stops_it_and_a_reset_brings_it_back() {
        let _guard = overlay_lock();
        let crafter = crafter();
        // A lead: four string and a slimeball (the 26.3 recipe's ingredients are read from the data).
        let lead = crafter
            .recipe_layout("minecraft:lead", 3)
            .expect("the lead recipe exists");
        let grid: Vec<ItemStack> = lead
            .iter()
            .map(|cell| match cell {
                Some(options) => ItemStack::from_name(&options[0]).unwrap(),
                None => ItemStack::empty(),
            })
            .collect();
        assert_eq!(crafter.craft(&grid, 3).name(), Some("minecraft:lead"));
        assert!(remove_recipe("minecraft:lead"));
        assert_eq!(crafter.craft(&grid, 3), ItemStack::empty());
        reset_recipes();
        assert_eq!(crafter.craft(&grid, 3).name(), Some("minecraft:lead"));
    }

    #[test]
    fn only_shaped_and_shapeless_recipes_are_added_and_bad_json_says_why() {
        let _guard = overlay_lock();
        let smelting = r#"{"type":"minecraft:smelting","ingredient":"minecraft:dirt","result":{"id":"minecraft:stone"},"experience":0.1,"cookingtime":200}"#;
        let refused = add_plugin_recipe("plugin:smelt", smelting).unwrap_err();
        assert!(refused.contains("crafting_shaped"), "{refused}");
        assert!(add_plugin_recipe("plugin:broken", "{").is_err());
    }

    #[test]
    fn clearing_hides_every_vanilla_recipe_and_the_plugin_ones() {
        let book = BOOK
            .get_or_init(load)
            .as_ref()
            .expect("the recipe book is loaded");
        let mut plugin = PluginRecipes::default();
        plugin
            .added
            .push(ferrite_recipe::parse_recipe_json("plugin:two_dirt", DIAMOND_FROM_DIRT).unwrap());
        clear_into(&mut plugin, &book.0);
        assert!(plugin.added.is_empty());
        assert_eq!(plugin.removed.len(), book.0.len());
        assert!(plugin.removed.contains("minecraft:lead"));
    }
}
