//! Additional vanilla 26.3 block entities:
//! - `brushable_block` (`suspicious_sand` / `suspicious_gravel` archaeology brushing)
//! - `lectern` (book placement, page turning, comparator signal)
//! - `decorated_pot` (single-stack storage + projectile shatter)
//! - `creaking_heart` (pale oak log alignment + night activation)
//! - `bell` (ringing highlights raiders within 48 blocks with `Glowing`)
//! - `crafter` (redstone pulse auto-crafting), `banner`, `skull`, `copper_golem_statue`,
//!   `potent_sulfur`, `shelf`, `test_instance_block` block entity NBT persistence.

use ferrite_entity::effect::{Effect, MobEffectInstance};
use ferrite_entity::mob;
use ferrite_entity::{EntityKey, EntityStore};
use ferrite_item::ItemStack;
use ferrite_math::AABB;
use ferrite_nbt::{Compound, Tag};
use ferrite_world::BlockEntity;

use super::anvil::AnvilChunkSource;
use super::blocks::{property, with_props};

/// Ensures a generic block entity exists on placement for all remaining 26.3 block entity types.
pub fn ensure_by_block(source: &AnvilChunkSource, [x, y, z]: [i32; 3], block_name: &str) {
    let be_id = match block_name {
        "minecraft:suspicious_sand" | "minecraft:suspicious_gravel" => "minecraft:brushable_block",
        "minecraft:lectern" => "minecraft:lectern",
        "minecraft:decorated_pot" => "minecraft:decorated_pot",
        "minecraft:creaking_heart" => "minecraft:creaking_heart",
        "minecraft:bell" => "minecraft:bell",
        "minecraft:crafter" => "minecraft:crafter",
        "minecraft:copper_golem_statue" => "minecraft:copper_golem_statue",
        "minecraft:potent_sulfur" => "minecraft:potent_sulfur",
        "minecraft:test_instance_block" => "minecraft:test_instance_block",
        n if n.ends_with("_banner") => "minecraft:banner",
        n if n.ends_with("_head") || n.ends_with("_skull") => "minecraft:skull",
        n if n.ends_with("_shelf") => "minecraft:shelf",
        n if n.ends_with("_hanging_sign") => "minecraft:hanging_sign",
        _ => return,
    };
    let Some(type_id) = ferrite_data::registries::id("minecraft:block_entity_type", be_id)
        .and_then(|id| u32::try_from(id).ok())
    else {
        return;
    };
    let _ = source.update_block_entity(x, y, z, |entity| {
        if entity.is_some() {
            return (false, ());
        }
        let mut nbt = Compound::new();
        nbt.insert("id".into(), Tag::String(be_id.into()));
        nbt.insert("x".into(), Tag::Int(x));
        nbt.insert("y".into(), Tag::Int(y));
        nbt.insert("z".into(), Tag::Int(z));
        *entity = Some(BlockEntity { type_id, data: nbt });
        (true, ())
    });
}

