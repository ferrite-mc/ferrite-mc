//! Sulfur cube archetypes (26.3 `SulfurCubeArchetype` + `SulfurCubeArchetypes`).
//!
//! A sulfur cube swallows one stack and takes the archetype of what it ate. The archetype decides
//! the cube's movement attributes (`bounciness`, friction and air-drag multipliers), whether it
//! floats (`buoyant`), whether it explodes (`explosion`), contact damage (`hot`) and how much
//! knockback it resists. Values are read from the 26.3 data generator tags and the
//! `SulfurCubeArchetypes` bootstrap in the official server jar.
//!
//! The archetype is derived from [`EntityData::absorbed`](crate::store::EntityData), so it
//! survives region hand-offs and the entity save without extra state.

use ferrite_item::ItemStack;

/// `SulfurCubeArchetypes` keys, in the order the server registers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Archetype {
    Regular,
    Bouncy,
    SlowBouncy,
    SlowFlat,
    FastFlat,
    Light,
    FastSliding,
    SlowSliding,
    HighResistance,
    Sticky,
    Explosive,
    Hot,
}

impl Archetype {
    /// The tag under `#minecraft:sulfur_cube_archetype/` this archetype matches.
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Regular => "sulfur_cube_archetype/regular",
            Self::Bouncy => "sulfur_cube_archetype/bouncy",
            Self::SlowBouncy => "sulfur_cube_archetype/slow_bouncy",
            Self::SlowFlat => "sulfur_cube_archetype/slow_flat",
            Self::FastFlat => "sulfur_cube_archetype/fast_flat",
            Self::Light => "sulfur_cube_archetype/light",
            Self::FastSliding => "sulfur_cube_archetype/fast_sliding",
            Self::SlowSliding => "sulfur_cube_archetype/slow_sliding",
            Self::HighResistance => "sulfur_cube_archetype/high_resistance",
            Self::Sticky => "sulfur_cube_archetype/sticky",
            Self::Explosive => "sulfur_cube_archetype/explosive",
            Self::Hot => "sulfur_cube_archetype/hot",
        }
    }

    /// The archetype key without the tag prefix, e.g. `"slow_bouncy"`.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Regular => "regular",
            Self::Bouncy => "bouncy",
            Self::SlowBouncy => "slow_bouncy",
            Self::SlowFlat => "slow_flat",
            Self::FastFlat => "fast_flat",
            Self::Light => "light",
            Self::FastSliding => "fast_sliding",
            Self::SlowSliding => "slow_sliding",
            Self::HighResistance => "high_resistance",
            Self::Sticky => "sticky",
            Self::Explosive => "explosive",
            Self::Hot => "hot",
        }
    }
}

/// `SulfurCubeArchetype$ExplosionData`: power, whether it burns, and the fuse length.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExplosionData {
    pub power: f32,
    pub causes_fire: bool,
    pub fuse: i32,
}

/// The parts of `SulfurCubeArchetype` this server applies.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArchetypeData {
    pub archetype: Archetype,
    /// `KNOCKBACK_RESISTANCE`/`EXPLOSION_KNOCKBACK_RESISTANCE` (the negated bootstrap value,
    /// clamped by the attribute to 0..=1).
    pub knockback_resistance: f64,
    /// `BOUNCINESS`.
    pub bounciness: f64,
    /// `FRICTION_MODIFIER`, applied as `1 - (1 - block_friction) * modifier`.
    pub friction_multiplier: f64,
    /// `AIR_DRAG_MODIFIER`, applied as `1 - (1 - air_drag) * modifier`.
    pub air_drag_multiplier: f64,
    /// `buoyant`: the cube floats in water.
    pub buoyant: bool,
    /// `explosion`: only an explosive archetype can prime and explode.
    pub explosion: Option<ExplosionData>,
    /// `contact_damage` (`hot`): fire damage on contact.
    pub contact_damage: f32,
}

