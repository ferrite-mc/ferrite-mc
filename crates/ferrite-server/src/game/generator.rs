//! Seam for producing chunks that do not exist on disk yet.
//!
//! The server only knows this trait. The world generator itself is not part of this repository:
//! it is meant to be supplied by a separate component (planned: the official vanilla generator
//! running in the embedded JVM, from the server jar the user downloads). Without one, missing
//! chunks come from the built-in flat test world.
use ferrite_world::{Chunk, LightData};

/// A generated column plus the light data the wire encoder needs.
pub struct Generated {
    pub chunk: Chunk,
    pub light: LightData,
}

/// Generates columns, spawn heights and biome lookups for newly requested chunks.
///
/// Implementations run on blocking threads, never on a region tick.
pub trait ChunkGenerator: Send + Sync {
    /// Generates the full column at chunk coordinates `(x, z)` for `dimension`.
    fn generate(&self, dimension: super::Dimension, x: i32, z: i32) -> anyhow::Result<Generated>;

    /// The first free block above `dimension`'s generated surface at block `(x, z)`.
    fn spawn_height(&self, dimension: super::Dimension, x: i32, z: i32) -> anyhow::Result<i32>;

    /// The nearest block position whose surface biome is `target`, within `radius` blocks.
    fn locate_biome(
        &self,
        target: &str,
        x: i32,
        z: i32,
        radius: i32,
    ) -> anyhow::Result<Option<(i32, i32)>>;
}
