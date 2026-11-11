//! Crafting recipes for container menus, matched with `ferrite-recipe`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use ferrite_inventory::{CookingOutput, Crafter, FurnaceKind, NoCrafting};
use ferrite_item::{
    Component, FireworkExplosion, Fireworks, ItemStack, PotDecorations, PotionContents, Trim,
    component_ids,
};
use ferrite_recipe::{CookingKind, RecipeBook, RecipeKind};
use serde_json::Value;

/// Vanilla `CraftingMenu.slotChangedCraftingGrid`: the grid's result from the recipe book.
///
/// Special recipes have no fixed result in `ferrite-recipe`. Repairing, banner duplication, book
/// cloning, fireworks, decorated pots, dyeing, shield decoration and tipped arrows are
/// implemented here; map extending still crafts nothing. No regular 26.3 crafting result carries
/// a component patch, so a result is its item and count.
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
        let regular = self
            .0
            .match_crafting(&names, width, grid.len() / width)
            .and_then(|recipe| recipe.result())
            .and_then(|result| {
                let count = i32::try_from(result.count).ok()?;
                Some(ItemStack::from_name(&result.item)?.copy_with_count(count))
            });
        regular
            .or_else(|| self.special(grid, width))
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
        self.0
            .match_crafting(&names, width, grid.len() / width)
            .map(|recipe| recipe.id.clone())
            .or_else(|| self.special_recipe_id(grid, width))
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

    fn enchantment_max_level(&self, enchantment: &str) -> Option<i32> {
        enchantment_info(enchantment).map(|data| data.max_level)
    }

    fn enchantment_anvil_cost(&self, enchantment: &str) -> i32 {
        enchantment_info(enchantment).map_or(1, |data| data.anvil_cost.max(1))
    }

    fn enchantment_supports(&self, enchantment: &str, item: &ItemStack) -> bool {
        if item.name() == Some("minecraft:enchanted_book") {
            return true;
        }
        let Some(data) = enchantment_info(enchantment) else {
            return true;
        };
        if data.supported_items.is_empty() {
            return true;
        }
        let Some(name) = item.name() else {
            return false;
        };
        data.supported_items.iter().any(|entry| {
            if let Some(tag) = entry.strip_prefix('#') {
                self.0.tags().contains(tag, name)
            } else {
                normalize_id(entry) == name
            }
        })
    }

    fn enchantments_compatible(&self, first: &str, second: &str) -> bool {
        let first = normalize_id(first);
        let second = normalize_id(second);
        if first == second {
            return true;
        }
        let tags = enchantment_tags();
        let conflicts = |a: &str, b: &str| {
            let Some(data) = enchantment_data().get(a) else {
                return false;
            };
            data.exclusive_set.iter().any(|entry| {
                if let Some(tag) = entry.strip_prefix('#') {
                    tags.get(&normalize_tag(tag))
                        .is_some_and(|set| set.contains(b))
                } else {
                    normalize_id(entry) == b
                }
            })
        };
        !conflicts(&first, &second) && !conflicts(&second, &first)
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

impl RecipeCrafter {
    /// A special recipe's id and raw JSON, or `None` when the data pack has no such recipe.
    fn special_recipe<'a>(&'a self, type_name: &str) -> Option<(&'a str, &'a Value)> {
        self.0
            .recipes()
            .iter()
            .find_map(|recipe| match &recipe.kind {
                RecipeKind::Special(special) if special.type_name == type_name => {
                    Some((recipe.id.as_str(), &special.data))
                }
                _ => None,
            })
    }

    /// The special crafting recipes whose result depends on the inputs. Vanilla ships no fixed
    /// result for these, so `ferrite-recipe` keeps only their raw JSON.
    fn special(&self, grid: &[ItemStack], width: usize) -> Option<ItemStack> {
        self.special_match(grid, width).map(|(_, stack)| stack)
    }

    /// The id of the special recipe `special` applies, for the `recipe_crafted` trigger.
    fn special_recipe_id(&self, grid: &[ItemStack], width: usize) -> Option<String> {
        self.special_match(grid, width).map(|(id, _)| id)
    }

    /// The matching special recipe and its result.
    fn special_match(&self, grid: &[ItemStack], width: usize) -> Option<(String, ItemStack)> {
        if width == 0 || !grid.len().is_multiple_of(width) {
            return None;
        }
        let items: Vec<&ItemStack> = grid.iter().filter(|stack| !stack.is_empty()).collect();
        if let Some((id, _)) = self.special_recipe("minecraft:crafting_special_repairitem")
            && let Some(result) = repair_item(&items)
        {
            return Some((id.to_owned(), result));
        }
        if let Some((id, _)) = self.special_recipe("minecraft:crafting_special_bannerduplicate")
            && let Some(result) = duplicate_banner(&items)
        {
            return Some((id.to_owned(), result));
        }
        if let Some((id, data)) = self.special_recipe("minecraft:crafting_special_shielddecoration")
            && let Some(result) = self.shield_decoration(data, &items)
        {
            return Some((id.to_owned(), result));
        }
        if let Some((id, _)) = self.special_recipe("minecraft:crafting_special_bookcloning")
            && let Some(result) = clone_book(&items)
        {
            return Some((id.to_owned(), result));
        }
        if let Some((id, data)) = self.special_recipe("minecraft:crafting_special_firework_rocket")
            && let Some(result) = self.firework_rocket(data, &items)
        {
            return Some((id.to_owned(), result));
        }
        if let Some((id, data)) = self.special_recipe("minecraft:crafting_special_firework_star")
            && let Some(result) = self.firework_star(data, &items)
        {
            return Some((id.to_owned(), result));
        }
        if let Some((id, data)) =
            self.special_recipe("minecraft:crafting_special_firework_star_fade")
            && let Some(result) = self.firework_star_fade(data, &items)
        {
            return Some((id.to_owned(), result));
        }
        if let Some((id, data)) = self.special_recipe("minecraft:crafting_decorated_pot")
            && let Some(result) = self.decorated_pot(data, &items, width)
        {
            return Some((id.to_owned(), result));
        }
        for recipe in self.0.recipes() {
            let RecipeKind::Special(special) = &recipe.kind else {
                continue;
            };
            if special.type_name != "minecraft:crafting_dye" {
                continue;
            }
            if let Some(result) = self.dye(&special.data, &items) {
                return Some((recipe.id.clone(), result));
            }
        }
        if let Some((id, data)) = self.special_recipe("minecraft:crafting_imbue")
            && let Some(result) = self.imbue(data, &items)
        {
            return Some((id.to_owned(), result));
        }
        None
    }

    /// Vanilla `ShieldDecorationRecipe`: the banner's pattern layers are copied onto a shield.
    fn shield_decoration(&self, data: &Value, items: &[&ItemStack]) -> Option<ItemStack> {
        let banner = field_str(data, "banner")?;
        let target = field_str(data, "target")?;
        let [first, second] = items else {
            return None;
        };
        let (banner, shield) =
            if first.name()? == target && self.matches_tag(banner, second.name()?) {
                (second, first)
            } else if second.name()? == target && self.matches_tag(banner, first.name()?) {
                (first, second)
            } else {
                return None;
            };
        let patterns = match banner.components.get(component_ids::BANNER_PATTERNS)? {
            Component::BannerPatterns(patterns) => patterns.clone(),
            Component::Opaque(tag) => {
                match Component::from_nbt(component_ids::BANNER_PATTERNS, tag) {
                    Component::BannerPatterns(patterns) => patterns,
                    _ => return None,
                }
            }
            _ => return None,
        };
        if patterns.0.is_empty() {
            return None;
        }
        let mut result = shield.copy_with_count(1);
        result.components.insert(
            component_ids::BANNER_PATTERNS,
            Component::BannerPatterns(patterns),
        );
        Some(result)
    }

    /// Vanilla `FireworkRocketRecipe`: paper, one to three gunpowder and up to seven stars.
    fn firework_rocket(&self, data: &Value, items: &[&ItemStack]) -> Option<ItemStack> {
        let shell = field_str(data, "shell")?;
        let fuel = field_str(data, "fuel")?;
        let star = field_str(data, "star")?;
        let mut shells = 0;
        let mut fuels = 0;
        let mut explosions = Vec::new();
        for item in items {
            let name = item.name()?;
            if name == shell {
                shells += 1;
            } else if name == fuel {
                fuels += 1;
            } else if name == star {
                let Component::FireworkExplosion(explosion) =
                    item.effective_component(component_ids::FIREWORK_EXPLOSION)?
                else {
                    return None;
                };
                explosions.push(explosion);
            } else {
                return None;
            }
        }
        if shells != 1 || !(1..=3).contains(&fuels) || explosions.len() > 7 {
            return None;
        }
        let mut result = craft_result(data)?;
        result.components.insert(
            component_ids::FIREWORKS,
            Component::Fireworks(Fireworks {
                flight_duration: fuels,
                explosions,
            }),
        );
        Some(result)
    }

    /// Vanilla `FireworkStarRecipe`: one gunpowder, one to eight dyes and optional shape, trail
    /// and twinkle ingredients.
    fn firework_star(&self, data: &Value, items: &[&ItemStack]) -> Option<ItemStack> {
        let fuel = field_str(data, "fuel")?;
        let trail = field_str(data, "trail")?;
        let twinkle = field_str(data, "twinkle")?;
        let shapes = data.get("shapes")?.as_object()?;
        let mut fuel_count = 0;
        let mut colors = Vec::new();
        let mut shape = None;
        let mut has_trail = false;
        let mut has_twinkle = false;
        for item in items {
            let name = item.name()?;
            if name == fuel {
                fuel_count += 1;
            } else if let Some(color) = dye_color(name) {
                colors.push(color);
            } else if name == trail {
                has_trail = true;
            } else if name == twinkle {
                has_twinkle = true;
            } else {
                if shape.is_some() {
                    return None;
                }
                let (shape_name, _) = shapes
                    .iter()
                    .find(|(_, value)| self.item_matches(value, name))?;
                shape = Some(shape_name.clone());
            }
        }
        if fuel_count != 1 || colors.is_empty() {
            return None;
        }
        let mut result = craft_result(data)?;
        result.components.insert(
            component_ids::FIREWORK_EXPLOSION,
            Component::FireworkExplosion(FireworkExplosion {
                shape: shape.unwrap_or_else(|| "small_ball".to_owned()),
                colors,
                fade_colors: Vec::new(),
                has_trail,
                has_twinkle,
            }),
        );
        Some(result)
    }

    /// Vanilla `FireworkStarFadeRecipe`: a star plus one to eight dyes adds fade colors.
    fn firework_star_fade(&self, data: &Value, items: &[&ItemStack]) -> Option<ItemStack> {
        let target = field_str(data, "target")?;
        let mut star = None;
        let mut colors = Vec::new();
        for item in items {
            let name = item.name()?;
            if name == target && star.is_none() {
                star = Some(*item);
            } else {
                colors.push(dye_color(name)?);
            }
        }
        if colors.is_empty() {
            return None;
        }
        let star = star?;
        let Component::FireworkExplosion(mut explosion) =
            star.effective_component(component_ids::FIREWORK_EXPLOSION)?
        else {
            return None;
        };
        explosion.fade_colors.extend(colors);
        let mut result = star.copy_with_count(1);
        result.components.insert(
            component_ids::FIREWORK_EXPLOSION,
            Component::FireworkExplosion(explosion),
        );
        Some(result)
    }

    /// Vanilla `DecoratedPotRecipe`, which only matches a filled 2x2 grid.
    fn decorated_pot(&self, data: &Value, items: &[&ItemStack], width: usize) -> Option<ItemStack> {
        if width != 2 || items.len() != 4 {
            return None;
        }
        let mut names = [String::new(), String::new(), String::new(), String::new()];
        for (index, side) in PotDecorations::SIDES.iter().enumerate() {
            let name = items[index].name()?;
            if !self.item_matches(data.get(*side)?, name) {
                return None;
            }
            names[index] = name.to_owned();
        }
        let mut result = craft_result(data)?;
        result.components.insert(
            component_ids::POT_DECORATIONS,
            Component::PotDecorations(PotDecorations(names)),
        );
        Some(result)
    }

    /// Vanilla `DyeRecipe` for one leather or wolf-armor target.
    fn dye(&self, data: &Value, items: &[&ItemStack]) -> Option<ItemStack> {
        let target = field_str(data, "target")?;
        let dye = field_str(data, "dye")?;
        let [first, second] = items else {
            return None;
        };
        let (target_item, dye_item) =
            if first.name()? == target && self.matches_tag(dye, second.name()?) {
                (first, second)
            } else if second.name()? == target && self.matches_tag(dye, first.name()?) {
                (second, first)
            } else {
                return None;
            };
        let color = dye_color(dye_item.name()?)?;
        let mixed = match target_item.effective_component(component_ids::DYED_COLOR) {
            Some(Component::DyedColor(old)) => blend_color(old, color),
            _ => color,
        };
        let mut result = craft_result(data).unwrap_or_else(|| target_item.copy_with_count(1));
        result
            .components
            .insert(component_ids::DYED_COLOR, Component::DyedColor(mixed));
        Some(result)
    }

    /// Vanilla `TippedArrowRecipe`: eight tipped arrows from an arrow and a lingering potion.
    fn imbue(&self, data: &Value, items: &[&ItemStack]) -> Option<ItemStack> {
        let material = field_str(data, "material")?;
        let source = field_str(data, "source")?;
        let [first, second] = items else {
            return None;
        };
        let potion = if first.name()? == material && second.name()? == source {
            second
        } else if second.name()? == material && first.name()? == source {
            first
        } else {
            return None;
        };
        let contents = match potion.effective_component(component_ids::POTION_CONTENTS)? {
            Component::PotionContents(contents) => Component::PotionContents(contents),
            Component::Raw(bytes) => Component::Raw(bytes),
            _ => return None,
        };
        let mut result = craft_result(data)?;
        result
            .components
            .insert(component_ids::POTION_CONTENTS, contents);
        Some(result)
    }

    /// Whether an ingredient value (an item name or a `#tag`) matches `item`.
    fn item_matches(&self, expected: &Value, item: &str) -> bool {
        match expected {
            Value::String(expected) => self.matches_tag(expected, item),
            Value::Array(values) => values.iter().any(|value| self.item_matches(value, item)),
            _ => false,
        }
    }

    fn matches_tag(&self, expected: &str, item: &str) -> bool {
        if let Some(tag) = expected.strip_prefix('#') {
            self.0.tags().contains(tag, item)
        } else {
            expected.strip_prefix("minecraft:").unwrap_or(expected)
                == item.strip_prefix("minecraft:").unwrap_or(item)
        }
    }
}

