//! The code-backed special crafting recipes (`crafting_special_*`, `crafting_dye`,
//! `crafting_imbue` and `crafting_decorated_pot`).
//!
//! The recipe JSON names the ingredients, but the result is assembled from the grid. This module
//! keeps that assembly separate from the normal shaped/shapeless/cooking path. Every special
//! recipe still has to match the same grid the vanilla recipe accepts; when a value needed to
//! assemble the result is missing (for example a map with no saved data), the recipe does not
//! match instead of inventing a result.

use ferrite_item::{Component, ItemStack, component_id, component_ids, map_id, set_map_id};
use ferrite_nbt::{Compound, List, Tag};
use ferrite_recipe::{Ingredient, ItemTags, Recipe, RecipeKind, SpecialRecipe};
use serde_json::Value;

/// Assembles `recipe` from `grid`, or `None` when the grid does not match.
pub(super) fn craft(recipe: &Recipe, grid: &[ItemStack], tags: &ItemTags) -> Option<ItemStack> {
    let RecipeKind::Special(special) = &recipe.kind else {
        return None;
    };
    let short = special
        .type_name
        .rsplit(':')
        .next()
        .unwrap_or(&special.type_name);
    let items: Vec<&ItemStack> = grid.iter().filter(|stack| !stack.is_empty()).collect();
    match short {
        "crafting_special_bannerduplicate" => banner_duplicate(special, &items),
        "crafting_special_bookcloning" => book_cloning(special, &items, tags),
        "crafting_special_firework_rocket" => firework_rocket(special, &items),
        "crafting_special_firework_star" => firework_star(special, &items, tags),
        "crafting_special_firework_star_fade" => firework_star_fade(special, &items, tags),
        "crafting_special_mapextending" => map_extending(special, &items, tags),
        "crafting_special_repairitem" => repair_item(&items),
        "crafting_special_shielddecoration" => shield_decoration(special, &items, tags),
        "crafting_dye" => dye(special, &items, tags),
        "crafting_imbue" => imbue(special, &items),
        "crafting_decorated_pot" => decorated_pot(special, grid, tags),
        _ => None,
    }
}

fn field<'a>(special: &'a SpecialRecipe, key: &str) -> Option<&'a str> {
    special.data.get(key)?.as_str()
}

fn named(stack: &ItemStack, name: &str) -> bool {
    stack.name() == Some(name)
}

fn result(special: &SpecialRecipe, fallback: &str, count: i32) -> Option<ItemStack> {
    let result = special.result.as_ref();
    let item = result
        .map(|result| result.item.as_str())
        .filter(|item| !item.is_empty())
        .unwrap_or(fallback);
    let count = result.map_or(count, |result| {
        i32::try_from(result.count).unwrap_or(1).max(1)
    });
    let mut stack = ItemStack::from_name(item)?.copy_with_count(count);
    if let Some(components) = result.and_then(|result| result.components.as_ref()) {
        apply_components(&mut stack, components)?;
    }
    Some(stack)
}

/// Applies a recipe result's JSON component patch. Unknown component names or values fail the
/// recipe rather than silently dropping data.
pub(super) fn apply_components(stack: &mut ItemStack, components: &Value) -> Option<()> {
    for (name, value) in components.as_object()? {
        let id = component_id(name)?;
        let tag = ferrite_nbt::snbt::parse(&value.to_string()).ok()?;
        stack.components.insert(id, Component::from_nbt(id, &tag));
    }
    Some(())
}

fn component_tag(stack: &ItemStack, name: &str) -> Option<Tag> {
    stack
        .effective_component(component_id(name)?)?
        .to_nbt()
        .ok()
}

fn ingredient(field: &str) -> Ingredient {
    Ingredient::from(field)
}

fn banner_duplicate(special: &SpecialRecipe, items: &[&ItemStack]) -> Option<ItemStack> {
    let banner = field(special, "banner")?;
    if items.len() != 2 || items.iter().any(|stack| !named(stack, banner)) {
        return None;
    }
    let patterns = component_id("banner_patterns");
    let source = items
        .iter()
        .find(|stack| patterns.is_some_and(|id| stack.components.contains(id)))
        .copied()
        .unwrap_or(items[0]);
    let mut out = result(special, banner, 1)?;
    out.components = source.components.clone();
    Some(out)
}

