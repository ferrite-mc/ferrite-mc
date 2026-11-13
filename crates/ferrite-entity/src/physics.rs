//! Entity movement against block collision shapes (vanilla `Entity.move` for simple entities).

use crate::store::{EntityStore, Flags};
use crate::{EntityKey, rng::Rng};
use ferrite_data::block_props::{self, FluidKind};
use ferrite_math::{AABB, EPSILON, Vec3, collide_with_step};

/// Read access to block states. `None` means the chunk is not loaded; it collides like a full
/// block, so entities never fall into unloaded terrain.
pub trait BlockGetter {
    fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16>;

    /// The light level at a block (vanilla `getMaxLocalRawBrightness`, 0..=15), when the world
    /// has light data. `None` means unknown, which light-dependent goals treat as dark.
    fn brightness(&self, _x: i32, _y: i32, _z: i32) -> Option<u8> {
        None
    }
}

fn state_props(state: Option<u16>) -> Option<&'static block_props::BlockProps> {
    state
        .filter(|s| usize::from(*s) < ferrite_data::blocks::STATE_COUNT)
        .map(block_props::get)
}

const FULL: [[f64; 6]; 1] = [[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]];

fn shape(state: Option<u16>) -> &'static [[f64; 6]] {
    match state.filter(|s| usize::from(*s) < ferrite_data::blocks::STATE_COUNT) {
        Some(state) => block_props::collision_shape(state),
        None => &FULL,
    }
}

/// World-space collision boxes of the blocks `area` can touch. Like vanilla's
/// `BlockCollisions`, one extra block is scanned below for shapes taller than a block.
pub fn collision_boxes(blocks: &impl BlockGetter, area: AABB) -> Vec<AABB> {
    let lo = |v: f64| (v - EPSILON).floor() as i32;
    let hi = |v: f64| (v + EPSILON).floor() as i32;
    let mut out = Vec::new();
    for x in lo(area.min.x)..=hi(area.max.x) {
        for z in lo(area.min.z)..=hi(area.max.z) {
            for y in lo(area.min.y) - 1..=hi(area.max.y) {
                let offset = Vec3::new(f64::from(x), f64::from(y), f64::from(z));
                for b in shape(blocks.block_state(x, y, z)) {
                    let world = AABB::new(b[0], b[1], b[2], b[3], b[4], b[5]).move_by(offset);
                    if world.intersects(area) {
                        out.push(world);
                    }
                }
            }
        }
    }
    out
}

/// Whether `area` overlaps no block collision box (vanilla `noCollision`).
pub fn no_collision(blocks: &impl BlockGetter, area: AABB) -> bool {
    collision_boxes(blocks, area).is_empty()
}

/// Vanilla `Mth.equal`.
fn mth_equal(a: f64, b: f64) -> bool {
    (b - a).abs() < f64::from(1.0E-5_f32)
}

fn block_pos(v: Vec3) -> (i32, i32, i32) {
    (v.x.floor() as i32, v.y.floor() as i32, v.z.floor() as i32)
}

/// Vanilla `getOnPos(offset)` without a supporting-block search: the block `offset` below the
/// feet.
pub fn on_pos(pos: Vec3, offset: f64) -> (i32, i32, i32) {
    block_pos(Vec3::new(pos.x, pos.y - offset, pos.z))
}

/// Friction of the block at `at`; unloaded blocks count as ordinary blocks.
pub fn friction(blocks: &impl BlockGetter, at: (i32, i32, i32)) -> f64 {
    state_props(blocks.block_state(at.0, at.1, at.2)).map_or(0.6, |p| f64::from(p.friction))
}

fn speed_factor(blocks: &impl BlockGetter, at: (i32, i32, i32)) -> f64 {
    state_props(blocks.block_state(at.0, at.1, at.2)).map_or(1.0, |p| f64::from(p.speed_factor))
}

/// How deep `area` reaches into water (or lava), vanilla's `getFluidHeight` without flow.
pub fn fluid_height(blocks: &impl BlockGetter, area: AABB, lava: bool) -> f64 {
    let is = |state: Option<u16>| {
        state_props(state)
            .and_then(|p| p.fluid)
            .and_then(|(kind, amount, _)| {
                let matches = match kind {
                    FluidKind::Water | FluidKind::FlowingWater => !lava,
                    FluidKind::Lava | FluidKind::FlowingLava => lava,
                    FluidKind::Other(_) => false,
                };
                matches.then_some(amount)
            })
    };
    let mut height: f64 = 0.0;
    let (x0, y0, z0) = block_pos(area.min);
    let (x1, y1, z1) = block_pos(area.max);
    for x in x0..=x1 {
        for z in z0..=z1 {
            for y in y0..=y1 {
                let Some(amount) = is(blocks.block_state(x, y, z)) else {
                    continue;
                };
                let full = is(blocks.block_state(x, y + 1, z)).is_some();
                let top = f64::from(y) + if full { 1.0 } else { f64::from(amount) / 9.0 };
                if top >= area.min.y {
                    height = height.max(top - area.min.y);
                }
            }
        }
    }
    height
}

