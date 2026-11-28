//! Crafting recipes for container menus, matched with `ferrite-recipe`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use std::collections::{HashMap, HashSet};

use ferrite_inventory::{CookingOutput, Crafter, EnchantRules, FurnaceKind, NoCrafting};
use ferrite_item::{Component, ItemStack, PotionContents, Trim, component_ids};
use ferrite_recipe::{CookingKind, RecipeBook, RecipeKind};

/// Vanilla `CraftingMenu.slotChangedCraftingGrid`: the grid's result from the recipe book.
///
/// Special recipes (`crafting_special_*`, such as firework rockets or banner copying) have no
/// fixed result in `ferrite-recipe` and craft nothing. A transmute recipe (dyeing a shulker box
/// or bundle, cloning a map) keeps the components of its input stack, as
/// `TransmuteRecipe.assemble` does with `transmuteCopy`; every other result is its item and count.
struct RecipeCrafter(RecipeBook);

/// A matched crafting recipe: its id and its fixed result.
struct Crafted {
    id: String,
    item: String,
    count: u32,
    /// The matched recipe when it is a transmute recipe.
    transmute: Option<ferrite_recipe::CraftingTransmute>,
}

impl RecipeCrafter {
    /// The recipe a grid crafts. Special recipes without a fixed result match nothing.
    fn find(&self, names: &[Option<&str>], width: usize, height: usize) -> Option<Crafted> {
        let recipe = self.0.match_crafting(names, width, height)?;
        if let RecipeKind::CraftingTransmute(transmute) = &recipe.kind {
            // A transmute recipe without a result item (map cloning) keeps the input's item.
            return Some(Crafted {
                id: recipe.id.clone(),
                item: transmute
                    .result
                    .as_ref()
                    .map(|result| result.item.clone())
                    .unwrap_or_default(),
                count: transmute.result.as_ref().map_or(1, |result| result.count),
                transmute: Some(transmute.clone()),
            });
        }
        let result = recipe.result()?;
        Some(Crafted {
            id: recipe.id.clone(),
            item: result.item.clone(),
            count: result.count,
            transmute: None,
        })
    }
}

impl RecipeCrafter {
    /// `TransmuteRecipe.assemble`: the first input stack with its item replaced by the recipe's
    /// result (or kept, for map cloning) and every component carried over, so a dyed shulker box
    /// keeps its contents. A recipe that adds its material count to the result counts the
    /// material stacks in the grid.
    fn transmute(
        &self,
        grid: &[ItemStack],
        recipe: &ferrite_recipe::CraftingTransmute,
        result_item: &str,
        base_count: i32,
    ) -> Option<ItemStack> {
        let tags = self.0.tags();
        let is_input = |stack: &ItemStack| {
            !stack.is_empty()
                && stack
                    .name()
                    .is_some_and(|name| recipe.input.matches(name, tags))
        };
        let input = grid.iter().find(|stack| is_input(stack))?;
        let materials = if recipe.add_material_count_to_result {
            let counted = grid
                .iter()
                .filter(|stack| {
                    !stack.is_empty()
                        && !is_input(stack)
                        && stack
                            .name()
                            .is_some_and(|name| recipe.material.matches(name, tags))
                })
                .count();
            i32::try_from(counted).ok()?
        } else {
            0
        };
        let mut out = input.clone();
        if !result_item.is_empty() {
            out.item = ferrite_item::item_id(result_item)?;
        }
        out.count = base_count + materials;
        Some(out)
    }

    /// Vanilla `RecipeManager.getRecipeFor(RecipeType, input)` for one furnace family, with the
    /// 26.3 recipe's result, cooking time and experience.
    fn cooking(&self, kind: FurnaceKind, input: &ItemStack) -> Option<CookingOutput> {
        let cooking_kind = match kind {
            FurnaceKind::Furnace => CookingKind::Smelting,
            FurnaceKind::BlastFurnace => CookingKind::Blasting,
            FurnaceKind::Smoker => CookingKind::Smoking,
        };
        self.cooking_of(cooking_kind, input)
    }

