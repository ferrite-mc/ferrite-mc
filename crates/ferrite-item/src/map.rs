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
    match stack.effective_component(ids::MAP_ID)? {
        Component::MapId(id) => Some(id),
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
