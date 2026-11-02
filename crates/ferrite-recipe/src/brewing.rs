//! Vanilla brewing (`net.minecraft.world.item.alchemy.PotionBrewing`).
//!
//! Brewing recipes are hard-coded in vanilla rather than shipped in the data pack, so the mixes
//! are listed here with their 26.3 potion and item ids. A mix either changes the potion in the
//! bottle (nether wart to awkward, redstone extends, glowstone amplifies, a fermented spider eye
//! inverts) or changes the bottle item (gunpowder to a splash potion, dragon's breath to a
//! lingering potion).
//!
//! Names are normalized like the rest of this crate: `sugar` and `minecraft:sugar` are the same
//! ingredient.

use crate::recipe::normalize_id;

/// One potion-to-potion mix: an ingredient turns `from` into `to`.
struct PotionMix {
    ingredient: &'static str,
    from: &'static str,
    to: &'static str,
}

/// One item-to-item mix: an ingredient turns `from` into `to`, keeping the potion.
struct ContainerMix {
    ingredient: &'static str,
    from: &'static str,
    to: &'static str,
}

/// The result of one brewing mix. `None` means the bottle keeps what it had.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Brewed {
    /// The new `minecraft:potion` id, when the mix changes the potion.
    pub potion: Option<&'static str>,
    /// The new bottle item, when the mix changes the container (splash, lingering).
    pub item: Option<&'static str>,
}

const fn m(ingredient: &'static str, from: &'static str, to: &'static str) -> PotionMix {
    PotionMix {
        ingredient,
        from,
        to,
    }
}

const fn c(ingredient: &'static str, from: &'static str, to: &'static str) -> ContainerMix {
    ContainerMix {
        ingredient,
        from,
        to,
    }
}

