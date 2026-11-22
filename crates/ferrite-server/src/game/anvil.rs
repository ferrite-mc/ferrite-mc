use super::{Dimension, generator::ChunkGenerator};
use ferrite_command::gamerule::GameRules;
use ferrite_nbt::{List, Tag, binary};
use ferrite_world::{
    BlobCompression, BlockStateRegistry, Chunk, FworldColdStore, LightData, LightSection,
    RegionCompression, RegionFile, RuntimeRegistry, chunk_from_nbt, encode_chunk_packet,
};
use std::path::Path;

#[cfg(test)]
pub fn read_chunk(
    world: &Path,
    registry: &RuntimeRegistry,
    x: i32,
    z: i32,
) -> anyhow::Result<Option<Vec<u8>>> {
    Ok(read_column(world, registry, Dimension::Overworld, x, z)?
        .map(|(chunk, light)| encode_chunk_packet(x, z, &chunk, &light))
        .transpose()?)
}
fn read_column(
    world: &Path,
    registry: &RuntimeRegistry,
    dimension: Dimension,
    x: i32,
    z: i32,
) -> anyhow::Result<Option<(Chunk, LightData)>> {
    let rx = x.div_euclid(32);
    let rz = z.div_euclid(32);
    let dir = world.join(dimension.region_dir());
    if !dir.exists() {
        return Ok(None);
    }
    let region = RegionFile::open(dir.join(format!("r.{rx}.{rz}.mca")), rx, rz)?;
    let Some(data) = region.read_chunk(x, z)? else {
        return Ok(None);
    };
    let (_, tag) = binary::read_named(&data)?;
    column_from_tag(&tag, dimension, registry).map(Some)
}

/// Builds a column and its stored light from the NBT of one Anvil chunk. Used for chunks read
/// from region files and for chunks returned by an external generator.
pub(super) fn column_from_tag(
    tag: &Tag,
    dimension: Dimension,
    registry: &RuntimeRegistry,
) -> anyhow::Result<(Chunk, LightData)> {
    let chunk = chunk_from_nbt(tag, dimension.min_y(), dimension.height() as u32, registry)?;
    let sections = dimension.light_sections();
    let min_section = dimension.min_y().div_euclid(16);
    let mut light = LightData {
        sky: vec![LightSection::Unknown; sections],
        block: vec![LightSection::Unknown; sections],
    };
    if let Tag::Compound(root) = tag
        && let Some(Tag::List(List::Compound(sections))) = root.get("sections")
    {
        for section in sections {
            let Some(Tag::Byte(y)) = section.get("Y") else {
                continue;
            };
            let idx = i32::from(*y) - min_section + 1;
            if !(0..light.sky.len() as i32).contains(&idx) {
                continue;
            }
            for (name, target) in [
                ("SkyLight", &mut light.sky),
                ("BlockLight", &mut light.block),
            ] {
                if let Some(Tag::ByteArray(bytes)) = section.get(name) {
                    anyhow::ensure!(bytes.len() == 2048, "invalid {name} length");
                    let mut array = Box::new([0; 2048]);
                    for (dst, src) in array.iter_mut().zip(bytes) {
                        *dst = *src as u8;
                    }
                    target[idx as usize] = LightSection::Data(array);
                }
            }
        }
    }
    Ok((chunk, light))
}

use ferrite_net::game::{SharedFrame, shared_frame_arc};
use ferrite_protocol::{Encode, FrameEncoder, Packet, VarInt, packets::play::ChunkData};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock, RwLock,
        atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering},
    },
};

const CACHE_LIMIT: usize = 4096;
/// Region files written concurrently by [`AnvilChunkSource::flush_all`].
const FLUSH_PARALLELISM: usize = 8;
/// Entries dropped by one eviction pass once the cache is full.
const EVICT_BATCH: usize = CACHE_LIMIT / 8;
const PENDING_LIMIT: usize = 128;
/// Emit the cache hit rate to the debug log every this many lookups.
const CACHE_LOG_INTERVAL: u64 = 4096;

/// Counters backing [`CacheStats`]. They are atomic so a lookup only bumps a
/// counter and the background encode task can count its own work.
#[derive(Default)]
struct Stats {
    lookups: AtomicU64,
    hits: AtomicU64,
    encodes: AtomicU64,
    invalidations: AtomicU64,
}

/// How the encoded chunk packet cache is doing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// `get`/`prefetch` calls.
    pub lookups: u64,
    /// Lookups answered with an already encoded packet.
    pub hits: u64,
    /// `level_chunk_with_light` packets built.
    pub encodes: u64,
    /// Cached packets dropped because the chunk changed.
    pub invalidations: u64,
}

impl CacheStats {
    /// Share of lookups served from cache, `0.0..=1.0`.
    pub fn hit_rate(&self) -> f64 {
        if self.lookups == 0 {
            0.0
        } else {
            self.hits as f64 / self.lookups as f64
        }
    }
}

enum Entry {
    Pending(u64),
    Ready(Result<Box<Column>, String>, AtomicU64),
}
struct Column {
    chunk: Arc<Chunk>,
    light: LightData,
    /// Complete `level_chunk_with_light` body (packet id + payload). Shared by
    /// every player that receives this chunk, exactly like Paper's packet
    /// cache: encoded once, copied only when it is handed to a connection.
    packet: Arc<[u8]>,
    /// Pre-compressed wire frame for `packet`, avoiding redundant per-player zlib compression.
    frame: Arc<SharedFrame>,
    /// This column was generated (or read from a flat source) rather than read back from an
    /// Anvil file. A disk-backed source writes it on chunk unload or server shutdown even when
    /// no block changed, matching vanilla, which saves every generated chunk.
    generated: bool,
}
struct Cache {
    entries: HashMap<(i32, i32, i32), Entry>,
    /// Secondary O(1) index mapping `(x, z)` to a resident `biome` key in `entries`.
    by_coord: HashMap<(i32, i32), i32>,
    clock: AtomicU64,
    dirty: HashMap<(i32, i32), Dirty>,
    /// Number of `Entry::Pending` in `entries`, so a miss does not count them under the lock.
    pending: usize,
    /// The biome variant most recently requested. Entries are keyed `(x, z, biome)` but the
    /// chunk contents do not depend on the variant, so block reads try this key first.
    biome_hint: AtomicI32,
    /// Chunks queued for batched relight when `lighting_pool` is active.
    pending_relight: HashSet<(i32, i32)>,
}
impl Cache {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            by_coord: HashMap::new(),
            clock: AtomicU64::new(0),
            dirty: HashMap::new(),
            pending: 0,
            biome_hint: AtomicI32::new(0),
            pending_relight: HashSet::new(),
        }
    }
    /// The loaded column of chunk `(x, z)` under any biome variant in guaranteed O(1) time.
    fn ready_column(&self, x: i32, z: i32) -> Option<&Column> {
        let hint = self.biome_hint.load(Ordering::Relaxed);
        if let Some(Entry::Ready(Ok(column), _)) = self.entries.get(&(x, z, hint)) {
            return Some(column);
        }
        if let Some(&biome) = self.by_coord.get(&(x, z))
            && let Some(Entry::Ready(Ok(column), _)) = self.entries.get(&(x, z, biome))
        {
            return Some(column);
        }
        self.entries
            .iter()
            .find_map(|(&(cx, cz, _), entry)| match entry {
                Entry::Ready(Ok(column), _) if (cx, cz) == (x, z) => Some(&**column),
                _ => None,
            })
    }
}
struct Dirty {
    chunk: Arc<ferrite_world::Chunk>,
    light: LightData,
    packet: Arc<[u8]>,
    frame: Arc<SharedFrame>,
    version: u64,
}

/// What a cache lookup found.
enum Lookup {
    Ready(Arc<[u8]>, Arc<SharedFrame>),
    Failed(String),
    Loading,
}

/// Block and light contents of one chunk, captured under the cache lock so a command snapshot
/// cannot observe half of a concurrent edit.
pub struct ChunkContent {
    pub min_y: i32,
    pub max_y: i32,
    /// Block states ordered `y`, then `z`, then `x`.
    pub blocks: Vec<u16>,
    pub sky_light: Vec<u8>,
    pub block_light: Vec<u8>,
}

struct FworldStore {
    blob_path: PathBuf,
    compression: BlobCompression,
    world: Mutex<FworldColdStore>,
    dirty: AtomicBool,
}

pub struct AnvilChunkSource {
    path: Option<PathBuf>,
    registry: Arc<RuntimeRegistry>,
    cache: RwLock<Cache>,
    stats: Stats,
    runtime: tokio::runtime::Handle,
    flush_lock: tokio::sync::Mutex<()>,
    pending_done: tokio::sync::Notify,
    flat: Mutex<HashMap<i32, (Chunk, LightData)>>,
    /// Vanilla generation for chunks missing from disk; `None` is the flat test world.
    terrain: Option<Arc<dyn ChunkGenerator>>,
    /// Which dimension this source reads, writes and generates.
    dimension: Dimension,
    /// Compression format used when flushing Anvil region chunks.
    region_compression: RegionCompression,
    /// Optional Ferrite single-blob (`.fworld`) RAM backend, flushed atomically when dirty.
    fworld: Option<Arc<FworldStore>>,
    /// Opt-in parallel `ferrite-light` pool toggle (default off).
    lighting_pool: AtomicBool,
    /// Guards overlapping background batch relight tasks.
    relight_in_flight: AtomicBool,
    /// Shared per-dimension `LightPool` instance keyed by worker count.
    light_pool: Mutex<Option<(usize, Arc<ferrite_light::LightPool>)>>,
    /// Shared wire frame encoder used to pre-compress cached chunk packets once per chunk.
    shared_encoder: RwLock<Arc<FrameEncoder>>,
    /// Set once for a runtime world, so code holding only the source resolves the right world.
    runtime_id: std::sync::OnceLock<uuid::Uuid>,
}
impl AnvilChunkSource {
    pub fn new(path: PathBuf, registry: RuntimeRegistry) -> Arc<Self> {
        Self::new_with_terrain(path, registry, None)
    }
    pub fn new_with_terrain(
        path: PathBuf,
        registry: RuntimeRegistry,
        terrain: Option<Arc<dyn ChunkGenerator>>,
    ) -> Arc<Self> {
        Self::with_dimension(path, Arc::new(registry), terrain, Dimension::Overworld)
    }
    /// A disk-backed source for one dimension. `path` is the world root; the dimension's
    /// vanilla storage tree (`DIM-1`, `DIM1`, ...) is appended.
    pub fn with_dimension(
        path: PathBuf,
        registry: Arc<RuntimeRegistry>,
        terrain: Option<Arc<dyn ChunkGenerator>>,
        dimension: Dimension,
    ) -> Arc<Self> {
        Self::with_dimension_and_compression(
            path,
            registry,
            terrain,
            dimension,
            RegionCompression::Zlib,
        )
    }

    pub fn with_dimension_and_compression(
        path: PathBuf,
        registry: Arc<RuntimeRegistry>,
        terrain: Option<Arc<dyn ChunkGenerator>>,
        dimension: Dimension,
        region_compression: RegionCompression,
    ) -> Arc<Self> {
        Arc::new(Self {
            path: Some(path),
            registry,
            cache: RwLock::new(Cache::new()),
            stats: Stats::default(),
            runtime: tokio::runtime::Handle::current(),
            flush_lock: tokio::sync::Mutex::new(()),
            pending_done: tokio::sync::Notify::new(),
            flat: Mutex::new(HashMap::new()),
            terrain,
            dimension,
            region_compression,
            fworld: None,
            lighting_pool: AtomicBool::new(false),
            relight_in_flight: AtomicBool::new(false),
            light_pool: Mutex::new(None),
            shared_encoder: RwLock::new(Arc::new(FrameEncoder::new_compressed(Some(256), 6, true))),
            runtime_id: Default::default(),
        })
    }