/// Vanilla `RepairItemRecipe`: two damaged copies of one item combine into one, adding the 5%
/// durability bonus vanilla gives a combined repair.
fn repair_item(items: &[&ItemStack]) -> Option<ItemStack> {
    let [left, right] = items else {
        return None;
    };
    if !left.is_same_item(right) || !left.is_damageable() || !right.is_damageable() {
        return None;
    }
    let max = left.max_damage()?;
    let left_durability = (max - left.damage()).max(0);
    let right_durability = (max - right.damage()).max(0);
    let total = (left_durability + right_durability + max / 20).min(max);
    let mut result = left.copy_with_count(1);
    result
        .components
        .insert(component_ids::DAMAGE, Component::Damage(max - total));
    Some(result)
}

/// Vanilla `BannerDuplicateRecipe`: a patterned banner plus a blank banner of the same colour
/// makes two copies of the patterned banner.
fn duplicate_banner(items: &[&ItemStack]) -> Option<ItemStack> {
    let [first, second] = items else {
        return None;
    };
    if !first.is_same_item(second) {
        return None;
    }
    let name = first.name()?;
    if !name.ends_with("_banner") {
        return None;
    }
    let patterned = |stack: &ItemStack| {
        stack
            .components
            .get(component_ids::BANNER_PATTERNS)
            .is_some()
    };
    let source = match (patterned(first), patterned(second)) {
        (true, false) => first,
        (false, true) => second,
        _ => return None,
    };
    Some(source.copy_with_count(2))
}

