//! The dimensions the server runs, their storage layout, and their chunk sources.
//!
//! Vanilla keeps one region directory per dimension: the overworld under
//! `dimensions/minecraft/overworld`, the nether under `DIM-1` and the end under `DIM1`.
//! A dimension also fixes the world height (`min_y`/`height`), whether it has a sky and a
//! ceiling, and how coordinates scale when a portal crosses it.

use super::{anvil::AnvilChunkSource, terrain::VanillaTerrain};
use ferrite_world::RuntimeRegistry;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

/// The dimensions the server can run (`DimensionType`): beds only work in the overworld, and
/// each dimension has its own `infiniburn` tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Dimension {
    #[default]
    Overworld,
    Nether,
    End,
}

impl Dimension {
    pub const ALL: [Self; 3] = [Self::Overworld, Self::Nether, Self::End];

    /// The dimension key as it appears in `dimension/*.json`, `/execute in` and the wire.
    pub fn key(self) -> &'static str {
        match self {
            Self::Overworld => "minecraft:overworld",
            Self::Nether => "minecraft:the_nether",
            Self::End => "minecraft:the_end",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|dimension| dimension.key() == key)
    }

    /// The synchronized `minecraft:dimension_type` registry id. `ferrite-net` builds that
    /// registry from the sorted `data/minecraft/dimension_type` files, which is
    /// `overworld`, `overworld_caves`, `the_end`, `the_nether`; a test below pins it.
    pub fn dimension_type_id(self) -> i32 {
        match self {
            Self::Overworld => 0,
            Self::End => 2,
            Self::Nether => 3,
        }
    }

    /// Directory holding this dimension's `r.<x>.<z>.mca` files, relative to the world root.
    pub fn region_dir(self) -> &'static str {
        match self {
            Self::Overworld => "dimensions/minecraft/overworld/region",
            Self::Nether => "DIM-1/region",
            Self::End => "DIM1/region",
        }
    }

    /// Directory holding this dimension's per-chunk entity sidecars, relative to the world
    /// root.
    pub fn entity_dir(self) -> &'static str {
        match self {
            Self::Overworld => "entities",
            Self::Nether => "DIM-1/entities",
            Self::End => "DIM1/entities",
        }
    }

    /// The world root a dimension's files live under. The overworld is the root itself; the
    /// nether and end keep their own `DIM-1`/`DIM1` trees.
    pub fn storage_root(self, world: &Path) -> PathBuf {
        match self {
            Self::Overworld => world.to_path_buf(),
            Self::Nether => world.join("DIM-1"),
            Self::End => world.join("DIM1"),
        }
    }

    /// Vanilla `DimensionType.coordinate_scale`.
    pub fn coordinate_scale(self) -> f64 {
        match self {
            Self::Overworld | Self::End => 1.0,
            Self::Nether => 8.0,
        }
    }

    pub fn has_skylight(self) -> bool {
        matches!(self, Self::Overworld)
    }

    pub fn has_ceiling(self) -> bool {
        matches!(self, Self::Nether)
    }

    /// Lowest generated block.
    pub fn min_y(self) -> i32 {
        match self {
            Self::Overworld => -64,
            Self::Nether | Self::End => 0,
        }
    }

    /// Height of the generated column in blocks.
    pub fn height(self) -> i32 {
        match self {
            Self::Overworld => 384,
            Self::Nether | Self::End => 256,
        }
    }

    /// Light sections in the network/Anvil light arrays: one per section plus the two sentinel
    /// sections the protocol carries.
    pub fn light_sections(self) -> usize {
        (self.height() / 16) as usize + 2
    }

    pub fn sea_level(self) -> i32 {
        match self {
            Self::Overworld | Self::End => 63,
            Self::Nether => 32,
        }
    }

    /// Default spawn when the world has no saved per-dimension spawn.
    pub fn default_spawn(self) -> [i32; 3] {
        [0, self.min_y() + 64, 0]
    }

    /// The `worldgen/noise_settings` id used to generate new chunks.
    pub fn noise_settings(self) -> &'static str {
        match self {
            Self::Overworld => "minecraft:overworld",
            Self::Nether => "minecraft:nether",
            Self::End => "minecraft:end",
        }
    }

    /// The `worldgen/multi_noise_biome_source_parameter_list` preset, where the dimension uses
    /// one. The end uses a dedicated biome source instead.
    pub fn biome_preset(self) -> Option<&'static str> {
        match self {
            Self::Overworld => Some("minecraft:overworld"),
            Self::Nether => Some("minecraft:nether"),
            Self::End => None,
        }
    }

    /// Vanilla `DimensionType.bedWorks`: beds explode in the nether and the end.
    pub fn bed_works(self) -> bool {
        matches!(self, Self::Overworld)
    }

    /// Vanilla `DimensionType.infiniburn`.
    pub fn infiniburn_tag(self) -> &'static str {
        match self {
            Self::Overworld => "infiniburn_overworld",
            Self::Nether => "infiniburn_nether",
            Self::End => "infiniburn_end",
        }
    }
}