    /// A `.fworld` single-blob world source loaded wholly into RAM and persisted atomically.
    pub fn with_fworld(
        blob_path: PathBuf,
        registry: Arc<RuntimeRegistry>,
        terrain: Option<Arc<dyn ChunkGenerator>>,
        dimension: Dimension,
        compression: BlobCompression,
        spawn: [i32; 3],
    ) -> anyhow::Result<Arc<Self>> {
        let existed = blob_path.exists();
        let world = if existed {
            FworldColdStore::read_file(&blob_path, compression)?
        } else {
            FworldColdStore::new(spawn, compression)
        };
        Ok(Arc::new(Self {
            path: None,
            registry,
            cache: RwLock::new(Cache::new()),
            stats: Stats::default(),
            runtime: tokio::runtime::Handle::current(),
            flush_lock: tokio::sync::Mutex::new(()),
            pending_done: tokio::sync::Notify::new(),
            flat: Mutex::new(HashMap::new()),
            terrain,
            dimension,
            region_compression: RegionCompression::Lz4,
            fworld: Some(Arc::new(FworldStore {
                blob_path,
                compression,
                world: Mutex::new(world),
                dirty: AtomicBool::new(!existed),
            })),
            lighting_pool: AtomicBool::new(false),
            relight_in_flight: AtomicBool::new(false),
            light_pool: Mutex::new(None),
            shared_encoder: RwLock::new(Arc::new(FrameEncoder::new_compressed(Some(256), 6, true))),
            runtime_id: Default::default(),
        }))
    }
    pub fn new_memory() -> Arc<Self> {
        Self::new_memory_with_terrain(None)
    }
    pub fn new_memory_with_terrain(terrain: Option<Arc<dyn ChunkGenerator>>) -> Arc<Self> {
        Self::new_memory_dimension(terrain, Dimension::Overworld)
    }
    /// An in-memory source for one dimension, used by tests and by worlds without a
    /// directory.
    pub fn new_memory_dimension(
        terrain: Option<Arc<dyn ChunkGenerator>>,
        dimension: Dimension,
    ) -> Arc<Self> {
        static REGISTRY: OnceLock<Arc<RuntimeRegistry>> = OnceLock::new();
        let registry = REGISTRY
            .get_or_init(|| {
                Arc::new(
                    RuntimeRegistry::load(
                        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join("../../vanilla/26.3/generated"),
                    )
                    .expect("loading vanilla registry"),
                )
            })
            .clone();
        Arc::new(Self {
            path: None,
            registry,
            cache: RwLock::new(Cache::new()),
            stats: Stats::default(),
            runtime: tokio::runtime::Handle::current(),
            flush_lock: tokio::sync::Mutex::new(()),
            pending_done: tokio::sync::Notify::new(),
            flat: Mutex::new(HashMap::new()),
            terrain,
            dimension,
            region_compression: RegionCompression::Zlib,
            fworld: None,
            lighting_pool: AtomicBool::new(false),
            relight_in_flight: AtomicBool::new(false),
            light_pool: Mutex::new(None),
            shared_encoder: RwLock::new(Arc::new(FrameEncoder::new_compressed(Some(256), 6, true))),
            runtime_id: Default::default(),
        })
    }
    pub fn dimension(&self) -> Dimension {
        self.dimension
    }
    pub fn region_compression(&self) -> RegionCompression {
        self.region_compression
    }
    pub fn fworld_blob_and_compression(&self) -> Option<(&std::path::Path, BlobCompression)> {
        self.fworld
            .as_ref()
            .map(|store| (store.blob_path.as_path(), store.compression))
    }
    pub fn fworld_dimension_path(base: &std::path::Path, dimension: Dimension) -> PathBuf {
        let suffix = match dimension {
            Dimension::Overworld => return base.to_path_buf(),
            Dimension::Nether => "nether",
            Dimension::End => "end",
        };
        let raw = base.to_string_lossy();
        if let Some(stem) = raw.strip_suffix(".fworld") {
            PathBuf::from(format!("{stem}.{suffix}.fworld"))
        } else {
            PathBuf::from(format!("{raw}.{suffix}.fworld"))
        }
    }
    /// Copy every block and light value of a resident chunk. The lock is held for the whole
    /// copy, so the result is one coherent state rather than a mix of before and after edits.
    pub fn chunk_content(&self, cx: i32, cz: i32) -> Option<ChunkContent> {
        let cache = self.cache.read().unwrap();
        let (chunk, light) = if let Some(dirty) = cache.dirty.get(&(cx, cz)) {
            (&dirty.chunk, &dirty.light)
        } else {
            let column = cache.ready_column(cx, cz)?;
            (&column.chunk, &column.light)
        };
        let min_y = self.dimension.min_y();
        let max_y = min_y + self.dimension.height();
        let mut blocks = Vec::with_capacity((self.dimension.height() * 256) as usize);
        let mut highest = [min_y; 256];
        for y in min_y..max_y {
            for z in 0..16 {
                for x in 0..16 {
                    let state = u16::try_from(chunk.block(x, y, z)?).ok()?;
                    if !ferrite_data::block_props::get(state).is_air {
                        highest[z as usize * 16 + x as usize] = y;
                    }
                    blocks.push(state);
                }
            }
        }
        let mut sky_light = Vec::with_capacity(blocks.len());
        let mut block_light = Vec::with_capacity(blocks.len());
        for y in min_y..max_y {
            let section = usize::try_from(y.div_euclid(16) - min_y.div_euclid(16)).ok()?;
            for z in 0..16 {
                for x in 0..16 {
                    let index = (y.rem_euclid(16) as usize * 256) + (z as usize * 16) + x as usize;
                    let sky = match light.sky.get(section) {
                        Some(LightSection::Data(data)) => {
                            (data[index / 2] >> ((index % 2) * 4)) & 15
                        }
                        // A chunk without stored sky light falls back to an open-sky scan:
                        // everything above the highest non-air block sees the sky.
                        _ => u8::from(y > highest[z as usize * 16 + x as usize]) * 15,
                    };
                    let block = match light.block.get(section) {
                        Some(LightSection::Data(data)) => {
                            (data[index / 2] >> ((index % 2) * 4)) & 15
                        }
                        _ => 0,
                    };
                    sky_light.push(sky);
                    block_light.push(block);
                }
            }
        }
        Some(ChunkContent {
            min_y,
            max_y,
            blocks,
            sky_light,
            block_light,
        })
    }
    /// The independently ticking world this source belongs to.
    pub fn world_id(&self) -> super::WorldId {
        self.runtime_id
            .get()
            .map_or(super::WorldId::Vanilla(self.dimension), |id| {
                super::WorldId::Runtime(*id)
            })
    }
    pub(super) fn bind_runtime_world(&self, id: uuid::Uuid) {
        let _ = self.runtime_id.set(id);
    }
    pub(super) fn registry(&self) -> Arc<RuntimeRegistry> {
        Arc::clone(&self.registry)
    }
    pub(super) fn terrain(&self) -> Option<Arc<dyn ChunkGenerator>> {
        self.terrain.clone()
    }
    /// Whether missing chunks use the flat test world instead of vanilla terrain.
    pub fn is_flat(&self) -> bool {
        self.terrain.is_none()
    }
    /// The world directory this source reads and writes, when it is backed by disk.
    pub fn path(&self) -> Option<&std::path::Path> {
        self.path.as_deref()
    }
    /// The first free block above the generated surface, when vanilla generation is enabled.
    pub fn spawn_height(&self, x: i32, z: i32) -> Option<i32> {
        self.terrain
            .as_ref()?
            .spawn_height(self.dimension, x, z)
            .ok()
    }

    /// The nearest position with the given biome (`id` or `#tag`), when vanilla generation is
    /// enabled. A flat test world has no biome source and reports no match.
    pub fn locate_biome(
        &self,
        target: &str,
        x: i32,
        z: i32,
    ) -> Option<anyhow::Result<Option<(i32, i32, i32)>>> {
        let terrain = self.terrain.as_ref()?;
        Some(terrain.locate_biome(self.dimension, target, x, z, 6_400))
    }

    /// The biome id at a block, when vanilla generation is enabled.
    pub fn biome_at(&self, x: i32, y: i32, z: i32) -> Option<anyhow::Result<String>> {
        Some(self.terrain.as_ref()?.biome_at(self.dimension, x, y, z))
    }

    /// The nearest structure of `target` (`id` or `#tag`), when vanilla generation is enabled.
    pub fn locate_structure(
        &self,
        target: &str,
        x: i32,
        z: i32,
        radius: i32,
    ) -> Option<anyhow::Result<Option<(i32, i32, i32)>>> {
        let terrain = self.terrain.as_ref()?;
        Some(terrain.locate_structure(self.dimension, target, x, z, radius))
    }

    /// The nearest point of interest of a type (`id` or `#tag`), when vanilla generation is on.
    pub fn locate_poi(
        &self,
        target: &str,
        x: i32,
        y: i32,
        z: i32,
        radius: i32,
    ) -> Option<anyhow::Result<Option<(i32, i32, i32)>>> {
        let terrain = self.terrain.as_ref()?;
        Some(terrain.locate_poi(self.dimension, target, x, y, z, radius))
    }

    /// Runs a vanilla placement in the worldgen host, when vanilla generation is enabled.
    pub fn place(&self, argument: &str) -> Option<anyhow::Result<super::generator::Placed>> {
        Some(self.terrain.as_ref()?.place(self.dimension, argument))
    }

    /// The structures starting in a chunk, when vanilla generation is enabled.
    pub fn structure_starts(
        &self,
        chunk_x: i32,
        chunk_z: i32,
    ) -> Option<anyhow::Result<Vec<super::generator::StructureStart>>> {
        Some(
            self.terrain
                .as_ref()?
                .structure_starts(self.dimension, chunk_x, chunk_z),
        )
    }

    /// Whether a piece of structure `id` covers the block, when vanilla generation is enabled.
    pub fn structure_at(&self, id: &str, x: i32, y: i32, z: i32) -> Option<anyhow::Result<bool>> {
        Some(
            self.terrain
                .as_ref()?
                .structure_at(self.dimension, id, x, y, z),
        )
    }
    /// The 26.3 world data file that stores the live game rule values.
    pub(super) fn game_rules_path(&self) -> Option<PathBuf> {
        self.path
            .as_ref()
            .map(|path| path.join("data/minecraft/game_rules.dat"))
    }

    /// Reads the persisted rules, falling back to the pre-26.3 `Data.GameRules`
    /// compound in `level.dat` for worlds created by older versions.
    pub(super) fn load_game_rules(&self) -> Option<GameRules> {
        if let Some(path) = self.game_rules_path()
            && let Ok(bytes) = std::fs::read(&path)
            && let Ok((_, tag)) = binary::read_compressed(&bytes, binary::Compression::Gzip)
            && let Some(rules) = GameRules::from_nbt(&tag)
        {
            return Some(rules);
        }
        let bytes = std::fs::read(self.path.as_ref()?.join("level.dat")).ok()?;
        let (_, tag) = binary::read_compressed(&bytes, binary::Compression::Gzip).ok()?;
        GameRules::from_level_dat(&tag)
    }