/// Vanilla `BookCloningRecipe`: a written book copied into a blank book and quill makes two
/// written books with the copy generation advanced by one. A copy of a copy cannot be copied.
fn clone_book(items: &[&ItemStack]) -> Option<ItemStack> {
    let [first, second] = items else {
        return None;
    };
    let written = |stack: &ItemStack| stack.name() == Some("minecraft:written_book");
    let writable = |stack: &ItemStack| {
        stack.name() == Some("minecraft:writable_book")
            && stack
                .components
                .get(component_ids::WRITABLE_BOOK_CONTENT)
                .is_none()
    };
    let source = if written(first) && writable(second) {
        first
    } else if written(second) && writable(first) {
        second
    } else {
        return None;
    };
    let content = match source.components.get(component_ids::WRITTEN_BOOK_CONTENT) {
        Some(Component::WrittenBookContent(content)) => content.clone(),
        Some(Component::Opaque(tag)) => {
            match Component::from_nbt(component_ids::WRITTEN_BOOK_CONTENT, tag) {
                Component::WrittenBookContent(content) => content,
                _ => return Some(source.copy_with_count(2)),
            }
        }
        _ => return Some(source.copy_with_count(2)),
    };
    if content.generation >= 2 {
        return None;
    }
    let mut result = source.copy_with_count(2);
    let mut content = content;
    content.generation += 1;
    result.components.insert(
        component_ids::WRITTEN_BOOK_CONTENT,
        Component::WrittenBookContent(content),
    );
    Some(result)
}