    /// The cooking recipe of any kind (campfires included) for `input`.
    fn cooking_of(&self, kind: CookingKind, input: &ItemStack) -> Option<CookingOutput> {
        let name = input.name()?;
        let recipe = self.0.match_cooking(kind, name)?;
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

/// A [`Crafter`] view that only crafts recipes the player has unlocked, for the
/// `limited_crafting` game rule. Every other operation (cooking, brewing, ...) belongs to the
/// wrapped crafter unchanged; this view is only used for menus with a crafting grid.
pub struct LimitedCrafter<'a> {
    base: &'a dyn Crafter,
    unlocked: std::collections::BTreeSet<String>,
}

impl<'a> LimitedCrafter<'a> {
    pub fn new(base: &'a dyn Crafter, unlocked: std::collections::BTreeSet<String>) -> Self {
        Self { base, unlocked }
    }

    /// The grid's recipe when this player unlocked it.
    fn unlocked_recipe(&self, grid: &[ItemStack], width: usize) -> Option<String> {
        self.base
            .recipe_id(grid, width)
            .filter(|id| self.unlocked.contains(id))
    }
}

impl Crafter for LimitedCrafter<'_> {
    fn craft(&self, grid: &[ItemStack], width: usize) -> ItemStack {
        if self.unlocked_recipe(grid, width).is_some() {
            self.base.craft(grid, width)
        } else {
            ItemStack::empty()
        }
    }

