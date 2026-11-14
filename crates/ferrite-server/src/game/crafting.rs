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

/// Recipes plugins changed at runtime (`Server#addRecipe`, `removeRecipe`, `clearRecipes`,
/// `resetRecipes`), applied on top of the vanilla book. Every kind a plugin can add lives here:
/// crafting, the furnace family, stonecutting and smithing.
#[derive(Default)]
struct PluginRecipes {
    /// Added recipes, in the order they were added.
    added: Vec<ferrite_recipe::Recipe>,
    /// Ids of vanilla recipes that no longer craft.
    removed: HashSet<String>,
    /// The item metadata of an added recipe's result (`ItemMeta` on the Bukkit result stack).
    extras: HashMap<String, ResultExtras>,
    /// Bumped by every change, so the recipe book can tell a player's copy is stale.
    revision: u64,
}

/// What a Bukkit result stack carries beyond item and count: wear and `ItemMeta`.
#[derive(Clone, Debug, PartialEq)]
struct ResultExtras {
    damage: i32,
    meta: Option<ferrite_plugin::ItemMetaSpec>,
}

impl PluginRecipes {
    /// `stack` (the recipe's item and count) with the metadata the plugin put on its result.
    fn dress(&self, id: &str, mut stack: ItemStack) -> ItemStack {
        let Some(extras) = self.extras.get(id) else {
            return stack;
        };
        if let Some(meta) = &extras.meta {
            stack = super::region::with_meta_spec(stack, meta);
        }
        if extras.damage > 0 {
            stack
                .components
                .insert(component_ids::DAMAGE, Component::Damage(extras.damage));
        }
        stack
    }

    fn visible(&self, recipe: &ferrite_recipe::Recipe) -> bool {
        !self.removed.contains(&recipe.id)
    }
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
    /// The result stack with a plugin's metadata, when the recipe has any.
    stack: Option<ItemStack>,
}