    /// Writes `data/minecraft/game_rules.dat`, stamped with `level.dat`'s `DataVersion`
    /// like a vanilla save.
    pub(super) fn save_game_rules(&self, rules: &GameRules) -> anyhow::Result<()> {
        let Some(path) = self.game_rules_path() else {
            return Ok(());
        };
        let data_version = self
            .data_version()
            .unwrap_or(ferrite_command::gamerule::DATA_VERSION);
        let tag = rules.to_nbt(data_version);
        let bytes = binary::write_compressed("", &tag, binary::Compression::Gzip)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, bytes)?;
        Ok(())
    }

    /// A dimension's source owns its own storage root, so its tickets live beside its other
    /// `data/minecraft` files.
    fn tickets_path(&self) -> Option<PathBuf> {
        self.path.as_ref().map(|path| {
            self.dimension
                .storage_root(path)
                .join("data/minecraft/chunk_tickets.dat")
        })
    }

    /// The `/forceload`ed chunks stored in this dimension's `chunk_tickets.dat`.
    pub(super) fn load_forced_chunks(&self) -> std::collections::BTreeSet<(String, i32, i32)> {
        let mut forced = std::collections::BTreeSet::new();
        let key = self.dimension.key();
        'file: {
            let Some(path) = self.tickets_path() else {
                break 'file;
            };
            let Ok(bytes) = std::fs::read(path) else {
                break 'file;
            };
            let Ok((_, Tag::Compound(root))) =
                binary::read_compressed(&bytes, binary::Compression::Gzip)
            else {
                break 'file;
            };
            let Some(Tag::Compound(data)) = root.get("data") else {
                break 'file;
            };
            let Some(Tag::List(List::Compound(tickets))) = data.get("tickets") else {
                break 'file;
            };
            for ticket in tickets {
                let is_forced = matches!(ticket.get("type"), Some(Tag::String(kind)) if kind == "minecraft:forced");
                if let (true, Some(Tag::IntArray(pos))) = (is_forced, ticket.get("chunk_pos"))
                    && let [x, z] = pos[..]
                {
                    forced.insert((key.to_owned(), x, z));
                }
            }
        }
        forced
    }

    /// Writes the forced chunks of one dimension to its `chunk_tickets.dat`, like vanilla's
    /// `forced` tickets (level 31). Other ticket types are runtime state and are not kept.
    pub(super) fn save_forced_chunks(
        &self,
        dimension: &str,
        forced: &std::collections::BTreeSet<(String, i32, i32)>,
    ) -> anyhow::Result<()> {
        let Some(path) = self.tickets_path() else {
            return Ok(());
        };
        let tickets: Vec<ferrite_nbt::Compound> = forced
            .iter()
            .filter(|(key, ..)| key == dimension)
            .map(|(_, x, z)| {
                let mut ticket = ferrite_nbt::Compound::new();
                ticket.insert("chunk_pos".into(), Tag::IntArray(vec![*x, *z]));
                ticket.insert("level".into(), Tag::Int(31));
                ticket.insert("type".into(), Tag::String("minecraft:forced".into()));
                ticket
            })
            .collect();
        let mut data = ferrite_nbt::Compound::new();
        data.insert("tickets".into(), Tag::List(List::Compound(tickets)));
        let mut root = ferrite_nbt::Compound::new();
        root.insert("data".into(), Tag::Compound(data));
        root.insert(
            "DataVersion".into(),
            Tag::Int(
                self.data_version()
                    .unwrap_or(ferrite_command::gamerule::DATA_VERSION),
            ),
        );
        let bytes = binary::write_compressed("", &Tag::Compound(root), binary::Compression::Gzip)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, bytes)?;
        Ok(())
    }

    fn fight_path(&self) -> Option<PathBuf> {
        self.path.as_ref().map(|path| {
            self.dimension
                .storage_root(path)
                .join("data/minecraft/ender_dragon_fight.dat")
        })
    }

    /// The saved dragon fight of this (end) dimension, if there is one.
    pub(super) fn load_fight(&self) -> Option<super::end_fight::Saved> {
        let bytes = std::fs::read(self.fight_path()?).ok()?;
        let (_, Tag::Compound(root)) =
            binary::read_compressed(&bytes, binary::Compression::Gzip).ok()?
        else {
            return None;
        };
        let Tag::Compound(data) = root.get("data")? else {
            return None;
        };
        super::end_fight::Saved::from_nbt(data)
    }

    /// Writes the dragon fight beside the other `data/minecraft` files of the end.
    pub(super) fn save_fight(&self, saved: &super::end_fight::Saved) -> anyhow::Result<()> {
        let Some(path) = self.fight_path() else {
            return Ok(());
        };
        let mut root = ferrite_nbt::Compound::new();
        root.insert("data".into(), Tag::Compound(saved.to_nbt()));
        root.insert(
            "DataVersion".into(),
            Tag::Int(
                self.data_version()
                    .unwrap_or(ferrite_command::gamerule::DATA_VERSION),
            ),
        );
        let bytes = binary::write_compressed("", &Tag::Compound(root), binary::Compression::Gzip)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, bytes)?;
        Ok(())
    }

    fn data_version(&self) -> Option<i32> {
        let bytes = std::fs::read(self.path.as_ref()?.join("level.dat")).ok()?;
        let (_, Tag::Compound(root)) =
            binary::read_compressed(&bytes, binary::Compression::Gzip).ok()?
        else {
            return None;
        };
        let Tag::Compound(data) = root.get("Data")? else {
            return None;
        };
        match data.get("DataVersion") {
            Some(Tag::Int(version)) => Some(*version),
            _ => None,
        }
    }
    pub fn modified_packet(&self, x: i32, z: i32) -> Option<Vec<u8>> {
        self.cache
            .read()
            .unwrap()
            .dirty
            .get(&(x, z))
            .map(|dirty| dirty.packet.to_vec())
    }
    /// Snapshot of the packet cache counters. The game loop and load tests
    /// report the hit rate from here.
    pub fn cache_stats(&self) -> CacheStats {
        CacheStats {
            lookups: self.stats.lookups.load(Ordering::Relaxed),
            hits: self.stats.hits.load(Ordering::Relaxed),
            encodes: self.stats.encodes.load(Ordering::Relaxed),
            invalidations: self.stats.invalidations.load(Ordering::Relaxed),
        }
    }
    pub(super) fn with_loaded_chunk<R>(
        &self,
        x: i32,
        z: i32,
        f: impl FnOnce(&Chunk) -> R,
    ) -> Option<R> {
        let cache = self.cache.read().unwrap();
        if let Some(dirty) = cache.dirty.get(&(x, z)) {
            return Some(f(&dirty.chunk));
        }
        let column = cache.ready_column(x, z)?;
        Some(f(&column.chunk))
    }
    /// Clones a shared reference to the loaded [`Chunk`] without copying sections.
    pub(super) fn chunk_arc(&self, x: i32, z: i32) -> Option<Arc<Chunk>> {
        let cache = self.cache.read().unwrap();
        if let Some(dirty) = cache.dirty.get(&(x, z)) {
            return Some(Arc::clone(&dirty.chunk));
        }
        cache.ready_column(x, z).map(|col| Arc::clone(&col.chunk))
    }
    /// Whether the chunk is resident in this source right now (loaded and readable).
    pub fn chunk_resident(&self, x: i32, z: i32) -> bool {
        self.with_loaded_chunk(x, z, |_| ()).is_some()
    }
    /// The chunks currently resident in this source, sorted by chunk coordinates.
    pub fn resident_chunks(&self) -> Vec<(i32, i32)> {
        let cache = self.cache.read().unwrap();
        let mut chunks: std::collections::BTreeSet<(i32, i32)> =
            cache.dirty.keys().copied().collect();
        for (&(x, z, _), entry) in &cache.entries {
            if matches!(entry, Entry::Ready(Ok(_), _)) {
                chunks.insert((x, z));
            }
        }
        chunks.into_iter().collect()
    }
    /// Whether the chunk is stored in this source's region files or loaded `.fworld` blob. Never
    /// reads region files or creates chunks; a pure memory-backed source stores nothing.
    pub fn chunk_saved(&self, x: i32, z: i32) -> bool {
        if let Some(fworld) = &self.fworld {
            return fworld.world.lock().unwrap().chunks.contains_key(&(x, z));
        }
        let Some(path) = &self.path else {
            return false;
        };
        let (rx, rz) = (x.div_euclid(32), z.div_euclid(32));
        let file = path
            .join(self.dimension.region_dir())
            .join(format!("r.{rx}.{rz}.mca"));
        file.is_file()
            && RegionFile::open(&file, rx, rz)
                .and_then(|region| region.read_chunk(x, z))
                .is_ok_and(|chunk| chunk.is_some())
    }
    pub(super) fn brightness(&self, x: i32, y: i32, z: i32) -> Option<u8> {
        let cache = self.cache.read().unwrap();
        let key = (x.div_euclid(16), z.div_euclid(16));
        let light = if let Some(d) = cache.dirty.get(&key) {
            &d.light
        } else {
            &cache.ready_column(key.0, key.1)?.light
        };
        let section =
            usize::try_from(y.div_euclid(16) - self.dimension.min_y().div_euclid(16) + 1).ok()?;
        let index = (y.rem_euclid(16) as usize * 256)
            + (z.rem_euclid(16) as usize * 16)
            + x.rem_euclid(16) as usize;
        let read = |sections: &[LightSection]| match sections.get(section) {
            Some(LightSection::Data(data)) => Some((data[index / 2] >> ((index % 2) * 4)) & 15),
            Some(LightSection::Empty) => Some(0),
            _ => None,
        };
        Some(
            read(&light.sky)
                .unwrap_or(0)
                .max(read(&light.block).unwrap_or(0)),
        )
    }
    pub fn block(&self, x: i32, y: i32, z: i32) -> Option<u32> {
        let key = (x.div_euclid(16), z.div_euclid(16));
        let cache = self.cache.read().unwrap();
        if let Some(d) = cache.dirty.get(&key) {
            return d
                .chunk
                .block(x.rem_euclid(16) as u8, y, z.rem_euclid(16) as u8);
        }
        let column = cache.ready_column(key.0, key.1)?;
        column
            .chunk
            .block(x.rem_euclid(16) as u8, y, z.rem_euclid(16) as u8)
    }
    /// Stored sky light at a block. A chunk without light data (the flat test world) falls back
    /// to an open-sky scan, so daylight still matches what a client sees.
    pub fn sky_light(&self, x: i32, y: i32, z: i32) -> Option<u8> {
        let stored = {
            let cache = self.cache.read().unwrap();
            let key = (x.div_euclid(16), z.div_euclid(16));
            let light = if let Some(dirty) = cache.dirty.get(&key) {
                &dirty.light
            } else {
                &cache.ready_column(key.0, key.1)?.light
            };
            let section = usize::try_from(y.div_euclid(16) + 4).ok()?;
            let index = (y.rem_euclid(16) as usize * 256)
                + (z.rem_euclid(16) as usize * 16)
                + x.rem_euclid(16) as usize;
            match light.sky.get(section) {
                Some(LightSection::Data(data)) => Some((data[index / 2] >> ((index % 2) * 4)) & 15),
                _ => None,
            }
        };
        if let Some(value) = stored {
            return Some(value);
        }
        for scan in y.clamp(-64, 319)..=319 {
            let state = self.block(x, scan, z)?;
            if !ferrite_data::block_props::get(u16::try_from(state).ok()?).is_air {
                return Some(0);
            }
        }
        Some(15)
    }
    /// The biome name at a block, from the chunk's biome cells. The flat test world has no
    /// biome source and reports its generated plains column.
    pub fn biome_name(&self, x: i32, y: i32, z: i32) -> Option<String> {
        let biome = self
            .with_loaded_chunk(x.div_euclid(16), z.div_euclid(16), |chunk| {
                let section = usize::try_from(y.div_euclid(16) + 4).ok()?;
                let index = (((y.rem_euclid(16) as usize) >> 2) << 4)
                    | (((z.rem_euclid(16) as usize) >> 2) << 2)
                    | ((x.rem_euclid(16) as usize) >> 2);
                chunk.sections().get(section)?.biomes().get(index)
            })
            .flatten();
        if let Some(id) = biome
            && let Some(name) = self.registry.biome_name(id)
        {
            return Some(name.to_owned());
        }
        self.is_flat().then(|| "minecraft:plains".to_owned())
    }
    /// Loads the chunk through this world's own loader when it is not resident, waiting up to
    /// `timeout`. `true` once the chunk can be read. This is how command reads reach chunks no
    /// player holds; it adds no scheduler hold, so the chunk may be evicted again later.
    pub fn ensure_chunk_loaded(
        self: &Arc<Self>,
        x: i32,
        z: i32,
        timeout: std::time::Duration,
    ) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        // A flat source generates its biome cells from the requested variant. Command reads
        // have no client biome id, so use plains rather than registry id 0 (badlands).
        let biome = self
            .registry
            .biome_id("minecraft:plains")
            .and_then(|id| i32::try_from(id).ok())
            .unwrap_or(0);
        loop {
            if self.with_loaded_chunk(x, z, |_| ()).is_some() {
                return true;
            }
            self.prefetch(x, z, biome);
            if self.with_loaded_chunk(x, z, |_| ()).is_some() {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    /// Y of the highest block a vanilla heightmap counts in a loaded column, from the chunk's
    /// stored heightmap. An uncounted column answers `min_y - 1`, like Paper.
    pub fn heightmap_y(&self, x: i32, z: i32, kind: ferrite_world::HeightmapKind) -> Option<i32> {
        let stored = self
            .with_loaded_chunk(x.div_euclid(16), z.div_euclid(16), |chunk| {
                chunk.height(kind, x.rem_euclid(16) as u8, z.rem_euclid(16) as u8)
            })
            .flatten()?;
        Some(i32::from(stored) + self.dimension.min_y() - 1)
    }
    /// The brighter of sky and block light at a position in a loaded chunk.
    pub fn light_level(&self, x: i32, y: i32, z: i32) -> Option<u8> {
        let sky = self.sky_light(x, y, z)?;
        Some(sky.max(self.brightness(x, y, z).unwrap_or(0)))
    }
    /// The highest non-air block in a loaded column.
    pub fn highest_block_y(&self, x: i32, z: i32) -> Option<i32> {
        self.block(x, 0, z)?;
        for y in (-64..=319).rev() {
            let state = self.block(x, y, z)?;
            if !ferrite_data::block_props::get(u16::try_from(state).ok()?).is_air {
                return Some(y);
            }
        }
        Some(-64)
    }
    /// The block entity at a block, when its chunk is loaded and has one.
    pub fn block_entity(&self, x: i32, y: i32, z: i32) -> Option<ferrite_world::BlockEntity> {
        let local = (x.rem_euclid(16) as u8, y, z.rem_euclid(16) as u8);
        self.with_loaded_chunk(x.div_euclid(16), z.div_euclid(16), |chunk| {
            chunk.block_entities.get(&local).cloned()
        })
        .flatten()
    }
    /// Every block entity in the loaded chunks, with its world position. Used by the beacon
    /// scan, which has to find block entities no player has opened.
    pub fn block_entities(&self) -> Vec<([i32; 3], ferrite_world::BlockEntity)> {
        let cache = self.cache.read().unwrap();
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (&(cx, cz), dirty) in &cache.dirty {
            seen.insert((cx, cz));
            for (&(x, y, z), entity) in &dirty.chunk.block_entities {
                out.push((
                    [cx * 16 + i32::from(x), y, cz * 16 + i32::from(z)],
                    entity.clone(),
                ));
            }
        }
        for (&(cx, cz, _), entry) in &cache.entries {
            if !seen.insert((cx, cz)) {
                continue;
            }
            if let Entry::Ready(Ok(column), _) = entry {
                for (&(x, y, z), entity) in &column.chunk.block_entities {
                    out.push((
                        [cx * 16 + i32::from(x), y, cz * 16 + i32::from(z)],
                        entity.clone(),
                    ));
                }
            }
        }
        out
    }

    /// Edits the block entity at a block (`None` when there is none) in a loaded chunk; the
    /// chunk is marked modified when `f` reports a change, so it is resent and saved.
    pub fn update_block_entity<R>(
        &self,
        x: i32,
        y: i32,
        z: i32,
        f: impl FnOnce(&mut Option<ferrite_world::BlockEntity>) -> (bool, R),
    ) -> anyhow::Result<R> {
        let key = (x.div_euclid(16), z.div_euclid(16));
        let local = (x.rem_euclid(16) as u8, y, z.rem_euclid(16) as u8);
        let mut cache = self.cache.write().unwrap();
        let (orig_chunk, light) = if let Some(d) = cache.dirty.get(&key) {
            (Arc::clone(&d.chunk), d.light.clone())
        } else {
            let column = cache
                .ready_column(key.0, key.1)
                .ok_or_else(|| anyhow::anyhow!("chunk not loaded"))?;
            (Arc::clone(&column.chunk), column.light.clone())
        };
        let mut entity = orig_chunk.block_entities.get(&local).cloned();
        let (changed, result) = f(&mut entity);
        if !changed {
            return Ok(result);
        }
        anyhow::ensure!(
            self.path.is_none()
                || cache.dirty.contains_key(&key)
                || cache.dirty.len() < CACHE_LIMIT,
            "dirty chunk cache is full"
        );
        let mut chunk = orig_chunk;
        let chunk_mut = Arc::make_mut(&mut chunk);
        if let Some(entity) = entity {
            chunk_mut.block_entities.insert(local, entity);
        } else {
            chunk_mut.block_entities.remove(&local);
        }
        self.stats.encodes.fetch_add(1, Ordering::Relaxed);
        let encoder = Arc::clone(&self.shared_encoder.read().unwrap());
        let (packet, frame) = encode_column_packet(key.0, key.1, &chunk, &light, &encoder)?;
        let version = cache.clock.fetch_add(1, Ordering::Relaxed) + 1;
        invalidate(&mut cache, &self.stats, key.0, key.1);
        cache.dirty.insert(
            key,
            Dirty {
                chunk,
                light,
                packet,
                frame,
                version,
            },
        );
        Ok(result)
    }
    pub fn set_block(&self, x: i32, y: i32, z: i32, state: u32) -> anyhow::Result<()> {
        self.compare_set_block(x, y, z, None, state)
    }
    pub fn compare_set_block(
        &self,
        x: i32,
        y: i32,
        z: i32,
        expected: Option<u32>,
        state: u32,
    ) -> anyhow::Result<()> {
        let key = (x.div_euclid(16), z.div_euclid(16));
        let mut cache = self.cache.write().unwrap();
        let (mut chunk, light) = if let Some(d) = cache.dirty.get(&key) {
            (Arc::clone(&d.chunk), d.light.clone())
        } else {
            let column = cache
                .ready_column(key.0, key.1)
                .ok_or_else(|| anyhow::anyhow!("chunk not loaded"))?;
            (Arc::clone(&column.chunk), column.light.clone())
        };
        anyhow::ensure!(
            self.path.is_none()
                || cache.dirty.contains_key(&key)
                || cache.dirty.len() < CACHE_LIMIT,
            "dirty chunk cache is full"
        );
        if let Some(expected) = expected {
            anyhow::ensure!(
                chunk.block(x.rem_euclid(16) as u8, y, z.rem_euclid(16) as u8) == Some(expected),
                "block changed"
            );
        }
        Arc::make_mut(&mut chunk).set_block(
            x.rem_euclid(16) as u8,
            y,
            z.rem_euclid(16) as u8,
            state,
            &*self.registry,
        )?;
        self.stats.encodes.fetch_add(1, Ordering::Relaxed);
        let encoder = Arc::clone(&self.shared_encoder.read().unwrap());
        let (packet, frame) = encode_column_packet(key.0, key.1, &chunk, &light, &encoder)?;
        let version = cache.clock.fetch_add(1, Ordering::Relaxed) + 1;
        invalidate(&mut cache, &self.stats, key.0, key.1);
        cache.dirty.insert(
            key,
            Dirty {
                chunk,
                light,
                packet,
                frame,
                version,
            },
        );
        if self.lighting_pool.load(Ordering::Relaxed) {
            cache.pending_relight.insert(key);
        }
        Ok(())
    }
    /// Atomically applies a set of expected-state block changes to loaded chunks.
    pub fn compare_set_blocks(&self, changes: &[([i32; 3], u32, u32)]) -> anyhow::Result<()> {
        let mut cache = self.cache.write().unwrap();
        let mut edited = std::collections::BTreeMap::new();
        let mut positions = std::collections::HashSet::new();
        for &(pos @ [x, y, z], expected, state) in changes {
            anyhow::ensure!(positions.insert(pos), "duplicate block position");
            let key = (x.div_euclid(16), z.div_euclid(16));
            if let std::collections::btree_map::Entry::Vacant(entry) = edited.entry(key) {
                let column = if let Some(dirty) = cache.dirty.get(&key) {
                    (Arc::clone(&dirty.chunk), dirty.light.clone())
                } else {
                    let column = cache
                        .ready_column(key.0, key.1)
                        .ok_or_else(|| anyhow::anyhow!("chunk not loaded"))?;
                    (Arc::clone(&column.chunk), column.light.clone())
                };
                entry.insert(column);
            }
            let (chunk, _) = edited.get_mut(&key).expect("inserted above");
            let local_x = x.rem_euclid(16) as u8;
            let local_z = z.rem_euclid(16) as u8;
            anyhow::ensure!(
                chunk.block(local_x, y, local_z) == Some(expected),
                "block changed"
            );
            Arc::make_mut(chunk).set_block(local_x, y, local_z, state, &*self.registry)?;
        }
        let new_dirty = edited
            .keys()
            .filter(|key| !cache.dirty.contains_key(key))
            .count();
        anyhow::ensure!(
            self.path.is_none() || cache.dirty.len() + new_dirty <= CACHE_LIMIT,
            "dirty chunk cache is full"
        );
        // Encoding can fail: prepare every packet before publishing any modified chunk.
        let encoder = Arc::clone(&self.shared_encoder.read().unwrap());
        let mut prepared = Vec::with_capacity(edited.len());
        for ((x, z), (chunk, light)) in edited {
            let (packet, frame) = encode_column_packet(x, z, &chunk, &light, &encoder)?;
            prepared.push(((x, z), chunk, light, packet, frame));
        }
        let track_relight = self.lighting_pool.load(Ordering::Relaxed);
        for ((x, z), chunk, light, packet, frame) in prepared {
            self.stats.encodes.fetch_add(1, Ordering::Relaxed);
            let version = cache.clock.fetch_add(1, Ordering::Relaxed) + 1;
            invalidate(&mut cache, &self.stats, x, z);
            cache.dirty.insert(
                (x, z),
                Dirty {
                    chunk,
                    light,
                    packet,
                    frame,
                    version,
                },
            );
            if track_relight {
                cache.pending_relight.insert((x, z));
            }
        }
        Ok(())
    }

    pub fn enable_lighting_pool(&self, enabled: bool) {
        self.lighting_pool.store(enabled, Ordering::Relaxed);
    }

    pub fn set_shared_compression(
        &self,
        threshold: Option<usize>,
        level: u32,
        skip_incompressible: bool,
    ) {
        *self.shared_encoder.write().unwrap() = Arc::new(FrameEncoder::new_compressed(
            threshold,
            level,
            skip_incompressible,
        ));
    }

    fn light_pool(&self, workers: usize) -> Arc<ferrite_light::LightPool> {
        let workers = workers.max(1);
        let mut guard = self.light_pool.lock().unwrap();
        if let Some((w, pool)) = &*guard
            && *w == workers
        {
            return Arc::clone(pool);
        }
        let pool = Arc::new(super::lighting::pool_for_dimension(self.dimension, workers));
        *guard = Some((workers, Arc::clone(&pool)));
        pool
    }

    /// Relights every loaded chunk in `coords` using `ferrite-light`'s parallel [`LightPool`],
    /// preparing every packet before publishing the whole batch under one cache lock.
    pub fn relight_batch(&self, coords: &[(i32, i32)], workers: usize) -> anyhow::Result<usize> {
        let mut targets = Vec::with_capacity(coords.len());
        let mut seen = HashSet::with_capacity(coords.len());
        let snapshot = {
            let cache = self.cache.read().unwrap();
            for &coord in coords {
                if !seen.insert(coord) {
                    continue;
                }
                if cache.dirty.contains_key(&coord)
                    || cache.ready_column(coord.0, coord.1).is_some()
                {
                    targets.push(coord);
                }
            }
            if targets.is_empty() {
                return Ok(0);
            }
            let needed = super::lighting::expand_neighbourhoods(&targets);
            let mut map = HashMap::with_capacity(needed.len());
            for (nx, nz) in needed {
                if let Some(d) = cache.dirty.get(&(nx, nz)) {
                    map.insert((nx, nz), Arc::clone(&d.chunk));
                } else if let Some(col) = cache.ready_column(nx, nz) {
                    map.insert((nx, nz), Arc::clone(&col.chunk));
                }
            }
            map
        };

        let pool = self.light_pool(workers);
        let relit = super::lighting::relight_batch_from_snapshot(&pool, &targets, snapshot)?;
        if relit.is_empty() {
            return Ok(0);
        }

        // Batch-wide atomic publication: build every encoded packet before mutating any cache slot.
        let encoder = Arc::clone(&self.shared_encoder.read().unwrap());
        let mut cache = self.cache.write().unwrap();
        let mut prepared = Vec::with_capacity(relit.len());
        for ((cx, cz), light) in relit {
            if let Some(d) = cache.dirty.get(&(cx, cz)) {
                let (packet, frame) = encode_column_packet(cx, cz, &d.chunk, &light, &encoder)?;
                prepared.push((
                    (cx, cz),
                    true,
                    Arc::clone(&d.chunk),
                    light,
                    packet,
                    frame,
                    false,
                ));
            } else if let Some(col) = cache.ready_column(cx, cz) {
                let (packet, frame) = encode_column_packet(cx, cz, &col.chunk, &light, &encoder)?;
                prepared.push((
                    (cx, cz),
                    false,
                    Arc::clone(&col.chunk),
                    light,
                    packet,
                    frame,
                    col.generated,
                ));
            }
        }
        let published = prepared.len();
        for ((cx, cz), is_dirty, chunk, light, packet, frame, generated) in prepared {
            self.stats.encodes.fetch_add(1, Ordering::Relaxed);
            let version = cache.clock.fetch_add(1, Ordering::Relaxed) + 1;
            invalidate(&mut cache, &self.stats, cx, cz);
            if is_dirty {
                cache.dirty.insert(
                    (cx, cz),
                    Dirty {
                        chunk,
                        light,
                        packet,
                        frame,
                        version,
                    },
                );
            } else {
                let hint = cache.biome_hint.load(Ordering::Relaxed);
                let key = (cx, cz, hint);
                cache.entries.insert(
                    key,
                    Entry::Ready(
                        Ok(Box::new(Column {
                            chunk,
                            light,
                            packet,
                            frame,
                            generated,
                        })),
                        AtomicU64::new(version),
                    ),
                );
                cache.by_coord.insert((cx, cz), hint);
            }
        }
        Ok(published)
    }

    pub fn kick_pending_relight(self: &Arc<Self>, workers: usize) {
        if !self.lighting_pool.load(Ordering::Relaxed)
            || self
                .relight_in_flight
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_err()
        {
            return;
        }
        let pending: Vec<(i32, i32)> = {
            let mut cache = self.cache.write().unwrap();
            cache.pending_relight.drain().collect()
        };
        if pending.is_empty() {
            self.relight_in_flight.store(false, Ordering::Release);
            return;
        }
        let targets = super::lighting::expand_neighbourhoods(&pending);
        let source = Arc::clone(self);
        self.runtime.spawn_blocking(move || {
            if let Err(error) = source.relight_batch(&targets, workers) {
                tracing::warn!(%error, "batched lighting pool update failed");
            }
            source.relight_in_flight.store(false, Ordering::Release);
        });
    }

    /// Sets the biome of every 4x4x4 cell touched by the block box `min..=max` (`/fillbiome`).
    /// `accept` sees the cell's current biome name and decides whether it is replaced. Returns
    /// the number of cells changed and the chunks that changed; fails, changing nothing, when a
    /// chunk of the box is not loaded or the biome is unknown.
    pub fn fill_biome(
        &self,
        min: [i32; 3],
        max: [i32; 3],
        biome: &str,
        accept: &dyn Fn(&str) -> bool,
    ) -> anyhow::Result<(usize, Vec<(i32, i32)>)> {
        let id = self
            .registry
            .biome_id(biome)
            .ok_or_else(|| anyhow::anyhow!("unknown biome {biome}"))?;
        let mut edited = Vec::new();
        let mut cache = self.cache.write().unwrap();
        let mut count = 0;
        for cx in min[0].div_euclid(16)..=max[0].div_euclid(16) {
            for cz in min[2].div_euclid(16)..=max[2].div_euclid(16) {
                let key = (cx, cz);
                let (orig_chunk, light) = if let Some(d) = cache.dirty.get(&key) {
                    (Arc::clone(&d.chunk), &d.light)
                } else {
                    let column = cache
                        .ready_column(key.0, key.1)
                        .ok_or_else(|| anyhow::anyhow!("chunk not loaded"))?;
                    (Arc::clone(&column.chunk), &column.light)
                };
                let mut edited_chunk: Option<Arc<Chunk>> = None;
                let mut changed = 0;
                for qy in (min[1] >> 2)..=(max[1] >> 2) {
                    let section = usize::try_from((qy >> 2) + 4).ok();
                    let Some(section) = section.filter(|s| *s < orig_chunk.sections().len()) else {
                        continue;
                    };
                    for qz in (min[2] >> 2)..=(max[2] >> 2) {
                        for qx in (min[0] >> 2)..=(max[0] >> 2) {
                            if qx.div_euclid(4) != cx || qz.div_euclid(4) != cz {
                                continue;
                            }
                            let index = (((qy & 3) as usize) << 4)
                                | (((qz & 3) as usize) << 2)
                                | ((qx & 3) as usize);
                            let current = edited_chunk.as_deref().unwrap_or(&orig_chunk).sections()
                                [section]
                                .biomes()
                                .get(index);
                            if current == Some(id) {
                                continue;
                            }
                            let name = current.and_then(|c| self.registry.biome_name(c));
                            if !name.is_some_and(accept) {
                                continue;
                            }
                            let chunk = edited_chunk.get_or_insert_with(|| Arc::clone(&orig_chunk));
                            Arc::make_mut(chunk).sections_mut()[section].set_biome(index, id)?;
                            changed += 1;
                        }
                    }
                }
                if changed > 0 {
                    count += changed;
                    edited.push((
                        key,
                        edited_chunk.expect("edited chunk present"),
                        light.clone(),
                    ));
                }
            }
        }
        let encoder = Arc::clone(&self.shared_encoder.read().unwrap());
        let mut chunks = Vec::new();
        for ((x, z), chunk, light) in edited {
            self.stats.encodes.fetch_add(1, Ordering::Relaxed);
            let (packet, frame) = encode_column_packet(x, z, &chunk, &light, &encoder)?;
            let version = cache.clock.fetch_add(1, Ordering::Relaxed) + 1;
            invalidate(&mut cache, &self.stats, x, z);
            cache.dirty.insert(
                (x, z),
                Dirty {
                    chunk,
                    light,
                    packet,
                    frame,
                    version,
                },
            );
            chunks.push((x, z));
        }
        Ok((count, chunks))
    }
    /// Records a modified column, with caller-supplied light, for streaming and eventual Anvil
    /// writeback. Gameplay edits go through [`Self::set_block`], which does the same bookkeeping;
    /// this exists so tests can inject a column with exact light data.
    #[cfg(test)]
    pub fn mark_modified(
        &self,
        x: i32,
        z: i32,
        chunk: ferrite_world::Chunk,
        light: &LightData,
    ) -> anyhow::Result<()> {
        self.stats.encodes.fetch_add(1, Ordering::Relaxed);
        let encoder = Arc::clone(&self.shared_encoder.read().unwrap());
        let (packet, frame) = encode_column_packet(x, z, &chunk, light, &encoder)?;
        let mut cache = self.cache.write().unwrap();
        anyhow::ensure!(
            self.path.is_none()
                || cache.dirty.contains_key(&(x, z))
                || cache.dirty.len() < CACHE_LIMIT,
            "dirty chunk cache is full"
        );
        let version = cache.clock.fetch_add(1, Ordering::Relaxed) + 1;
        invalidate(&mut cache, &self.stats, x, z);
        cache.dirty.insert(
            (x, z),
            Dirty {
                chunk: Arc::new(chunk),
                light: light.clone(),
                packet,
                frame,
                version,
            },
        );
        Ok(())
    }
    pub async fn flush_chunk(self: &Arc<Self>, x: i32, z: i32) -> anyhow::Result<()> {
        if self.path.is_none() && self.fworld.is_none() {
            return Ok(());
        }
        let _guard = self.flush_lock.lock().await;
        self.flush_chunk_locked(x, z).await
    }
    /// [`Self::flush_chunk`] for a caller that already holds `flush_lock`.
    async fn flush_chunk_locked(self: &Arc<Self>, x: i32, z: i32) -> anyhow::Result<()> {
        // A generated column has no dirty entry: it is still cached and has to reach disk like
        // a modified one. `dirty_version` distinguishes the two cases for the cleanup below.
        let item = {
            let cache = self.cache.read().unwrap();
            if let Some(d) = cache.dirty.get(&(x, z)) {
                Some((Arc::clone(&d.chunk), d.light.clone(), Some(d.version)))
            } else {
                cache
                    .ready_column(x, z)
                    .filter(|col| col.generated)
                    .map(|col| (Arc::clone(&col.chunk), col.light.clone(), None))
            }
        };
        let Some((chunk, light, dirty_version)) = item else {
            return Ok(());
        };
        let source = Arc::clone(self);
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let bytes =
                serialize_column_nbt(x, z, &chunk, &light, source.dimension, &source.registry)?;
            if let Some(fworld) = &source.fworld {
                fworld.world.lock().unwrap().insert_raw(x, z, &bytes)?;
                fworld.dirty.store(true, Ordering::Release);
            } else if let Some(path) = &source.path {
                let dir = path.join(source.dimension.region_dir());
                std::fs::create_dir_all(&dir)?;
                let rx = x.div_euclid(32);
                let rz = z.div_euclid(32);
                let region = RegionFile::open(dir.join(format!("r.{rx}.{rz}.mca")), rx, rz)?;
                region.write_chunk(x, z, &bytes, source.region_compression)?;
            }
            Ok(())
        })
        .await??;
        let mut cache = self.cache.write().unwrap();
        match dirty_version {
            Some(version) => {
                if cache
                    .dirty
                    .get(&(x, z))
                    .is_some_and(|d| d.version == version)
                    && let Some(flushed) = cache.dirty.remove(&(x, z))
                {
                    let hint = cache.biome_hint.load(Ordering::Relaxed);
                    let now = cache.clock.fetch_add(1, Ordering::Relaxed) + 1;
                    cache.entries.insert(
                        (x, z, hint),
                        Entry::Ready(
                            Ok(Box::new(Column {
                                chunk: flushed.chunk,
                                light: flushed.light,
                                packet: flushed.packet,
                                frame: flushed.frame,
                                generated: false,
                            })),
                            AtomicU64::new(now),
                        ),
                    );
                    cache.by_coord.insert((x, z), hint);
                }
            }
            None => {
                for (&(cx, cz, _), entry) in cache.entries.iter_mut() {
                    if let Entry::Ready(Ok(column), _) = entry
                        && (cx, cz) == (x, z)
                    {
                        column.generated = false;
                    }
                }
            }
        }
        Ok(())
    }
    /// Every column that still has to reach disk: modified ones plus generated-but-unsaved ones.
    fn pending_flush_keys(&self) -> Vec<(i32, i32)> {
        let cache = self.cache.read().unwrap();
        let mut keys: Vec<(i32, i32)> = cache.dirty.keys().copied().collect();
        for (&(x, z, _), entry) in cache.entries.iter() {
            if let Entry::Ready(Ok(column), _) = entry
                && column.generated
                && !keys.contains(&(x, z))
            {
                keys.push((x, z));
            }
        }
        keys
    }
    /// Saves every pending column. Columns are grouped by region file: one file is written by a
    /// single task at a time (a `RegionFile` is opened per write), while different region files
    /// are written concurrently, at most [`FLUSH_PARALLELISM`] at once.
    /// When `.fworld` is active, all pending columns are serialized and compressed in one
    /// multi-core parallel batch before the atomic `.fworld` disk flush.
    pub async fn flush_all(self: &Arc<Self>) -> anyhow::Result<()> {
        if self.path.is_none() && self.fworld.is_none() {
            return Ok(());
        }
        let _guard = self.flush_lock.lock().await;
        if let Some(fworld) = &self.fworld {
            loop {
                type PendingBatchItem = (i32, i32, Arc<Chunk>, LightData, Option<u64>);
                let batch: Vec<PendingBatchItem> = {
                    let cache = self.cache.read().unwrap();
                    let mut items = Vec::new();
                    let mut seen = HashSet::new();
                    for (&(x, z), d) in &cache.dirty {
                        seen.insert((x, z));
                        items.push((x, z, Arc::clone(&d.chunk), d.light.clone(), Some(d.version)));
                    }
                    for (&(x, z, _), entry) in &cache.entries {
                        if let Entry::Ready(Ok(column), _) = entry
                            && column.generated
                            && seen.insert((x, z))
                        {
                            items.push((
                                x,
                                z,
                                Arc::clone(&column.chunk),
                                column.light.clone(),
                                None,
                            ));
                        }
                    }
                    items
                };
                if batch.is_empty() {
                    if fworld.dirty.swap(false, Ordering::AcqRel) {
                        let store = Arc::clone(fworld);
                        let res = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                            store
                                .world
                                .lock()
                                .unwrap()
                                .write_file_atomic(&store.blob_path)?;
                            Ok(())
                        })
                        .await;
                        match res {
                            Ok(Ok(())) => {}
                            Ok(Err(err)) => {
                                fworld.dirty.store(true, Ordering::Release);
                                return Err(err);
                            }
                            Err(join_err) => {
                                fworld.dirty.store(true, Ordering::Release);
                                return Err(join_err.into());
                            }
                        }
                    }
                    return Ok(());
                }
                let versions: Vec<(i32, i32, Option<u64>)> =
                    batch.iter().map(|&(x, z, _, _, ver)| (x, z, ver)).collect();
                let source = Arc::clone(self);
                let store = Arc::clone(fworld);
                tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                    let workers = std::thread::available_parallelism()
                        .map(|n| n.get())
                        .unwrap_or(1)
                        .min(batch.len())
                        .max(1);
                    let raw_entries: Vec<((i32, i32), Vec<u8>)> = if workers <= 1 || batch.len() < 4
                    {
                        let mut out = Vec::with_capacity(batch.len());
                        for (x, z, chunk, light, _) in &batch {
                            let bytes = serialize_column_nbt(
                                *x,
                                *z,
                                chunk,
                                light,
                                source.dimension,
                                &source.registry,
                            )?;
                            out.push(((*x, *z), bytes));
                        }
                        out
                    } else {
                        type NbtBatchResult = anyhow::Result<Vec<((i32, i32), Vec<u8>)>>;
                        let chunk_size = batch.len().div_ceil(workers);
                        let mut results: Vec<NbtBatchResult> =
                            (0..batch.len().div_ceil(chunk_size))
                                .map(|_| Ok(Vec::new()))
                                .collect();
                        std::thread::scope(|s| {
                            for (slot, slice) in results.iter_mut().zip(batch.chunks(chunk_size)) {
                                let reg = &source.registry;
                                let dim = source.dimension;
                                s.spawn(move || {
                                    let mut local = Vec::with_capacity(slice.len());
                                    for (x, z, chunk, light, _) in slice {
                                        match serialize_column_nbt(*x, *z, chunk, light, dim, reg) {
                                            Ok(bytes) => local.push(((*x, *z), bytes)),
                                            Err(e) => {
                                                *slot = Err(e);
                                                return;
                                            }
                                        }
                                    }
                                    *slot = Ok(local);
                                });
                            }
                        });
                        let mut out = Vec::with_capacity(batch.len());
                        for res in results {
                            out.extend(res?);
                        }
                        out
                    };
                    store.world.lock().unwrap().insert_raw_batch(&raw_entries)?;
                    store.dirty.store(true, Ordering::Release);
                    Ok(())
                })
                .await??;
                let mut cache = self.cache.write().unwrap();
                let hint = cache.biome_hint.load(Ordering::Relaxed);
                for (x, z, dirty_version) in versions {
                    match dirty_version {
                        Some(version) => {
                            if cache
                                .dirty
                                .get(&(x, z))
                                .is_some_and(|d| d.version == version)
                                && let Some(flushed) = cache.dirty.remove(&(x, z))
                            {
                                let now = cache.clock.fetch_add(1, Ordering::Relaxed) + 1;
                                cache.entries.insert(
                                    (x, z, hint),
                                    Entry::Ready(
                                        Ok(Box::new(Column {
                                            chunk: flushed.chunk,
                                            light: flushed.light,
                                            packet: flushed.packet,
                                            frame: flushed.frame,
                                            generated: false,
                                        })),
                                        AtomicU64::new(now),
                                    ),
                                );
                                cache.by_coord.insert((x, z), hint);
                            }
                        }
                        None => {
                            for (&(cx, cz, _), entry) in cache.entries.iter_mut() {
                                if let Entry::Ready(Ok(column), _) = entry
                                    && (cx, cz) == (x, z)
                                {
                                    column.generated = false;
                                }
                            }
                        }
                    }
                }
            }
        }
        loop {
            let keys = self.pending_flush_keys();
            if keys.is_empty() {
                return Ok(());
            }
            let mut groups: HashMap<(i32, i32), Vec<(i32, i32)>> = HashMap::new();
            for (x, z) in keys {
                groups
                    .entry((x.div_euclid(32), z.div_euclid(32)))
                    .or_default()
                    .push((x, z));
            }
            let mut groups: Vec<Vec<(i32, i32)>> = groups.into_values().collect();
            while !groups.is_empty() {
                let wave: Vec<_> = groups.split_off(groups.len().saturating_sub(FLUSH_PARALLELISM));
                let mut tasks = tokio::task::JoinSet::new();
                for group in wave {
                    let source = Arc::clone(self);
                    tasks.spawn(async move {
                        for (x, z) in group {
                            source.flush_chunk_locked(x, z).await?;
                        }
                        anyhow::Ok(())
                    });
                }
                while let Some(joined) = tasks.join_next().await {
                    joined??;
                }
            }
        }
    }
    /// Flush after callers have stopped scheduling loads and mutations for this source.
    /// Existing background loads are allowed to finish before generated columns are saved.
    pub async fn flush_quiesced(self: &Arc<Self>) -> anyhow::Result<()> {
        loop {
            let notified = self.pending_done.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.cache.read().unwrap().pending == 0 {
                break;
            }
            notified.await;
        }
        self.flush_all().await
    }
    /// Encoded `level_chunk_with_light` body for `(x, z)`, starting a
    /// background load on first use. `None` means the chunk is still loading.
    pub fn get(self: &Arc<Self>, x: i32, z: i32, biome: i32) -> Option<Result<Arc<[u8]>, String>> {
        match self.lookup(x, z, biome) {
            Lookup::Ready(packet, _) => Some(Ok(packet)),
            Lookup::Failed(error) => Some(Err(error)),
            Lookup::Loading => None,
        }
    }
    /// Pre-compressed `SharedFrame` for `(x, z)`, starting a background load on first use.
    /// Allows all players receiving this chunk to share the same pre-compressed wire frame.
    pub fn get_frame(
        self: &Arc<Self>,
        x: i32,
        z: i32,
        biome: i32,
    ) -> Option<Result<Arc<SharedFrame>, String>> {
        match self.lookup(x, z, biome) {
            Lookup::Ready(_, frame) => Some(Ok(frame)),
            Lookup::Failed(error) => Some(Err(error)),
            Lookup::Loading => None,
        }
    }
    /// Ensure `(x, z)` is being loaded without copying its packet. Prefetching
    /// must not go through [`Self::get`]: that copied the whole body for every
    /// player while holding the cache lock, only to throw it away.
    pub fn prefetch(self: &Arc<Self>, x: i32, z: i32, biome: i32) {
        let _ = self.lookup(x, z, biome);
    }
    fn lookup(self: &Arc<Self>, x: i32, z: i32, biome: i32) -> Lookup {
        let key = (x, z, biome);
        let lookups = self.stats.lookups.fetch_add(1, Ordering::Relaxed) + 1;
        if lookups.is_multiple_of(CACHE_LOG_INTERVAL) {
            let stats = self.cache_stats();
            tracing::debug!(
                lookups = stats.lookups,
                hits = stats.hits,
                hit_rate = stats.hit_rate(),
                encodes = stats.encodes,
                invalidations = stats.invalidations,
                "chunk packet cache"
            );
        }
        // Fast path: shared read lock for cache hits and already-pending loads.
        {
            let cache = self.cache.read().unwrap();
            cache.biome_hint.store(biome, Ordering::Relaxed);
            let now = cache.clock.fetch_add(1, Ordering::Relaxed) + 1;
            if let Some(dirty) = cache.dirty.get(&(x, z)) {
                self.stats.hits.fetch_add(1, Ordering::Relaxed);
                return Lookup::Ready(Arc::clone(&dirty.packet), Arc::clone(&dirty.frame));
            }
            if let Some(entry) = cache.entries.get(&key) {
                return match entry {
                    Entry::Pending(_) => Lookup::Loading,
                    Entry::Ready(data, used) => {
                        used.store(now, Ordering::Relaxed);
                        self.stats.hits.fetch_add(1, Ordering::Relaxed);
                        match data {
                            Ok(column) => {
                                Lookup::Ready(Arc::clone(&column.packet), Arc::clone(&column.frame))
                            }
                            Err(error) => Lookup::Failed(error.clone()),
                        }
                    }
                };
            }
        }
        // Slow path: exclusive write lock for cache misses.
        let mut cache = self.cache.write().unwrap();
        cache.biome_hint.store(biome, Ordering::Relaxed);
        let now = cache.clock.fetch_add(1, Ordering::Relaxed) + 1;
        if let Some(dirty) = cache.dirty.get(&(x, z)) {
            self.stats.hits.fetch_add(1, Ordering::Relaxed);
            return Lookup::Ready(Arc::clone(&dirty.packet), Arc::clone(&dirty.frame));
        }
        if let Some(entry) = cache.entries.get(&key) {
            return match entry {
                Entry::Pending(_) => Lookup::Loading,
                Entry::Ready(data, used) => {
                    used.store(now, Ordering::Relaxed);
                    self.stats.hits.fetch_add(1, Ordering::Relaxed);
                    match data {
                        Ok(column) => {
                            Lookup::Ready(Arc::clone(&column.packet), Arc::clone(&column.frame))
                        }
                        Err(error) => Lookup::Failed(error.clone()),
                    }
                }
            };
        }
        if cache.pending >= PENDING_LIMIT {
            return Lookup::Loading;
        }
        if cache.entries.len() >= CACHE_LIMIT {
            // Drop the least recently used eighth in one pass, so the scan under the lock is paid
            // once per `EVICT_BATCH` misses instead of on every miss.
            let mut ready: Vec<_> = cache
                .entries
                .iter()
                .filter_map(|(k, v)| match v {
                    Entry::Ready(_, used) => Some((used.load(Ordering::Relaxed), *k)),
                    Entry::Pending(_) => None,
                })
                .collect();
            if ready.is_empty() {
                return Lookup::Loading;
            }
            let batch = EVICT_BATCH.min(ready.len());
            ready.select_nth_unstable(batch - 1);
            for (_, k) in &ready[..batch] {
                cache.entries.remove(k);
                if cache.by_coord.get(&(k.0, k.1)) == Some(&k.2) {
                    cache.by_coord.remove(&(k.0, k.1));
                }
            }
        }
        cache.entries.insert(key, Entry::Pending(now));
        cache.pending += 1;
        drop(cache);
        let source = Arc::clone(self);
        let request = now;
        self.runtime.spawn_blocking(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> anyhow::Result<Column> {
                let loaded = if let Some(fworld) = &source.fworld {
                    let bytes = fworld.world.lock().unwrap().get_raw(x, z)?;
                    match bytes {
                        Some(raw) => {
                            let (_, tag) = binary::read_named(&raw)?;
                            Some(column_from_tag(&tag, source.dimension, &source.registry)?)
                        }
                        None => None,
                    }
                } else {
                    match &source.path {
                        Some(path) => {
                            read_column(path, &source.registry, source.dimension, x, z)?
                        }
                        None => None,
                    }
                };
                let generated =
                    loaded.is_none() && (source.path.is_some() || source.fworld.is_some());
                let (chunk, light) = match loaded {
                    Some(column) => column,
                    None => {
                        if let Some(terrain) = &source.terrain {
                            let generated = terrain.generate(source.dimension, x, z)?;
                            let light = if source.lighting_pool.load(Ordering::Relaxed) {
                                super::lighting::compute_initial_light(
                                    source.dimension,
                                    x,
                                    z,
                                    &generated.chunk,
                                )
                            } else {
                                generated.light
                            };
                            (generated.chunk, light)
                        } else {
                            let mut flat = source.flat.lock().unwrap();
                            if let Some(column) = flat.get(&biome) {
                                column.clone()
                            } else {
                                let column = (
                                    flat_chunk(&source.registry, biome, source.dimension)?,
                                    LightData {
                                        sky: vec![
                                            LightSection::Empty;
                                            source.dimension.light_sections()
                                        ],
                                        block: vec![
                                            LightSection::Empty;
                                            source.dimension.light_sections()
                                        ],
                                    },
                                );
                                flat.insert(biome, column.clone());
                                column
                            }
                        }
                    }
                };
                source.stats.encodes.fetch_add(1, Ordering::Relaxed);
                let encoder = Arc::clone(&source.shared_encoder.read().unwrap());
                let (packet, frame) = encode_column_packet(x, z, &chunk, &light, &encoder)?;
                Ok(Column {
                    chunk: Arc::new(chunk),
                    light,
                    packet,
                    frame,
                    generated,
                })
            }))
            .map_err(|_| "chunk loading task panicked".to_owned())
            .and_then(|result| result.map_err(|e| format!("{e:#}")));
            let mut cache = source.cache.write().unwrap();
            let now = cache.clock.fetch_add(1, Ordering::Relaxed) + 1;
            if matches!(cache.entries.get(&key), Some(Entry::Pending(version)) if *version == request)
            {
                let is_ok = result.is_ok();
                cache.entries.insert(
                    key,
                    Entry::Ready(result.map(Box::new), AtomicU64::new(now)),
                );
                if is_ok {
                    cache.by_coord.insert((x, z), biome);
                }
                cache.pending -= 1;
            }
            drop(cache);
            source.pending_done.notify_waiters();
        });
        Lookup::Loading
    }
}
fn serialize_column_nbt(
    x: i32,
    z: i32,
    chunk: &Chunk,
    light: &LightData,
    dimension: Dimension,
    registry: &RuntimeRegistry,
) -> anyhow::Result<Vec<u8>> {
    let mut tag = ferrite_world::chunk_to_nbt(chunk, registry)?;
    if let Tag::Compound(root) = &mut tag {
        root.insert("xPos".into(), Tag::Int(x));
        root.insert("zPos".into(), Tag::Int(z));
        if let Some(Tag::List(List::Compound(entities))) = root.get_mut("block_entities") {
            for entity in entities {
                for (key, chunk_coord) in [("x", x), ("z", z)] {
                    if let Some(Tag::Int(value)) = entity.get_mut(key) {
                        *value = chunk_coord * 16 + value.rem_euclid(16);
                    }
                }
            }
        }
    }
    apply_light(&mut tag, light, dimension)?;
    Ok(binary::write_named("", &tag)?)
}
/// Encode the complete `level_chunk_with_light` body (packet id + payload) and its pre-compressed
/// [`SharedFrame`] once so every recipient can share both the raw and compressed wire bytes.
fn encode_column_packet(
    x: i32,
    z: i32,
    chunk: &Chunk,
    light: &LightData,
    encoder: &FrameEncoder,
) -> anyhow::Result<(Arc<[u8]>, Arc<SharedFrame>)> {
    let body = encode_chunk_packet(x, z, chunk, light)?;
    let mut packet = Vec::with_capacity(5 + body.len());
    VarInt(ChunkData::ID).encode(&mut packet);
    packet.extend_from_slice(&body);
    let raw: Arc<[u8]> = Arc::from(packet);
    let frame = shared_frame_arc(encoder, Arc::clone(&raw));
    Ok((raw, frame))
}
/// Drop every cached packet for chunk `(x, z)` after a block or light change.
fn invalidate(cache: &mut Cache, stats: &Stats, x: i32, z: i32) {
    let before = cache.entries.len();
    let mut dropped_pending = 0;
    cache.entries.retain(|&(cx, cz, _), entry| {
        let keep = (cx, cz) != (x, z);
        if !keep && matches!(entry, Entry::Pending(_)) {
            dropped_pending += 1;
        }
        keep
    });
    cache.by_coord.remove(&(x, z));
    cache.pending -= dropped_pending;
    // A modified chunk's own packet is replaced by the caller right after this; that drops a
    // cached packet as well, so repeated edits of one chunk are counted too.
    if cache.entries.len() < before || cache.dirty.contains_key(&(x, z)) {
        stats.invalidations.fetch_add(1, Ordering::Relaxed);
        tracing::debug!(x, z, "chunk packet cache invalidated");
    }
}
/// Writes the minimal vanilla `level.dat` a new world needs to remember its spawn. Existing
/// fields from a real vanilla `level.dat` are not modeled here; callers only use this for a
/// freshly created Ferrite world.
pub fn write_spawn(world: &Path, spawn: [i32; 3]) -> anyhow::Result<()> {
    write_level(world, spawn, None, None)
}

