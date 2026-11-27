//! Vanilla 26.3 `ComposterBlock` and the `minecraft:compostable` item component.
//!
//! The item report gives each compostable item a layer provider
//! (`minecraft:compostable/{low,low_medium,medium,medium_high,always_add_one}`). Vanilla
//! resolves the provider against the current composter state: at level 0 the layer always
//! succeeds, otherwise the provider's weighted list decides. Reaching level 7 schedules the
//! 20-tick step to level 8 (`READY`); using a full composter extracts one bone meal and resets
//! it to level 0.

use ferrite_data::blocks;
use ferrite_entity::Rng;
use ferrite_item::{Component, ItemStack};
use ferrite_nbt::Tag;

/// Vanilla `ComposterBlock.READY`.
pub(super) const READY: u8 = 8;
/// Vanilla `ComposterBlock.MAX_LEVEL`, the last fillable level before the ready step.
pub(super) const MAX_LEVEL: u8 = 7;

/// What a right click did to the composter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ComposterUse {
    /// The held compostable item was consumed. `state` is the block state to write; when the
    /// layer roll failed it equals the state passed in. `ready` schedules the 20-tick step.
    Filled { state: u16, ready: bool },
    /// A full composter was used: one bone meal drops and the block resets to level 0.
    Harvest,
}

fn property(state: u16, key: &str) -> Option<&'static str> {
    blocks::state(state)?
        .1
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, value)| *value)
}

/// A copy of `state` with `level` replaced.
fn with_level(state: u16, level: u8) -> Option<u16> {
    let value = level.to_string();
    let (name, props) = blocks::state(state)?;
    let mut properties: Vec<(&str, &str)> =
        props.iter().map(|&(key, value)| (key, value)).collect();
    properties.iter_mut().find(|(key, _)| *key == "level")?.1 = value.as_str();
    blocks::state_id(name, &properties)
}

/// The success percentage of a compostable stack's `layers` provider, from the 26.3 item
/// report: `low` 30, `low_medium` 50, `medium` 65, `medium_high` 85, `always_add_one` 100.
fn layer_chance(stack: &ItemStack) -> Option<u8> {
    let component = stack.effective_component(ferrite_item::component_ids::COMPOSTABLE)?;
    let Component::Opaque(Tag::Compound(component)) = component else {
        return None;
    };
    let Some(Tag::String(name)) = component.get("layers") else {
        return None;
    };
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    match name {
        "compostable/always_add_one" => Some(100),
        "compostable/low" => Some(30),
        "compostable/low_medium" => Some(50),
        "compostable/medium" => Some(65),
        "compostable/medium_high" => Some(85),
        _ => None,
    }
}

/// Vanilla `Compostable.layers` for this composter level: the number of layers added (0 or 1).
///
/// Level 0 always accepts the first layer via the provider's `match_block` case. Otherwise the
/// provider's weighted list decides.
fn layer_roll(level: u8, chance: u8, rng: &mut Rng) -> u8 {
    if level == 0 {
        return 1;
    }
    u8::from(rng.next_int(100) < i32::from(chance))
}

/// Vanilla `ComposterBlock.useItemOn` followed by `useWithoutItem`. `None` means the click is
/// not a composter interaction, so item placement and the other block handlers continue.
pub(super) fn use_on(state: u16, held: &ItemStack, rng: &mut Rng) -> Option<ComposterUse> {
    let (name, _) = blocks::state(state)?;
    if name != "minecraft:composter" {
        return None;
    }
    let level = property(state, "level")?.parse::<u8>().ok()?;
    if level >= READY {
        return Some(ComposterUse::Harvest);
    }
    let chance = layer_chance(held)?;
    if level >= MAX_LEVEL {
        // Vanilla awards the stat and consumes the item at level 7 without adding a layer.
        return Some(ComposterUse::Filled {
            state,
            ready: false,
        });
    }
    if layer_roll(level, chance, rng) == 0 {
        return Some(ComposterUse::Filled {
            state,
            ready: false,
        });
    }
    let next = with_level(state, level + 1)?;
    Some(ComposterUse::Filled {
        state: next,
        ready: level + 1 == MAX_LEVEL,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(level: u8) -> u16 {
        let value = level.to_string();
        blocks::state_id("composter", &[("level", &value)]).expect("vanilla composter state")
    }

    fn stack(name: &str) -> ItemStack {
        ItemStack::from_name(name).expect("vanilla item")
    }

    #[test]
    fn item_components_pick_the_vanilla_layer_provider() {
        assert_eq!(layer_chance(&stack("short_grass")), Some(30));
        assert_eq!(layer_chance(&stack("melon_slice")), Some(50));
        assert_eq!(layer_chance(&stack("apple")), Some(65));
        assert_eq!(layer_chance(&stack("hay_block")), Some(85));
        assert_eq!(layer_chance(&stack("pumpkin_pie")), Some(100));
        assert_eq!(layer_chance(&stack("stone")), None);
    }

    #[test]
    fn level_zero_always_accepts_the_first_layer() {
        let mut rng = Rng::new(1);
        let outcome = use_on(state(0), &stack("wheat_seeds"), &mut rng).unwrap();
        assert_eq!(
            outcome,
            ComposterUse::Filled {
                state: state(1),
                ready: false
            }
        );
    }

    #[test]
    fn a_successful_layer_at_six_makes_the_composter_ready_at_seven() {
        let mut rng = Rng::new(1);
        let outcome = use_on(state(6), &stack("pumpkin_pie"), &mut rng).unwrap();
        assert_eq!(
            outcome,
            ComposterUse::Filled {
                state: state(7),
                ready: true
            }
        );
    }

    #[test]
    fn level_seven_consumes_the_item_without_a_layer() {
        let mut rng = Rng::new(1);
        assert_eq!(
            use_on(state(7), &stack("pumpkin_pie"), &mut rng),
            Some(ComposterUse::Filled {
                state: state(7),
                ready: false
            })
        );
    }

    #[test]
    fn a_full_composter_harvests_bone_meal() {
        let mut rng = Rng::new(1);
        assert_eq!(
            use_on(state(8), &stack("stone"), &mut rng),
            Some(ComposterUse::Harvest)
        );
    }

    #[test]
    fn a_non_compostable_item_passes_through() {
        let mut rng = Rng::new(1);
        assert_eq!(use_on(state(3), &stack("stone"), &mut rng), None);
    }

    #[test]
    fn weighted_layers_follow_the_vanilla_percentages() {
        let mut rng = Rng::new(7);
        let hits = (0..10_000)
            .filter(|_| layer_roll(3, 30, &mut rng) == 1)
            .count();
        assert!(
            (2_700..3_300).contains(&hits),
            "low should succeed about 30% of the time, got {hits}"
        );
        let hits = (0..10_000)
            .filter(|_| layer_roll(3, 85, &mut rng) == 1)
            .count();
        assert!(
            (8_200..8_800).contains(&hits),
            "medium_high should succeed about 85% of the time, got {hits}"
        );
    }
}
