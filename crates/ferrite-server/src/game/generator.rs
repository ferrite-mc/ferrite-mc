//! Seam for producing chunks that do not exist on disk yet.
//!
//! The server only knows this trait. The world generator itself is not part of this repository:
//! it is meant to be supplied by a separate component (planned: the official vanilla generator
//! running in the embedded JVM, from the server jar the user downloads). Without one, missing
//! chunks come from the built-in flat test world.
use ferrite_world::{Chunk, LightData};

/// Blocks a placement changed: world position and block state id.
pub type PlacedBlocks = Vec<([i32; 3], u16)>;

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

    /// The nearest block position whose biome is `target` (`id` or `#tag`), within `radius` blocks.
    fn locate_biome(
        &self,
        dimension: super::Dimension,
        target: &str,
        x: i32,
        z: i32,
        radius: i32,
    ) -> anyhow::Result<Option<(i32, i32, i32)>>;

    /// The biome id at a block position.
    fn biome_at(
        &self,
        _dimension: super::Dimension,
        _x: i32,
        _y: i32,
        _z: i32,
    ) -> anyhow::Result<String> {
        anyhow::bail!("this generator has no biome source")
    }

    /// The nearest structure of `target` (`id` or `#tag`) around block `(x, z)`.
    fn locate_structure(
        &self,
        _dimension: super::Dimension,
        _target: &str,
        _x: i32,
        _z: i32,
        _radius: i32,
    ) -> anyhow::Result<Option<(i32, i32, i32)>> {
        anyhow::bail!("this generator has no structures")
    }

    /// Runs the vanilla `/place feature|jigsaw|structure` described by `argument`
    /// (`kind;...;x;y;z`) and returns the blocks it changed as `(position, state id)`.
    fn place(&self, _dimension: super::Dimension, _argument: &str) -> anyhow::Result<PlacedBlocks> {
        anyhow::bail!("this generator cannot place features or structures")
    }

    /// The nearest point of interest of type `target` (`id` or `#tag`) within `radius` blocks.
    fn locate_poi(
        &self,
        _dimension: super::Dimension,
        _target: &str,
        _x: i32,
        _y: i32,
        _z: i32,
        _radius: i32,
    ) -> anyhow::Result<Option<(i32, i32, i32)>> {
        anyhow::bail!("this generator has no points of interest")
    }

    /// Whether a piece of structure `id` covers the block.
    fn structure_at(
        &self,
        _dimension: super::Dimension,
        _id: &str,
        _x: i32,
        _y: i32,
        _z: i32,
    ) -> anyhow::Result<bool> {
        anyhow::bail!("this generator has no structures")
    }
}