/// Writes the spawn, seed and the dimension a Ferrite runtime world was opened with.
/// `level.dat` has no vanilla field for a runtime world's dimension (vanilla stores dimensions
/// as directories), so Ferrite records its own key and reopens the world in that dimension.
pub fn write_spawn_seed_and_dimension(
    world: &Path,
    spawn: [i32; 3],
    seed: i64,
    dimension: Dimension,
) -> anyhow::Result<()> {
    write_level(world, spawn, Some(seed), Some(dimension))
}

/// The dimension recorded by [`write_spawn_seed_and_dimension`], or `None` for a world written
/// before dimensions were stored.
pub fn read_dimension(world: &Path) -> anyhow::Result<Option<Dimension>> {
    let bytes = std::fs::read(world.join("level.dat"))?;
    let (_, tag) = binary::read_compressed(&bytes, binary::Compression::Gzip)?;
    let Tag::Compound(root) = tag else {
        anyhow::bail!("level.dat root is not a compound")
    };
    let Some(Tag::Compound(data)) = root.get("Data") else {
        anyhow::bail!("level.dat missing Data")
    };
    match data.get("FerriteDimension") {
        Some(Tag::String(key)) => Ok(Dimension::from_key(key)),
        Some(_) => anyhow::bail!("level.dat FerriteDimension is not a string"),
        None => Ok(None),
    }
}

