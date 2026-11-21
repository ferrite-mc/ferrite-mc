//! Game math shared by movement validation, block interaction and entity
//! physics: vectors, block/section coordinates, directions, axis-aligned
//! boxes, voxel collision with step-up, and block raycasts.
//!
//! Behaviour mirrors the vanilla helpers it replaces: collision resolution
//! applies the usual `1e-7` epsilon, moves the box along Y first and then
//! along the larger horizontal axis, and block raycasts walk voxels in DDA
//! order.

pub mod aabb;
pub mod direction;
pub mod mth;
pub mod pos;
pub mod raycast;
pub mod shape;
pub mod vec3;
pub mod view;

pub use aabb::{AABB, ClipHit};
pub use direction::{Axis, Direction};
pub use pos::{BlockPos, ChunkPos, SectionPos};
pub use raycast::{BlockHit, raycast, raycast_shapes};
pub use shape::{PLAYER_MAX_UP_STEP, VoxelShape, collide, collide_with_step};
pub use vec3::Vec3;
pub use view::{look_from_yaw_pitch, yaw_pitch_from_look};

/// Tolerance used wherever vanilla applies its `1e-7` epsilon.
pub const EPSILON: f64 = 1e-7;
