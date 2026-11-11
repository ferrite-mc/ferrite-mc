//! The dimensions the server runs, their storage layout, and their chunk sources.
//!
//! Vanilla keeps one region directory per dimension: the overworld under
//! `dimensions/minecraft/overworld`, the nether under `DIM-1` and the end under `DIM1`.
//! A dimension also fixes the world height (`min_y`/`height`), whether it has a sky and a
//! ceiling, and how coordinates scale when a portal crosses it.

use super::{anvil::AnvilChunkSource, generator::ChunkGenerator};
use ferrite_world::RuntimeRegistry;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Stable metadata for a non-vanilla world. This catalog is the identity/storage seam for
/// runtime worlds; region scheduling still uses `Dimension` and must be migrated before these
/// descriptors can be exposed through Bukkit's `Server#createWorld`.
#[derive(Clone)]
pub struct RuntimeWorldDescriptor {
    pub id: uuid::Uuid,
    pub name: String,
    pub key: String,
    pub spawn: [i32; 3],
    pub dimension: Dimension,
    /// The world seed. An existing world keeps the seed in its `level.dat`; the seed a caller
    /// requests only applies when the world is first created.
    pub seed: i64,
    pub source: Arc<AnvilChunkSource>,
}

#[derive(Clone, Copy)]
struct RuntimeWorldSettings {
    spawn: [i32; 3],
    dimension: Dimension,
    seed: i64,
}

/// What a caller asks for when opening a runtime world. An existing world keeps the spawn and
/// seed stored in its `level.dat`; these only apply when the world is first created.
#[derive(Clone, Copy)]
pub struct RuntimeWorldRequest {
    pub dimension: Dimension,
    pub spawn: [i32; 3],
    pub seed: i64,
}

/// Owns runtime-world identity and disk-backed chunk sources. Names are unique ignoring case.
#[derive(Default)]
pub struct RuntimeWorldCatalog {
    by_id: HashMap<uuid::Uuid, RuntimeWorldDescriptor>,
    by_name: HashMap<String, uuid::Uuid>,
    by_key: HashMap<String, uuid::Uuid>,
}

/// Validate a single portable directory component and derive a valid Minecraft resource key.
fn runtime_world_identity(name: &str) -> Result<(String, String), &'static str> {
    let name = name.trim();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.ends_with('.')
        || name.chars().any(|c| {
            c.is_control() || matches!(c, '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*')
        })
    {
        return Err("invalid world name");
    }
    // Windows treats these device names as special even when followed by an extension.
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
    {
        return Err("invalid world name");
    }
    let folded = name.to_lowercase();
    let mut path = String::new();
    for byte in folded.bytes() {
        match byte {
            b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.' => path.push(char::from(byte)),
            b' ' => path.push('_'),
            _ => {
                use std::fmt::Write;
                write!(path, "_{byte:02x}").expect("writing to a String cannot fail");
            }
        }
    }
    Ok((folded, format!("ferrite:{path}")))
}

