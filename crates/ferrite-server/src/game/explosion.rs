//! Vanilla `Explosion`: the 16x16x16 surface ray cast of `Explosion.explode`, per-state
//! explosion resistance, the `Explosion.getSeenPercent` exposure rays used for knockback, and
//! loot-table drops (vanilla `Block.getDrops` -> `ferrite-loot`).

use super::neighbors::BlockWorld;
use ferrite_data::{block_props, blocks};
use ferrite_entity::Rng;
use ferrite_math::{AABB, Vec3, raycast};
use std::collections::HashSet;

/// Vanilla `PrimedTnt` power.
pub const TNT_POWER: f32 = 4.0;
/// Vanilla `Creeper` explosion radius (`Creeper.DEFAULT_EXPLOSION_RADIUS`).
pub const CREEPER_POWER: f32 = 3.0;
/// Vanilla charged creeper multiplier (`Creeper` charged explosion radius).
pub const CHARGED_CREEPER_POWER: f32 = 6.0;

/// Vanilla `Explosion` steps 0.3 blocks per ray and subtracts `(resistance + 0.3) * 0.3`.
const RAY_STEP: f64 = 0.3;
const RAY_DECAY: f32 = 0.3 * 0.75;

/// Vanilla `BlockState.getExplosionResistance`; fluids use vanilla's `FluidState` value 100.
pub fn resistance_of(state: u16) -> f32 {
    if usize::from(state) >= blocks::STATE_COUNT {
        return 0.0;
    }
    let props = block_props::get(state);
    if props.fluid.is_some() {
        return 100.0;
    }
    blocks::state(state)
        .and_then(|(name, _)| block_props::block_info(name))
        .map_or(0.0, |info| info.explosion_resistance)
}

/// Vanilla `BlockState.shouldBlockExplode`: fluids and air are never part of the blast, and
/// blocks at or above resistance 100 stop the ray before this point.
fn should_explode(state: u16) -> bool {
    if usize::from(state) >= blocks::STATE_COUNT {
        return false;
    }
    let props = block_props::get(state);
    !props.is_air && props.fluid.is_none() && resistance_of(state) < 100.0
}

/// Vanilla `Explosion.explode` block selection: one ray for each cell of the 16x16x16 cube's
/// surface, stepping 0.3 blocks and decaying by `(resistance + 0.3) * 0.3` plus the loop's
/// 0.225 per step. Returns the block positions in sorted order.
pub fn explode_blocks(
    world: &impl BlockWorld,
    center: [f64; 3],
    power: f32,
    rng: &mut Rng,
) -> Vec<[i32; 3]> {
    let mut to_blow = HashSet::new();
    for j in 0..16 {
        for k in 0..16 {
            for l in 0..16 {
                if j != 0 && j != 15 && k != 0 && k != 15 && l != 0 && l != 15 {
                    continue;
                }
                let mut direction = [
                    f64::from(j) / 15.0 * 2.0 - 1.0,
                    f64::from(k) / 15.0 * 2.0 - 1.0,
                    f64::from(l) / 15.0 * 2.0 - 1.0,
                ];
                let length = (direction[0] * direction[0]
                    + direction[1] * direction[1]
                    + direction[2] * direction[2])
                    .sqrt();
                if length == 0.0 {
                    continue;
                }
                for value in &mut direction {
                    *value /= length;
                }
                // Vanilla `Explosion` starts each ray at `radius * (0.7 + random * 0.6)`.
                let mut remaining = power * (0.7 + rng.next_f32() * 0.6);
                let mut at = center;
                while remaining > 0.0 {
                    let pos = [
                        at[0].floor() as i32,
                        at[1].floor() as i32,
                        at[2].floor() as i32,
                    ];
                    // Region-local: an unloaded neighbour stops the ray instead of loading it.
                    let Some(state) = world.block(pos[0], pos[1], pos[2]) else {
                        break;
                    };
                    if usize::from(state) < blocks::STATE_COUNT {
                        let props = block_props::get(state);
                        if !props.is_air || props.fluid.is_some() {
                            remaining -= (resistance_of(state) + 0.3) * 0.3;
                        }
                        if remaining > 0.0 && should_explode(state) {
                            to_blow.insert(pos);
                        }
                    }
                    for (axis, value) in direction.iter().enumerate() {
                        at[axis] += value * RAY_STEP;
                    }
                    remaining -= RAY_DECAY;
                }
            }
        }
    }
    let mut out: Vec<_> = to_blow.into_iter().collect();
    out.sort_unstable();
    out
}

