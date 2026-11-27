//! Sculk ecosystem block entities (vanilla `SculkCatalystBlockEntity`,
//! `SculkShriekerBlockEntity`, `SculkSensorBlockEntity`, `CalibratedSculkSensorBlockEntity`).

use ferrite_nbt::{Compound, Tag};
use ferrite_world::BlockEntity;

use super::anvil::AnvilChunkSource;
use super::blocks::{property, with_props};

pub const CATALYST: &str = "minecraft:sculk_catalyst";
pub const SHRIEKER: &str = "minecraft:sculk_shrieker";
pub const SENSOR: &str = "minecraft:sculk_sensor";
pub const CALIBRATED_SENSOR: &str = "minecraft:calibrated_sculk_sensor";

/// Vanilla vibration frequencies (`VibrationSystem.VIBRATION_FREQUENCY_FOR_EVENT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vibration {
    Step = 1,
    ProjectileLand = 2,
    EntityDamage = 7,
    Eat = 8,
    BlockDestroy = 12,
    BlockPlace = 13,
    Explode = 15,
}

impl Vibration {
    pub fn frequency(self) -> u8 {
        self as u8
    }
}

/// Vanilla `CalibratedSculkSensor`: accepts all vibrations when `input_signal == 0`,
/// otherwise only when `vibration.frequency() == input_signal`.
pub fn calibrated_accepts(vibration: Vibration, input_signal: u8) -> bool {
    input_signal == 0 || vibration.frequency() == input_signal
}

fn is_sculk_replaceable(name: &str) -> bool {
    matches!(
        name,
        "minecraft:stone"
            | "minecraft:deepslate"
            | "minecraft:dirt"
            | "minecraft:grass_block"
            | "minecraft:coarse_dirt"
            | "minecraft:sand"
            | "minecraft:gravel"
            | "minecraft:andesite"
            | "minecraft:diorite"
            | "minecraft:granite"
            | "minecraft:tuff"
            | "minecraft:netherrack"
    )
}

/// Ensures a sculk block entity (`sculk_catalyst`, `sculk_shrieker`, `sculk_sensor`,
/// `calibrated_sculk_sensor`) exists at `pos`.
pub fn ensure(source: &AnvilChunkSource, [x, y, z]: [i32; 3], block_entity_id: &str) {
    let Some(type_id) =
        ferrite_data::registries::id("minecraft:block_entity_type", block_entity_id)
            .and_then(|id| u32::try_from(id).ok())
    else {
        return;
    };
    let _ = source.update_block_entity(x, y, z, |entity| {
        if entity.is_some() {
            return (false, ());
        }
        let mut nbt = Compound::new();
        nbt.insert("id".into(), Tag::String(block_entity_id.into()));
        nbt.insert("x".into(), Tag::Int(x));
        nbt.insert("y".into(), Tag::Int(y));
        nbt.insert("z".into(), Tag::Int(z));
        *entity = Some(BlockEntity { type_id, data: nbt });
        (true, ())
    });
}

/// Vanilla `SculkCatalystBlockEntity.handleGameEvent(ENTITY_DIE)`: if a `sculk_catalyst` exists
/// within 8 blocks of `death_pos`, converts up to `xp_budget.min(12)` replaceable blocks around
/// the death position into `minecraft:sculk` and blooms the catalyst. Returns changed positions.
pub fn spread_on_mob_death(
    source: &AnvilChunkSource,
    death_pos: [i32; 3],
    xp_budget: usize,
) -> Vec<[i32; 3]> {
    let Some(sculk_state) = ferrite_data::blocks::default_state("sculk") else {
        return Vec::new();
    };
    let mut catalyst_pos = None;
    'search: for dx in -8..=8 {
        for dy in -8..=8 {
            for dz in -8..=8 {
                if dx * dx + dy * dy + dz * dz > 64 {
                    continue;
                }
                let p = [death_pos[0] + dx, death_pos[1] + dy, death_pos[2] + dz];
                let is_cat = source
                    .block(p[0], p[1], p[2])
                    .and_then(|s| u16::try_from(s).ok())
                    .and_then(ferrite_data::blocks::state)
                    .is_some_and(|(n, _)| n == CATALYST);
                if is_cat {
                    catalyst_pos = Some(p);
                    break 'search;
                }
            }
        }
    }
    let Some(cat) = catalyst_pos else {
        return Vec::new();
    };

    let mut changed = Vec::new();
    if let Some(cur) = source
        .block(cat[0], cat[1], cat[2])
        .and_then(|s| u16::try_from(s).ok())
        && let Some(bloomed) = with_props(cur, &[("bloom", "true")])
        && source
            .set_block(cat[0], cat[1], cat[2], u32::from(bloomed))
            .is_ok()
    {
        changed.push(cat);
    }

    let budget = xp_budget.clamp(1, 12);
    let mut spread = 0usize;
    'spread: for r in 0..=2i32 {
        for dx in -r..=r {
            for dy in -2..=0 {
                for dz in -r..=r {
                    if spread >= budget {
                        break 'spread;
                    }
                    let p = [death_pos[0] + dx, death_pos[1] + dy, death_pos[2] + dz];
                    let replaceable = source
                        .block(p[0], p[1], p[2])
                        .and_then(|s| u16::try_from(s).ok())
                        .and_then(ferrite_data::blocks::state)
                        .is_some_and(|(n, _)| is_sculk_replaceable(n));
                    if replaceable
                        && source
                            .set_block(p[0], p[1], p[2], u32::from(sculk_state))
                            .is_ok()
                    {
                        changed.push(p);
                        spread += 1;
                    }
                }
            }
        }
    }
    changed
}