/// Vanilla `PotionBrewing.POTION_MIXES`, in the order the base game registers them.
const POTION_MIXES: &[PotionMix] = &[
    // Water into the four base potions (`Potions`).
    m(
        "minecraft:nether_wart",
        "minecraft:water",
        "minecraft:awkward",
    ),
    m(
        "minecraft:fermented_spider_eye",
        "minecraft:water",
        "minecraft:weakness",
    ),
    m(
        "minecraft:glowstone_dust",
        "minecraft:water",
        "minecraft:thick",
    ),
    // Water plus one of these is a mundane potion.
    m(
        "minecraft:spider_eye",
        "minecraft:water",
        "minecraft:mundane",
    ),
    m("minecraft:sugar", "minecraft:water", "minecraft:mundane"),
    m(
        "minecraft:blaze_powder",
        "minecraft:water",
        "minecraft:mundane",
    ),
    m(
        "minecraft:glistering_melon_slice",
        "minecraft:water",
        "minecraft:mundane",
    ),
    m(
        "minecraft:magma_cream",
        "minecraft:water",
        "minecraft:mundane",
    ),
    m(
        "minecraft:golden_carrot",
        "minecraft:water",
        "minecraft:mundane",
    ),
    m(
        "minecraft:rabbit_foot",
        "minecraft:water",
        "minecraft:mundane",
    ),
    m(
        "minecraft:pufferfish",
        "minecraft:water",
        "minecraft:mundane",
    ),
    // Awkward into the effect potions.
    m(
        "minecraft:golden_carrot",
        "minecraft:awkward",
        "minecraft:night_vision",
    ),
    m(
        "minecraft:magma_cream",
        "minecraft:awkward",
        "minecraft:fire_resistance",
    ),
    m(
        "minecraft:blaze_powder",
        "minecraft:awkward",
        "minecraft:strength",
    ),
    m(
        "minecraft:sugar",
        "minecraft:awkward",
        "minecraft:swiftness",
    ),
    m(
        "minecraft:rabbit_foot",
        "minecraft:awkward",
        "minecraft:leaping",
    ),
    m(
        "minecraft:glistering_melon_slice",
        "minecraft:awkward",
        "minecraft:healing",
    ),
    m(
        "minecraft:spider_eye",
        "minecraft:awkward",
        "minecraft:poison",
    ),
    m(
        "minecraft:pufferfish",
        "minecraft:awkward",
        "minecraft:water_breathing",
    ),
    m(
        "minecraft:turtle_helmet",
        "minecraft:awkward",
        "minecraft:turtle_master",
    ),
    m(
        "minecraft:phantom_membrane",
        "minecraft:awkward",
        "minecraft:slow_falling",
    ),
    m(
        "minecraft:wind_charge",
        "minecraft:awkward",
        "minecraft:wind_charged",
    ),
    m("minecraft:cobweb", "minecraft:awkward", "minecraft:weaving"),
    m(
        "minecraft:slime_block",
        "minecraft:awkward",
        "minecraft:oozing",
    ),
    m("minecraft:stone", "minecraft:awkward", "minecraft:infested"),
    // Redstone extends the effect.
    m(
        "minecraft:redstone",
        "minecraft:night_vision",
        "minecraft:long_night_vision",
    ),
    m(
        "minecraft:redstone",
        "minecraft:invisibility",
        "minecraft:long_invisibility",
    ),
    m(
        "minecraft:redstone",
        "minecraft:leaping",
        "minecraft:long_leaping",
    ),
    m(
        "minecraft:redstone",
        "minecraft:fire_resistance",
        "minecraft:long_fire_resistance",
    ),
    m(
        "minecraft:redstone",
        "minecraft:swiftness",
        "minecraft:long_swiftness",
    ),
    m(
        "minecraft:redstone",
        "minecraft:slowness",
        "minecraft:long_slowness",
    ),
    m(
        "minecraft:redstone",
        "minecraft:water_breathing",
        "minecraft:long_water_breathing",
    ),
    m(
        "minecraft:redstone",
        "minecraft:poison",
        "minecraft:long_poison",
    ),
    m(
        "minecraft:redstone",
        "minecraft:regeneration",
        "minecraft:long_regeneration",
    ),
    m(
        "minecraft:redstone",
        "minecraft:strength",
        "minecraft:long_strength",
    ),
    m(
        "minecraft:redstone",
        "minecraft:weakness",
        "minecraft:long_weakness",
    ),
    m(
        "minecraft:redstone",
        "minecraft:slow_falling",
        "minecraft:long_slow_falling",
    ),
    m(
        "minecraft:redstone",
        "minecraft:turtle_master",
        "minecraft:long_turtle_master",
    ),
    // Glowstone strengthens the effect.
    m(
        "minecraft:glowstone_dust",
        "minecraft:leaping",
        "minecraft:strong_leaping",
    ),
    m(
        "minecraft:glowstone_dust",
        "minecraft:swiftness",
        "minecraft:strong_swiftness",
    ),
    m(
        "minecraft:glowstone_dust",
        "minecraft:slowness",
        "minecraft:strong_slowness",
    ),
    m(
        "minecraft:glowstone_dust",
        "minecraft:healing",
        "minecraft:strong_healing",
    ),
    m(
        "minecraft:glowstone_dust",
        "minecraft:harming",
        "minecraft:strong_harming",
    ),
    m(
        "minecraft:glowstone_dust",
        "minecraft:poison",
        "minecraft:strong_poison",
    ),
    m(
        "minecraft:glowstone_dust",
        "minecraft:regeneration",
        "minecraft:strong_regeneration",
    ),
    m(
        "minecraft:glowstone_dust",
        "minecraft:strength",
        "minecraft:strong_strength",
    ),
    m(
        "minecraft:glowstone_dust",
        "minecraft:turtle_master",
        "minecraft:strong_turtle_master",
    ),
    // A fermented spider eye inverts the effect.
    m(
        "minecraft:fermented_spider_eye",
        "minecraft:swiftness",
        "minecraft:slowness",
    ),
    m(
        "minecraft:fermented_spider_eye",
        "minecraft:long_swiftness",
        "minecraft:long_slowness",
    ),
    m(
        "minecraft:fermented_spider_eye",
        "minecraft:strong_swiftness",
        "minecraft:strong_slowness",
    ),
    m(
        "minecraft:fermented_spider_eye",
        "minecraft:leaping",
        "minecraft:slowness",
    ),
    m(
        "minecraft:fermented_spider_eye",
        "minecraft:long_leaping",
        "minecraft:long_slowness",
    ),
    m(
        "minecraft:fermented_spider_eye",
        "minecraft:strong_leaping",
        "minecraft:strong_slowness",
    ),
    m(
        "minecraft:fermented_spider_eye",
        "minecraft:healing",
        "minecraft:harming",
    ),
    m(
        "minecraft:fermented_spider_eye",
        "minecraft:strong_healing",
        "minecraft:strong_harming",
    ),
    m(
        "minecraft:fermented_spider_eye",
        "minecraft:poison",
        "minecraft:harming",
    ),
    m(
        "minecraft:fermented_spider_eye",
        "minecraft:long_poison",
        "minecraft:harming",
    ),
    m(
        "minecraft:fermented_spider_eye",
        "minecraft:strong_poison",
        "minecraft:strong_harming",
    ),
    m(
        "minecraft:fermented_spider_eye",
        "minecraft:night_vision",
        "minecraft:invisibility",
    ),
    m(
        "minecraft:fermented_spider_eye",
        "minecraft:long_night_vision",
        "minecraft:long_invisibility",
    ),
];