fn book_cloning(
    special: &SpecialRecipe,
    items: &[&ItemStack],
    tags: &ItemTags,
) -> Option<ItemStack> {
    let source_name = field(special, "source")?;
    let material = field(special, "material")?;
    if items.len() != 2 {
        return None;
    }
    let source_index = items.iter().position(|stack| named(stack, source_name))?;
    let material_stack = items[1 - source_index];
    if !ingredient(material).matches(material_stack.name()?, tags) {
        return None;
    }
    let source = items[source_index];
    let Component::WrittenBookContent(mut content) =
        source.effective_component(component_ids::WRITTEN_BOOK_CONTENT)?
    else {
        return None;
    };
    if content.generation >= 3 {
        return None;
    }
    content.generation += 1;
    let mut out = result(special, "minecraft:written_book", 1)?;
    out.components.insert(
        component_ids::WRITTEN_BOOK_CONTENT,
        Component::WrittenBookContent(content),
    );
    Some(out)
}

fn repair_item(items: &[&ItemStack]) -> Option<ItemStack> {
    if items.len() != 2 {
        return None;
    }
    let (left, right) = (items[0], items[1]);
    if left.name()? != right.name()? {
        return None;
    }
    let max = left.max_damage()?;
    let durability = (max - left.damage()).max(0) + (max - right.damage()).max(0);
    // `RepairItemRecipe`: the repaired item keeps 5% of its maximum durability as a bonus.
    let repaired = (durability + max * 5 / 100).min(max);
    let damage = (max - repaired).max(0);
    let mut out = left.copy_with_count(1);
    if damage == 0 {
        out.components.remove(component_ids::DAMAGE);
    } else {
        out.components
            .insert(component_ids::DAMAGE, Component::Damage(damage));
    }
    Some(out)
}

fn dye(special: &SpecialRecipe, items: &[&ItemStack], tags: &ItemTags) -> Option<ItemStack> {
    let target = field(special, "target")?;
    let dye = field(special, "dye")?;
    let target_index = items.iter().position(|stack| named(stack, target))?;
    let dyes: Vec<&ItemStack> = items
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != target_index)
        .map(|(_, stack)| *stack)
        .collect();
    if dyes.is_empty()
        || dyes.iter().any(|stack| {
            stack
                .name()
                .is_none_or(|name| !ingredient(dye).matches(name, tags))
        })
    {
        return None;
    }
    let colors: Vec<i32> = dyes
        .iter()
        .map(|stack| dye_color(stack.name()?))
        .collect::<Option<_>>()?;
    let color = average_color(&colors);
    let mut out = items[target_index].copy_with_count(1);
    out.components
        .insert(component_ids::DYED_COLOR, Component::DyedColor(color));
    Some(out)
}

fn imbue(special: &SpecialRecipe, items: &[&ItemStack]) -> Option<ItemStack> {
    let source_name = field(special, "source")?;
    let material = field(special, "material")?;
    if items.len() != 9 {
        return None;
    }
    let source_index = items.iter().position(|stack| named(stack, source_name))?;
    if items
        .iter()
        .enumerate()
        .any(|(index, stack)| index != source_index && !named(stack, material))
    {
        return None;
    }
    let contents = items[source_index].effective_component(component_ids::POTION_CONTENTS)?;
    let mut out = result(special, "minecraft:tipped_arrow", 8)?;
    out.components
        .insert(component_ids::POTION_CONTENTS, contents);
    Some(out)
}