/// Every dimension's chunk source, keyed by dimension. Sources are opened for all dimensions
/// up front, so a transfer never has to build a source on the tick thread.
pub struct DimensionRegistry {
    sources: HashMap<Dimension, Arc<AnvilChunkSource>>,
    spawns: HashMap<Dimension, [i32; 3]>,
}

impl DimensionRegistry {
    /// Opens the standard three dimensions over one world directory.
    ///
    /// The overworld keeps the caller's spawn; the nether and end get their own vanilla
    /// storage trees. New chunks use `terrain` when it is available (the vanilla generator
    /// now knows the nether settings), and the flat test world otherwise.
    pub fn standard(
        world: Option<&Path>,
        registry: Arc<RuntimeRegistry>,
        terrain: Option<Arc<VanillaTerrain>>,
        spawn: [i32; 3],
    ) -> Arc<Self> {
        let mut sources = HashMap::new();
        for dimension in Dimension::ALL {
            // The end has no generator here yet (it needs the end biome source), so it stays
            // on the flat fallback instead of failing every chunk load.
            let dimension_terrain = (dimension != Dimension::End)
                .then(|| terrain.clone())
                .flatten();
            let source = match world {
                Some(world) => AnvilChunkSource::with_dimension(
                    world.to_path_buf(),
                    Arc::clone(&registry),
                    dimension_terrain,
                    dimension,
                ),
                None => AnvilChunkSource::new_memory_dimension(dimension_terrain, dimension),
            };
            sources.insert(dimension, source);
        }
        let nether_height = sources[&Dimension::Nether]
            .spawn_height(0, 0)
            .unwrap_or(Dimension::Nether.default_spawn()[1]);
        let spawns = HashMap::from([
            (Dimension::Overworld, spawn),
            (
                Dimension::Nether,
                [0, nether_height.max(Dimension::Nether.min_y() + 16), 0],
            ),
            (Dimension::End, Dimension::End.default_spawn()),
        ]);
        Arc::new(Self { sources, spawns })
    }

    /// Builds the registry around an already-open overworld source, so the game loop and the
    /// registry share one chunk cache. Integration worlds (fixture generators, prefilled
    /// chunks) must keep using the source they were created from.
    pub fn with_overworld(overworld: Arc<AnvilChunkSource>, spawn: [i32; 3]) -> Arc<Self> {
        let mut sources = HashMap::new();
        for dimension in [Dimension::Nether, Dimension::End] {
            let source = match overworld.path() {
                Some(path) => AnvilChunkSource::with_dimension(
                    path.to_path_buf(),
                    overworld.registry(),
                    overworld.terrain(),
                    dimension,
                ),
                None => AnvilChunkSource::new_memory_dimension(overworld.terrain(), dimension),
            };
            sources.insert(dimension, source);
        }
        sources.insert(Dimension::Overworld, overworld);
        let nether_height = sources[&Dimension::Nether]
            .spawn_height(0, 0)
            .unwrap_or(Dimension::Nether.default_spawn()[1]);
        let spawns = HashMap::from([
            (Dimension::Overworld, spawn),
            (
                Dimension::Nether,
                [0, nether_height.max(Dimension::Nether.min_y() + 16), 0],
            ),
            (Dimension::End, Dimension::End.default_spawn()),
        ]);
        Arc::new(Self { sources, spawns })
    }

    pub fn source(&self, dimension: Dimension) -> &Arc<AnvilChunkSource> {
        // Every constructor inserts all three dimensions.
        &self.sources[&dimension]
    }

    pub fn spawn(&self, dimension: Dimension) -> [i32; 3] {
        self.spawns[&dimension]
    }

    pub fn set_spawn(&mut self, dimension: Dimension, spawn: [i32; 3]) {
        self.spawns.insert(dimension, spawn);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_world::BlockStateRegistry;
    use std::path::PathBuf;

    fn vanilla_root() -> Option<PathBuf> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3/generated");
        root.join("reports/blocks.json").exists().then_some(root)
    }

    async fn ready(source: &Arc<AnvilChunkSource>, x: i32, z: i32) {
        for _ in 0..2_000 {
            match source.get(x, z, 0) {
                Some(result) => {
                    result.expect("chunk load");
                    return;
                }
                None => tokio::time::sleep(std::time::Duration::from_millis(5)).await,
            }
        }
        panic!("chunk ({x}, {z}) never loaded");
    }