    fn recipe_id(&self, grid: &[ItemStack], width: usize) -> Option<String> {
        self.unlocked_recipe(grid, width)
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
    /// `min_cost`: `Enchantment.getMinCost(level) = base + (level - 1) * per_level_above_first`.
    min_cost: (i32, i32),
    /// Whether the enchantment is in `#minecraft:curse`.
    curse: bool,
}

static ENCHANTMENTS: OnceLock<HashMap<String, EnchantEntry>> = OnceLock::new();

/// Loads every enchantment's maximum level, anvil weight, supported items and exclusive set.
fn enchantments() -> &'static HashMap<String, EnchantEntry> {
    ENCHANTMENTS.get_or_init(|| {
        let Some(dir) = data_dir() else {
            return HashMap::new();
        };
        let mut map = HashMap::new();
        let curses = enchantment_tag(&dir, "#minecraft:curse");
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
            let min_cost = {
                let field = |name: &str| {
                    json.get("min_cost")
                        .and_then(|cost| cost.get(name))
                        .and_then(serde_json::Value::as_i64)
                        .and_then(|number| i32::try_from(number).ok())
                        .unwrap_or(0)
                };
                (field("base"), field("per_level_above_first"))
            };
            map.insert(
                format!("minecraft:{stem}"),
                EnchantEntry {
                    min_cost,
                    curse: curses.contains(&format!("minecraft:{stem}")),
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

    fn grindstone_enchantment(&self, enchantment: &str, level: i32) -> Option<(i32, bool)> {
        let entry = enchantments().get(enchantment)?;
        let (base, per_level) = entry.min_cost;
        Some((base + (level - 1) * per_level, entry.curse))
    }

    fn random_below(&self, bound: i32) -> i32 {
        use std::hash::BuildHasher;
        if bound <= 0 {
            return 0;
        }
        // `RandomState` is seeded from the OS per call, which is all an XP roll needs.
        let roll = std::collections::hash_map::RandomState::new().hash_one(0u8);
        (roll % u64::from(bound.unsigned_abs())) as i32
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
                if let Some(transmute) = &crafted.transmute {
                    return self.transmute(grid, transmute, &crafted.item, count);
                }
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
            .filter_map(|recipe| {
                let result = recipe.result()?;
                let count = i32::try_from(result.count).ok()?;
                Some(ItemStack::from_name(&result.item)?.copy_with_count(count))
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
        let name_of = |stack: &ItemStack| if stack.is_empty() { None } else { stack.name() };
        let base = name_of(base)?;
        let (template, addition) = (name_of(template), name_of(addition));
        let tags = self.0.tags();
        for recipe in self.0.recipes() {
            if let RecipeKind::SmithingTrim(trim) = &recipe.kind {
                if !slot_matches(&trim.template, template, tags)
                    || !trim.base.matches(base, tags)
                    || !slot_matches(&trim.addition, addition, tags)
                {
                    continue;
                }
                let (Some(material), Some(pattern)) =
                    (addition.and_then(trim_material_of), trim.pattern.as_deref())
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
            if !slot_matches(&transform.template, template, tags)
                || !transform.base.matches(base, tags)
                || !slot_matches(&transform.addition, addition, tags)
            {
                continue;
            }
            let count = i32::try_from(transform.result.count).ok()?;
            // Vanilla `TransmuteRecipe.createWithOriginalComponents`: the result item keeps the
            // base item's components (enchantments, name, damage), so an enchanted diamond
            // sword upgrades to an enchanted netherite sword.
            let mut out = base_stack.copy_with_count(count);
            out.item = ItemStack::from_name(&transform.result.item)?.item;
            return Some((recipe.id.clone(), out));
        }
        None
    }
}

/// Whether a smithing slot holding `item` satisfies `ingredient`. A smithing recipe's template and
/// addition are optional in vanilla (`Optional<Ingredient>`): an absent one is an ingredient
/// with no alternatives, which only an empty slot satisfies.
fn slot_matches(
    ingredient: &ferrite_recipe::Ingredient,
    item: Option<&str>,
    tags: &ferrite_recipe::ItemTags,
) -> bool {
    match item {
        None => ingredient.is_empty(),
        Some(item) => ingredient.matches(item, tags),
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
    let dir = ferrite_protocol::vanilla_generated_dir(Some(Path::new(env!("CARGO_MANIFEST_DIR"))))
        .join("data/minecraft");
    dir.join("recipe").is_dir().then_some(dir)
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

/// Campfire cooking for `input`: the vanilla `campfire_cooking` recipe.
pub fn campfire_recipe(input: &ItemStack) -> Option<CookingOutput> {
    BOOK.get_or_init(load)
        .as_ref()?
        .cooking_of(CookingKind::CampfireCooking, input)
}

/// Encodes the vanilla smithing trim recipe book entries at runtime (they have no fixed result
/// item, so their preview is generated from the pattern). `index_of` assigns each recipe id a
/// display id.
pub(super) fn smithing_trim_book_entries(
    index_of: &mut dyn FnMut(&str) -> i32,
) -> Vec<(String, super::recipe_wire::Encoded)> {
    let Some(book) = BOOK.get_or_init(load).as_ref() else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for recipe in book.0.recipes() {
        if matches!(recipe.kind, RecipeKind::SmithingTrim(_)) {
            let index = index_of(&recipe.id);
            if let Some(encoded) = super::recipe_wire::encode_entry(
                recipe,
                book.0.tags(),
                index,
                None,
                &ItemStack::empty(),
            ) {
                entries.push((recipe.id.clone(), encoded));
            }
        }
    }
    entries
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

    struct UnlockableCrafter;

    impl Crafter for UnlockableCrafter {
        fn craft(&self, _: &[ItemStack], _: usize) -> ItemStack {
            ItemStack::from_name("minecraft:oak_planks")
                .unwrap()
                .copy_with_count(4)
        }

        fn recipe_id(&self, _: &[ItemStack], _: usize) -> Option<String> {
            Some("minecraft:oak_planks".to_owned())
        }
    }

    #[test]
    fn limited_crafting_hides_only_unlocked_recipes() {
        let base = UnlockableCrafter;
        let grid = [ItemStack::from_name("minecraft:oak_log").unwrap()];
        let locked = LimitedCrafter::new(&base, std::collections::BTreeSet::new());
        assert!(locked.craft(&grid, 1).is_empty());
        assert_eq!(locked.recipe_id(&grid, 1), None);

        let unlocked = LimitedCrafter::new(
            &base,
            std::collections::BTreeSet::from(["minecraft:oak_planks".to_owned()]),
        );
        assert_eq!(
            unlocked.recipe_id(&grid, 1).as_deref(),
            Some("minecraft:oak_planks")
        );
        assert_eq!(
            unlocked.craft(&grid, 1).name(),
            Some("minecraft:oak_planks")
        );
    }

    fn stack(name: &str, count: i32) -> ItemStack {
        ItemStack::from_name(name).unwrap().copy_with_count(count)
    }

    #[test]
    fn dyeing_a_shulker_box_keeps_its_contents() {
        let crafter = crafter();
        let mut shulker = stack("shulker_box", 1);
        shulker.components.insert(
            component_ids::CONTAINER,
            Component::Container(vec![stack("diamond", 3)]),
        );
        let grid = [shulker.clone(), stack("red_dye", 1)];
        let dyed = crafter.craft(&grid, 2);
        assert_eq!(dyed.name(), Some("minecraft:red_shulker_box"));
        assert_eq!(dyed.count, 1);
        assert_eq!(
            dyed.effective_component(component_ids::CONTAINER),
            Some(Component::Container(vec![stack("diamond", 3)])),
            "`TransmuteRecipe.assemble` copies the input's components"
        );
        // The same dye on another colour of box works too, and an empty box stays empty.
        let blue = crafter.craft(&[stack("red_shulker_box", 1), stack("blue_dye", 1)], 2);
        assert_eq!(blue.name(), Some("minecraft:blue_shulker_box"));
    }

    #[test]
    fn cloning_a_map_keeps_the_filled_map_and_counts_the_blank_ones() {
        let crafter = crafter();
        let mut filled = stack("filled_map", 1);
        filled
            .components
            .insert(component_ids::MAP_ID, Component::MapId(7));
        let cloned = crafter.craft(&[filled.clone(), stack("map", 1), stack("map", 1)], 3);
        assert_eq!(cloned.name(), Some("minecraft:filled_map"));
        assert_eq!(cloned.count, 3, "the original and two copies");
        assert_eq!(
            cloned.effective_component(component_ids::MAP_ID),
            Some(Component::MapId(7))
        );
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

    #[test]
    fn smithing_trims_get_a_runtime_recipe_book_entry() {
        let mut next = 10_000;
        let entries = smithing_trim_book_entries(&mut |_| {
            next += 1;
            next
        });
        assert!(
            entries
                .iter()
                .any(|(id, encoded)| id.ends_with("_smithing_trim") && !encoded.display.is_empty()),
            "vanilla trims are encoded at runtime"
        );
    }

    #[test]
    fn vanilla_cooking_campfire_stonecutting_and_brewing_match() {
        let crafter = crafter();
        let ore = stack("raw_iron", 1);
        assert!(crafter.cook(FurnaceKind::Furnace, &ore).is_some());
        assert!(crafter.cook(FurnaceKind::BlastFurnace, &ore).is_some());

        let cod = stack("cod", 1);
        let cooked = campfire_recipe(&cod).expect("cod cooks on a campfire");
        assert_eq!(cooked.result.name(), Some("minecraft:cooked_cod"));
        assert!(campfire_recipe(&stack("apple", 1)).is_none());

        let stone = stack("stone", 1);
        assert!(crafter.stonecutting(&stone).len() > 1);
        assert!(crafter.stonecutting(&stack("dirt", 1)).is_empty());

        let mut water = stack("potion", 1);
        water.components.insert(
            component_ids::POTION_CONTENTS,
            Component::PotionContents(PotionContents {
                potion: Some("minecraft:water".into()),
                ..Default::default()
            }),
        );
        let awkward = crafter
            .brew(&stack("nether_wart", 1), &water)
            .expect("nether wart brews water into awkward potion");
        assert!(matches!(
            awkward.effective_component(component_ids::POTION_CONTENTS),
            Some(Component::PotionContents(contents))
                if contents.potion.as_deref() == Some("minecraft:awkward")
        ));
        assert!(crafter.is_brewing_ingredient(&stack("nether_wart", 1)));
        assert!(!crafter.is_brewing_ingredient(&stack("diamond", 1)));
    }

    #[test]
    fn a_netherite_upgrade_keeps_the_enchantments_of_the_diamond_tool() {
        // Regression: the smithing result used to be a fresh stack, so the upgrade dropped them.
        let crafter = crafter();
        let mut sword = stack("diamond_sword", 1);
        let mut enchants = ferrite_item::Enchantments::new();
        enchants.set("minecraft:sharpness", 5);
        sword.components.insert(
            component_ids::ENCHANTMENTS,
            Component::Enchantments(enchants),
        );
        let template = stack("netherite_upgrade_smithing_template", 1);
        let out = crafter
            .smithing(&template, &sword, &stack("netherite_ingot", 1))
            .expect("the vanilla netherite upgrade");
        assert_eq!(out.name(), Some("minecraft:netherite_sword"));
        assert!(matches!(
            out.effective_component(component_ids::ENCHANTMENTS),
            Some(Component::Enchantments(e)) if e.level("minecraft:sharpness") == Some(5)
        ));
    }
}
