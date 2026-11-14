//! The enchanting table: vanilla `EnchantmentMenu` offers, `EnchantmentHelper` costs and
//! selection, over the loot crate's enchantment data.
//!
//! Randomness is `java.util.Random` seeded the way the menu seeds its source, so the same
//! seed, item and bookshelf count give the same buttons. (Vanilla's source type is not
//! Java's LCG, so a seed does not reproduce vanilla's exact buttons.)

use ferrite_inventory::{EnchantOffer, EnchantTable};
use ferrite_item::ItemStack;

/// `java.util.Random`.
pub struct JavaRandom(i64);

const MASK: i64 = (1 << 48) - 1;

impl JavaRandom {
    pub fn new(seed: i64) -> Self {
        Self((seed ^ 0x5_DEEC_E66D) & MASK)
    }

    pub fn set_seed(&mut self, seed: i64) {
        self.0 = (seed ^ 0x5_DEEC_E66D) & MASK;
    }

    fn next(&mut self, bits: u32) -> i32 {
        self.0 = (self.0.wrapping_mul(0x5_DEEC_E66D).wrapping_add(0xB)) & MASK;
        (self.0 >> (48 - bits)) as i32
    }
}

impl ferrite_loot::Rng for JavaRandom {
    fn next_int(&mut self, bound: i32) -> i32 {
        if bound & (bound - 1) == 0 {
            return ((i64::from(bound) * i64::from(self.next(31))) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let value = bits % bound;
            if bits.wrapping_sub(value).wrapping_add(bound - 1) >= 0 {
                return value;
            }
        }
    }

    fn next_float(&mut self) -> f32 {
        self.next(24) as f32 / (1 << 24) as f32
    }
}

/// `EnchantmentHelper.getEnchantmentCost`.
pub fn cost(rng: &mut impl ferrite_loot::Rng, slot: i32, bookshelves: i32) -> i32 {
    let shelves = bookshelves.min(15);
    let base = rng.next_int(8) + 1 + (shelves >> 1) + rng.next_int(shelves + 1);
    match slot {
        0 => (base / 3).max(1),
        1 => base * 2 / 3 + 1,
        _ => base.max(shelves * 2),
    }
}

/// The id the client knows an enchantment by: its position in the synchronized `enchantment`
/// registry, which the network layer sends in sorted file order (see `ferrite_net::registry`).
fn enchantment_id(name: &str) -> Option<i32> {
    static IDS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    let ids = IDS.get_or_init(|| {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../vanilla/26.3/generated/data/minecraft/enchantment");
        let mut names = Vec::new();
        let mut stack = vec![dir.clone()];
        while let Some(current) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&current) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "json")
                    && let Ok(relative) = path.strip_prefix(&dir)
                {
                    names.push(format!(
                        "minecraft:{}",
                        relative
                            .with_extension("")
                            .to_string_lossy()
                            .replace('\\', "/")
                    ));
                }
            }
        }
        names.sort();
        names
    });
    let wanted = if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    };
    ids.iter().position(|id| *id == wanted).map(|i| i as i32)
}

#[derive(Debug)]
pub struct VanillaEnchantTable;

fn enchantability(item: &ItemStack) -> Option<i32> {
    use ferrite_loot::LootHost;
    super::drops::ServerLootHost::default().enchantability(item.name()?)
}

/// `EnchantmentMenu.getEnchantmentList`: reseeds with `seed + slot`, selects, and drops one
/// random entry from a book's list.
fn list(
    rng: &mut JavaRandom,
    item: &ItemStack,
    slot: usize,
    cost: i32,
    seed: i32,
) -> Vec<(String, i32)> {
    use ferrite_loot::Rng;
    let (Some(tables), Some(value), Some(name)) =
        (super::drops::tables(), enchantability(item), item.name())
    else {
        return Vec::new();
    };
    rng.set_seed(i64::from(seed) + slot as i64);
    let mut picked = tables.enchanting_table_list(name, value, cost, rng);
    if name == "minecraft:book" && picked.len() > 1 {
        let drop = rng.next_int(picked.len() as i32) as usize;
        picked.remove(drop);
    }
    picked
}

impl EnchantTable for VanillaEnchantTable {
    fn offers(&self, item: &ItemStack, bookshelves: i32, seed: i32) -> [EnchantOffer; 3] {
        use ferrite_loot::Rng;
        let none = EnchantOffer {
            cost: 0,
            clue: -1,
            clue_level: -1,
        };
        let mut offers = [none; 3];
        let mut rng = JavaRandom::new(i64::from(seed));
        for (slot, offer) in offers.iter_mut().enumerate() {
            let value = cost(&mut rng, slot as i32, bookshelves);
            offer.cost = if value < slot as i32 + 1 { 0 } else { value };
        }
        for (slot, offer) in offers.iter_mut().enumerate() {
            if offer.cost <= 0 {
                continue;
            }
            let picked = list(&mut rng, item, slot, offer.cost, seed);
            if picked.is_empty() {
                continue;
            }
            let (name, level) = &picked[rng.next_int(picked.len() as i32) as usize];
            offer.clue = enchantment_id(name).unwrap_or(-1);
            offer.clue_level = *level;
        }
        offers
    }