impl RecipeCrafter {
    /// The recipe a grid crafts: the first vanilla one a plugin has not removed, then the first
    /// plugin one. Special recipes without a fixed result match nothing.
    fn find(&self, names: &[Option<&str>], width: usize, height: usize) -> Option<Crafted> {
        let plugin = plugin_recipes().read().unwrap();
        let recipe = self
            .0
            .match_crafting_where(names, width, height, &|recipe| plugin.visible(recipe))
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
        let stack = plugin.extras.contains_key(&recipe.id).then(|| {
            let base = ItemStack::from_name(&result.item)
                .map(|stack| stack.copy_with_count(i32::try_from(result.count).unwrap_or(1)));
            base.map(|base| plugin.dress(&recipe.id, base))
        });
        Some(Crafted {
            id: recipe.id.clone(),
            item: result.item.clone(),
            count: result.count,
            stack: stack.flatten(),
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
        self.cooking_of(cooking_kind, input)
    }

    /// The cooking recipe of any kind (campfires included) for `input`: the first vanilla one a
    /// plugin has not removed, then the first plugin one.
    fn cooking_of(&self, kind: CookingKind, input: &ItemStack) -> Option<CookingOutput> {
        let name = input.name()?;
        let plugin = plugin_recipes().read().unwrap();
        let recipe = self
            .0
            .match_cooking_where(kind, name, &|recipe| plugin.visible(recipe))
            .or_else(|| {
                ferrite_recipe::match_cooking_in(&plugin.added, self.0.tags(), kind, name)
            })?;
        let RecipeKind::Cooking(cooking) = &recipe.kind else {
            return None;
        };
        let result = ItemStack::from_name(&cooking.result.item)?
            .copy_with_count(i32::try_from(cooking.result.count).ok()?);
        Some(CookingOutput {
            result: plugin.dress(&recipe.id, result),
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
                if let Some(stack) = crafted.stack {
                    return Some(stack);
                }
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
        let plugin = plugin_recipes().read().unwrap();
        let vanilla = self
            .0
            .stonecutting_options_where(name, &|recipe| plugin.visible(recipe));
        let added = ferrite_recipe::stonecutting_in(&plugin.added, self.0.tags(), name);
        vanilla
            .into_iter()
            .chain(added)
            .filter_map(|recipe| {
                let result = recipe.result()?;
                let count = i32::try_from(result.count).ok()?;
                let stack = ItemStack::from_name(&result.item)?.copy_with_count(count);
                Some(plugin.dress(&recipe.id, stack))
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
        let plugin = plugin_recipes().read().unwrap();
        let recipe = plugin
            .added
            .iter()
            .find(|recipe| recipe.id == id)
            .or_else(|| self.0.recipe(id).filter(|recipe| plugin.visible(recipe)))?;
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
        let plugin = plugin_recipes().read().unwrap();
        let recipes = self
            .0
            .recipes()
            .iter()
            .filter(|recipe| plugin.visible(recipe))
            .chain(plugin.added.iter());
        for recipe in recipes {
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
            return Some((recipe.id.clone(), plugin.dress(&recipe.id, out)));
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

/// The recipe types a plugin can add: Bukkit's `ShapedRecipe`, `ShapelessRecipe`, the four cooking
/// recipes, `StonecuttingRecipe`, `SmithingTransformRecipe` and `SmithingTrimRecipe`.
fn addable(kind: &RecipeKind) -> bool {
    !matches!(
        kind,
        RecipeKind::CraftingTransmute(_) | RecipeKind::Special(_)
    )
}

/// `Server#addRecipe`: a recipe in vanilla data pack JSON, with the metadata of its result stack.
/// `Ok(false)` when the id is taken by a recipe that still exists; `Err` for JSON that is not one
/// of the addable types (transmute and the `crafting_special_*` family are code, not data).
pub fn add_plugin_recipe(
    id: &str,
    json: &str,
    meta: Option<ferrite_plugin::ItemMetaSpec>,
    damage: i32,
) -> Result<bool, String> {
    let Some(book) = BOOK.get_or_init(load).as_ref() else {
        return Err("recipes are not loaded".into());
    };
    let recipe = ferrite_recipe::parse_recipe_json(id, json).map_err(|error| error.to_string())?;
    if !addable(&recipe.kind) {
        return Err(format!(
            "{} recipes cannot be added at runtime; only crafting_shaped, crafting_shapeless, \
             smelting, blasting, smoking, campfire_cooking, stonecutting, smithing_transform and \
             smithing_trim",
            recipe.type_name()
        ));
    }
    let mut plugin = plugin_recipes().write().unwrap();
    let taken = plugin.added.iter().any(|known| known.id == id)
        || (book.0.recipe(id).is_some() && !plugin.removed.contains(id));
    if taken {
        return Ok(false);
    }
    if meta.is_some() || damage > 0 {
        plugin
            .extras
            .insert(id.to_owned(), ResultExtras { damage, meta });
    }
    plugin.added.push(recipe);
    plugin.revision += 1;
    Ok(true)
}

/// `Server#removeRecipe`: a plugin recipe disappears, a vanilla one stops matching. `false` when
/// nothing by that id exists.
pub fn remove_recipe(id: &str) -> bool {
    let mut plugin = plugin_recipes().write().unwrap();
    if let Some(index) = plugin.added.iter().position(|known| known.id == id) {
        plugin.added.remove(index);
        plugin.extras.remove(id);
        plugin.revision += 1;
        return true;
    }
    let vanilla = BOOK
        .get_or_init(load)
        .as_ref()
        .is_some_and(|book| book.0.recipe(id).is_some());
    let removed = vanilla && plugin.removed.insert(id.to_owned());
    if removed {
        plugin.revision += 1;
    }
    removed
}

/// `Server#clearRecipes`: no recipe is left, vanilla ones included.
pub fn clear_recipes() {
    if let Some(book) = BOOK.get_or_init(load).as_ref() {
        let mut plugin = plugin_recipes().write().unwrap();
        clear_into(&mut plugin, &book.0);
        plugin.revision += 1;
    }
}

fn clear_into(plugin: &mut PluginRecipes, book: &RecipeBook) {
    plugin.added.clear();
    plugin.extras.clear();
    plugin.removed = book
        .recipes()
        .iter()
        .map(|recipe| recipe.id.clone())
        .collect();
}

/// `Server#resetRecipes`: vanilla recipes only, as loaded.
pub fn reset_recipes() {
    let mut plugin = plugin_recipes().write().unwrap();
    let revision = plugin.revision + 1;
    *plugin = PluginRecipes {
        revision,
        ..PluginRecipes::default()
    };
}

/// Campfire cooking for `input`: the `campfire_cooking` recipe, vanilla or a plugin's. The host has
/// no campfire block entity yet, so only a test and the recipe lookup use this.
pub fn campfire_recipe(input: &ItemStack) -> Option<CookingOutput> {
    BOOK.get_or_init(load)
        .as_ref()?
        .cooking_of(CookingKind::CampfireCooking, input)
}

/// The recipe book as plugins changed it: what the client must be told on top of the vanilla
/// `recipes.json`.
pub struct PluginBookView {
    /// Grows with every `addRecipe`, `removeRecipe`, `clearRecipes` and `resetRecipes`.
    pub revision: u64,
    /// Vanilla recipe ids a plugin removed: the client's book must not list them.
    pub hidden: HashSet<String>,
    /// The plugin recipes the client's book can show (smithing trims and other kinds without a
    /// plain display stay server-side): id and encoded entry.
    pub entries: Vec<(String, super::recipe_wire::Encoded)>,
    /// The `update_recipes` body for this recipe set (furnace and smithing slot items, stonecutter
    /// options), or `None` when it cannot be built.
    pub update: Option<Vec<u8>>,
}

/// The current revision of the plugin recipe overlay (a cheap check before building a view).
pub fn plugin_recipe_revision() -> u64 {
    plugin_recipes().read().unwrap().revision
}

/// Encodes what plugins changed for the client's recipe book. `index_of` hands out the recipe
/// display id of a plugin recipe (stable for an id), `base_update` is the vanilla `update_recipes`
/// body.
pub fn plugin_book_view(
    base_update: &[u8],
    index_of: &mut dyn FnMut(&str) -> i32,
) -> Option<PluginBookView> {
    let book = BOOK.get_or_init(load).as_ref()?;
    let plugin = plugin_recipes().read().unwrap();
    let mut entries = Vec::new();
    for recipe in &plugin.added {
        let Some(result) = recipe.result() else {
            continue;
        };
        let Some(base) = ItemStack::from_name(&result.item) else {
            continue;
        };
        let stack = plugin.dress(
            &recipe.id,
            base.copy_with_count(i32::try_from(result.count).unwrap_or(1)),
        );
        let index = index_of(&recipe.id);
        if let Some(encoded) =
            super::recipe_wire::encode_entry(recipe, book.0.tags(), index, None, &stack)
        {
            entries.push((recipe.id.clone(), encoded));
        }
    }
    let result_of = |recipe: &ferrite_recipe::Recipe| {
        let result = recipe.result()?;
        let stack =
            ItemStack::from_name(&result.item)?.copy_with_count(i32::try_from(result.count).ok()?);
        Some(plugin.dress(&recipe.id, stack))
    };
    let update = super::recipe_wire::update_body(
        base_update,
        &book.0,
        &|recipe| plugin.visible(recipe),
        &plugin.added,
        &result_of,
    );
    Some(PluginBookView {
        revision: plugin.revision,
        hidden: plugin.removed.clone(),
        entries,
        update,
    })
}

/// The plugin recipe tests share one process-wide overlay, so they run one at a time.
#[cfg(test)]
pub(crate) fn overlay_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
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

    use super::overlay_lock;

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
            add_plugin_recipe("plugin:two_dirt", DIAMOND_FROM_DIRT, None, 0),
            Ok(true)
        );
        assert_eq!(crafter.craft(&grid, 2), stack("diamond", 3));
        assert_eq!(
            crafter.recipe_id(&grid, 2).as_deref(),
            Some("plugin:two_dirt")
        );
        // The id is taken now, by a plugin recipe and by a vanilla one.
        assert_eq!(
            add_plugin_recipe("plugin:two_dirt", DIAMOND_FROM_DIRT, None, 0),
            Ok(false)
        );
        assert_eq!(
            add_plugin_recipe("minecraft:lead", DIAMOND_FROM_DIRT, None, 0),
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
    fn transmute_and_special_recipes_are_refused_and_bad_json_says_why() {
        let _guard = overlay_lock();
        let transmute = r#"{"type":"minecraft:crafting_transmute","input":"minecraft:dirt","material":"minecraft:sand","result":"minecraft:stone"}"#;
        let refused = add_plugin_recipe("plugin:transmute", transmute, None, 0).unwrap_err();
        assert!(refused.contains("smithing_trim"), "{refused}");
        let special = r#"{"type":"minecraft:crafting_special_bookcloning"}"#;
        assert!(add_plugin_recipe("plugin:special", special, None, 0).is_err());
        assert!(add_plugin_recipe("plugin:broken", "{", None, 0).is_err());
    }

    fn named(text: &str) -> ferrite_plugin::ItemMetaSpec {
        ferrite_plugin::ItemMetaSpec {
            name: Some(format!(r#"{{"text":"{text}"}}"#)),
            lore: vec![r#"{"text":"made by a plugin"}"#.to_owned()],
            enchants: vec![("minecraft:sharpness".to_owned(), 3)],
            unbreakable: false,
            model: None,
            repair: 0,
            flags: Vec::new(),
            custom_data: None,
            book: None,
        }
    }

    fn custom_name(stack: &ItemStack) -> Option<String> {
        match stack.effective_component(component_ids::CUSTOM_NAME) {
            Some(Component::CustomName(name)) => Some(name.to_json()),
            _ => None,
        }
    }

    #[test]
    fn a_crafting_result_keeps_its_name_lore_enchantments_and_wear() {
        let _guard = overlay_lock();
        let crafter = crafter();
        let grid = [
            stack("dirt", 1),
            stack("dirt", 1),
            ItemStack::empty(),
            ItemStack::empty(),
        ];
        let json = r#"{"type":"minecraft:crafting_shapeless","ingredients":["minecraft:dirt","minecraft:dirt"],"result":{"id":"minecraft:diamond_sword","count":1}}"#;
        assert_eq!(
            add_plugin_recipe("plugin:meta_craft", json, Some(named("Dirt Blade")), 7),
            Ok(true)
        );
        let crafted = crafter.craft(&grid, 2);
        assert_eq!(crafted.name(), Some("minecraft:diamond_sword"));
        assert!(custom_name(&crafted).is_some_and(|n| n.contains("Dirt Blade")));
        assert!(matches!(
            crafted.effective_component(component_ids::LORE),
            Some(Component::Lore(lines)) if lines.len() == 1
        ));
        assert!(matches!(
            crafted.effective_component(component_ids::ENCHANTMENTS),
            Some(Component::Enchantments(e)) if e.level("minecraft:sharpness") == Some(3)
        ));
        assert_eq!(
            crafted.effective_component(component_ids::DAMAGE),
            Some(Component::Damage(7))
        );
        assert!(remove_recipe("plugin:meta_craft"));
        assert_eq!(crafter.craft(&grid, 2), ItemStack::empty());
        assert!(
            !plugin_recipes()
                .read()
                .unwrap()
                .extras
                .contains_key("plugin:meta_craft")
        );
    }

    #[test]
    fn furnace_blast_furnace_and_smoker_recipes_are_added_matched_dressed_and_removed() {
        let _guard = overlay_lock();
        let crafter = crafter();
        let gravel = stack("gravel", 1);
        for (kind, type_name) in [
            (FurnaceKind::Furnace, "smelting"),
            (FurnaceKind::BlastFurnace, "blasting"),
            (FurnaceKind::Smoker, "smoking"),
        ] {
            assert!(
                crafter.cook(kind, &gravel).is_none(),
                "{type_name}: no recipe yet"
            );
            let id = format!("plugin:cook_{type_name}");
            let json = format!(
                r#"{{"type":"minecraft:{type_name}","ingredient":"minecraft:gravel","result":{{"id":"minecraft:glass","count":2}},"experience":1.5,"cookingtime":37}}"#
            );
            assert_eq!(
                add_plugin_recipe(&id, &json, Some(named("Fused")), 0),
                Ok(true)
            );
            let out = crafter
                .cook(kind, &gravel)
                .expect("the plugin recipe cooks");
            assert_eq!(out.result.name(), Some("minecraft:glass"));
            assert_eq!(out.result.count, 2);
            assert_eq!((out.cook_time, out.experience), (37, 1.5));
            assert!(
                custom_name(&out.result).is_some_and(|n| n.contains("Fused")),
                "{type_name}"
            );
            // The other two families do not see it.
            for other in [
                FurnaceKind::Furnace,
                FurnaceKind::BlastFurnace,
                FurnaceKind::Smoker,
            ] {
                if other != kind {
                    assert!(
                        crafter.cook(other, &gravel).is_none()
                            || !custom_name(&crafter.cook(other, &gravel).unwrap().result)
                                .is_some_and(|n| n.contains("Fused")),
                    );
                }
            }
            assert!(remove_recipe(&id));
            assert!(
                crafter.cook(kind, &gravel).is_none(),
                "{type_name}: removed"
            );
        }
    }

    #[test]
    fn removing_a_vanilla_smelting_recipe_stops_the_furnace_and_a_reset_restores_it() {
        let _guard = overlay_lock();
        let crafter = crafter();
        let ore = stack("raw_iron", 1);
        assert!(crafter.cook(FurnaceKind::Furnace, &ore).is_some());
        assert!(remove_recipe("minecraft:iron_ingot_from_smelting_raw_iron"));
        assert!(crafter.cook(FurnaceKind::Furnace, &ore).is_none());
        assert!(
            crafter.cook(FurnaceKind::BlastFurnace, &ore).is_some(),
            "the blasting recipe is a different id"
        );
        reset_recipes();
        assert!(crafter.cook(FurnaceKind::Furnace, &ore).is_some());
    }

    #[test]
    fn a_campfire_recipe_is_added_matched_and_removed() {
        let _guard = overlay_lock();
        let apple = stack("apple", 1);
        assert!(campfire_recipe(&apple).is_none());
        let json = r#"{"type":"minecraft:campfire_cooking","ingredient":"minecraft:apple","result":{"id":"minecraft:cooked_cod"},"experience":0.35,"cookingtime":600}"#;
        assert_eq!(
            add_plugin_recipe("plugin:roast_apple", json, Some(named("Roast")), 0),
            Ok(true)
        );
        let out = campfire_recipe(&apple).expect("the campfire recipe matches");
        assert_eq!((out.cook_time, out.experience), (600, 0.35));
        assert!(custom_name(&out.result).is_some_and(|n| n.contains("Roast")));
        assert!(
            crafter().cook(FurnaceKind::Furnace, &apple).is_none(),
            "a campfire recipe is not a furnace recipe"
        );
        assert!(remove_recipe("plugin:roast_apple"));
        assert!(campfire_recipe(&apple).is_none());
        // A vanilla campfire recipe exists and can be removed too.
        let cod = stack("cod", 1);
        assert!(campfire_recipe(&cod).is_some());
        assert!(remove_recipe("minecraft:cooked_cod_from_campfire_cooking"));
        assert!(campfire_recipe(&cod).is_none());
        reset_recipes();
        assert!(campfire_recipe(&cod).is_some());
    }

    #[test]
    fn stonecutter_recipes_are_added_listed_dressed_and_removed() {
        let _guard = overlay_lock();
        let crafter = crafter();
        let dirt = stack("dirt", 1);
        assert!(crafter.stonecutting(&dirt).is_empty());
        let json = r#"{"type":"minecraft:stonecutting","ingredient":"minecraft:dirt","result":{"id":"minecraft:stick","count":6}}"#;
        assert_eq!(
            add_plugin_recipe("plugin:cut_dirt", json, Some(named("Twig")), 0),
            Ok(true)
        );
        let options = crafter.stonecutting(&dirt);
        assert_eq!(options.len(), 1);
        assert_eq!(
            (options[0].name(), options[0].count),
            (Some("minecraft:stick"), 6)
        );
        assert!(custom_name(&options[0]).is_some_and(|n| n.contains("Twig")));
        // The menu offers it and crafts it.
        let mut inventory = ferrite_inventory::PlayerInventory::new();
        let mut menu = ferrite_inventory::Menu::stonecutter(1);
        menu.set_slot(&mut inventory, crafter, 0, stack("dirt", 4));
        assert_eq!(menu.result().name(), Some("minecraft:stick"));
        assert!(remove_recipe("plugin:cut_dirt"));
        assert!(crafter.stonecutting(&dirt).is_empty());
        // A vanilla stonecutter recipe can be removed and comes back on reset.
        let stone = stack("stone", 1);
        let before = crafter.stonecutting(&stone).len();
        assert!(before > 1);
        assert!(remove_recipe(
            "minecraft:stone_slab_from_stone_stonecutting"
        ));
        assert_eq!(crafter.stonecutting(&stone).len(), before - 1);
        reset_recipes();
        assert_eq!(crafter.stonecutting(&stone).len(), before);
    }

    #[test]
    fn smithing_recipes_are_added_matched_and_keep_the_base_components() {
        let _guard = overlay_lock();
        let crafter = crafter();
        let template = stack("stick", 1);
        let mut sword = stack("diamond_sword", 1);
        let mut enchants = ferrite_item::Enchantments::new();
        enchants.set("minecraft:sharpness", 4);
        sword.components.insert(
            component_ids::ENCHANTMENTS,
            Component::Enchantments(enchants),
        );
        // No template and no addition: both are optional in vanilla.
        let json = r#"{"type":"minecraft:smithing_transform","base":"minecraft:diamond_sword","result":{"id":"minecraft:golden_sword"}}"#;
        assert!(
            crafter
                .smithing(&ItemStack::empty(), &sword, &ItemStack::empty())
                .is_none()
        );
        assert_eq!(
            add_plugin_recipe("plugin:gild", json, Some(named("Gilded")), 0),
            Ok(true)
        );
        let out = crafter
            .smithing(&ItemStack::empty(), &sword, &ItemStack::empty())
            .expect("the plugin smithing recipe matches an empty template and addition");
        assert_eq!(out.name(), Some("minecraft:golden_sword"));
        assert!(custom_name(&out).is_some_and(|n| n.contains("Gilded")));
        assert_eq!(
            crafter
                .smithing_recipe_id(&ItemStack::empty(), &sword, &ItemStack::empty())
                .as_deref(),
            Some("plugin:gild")
        );
        // A recipe without a template only matches an empty template slot.
        assert!(
            crafter
                .smithing(&template, &sword, &ItemStack::empty())
                .is_none()
        );
        assert!(remove_recipe("plugin:gild"));
        assert!(
            crafter
                .smithing(&ItemStack::empty(), &sword, &ItemStack::empty())
                .is_none()
        );

        // A recipe with a template and an addition, no metadata on the result: the base's enchantments survive.
        let json = r#"{"type":"minecraft:smithing_transform","template":"minecraft:stick","base":"minecraft:diamond_sword","addition":"minecraft:gold_ingot","result":{"id":"minecraft:iron_sword"}}"#;
        assert_eq!(add_plugin_recipe("plugin:swap", json, None, 0), Ok(true));
        let out = crafter
            .smithing(&template, &sword, &stack("gold_ingot", 1))
            .expect("template, base and addition match");
        assert_eq!(out.name(), Some("minecraft:iron_sword"));
        assert!(matches!(
            out.effective_component(component_ids::ENCHANTMENTS),
            Some(Component::Enchantments(e)) if e.level("minecraft:sharpness") == Some(4)
        ));
        assert!(
            crafter
                .smithing(&template, &sword, &stack("iron_ingot", 1))
                .is_none()
        );
        assert!(remove_recipe("plugin:swap"));
    }

    #[test]
    fn a_netherite_upgrade_keeps_the_enchantments_of_the_diamond_tool() {
        // Regression: the smithing result used to be a fresh stack, so the upgrade dropped them.
        let _guard = overlay_lock();
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

    #[test]
    fn a_smithing_trim_recipe_from_a_plugin_trims_with_its_own_pattern() {
        let _guard = overlay_lock();
        let crafter = crafter();
        let json = r##"{"type":"minecraft:smithing_trim","template":"minecraft:stick","base":"#minecraft:trimmable_armor","addition":"minecraft:amethyst_shard","pattern":"minecraft:wild"}"##;
        assert_eq!(
            add_plugin_recipe("plugin:trim_stick", json, None, 0),
            Ok(true)
        );
        let armor = stack("iron_helmet", 1);
        let trimmed = crafter
            .smithing(&stack("stick", 1), &armor, &stack("amethyst_shard", 1))
            .expect("the plugin trim matches");
        assert!(matches!(
            trimmed.effective_component(component_ids::TRIM),
            Some(Component::Trim(trim)) if trim.pattern == "minecraft:wild" && trim.material == "minecraft:amethyst"
        ));
        assert!(remove_recipe("plugin:trim_stick"));
        assert!(
            crafter
                .smithing(&stack("stick", 1), &armor, &stack("amethyst_shard", 1))
                .is_none()
        );
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