/// A special recipe's string field.
fn field_str<'a>(data: &'a Value, key: &str) -> Option<&'a str> {
    data.get(key)?.as_str()
}

/// A special recipe's fixed result.
fn craft_result(data: &Value) -> Option<ItemStack> {
    let result = data.get("result")?;
    let name = result.get("id")?.as_str()?;
    let count = result.get("count").and_then(Value::as_i64).unwrap_or(1);
    Some(ItemStack::from_name(name)?.copy_with_count(i32::try_from(count).ok()?))
}

/// Packed RGB of a vanilla dye item.
fn dye_color(name: &str) -> Option<i32> {
    const DYES: [(&str, i32); 16] = [
        ("white", 0xF9FFFE),
        ("orange", 0xF9801D),
        ("magenta", 0xC74EBD),
        ("light_blue", 0x3AB3DA),
        ("yellow", 0xFED83D),
        ("lime", 0x80C71F),
        ("pink", 0xF38BAA),
        ("gray", 0x474F52),
        ("light_gray", 0x9D9D97),
        ("cyan", 0x169C9C),
        ("purple", 0x8932B8),
        ("blue", 0x3C44AA),
        ("brown", 0x835432),
        ("green", 0x5E7C16),
        ("red", 0xB02E26),
        ("black", 0x1D1D21),
    ];
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    let name = name.strip_suffix("_dye").unwrap_or(name);
    DYES.iter()
        .find(|(dye, _)| *dye == name)
        .map(|(_, color)| *color)
}

