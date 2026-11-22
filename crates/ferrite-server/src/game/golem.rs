//! Vanilla `CarvedPumpkinBlock` golem construction (26.3 jar).
//!
//! The jar builds both patterns with `BlockPatternBuilder` and matches them around the placed
//! pumpkin. Its first aisle is the top row, so the patterns are:
//!
//! * snow golem: `"^"`, `"#"`, `"#"` (`#` = `minecraft:snow_block`, `^` = carved pumpkin or
//!   jack o' lantern);
//! * iron golem: `"~^~"`, `"###"`, `"~#~"` (`#` = `minecraft:iron_block`, `~` = air).
//!
//! `CarvedPumpkinBlock.trySpawnGolem` spawns at `pattern.getBlock(0, 2, 0)` (snow) or
//! `getBlock(1, 2, 0)` (iron), which is the bottom block, then `spawnGolemInWorld` snaps the
//! entity to that block plus `(0.5, 0.05, 0.5)` with yaw and pitch `0.0`. `clearPatternBlocks`
//! sets every cell of the pattern to air and emits level event `2001` with the old state id.
//!
//! Only upright patterns are modelled. Vanilla `BlockPattern.find` also accepts the same pattern
//! lying in another plane; a flat structure is not a practical build and is left unmodelled.

use ferrite_entity::mob::MobKind;

/// One complete golem pattern found around a placed pumpkin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Build {
    /// The mob vanilla creates for this pattern.
    pub kind: MobKind,
    /// Every block cell the pattern covers, including the air cells it requires. All of them
    /// must be owned by one region before any block is changed.
    pub cells: Vec<[i32; 3]>,
    /// The non-air pattern blocks, with their current states, in clear order.
    pub removed: Vec<([i32; 3], u16)>,
    /// Bottom-center block the entity is snapped to.
    pub spawn: [i32; 3],
}

fn is_block(state: u16, name: &str) -> bool {
    ferrite_data::blocks::state(state).is_some_and(|(found, _)| found == name)
}

fn is_pumpkin(state: u16) -> bool {
    is_block(state, "minecraft:carved_pumpkin") || is_block(state, "minecraft:jack_o_lantern")
}

fn is_air(state: u16) -> bool {
    usize::from(state) < ferrite_data::blocks::STATE_COUNT
        && ferrite_data::block_props::get(state).is_air
}

pub fn is_pumpkin_state(state: u16) -> bool {
    is_pumpkin(state)
}

/// Returns whether placing `pumpkin_state` at `pumpkin` would complete a snow or iron golem pattern.
pub fn can_spawn_with_pumpkin(
    pumpkin: [i32; 3],
    pumpkin_state: u16,
    state: impl Fn(i32, i32, i32) -> Option<u16>,
) -> bool {
    is_pumpkin(pumpkin_state)
        && construction_at(pumpkin, |x, y, z| {
            if [x, y, z] == pumpkin {
                Some(pumpkin_state)
            } else {
                state(x, y, z)
            }
        })
        .is_some()
}

