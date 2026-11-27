//! Special vanilla 26.3 item & block mechanics:
//! - Bone Meal (`minecraft:bone_meal`) crop/sapling/grass fertilization
//! - Sponge (`minecraft:sponge` / `minecraft:wet_sponge`) 65-block BFS water absorption & Nether drying
//! - Respawn Anchor (`minecraft:respawn_anchor`) glowstone charging, Nether respawn, Overworld explosion
//! - Copper Bulb (`*_copper_bulb`) redstone/interaction toggle

use std::collections::{HashSet, VecDeque};

use super::Dimension;
use super::anvil::AnvilChunkSource;
use super::blocks::{property, with_props};

/// Vanilla `SpongeBlock.removeWaterBreadthFirstSearch`: maximum 65 water blocks within Manhattan
/// distance 6 are drained into air, converting `sponge` -> `wet_sponge`. In the Nether, placing
/// `wet_sponge` dries it immediately into `sponge`. Returns `(pos, new_state)` updates.
pub fn handle_sponge(
    source: &AnvilChunkSource,
    pos: [i32; 3],
    block_name: &str,
    dimension: Dimension,
) -> Vec<([i32; 3], u16)> {
    if block_name == "minecraft:wet_sponge" && dimension == Dimension::Nether {
        if let Some(dry) = ferrite_data::blocks::default_state("sponge")
            && source
                .set_block(pos[0], pos[1], pos[2], u32::from(dry))
                .is_ok()
        {
            return vec![(pos, dry)];
        }
        return Vec::new();
    }
    if block_name != "minecraft:sponge" {
        return Vec::new();
    }
    let Some(air) = ferrite_data::blocks::default_state("air") else {
        return Vec::new();
    };
    let Some(wet) = ferrite_data::blocks::default_state("wet_sponge") else {
        return Vec::new();
    };
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    visited.insert(pos);
    queue.push_back((pos, 0u8));
    let mut drained = Vec::new();

    while let Some((cur, dist)) = queue.pop_front() {
        if drained.len() >= 65 {
            break;
        }
        for (dx, dy, dz) in [
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
        ] {
            if drained.len() >= 65 {
                break;
            }
            let next = [cur[0] + dx, cur[1] + dy, cur[2] + dz];
            if !visited.insert(next) {
                continue;
            }
            let is_water = source.block(next[0], next[1], next[2]).is_some_and(|s| {
                ferrite_fluid::fluid_state(s).kind == ferrite_fluid::FluidKind::Water
            });
            if is_water {
                if source
                    .set_block(next[0], next[1], next[2], u32::from(air))
                    .is_ok()
                {
                    drained.push((next, air));
                }
                if dist < 6 {
                    queue.push_back((next, dist + 1));
                }
            }
        }
    }
    if !drained.is_empty()
        && source
            .set_block(pos[0], pos[1], pos[2], u32::from(wet))
            .is_ok()
    {
        drained.push((pos, wet));
    }
    drained
}

/// Vanilla `BoneMealItem.growCrop`: advances crop age, sapling stage, or sprouts grass/flowers
/// on `grass_block`. Returns changed `(pos, new_state)` pairs if the bone meal had an effect.
pub fn apply_bone_meal(
    source: &AnvilChunkSource,
    pos: [i32; 3],
    state: u16,
) -> Option<Vec<([i32; 3], u16)>> {
    let (name, _) = ferrite_data::blocks::state(state)?;
    let max_age: Option<u8> = match name {
        "minecraft:wheat"
        | "minecraft:carrots"
        | "minecraft:potatoes"
        | "minecraft:melon_stem"
        | "minecraft:pumpkin_stem" => Some(7),
        "minecraft:beetroots" | "minecraft:sweet_berry_bush" => Some(3),
        "minecraft:cocoa" => Some(2),
        "minecraft:torchflower_crop" => Some(1),
        "minecraft:pitcher_crop" => Some(4),
        _ => None,
    };
    if let Some(max) = max_age {
        let cur: u8 = property(state, "age")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        if cur >= max {
            return None;
        }
        let next_age = (cur + 2).min(max);
        let age_str = next_age.to_string();
        let next_state = with_props(state, &[("age", age_str.as_str())])?;
        let _ = source.set_block(pos[0], pos[1], pos[2], u32::from(next_state));
        return Some(vec![(pos, next_state)]);
    }
    if name.ends_with("_sapling") {
        let cur = property(state, "stage").unwrap_or("0");
        if cur == "0" {
            let next_state = with_props(state, &[("stage", "1")])?;
            let _ = source.set_block(pos[0], pos[1], pos[2], u32::from(next_state));
            return Some(vec![(pos, next_state)]);
        }
    }
    if name == "minecraft:grass_block" {
        let above = [pos[0], pos[1] + 1, pos[2]];
        let air_above = source
            .block(above[0], above[1], above[2])
            .and_then(|s| u16::try_from(s).ok())
            .is_some_and(|s| ferrite_data::block_props::get(s).is_air);
        if air_above && let Some(grass) = ferrite_data::blocks::default_state("short_grass") {
            let _ = source.set_block(above[0], above[1], above[2], u32::from(grass));
            return Some(vec![(above, grass)]);
        }
    }
    None
}