/// Vanilla's one-dye blend: each channel is the average of the old and new colors.
fn blend_color(first: i32, second: i32) -> i32 {
    let channel = |shift: u32| {
        let a = (first >> shift) & 0xFF;
        let b = (second >> shift) & 0xFF;
        (((a + b) / 2) & 0xFF) << shift
    };
    channel(16) | channel(8) | channel(0)
}

/// One 26.3 enchantment's anvil-relevant data.
#[derive(Clone, Debug)]
struct EnchantmentData {
    max_level: i32,
    anvil_cost: i32,
    /// Item ids or `#` tags the enchantment can be applied to.
    supported_items: Vec<String>,
    /// Enchantment ids or `#` tags it conflicts with.
    exclusive_set: Vec<String>,
}

/// Every enchantment's anvil data, read once from the generated data pack.
fn enchantment_data() -> &'static HashMap<String, EnchantmentData> {
    static DATA: OnceLock<HashMap<String, EnchantmentData>> = OnceLock::new();
    DATA.get_or_init(|| {
        let mut data = HashMap::new();
        let Some(dir) = data_dir().map(|dir| dir.join("enchantment")) else {
            return data;
        };
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return data;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            let Some(max_level) = value.get("max_level").and_then(Value::as_i64) else {
                continue;
            };
            let Ok(max_level) = i32::try_from(max_level) else {
                continue;
            };
            let anvil_cost = value
                .get("anvil_cost")
                .and_then(Value::as_i64)
                .and_then(|cost| i32::try_from(cost).ok())
                .unwrap_or(1);
            data.insert(
                format!("minecraft:{name}"),
                EnchantmentData {
                    max_level,
                    anvil_cost,
                    supported_items: name_list(value.get("supported_items")),
                    exclusive_set: name_list(value.get("exclusive_set")),
                },
            );
        }
        data
    })
}