fn write_level(
    world: &Path,
    spawn: [i32; 3],
    seed: Option<i64>,
    dimension: Option<Dimension>,
) -> anyhow::Result<()> {
    let mut data = ferrite_nbt::Compound::new();
    let mut pos = ferrite_nbt::Compound::new();
    pos.insert("pos".into(), Tag::IntArray(spawn.to_vec()));
    data.insert("spawn".into(), Tag::Compound(pos));
    if let Some(seed) = seed {
        let mut settings = ferrite_nbt::Compound::new();
        settings.insert("seed".into(), Tag::Long(seed));
        data.insert("WorldGenSettings".into(), Tag::Compound(settings));
    }
    if let Some(dimension) = dimension {
        data.insert(
            "FerriteDimension".into(),
            Tag::String(dimension.key().to_owned()),
        );
    }
    data.insert("DataVersion".into(), Tag::Int(5023));
    let mut root = ferrite_nbt::Compound::new();
    root.insert("Data".into(), Tag::Compound(data));
    let bytes = binary::write_compressed("", &Tag::Compound(root), binary::Compression::Gzip)?;
    std::fs::create_dir_all(world)?;
    std::fs::write(world.join("level.dat"), bytes)?;
    Ok(())
}

/// The seed recorded by [`write_spawn_seed_and_dimension`], or `None` for a `level.dat` that has none
/// (worlds created before seeds were stored).
pub fn read_seed(world: &Path) -> anyhow::Result<Option<i64>> {
    let bytes = std::fs::read(world.join("level.dat"))?;
    let (_, tag) = binary::read_compressed(&bytes, binary::Compression::Gzip)?;
    let Tag::Compound(root) = tag else {
        anyhow::bail!("level.dat root is not a compound")
    };
    let Some(Tag::Compound(data)) = root.get("Data") else {
        anyhow::bail!("level.dat missing Data")
    };
    Ok(match data.get("WorldGenSettings") {
        Some(Tag::Compound(settings)) => match settings.get("seed") {
            Some(Tag::Long(seed)) => Some(*seed),
            Some(_) => anyhow::bail!("level.dat seed is not a long"),
            None => None,
        },
        _ => None,
    })
}