impl RuntimeWorldCatalog {
    /// Register a world backed by its own Anvil directory. This does not register ticking regions.
    pub fn create(
        &mut self,
        id: uuid::Uuid,
        name: &str,
        root: &Path,
        registry: Arc<RuntimeRegistry>,
        terrain: Option<Arc<dyn ChunkGenerator>>,
        spawn: [i32; 3],
    ) -> Result<RuntimeWorldDescriptor, &'static str> {
        self.create_for_dimension(
            id,
            name,
            root,
            registry,
            terrain,
            RuntimeWorldSettings {
                spawn,
                dimension: Dimension::Overworld,
                seed: 0,
            },
        )
    }

    fn create_for_dimension(
        &mut self,
        id: uuid::Uuid,
        name: &str,
        root: &Path,
        registry: Arc<RuntimeRegistry>,
        terrain: Option<Arc<dyn ChunkGenerator>>,
        settings: RuntimeWorldSettings,
    ) -> Result<RuntimeWorldDescriptor, &'static str> {
        let name = name.trim();
        let (folded, key) = runtime_world_identity(name)?;
        if self.by_id.contains_key(&id)
            || self.by_name.contains_key(&folded)
            || self.by_key.contains_key(&key)
        {
            return Err("world id, name or key already loaded");
        }
        let source = AnvilChunkSource::with_dimension(
            root.join(name),
            registry,
            terrain,
            settings.dimension,
        );
        let descriptor = RuntimeWorldDescriptor {
            id,
            name: name.to_owned(),
            key: key.clone(),
            spawn: settings.spawn,
            dimension: settings.dimension,
            seed: settings.seed,
            source,
        };
        self.by_name.insert(folded, id);
        self.by_key.insert(key, id);
        self.by_id.insert(id, descriptor.clone());
        Ok(descriptor)
    }

    pub fn get(&self, id: uuid::Uuid) -> Option<&RuntimeWorldDescriptor> {
        self.by_id.get(&id)
    }
    pub fn by_name(&self, name: &str) -> Option<&RuntimeWorldDescriptor> {
        self.by_name
            .get(&name.trim().to_lowercase())
            .and_then(|id| self.by_id.get(id))
    }
    pub fn unload(&mut self, id: uuid::Uuid) -> Option<RuntimeWorldDescriptor> {
        let descriptor = self.by_id.remove(&id)?;
        self.by_name.remove(&descriptor.name.to_lowercase());
        self.by_key.remove(&descriptor.key);
        Some(descriptor)
    }

    /// Open or create an independent world, preserving its Bukkit UUID and spawn on restart.
    pub fn open_persistent(
        &mut self,
        name: &str,
        root: &Path,
        registry: Arc<RuntimeRegistry>,
        terrain: Option<Arc<dyn ChunkGenerator>>,
        request: RuntimeWorldRequest,
    ) -> anyhow::Result<RuntimeWorldDescriptor> {
        use std::io::Write;
        let RuntimeWorldRequest {
            dimension,
            spawn: initial_spawn,
            seed: requested_seed,
        } = request;
        let name = name.trim();
        let (folded, key) = runtime_world_identity(name).map_err(anyhow::Error::msg)?;
        anyhow::ensure!(
            !self.by_name.contains_key(&folded) && !self.by_key.contains_key(&key),
            "world name or key already loaded"
        );
        let world_root = root.join(name);
        std::fs::create_dir_all(&world_root)?;
        let uid_path = world_root.join("uid.dat");
        let id = match std::fs::read(&uid_path) {
            Ok(bytes) => {
                anyhow::ensure!(bytes.len() == 16, "invalid uid.dat length");
                uuid::Uuid::from_slice(&bytes)?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let id = uuid::Uuid::new_v4();
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&uid_path)?;
                file.write_all(id.as_bytes())?;
                file.sync_all()?;
                id
            }
            Err(error) => return Err(error.into()),
        };
        anyhow::ensure!(!self.by_id.contains_key(&id), "world id already loaded");
        // An existing world keeps the seed it was created with; a `level.dat` written before
        // seeds were stored belongs to a world that could only have been created with seed 0.
        let (spawn, seed) = if world_root.join("level.dat").exists() {
            (
                super::anvil::read_spawn(&world_root)?,
                super::anvil::read_seed(&world_root)?.unwrap_or(0),
            )
        } else {
            super::anvil::write_spawn_and_seed(&world_root, initial_spawn, requested_seed)?;
            (initial_spawn, requested_seed)
        };
        self.create_for_dimension(
            id,
            name,
            root,
            registry,
            terrain,
            RuntimeWorldSettings {
                spawn,
                dimension,
                seed,
            },
        )
        .map_err(anyhow::Error::msg)
    }

    /// Flush an already quiesced world before removing it from the catalog.
    pub async fn unload_saved(
        &mut self,
        id: uuid::Uuid,
        save: bool,
    ) -> anyhow::Result<Option<RuntimeWorldDescriptor>> {
        let Some(world) = self.by_id.get(&id) else {
            return Ok(None);
        };
        if save {
            world.source.flush_quiesced().await?;
        }
        Ok(self.unload(id))
    }
}

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
        terrain: Option<Arc<dyn ChunkGenerator>>,
        spawn: [i32; 3],
    ) -> Arc<Self> {
        let mut sources = HashMap::new();
        for dimension in Dimension::ALL {
            // The worldgen host generates every dimension (the end is dimension id 2); without a
            // host there is no terrain and each dimension uses the flat fallback.
            let dimension_terrain = terrain.clone();
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

    /// Builds the registry around an already-open source, so the game loop and the registry
    /// share one chunk cache. Integration worlds (fixture generators, prefilled chunks) keep
    /// using the source they were created from, which belongs to `dimension` (the configured
    /// world dimension, usually the overworld).
    pub fn with_source(
        dimension: Dimension,
        source: Arc<AnvilChunkSource>,
        spawn: [i32; 3],
    ) -> Arc<Self> {
        let mut sources = HashMap::new();
        for other in Dimension::ALL {
            let other_source = if other == dimension {
                Arc::clone(&source)
            } else {
                match source.path() {
                    Some(path) => AnvilChunkSource::with_dimension(
                        path.to_path_buf(),
                        source.registry(),
                        source.terrain(),
                        other,
                    ),
                    None => AnvilChunkSource::new_memory_dimension(source.terrain(), other),
                }
            };
            sources.insert(other, other_source);
        }
        let mut spawns = HashMap::from([
            (Dimension::Overworld, Dimension::Overworld.default_spawn()),
            (Dimension::Nether, Dimension::Nether.default_spawn()),
            (Dimension::End, Dimension::End.default_spawn()),
        ]);
        let nether_height = sources[&Dimension::Nether]
            .spawn_height(0, 0)
            .unwrap_or(Dimension::Nether.default_spawn()[1]);
        spawns.insert(
            Dimension::Nether,
            [0, nether_height.max(Dimension::Nether.min_y() + 16), 0],
        );
        spawns.insert(dimension, spawn);
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
    use std::path::PathBuf;

    #[tokio::test]
    async fn runtime_world_uuid_and_spawn_survive_unload_and_reopen() {
        let Some(root) = vanilla_root() else { return };
        let registry = Arc::new(RuntimeRegistry::load(root).unwrap());
        let directory = tempfile::tempdir().unwrap();
        let mut catalog = RuntimeWorldCatalog::default();
        let first = catalog
            .open_persistent(
                "extra",
                directory.path(),
                registry.clone(),
                None,
                RuntimeWorldRequest {
                    dimension: Dimension::Overworld,
                    spawn: [3, 70, -4],
                    seed: 0,
                },
            )
            .unwrap();
        let id = first.id;
        assert_eq!(
            std::fs::read(directory.path().join("extra/uid.dat")).unwrap(),
            id.as_bytes()
        );
        assert!(catalog.unload_saved(id, true).await.unwrap().is_some());
        let reopened = catalog
            .open_persistent(
                "extra",
                directory.path(),
                registry,
                None,
                RuntimeWorldRequest {
                    dimension: Dimension::Overworld,
                    spawn: [0, 64, 0],
                    seed: 0,
                },
            )
            .unwrap();
        assert_eq!(reopened.id, id);
        assert_eq!(reopened.spawn, [3, 70, -4]);
    }

    #[tokio::test]
    async fn runtime_world_seed_is_stored_in_level_dat_and_survives_reopen() {
        let Some(root) = vanilla_root() else { return };
        let registry = Arc::new(RuntimeRegistry::load(root).unwrap());
        let directory = tempfile::tempdir().unwrap();
        let mut catalog = RuntimeWorldCatalog::default();
        let open = |catalog: &mut RuntimeWorldCatalog, name: &str, seed: i64| {
            catalog
                .open_persistent(
                    name,
                    directory.path(),
                    registry.clone(),
                    None,
                    RuntimeWorldRequest {
                        dimension: Dimension::Overworld,
                        spawn: [0, 64, 0],
                        seed,
                    },
                )
                .unwrap()
        };
        // Any seed is accepted: zero, a large positive one and a negative one.
        for (name, seed) in [("zero", 0), ("big", i64::MAX), ("negative", -8_912_345_678)] {
            let created = open(&mut catalog, name, seed);
            assert_eq!(created.seed, seed, "{name}: descriptor carries the seed");
            assert_eq!(
                super::super::anvil::read_seed(&directory.path().join(name)).unwrap(),
                Some(seed),
                "{name}: level.dat records the seed"
            );
            assert!(
                catalog
                    .unload_saved(created.id, true)
                    .await
                    .unwrap()
                    .is_some()
            );
            // A different requested seed must not rewrite an existing world.
            let reopened = open(&mut catalog, name, seed.wrapping_add(1));
            assert_eq!(reopened.id, created.id);
            assert_eq!(reopened.seed, seed, "{name}: reopen keeps the stored seed");
        }
    }

    #[tokio::test]
    async fn a_level_dat_without_a_seed_reopens_with_seed_zero() {
        let Some(root) = vanilla_root() else { return };
        let registry = Arc::new(RuntimeRegistry::load(root).unwrap());
        let directory = tempfile::tempdir().unwrap();
        // The shape written before seeds were stored: spawn only.
        super::super::anvil::write_spawn(&directory.path().join("old"), [1, 2, 3]).unwrap();
        let mut catalog = RuntimeWorldCatalog::default();
        let world = catalog
            .open_persistent(
                "old",
                directory.path(),
                registry,
                None,
                RuntimeWorldRequest {
                    dimension: Dimension::Overworld,
                    spawn: [0, 64, 0],
                    seed: 777,
                },
            )
            .unwrap();
        assert_eq!(world.spawn, [1, 2, 3]);
        assert_eq!(world.seed, 0, "an existing world is never re-seeded");
    }

    #[test]
    fn runtime_world_names_are_portable_and_generate_valid_resource_keys() {
        for name in [
            "",
            ".",
            "..",
            "../escape",
            "a\\b",
            "C:drive",
            "bad?name",
            "name.",
            "NUL",
            "com1.txt",
            "Lpt9",
        ] {
            assert!(runtime_world_identity(name).is_err(), "accepted {name:?}");
        }
        assert_eq!(
            runtime_world_identity(" My World ").unwrap(),
            ("my world".into(), "ferrite:my_world".into())
        );
        let (_, key) = runtime_world_identity("한글").unwrap();
        assert!(key.strip_prefix("ferrite:").unwrap().bytes().all(
            |byte| byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'_' | b'-' | b'.')
        ));
    }

    #[tokio::test]
    async fn runtime_world_catalog_rejects_colliding_keys_and_releases_them_on_unload() {
        let Some(root) = vanilla_root() else { return };
        let registry = Arc::new(RuntimeRegistry::load(&root).unwrap());
        let world_dir = tempfile::tempdir().unwrap();
        let mut catalog = RuntimeWorldCatalog::default();
        let first = uuid::Uuid::from_u128(1);
        let second = uuid::Uuid::from_u128(2);
        let created = catalog
            .create(
                first,
                "My World",
                world_dir.path(),
                Arc::clone(&registry),
                None,
                [0, 64, 0],
            )
            .unwrap();
        assert_eq!(created.key, "ferrite:my_world");
        assert_eq!(catalog.by_name("MY WORLD").unwrap().id, first);
        assert!(
            catalog
                .create(
                    second,
                    "my_world",
                    world_dir.path(),
                    Arc::clone(&registry),
                    None,
                    [0, 64, 0]
                )
                .is_err()
        );
        assert!(catalog.unload(first).is_some());
        assert!(
            catalog
                .create(
                    second,
                    "my_world",
                    world_dir.path(),
                    registry,
                    None,
                    [0, 64, 0]
                )
                .is_ok()
        );
    }

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