/// Whether the block collision shapes between `from` and `to` leave the segment open
/// (vanilla `ClipContext.Block.COLLIDER` used by `Explosion.getSeenPercent`).
fn sees(world: &impl BlockWorld, from: Vec3, to: Vec3) -> bool {
    let delta = to - from;
    let distance = delta.length();
    if distance < 1.0E-7 {
        return true;
    }
    let direction = delta * (1.0 / distance);
    let mut shapes: std::collections::HashMap<(i32, i32, i32), Vec<AABB>> =
        std::collections::HashMap::new();
    let start = ferrite_math::BlockPos::containing(from);
    let end = ferrite_math::BlockPos::containing(to);
    for x in start.x.min(end.x)..=start.x.max(end.x) {
        for y in start.y.min(end.y)..=start.y.max(end.y) {
            for z in start.z.min(end.z)..=start.z.max(end.z) {
                let boxes: Vec<AABB> = match world.block(x, y, z) {
                    Some(state) if usize::from(state) < blocks::STATE_COUNT => {
                        block_props::collision_shape(state)
                            .iter()
                            .map(|b| AABB::new(b[0], b[1], b[2], b[3], b[4], b[5]))
                            .collect()
                    }
                    Some(_) => Vec::new(),
                    // An unloaded chunk occludes like a full block.
                    None => vec![AABB::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0)],
                };
                if !boxes.is_empty() {
                    shapes.insert((x, y, z), boxes);
                }
            }
        }
    }
    raycast(from, direction, distance, |pos| {
        shapes.get(&(pos.x, pos.y, pos.z)).map(Vec::as_slice)
    })
    .is_none()
}

/// Vanilla `Explosion.getSeenPercent`: samples a grid over the entity's box and counts the
/// samples with an unobstructed line to the explosion center.
pub fn seen_percent(world: &impl BlockWorld, center: [f64; 3], bbox: AABB) -> f32 {
    let from = Vec3::new(center[0], center[1], center[2]);
    let size = [
        bbox.max.x - bbox.min.x,
        bbox.max.y - bbox.min.y,
        bbox.max.z - bbox.min.z,
    ];
    let step = size.map(|extent| 1.0 / (extent * 2.0 + 1.0));
    let offset = [0, 1, 2].map(|axis| {
        let count = (1.0 / step[axis]).floor();
        (1.0 - count * step[axis]) / 2.0
    });
    let mut total = 0u32;
    let mut visible = 0u32;
    let mut x = offset[0];
    while x <= 1.0 {
        let mut y = offset[1];
        while y <= 1.0 {
            let mut z = offset[2];
            while z <= 1.0 {
                let sample = Vec3::new(
                    bbox.min.x + size[0] * x,
                    bbox.min.y + size[1] * y,
                    bbox.min.z + size[2] * z,
                );
                total += 1;
                if sees(world, from, sample) {
                    visible += 1;
                }
                z += step[2];
            }
            y += step[1];
        }
        x += step[0];
    }
    if total == 0 {
        1.0
    } else {
        visible as f32 / total as f32
    }
}

/// Explosion damage falls with distance and the exposed fraction of the entity's bounds.
pub fn damage(
    world: &impl BlockWorld,
    center: [f64; 3],
    power: f32,
    feet: Vec3,
    bbox: AABB,
) -> f32 {
    let radius = f64::from(power) * 2.0;
    if radius <= 0.0 {
        return 0.0;
    }
    let distance = (feet - Vec3::new(center[0], center[1], center[2])).length() / radius;
    if distance > 1.0 {
        return 0.0;
    }
    let impact = (1.0 - distance) * f64::from(seen_percent(world, center, bbox));
    (((impact * impact + impact) / 2.0 * 7.0 * radius) + 1.0) as f32
}