/// Matches the snow-golem and iron-golem patterns that have `pumpkin` as their top-center block.
/// `state` returns `None` for a block outside the loaded world, which refuses construction.
pub fn construction_at(
    pumpkin: [i32; 3],
    state: impl Fn(i32, i32, i32) -> Option<u16>,
) -> Option<Build> {
    let pumpkin_state = state(pumpkin[0], pumpkin[1], pumpkin[2])?;
    if !is_pumpkin(pumpkin_state) {
        return None;
    }
    let [x, y, z] = pumpkin;

    // SnowGolem full pattern: pumpkin, snow block, snow block.
    let first = state(x, y - 1, z)?;
    let second = state(x, y - 2, z)?;
    if is_block(first, "minecraft:snow_block") && is_block(second, "minecraft:snow_block") {
        return Some(Build {
            kind: MobKind::SnowGolem,
            cells: vec![pumpkin, [x, y - 1, z], [x, y - 2, z]],
            removed: vec![
                (pumpkin, pumpkin_state),
                ([x, y - 1, z], first),
                ([x, y - 2, z], second),
            ],
            spawn: [x, y - 2, z],
        });
    }

    // IronGolem full pattern, in both horizontal plane orientations. The T is symmetric, so a
    // half turn is the same shape and 90 degrees swaps the axis.
    for (sx, sz) in [(1, 0), (0, 1)] {
        let sides = [
            [x - sx, y, z - sz],
            [x + sx, y, z + sz],
            [x - sx, y - 2, z - sz],
            [x + sx, y - 2, z + sz],
        ];
        if !sides
            .iter()
            .all(|at| state(at[0], at[1], at[2]).is_some_and(is_air))
        {
            continue;
        }
        let row = [
            [x - sx, y - 1, z - sz],
            [x, y - 1, z],
            [x + sx, y - 1, z + sz],
        ];
        let bottom = [x, y - 2, z];
        let (Some(a), Some(b), Some(c), Some(bottom_state)) = (
            state(row[0][0], row[0][1], row[0][2]),
            state(row[1][0], row[1][1], row[1][2]),
            state(row[2][0], row[2][1], row[2][2]),
            state(bottom[0], bottom[1], bottom[2]),
        ) else {
            continue;
        };
        if ![
            is_block(a, "minecraft:iron_block"),
            is_block(b, "minecraft:iron_block"),
            is_block(c, "minecraft:iron_block"),
            is_block(bottom_state, "minecraft:iron_block"),
        ]
        .into_iter()
        .all(|matched| matched)
        {
            continue;
        }
        return Some(Build {
            kind: MobKind::IronGolem,
            cells: vec![
                pumpkin, sides[0], sides[1], row[0], row[1], row[2], sides[2], bottom, sides[3],
            ],
            removed: vec![
                (pumpkin, pumpkin_state),
                (row[0], a),
                (row[1], b),
                (row[2], c),
                (bottom, bottom_state),
            ],
            spawn: bottom,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn grid(entries: &[([i32; 3], &str)]) -> HashMap<[i32; 3], u16> {
        entries
            .iter()
            .map(|(at, name)| {
                (
                    *at,
                    ferrite_data::blocks::default_state(name).expect("vanilla block"),
                )
            })
            .collect()
    }

    /// Missing cells in these tests are air, like an empty part of a loaded world.
    fn read(blocks: &HashMap<[i32; 3], u16>, x: i32, y: i32, z: i32) -> Option<u16> {
        blocks
            .get(&[x, y, z])
            .copied()
            .or_else(|| ferrite_data::blocks::default_state("minecraft:air"))
    }

    #[test]
    fn two_snow_blocks_below_a_pumpkin_make_a_snow_golem() {
        let blocks = grid(&[
            ([0, 10, 0], "minecraft:carved_pumpkin"),
            ([0, 9, 0], "minecraft:snow_block"),
            ([0, 8, 0], "minecraft:snow_block"),
        ]);
        let build = construction_at([0, 10, 0], |x, y, z| read(&blocks, x, y, z))
            .expect("snow golem pattern");
        assert_eq!(build.kind, MobKind::SnowGolem);
        assert_eq!(build.spawn, [0, 8, 0]);
        assert_eq!(build.removed.len(), 3);
        assert_eq!(build.cells.len(), 3);
    }

    #[test]
    fn an_iron_t_makes_an_iron_golem_on_either_horizontal_axis() {
        for (sx, sz) in [(1, 0), (0, 1)] {
            let at = |x: i32, y: i32, z: i32| [x, y, z];
            let blocks = grid(&[
                (at(0, 10, 0), "minecraft:carved_pumpkin"),
                (at(-sx, 9, -sz), "minecraft:iron_block"),
                (at(0, 9, 0), "minecraft:iron_block"),
                (at(sx, 9, sz), "minecraft:iron_block"),
                (at(0, 8, 0), "minecraft:iron_block"),
            ]);
            let build = construction_at([0, 10, 0], |x, y, z| read(&blocks, x, y, z))
                .expect("iron golem pattern");
            assert_eq!(build.kind, MobKind::IronGolem);
            assert_eq!(build.spawn, [0, 8, 0]);
            assert_eq!(build.removed.len(), 5);
            assert_eq!(build.cells.len(), 9);
        }
    }

    #[test]
    fn a_jack_o_lantern_also_completes_the_pattern() {
        let blocks = grid(&[
            ([0, 10, 0], "minecraft:jack_o_lantern"),
            ([0, 9, 0], "minecraft:iron_block"),
            ([0, 8, 0], "minecraft:iron_block"),
            ([1, 9, 0], "minecraft:iron_block"),
            ([-1, 9, 0], "minecraft:iron_block"),
        ]);
        assert!(construction_at([0, 10, 0], |x, y, z| read(&blocks, x, y, z)).is_some());
    }

    #[test]
    fn incomplete_and_obstructed_shapes_do_not_match() {
        // `read` returns air for cells absent from the map (see above).

        // Only one snow block.
        let snow = grid(&[
            ([0, 10, 0], "minecraft:carved_pumpkin"),
            ([0, 9, 0], "minecraft:snow_block"),
        ]);
        assert!(construction_at([0, 10, 0], |x, y, z| read(&snow, x, y, z)).is_none());

        // The complete iron T, but a stone in one of the air cells beside the pumpkin.
        let iron = grid(&[
            ([0, 10, 0], "minecraft:carved_pumpkin"),
            ([-1, 9, 0], "minecraft:iron_block"),
            ([0, 9, 0], "minecraft:iron_block"),
            ([1, 9, 0], "minecraft:iron_block"),
            ([0, 8, 0], "minecraft:iron_block"),
            ([-1, 10, 0], "minecraft:stone"),
        ]);
        assert!(construction_at([0, 10, 0], |x, y, z| read(&iron, x, y, z)).is_none());

        // Iron T with the bottom block missing.
        let mut incomplete = iron.clone();
        incomplete.remove(&[0, 8, 0]);
        incomplete.remove(&[-1, 10, 0]);
        assert!(construction_at([0, 10, 0], |x, y, z| read(&incomplete, x, y, z)).is_none());

        // The shape is anchored on the pumpkin, not on some other block.
        assert!(construction_at([0, 9, 0], |x, y, z| read(&snow, x, y, z)).is_none());
    }
}
