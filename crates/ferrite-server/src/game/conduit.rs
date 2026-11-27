//! Conduit block entities (vanilla `ConduitBlockEntity`).
//!
//! Validates the 3x3x3 water shell and 5x5 orthogonal prismarine/sea-lantern activating frame,
//! grants `minecraft:conduit_power` to nearby wet players every 40 ticks, and damages nearby
//! hostile mobs at a full 42-block frame.

use ferrite_entity::effect::{Effect, MobEffectInstance};
use ferrite_nbt::{Compound, Tag};
use ferrite_world::BlockEntity;

use super::anvil::AnvilChunkSource;

/// The block and block entity type name of a conduit.
pub const BLOCK: &str = "minecraft:conduit";
/// Ticks between conduit effect/attack pulses (vanilla refreshes every 40 ticks).
pub const EFFECT_INTERVAL: u32 = 40;
/// Duration of `ConduitPower` applied on each pulse (vanilla 260 ticks = 13 seconds).
pub const EFFECT_DURATION: i32 = 260;
/// Minimum activating blocks required for the conduit to turn on.
pub const MIN_FRAME_BLOCKS: usize = 16;
/// Full activating frame size required for the hostile-mob attack beam.
pub const FULL_FRAME_BLOCKS: usize = 42;
/// Hostile mob attack range around a fully activated conduit (vanilla 8 blocks).
pub const ATTACK_RANGE: f64 = 8.0;
/// Hostile mob attack damage per pulse (vanilla 4.0 magic damage).
pub const ATTACK_DAMAGE: f32 = 4.0;

/// Whether a block name counts toward a conduit's activating frame (`ConduitBlockEntity.VALID_BLOCKS`).
pub fn is_frame_block(name: &str) -> bool {
    matches!(
        name,
        "minecraft:prismarine"
            | "minecraft:prismarine_bricks"
            | "minecraft:dark_prismarine"
            | "minecraft:sea_lantern"
    )
}

/// Whether a block state is water or waterlogged (required inside the 3x3x3 shell around the conduit).
pub fn is_water_block(state: u16) -> bool {
    let fluid = ferrite_fluid::fluid_state(u32::from(state)).kind;
    if fluid == ferrite_fluid::FluidKind::Water {
        return true;
    }
    ferrite_data::blocks::state(state).is_some_and(|(_, props)| {
        props
            .iter()
            .any(|&(k, v)| k == "waterlogged" && v == "true")
    })
}

/// Counts valid activating frame blocks around `pos` (`ConduitBlockEntity.updateShape`).
/// Returns `0` if any block in the inner 3x3x3 cube is not water/waterlogged.
pub fn scan_frame(source: &AnvilChunkSource, [x, y, z]: [i32; 3]) -> usize {
    for dx in -1..=1 {
        for dy in -1..=1 {
            for dz in -1..=1 {
                let is_water = source
                    .block(x + dx, y + dy, z + dz)
                    .and_then(|s| u16::try_from(s).ok())
                    .is_some_and(is_water_block);
                if !is_water {
                    return 0;
                }
            }
        }
    }

    let mut count = 0usize;
    for dx in -2..=2i32 {
        for dy in -2..=2i32 {
            for dz in -2..=2i32 {
                let ax = dx.abs();
                let ay = dy.abs();
                let az = dz.abs();
                // Three 5x5 orthogonal rings at distance 2 from the center.
                let on_ring = (ax == 2 && (ay == 0 || az == 0))
                    || (ay == 2 && (ax == 0 || az == 0))
                    || (az == 2 && (ax == 0 || ay == 0));
                if !on_ring {
                    continue;
                }
                let valid = source
                    .block(x + dx, y + dy, z + dz)
                    .and_then(|s| u16::try_from(s).ok())
                    .and_then(ferrite_data::blocks::state)
                    .is_some_and(|(name, _)| is_frame_block(name));
                if valid {
                    count += 1;
                }
            }
        }
    }
    count
}

/// Vanilla `ConduitBlockEntity.applyEffects` radius: `(frame_blocks / 7) * 16` blocks
/// (0 when fewer than 16 blocks; 32..=96 blocks once active).
pub fn effect_range(frame_blocks: usize) -> f64 {
    if frame_blocks < MIN_FRAME_BLOCKS {
        0.0
    } else {
        ((frame_blocks / 7) * 16) as f64
    }
}

/// The `ConduitPower` effect instance applied by an active conduit.
pub fn conduit_effect() -> MobEffectInstance {
    MobEffectInstance::new(Effect::ConduitPower.id(), EFFECT_DURATION, 0)
}

/// Ensures a freshly placed conduit has its block entity so `tick_conduits` finds it.
pub fn ensure(source: &AnvilChunkSource, [x, y, z]: [i32; 3]) {
    let Some(type_id) = ferrite_data::registries::id("minecraft:block_entity_type", BLOCK)
        .and_then(|id| u32::try_from(id).ok())
    else {
        return;
    };
    let _ = source.update_block_entity(x, y, z, |entity| {
        if entity.is_some() {
            return (false, ());
        }
        let mut nbt = Compound::new();
        nbt.insert("id".into(), Tag::String(BLOCK.into()));
        nbt.insert("x".into(), Tag::Int(x));
        nbt.insert("y".into(), Tag::Int(y));
        nbt.insert("z".into(), Tag::Int(z));
        *entity = Some(BlockEntity { type_id, data: nbt });
        (true, ())
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn conduit_frame_scan_and_range_match_vanilla() {
        let source = AnvilChunkSource::new_memory();
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let center = [10, 64, 10];
        let water = ferrite_data::blocks::default_state("water").unwrap();
        let conduit = ferrite_data::blocks::default_state("conduit").unwrap();
        let prismarine = ferrite_data::blocks::default_state("prismarine").unwrap();

        // Fill inner 3x3x3 with water and conduit (default conduit is waterlogged=true).
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let s = if dx == 0 && dy == 0 && dz == 0 {
                        conduit
                    } else {
                        water
                    };
                    source
                        .set_block(center[0] + dx, center[1] + dy, center[2] + dz, u32::from(s))
                        .unwrap();
                }
            }
        }
        assert_eq!(scan_frame(&source, center), 0);
        assert_eq!(effect_range(0), 0.0);

        // Build one complete 5x5 horizontal ring (16 blocks) -> activates with 32-block range.
        for dx in -2..=2i32 {
            for dz in -2..=2i32 {
                if dx.abs() == 2 || dz.abs() == 2 {
                    source
                        .set_block(
                            center[0] + dx,
                            center[1],
                            center[2] + dz,
                            u32::from(prismarine),
                        )
                        .unwrap();
                }
            }
        }
        assert_eq!(scan_frame(&source, center), 16);
        assert_eq!(effect_range(16), 32.0);

        // Complete all three orthogonal rings -> 42 blocks, 96-block range.
        for dx in -2..=2i32 {
            for dy in -2..=2i32 {
                for dz in -2..=2i32 {
                    let ax = dx.abs();
                    let ay = dy.abs();
                    let az = dz.abs();
                    if (ax == 2 && (ay == 0 || az == 0))
                        || (ay == 2 && (ax == 0 || az == 0))
                        || (az == 2 && (ax == 0 || ay == 0))
                    {
                        source
                            .set_block(
                                center[0] + dx,
                                center[1] + dy,
                                center[2] + dz,
                                u32::from(prismarine),
                            )
                            .unwrap();
                    }
                }
            }
        }
        assert_eq!(scan_frame(&source, center), FULL_FRAME_BLOCKS);
        assert_eq!(effect_range(FULL_FRAME_BLOCKS), 96.0);
    }
}
