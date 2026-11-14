//! `enchant_with_levels` and `enchant_randomly`, following vanilla's `EnchantmentHelper`
//! (`selectEnchantment`, `getAvailableEnchantmentResults`, `filterCompatibleEnchantments`) and
//! `EnchantRandomlyFunction`.

use crate::data::{Enchantment, GameData};
use crate::{LootHost, LootStack, Rng, normalize_id};
use ferrite_nbt::{Compound, Tag};

const BOOK: &str = "minecraft:book";
const ENCHANTED_BOOK: &str = "minecraft:enchanted_book";

/// The component an item's enchantments live in.
fn component_for(item: &str) -> &'static str {
    if item == ENCHANTED_BOOK {
        "minecraft:stored_enchantments"
    } else {
        "minecraft:enchantments"
    }
}

/// Adds `level` of `id`, replacing a lower or equal level (vanilla `ItemStack.enchant` keeps
/// the maximum).
pub(crate) fn add_enchantment(stack: &mut LootStack, id: &str, level: i32) {
    let name = component_for(&stack.item);
    let mut levels = match stack.components.remove(name) {
        Some(Tag::Compound(levels)) => levels,
        _ => Compound::new(),
    };
    let id = normalize_id(id);
    let keep = match levels.get(&id) {
        Some(Tag::Int(existing)) => (*existing).max(level),
        _ => level,
    };
    levels.insert(id, Tag::Int(keep));
    stack
        .components
        .insert(name.to_owned(), Tag::Compound(levels));
}

/// Picks one element by weight, vanilla `WeightedRandom.getRandomItem`.
fn pick<'a>(entries: &'a [(&'a Enchantment, i32)], rng: &mut impl Rng) -> Option<usize> {
    let total: i64 = entries
        .iter()
        .map(|(enchantment, _)| i64::from(enchantment.weight.max(0)))
        .sum();
    if total <= 0 {
        return None;
    }
    let mut roll = i64::from(rng.next_int(i32::try_from(total).unwrap_or(i32::MAX)));
    for (index, (enchantment, _)) in entries.iter().enumerate() {
        roll -= i64::from(enchantment.weight.max(0));
        if roll < 0 {
            return Some(index);
        }
    }
    None
}

/// `EnchantmentHelper.getAvailableEnchantmentResults`: the highest level of every enchantment
/// whose cost range covers `power`.
fn available<'a>(
    power: i32,
    item: &str,
    candidates: &'a [&'a Enchantment],
) -> Vec<(&'a Enchantment, i32)> {
    let is_book = item == BOOK;
    let mut out = Vec::new();
    for enchantment in candidates {
        if !(is_book || enchantment.primary.iter().any(|id| id == item)) {
            continue;
        }
        for level in (1..=enchantment.max_level).rev() {
            if power >= enchantment.min_cost.at(level) && power <= enchantment.max_cost.at(level) {
                out.push((*enchantment, level));
                break;
            }
        }
    }
    out
}

/// `EnchantmentHelper.selectEnchantment`.
fn select<'a>(
    rng: &mut impl Rng,
    enchantability: i32,
    levels: i32,
    item: &str,
    candidates: &'a [&'a Enchantment],
) -> Vec<(&'a Enchantment, i32)> {
    let mut result = Vec::new();
    let quarter = enchantability / 4 + 1;
    let mut power = levels
        .saturating_add(1)
        .saturating_add(rng.next_int(quarter))
        .saturating_add(rng.next_int(quarter));
    let span = (rng.next_float() + rng.next_float() - 1.0) * 0.15;
    power = (((power as f32) + (power as f32) * span + 0.5).floor() as i32).max(1);
    let mut pool = available(power, item, candidates);
    if let Some(index) = pick(&pool, rng) {
        result.push(pool[index]);
    }
    while rng.next_int(50) <= power {
        if let Some(&(last, _)) = result.last() {
            pool.retain(|(other, _)| {
                other.id != last.id
                    && !last.exclusive.contains(&other.id)
                    && !other.exclusive.contains(&last.id)
            });
        }
        if pool.is_empty() {
            break;
        }
        if let Some(index) = pick(&pool, rng) {
            result.push(pool[index]);
        }
        power /= 2;
    }
    result
}

fn candidates<'a>(data: &'a GameData, options: &Option<Vec<String>>) -> Vec<&'a Enchantment> {
    data.enchantments
        .iter()
        .filter(|enchantment| {
            options
                .as_ref()
                .is_none_or(|ids| ids.contains(&enchantment.id))
        })
        .collect()
}

/// `EnchantmentHelper.selectEnchantment` over the `#minecraft:in_enchanting_table` enchantments
/// (what an enchanting table offers): `(enchantment id, level)` pairs, empty when none apply.
pub(crate) fn for_table(
    data: &GameData,
    item: &str,
    enchantability: i32,
    levels: i32,
    rng: &mut impl Rng,
) -> Vec<(String, i32)> {
    let options = Some(data.resolve_ids(
        "enchantment",
        &serde_json::Value::String("#minecraft:in_enchanting_table".into()),
    ));
    let candidates = candidates(data, &options);
    select(rng, enchantability, levels, item, &candidates)
        .into_iter()
        .map(|(enchantment, level)| (enchantment.id.clone(), level))
        .collect()
}

/// `enchant_with_levels`: enchants as the enchanting table would at `levels`.
pub(crate) fn with_levels(
    stack: &mut LootStack,
    levels: i32,
    options: &Option<Vec<String>>,
    data: &GameData,
    host: Option<&dyn LootHost>,
    rng: &mut impl Rng,
) {
    let Some(enchantability) = host.and_then(|host| host.enchantability(&stack.item)) else {
        // No `minecraft:enchantable` component: vanilla selects nothing.
        return;
    };
    let candidates = candidates(data, options);
    let chosen = select(rng, enchantability, levels, &stack.item, &candidates);
    if chosen.is_empty() {
        return;
    }
    if stack.item == BOOK {
        *stack = LootStack::new(ENCHANTED_BOOK);
    }
    for (enchantment, level) in chosen {
        add_enchantment(stack, &enchantment.id, level);
    }
}

/// `enchant_randomly`: one random enchantment at a random level.
pub(crate) fn randomly(
    stack: &mut LootStack,
    options: &Option<Vec<String>>,
    only_compatible: bool,
    data: &GameData,
    rng: &mut impl Rng,
) {
    let is_book = stack.item == BOOK;
    let check = !is_book && only_compatible;
    let list: Vec<&Enchantment> = candidates(data, options)
        .into_iter()
        .filter(|enchantment| !check || enchantment.supported.contains(&stack.item))
        .collect();
    if list.is_empty() {
        return;
    }
    let chosen = list[rng.next_int(i32::try_from(list.len()).unwrap_or(i32::MAX)) as usize];
    // Mth.nextInt(random, min, max): min + nextInt(max - min + 1); the minimum level is 1.
    let level = 1 + rng.next_int(chosen.max_level.max(1));
    if is_book {
        *stack = LootStack::new(ENCHANTED_BOOK);
    }
    add_enchantment(stack, &chosen.id, level);
}