/// Outcome of right-clicking a `minecraft:respawn_anchor`.
#[derive(Debug, PartialEq, Eq)]
pub enum RespawnAnchorOutcome {
    /// Consumed 1 glowstone to raise `charges` to the new state.
    Charged(u16),
    /// Anchor is in the Nether with charges > 0: player respawn point set.
    SetRespawn,
    /// Anchor used outside the Nether with charges > 0: replaced with air and explodes (power 5.0).
    Explode(u16),
}

/// Vanilla `RespawnAnchorBlock.useItemOn` / `useWithoutItem`.
pub fn interact_respawn_anchor(
    source: &AnvilChunkSource,
    pos: [i32; 3],
    state: u16,
    holding_glowstone: bool,
    dimension: Dimension,
) -> Option<RespawnAnchorOutcome> {
    let (name, _) = ferrite_data::blocks::state(state)?;
    if name != "minecraft:respawn_anchor" {
        return None;
    }
    let charges: u8 = property(state, "charges")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if holding_glowstone && charges < 4 {
        let next_str = (charges + 1).to_string();
        let next = with_props(state, &[("charges", next_str.as_str())])?;
        let _ = source.set_block(pos[0], pos[1], pos[2], u32::from(next));
        return Some(RespawnAnchorOutcome::Charged(next));
    }
    if charges == 0 {
        return None;
    }
    if dimension == Dimension::Nether {
        Some(RespawnAnchorOutcome::SetRespawn)
    } else {
        let air = ferrite_data::blocks::default_state("air")?;
        let _ = source.set_block(pos[0], pos[1], pos[2], u32::from(air));
        Some(RespawnAnchorOutcome::Explode(air))
    }
}

/// Toggles the `lit` property on any `*_copper_bulb` block.
pub fn toggle_copper_bulb(state: u16) -> Option<u16> {
    let (name, _) = ferrite_data::blocks::state(state)?;
    if !name.ends_with("copper_bulb") {
        return None;
    }
    let lit = property(state, "lit") == Some("true");
    with_props(state, &[("lit", if lit { "false" } else { "true" })])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sponge_bone_meal_respawn_anchor_and_copper_bulb_work() {
        let source = AnvilChunkSource::new_memory();
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        // 1. Sponge absorbs adjacent water and turns into wet_sponge; dries in Nether.
        let water = ferrite_data::blocks::default_state("water").unwrap();
        let sponge = ferrite_data::blocks::default_state("sponge").unwrap();
        source.set_block(5, 64, 5, u32::from(sponge)).unwrap();
        source.set_block(6, 64, 5, u32::from(water)).unwrap();
        source.set_block(7, 64, 5, u32::from(water)).unwrap();
        let updates = handle_sponge(
            &source,
            [5, 64, 5],
            "minecraft:sponge",
            Dimension::Overworld,
        );
        assert_eq!(updates.len(), 3, "2 water blocks + wet_sponge");
        assert_eq!(
            ferrite_data::blocks::state(source.block(5, 64, 5).unwrap() as u16).map(|(n, _)| n),
            Some("minecraft:wet_sponge")
        );
        let dry = handle_sponge(
            &source,
            [5, 64, 5],
            "minecraft:wet_sponge",
            Dimension::Nether,
        );
        assert_eq!(dry.len(), 1);
        assert_eq!(
            ferrite_data::blocks::state(source.block(5, 64, 5).unwrap() as u16).map(|(n, _)| n),
            Some("minecraft:sponge")
        );

        // 2. Bone meal advances wheat crop age.
        let wheat = ferrite_data::blocks::default_state("wheat").unwrap();
        source.set_block(2, 64, 2, u32::from(wheat)).unwrap();
        let grown = apply_bone_meal(&source, [2, 64, 2], wheat).unwrap();
        assert_eq!(property(grown[0].1, "age"), Some("2"));

        // 3. Respawn anchor charges with glowstone, sets respawn in Nether, explodes in Overworld.
        let anchor = ferrite_data::blocks::default_state("respawn_anchor").unwrap();
        source.set_block(3, 64, 3, u32::from(anchor)).unwrap();
        let RespawnAnchorOutcome::Charged(c1) =
            interact_respawn_anchor(&source, [3, 64, 3], anchor, true, Dimension::Nether).unwrap()
        else {
            panic!("expected Charged");
        };
        assert_eq!(property(c1, "charges"), Some("1"));
        assert_eq!(
            interact_respawn_anchor(&source, [3, 64, 3], c1, false, Dimension::Nether),
            Some(RespawnAnchorOutcome::SetRespawn)
        );
        assert!(matches!(
            interact_respawn_anchor(&source, [3, 64, 3], c1, false, Dimension::Overworld),
            Some(RespawnAnchorOutcome::Explode(_))
        ));

        // 4. Copper bulb toggles lit.
        let bulb = ferrite_data::blocks::default_state("copper_bulb").unwrap();
        let lit = toggle_copper_bulb(bulb).unwrap();
        assert_eq!(property(lit, "lit"), Some("true"));
        assert_eq!(
            property(toggle_copper_bulb(lit).unwrap(), "lit"),
            Some("false")
        );
    }
}