/// Vanilla archaeology brushing (`BrushableBlockEntity.brush`): advances `dusted` 0->1->2->3
/// on `suspicious_sand` / `suspicious_gravel`. On completion, turns the block into plain
/// `sand` / `gravel` and returns the unearthed `ItemStack`.
pub fn brush_block(
    source: &AnvilChunkSource,
    pos: [i32; 3],
    state: u16,
) -> Option<(u16, Option<ItemStack>)> {
    let (name, _) = ferrite_data::blocks::state(state)?;
    let plain_name = match name {
        "minecraft:suspicious_sand" => "sand",
        "minecraft:suspicious_gravel" => "gravel",
        _ => return None,
    };
    let dusted: u8 = property(state, "dusted")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if dusted < 3 {
        let next_str = match dusted + 1 {
            1 => "1",
            2 => "2",
            _ => "3",
        };
        let next_state = with_props(state, &[("dusted", next_str)])?;
        let _ = source.set_block(pos[0], pos[1], pos[2], u32::from(next_state));
        return Some((next_state, None));
    }
    let stored = source.block_entity(pos[0], pos[1], pos[2]).and_then(|be| {
        if let Some(Tag::Compound(item_nbt)) = be.data.get("item") {
            ItemStack::from_nbt(item_nbt).ok()
        } else {
            None
        }
    });
    let loot = stored.filter(|s| !s.is_empty()).unwrap_or_else(|| {
        let pool = [
            "minecraft:emerald",
            "minecraft:gold_nugget",
            "minecraft:brick",
            "minecraft:coal",
            "minecraft:diamond",
        ];
        let idx = ((pos[0].wrapping_mul(31) ^ pos[1].wrapping_mul(17) ^ pos[2].wrapping_mul(13))
            as usize)
            % pool.len();
        ItemStack::from_name(pool[idx]).unwrap_or_else(ItemStack::empty)
    });
    let plain_state = ferrite_data::blocks::default_state(plain_name)?;
    let _ = source.set_block(pos[0], pos[1], pos[2], u32::from(plain_state));
    Some((plain_state, Some(loot)))
}

/// Vanilla `LecternBlock.useItemOn` / `LecternBlockEntity`: places a written/writable book onto
/// an empty lectern, or turns the page (and extracts when sneaking) when a book is present.
/// Returns `(new_state, extracted_book, consumed_held)`.
pub fn interact_lectern(
    source: &AnvilChunkSource,
    pos: [i32; 3],
    state: u16,
    held: &ItemStack,
    sneaking: bool,
) -> Option<(u16, Option<ItemStack>, bool)> {
    let (name, _) = ferrite_data::blocks::state(state)?;
    if name != "minecraft:lectern" {
        return None;
    }
    let has_book = property(state, "has_book") == Some("true");
    if !has_book {
        if !matches!(
            held.name(),
            Some("minecraft:written_book" | "minecraft:writable_book")
        ) {
            return None;
        }
        let mut book = held.clone();
        book.count = 1;
        ensure_by_block(source, pos, "minecraft:lectern");
        if let Ok(book_nbt) = book.to_nbt() {
            let _ = source.update_block_entity(pos[0], pos[1], pos[2], |be| {
                if let Some(be) = be.as_mut() {
                    be.data
                        .insert("Book".into(), Tag::Compound(book_nbt.clone()));
                    be.data.insert("Page".into(), Tag::Int(0));
                    return (true, ());
                }
                (false, ())
            });
        }
        let next = with_props(state, &[("has_book", "true")])?;
        let _ = source.set_block(pos[0], pos[1], pos[2], u32::from(next));
        Some((next, None, true))
    } else if sneaking {
        let mut taken = None;
        let _ = source.update_block_entity(pos[0], pos[1], pos[2], |be| {
            if let Some(be) = be.as_mut() {
                if let Some(Tag::Compound(book_nbt)) = be.data.shift_remove("Book") {
                    taken = ItemStack::from_nbt(&book_nbt).ok();
                }
                be.data.insert("Page".into(), Tag::Int(0));
                return (true, ());
            }
            (false, ())
        });
        let next = with_props(state, &[("has_book", "false")])?;
        let _ = source.set_block(pos[0], pos[1], pos[2], u32::from(next));
        Some((next, taken, false))
    } else {
        let _ = source.update_block_entity(pos[0], pos[1], pos[2], |be| {
            if let Some(be) = be.as_mut() {
                let page = match be.data.get("Page") {
                    Some(Tag::Int(p)) => (*p + 1) % 15,
                    _ => 1,
                };
                be.data.insert("Page".into(), Tag::Int(page));
                return (true, ());
            }
            (false, ())
        });
        Some((state, None, false))
    }
}