fn firework_star(
    special: &SpecialRecipe,
    items: &[&ItemStack],
    tags: &ItemTags,
) -> Option<ItemStack> {
    let dye = field(special, "dye")?;
    let fuel = field(special, "fuel")?;
    let trail = field(special, "trail")?;
    let twinkle = field(special, "twinkle")?;
    let shapes = special.data.get("shapes")?.as_object()?;
    let mut colors = Vec::new();
    let mut fuel_count = 0;
    let mut shape: Option<String> = None;
    let mut has_trail = false;
    let mut has_twinkle = false;
    for stack in items {
        let name = stack.name()?;
        if ingredient(dye).matches(name, tags) {
            colors.push(dye_color(name)?);
        } else if name == fuel {
            fuel_count += 1;
            if fuel_count > 1 {
                return None;
            }
        } else if name == trail {
            if has_trail {
                return None;
            }
            has_trail = true;
        } else if name == twinkle {
            if has_twinkle {
                return None;
            }
            has_twinkle = true;
        } else if let Some((found, _)) = shapes.iter().find(|(_, item)| item.as_str() == Some(name))
        {
            if shape.is_some() {
                return None;
            }
            shape = Some(found.clone());
        } else {
            return None;
        }
    }
    // `FireworkStarRecipe` needs exactly one gunpowder; dyes alone are not a star.
    if colors.is_empty() || colors.len() > 8 || fuel_count != 1 {
        return None;
    }
    let mut explosion = Compound::new();
    explosion.insert(
        "shape".into(),
        Tag::String(shape.unwrap_or_else(|| "small_ball".to_owned())),
    );
    explosion.insert("colors".into(), Tag::List(List::Int(colors)));
    if has_trail {
        explosion.insert("trail".into(), Tag::Byte(1));
    }
    if has_twinkle {
        explosion.insert("twinkle".into(), Tag::Byte(1));
    }
    let mut out = result(special, "minecraft:firework_star", 1)?;
    out.components.insert(
        component_id("firework_explosion")?,
        Component::Opaque(Tag::Compound(explosion)),
    );
    Some(out)
}

fn firework_star_fade(
    special: &SpecialRecipe,
    items: &[&ItemStack],
    tags: &ItemTags,
) -> Option<ItemStack> {
    let target = field(special, "target")?;
    let dye = field(special, "dye")?;
    let target_index = items.iter().position(|stack| named(stack, target))?;
    let dyes: Vec<&ItemStack> = items
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != target_index)
        .map(|(_, stack)| *stack)
        .collect();
    if dyes.is_empty()
        || dyes.iter().any(|stack| {
            stack
                .name()
                .is_none_or(|name| !ingredient(dye).matches(name, tags))
        })
    {
        return None;
    }
    let colors: Vec<i32> = dyes
        .iter()
        .map(|stack| dye_color(stack.name()?))
        .collect::<Option<_>>()?;
    let id = component_id("firework_explosion")?;
    let tag = component_tag(items[target_index], "firework_explosion")?;
    let Tag::Compound(mut explosion) = tag else {
        return None;
    };
    explosion.insert("fade_colors".into(), Tag::List(List::Int(colors)));
    let mut out = items[target_index].copy_with_count(1);
    out.components
        .insert(id, Component::Opaque(Tag::Compound(explosion)));
    Some(out)
}

fn firework_rocket(special: &SpecialRecipe, items: &[&ItemStack]) -> Option<ItemStack> {
    let shell = field(special, "shell")?;
    let fuel = field(special, "fuel")?;
    let star = field(special, "star")?;
    let mut shell_count = 0;
    let mut fuel_count = 0;
    let mut explosions = Vec::new();
    for stack in items {
        let name = stack.name()?;
        if name == shell {
            shell_count += 1;
            if shell_count > 1 {
                return None;
            }
        } else if name == fuel {
            fuel_count += 1;
            if fuel_count > 3 {
                return None;
            }
        } else if name == star {
            let tag = component_tag(stack, "firework_explosion")?;
            let Tag::Compound(explosion) = tag else {
                return None;
            };
            explosions.push(explosion);
        } else {
            return None;
        }
    }
    if shell_count != 1 || fuel_count == 0 {
        return None;
    }
    let mut fireworks = Compound::new();
    fireworks.insert("flight_duration".into(), Tag::Byte(fuel_count));
    fireworks.insert("explosions".into(), Tag::List(List::Compound(explosions)));
    let mut out = result(special, "minecraft:firework_rocket", 3)?;
    out.components.insert(
        component_id("fireworks")?,
        Component::Opaque(Tag::Compound(fireworks)),
    );
    Some(out)
}