/// Moves an entity by `delta`, colliding with blocks (vanilla `Entity.move` with
/// `MoverType.SELF`, no step-up): updates position, collision and on-ground flags, zeroes the
/// velocity on blocked axes (restitution 0) and applies the block speed factor.
pub fn move_entity(
    store: &mut EntityStore,
    key: EntityKey,
    delta: Vec3,
    blocks: &impl BlockGetter,
) {
    let (Some(pos), Some(bbox), Some(mut flags)) =
        (store.pos(key), store.bbox(key), store.flags(key))
    else {
        return;
    };
    if flags.has(Flags::NO_PHYSICS) {
        store.set_pos(key, pos + delta);
        flags.set(Flags::HORIZONTAL_COLLISION, false);
        flags.set(Flags::VERTICAL_COLLISION, false);
        *store.flags_mut(key).unwrap() = flags;
        return;
    }
    let shapes = collision_boxes(blocks, bbox.expand_towards(delta));
    let movement = collide_with_step(bbox, delta, &shapes, 0.0);
    let length = movement.length_sqr();
    let mut pos = pos;
    if length > 1.0E-7 || delta.length_sqr() - length < 1.0E-7 {
        pos += movement;
        store.set_pos(key, pos);
    }
    let x_collision = !mth_equal(delta.x, movement.x);
    let z_collision = !mth_equal(delta.z, movement.z);
    let horizontal = x_collision || z_collision;
    flags.set(Flags::HORIZONTAL_COLLISION, horizontal);
    let moved_vertically = delta.y.abs() > 0.0;
    let mut vertical = flags.has(Flags::VERTICAL_COLLISION);
    if moved_vertically {
        vertical = delta.y != movement.y;
        flags.set(Flags::VERTICAL_COLLISION, vertical);
        flags.set(Flags::ON_GROUND, vertical && delta.y < 0.0);
    }
    let mut vel = store.vel(key).unwrap_or_default();
    if moved_vertically && vertical || horizontal {
        // Restitution 0: blocked axes stop. (Bouncy blocks are not modelled yet.)
        if x_collision {
            vel.x = 0.0;
        }
        if z_collision {
            vel.z = 0.0;
        }
        if vertical {
            vel.y = 0.0;
        }
    }
    let here = speed_factor(blocks, block_pos(pos));
    let factor = if here == 1.0 {
        speed_factor(blocks, on_pos(pos, 0.999_999))
    } else {
        here
    };
    vel.x *= factor;
    vel.z *= factor;
    store.set_vel(key, vel);
    *store.flags_mut(key).unwrap() = flags;
}

/// Vanilla `moveTowardsClosestSpace`: pushes an entity stuck in a block toward the nearest
/// open face.
pub fn move_towards_closest_space(
    store: &mut EntityStore,
    key: EntityKey,
    at: Vec3,
    blocks: &impl BlockGetter,
    rng: &mut Rng,
) {
    let (x, y, z) = block_pos(at);
    let local = at - Vec3::new(f64::from(x), f64::from(y), f64::from(z));
    // North, south, west, east, up: (axis, step, offset).
    let directions = [
        (2, -1, (0, 0, -1)),
        (2, 1, (0, 0, 1)),
        (0, -1, (-1, 0, 0)),
        (0, 1, (1, 0, 0)),
        (1, 1, (0, 1, 0)),
    ];
    let mut best = (1, 1);
    let mut closest = f64::MAX;
    for (axis, step, (dx, dy, dz)) in directions {
        let full = state_props(blocks.block_state(x + dx, y + dy, z + dz))
            .is_none_or(|p| p.full_collision);
        if !full {
            let d = [local.x, local.y, local.z][axis];
            let oriented = if step > 0 { 1.0 - d } else { d };
            if oriented < closest {
                closest = oriented;
                best = (axis, step);
            }
        }
    }
    let speed = f64::from(rng.next_f32() * 0.2 + 0.1) * f64::from(best.1);
    let scaled = store.vel(key).unwrap_or_default() * 0.75;
    let vel = match best.0 {
        0 => Vec3::new(speed, scaled.y, scaled.z),
        1 => Vec3::new(scaled.x, speed, scaled.z),
        _ => Vec3::new(scaled.x, scaled.y, speed),
    };
    store.set_vel(key, vel);
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Stone below `floor`, air above, plus explicit overrides.
    pub struct TestWorld {
        pub floor: i32,
        pub blocks: HashMap<(i32, i32, i32), u16>,
    }

    impl TestWorld {
        pub fn flat(floor: i32) -> Self {
            Self {
                floor,
                blocks: HashMap::new(),
            }
        }
    }

    impl BlockGetter for TestWorld {
        fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
            if let Some(state) = self.blocks.get(&(x, y, z)) {
                return Some(*state);
            }
            let name = if y < self.floor { "stone" } else { "air" };
            ferrite_data::blocks::default_state(name)
        }
    }

    #[test]
    fn boxes_and_fluids() {
        let world = TestWorld::flat(64);
        let above = AABB::new(0.2, 64.0, 0.2, 0.4, 64.2, 0.4);
        assert!(no_collision(&world, above));
        assert!(!no_collision(
            &world,
            above.move_by(Vec3::new(0.0, -0.5, 0.0))
        ));
        let mut wet = TestWorld::flat(64);
        let water = ferrite_data::blocks::default_state("water").unwrap();
        wet.blocks.insert((0, 64, 0), water);
        let depth = fluid_height(&wet, above, false);
        assert!(depth > 0.1, "{depth}");
        assert_eq!(fluid_height(&wet, above, true), 0.0);
        assert!((friction(&world, (0, 63, 0)) - 0.6).abs() < 1e-6);
    }
}