impl ArchetypeData {
    /// The registered data for `archetype`.
    pub const fn of(archetype: Archetype) -> Self {
        use Archetype::*;
        // The bootstrap passes (knockback resistance, bounciness, friction, air drag) to
        // `archetype()`; the first value is subtracted from the resistance attribute.
        let (knockback_resistance, bounciness, friction, air_drag, buoyant): (
            f64,
            f64,
            f64,
            f64,
            bool,
        ) = match archetype {
            Regular => (1.0, 0.5, 0.3, 0.1, true),
            Bouncy => (2.0, 0.9, 0.3, 0.01, true),
            SlowBouncy => (-0.4, 0.6, 0.3, 0.05, false),
            SlowFlat => (-0.5, 0.4, 0.4, 0.1, false),
            FastFlat => (1.0, 0.5, 0.2, 0.01, false),
            Light => (1.0, 1.0, 0.3, 1.8, true),
            FastSliding => (-0.5, 0.1, 0.05, 0.01, false),
            SlowSliding => (-0.8, 0.1, 0.05, 0.01, false),
            HighResistance => (-0.7, 0.2, 1.0, 0.01, false),
            Sticky => (2.0, 0.0, 2.0, 0.01, false),
            Explosive => (1.0, 0.5, 0.3, 0.3, true),
            Hot => (1.0, 0.5, 0.3, 0.1, true),
        };
        let explosion = match archetype {
            Explosive => Some(ExplosionData {
                power: 3.0,
                causes_fire: false,
                fuse: 120,
            }),
            _ => None,
        };
        let contact_damage = if matches!(archetype, Hot) { 1.0 } else { 0.0 };
        Self {
            archetype,
            knockback_resistance: (-knockback_resistance).clamp(0.0, 1.0),
            bounciness,
            friction_multiplier: friction,
            air_drag_multiplier: air_drag,
            buoyant,
            explosion,
            contact_damage,
        }
    }
}

/// Every registered archetype.
pub const ARCHETYPES: [Archetype; 12] = [
    Archetype::Regular,
    Archetype::Bouncy,
    Archetype::SlowBouncy,
    Archetype::SlowFlat,
    Archetype::FastFlat,
    Archetype::Light,
    Archetype::FastSliding,
    Archetype::SlowSliding,
    Archetype::HighResistance,
    Archetype::Sticky,
    Archetype::Explosive,
    Archetype::Hot,
];

/// The archetype an item belongs to, by the `#minecraft:sulfur_cube_archetype/*` tags.
pub fn archetype_for(item: &str) -> Option<Archetype> {
    ARCHETYPES
        .into_iter()
        .find(|archetype| ferrite_data::tags::contains("item", archetype.tag(), item))
}

/// The archetype of a cube carrying `absorbed`; the first stack decides, like
/// `SulfurCube.matchingArchetypes` (the cube holds one stack).
pub fn of_items(absorbed: &[ItemStack]) -> Option<ArchetypeData> {
    absorbed
        .iter()
        .filter_map(ItemStack::name)
        .find_map(archetype_for)
        .map(ArchetypeData::of)
}

/// The explosion the absorbed contents grant, if the cube can explode at all.
pub fn explosion(absorbed: &[ItemStack]) -> Option<ExplosionData> {
    of_items(absorbed).and_then(|data| data.explosion)
}

/// Vanilla `LivingEntity.computeModifiedFriction`:
/// `clamp(1 - (1 - base) * modifier, 0, 1)`.
pub fn modified_friction(base: f64, modifier: f64) -> f64 {
    (1.0 - (1.0 - base) * modifier).clamp(0.0, 1.0)
}

/// The block friction of a sulfur cube on `base` friction.
pub fn ground_friction(absorbed: &[ItemStack], base: f64) -> f64 {
    of_items(absorbed).map_or(base, |data| {
        modified_friction(base, data.friction_multiplier)
    })
}

/// The air drag of a sulfur cube on `base` drag.
pub fn air_drag(absorbed: &[ItemStack], base: f64) -> f64 {
    of_items(absorbed).map_or(base, |data| {
        modified_friction(base, data.air_drag_multiplier)
    })
}

/// The upward speed a buoyant archetype adds while submerged.
pub const BUOYANCY_LIFT: f64 = 0.04;

/// Whether the absorbed contents make the cube buoyant.
pub fn buoyant(absorbed: &[ItemStack]) -> bool {
    of_items(absorbed).is_some_and(|data| data.buoyant)
}

/// Whether the absorbed contents make the cube explode like TNT.
pub fn is_explosive(absorbed: &[ItemStack]) -> bool {
    explosion(absorbed).is_some()
}

/// How strongly a sulfur cube resists `mob::knockback` (0..=1).
pub fn knockback_resistance(absorbed: &[ItemStack]) -> f64 {
    of_items(absorbed).map_or(0.0, |data| data.knockback_resistance)
}