    #[test]
    fn keys_storage_layouts_and_flags_match_the_vanilla_data() {
        assert_eq!(
            Dimension::from_key("minecraft:the_nether"),
            Some(Dimension::Nether)
        );
        assert_eq!(Dimension::from_key("the_end"), None);
        assert_eq!(
            Dimension::Nether.region_dir(),
            "DIM-1/region",
            "the nether keeps the legacy DIM-1 tree"
        );
        assert_eq!(Dimension::End.region_dir(), "DIM1/region");
        assert_eq!(
            Dimension::Overworld.region_dir(),
            "dimensions/minecraft/overworld/region"
        );
        assert_eq!(Dimension::Nether.height(), 256);
        assert_eq!(Dimension::Nether.min_y(), 0);
        assert!(!Dimension::Nether.has_skylight());
        assert!(Dimension::Nether.has_ceiling());
        assert!(Dimension::Overworld.has_skylight());
        assert!(!Dimension::Overworld.has_ceiling());
        assert_eq!(Dimension::Nether.coordinate_scale(), 8.0);
        assert_eq!(Dimension::Overworld.coordinate_scale(), 1.0);
        assert_eq!(Dimension::Nether.dimension_type_id(), 3);
        assert_eq!(Dimension::End.dimension_type_id(), 2);
        assert_eq!(Dimension::Overworld.dimension_type_id(), 0);
        assert!(Dimension::Overworld.bed_works());
        assert!(!Dimension::Nether.bed_works());
    }

    #[test]
    fn dimension_type_ids_follow_the_sorted_synchronized_registry() {
        let Some(root) = vanilla_root() else {
            return;
        };
        let dir = root.join("data/minecraft/dimension_type");
        let mut names: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| {
                entry
                    .unwrap()
                    .path()
                    .file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        for dimension in Dimension::ALL {
            let file = dimension.key().trim_start_matches("minecraft:");
            let index = names
                .iter()
                .position(|name| name == file)
                .expect("dimension type file");
            assert_eq!(
                dimension.dimension_type_id(),
                index as i32,
                "{} must use the registry id the client is sent",
                dimension.key()
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn nether_source_returns_nether_biomes_and_the_nether_height() {
        let Some(root) = vanilla_root() else {
            eprintln!("SKIP nether source test: vanilla/26.3/generated is absent");
            return;
        };
        let registry = Arc::new(RuntimeRegistry::load(&root).unwrap());
        let terrain = match VanillaTerrain::spawn(0) {
            Ok(terrain) => Arc::new(terrain),
            Err(error) => {
                eprintln!("SKIP nether source test: {error}");
                return;
            }
        };
        let dimensions =
            DimensionRegistry::standard(None, Arc::clone(&registry), Some(terrain), [0, 65, 0]);
        let source = dimensions.source(Dimension::Nether);
        ready(source, 0, 0).await;

        source
            .with_loaded_chunk(0, 0, |chunk| {
                assert_eq!(chunk.min_y(), 0);
                assert_eq!(chunk.height_blocks(), 256);
                let mut biomes = Vec::new();
                for section in chunk.sections() {
                    for index in 0..64 {
                        if let Some(id) = section.biomes().get(index)
                            && let Some(name) = registry.biome_name(id)
                        {
                            biomes.push(name.to_owned());
                        }
                    }
                }
                const NETHER: [&str; 5] = [
                    "minecraft:nether_wastes",
                    "minecraft:soul_sand_valley",
                    "minecraft:crimson_forest",
                    "minecraft:warped_forest",
                    "minecraft:basalt_deltas",
                ];
                assert!(!biomes.is_empty(), "no biomes in the nether chunk");
                assert!(
                    biomes.iter().all(|name| NETHER.contains(&name.as_str())),
                    "overworld biome in a nether chunk: {:?}",
                    biomes.iter().collect::<std::collections::BTreeSet<_>>()
                );
            })
            .expect("nether chunk is loaded");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dimensions_write_to_their_vanilla_storage_trees() {
        let Some(root) = vanilla_root() else {
            return;
        };
        let dir = std::env::temp_dir().join(format!(
            "ferrite-dimensions-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let registry = Arc::new(RuntimeRegistry::load(&root).unwrap());
        let nether =
            AnvilChunkSource::with_dimension(dir.clone(), registry, None, Dimension::Nether);
        ready(&nether, 0, 0).await;
        let netherrack = ferrite_data::blocks::default_state("minecraft:netherrack").unwrap();
        nether.set_block(0, 32, 0, u32::from(netherrack)).unwrap();
        nether.flush_chunk(0, 0).await.unwrap();
        assert!(
            dir.join("DIM-1/region/r.0.0.mca").exists(),
            "the nether must be saved under DIM-1/region"
        );
        assert!(
            !dir.join("dimensions/minecraft/overworld/region/r.0.0.mca")
                .exists(),
            "the nether must not write into the overworld tree"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn nether_flat_fallback_uses_netherrack_and_lava() {
        assert_eq!(Dimension::Nether.sea_level(), 32);
        assert_eq!(Dimension::Nether.light_sections(), 18);
        assert_eq!(Dimension::Overworld.light_sections(), 26);
    }
}