/// Vanilla `DecoratedPotBlock.useItemOn`: stores up to one full stack of a single item type in
/// the pot's `item` NBT. Returns `Some(true)` if 1 held item was inserted.
pub fn insert_decorated_pot(
    source: &AnvilChunkSource,
    pos: [i32; 3],
    held: &ItemStack,
) -> Option<bool> {
    if held.is_empty() {
        return Some(false);
    }
    ensure_by_block(source, pos, "minecraft:decorated_pot");
    let mut inserted = false;
    let _ = source.update_block_entity(pos[0], pos[1], pos[2], |be| {
        let Some(be) = be.as_mut() else {
            return (false, ());
        };
        let mut current = match be.data.get("item") {
            Some(Tag::Compound(c)) => ItemStack::from_nbt(c).ok().unwrap_or_else(ItemStack::empty),
            _ => ItemStack::empty(),
        };
        if current.is_empty() {
            current = held.clone();
            current.count = 1;
            inserted = true;
        } else if current.item == held.item
            && current.components == held.components
            && current.count < current.max_stack_size()
        {
            current.count += 1;
            inserted = true;
        }
        if inserted && let Ok(nbt) = current.to_nbt() {
            be.data.insert("item".into(), Tag::Compound(nbt));
            return (true, ());
        }
        (false, ())
    });
    Some(inserted)
}

/// Vanilla `CreakingHeartBlock.hasRequiredLogs`: checks aligned `pale_oak_log` (or wood) on both
/// sides of `axis` (`x`/`y`/`z`) and computes `uprooted` / `dormant` / `awake` (at night).
pub fn evaluate_creaking_heart(
    source: &AnvilChunkSource,
    pos: [i32; 3],
    state: u16,
    is_night: bool,
) -> Option<(u16, bool)> {
    let (name, _) = ferrite_data::blocks::state(state)?;
    if name != "minecraft:creaking_heart" {
        return None;
    }
    let axis = property(state, "axis").unwrap_or("y");
    let (dx, dy, dz) = match axis {
        "x" => (1, 0, 0),
        "z" => (0, 0, 1),
        _ => (0, 1, 0),
    };
    let is_pale_log = |p: [i32; 3]| {
        source
            .block(p[0], p[1], p[2])
            .and_then(|s| u16::try_from(s).ok())
            .and_then(ferrite_data::blocks::state)
            .is_some_and(|(n, _)| {
                matches!(
                    n,
                    "minecraft:pale_oak_log"
                        | "minecraft:stripped_pale_oak_log"
                        | "minecraft:pale_oak_wood"
                        | "minecraft:stripped_pale_oak_wood"
                )
            })
    };
    let aligned = is_pale_log([pos[0] + dx, pos[1] + dy, pos[2] + dz])
        && is_pale_log([pos[0] - dx, pos[1] - dy, pos[2] - dz]);
    let target_state = if !aligned {
        "uprooted"
    } else if is_night {
        "awake"
    } else {
        "dormant"
    };
    let next = with_props(state, &[("creaking_heart_state", target_state)])?;
    if next != state {
        let _ = source.set_block(pos[0], pos[1], pos[2], u32::from(next));
    }
    Some((next, target_state == "awake"))
}

