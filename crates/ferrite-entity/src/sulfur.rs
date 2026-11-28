//! Sulfur cube contents (26.3 `SulfurCube`).
//!
//! A sulfur cube swallows one stack and takes the archetype of what it ate. This module owns the
//! part of that behaviour the server models: the explosion power the absorbed stack grants.

use ferrite_item::ItemStack;

/// A sulfur cube with no absorbed item explodes like a creeper (`power 2`).
pub const BASE_EXPLOSION_POWER: f32 = 2.0;
/// `#minecraft:sulfur_cube_archetype/explosive` (TNT): the cube explodes like TNT (`power 4`).
pub const EXPLOSIVE_EXPLOSION_POWER: f32 = 4.0;

/// The explosion power of a sulfur cube carrying `absorbed`.
pub fn explosion_power(absorbed: &[ItemStack]) -> f32 {
    let explosive = absorbed
        .iter()
        .filter_map(ItemStack::name)
        .any(|name| ferrite_data::tags::contains("item", "sulfur_cube_archetype/explosive", name));
    if explosive {
        EXPLOSIVE_EXPLOSION_POWER
    } else {
        BASE_EXPLOSION_POWER
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(name: &str) -> ItemStack {
        ItemStack::from_name(name).expect("a vanilla item")
    }

    #[test]
    fn an_absorbed_explosive_makes_the_cube_explode_like_tnt() {
        assert_eq!(explosion_power(&[]), BASE_EXPLOSION_POWER);
        assert_eq!(
            explosion_power(&[stack("minecraft:dirt")]),
            BASE_EXPLOSION_POWER
        );
        assert_eq!(
            explosion_power(&[stack("minecraft:tnt")]),
            EXPLOSIVE_EXPLOSION_POWER
        );
    }
}
