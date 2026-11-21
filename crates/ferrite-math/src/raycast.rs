use crate::EPSILON;
use crate::aabb::{AABB, ClipHit};
use crate::direction::{Axis, Direction};
use crate::pos::BlockPos;
use crate::vec3::Vec3;

/// The first block hit by a ray, with the face and world-space hit point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockHit {
    pub pos: BlockPos,
    pub face: Direction,
    pub location: Vec3,
}

/// Casts a ray from `eye` along `look` for up to `max_distance` blocks,
/// visiting voxels in DDA order like vanilla's `BlockGetter.clip`.
///
/// `shape_at` returns the block-local collision boxes for a position, or
/// `None` if the block has no shape.
pub fn raycast<'a, F>(eye: Vec3, look: Vec3, max_distance: f64, shape_at: F) -> Option<BlockHit>
where
    F: Fn(BlockPos) -> Option<&'a [AABB]>,
{
    raycast_shapes(eye, look, max_distance, |pos| shape_at(pos).unwrap_or(&[]))
}

/// Raycasts through block voxels using a lazy shape callback that can return borrowed or inline
/// collision slices (avoiding upfront 3D bounding-box `HashMap` scans).
pub fn raycast_shapes<F, S>(
    eye: Vec3,
    look: Vec3,
    max_distance: f64,
    mut shape_at: F,
) -> Option<BlockHit>
where
    F: FnMut(BlockPos) -> S,
    S: AsRef<[AABB]>,
{
    if !max_distance.is_finite() || max_distance <= 0.0 {
        return None;
    }

    let direction = look.normalize();
    if direction == Vec3::ZERO {
        return None;
    }

    let end = eye + direction * max_distance;
    let mut pos = BlockPos::containing(eye);
    let end_pos = BlockPos::containing(end);

    let mut step = [0_i32; 3];
    let mut t_max = [f64::INFINITY; 3];
    let mut t_delta = [f64::INFINITY; 3];

    for (index, axis) in [Axis::X, Axis::Y, Axis::Z].into_iter().enumerate() {
        let component = direction.component(axis);
        let origin = eye.component(axis);
        if component > EPSILON {
            step[index] = 1;
            t_delta[index] = 1.0 / component;
            t_max[index] = (1.0 - frac(origin)) * t_delta[index];
        } else if component < -EPSILON {
            step[index] = -1;
            t_delta[index] = -1.0 / component;
            t_max[index] = frac(origin) * t_delta[index];
        }
    }

    loop {
        let boxes = shape_at(pos);
        let boxes_slice = boxes.as_ref();
        if !boxes_slice.is_empty() {
            let offset = Vec3::new(f64::from(pos.x), f64::from(pos.y), f64::from(pos.z));
            let mut nearest: Option<(f64, ClipHit)> = None;

            for shape in boxes_slice {
                let world = shape.move_by(offset);
                if let Some(hit) = world.clip(eye, end) {
                    let distance = (hit.location - eye).length();
                    if distance <= max_distance + EPSILON
                        && nearest.is_none_or(|(best, _)| distance < best - EPSILON)
                    {
                        nearest = Some((distance, hit));
                    }
                }
            }

            if let Some((_, hit)) = nearest {
                return Some(BlockHit {
                    pos,
                    face: hit.face,
                    location: hit.location,
                });
            }
        }

        if pos == end_pos {
            return None;
        }

        let axis = if t_max[0] <= t_max[1] && t_max[0] <= t_max[2] {
            0
        } else if t_max[1] <= t_max[2] {
            1
        } else {
            2
        };

        if t_max[axis] > max_distance + EPSILON {
            return None;
        }

        match axis {
            0 => pos.x += step[0],
            1 => pos.y += step[1],
            _ => pos.z += step[2],
        }
        t_max[axis] += t_delta[axis];
    }
}

fn frac(value: f64) -> f64 {
    value - value.floor()
}