/// Vanilla `Explosion` entity push: `(1 - distanceSqr / radius²) * getSeenPercent()` along the
/// direction from the blast center. Returns the velocity delta to add to the entity.
pub fn impulse(
    world: &impl BlockWorld,
    center: [f64; 3],
    power: f32,
    feet: Vec3,
    bbox: AABB,
) -> Option<[f64; 3]> {
    if power <= 0.0 {
        return None;
    }
    let dx = feet.x - center[0];
    let dy = feet.y - center[1];
    let dz = feet.z - center[2];
    let distance_sqr = dx * dx + dy * dy + dz * dz;
    let scaled = distance_sqr / f64::from(power * power);
    if scaled > 1.0 {
        return None;
    }
    let distance = distance_sqr.sqrt();
    if distance == 0.0 {
        return None;
    }
    let seen = f64::from(seen_percent(world, center, bbox));
    let strength = (1.0 - scaled) * seen;
    Some([
        dx / distance * strength,
        dy / distance * strength,
        dz / distance * strength,
    ])
}

#[cfg(test)]
mod tests {
    use super::super::anvil::AnvilChunkSource;
    use super::*;
    use ferrite_world::RuntimeRegistry;
    use std::collections::HashMap;

    struct Grid(HashMap<[i32; 3], u16>);

    impl Grid {
        fn new() -> Self {
            Self(HashMap::new())
        }
        fn set(&mut self, pos: [i32; 3], name: &str) {
            self.0
                .insert(pos, ferrite_data::blocks::default_state(name).unwrap());
        }
    }

    impl BlockWorld for Grid {
        fn block(&self, x: i32, y: i32, z: i32) -> Option<u16> {
            Some(
                *self
                    .0
                    .get(&[x, y, z])
                    .unwrap_or(&ferrite_data::blocks::default_state("air").unwrap()),
            )
        }
    }

    #[test]
    fn explosion_damage_falls_with_distance_and_respects_cover() {
        let mut world = Grid::new();
        let center = [0.5, 1.0, 0.5];
        let near = AABB::new(1.2, 0.0, 0.2, 1.8, 1.8, 0.8);
        let far = AABB::new(5.2, 0.0, 0.2, 5.8, 1.8, 0.8);
        let near_damage = damage(&world, center, 4.0, Vec3::new(1.5, 1.0, 0.5), near);
        let far_damage = damage(&world, center, 4.0, Vec3::new(5.5, 1.0, 0.5), far);
        assert!(near_damage > far_damage && far_damage > 0.0);
        assert_eq!(
            damage(&world, center, 4.0, Vec3::new(9.0, 1.0, 0.5), far),
            0.0
        );
        for y in -1..=3 {
            for z in -2..=2 {
                world.set([3, y, z], "stone");
            }
        }
        assert!(damage(&world, center, 4.0, Vec3::new(5.5, 1.0, 0.5), far) < far_damage);
    }

    #[test]
    fn blast_destroys_stone_but_obsidian_resists() {
        let mut world = Grid::new();
        for x in -3..=3 {
            for z in -3..=3 {
                world.set([x, -1, z], "stone");
            }
        }
        world.set([1, -1, 0], "obsidian");
        let mut rng = Rng::new(7);
        let destroyed = explode_blocks(&world, [0.5, 0.0, 0.5], TNT_POWER, &mut rng);
        assert!(destroyed.contains(&[0, -1, 0]), "{destroyed:?}");
        assert!(!destroyed.contains(&[1, -1, 0]), "{destroyed:?}");
        assert_eq!(
            resistance_of(ferrite_data::blocks::default_state("obsidian").unwrap()),
            1200.0
        );
        assert!(resistance_of(ferrite_data::blocks::default_state("stone").unwrap()) < 100.0);
    }

