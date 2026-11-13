//! Filled-map helpers: the `minecraft:map_id` identity and the transient
//! `minecraft:map_post_processing` marker a cartography table puts on its result.

use crate::{Component, ItemStack, component_ids as ids};

/// Vanilla `MapPostProcessing`, the work still to do when a cartography result is taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapPostProcessing {
    /// Lock the map: the taker gets a new id holding a locked copy.
    Lock,
    /// Zoom the map out: the taker gets a new id holding a blank, larger map.
    Scale,
}

impl MapPostProcessing {
    fn id(self) -> u8 {
        match self {
            Self::Lock => 0,
            Self::Scale => 1,
        }
    }
}

/// The saved-map id of a filled map, or `None` for any other stack.
pub fn map_id(stack: &ItemStack) -> Option<i32> {
    // Only the stack's own patch can carry a map id: no item has one among its defaults (pinned
    // by `no_item_has_a_default_map_id`). Going through `effective_component` instead decoded the
    // item's whole default table from NBT on every call, ~10 us for an item without a map id,
    // and this runs for both held stacks of every player on every tick.
    match stack.components.get(ids::MAP_ID)? {
        Component::MapId(id) => Some(*id),
        _ => None,
    }
}

/// Points a stack at saved map `id`.
pub fn set_map_id(stack: &mut ItemStack, id: i32) {
    stack.components.insert(ids::MAP_ID, Component::MapId(id));
}

/// The pending post-processing on a stack. The component is network-only (vanilla has no save
/// codec for it), so it is kept as its one-byte wire payload.
pub fn map_post_processing(stack: &ItemStack) -> Option<MapPostProcessing> {
    match stack.components.get(ids::MAP_POST_PROCESSING)? {
        Component::Raw(bytes) => match bytes.as_slice() {
            [0] => Some(MapPostProcessing::Lock),
            [1] => Some(MapPostProcessing::Scale),
            _ => None,
        },
        _ => None,
    }
}

/// Sets or clears the pending post-processing.
pub fn set_map_post_processing(stack: &mut ItemStack, work: Option<MapPostProcessing>) {
    match work {
        Some(work) => {
            stack
                .components
                .insert(ids::MAP_POST_PROCESSING, Component::Raw(vec![work.id()]));
        }
        None => {
            stack.components.remove(ids::MAP_POST_PROCESSING);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(id: i32) -> ItemStack {
        let mut stack = ItemStack::from_name("minecraft:filled_map").unwrap();
        set_map_id(&mut stack, id);
        stack
    }

    #[test]
    fn map_id_round_trips_and_is_absent_on_other_items() {
        assert_eq!(map_id(&filled(7)), Some(7));
        assert_eq!(
            map_id(&ItemStack::from_name("minecraft:map").unwrap()),
            None
        );
        assert_eq!(map_id(&ItemStack::empty()), None);
    }

    #[test]
    fn no_item_has_a_default_map_id() {
        for item in 0..crate::item_count() {
            let item = u16::try_from(item).unwrap();
            let defaults = crate::default_components(item).unwrap();
            assert!(
                !defaults.contains_key(&ids::MAP_ID),
                "item {item} would have a map id by default"
            );
        }
    }

    #[test]
    fn a_removed_or_absent_map_id_reads_as_none() {
        let mut stack = filled(5);
        assert_eq!(map_id(&stack), Some(5));
        stack.components.remove(ids::MAP_ID);
        assert_eq!(map_id(&stack), None);
    }

    /// Repeatable measurement behind the `send_held_maps` change (not a pass/fail test):
    /// `cargo test --release -p ferrite-item map_id_read_cost -- --ignored --nocapture`.
    #[test]
    #[ignore = "benchmark; run explicitly in release mode"]
    fn map_id_read_cost() {
        use std::hint::black_box;
        use std::time::Instant;
        const N: u32 = 200_000;
        for (label, stack) in [
            ("filled map", filled(7)),
            (
                "plain item",
                ItemStack::from_name("minecraft:diamond_sword").unwrap(),
            ),
        ] {
            // Before: `send_held_maps` cloned the stack and read the id with `effective_component`.
            let start = Instant::now();
            for _ in 0..N {
                let copy = black_box(&stack).clone();
                black_box(copy.effective_component(ids::MAP_ID));
            }
            let before = start.elapsed();
            // After: `map_id` reads the stack's own patch by reference.
            let start = Instant::now();
            for _ in 0..N {
                black_box(map_id(black_box(&stack)));
            }
            let after = start.elapsed();
            println!(
                "{label}: before (clone + effective_component) {:.1} ns, after (map_id by reference) {:.1} ns per call",
                before.as_nanos() as f64 / f64::from(N),
                after.as_nanos() as f64 / f64::from(N),
            );
        }
    }

    #[test]
    fn map_id_survives_the_nbt_save_format() {
        let stack = filled(300);
        let back = ItemStack::from_nbt(&stack.to_nbt().unwrap()).unwrap();
        assert_eq!(map_id(&back), Some(300));
    }

    #[test]
    fn post_processing_is_set_read_and_cleared() {
        let mut stack = filled(1);
        assert_eq!(map_post_processing(&stack), None);
        set_map_post_processing(&mut stack, Some(MapPostProcessing::Scale));
        assert_eq!(map_post_processing(&stack), Some(MapPostProcessing::Scale));
        set_map_post_processing(&mut stack, Some(MapPostProcessing::Lock));
        assert_eq!(map_post_processing(&stack), Some(MapPostProcessing::Lock));
        set_map_post_processing(&mut stack, None);
        assert_eq!(map_post_processing(&stack), None);
    }

    #[test]
    fn post_processing_marker_survives_the_wire() {
        let mut stack = filled(2);
        set_map_post_processing(&mut stack, Some(MapPostProcessing::Lock));
        let bytes = stack.encode_network().unwrap();
        let back = ItemStack::decode_network(&bytes).unwrap();
        assert_eq!(map_id(&back), Some(2));
        assert_eq!(map_post_processing(&back), Some(MapPostProcessing::Lock));
    }
}