/// The bounciness of the absorbed archetype; 0 without one.
pub fn bounciness(absorbed: &[ItemStack]) -> f64 {
    of_items(absorbed).map_or(0.0, |data| data.bounciness)
}

/// The fire damage a `hot` cube deals to a body it touches.
pub fn contact_damage(absorbed: &[ItemStack]) -> f32 {
    of_items(absorbed).map_or(0.0, |data| data.contact_damage)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(name: &str) -> ItemStack {
        ItemStack::from_name(name).expect("a vanilla item")
    }

    #[test]
    fn every_archetype_tag_maps_to_its_registered_data() {
        assert_eq!(
            archetype_for("minecraft:oak_planks"),
            Some(Archetype::Bouncy)
        );
        assert_eq!(archetype_for("minecraft:oak_log"), Some(Archetype::Bouncy));
        assert_eq!(archetype_for("minecraft:tnt"), Some(Archetype::Explosive));
        assert_eq!(archetype_for("minecraft:magma_block"), Some(Archetype::Hot));
        assert_eq!(
            archetype_for("minecraft:honeycomb_block"),
            Some(Archetype::Sticky)
        );
        assert_eq!(
            archetype_for("minecraft:blue_ice"),
            Some(Archetype::FastSliding)
        );
        assert_eq!(
            archetype_for("minecraft:soul_sand"),
            Some(Archetype::HighResistance)
        );
        assert_eq!(
            archetype_for("minecraft:white_wool"),
            Some(Archetype::Light)
        );
        assert_eq!(archetype_for("minecraft:dirt"), Some(Archetype::Regular));
        assert_eq!(archetype_for("minecraft:diamond"), None);

        for archetype in ARCHETYPES {
            let data = ArchetypeData::of(archetype);
            assert_eq!(data.archetype, archetype);
            assert!((0.0..=1.0).contains(&data.knockback_resistance));
        }
    }

    #[test]
    fn only_the_explosive_archetype_explodes_with_tnt_power() {
        assert_eq!(explosion(&[]), None);
        assert_eq!(explosion(&[stack("minecraft:dirt")]), None);
        assert_eq!(
            explosion(&[stack("minecraft:tnt")]),
            Some(ExplosionData {
                power: 3.0,
                causes_fire: false,
                fuse: 120,
            })
        );
        assert!(is_explosive(&[stack("minecraft:tnt")]));
        assert!(!is_explosive(&[stack("minecraft:stone")]));
    }

    #[test]
    fn movement_modifiers_follow_compute_modified_friction() {
        assert!((modified_friction(0.6, 0.3) - 0.88).abs() < 1e-9);
        assert!((modified_friction(0.6, 2.0) - 0.2).abs() < 1e-9);
        assert!((modified_friction(0.6, 0.05) - 0.98).abs() < 1e-9);
        assert!((modified_friction(0.98, 1.8) - 0.964).abs() < 1e-9);
        assert_eq!(modified_friction(0.6, 10.0), 0.0);

        assert!((ground_friction(&[stack("minecraft:honeycomb_block")], 0.6) - 0.2).abs() < 1e-9);
        assert!((ground_friction(&[stack("minecraft:blue_ice")], 0.6) - 0.98).abs() < 1e-9);
        assert!((air_drag(&[stack("minecraft:white_wool")], 0.98) - 0.964).abs() < 1e-9);
        assert_eq!(ground_friction(&[], 0.6), 0.6);
        assert!(buoyant(&[stack("minecraft:oak_planks")]));
        assert!(!buoyant(&[stack("minecraft:soul_sand")]));
        assert_eq!(bounciness(&[stack("minecraft:white_wool")]), 1.0);
    }

    #[test]
    fn resistance_and_hot_contact_damage_come_from_the_archetype() {
        assert!((knockback_resistance(&[stack("minecraft:soul_sand")]) - 0.7).abs() < 1e-9);
        // The attribute clamps at zero: the bootstrap's -2 can only cancel the base value.
        assert_eq!(
            knockback_resistance(&[stack("minecraft:honeycomb_block")]),
            0.0
        );
        assert_eq!(knockback_resistance(&[stack("minecraft:tnt")]), 0.0);
        assert_eq!(contact_damage(&[stack("minecraft:magma_block")]), 1.0);
        assert_eq!(contact_damage(&[stack("minecraft:dirt")]), 0.0);
    }
}