fn enchantment_info(enchantment: &str) -> Option<&'static EnchantmentData> {
    enchantment_data().get(&normalize_id(enchantment))
}

/// The resolved `tags/enchantment/**` sets, keyed by `#minecraft:...`.
fn enchantment_tags() -> &'static HashMap<String, HashSet<String>> {
    static TAGS: OnceLock<HashMap<String, HashSet<String>>> = OnceLock::new();
    TAGS.get_or_init(|| {
        let Some(dir) = data_dir().map(|dir| dir.join("tags/enchantment")) else {
            return HashMap::new();
        };
        let mut raw = HashMap::new();
        collect_enchantment_tags(&dir, &dir, &mut raw);
        let mut resolved = HashMap::new();
        for name in raw.keys() {
            resolve_enchantment_tag(name, &raw, &mut resolved, &mut HashSet::new());
        }
        resolved
    })
}

fn collect_enchantment_tags(root: &Path, dir: &Path, tags: &mut HashMap<String, Vec<String>>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_enchantment_tags(root, &path, tags);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let Some(relative) = relative.to_str() else {
            continue;
        };
        let name = format!(
            "#minecraft:{}",
            relative.replace('\\', "/").trim_end_matches(".json")
        );
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let mut values = Vec::new();
        if let Some(Value::Array(entries)) = value.get("values") {
            for entry in entries {
                match entry {
                    Value::String(entry) => values.push(entry.clone()),
                    Value::Object(object) => {
                        if let Some(Value::String(id)) = object.get("id") {
                            values.push(id.clone());
                        }
                    }
                    _ => {}
                }
            }
        }
        tags.insert(name, values);
    }
}

fn resolve_enchantment_tag(
    name: &str,
    raw: &HashMap<String, Vec<String>>,
    resolved: &mut HashMap<String, HashSet<String>>,
    visiting: &mut HashSet<String>,
) -> HashSet<String> {
    if let Some(set) = resolved.get(name) {
        return set.clone();
    }
    if !visiting.insert(name.to_owned()) {
        return HashSet::new();
    }
    let mut set = HashSet::new();
    if let Some(entries) = raw.get(name) {
        for entry in entries {
            if entry.starts_with('#') {
                set.extend(resolve_enchantment_tag(
                    &normalize_tag(entry),
                    raw,
                    resolved,
                    visiting,
                ));
            } else {
                set.insert(normalize_id(entry));
            }
        }
    }
    visiting.remove(name);
    resolved.insert(name.to_owned(), set.clone());
    set
}

fn name_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(name)) => vec![name.clone()],
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

fn normalize_id(name: &str) -> String {
    if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    }
}