/// Vanilla `SculkShriekerBlockEntity.tryShriek`: increments `warning_level` (capped at 4),
/// sets `shrieking=true` when `can_summon=true`, and returns a Warden spawn position once
/// `warning_level >= 4` and `spawn_wardens` is enabled.
pub fn trigger_shrieker(
    source: &AnvilChunkSource,
    pos: [i32; 3],
    warning_level: &mut u8,
    spawn_wardens: bool,
) -> Option<[f64; 3]> {
    let cur = source
        .block(pos[0], pos[1], pos[2])
        .and_then(|s| u16::try_from(s).ok())?;
    let (name, _) = ferrite_data::blocks::state(cur)?;
    if name != SHRIEKER || property(cur, "can_summon") != Some("true") {
        return None;
    }
    if let Some(shrieking) = with_props(cur, &[("shrieking", "true")]) {
        let _ = source.set_block(pos[0], pos[1], pos[2], u32::from(shrieking));
    }
    *warning_level = (*warning_level).saturating_add(1).min(4);
    if *warning_level >= 4 && spawn_wardens {
        Some([
            f64::from(pos[0]) + 0.5,
            f64::from(pos[1]) + 1.0,
            f64::from(pos[2]) + 0.5,
        ])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sculk_catalyst_spreads_shrieker_summons_warden_and_calibrated_filters() {
        let source = AnvilChunkSource::new_memory();
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let stone = ferrite_data::blocks::default_state("stone").unwrap();
        let cat = ferrite_data::blocks::default_state("sculk_catalyst").unwrap();
        for x in 2..=8 {
            for z in 2..=8 {
                source.set_block(x, 63, z, u32::from(stone)).unwrap();
            }
        }
        source.set_block(5, 64, 5, u32::from(cat)).unwrap();
        ensure(&source, [5, 64, 5], CATALYST);

        let changed = spread_on_mob_death(&source, [6, 64, 6], 5);
        assert!(changed.len() >= 2, "catalyst bloomed and spread sculk");
        let cat_now = source.block(5, 64, 5).unwrap() as u16;
        assert_eq!(property(cat_now, "bloom"), Some("true"));

        let shrieker = ferrite_data::blocks::state_id(
            "minecraft:sculk_shrieker",
            &[
                ("can_summon", "true"),
                ("shrieking", "false"),
                ("waterlogged", "false"),
            ],
        )
        .unwrap();
        source.set_block(8, 64, 8, u32::from(shrieker)).unwrap();
        let mut warn = 0u8;
        assert!(trigger_shrieker(&source, [8, 64, 8], &mut warn, true).is_none());
        assert_eq!(warn, 1);
        warn = 3;
        assert!(trigger_shrieker(&source, [8, 64, 8], &mut warn, false).is_none());
        assert_eq!(warn, 4);
        assert!(trigger_shrieker(&source, [8, 64, 8], &mut warn, true).is_some());

        assert!(calibrated_accepts(Vibration::BlockPlace, 0));
        assert!(calibrated_accepts(Vibration::BlockPlace, 13));
        assert!(!calibrated_accepts(Vibration::BlockPlace, 7));
    }
}
