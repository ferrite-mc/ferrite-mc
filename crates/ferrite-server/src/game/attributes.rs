//! Entity attributes for `/attribute`: base values and modifiers kept per entity.
//!
//! Defaults and ranges are the generic vanilla ones (`Attributes`); players override a few.
//! Mob-type specific defaults (a zombie's movement speed, say) are not modeled, and nothing in
//! the simulation reads these values yet, so they are queryable state, not gameplay changes.

use std::collections::BTreeMap;

/// Default, minimum and maximum of an attribute.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    pub default: f64,
    pub min: f64,
    pub max: f64,
}

const fn range(default: f64, min: f64, max: f64) -> Range {
    Range { default, min, max }
}

/// The generic attribute table: path under `minecraft:` and its range.
pub fn range_of(path: &str) -> Option<Range> {
    Some(match path {
        "air_drag_modifier" => range(1.0, 0.0, 2048.0),
        "armor" => range(0.0, 0.0, 30.0),
        "armor_toughness" => range(0.0, 0.0, 20.0),
        "attack_damage" => range(2.0, 0.0, 2048.0),
        "attack_knockback" => range(0.0, 0.0, 5.0),
        "attack_speed" => range(4.0, 0.0, 1024.0),
        "below_name_distance" => range(10.0, 0.0, 512.0),
        "block_break_speed" => range(1.0, 0.0, 1024.0),
        "block_interaction_range" => range(4.5, 0.0, 64.0),
        "bounciness" => range(0.0, 0.0, 1.0),
        "burning_time" => range(1.0, 0.0, 1024.0),
        "camera_distance" => range(4.0, 0.0, 32.0),
        "explosion_knockback_resistance" => range(0.0, 0.0, 1.0),
        "entity_interaction_range" => range(3.0, 0.0, 64.0),
        "fall_damage_multiplier" => range(1.0, 0.0, 100.0),
        "flying_speed" => range(0.4, 0.0, 1024.0),
        "follow_range" => range(32.0, 0.0, 2048.0),
        "friction_modifier" => range(1.0, 0.0, 2048.0),
        "gravity" => range(0.08, -1.0, 1.0),
        "jump_strength" => range(0.419_999_986_886_978_15, 0.0, 32.0),
        "knockback_resistance" => range(0.0, -2.0, 1.0),
        "luck" => range(0.0, -1024.0, 1024.0),
        "max_absorption" => range(0.0, 0.0, 2048.0),
        "max_health" => range(20.0, 1.0, 1024.0),
        "mining_efficiency" => range(0.0, 0.0, 1024.0),
        "movement_efficiency" => range(0.0, 0.0, 1.0),
        "movement_speed" => range(0.7, 0.0, 1024.0),
        "name_tag_distance" => range(64.0, 0.0, 512.0),
        "oxygen_bonus" => range(0.0, 0.0, 1024.0),
        "safe_fall_distance" => range(3.0, -1024.0, 1024.0),
        "scale" => range(1.0, 0.0625, 16.0),
        "sneaking_speed" => range(0.3, 0.0, 1.0),
        "spawn_reinforcements" => range(0.0, 0.0, 1.0),
        "step_height" => range(0.6, 0.0, 10.0),
        "submerged_mining_speed" => range(0.2, 0.0, 20.0),
        "sweeping_damage_ratio" => range(0.0, 0.0, 1.0),
        "tempt_range" => range(10.0, 0.0, 2048.0),
        "water_movement_efficiency" => range(0.0, 0.0, 1.0),
        "waypoint_transmit_range" => range(0.0, 0.0, 60_000_000.0),
        "waypoint_receive_range" => range(0.0, 0.0, 60_000_000.0),
        _ => return None,
    })
}

/// The default of `path` for a player (`Player.createAttributes`).
pub fn default_for(path: &str, player: bool) -> Option<Range> {
    let mut found = range_of(path)?;
    if player {
        match path {
            "attack_damage" => found.default = 1.0,
            "movement_speed" => found.default = 0.1,
            "waypoint_transmit_range" | "waypoint_receive_range" => found.default = 60_000_000.0,
            _ => {}
        }
    }
    Some(found)
}

