//! End portal frames: placing an eye of ender in a frame and lighting the portal.
//!
//! A frame faces the player who placed it, so the frames of a finished ring face away from the
//! centre. Twelve frames with an eye, three per side around a 3x3 interior, light the interior
//! as `end_portal` blocks.

use ferrite_data::blocks::{state, state_id};

const FRAME: &str = "minecraft:end_portal_frame";
const PORTAL: &str = "minecraft:end_portal";

/// The twelve frame positions around a ring centre, with the facing each must have.
const RING: [([i32; 2], &str); 12] = [
    ([-1, -2], "north"),
    ([0, -2], "north"),
    ([1, -2], "north"),
    ([-1, 2], "south"),
    ([0, 2], "south"),
    ([1, 2], "south"),
    ([-2, -1], "west"),
    ([-2, 0], "west"),
    ([-2, 1], "west"),
    ([2, -1], "east"),
    ([2, 0], "east"),
    ([2, 1], "east"),
];

/// The facing of an empty frame state, or `None` when the state is not an empty frame.
pub fn empty_frame_facing(frame: u16) -> Option<&'static str> {
    let (name, props) = state(frame)?;
    if name != FRAME || !props.contains(&("eye", "false")) {
        return None;
    }
    props
        .iter()
        .find(|(key, _)| *key == "facing")
        .map(|(_, value)| *value)
}

/// The same frame with an eye in it.
pub fn with_eye(frame: u16) -> Option<u16> {
    let facing = empty_frame_facing(frame)?;
    state_id(FRAME, &[("eye", "true"), ("facing", facing)])
}

/// Whether `block` is a frame holding an eye and facing `facing`.
fn filled_facing(block: Option<u16>, facing: &str) -> bool {
    let Some((name, props)) = block.and_then(state) else {
        return false;
    };
    name == FRAME && props.contains(&("eye", "true")) && props.contains(&("facing", facing))
}

/// The centre of the complete ring that `frame` belongs to, if the ring is now complete.
/// `block(x, y, z)` reads the world.
pub fn complete_ring(
    block: &dyn Fn(i32, i32, i32) -> Option<u16>,
    frame: [i32; 3],
) -> Option<[i32; 3]> {
    RING.iter().find_map(|([dx, dz], _)| {
        let center = [frame[0] - dx, frame[1], frame[2] - dz];
        RING.iter()
            .all(|([rx, rz], facing)| {
                filled_facing(block(center[0] + rx, center[1], center[2] + rz), facing)
            })
            .then_some(center)
    })
}

/// The nine interior blocks of a ring and the state they become.
pub fn portal_blocks(center: [i32; 3]) -> Option<Vec<([i32; 3], u16)>> {
    let portal = ferrite_data::blocks::default_state(PORTAL)?;
    let mut blocks = Vec::with_capacity(9);
    for dx in -1..=1 {
        for dz in -1..=1 {
            blocks.push(([center[0] + dx, center[1], center[2] + dz], portal));
        }
    }
    Some(blocks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn frame(eye: bool, facing: &str) -> u16 {
        state_id(
            FRAME,
            &[
                ("eye", if eye { "true" } else { "false" }),
                ("facing", facing),
            ],
        )
        .unwrap()
    }

    fn ring(center: [i32; 3], eyes: bool) -> HashMap<[i32; 3], u16> {
        RING.iter()
            .map(|([dx, dz], facing)| {
                (
                    [center[0] + dx, center[1], center[2] + dz],
                    frame(eyes, facing),
                )
            })
            .collect()
    }

    #[test]
    fn an_empty_frame_takes_an_eye_and_keeps_its_facing() {
        let empty = frame(false, "west");
        assert_eq!(empty_frame_facing(empty), Some("west"));
        assert_eq!(with_eye(empty), Some(frame(true, "west")));
        assert_eq!(
            empty_frame_facing(frame(true, "west")),
            None,
            "already has an eye"
        );
        assert_eq!(with_eye(0), None);
    }

    #[test]
    fn twelve_eyes_facing_outward_light_the_portal() {
        let center = [10, 40, -6];
        let world = ring(center, true);
        let read = |x, y, z| world.get(&[x, y, z]).copied();
        // Any of the twelve frames finds the same centre.
        for ([dx, dz], _) in RING {
            assert_eq!(
                complete_ring(&read, [center[0] + dx, center[1], center[2] + dz]),
                Some(center)
            );
        }
        let blocks = portal_blocks(center).unwrap();
        assert_eq!(blocks.len(), 9);
        assert!(
            blocks
                .iter()
                .all(|(_, s)| crate::game::portal::is_end_portal(*s))
        );
        assert!(blocks.iter().any(|(at, _)| *at == center));
    }

    #[test]
    fn a_missing_eye_or_a_wrong_facing_keeps_it_dark() {
        let center = [0, 64, 0];
        let mut world = ring(center, true);
        world.insert([1, 64, -2], frame(false, "north"));
        let read = |x, y, z| world.get(&[x, y, z]).copied();
        assert_eq!(
            complete_ring(&read, [0, 64, -2]),
            None,
            "an empty frame leaves a gap"
        );

        let mut turned = ring(center, true);
        turned.insert([0, 64, 2], frame(true, "north"));
        let read = |x, y, z| turned.get(&[x, y, z]).copied();
        assert_eq!(
            complete_ring(&read, [0, 64, 2]),
            None,
            "frames must face away from the centre"
        );

        let missing: HashMap<[i32; 3], u16> = ring(center, true).into_iter().skip(1).collect();
        let read = |x, y, z| missing.get(&[x, y, z]).copied();
        assert_eq!(complete_ring(&read, [0, 64, 2]), None);
    }
}