    fn enchantments(
        &self,
        item: &ItemStack,
        slot: usize,
        cost: i32,
        seed: i32,
    ) -> Vec<(String, i32)> {
        list(&mut JavaRandom::new(0), item, slot, cost, seed)
    }
}

/// `EnchantingTableBlock.BOOKSHELF_OFFSETS`: the ring two blocks out, at table height and one
/// above, whose shelves count when the block between it and the table is clear.
pub fn bookshelf_offsets() -> Vec<[i32; 3]> {
    let mut out = Vec::new();
    for x in -2i32..=2 {
        for y in 0..=1 {
            for z in -2i32..=2 {
                if x.abs() == 2 || z.abs() == 2 {
                    out.push([x, y, z]);
                }
            }
        }
    }
    out
}

/// How many bookshelves power the table at `at` (`EnchantingTableBlock.isValidBookShelf`).
pub fn bookshelves(source: &super::anvil::AnvilChunkSource, at: [i32; 3]) -> i32 {
    let name = |pos: [i32; 3]| {
        source
            .block(pos[0], pos[1], pos[2])
            .and_then(|state| u16::try_from(state).ok())
            .and_then(ferrite_data::blocks::state)
            .map(|(name, _)| name)
    };
    bookshelf_offsets()
        .into_iter()
        .filter(|[x, y, z]| {
            name([at[0] + x, at[1] + y, at[2] + z]) == Some("minecraft:bookshelf")
                && name([at[0] + x / 2, at[1] + y, at[2] + z / 2]).is_some_and(|n| {
                    ferrite_data::tags::contains("block", "minecraft:replaceable", n)
                })
        })
        .count() as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_loot::Rng;

    #[test]
    fn java_random_matches_the_jdk() {
        // The same LCG worked out independently for seed 42 (bound 100, then bound 10).
        let mut rng = JavaRandom::new(42);
        assert_eq!(
            [rng.next_int(100), rng.next_int(100), rng.next_int(100)],
            [30, 63, 48]
        );
        let mut rng = JavaRandom::new(42);
        assert_eq!(
            [rng.next_int(10), rng.next_int(10), rng.next_int(10)],
            [0, 3, 8]
        );
    }

    #[test]
    fn fifteen_shelves_make_the_top_button_thirty() {
        let mut high = 0;
        for seed in 0..200 {
            let mut rng = JavaRandom::new(seed);
            high = high.max(cost(&mut rng, 2, 15));
            let mut rng = JavaRandom::new(seed);
            assert!(
                cost(&mut rng, 2, 15) >= 30,
                "slot three never falls below twice the shelves"
            );
        }
        assert_eq!(high, 30.max(high));
        let mut rng = JavaRandom::new(1);
        assert_eq!(
            cost(&mut rng, 0, 0).clamp(1, 3),
            cost(&mut JavaRandom::new(1), 0, 0)
        );
    }

    #[test]
    fn a_table_offers_real_enchantments_for_the_item() {
        let table = VanillaEnchantTable;
        let sword = ferrite_item::ItemStack::from_name("diamond_sword").unwrap();
        let offers = table.offers(&sword, 15, 12345);
        assert!(offers.iter().all(|o| o.cost >= 1), "{offers:?}");
        assert!(offers[2].cost >= 30, "fifteen shelves: {offers:?}");
        assert!(
            offers.iter().all(|o| o.clue >= 0 && o.clue_level >= 1),
            "{offers:?}"
        );
        let again = table.offers(&sword, 15, 12345);
        assert_eq!(offers, again, "the same seed gives the same buttons");
        let list = table.enchantments(&sword, 2, offers[2].cost, 12345);
        assert!(!list.is_empty());
        assert!(
            list.iter()
                .all(|(name, level)| name.starts_with("minecraft:") && *level >= 1)
        );
        // Without shelves the top button stays cheap.
        let bare = table.offers(&sword, 0, 12345);
        assert!(bare[2].cost <= 8 + 1 + 1, "{bare:?}");
        // A stick cannot be enchanted: the menu shows nothing (no `enchantable` component).
        let stick = ferrite_item::ItemStack::from_name("stick").unwrap();
        assert!(table.enchantments(&stick, 0, 10, 1).is_empty());
    }

    #[test]
    fn clue_ids_follow_the_synchronized_registry_order() {
        assert_eq!(enchantment_id("minecraft:aqua_affinity"), Some(0));
        assert_eq!(enchantment_id("aqua_affinity"), Some(0));
        assert!(enchantment_id("minecraft:sharpness") > enchantment_id("minecraft:power"));
        assert_eq!(enchantment_id("minecraft:nonsense"), None);
    }

    #[test]
    fn the_shelf_ring_has_thirty_two_spots() {
        assert_eq!(bookshelf_offsets().len(), 32);
    }
}