fn normalize_tag(name: &str) -> String {
    let name = name.strip_prefix('#').unwrap_or(name);
    format!("#{}", normalize_id(name))
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
    let recipe = book.0.match_crafting(names, width, height)?;
    let result = recipe.result()?;
    Some((recipe.id.clone(), result.item.clone(), result.count))
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
    fn special_recipes_repair_duplicate_and_clone() {
        let crafter = crafter();
        let empty = ItemStack::empty;
        // Two damaged iron swords become one undamaged sword.
        let mut left = stack("iron_sword", 1);
        left.components
            .insert(component_ids::DAMAGE, Component::Damage(100));
        let mut right = stack("iron_sword", 1);
        right
            .components
            .insert(component_ids::DAMAGE, Component::Damage(100));
        let repaired = crafter.craft(&[left, right, empty(), empty()], 2);
        assert_eq!(repaired.name(), Some("minecraft:iron_sword"));
        assert_eq!(repaired.count, 1);
        assert_eq!(repaired.damage(), 0);

        // A patterned banner plus a blank banner of the same colour make two banners.
        let mut patterned = stack("white_banner", 1);
        patterned
            .components
            .insert(component_ids::BANNER_PATTERNS, Component::Raw(Vec::new()));
        let duplicated = crafter.craft(&[patterned, stack("white_banner", 1), empty(), empty()], 2);
        assert_eq!(duplicated.name(), Some("minecraft:white_banner"));
        assert_eq!(duplicated.count, 2);
        // Two blank banners are not a recipe.
        assert_eq!(
            crafter.craft(
                &[
                    stack("white_banner", 1),
                    stack("white_banner", 1),
                    empty(),
                    empty()
                ],
                2
            ),
            empty()
        );

        // A written book copied into a blank book and quill makes two written books.
        let mut written = stack("written_book", 1);
        written.components.insert(
            component_ids::WRITTEN_BOOK_CONTENT,
            Component::Raw(Vec::new()),
        );
        let cloned = crafter.craft(&[written, stack("writable_book", 1), empty(), empty()], 2);
        assert_eq!(cloned.name(), Some("minecraft:written_book"));
        assert_eq!(cloned.count, 2);
    }

    #[test]
    fn special_recipes_decorate_shields_and_advance_book_generation() {
        use ferrite_item::{BannerPattern, BannerPatterns, WrittenBookContent};

        let crafter = crafter();
        let empty = ItemStack::empty;
        let patterns = BannerPatterns(vec![BannerPattern {
            pattern: "minecraft:creeper".to_owned(),
            color: "red".to_owned(),
        }]);
        let mut banner = stack("white_banner", 1);
        banner.components.insert(
            component_ids::BANNER_PATTERNS,
            Component::BannerPatterns(patterns.clone()),
        );
        let shield = crafter.craft(&[banner, stack("shield", 1), empty(), empty()], 2);
        assert_eq!(shield.name(), Some("minecraft:shield"));
        assert_eq!(
            shield.effective_component(component_ids::BANNER_PATTERNS),
            Some(Component::BannerPatterns(patterns))
        );

        let content = WrittenBookContent {
            title: "T".to_owned(),
            filtered_title: None,
            author: "A".to_owned(),
            generation: 0,
            pages: vec![(ferrite_nbt::Tag::String("page".to_owned()), None)],
            resolved: true,
        };
        let mut book = stack("written_book", 1);
        book.components.insert(
            component_ids::WRITTEN_BOOK_CONTENT,
            Component::WrittenBookContent(content.clone()),
        );
        let copied = crafter.craft(&[book, stack("writable_book", 1), empty(), empty()], 2);
        assert!(matches!(
            copied.effective_component(component_ids::WRITTEN_BOOK_CONTENT),
            Some(Component::WrittenBookContent(content)) if content.generation == 1
        ));

        let mut last_copy = stack("written_book", 1);
        last_copy.components.insert(
            component_ids::WRITTEN_BOOK_CONTENT,
            Component::WrittenBookContent(WrittenBookContent {
                generation: 2,
                ..content
            }),
        );
        assert_eq!(
            crafter.craft(&[last_copy, stack("writable_book", 1), empty(), empty()], 2),
            empty()
        );
    }

    #[test]
    fn special_recipes_build_fireworks_pottery_and_tipped_arrows() {
        let crafter = crafter();
        let empty = ItemStack::empty;
        // Star: gunpowder, red dye and a feather make a burst star.
        let grid = vec![
            stack("gunpowder", 1),
            stack("red_dye", 1),
            stack("feather", 1),
            empty(),
            empty(),
            empty(),
            empty(),
            empty(),
            empty(),
        ];
        let star = crafter.craft(&grid, 3);
        assert_eq!(star.name(), Some("minecraft:firework_star"));
        assert_eq!(
            star.effective_component(component_ids::FIREWORK_EXPLOSION),
            Some(Component::FireworkExplosion(FireworkExplosion {
                shape: "burst".to_owned(),
                colors: vec![0xB02E26],
                fade_colors: Vec::new(),
                has_trail: false,
                has_twinkle: false,
            }))
        );

        // A rocket takes paper, gunpowder and the star, and keeps its explosion.
        let rocket = crafter.craft(
            &[
                stack("paper", 1),
                stack("gunpowder", 1),
                star,
                empty(),
                empty(),
                empty(),
                empty(),
                empty(),
                empty(),
            ],
            3,
        );
        assert_eq!(rocket.name(), Some("minecraft:firework_rocket"));
        assert_eq!(rocket.count, 3);
        assert!(matches!(
            rocket.effective_component(component_ids::FIREWORKS),
            Some(Component::Fireworks(fireworks))
                if fireworks.flight_duration == 1 && fireworks.explosions.len() == 1
        ));

        // Dyeing leather sets its packed RGB color.
        let boots = crafter.craft(
            &[
                stack("leather_boots", 1),
                stack("red_dye", 1),
                empty(),
                empty(),
            ],
            2,
        );
        assert_eq!(
            boots.effective_component(component_ids::DYED_COLOR),
            Some(Component::DyedColor(0xB02E26))
        );

        // Four sherds make a decorated pot with the sides in slot order.
        let pot = crafter.craft(
            &[
                stack("brick", 1),
                stack("angler_pottery_sherd", 1),
                stack("blade_pottery_sherd", 1),
                stack("brick", 1),
            ],
            2,
        );
        assert_eq!(
            pot.effective_component(component_ids::POT_DECORATIONS),
            Some(Component::PotDecorations(PotDecorations([
                "minecraft:brick".to_owned(),
                "minecraft:angler_pottery_sherd".to_owned(),
                "minecraft:blade_pottery_sherd".to_owned(),
                "minecraft:brick".to_owned(),
            ])))
        );

        // An arrow and a lingering potion make eight tipped arrows.
        let mut potion = stack("lingering_potion", 1);
        potion.components.insert(
            component_ids::POTION_CONTENTS,
            Component::PotionContents(PotionContents {
                potion: Some("minecraft:swiftness".to_owned()),
                ..PotionContents::default()
            }),
        );
        let arrows = crafter.craft(&[stack("arrow", 1), potion, empty(), empty()], 2);
        assert_eq!(arrows.name(), Some("minecraft:tipped_arrow"));
        assert_eq!(arrows.count, 8);
        assert_eq!(
            arrows.effective_component(component_ids::POTION_CONTENTS),
            Some(Component::PotionContents(PotionContents {
                potion: Some("minecraft:swiftness".to_owned()),
                ..PotionContents::default()
            }))
        );
    }

    #[test]
    fn anvil_rules_read_enchantment_data() {
        let crafter = crafter();
        assert_eq!(
            crafter.enchantment_max_level("minecraft:sharpness"),
            Some(5)
        );
        assert_eq!(crafter.enchantment_anvil_cost("minecraft:smite"), 2);
        assert!(crafter.enchantment_supports("minecraft:sharpness", &stack("iron_sword", 1)));
        assert!(!crafter.enchantment_supports("minecraft:sharpness", &stack("leather_boots", 1)));
        assert!(!crafter.enchantments_compatible("minecraft:sharpness", "minecraft:smite"));
        assert!(crafter.enchantments_compatible("minecraft:sharpness", "minecraft:unbreaking"));
    }
}