/// Vanilla `BellBlockEntity.makeRaidersGlow`: highlights every raider within 48 blocks of the
/// rung bell with `Effect::Glowing` for 60 ticks. Returns the number of illuminated raiders.
pub fn ring_bell_highlight_raiders(store: &mut EntityStore, pos: [i32; 3]) -> usize {
    let cx = f64::from(pos[0]) + 0.5;
    let cy = f64::from(pos[1]) + 0.5;
    let cz = f64::from(pos[2]) + 0.5;
    let area = AABB::new(
        cx - 48.0,
        cy - 48.0,
        cz - 48.0,
        cx + 48.0,
        cy + 48.0,
        cz + 48.0,
    );
    let keys: Vec<EntityKey> = store
        .query_vec(area)
        .into_iter()
        .filter(|&k| mob::state(store, k).is_some_and(|m| !m.is_dead() && m.kind.is_raider()))
        .collect();
    let glow = MobEffectInstance::new(Effect::Glowing.id(), 60, 0);
    for key in &keys {
        mob::add_effect(store, *key, glow);
    }
    keys.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn archaeology_lectern_pot_creaking_heart_and_bell_work() {
        let source = AnvilChunkSource::new_memory();
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        // 1. Brushable suspicious sand (dusted 0->1->2->3->sand + loot).
        let sus = ferrite_data::blocks::default_state("suspicious_sand").unwrap();
        source.set_block(2, 64, 2, u32::from(sus)).unwrap();
        let mut st = sus;
        for step in 1..=3 {
            let (next, loot) = brush_block(&source, [2, 64, 2], st).unwrap();
            assert!(loot.is_none());
            assert_eq!(property(next, "dusted"), Some(step.to_string().as_str()));
            st = next;
        }
        let (final_st, loot) = brush_block(&source, [2, 64, 2], st).unwrap();
        assert_eq!(
            ferrite_data::blocks::state(final_st).map(|(n, _)| n),
            Some("minecraft:sand")
        );
        assert!(loot.is_some_and(|s| !s.is_empty()));

        // 2. Lectern place book + turn page + sneak extract.
        let lectern = ferrite_data::blocks::default_state("lectern").unwrap();
        source.set_block(4, 64, 4, u32::from(lectern)).unwrap();
        let book = ItemStack::from_name("minecraft:written_book").unwrap();
        let (placed_st, _, consumed) =
            interact_lectern(&source, [4, 64, 4], lectern, &book, false).unwrap();
        assert!(consumed);
        assert_eq!(property(placed_st, "has_book"), Some("true"));
        let (removed_st, taken, _) =
            interact_lectern(&source, [4, 64, 4], placed_st, &ItemStack::empty(), true).unwrap();
        assert_eq!(property(removed_st, "has_book"), Some("false"));
        assert_eq!(
            taken.as_ref().and_then(ItemStack::name),
            Some("minecraft:written_book")
        );

        // 3. Decorated pot stores matching stack.
        let pot = ferrite_data::blocks::default_state("decorated_pot").unwrap();
        source.set_block(6, 64, 6, u32::from(pot)).unwrap();
        let emerald = ItemStack::from_name("minecraft:emerald").unwrap();
        assert_eq!(
            insert_decorated_pot(&source, [6, 64, 6], &emerald),
            Some(true)
        );
        assert_eq!(
            insert_decorated_pot(&source, [6, 64, 6], &emerald),
            Some(true)
        );

        // 4. Creaking heart wakes at night when sandwiched between pale oak logs.
        let log = ferrite_data::blocks::default_state("pale_oak_log").unwrap();
        let heart = ferrite_data::blocks::default_state("creaking_heart").unwrap();
        source.set_block(8, 63, 8, u32::from(log)).unwrap();
        source.set_block(8, 64, 8, u32::from(heart)).unwrap();
        source.set_block(8, 65, 8, u32::from(log)).unwrap();
        let (day_st, awake_day) =
            evaluate_creaking_heart(&source, [8, 64, 8], heart, false).unwrap();
        assert!(!awake_day);
        assert_eq!(property(day_st, "creaking_heart_state"), Some("dormant"));
        let (night_st, awake_night) =
            evaluate_creaking_heart(&source, [8, 64, 8], day_st, true).unwrap();
        assert!(awake_night);
        assert_eq!(property(night_st, "creaking_heart_state"), Some("awake"));

        // 5. Bell highlights nearby raiders with Glowing.
        let mut store = EntityStore::default();
        let mut rng = ferrite_entity::Rng::new(1);
        let pillager = store.insert(mob::new_mob(
            mob::MobKind::Pillager,
            ferrite_math::Vec3::new(10.0, 64.0, 10.0),
            &mut rng,
        ));
        assert_eq!(ring_bell_highlight_raiders(&mut store, [8, 64, 8]), 1);
        assert!(
            store
                .data(pillager)
                .unwrap()
                .effects
                .get(Effect::Glowing)
                .is_some()
        );
    }
}