/// The default of `path` for a non-player entity of protocol type id `kind`: the type's own
/// default attribute (`DefaultAttributes`). `None` when the type does not carry the attribute
/// at all, which includes every non-living type: vanilla answers `failed.no_attribute` there.
pub fn default_for_kind(path: &str, kind: u16) -> Option<Range> {
    let mut found = range_of(path)?;
    found.default = ferrite_data::entity_props::default_attribute(kind, path)?;
    Some(found)
}

/// The range `/attribute` uses for `target`: players use the player table, other entities their
/// own type's defaults.
pub fn limits_for(target: &super::selectors::SelectedEntity, path: &str) -> Option<Range> {
    match target.kind {
        Some(kind) if !target.player => default_for_kind(path, kind),
        _ => default_for(path, target.player),
    }
}

/// How a modifier combines with the base value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[allow(clippy::enum_variant_names)] // vanilla names: add_value, add_multiplied_*
pub enum Operation {
    AddValue,
    AddMultipliedBase,
    AddMultipliedTotal,
}

/// One entity's state for one attribute.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stored {
    pub base: Option<f64>,
    pub modifiers: BTreeMap<String, (f64, Operation)>,
}

impl Stored {
    /// The value vanilla computes: base, plus flat additions, plus base-relative multipliers,
    /// then every total multiplier, clamped to the attribute's range.
    pub fn value(&self, limits: Range, default: f64) -> f64 {
        let base = self.base.unwrap_or(default);
        let mut flat = base;
        let mut relative = 0.0;
        for (amount, op) in self.modifiers.values() {
            match op {
                Operation::AddValue => flat += amount,
                Operation::AddMultipliedBase => relative += base * amount,
                Operation::AddMultipliedTotal => {}
            }
        }
        let mut total = flat + relative;
        for (amount, op) in self.modifiers.values() {
            if *op == Operation::AddMultipliedTotal {
                total *= 1.0 + amount;
            }
        }
        total.clamp(limits.min, limits.max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_apply_in_vanilla_order_and_clamp() {
        let limits = range_of("movement_speed").unwrap();
        let mut stored = Stored {
            base: Some(1.0),
            ..Stored::default()
        };
        stored
            .modifiers
            .insert("a".into(), (1.0, Operation::AddValue));
        stored
            .modifiers
            .insert("b".into(), (0.5, Operation::AddMultipliedBase));
        stored
            .modifiers
            .insert("c".into(), (1.0, Operation::AddMultipliedTotal));
        // (1 + 1 + 1 * 0.5) * (1 + 1)
        assert_eq!(stored.value(limits, 0.7), 5.0);
        stored
            .modifiers
            .insert("d".into(), (5000.0, Operation::AddValue));
        assert_eq!(stored.value(limits, 0.7), 1024.0);
    }

    #[test]
    fn unset_base_uses_the_entity_default() {
        let player = default_for("movement_speed", true).unwrap();
        assert_eq!(Stored::default().value(player, player.default), 0.1);
        assert_eq!(default_for("max_health", false).unwrap().default, 20.0);
        assert!(range_of("minecraft:nope").is_none());
    }

    fn kind(name: &str) -> u16 {
        ferrite_data::entity_props::by_name(name).unwrap().id
    }

    #[test]
    fn mobs_use_their_own_type_defaults() {
        let cow = default_for_kind("max_health", kind("cow")).unwrap();
        assert_eq!(cow.default, 10.0);
        assert_eq!((cow.min, cow.max), (1.0, 1024.0));
        let zombie = default_for_kind("movement_speed", kind("zombie")).unwrap();
        assert_ne!(zombie.default, 0.7);
        assert_eq!(
            default_for_kind("attack_damage", kind("zombie"))
                .unwrap()
                .default,
            3.0
        );
    }

    #[test]
    fn types_without_the_attribute_have_none() {
        // A cow does not attack; items and armor stands are not living at all.
        assert!(default_for_kind("attack_damage", kind("cow")).is_none());
        assert!(default_for_kind("max_health", kind("item")).is_none());
        assert!(default_for_kind("minecraft:nope", kind("cow")).is_none());
    }
}