/// Vanilla `PotionBrewing.CONTAINER_MIXES`: gunpowder makes a splash potion, dragon's breath a
/// lingering one.
const CONTAINER_MIXES: &[ContainerMix] = &[
    c(
        "minecraft:gunpowder",
        "minecraft:potion",
        "minecraft:splash_potion",
    ),
    c(
        "minecraft:dragon_breath",
        "minecraft:splash_potion",
        "minecraft:lingering_potion",
    ),
];

/// The mix an ingredient performs on a bottle, or `None` when the two do not brew.
///
/// `item` is the bottle item (`minecraft:potion`, `minecraft:splash_potion`, ...) and `potion`
/// the `minecraft:potion` id from its `minecraft:potion_contents` component, if any.
pub fn mix(ingredient: &str, item: &str, potion: Option<&str>) -> Option<Brewed> {
    let ingredient = normalize_id(ingredient);
    let item = normalize_id(item);
    if let Some(container) = CONTAINER_MIXES
        .iter()
        .find(|mix| mix.ingredient == ingredient.as_ref() && mix.from == item.as_ref())
    {
        return Some(Brewed {
            potion: None,
            item: Some(container.to),
        });
    }
    let potion = normalize_id(potion?);
    POTION_MIXES
        .iter()
        .find(|mix| mix.ingredient == ingredient.as_ref() && mix.from == potion.as_ref())
        .map(|mix| Brewed {
            potion: Some(mix.to),
            item: None,
        })
}

/// Whether an item is an ingredient of any brewing mix (vanilla
/// `PotionBrewing.isIngredient`).
pub fn is_ingredient(item: &str) -> bool {
    let item = normalize_id(item);
    POTION_MIXES
        .iter()
        .any(|mix| mix.ingredient == item.as_ref())
        || CONTAINER_MIXES
            .iter()
            .any(|mix| mix.ingredient == item.as_ref())
}