pub fn read_spawn(world: &Path) -> anyhow::Result<[i32; 3]> {
    let bytes = std::fs::read(world.join("level.dat"))?;
    let (_, tag) = binary::read_compressed(&bytes, binary::Compression::Gzip)?;
    let Tag::Compound(root) = tag else {
        anyhow::bail!("level.dat root is not a compound")
    };
    let Some(Tag::Compound(data)) = root.get("Data") else {
        anyhow::bail!("level.dat missing Data")
    };
    let Some(Tag::Compound(spawn)) = data.get("spawn") else {
        anyhow::bail!("level.dat missing spawn")
    };
    let Some(Tag::IntArray(pos)) = spawn.get("pos") else {
        anyhow::bail!("level.dat missing spawn position")
    };
    anyhow::ensure!(pos.len() == 3, "invalid spawn position");
    Ok([pos[0], pos[1], pos[2]])
}

fn apply_light(tag: &mut Tag, light: &LightData, dimension: Dimension) -> anyhow::Result<()> {
    let Tag::Compound(root) = tag else {
        anyhow::bail!("invalid chunk root")
    };
    let Some(Tag::List(List::Compound(sections))) = root.get_mut("sections") else {
        anyhow::bail!("missing sections")
    };
    for (name, values) in [("SkyLight", &light.sky), ("BlockLight", &light.block)] {
        for (idx, value) in values.iter().enumerate() {
            let data = match value {
                LightSection::Unknown => continue,
                LightSection::Empty => vec![0; 2048],
                LightSection::Data(bytes) => bytes.iter().map(|b| *b as i8).collect(),
            };
            let y = i8::try_from(idx as i32 - 1 + dimension.min_y().div_euclid(16))?;
            let section_index = match sections
                .iter()
                .position(|s| s.get("Y") == Some(&Tag::Byte(y)))
            {
                Some(i) => i,
                None => {
                    let mut section = ferrite_nbt::Compound::new();
                    section.insert("Y".into(), Tag::Byte(y));
                    sections.push(section);
                    sections.len() - 1
                }
            };
            sections[section_index].insert(name.into(), Tag::ByteArray(data));
        }
    }
    Ok(())
}