    #[test]
    fn a_stronger_blast_reaches_further() {
        let mut world = Grid::new();
        for x in -3..=3 {
            for y in -3..=3 {
                for z in -3..=3 {
                    world.set([x, y, z], "dirt");
                }
            }
        }
        let mut rng = Rng::new(11);
        let weak = explode_blocks(&world, [0.5, 0.5, 0.5], CREEPER_POWER, &mut rng);
        let mut rng = Rng::new(11);
        let strong = explode_blocks(&world, [0.5, 0.5, 0.5], CHARGED_CREEPER_POWER, &mut rng);
        assert!(
            strong.len() > weak.len(),
            "{} <= {}",
            strong.len(),
            weak.len()
        );
    }

    #[test]
    fn exposure_shrinks_behind_a_wall_and_knockback_follows_the_ray() {
        let mut world = Grid::new();
        for x in -2..=2 {
            for y in 0..=3 {
                world.set([x, y, 3], "stone");
            }
        }
        // In front of the wall the entity is fully exposed; behind it the wall hides it.
        let clear = AABB::new(-0.3, 0.0, 1.0, 0.3, 1.8, 1.6);
        let exposed = seen_percent(&world, [0.5, 1.0, 0.5], clear);
        assert_eq!(exposed, 1.0);
        let hidden = AABB::new(-0.3, 0.0, 4.0, 0.3, 1.8, 4.6);
        let blocked = seen_percent(&world, [0.5, 1.0, 0.5], hidden);
        assert!(blocked < 0.2, "{blocked}");
        let push = impulse(
            &world,
            [0.5, 1.0, 0.5],
            TNT_POWER,
            Vec3::new(0.5, 1.0, 1.3),
            clear,
        )
        .expect("inside the blast");
        assert!(push[2] > 0.0);
        assert!(
            impulse(
                &world,
                [0.5, 1.0, 0.5],
                TNT_POWER,
                Vec3::new(0.5, 1.0, 10.0),
                hidden
            )
            .is_none()
        );
    }

    /// The fixture world gives a real chunk to explode in: dirt drops through ferrite-loot and
    /// obsidian's resistance keeps it standing.
    #[tokio::test]
    async fn fixture_world_blast_uses_vanilla_resistance_and_loot() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3");
        let fixture =
            root.join("fixture-world/world/dimensions/minecraft/overworld/region/r.0.0.mca");
        if !fixture.exists() {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("dimensions/minecraft/overworld/region");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy(fixture, dir.join("r.0.0.mca")).unwrap();
        let registry = RuntimeRegistry::load(root.join("generated")).unwrap();
        let source = AnvilChunkSource::new(temp.path().to_path_buf(), registry);
        loop {
            if let Some(result) = source.get(0, 0, 0) {
                result.unwrap();
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let air = u32::from(ferrite_data::blocks::default_state("air").unwrap());
        let dirt = u32::from(ferrite_data::blocks::default_state("dirt").unwrap());
        let obsidian = u32::from(ferrite_data::blocks::default_state("obsidian").unwrap());
        // All in chunk (0, 0): a dirt plate with one obsidian block beside the blast.
        for x in 0..=3 {
            for z in 0..=3 {
                source.set_block(x, 200, z, dirt).unwrap();
                source.set_block(x, 201, z, air).unwrap();
            }
        }
        source.set_block(2, 200, 1, obsidian).unwrap();
        let mut rng = Rng::new(3);
        let destroyed = explode_blocks(&*source, [1.5, 201.0, 1.5], TNT_POWER, &mut rng);
        assert!(destroyed.contains(&[1, 200, 1]), "{destroyed:?}");
        assert!(!destroyed.contains(&[2, 200, 1]), "{destroyed:?}");
        let dirt_id = ferrite_data::registries::id("minecraft:item", "minecraft:dirt").unwrap();
        let drops = super::super::drops::block_drops(
            u16::try_from(dirt).unwrap(),
            &ferrite_item::ItemStack::empty(),
            &mut Rng::new(1),
        );
        assert!(
            drops.iter().any(|stack| stack.item_id == dirt_id),
            "{drops:?}"
        );
    }
}