/// Every potion a mix can produce, for tests and the recipe book.
pub fn products() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = POTION_MIXES.iter().map(|mix| mix.to).collect();
    out.extend(CONTAINER_MIXES.iter().map(|mix| mix.to));
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brew(ingredient: &str, item: &str, potion: &str) -> Brewed {
        mix(ingredient, item, Some(potion))
            .unwrap_or_else(|| panic!("{potion} + {ingredient} should brew"))
    }

    #[test]
    fn water_brews_the_base_potions_and_awkward_brews_the_effects() {
        assert_eq!(
            brew(
                "minecraft:nether_wart",
                "minecraft:potion",
                "minecraft:water"
            )
            .potion,
            Some("minecraft:awkward")
        );
        assert_eq!(
            brew(
                "minecraft:golden_carrot",
                "minecraft:potion",
                "minecraft:awkward"
            )
            .potion,
            Some("minecraft:night_vision")
        );
        assert_eq!(
            brew("minecraft:sugar", "minecraft:potion", "minecraft:awkward").potion,
            Some("minecraft:swiftness")
        );
        assert_eq!(
            brew("minecraft:stone", "minecraft:potion", "minecraft:awkward").potion,
            Some("minecraft:infested")
        );
        assert_eq!(
            mix(
                "minecraft:glowstone_dust",
                "minecraft:potion",
                Some("minecraft:night_vision")
            ),
            None,
            "night vision has no stronger variant"
        );
        assert_eq!(
            mix(
                "minecraft:sugar",
                "minecraft:potion",
                Some("minecraft:mundane")
            ),
            None,
            "a mundane potion has no further mixes"
        );
    }

    #[test]
    fn redstone_extends_glowstone_amplifies_and_spider_eyes_invert() {
        assert_eq!(
            brew(
                "minecraft:redstone",
                "minecraft:potion",
                "minecraft:swiftness"
            )
            .potion,
            Some("minecraft:long_swiftness")
        );
        assert_eq!(
            brew(
                "minecraft:glowstone_dust",
                "minecraft:potion",
                "minecraft:swiftness"
            )
            .potion,
            Some("minecraft:strong_swiftness")
        );
        assert_eq!(
            brew(
                "minecraft:fermented_spider_eye",
                "minecraft:potion",
                "minecraft:strong_swiftness"
            )
            .potion,
            Some("minecraft:strong_slowness")
        );
        assert_eq!(
            brew(
                "minecraft:fermented_spider_eye",
                "minecraft:potion",
                "minecraft:long_night_vision"
            )
            .potion,
            Some("minecraft:long_invisibility")
        );
        // Long and strong variants of the same effect do not extend each other.
        assert_eq!(
            mix(
                "minecraft:redstone",
                "minecraft:potion",
                Some("minecraft:long_swiftness")
            ),
            None
        );
        assert_eq!(
            mix(
                "minecraft:glowstone_dust",
                "minecraft:potion",
                Some("minecraft:strong_swiftness")
            ),
            None
        );
    }

    #[test]
    fn gunpowder_and_dragon_breath_change_the_bottle() {
        assert_eq!(
            brew(
                "minecraft:gunpowder",
                "minecraft:potion",
                "minecraft:swiftness"
            ),
            Brewed {
                potion: None,
                item: Some("minecraft:splash_potion")
            }
        );
        assert_eq!(
            brew(
                "minecraft:dragon_breath",
                "minecraft:splash_potion",
                "minecraft:swiftness"
            ),
            Brewed {
                potion: None,
                item: Some("minecraft:lingering_potion")
            }
        );
        // Dragon's breath does nothing to a regular potion, gunpowder nothing to a splash one.
        assert_eq!(
            mix(
                "minecraft:dragon_breath",
                "minecraft:potion",
                Some("minecraft:swiftness")
            ),
            None
        );
        assert_eq!(
            mix(
                "minecraft:gunpowder",
                "minecraft:splash_potion",
                Some("minecraft:swiftness")
            ),
            None
        );
    }

    #[test]
    fn ingredient_names_are_namespaced_but_optional_and_ingredients_are_recognised() {
        assert!(mix("sugar", "potion", Some("awkward")).is_some());
        assert!(is_ingredient("minecraft:nether_wart"));
        assert!(is_ingredient("gunpowder"));
        assert!(is_ingredient("dragon_breath"));
        assert!(!is_ingredient("minecraft:diamond"));
        assert!(!is_ingredient("minecraft:glass_bottle"));
        // Without a potion id only the container mixes apply.
        assert!(mix("minecraft:gunpowder", "minecraft:potion", None).is_some());
        assert_eq!(mix("minecraft:nether_wart", "minecraft:potion", None), None);
        assert!(products().contains(&"minecraft:lingering_potion"));
        assert!(products().contains(&"minecraft:infested"));
    }
}