pub(super) fn flat_chunk(
    registry: &RuntimeRegistry,
    _biome: i32,
    dimension: Dimension,
) -> anyhow::Result<Chunk> {
    // The flat test world has no biome source; its generated column is plains regardless of
    // the cache hint a caller used.
    let biome = registry
        .biome_id("minecraft:plains")
        .ok_or_else(|| anyhow::anyhow!("missing minecraft:plains"))?;
    let state = |name: &str| {
        registry
            .block_id(name, &Default::default())
            .ok_or_else(|| anyhow::anyhow!("missing {name}"))
    };
    let air = state("minecraft:air")?;
    let bedrock = state("minecraft:bedrock")?;
    let mut chunk = Chunk::new(
        dimension.min_y(),
        dimension.height() as u32,
        air,
        biome,
        registry.block_bits(),
        registry.biome_bits(),
    )?;
    if dimension == Dimension::Overworld {
        let dirt = state("minecraft:dirt")?;
        let grass = state("minecraft:grass_block")?;
        for (section, layer, state) in [
            (7, 13, bedrock),
            (7, 14, dirt),
            (7, 15, dirt),
            (8, 0, grass),
        ] {
            for index in layer * 256..(layer + 1) * 256 {
                chunk.sections_mut()[section].set_block(index, state, registry)?;
            }
        }
    } else {
        // A small solid platform for the nether/end fallback, one block of bedrock at the
        // bottom. Real generation replaces it whenever the vanilla data is available.
        let top = state(match dimension {
            Dimension::Nether => "minecraft:netherrack",
            _ => "minecraft:end_stone",
        })?;
        for y in dimension.min_y()..=63 {
            let block = if y == dimension.min_y() { bedrock } else { top };
            for z in 0..16u8 {
                for x in 0..16u8 {
                    chunk.set_block(x, y, z, block, registry)?;
                }
            }
        }
    }
    chunk.recompute_heightmaps(registry);
    Ok(chunk)
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn forced_chunks_survive_a_restart_per_dimension() {
        let dir = tempfile::tempdir().unwrap();
        let registry = Arc::new(
            RuntimeRegistry::load(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../vanilla/26.3/generated"),
            )
            .unwrap(),
        );
        let open = |dimension| {
            AnvilChunkSource::with_dimension(
                dir.path().to_path_buf(),
                registry.clone(),
                None,
                dimension,
            )
        };
        let nether = open(Dimension::Nether);
        assert!(nether.load_forced_chunks().is_empty());
        let key = Dimension::Nether.key().to_owned();
        let forced: std::collections::BTreeSet<_> =
            [(key.clone(), -3, 7), (key.clone(), 0, 0)].into();
        nether.save_forced_chunks(&key, &forced).unwrap();
        assert!(
            dir.path()
                .join("DIM-1/data/minecraft/chunk_tickets.dat")
                .exists()
        );
        assert_eq!(open(Dimension::Nether).load_forced_chunks(), forced);
        assert!(open(Dimension::Overworld).load_forced_chunks().is_empty());
        nether
            .save_forced_chunks(&key, &Default::default())
            .unwrap();
        assert!(open(Dimension::Nether).load_forced_chunks().is_empty());
    }

    #[tokio::test]
    async fn chunk_saved_sees_a_chunk_on_disk_that_is_not_resident() {
        let dir = tempfile::tempdir().unwrap();
        let registry = Arc::new(
            RuntimeRegistry::load(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../vanilla/26.3/generated"),
            )
            .unwrap(),
        );
        let open = || {
            AnvilChunkSource::with_dimension(
                dir.path().to_path_buf(),
                registry.clone(),
                None,
                Dimension::Overworld,
            )
        };
        let first = open();
        assert!(!first.chunk_saved(3, -2), "nothing is on disk yet");
        assert!(first.ensure_chunk_loaded(3, -2, std::time::Duration::from_secs(10)));
        assert!(first.chunk_resident(3, -2));
        first.flush_chunk(3, -2).await.unwrap();
        assert!(first.chunk_saved(3, -2), "flushed to the region file");
        assert!(!first.chunk_saved(4, -2), "a neighbour was never written");
        drop(first);

        // A fresh source over the same directory stands in for an evicted chunk.
        let second = open();
        assert!(!second.chunk_resident(3, -2), "nothing is resident yet");
        assert!(second.chunk_saved(3, -2), "the chunk is still on disk");
        assert!(!second.chunk_resident(3, -2), "asking never loads it");
    }

    use super::*;

    async fn ready(source: &Arc<AnvilChunkSource>, x: i32, z: i32, biome: i32) -> Arc<[u8]> {
        for _ in 0..500 {
            match source.get(x, z, biome) {
                Some(Ok(packet)) => return packet,
                Some(Err(error)) => panic!("chunk ({x}, {z}) failed: {error}"),
                None => tokio::time::sleep(std::time::Duration::from_millis(2)).await,
            }
        }
        panic!("chunk ({x}, {z}) never loaded");
    }

    /// What a caller that skipped the cache would build for a flat chunk.
    fn encode_uncached(source: &AnvilChunkSource, x: i32, z: i32, biome: i32) -> Vec<u8> {
        let chunk = flat_chunk(&source.registry, biome, source.dimension).unwrap();
        let light = LightData {
            sky: vec![LightSection::Empty; 26],
            block: vec![LightSection::Empty; 26],
        };
        let body = encode_chunk_packet(x, z, &chunk, &light).unwrap();
        let mut packet = Vec::new();
        VarInt(ChunkData::ID).encode(&mut packet);
        packet.extend(body);
        packet
    }

    /// Block reads run on every game tick under the cache lock. They used to scan every cache
    /// entry looking for the chunk, so a read cost grew with the number of loaded chunks (the End
    /// fight tests spent most of their tick time here). The cost must not depend on cache size.
    #[tokio::test]
    async fn block_reads_do_not_scan_the_whole_cache() {
        async fn load(source: &Arc<AnvilChunkSource>, chunks: i32) {
            for first in (0..chunks).step_by(100) {
                for i in first..(first + 100).min(chunks) {
                    source.prefetch(i % 40, i / 40, 0);
                }
                for i in first..(first + 100).min(chunks) {
                    ready(source, i % 40, i / 40, 0).await;
                }
            }
        }
        fn best_of(source: &AnvilChunkSource) -> std::time::Duration {
            (0..7)
                .map(|_| {
                    let start = std::time::Instant::now();
                    for i in 0..20_000 {
                        std::hint::black_box(source.block(i % 16, 0, 0));
                    }
                    start.elapsed()
                })
                .min()
                .unwrap()
        }
        let small = AnvilChunkSource::new_memory();
        load(&small, 1).await;
        let large = AnvilChunkSource::new_memory();
        load(&large, 1500).await;
        assert!(small.block(1, 0, 0).is_some() && large.block(1, 0, 0).is_some());
        let (one, many) = (best_of(&small), best_of(&large));
        eprintln!("block() x20000: 1 chunk cached {one:?}, 1500 chunks cached {many:?}");
        assert!(
            many < one * 5,
            "block reads slow down with the cache: {one:?} with 1 chunk, {many:?} with 1500"
        );
    }

    /// Whichever biome variant a chunk was cached under, block reads find it.
    #[tokio::test]
    async fn block_reads_find_a_chunk_cached_under_another_biome_variant() {
        let source = AnvilChunkSource::new_memory();
        ready(&source, 3, 4, 7).await;
        source.prefetch(5, 5, 0);
        ready(&source, 5, 5, 0).await;
        // The hint now points at variant 0, but chunk (3, 4) is only cached under variant 7.
        assert!(source.block(3 * 16, 0, 4 * 16).is_some());
        assert!(source.block(5 * 16, 0, 5 * 16).is_some());
        assert!(source.block(100 * 16, 0, 0).is_none());
    }

    #[tokio::test]
    async fn two_players_share_one_encoded_packet() {
        let source = AnvilChunkSource::new_memory();
        source.prefetch(0, 0, 0);
        let first = ready(&source, 0, 0, 0).await;
        let second = ready(&source, 0, 0, 0).await;

        assert!(
            Arc::ptr_eq(&first, &second),
            "both players must receive the same cached bytes"
        );
        assert_eq!(&*first, encode_uncached(&source, 0, 0, 0));
        let stats = source.cache_stats();
        assert_eq!(stats.encodes, 1, "{stats:?}");
        assert!(stats.hits >= 2, "{stats:?}");
        assert_eq!(stats.invalidations, 0, "{stats:?}");
    }

    #[tokio::test]
    async fn fill_biome_replaces_matching_cells_across_chunks() {
        let source = AnvilChunkSource::new_memory();
        let before = ready(&source, 0, 0, 0).await;
        ready(&source, 1, 0, 0).await;
        let initial = source.biome_name(0, 64, 0).unwrap();
        assert_ne!(initial, "minecraft:desert");

        // 8 blocks wide in x across the chunk border: 2 cells on each side, 1 in y and z.
        let (count, chunks) = source
            .fill_biome([8, 64, 0], [23, 64, 3], "minecraft:desert", &|_| true)
            .unwrap();
        assert_eq!(count, 4);
        assert_eq!(chunks.len(), 2);
        assert_eq!(
            source.biome_name(8, 64, 0).as_deref(),
            Some("minecraft:desert")
        );
        assert_eq!(
            source.biome_name(20, 64, 3).as_deref(),
            Some("minecraft:desert")
        );
        assert_eq!(
            source.biome_name(4, 64, 0).as_deref(),
            Some(initial.as_str())
        );
        assert_ne!(
            &*ready(&source, 0, 0, 0).await,
            &*before,
            "the chunk is re-encoded"
        );

        // A filter that rejects everything, and cells already holding the biome, change nothing.
        let (count, _) = source
            .fill_biome([0, 64, 0], [3, 64, 3], "minecraft:desert", &|name| {
                name == "minecraft:nonexistent"
            })
            .unwrap();
        assert_eq!(count, 0);
        let (count, _) = source
            .fill_biome([8, 64, 0], [23, 64, 3], "minecraft:desert", &|_| true)
            .unwrap();
        assert_eq!(count, 0);
        assert!(
            source
                .fill_biome([0, 64, 0], [3, 64, 3], "minecraft:nope", &|_| true)
                .is_err()
        );
        assert!(
            source
                .fill_biome([160, 64, 0], [163, 64, 3], "minecraft:desert", &|_| true)
                .is_err()
        );
    }

    #[tokio::test]
    async fn block_batch_is_atomic_across_loaded_chunks() {
        let source = AnvilChunkSource::new_memory();
        ready(&source, 0, 0, 0).await;
        ready(&source, 1, 0, 0).await;
        let stone = u32::from(ferrite_data::blocks::default_state("stone").unwrap());
        let a = [15, 70, 0];
        let b = [16, 70, 0];
        let old_a = source.block(a[0], a[1], a[2]).unwrap();
        let old_b = source.block(b[0], b[1], b[2]).unwrap();
        assert!(
            source
                .compare_set_blocks(&[(a, old_a, stone), (b, old_b + 1, stone)])
                .is_err()
        );
        assert_eq!(source.block(a[0], a[1], a[2]), Some(old_a));
        assert_eq!(source.block(b[0], b[1], b[2]), Some(old_b));
        source
            .compare_set_blocks(&[(a, old_a, stone), (b, old_b, stone)])
            .unwrap();
        assert_eq!(source.block(a[0], a[1], a[2]), Some(stone));
        assert_eq!(source.block(b[0], b[1], b[2]), Some(stone));
    }

    #[tokio::test]
    async fn block_change_invalidates_only_the_edited_chunk() {
        let source = AnvilChunkSource::new_memory();
        let edited_before = ready(&source, 0, 0, 0).await;
        let neighbour = ready(&source, 1, 0, 0).await;
        assert_eq!(source.cache_stats().encodes, 2);

        let stone = source
            .registry
            .block_id("minecraft:stone", &Default::default())
            .unwrap();
        source.set_block(0, 64, 0, stone).unwrap();

        let stats = source.cache_stats();
        assert_eq!(stats.invalidations, 1, "{stats:?}");
        assert_eq!(stats.encodes, 3, "{stats:?}");
        assert_eq!(source.block(0, 64, 0), Some(stone));

        let edited_after = ready(&source, 0, 0, 0).await;
        assert_ne!(
            &*edited_before, &*edited_after,
            "an edited chunk must not keep serving its old packet"
        );
        let neighbour_after = ready(&source, 1, 0, 0).await;
        assert!(Arc::ptr_eq(&neighbour, &neighbour_after));
        assert_eq!(
            source.cache_stats().encodes,
            3,
            "neighbour must not re-encode"
        );
    }

    #[tokio::test]
    async fn light_change_invalidates_the_cached_packet() {
        let source = AnvilChunkSource::new_memory();
        let before = ready(&source, 3, 3, 0).await;
        assert_eq!(source.cache_stats().encodes, 1);

        let mut light = LightData {
            sky: vec![LightSection::Empty; 26],
            block: vec![LightSection::Empty; 26],
        };
        light.sky[8] = LightSection::Data(Box::new([0x5a; 2048]));
        let chunk = flat_chunk(&source.registry, 0, source.dimension).unwrap();
        source.mark_modified(3, 3, chunk, &light).unwrap();

        let stats = source.cache_stats();
        assert_eq!(stats.invalidations, 1, "{stats:?}");
        assert_eq!(stats.encodes, 2, "{stats:?}");
        let after = ready(&source, 3, 3, 0).await;
        assert_ne!(
            &*before, &*after,
            "a light change must re-encode the chunk packet"
        );
    }

    fn sign_entity(pos: [i32; 3], text: &str) -> ferrite_world::BlockEntity {
        let type_id = ferrite_data::registries::id("minecraft:block_entity_type", "minecraft:sign")
            .and_then(|id| u32::try_from(id).ok())
            .expect("sign block entity type");
        let mut data = ferrite_nbt::Compound::new();
        data.insert("id".into(), Tag::String("minecraft:sign".into()));
        data.insert("x".into(), Tag::Int(pos[0]));
        data.insert("y".into(), Tag::Int(pos[1]));
        data.insert("z".into(), Tag::Int(pos[2]));
        data.insert("marker".into(), Tag::String(text.into()));
        ferrite_world::BlockEntity { type_id, data }
    }

    /// Regression: a viewer asking for a chunk after a cache hit must receive the changed
    /// block entity, on the first edit and on later edits of an already modified chunk.
    #[tokio::test]
    async fn block_entity_change_reaches_a_viewer_after_a_cache_hit() {
        let source = AnvilChunkSource::new_memory();
        let pos = [3, 70, 4];
        let original = ready(&source, 0, 0, 0).await;
        // Cache hit: the packet is shared before any edit.
        assert!(Arc::ptr_eq(&original, &ready(&source, 0, 0, 0).await));

        source
            .update_block_entity(pos[0], pos[1], pos[2], |slot| {
                *slot = Some(sign_entity(pos, "first"));
                (true, ())
            })
            .unwrap();
        let first = ready(&source, 0, 0, 0).await;
        assert_ne!(&*original, &*first, "viewer still got the pre-edit packet");
        assert!(Arc::ptr_eq(&first, &ready(&source, 0, 0, 0).await));
        assert_eq!(source.cache_stats().invalidations, 1);

        // A second edit of the now-dirty chunk replaces the cached packet again.
        source
            .update_block_entity(pos[0], pos[1], pos[2], |slot| {
                *slot = Some(sign_entity(pos, "second"));
                (true, ())
            })
            .unwrap();
        let second = ready(&source, 0, 0, 0).await;
        assert_ne!(&*first, &*second, "second edit served from a stale packet");
        assert_eq!(source.cache_stats().invalidations, 2);

        // Removing the block entity is a change too.
        source
            .update_block_entity(pos[0], pos[1], pos[2], |slot| (slot.take().is_some(), ()))
            .unwrap();
        let removed = ready(&source, 0, 0, 0).await;
        assert_ne!(&*second, &*removed);
        assert_eq!(&*removed, &*original, "no block entity again");

        // A no-op closure must not re-encode or invalidate.
        let encodes = source.cache_stats().encodes;
        source
            .update_block_entity(pos[0], pos[1], pos[2], |_| (false, ()))
            .unwrap();
        assert_eq!(source.cache_stats().encodes, encodes);
    }

    #[tokio::test]
    async fn fworld_blob_persists_modified_and_generated_chunks() {
        let dir = tempfile::tempdir().unwrap();
        let blob_path = dir.path().join("world.fworld");
        let registry = Arc::new(
            RuntimeRegistry::load(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../vanilla/26.3/generated"),
            )
            .unwrap(),
        );
        const STONE: u32 = 1;

        let first = AnvilChunkSource::with_fworld(
            blob_path.clone(),
            registry.clone(),
            None,
            Dimension::Overworld,
            ferrite_world::BlobCompression::Lz4,
            [0, 65, 0],
        )
        .unwrap();
        let _ = ready(&first, 0, 0, 0).await;
        first.set_block(3, 70, 5, STONE).unwrap();
        assert_eq!(first.block(3, 70, 5), Some(STONE));
        first.flush_all().await.unwrap();
        assert!(blob_path.is_file(), ".fworld blob file should be written");
        drop(first);

        let second = AnvilChunkSource::with_fworld(
            blob_path,
            registry,
            None,
            Dimension::Overworld,
            ferrite_world::BlobCompression::Lz4,
            [0, 65, 0],
        )
        .unwrap();
        assert!(second.chunk_saved(0, 0));
        let _ = ready(&second, 0, 0, 0).await;
        assert_eq!(second.block(3, 70, 5), Some(STONE));
    }

    #[tokio::test]
    async fn light_pool_batch_relight_matches_serial_and_publishes_atomically() {
        let torch = ferrite_data::blocks::default_state("torch").expect("torch default state");

        let setup = |workers: usize| {
            let src = AnvilChunkSource::new_memory();
            src.enable_lighting_pool(true);
            (src, workers)
        };

        let (serial_src, _) = setup(1);
        let (pool_src, _) = setup(4);

        let coords = [(0, 0), (1, 0), (0, 1), (1, 1)];
        for &(cx, cz) in &coords {
            let _ = ready(&serial_src, cx, cz, 0).await;
            let _ = ready(&pool_src, cx, cz, 0).await;
        }
        // Place a torch near chunk border (15, 70, 15) so light crosses into (1,0), (0,1), (1,1).
        serial_src.set_block(15, 70, 15, u32::from(torch)).unwrap();
        pool_src.set_block(15, 70, 15, u32::from(torch)).unwrap();

        let published_serial = serial_src.relight_batch(&coords, 1).unwrap();
        let published_pool = pool_src.relight_batch(&coords, 4).unwrap();
        assert_eq!(published_serial, 4);
        assert_eq!(published_pool, 4);

        for &(cx, cz) in &coords {
            let snap_s = serial_src.chunk_content(cx, cz).unwrap();
            let snap_p = pool_src.chunk_content(cx, cz).unwrap();
            assert_eq!(
                snap_s.sky_light, snap_p.sky_light,
                "sky light mismatch at ({cx}, {cz})"
            );
            assert_eq!(
                snap_s.block_light, snap_p.block_light,
                "block light mismatch at ({cx}, {cz})"
            );
            // Torch at (15, 70, 15) must illuminate neighbor chunk (16, 70, 15) above 0.
            if (cx, cz) == (1, 0) {
                let b = &snap_p.block_light;
                assert!(
                    b.iter().any(|&v| v > 0),
                    "expected block light spill into neighbour chunk (1, 0)"
                );
            }
        }
    }

    #[test]
    fn rust_generator_handles_all_dimensions_locate_biome_and_place_tree() {
        let vanilla_data =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3/generated");
        if !vanilla_data.join("reports/blocks.json").exists() {
            return;
        }
        let registry = Arc::new(RuntimeRegistry::load(&vanilla_data).unwrap());
        let generator = crate::game::RustGenerator::with_vanilla_data(42, registry, &vanilla_data);

        for dim in [Dimension::Overworld, Dimension::Nether, Dimension::End] {
            let out = generator.generate(dim, 0, 0).unwrap();
            assert_eq!(out.chunk.sections().len() + 2, dim.light_sections());
            let biome = generator.biome_at(dim, 0, 64, 0).unwrap();
            assert!(biome.starts_with("minecraft:"), "unexpected biome {biome}");
        }
        let located = generator
            .locate_biome(Dimension::End, "minecraft:the_end", 0, 0, 128)
            .unwrap();
        assert_eq!(located, Some((0, 64, 0)));

        let placed = generator
            .place_tree(Dimension::Overworld, "minecraft:oak", [8, 65, 8], None)
            .unwrap();
        assert!(!placed.blocks.is_empty());

        let village = generator
            .locate_structure(Dimension::Overworld, "#minecraft:village", 0, 0, 4096)
            .unwrap();
        let (vx, vy, vz) = village.expect("village found within radius");
        assert!(
            generator
                .structure_at(Dimension::Overworld, "#minecraft:village", vx, vy, vz)
                .unwrap()
        );
        let starts = generator
            .structure_starts(Dimension::Overworld, vx.div_euclid(16), vz.div_euclid(16))
            .unwrap();
        assert!(!starts.is_empty());
        assert!(
            generator
                .locate_poi(Dimension::Overworld, "minecraft:meeting", 0, 64, 0, 4096)
                .unwrap()
                .is_some()
        );
        let placed_struct = generator
            .place(Dimension::Overworld, "structure;minecraft:igloo;0;64;0")
            .unwrap();
        assert!(placed_struct.blocks.len() > 9);
    }
}