fn map_extending(
    special: &SpecialRecipe,
    items: &[&ItemStack],
    tags: &ItemTags,
) -> Option<ItemStack> {
    let map = field(special, "map")?;
    let material = field(special, "material")?;
    if items.len() != 9 {
        return None;
    }
    let map_index = items.iter().position(|stack| {
        stack
            .name()
            .is_some_and(|name| ingredient(map).matches(name, tags))
    })?;
    if items
        .iter()
        .enumerate()
        .any(|(index, stack)| index != map_index && !named(stack, material))
    {
        return None;
    }
    let map_stack = items[map_index];
    let stored = map_id(map_stack)?;
    let store = super::super::map::primary_store()?;
    let saved = store.get(stored).ok().flatten()?;
    let new_id = store.allocate(saved.scaled()).ok()?;
    let mut out = map_stack.copy_with_count(1);
    set_map_id(&mut out, new_id);
    Some(out)
}

fn shield_decoration(
    special: &SpecialRecipe,
    items: &[&ItemStack],
    tags: &ItemTags,
) -> Option<ItemStack> {
    let target = field(special, "target")?;
    let banner = field(special, "banner")?;
    if items.len() != 2 {
        return None;
    }
    let target_index = items.iter().position(|stack| named(stack, target))?;
    let banner_stack = items[1 - target_index];
    if !ingredient(banner).matches(banner_stack.name()?, tags) {
        return None;
    }
    let mut out = result(special, target, 1)?;
    for name in ["banner_patterns", "base_color"] {
        if let Some(id) = component_id(name)
            && let Some(component) = banner_stack.effective_component(id)
        {
            out.components.insert(id, component);
        }
    }
    if let Some(color) = banner_stack.name().and_then(banner_color)
        && let Some(id) = component_id("base_color")
        && !out.components.contains(id)
    {
        out.components
            .insert(id, Component::Opaque(Tag::String(color)));
    }
    Some(out)
}

fn decorated_pot(
    special: &SpecialRecipe,
    grid: &[ItemStack],
    tags: &ItemTags,
) -> Option<ItemStack> {
    if grid.len() != 9 || !named(&grid[4], "minecraft:brick") {
        return None;
    }
    // The 3x3 recipe is a plus: back is above the brick, front below, left and right beside it.
    let sides = [("back", 1), ("left", 3), ("right", 5), ("front", 7)];
    let mut decorations = Compound::new();
    for (side, index) in sides {
        let stack = &grid[index];
        let name = stack.name()?;
        if !ingredient(field(special, side)?).matches(name, tags) {
            return None;
        }
        let mut entry = Compound::new();
        entry.insert("id".into(), Tag::String(name.to_owned()));
        decorations.insert(side.into(), Tag::Compound(entry));
    }
    if grid
        .iter()
        .enumerate()
        .any(|(index, stack)| !matches!(index, 1 | 3 | 4 | 5 | 7) && !stack.is_empty())
    {
        return None;
    }
    let mut out = result(special, "minecraft:decorated_pot", 1)?;
    out.components.insert(
        component_id("pot_decorations")?,
        Component::Opaque(Tag::Compound(decorations)),
    );
    Some(out)
}

fn average_color(colors: &[i32]) -> i32 {
    let mut red = 0;
    let mut green = 0;
    let mut blue = 0;
    for color in colors {
        red += (color >> 16) & 0xff;
        green += (color >> 8) & 0xff;
        blue += color & 0xff;
    }
    let count = i32::try_from(colors.len()).unwrap_or(1).max(1);
    ((red / count) << 16) | ((green / count) << 8) | (blue / count)
}

fn banner_color(name: &str) -> Option<String> {
    Some(
        name.strip_prefix("minecraft:")?
            .strip_suffix("_banner")?
            .to_owned(),
    )
}

fn dye_color(name: &str) -> Option<i32> {
    Some(match name.strip_prefix("minecraft:").unwrap_or(name) {
        "white_dye" => 0xF9FFFE,
        "orange_dye" => 0xF9801D,
        "magenta_dye" => 0xC74EBD,
        "light_blue_dye" => 0x3AB3DA,
        "yellow_dye" => 0xFED83D,
        "lime_dye" => 0x80C71F,
        "pink_dye" => 0xF38BAA,
        "gray_dye" => 0x474F52,
        "light_gray_dye" => 0x9D9D97,
        "cyan_dye" => 0x169C9C,
        "purple_dye" => 0x8932B8,
        "blue_dye" => 0x3C44AA,
        "brown_dye" => 0x835432,
        "green_dye" => 0x5E7C16,
        "red_dye" => 0xB02E26,
        "black_dye" => 0x1D1D21,
        _ => return None,
    })
}
