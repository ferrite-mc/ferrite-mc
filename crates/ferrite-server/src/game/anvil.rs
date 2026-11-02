use super::{Dimension, terrain::VanillaTerrain};
use ferrite_command::gamerule::GameRules;
use ferrite_nbt::{List, Tag, binary};
use ferrite_world::{
    BlockStateRegistry, Chunk, LightData, LightSection, RegionFile, RuntimeRegistry,
    chunk_from_nbt, encode_chunk_packet,
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
    let chunk = chunk_from_nbt(&tag, dimension.min_y(), dimension.height() as u32, registry)?;
    let sections = dimension.light_sections();
    let min_section = dimension.min_y().div_euclid(16);
    let mut light = LightData {
        sky: vec![LightSection::Unknown; sections],
        block: vec![LightSection::Unknown; sections],
    };
    if let Tag::Compound(root) = &tag
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
    Ok(Some((chunk, light)))
}

use ferrite_protocol::{Encode, Packet, VarInt, packets::play::ChunkData};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

const CACHE_LIMIT: usize = 4096;
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
    Ready(Result<Box<Column>, String>, u64),
}
struct Column {
    chunk: Chunk,
    light: LightData,
    /// Complete `level_chunk_with_light` body (packet id + payload). Shared by
    /// every player that receives this chunk, exactly like Paper's packet
    /// cache: encoded once, copied only when it is handed to a connection.
    packet: Arc<[u8]>,
}
struct Cache {
    entries: HashMap<(i32, i32, i32), Entry>,
    clock: u64,
    dirty: HashMap<(i32, i32), Dirty>,
}
struct Dirty {
    chunk: ferrite_world::Chunk,
    light: LightData,
    packet: Arc<[u8]>,
    version: u64,
}

/// What a cache lookup found.
enum Lookup {
    Ready(Arc<[u8]>),
    Failed(String),
    Loading,
}

pub struct AnvilChunkSource {
    path: Option<PathBuf>,
    registry: Arc<RuntimeRegistry>,
    cache: Mutex<Cache>,
    stats: Stats,
    runtime: tokio::runtime::Handle,
    flush_lock: tokio::sync::Mutex<()>,
    flat: Mutex<HashMap<i32, (Chunk, LightData)>>,
    /// Vanilla generation for chunks missing from disk; `None` is the flat test world.
    terrain: Option<Arc<VanillaTerrain>>,
    /// Which dimension this source reads, writes and generates.
    dimension: Dimension,
}
impl AnvilChunkSource {
    pub fn new(path: PathBuf, registry: RuntimeRegistry) -> Arc<Self> {
        Self::new_with_terrain(path, registry, None)
    }
    pub fn new_with_terrain(
        path: PathBuf,
        registry: RuntimeRegistry,
        terrain: Option<Arc<VanillaTerrain>>,
    ) -> Arc<Self> {
        Self::with_dimension(path, Arc::new(registry), terrain, Dimension::Overworld)
    }
    /// A disk-backed source for one dimension. `path` is the world root; the dimension's
    /// vanilla storage tree (`DIM-1`, `DIM1`, ...) is appended.
    pub fn with_dimension(
        path: PathBuf,
        registry: Arc<RuntimeRegistry>,
        terrain: Option<Arc<VanillaTerrain>>,
        dimension: Dimension,
    ) -> Arc<Self> {
        Arc::new(Self {
            path: Some(path),
            registry,
            cache: Mutex::new(Cache {
                entries: HashMap::new(),
                clock: 0,
                dirty: HashMap::new(),
            }),
            stats: Stats::default(),
            runtime: tokio::runtime::Handle::current(),
            flush_lock: tokio::sync::Mutex::new(()),
            flat: Mutex::new(HashMap::new()),
            terrain,
            dimension,
        })
    }
    pub fn new_memory() -> Arc<Self> {
        Self::new_memory_with_terrain(None)
    }
    pub fn new_memory_with_terrain(terrain: Option<Arc<VanillaTerrain>>) -> Arc<Self> {
        Self::new_memory_dimension(terrain, Dimension::Overworld)
    }
    /// An in-memory source for one dimension, used by tests and by worlds without a
    /// directory.
    pub fn new_memory_dimension(
        terrain: Option<Arc<VanillaTerrain>>,
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
            cache: Mutex::new(Cache {
                entries: HashMap::new(),
                clock: 0,
                dirty: HashMap::new(),
            }),
            stats: Stats::default(),
            runtime: tokio::runtime::Handle::current(),
            flush_lock: tokio::sync::Mutex::new(()),
            flat: Mutex::new(HashMap::new()),
            terrain,
            dimension,
        })
    }
    pub fn dimension(&self) -> Dimension {
        self.dimension
    }
    pub(super) fn registry(&self) -> Arc<RuntimeRegistry> {
        Arc::clone(&self.registry)
    }
    pub(super) fn terrain(&self) -> Option<Arc<VanillaTerrain>> {
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
            .spawn_height_dimension(self.dimension, x, z)
            .ok()
    }

    /// The nearest position with the given surface biome, when vanilla generation is enabled.
    /// A flat test world has no biome source and reports no match.
    pub fn locate_biome(
        &self,
        target: &str,
        x: i32,
        z: i32,
    ) -> Option<anyhow::Result<Option<(i32, i32)>>> {
        let terrain = self.terrain.as_ref()?;
        Some(terrain.locate_biome(target, x, z, 6_400))
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
            .lock()
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
        let cache = self.cache.lock().unwrap();
        if let Some(dirty) = cache.dirty.get(&(x, z)) {
            return Some(f(&dirty.chunk));
        }
        let column = cache.entries.iter().find_map(|(&(cx, cz, _), entry)| {
            if (cx, cz) == (x, z)
                && let Entry::Ready(Ok(column), _) = entry
            {
                return Some(column);
            }
            None
        })?;
        Some(f(&column.chunk))
    }
    pub(super) fn brightness(&self, x: i32, y: i32, z: i32) -> Option<u8> {
        let cache = self.cache.lock().unwrap();
        let key = (x.div_euclid(16), z.div_euclid(16));
        let light = if let Some(d) = cache.dirty.get(&key) {
            &d.light
        } else {
            &cache
                .entries
                .iter()
                .find_map(|(&(cx, cz, _), entry)| {
                    if (cx, cz) == key
                        && let Entry::Ready(Ok(column), _) = entry
                    {
                        return Some(column);
                    }
                    None
                })?
                .light
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
        let cache = self.cache.lock().unwrap();
        if let Some(d) = cache.dirty.get(&key) {
            return d
                .chunk
                .block(x.rem_euclid(16) as u8, y, z.rem_euclid(16) as u8);
        }
        let column = cache.entries.iter().find_map(|(&(cx, cz, _), entry)| {
            if (cx, cz) == key {
                if let Entry::Ready(Ok(c), _) = entry {
                    Some(c)
                } else {
                    None
                }
            } else {
                None
            }
        })?;
        column
            .chunk
            .block(x.rem_euclid(16) as u8, y, z.rem_euclid(16) as u8)
    }
    /// Stored sky light at a block. A chunk without light data (the flat test world) falls back
    /// to an open-sky scan, so daylight still matches what a client sees.
    pub fn sky_light(&self, x: i32, y: i32, z: i32) -> Option<u8> {
        let stored = {
            let cache = self.cache.lock().unwrap();
            let key = (x.div_euclid(16), z.div_euclid(16));
            let light = if let Some(dirty) = cache.dirty.get(&key) {
                &dirty.light
            } else {
                &cache
                    .entries
                    .iter()
                    .find_map(|(&(cx, cz, _), entry)| {
                        if (cx, cz) == key
                            && let Entry::Ready(Ok(column), _) = entry
                        {
                            return Some(column);
                        }
                        None
                    })?
                    .light
            };
            let section = usize::try_from(y.div_euclid(16) + 5).ok()?;
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
                let section = usize::try_from(y.div_euclid(16) + 5).ok()?;
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
        let cache = self.cache.lock().unwrap();
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
        let mut cache = self.cache.lock().unwrap();
        let (mut chunk, light) = if let Some(d) = cache.dirty.get(&key) {
            (d.chunk.clone(), d.light.clone())
        } else {
            let column = cache
                .entries
                .iter()
                .find_map(|(&(cx, cz, _), entry)| match entry {
                    Entry::Ready(Ok(c), _) if (cx, cz) == key => Some(c),
                    _ => None,
                })
                .ok_or_else(|| anyhow::anyhow!("chunk not loaded"))?;
            (column.chunk.clone(), column.light.clone())
        };
        let mut entity = chunk.block_entities.remove(&local);
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
        if let Some(entity) = entity {
            chunk.block_entities.insert(local, entity);
        }
        self.stats.encodes.fetch_add(1, Ordering::Relaxed);
        let packet = encode_column_packet(key.0, key.1, &chunk, &light)?;
        cache.clock += 1;
        let version = cache.clock;
        invalidate(&mut cache, &self.stats, key.0, key.1);
        cache.dirty.insert(
            key,
            Dirty {
                chunk,
                light,
                packet,
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
        let mut cache = self.cache.lock().unwrap();
        let (mut chunk, light) = if let Some(d) = cache.dirty.get(&key) {
            (d.chunk.clone(), d.light.clone())
        } else {
            let column = cache
                .entries
                .iter()
                .find_map(|(&(cx, cz, _), entry)| {
                    if (cx, cz) == key {
                        if let Entry::Ready(Ok(c), _) = entry {
                            Some(c)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .ok_or_else(|| anyhow::anyhow!("chunk not loaded"))?;
            (column.chunk.clone(), column.light.clone())
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
        chunk.set_block(
            x.rem_euclid(16) as u8,
            y,
            z.rem_euclid(16) as u8,
            state,
            &*self.registry,
        )?;
        self.stats.encodes.fetch_add(1, Ordering::Relaxed);
        let packet = encode_column_packet(key.0, key.1, &chunk, &light)?;
        cache.clock += 1;
        let version = cache.clock;
        invalidate(&mut cache, &self.stats, key.0, key.1);
        cache.dirty.insert(
            key,
            Dirty {
                chunk,
                light,
                packet,
                version,
            },
        );
        Ok(())
    }
    /// Records a modified column for streaming and eventual Anvil writeback.
    /// TODO: call this when block-editing gameplay is implemented.
    pub fn mark_modified(
        &self,
        x: i32,
        z: i32,
        chunk: ferrite_world::Chunk,
        light: &LightData,
    ) -> anyhow::Result<()> {
        self.stats.encodes.fetch_add(1, Ordering::Relaxed);
        let packet = encode_column_packet(x, z, &chunk, light)?;
        let mut cache = self.cache.lock().unwrap();
        anyhow::ensure!(
            self.path.is_none()
                || cache.dirty.contains_key(&(x, z))
                || cache.dirty.len() < CACHE_LIMIT,
            "dirty chunk cache is full"
        );
        cache.clock += 1;
        let version = cache.clock;
        invalidate(&mut cache, &self.stats, x, z);
        cache.dirty.insert(
            (x, z),
            Dirty {
                chunk,
                light: light.clone(),
                packet,
                version,
            },
        );
        Ok(())
    }
    pub async fn flush_chunk(self: &Arc<Self>, x: i32, z: i32) -> anyhow::Result<()> {
        if self.path.is_none() {
            return Ok(());
        }
        let _guard = self.flush_lock.lock().await;
        let item = {
            let cache = self.cache.lock().unwrap();
            cache
                .dirty
                .get(&(x, z))
                .map(|d| (d.chunk.clone(), d.light.clone(), d.version))
        };
        let Some((chunk, light, version)) = item else {
            return Ok(());
        };
        let source = Arc::clone(self);
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let dir = source
                .path
                .as_ref()
                .unwrap()
                .join(source.dimension.region_dir());
            std::fs::create_dir_all(&dir)?;
            let rx = x.div_euclid(32);
            let rz = z.div_euclid(32);
            let region = RegionFile::open(dir.join(format!("r.{rx}.{rz}.mca")), rx, rz)?;
            let mut tag = ferrite_world::chunk_to_nbt(&chunk, &*source.registry)?;
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
            apply_light(&mut tag, &light, source.dimension)?;
            let bytes = binary::write_named("", &tag)?;
            region.write_chunk(x, z, &bytes, ferrite_world::RegionCompression::Zlib)?;
            Ok(())
        })
        .await??;
        let mut cache = self.cache.lock().unwrap();
        if cache
            .dirty
            .get(&(x, z))
            .is_some_and(|d| d.version == version)
        {
            cache.dirty.remove(&(x, z));
        }
        Ok(())
    }
    pub async fn flush_all(self: &Arc<Self>) -> anyhow::Result<()> {
        if self.path.is_none() {
            return Ok(());
        }
        loop {
            let next = { self.cache.lock().unwrap().dirty.keys().next().copied() };
            let Some((x, z)) = next else { return Ok(()) };
            self.flush_chunk(x, z).await?;
        }
    }
    /// Encoded `level_chunk_with_light` body for `(x, z)`, starting a
    /// background load on first use. `None` means the chunk is still loading.
    pub fn get(self: &Arc<Self>, x: i32, z: i32, biome: i32) -> Option<Result<Arc<[u8]>, String>> {
        match self.lookup(x, z, biome) {
            Lookup::Ready(packet) => Some(Ok(packet)),
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
        let mut cache = self.cache.lock().unwrap();
        cache.clock += 1;
        let now = cache.clock;
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
        if let Some(dirty) = cache.dirty.get(&(x, z)) {
            self.stats.hits.fetch_add(1, Ordering::Relaxed);
            return Lookup::Ready(Arc::clone(&dirty.packet));
        }
        if let Some(entry) = cache.entries.get_mut(&key) {
            return match entry {
                Entry::Pending(_) => Lookup::Loading,
                Entry::Ready(data, used) => {
                    *used = now;
                    self.stats.hits.fetch_add(1, Ordering::Relaxed);
                    match data {
                        Ok(column) => Lookup::Ready(Arc::clone(&column.packet)),
                        Err(error) => Lookup::Failed(error.clone()),
                    }
                }
            };
        }
        if cache
            .entries
            .values()
            .filter(|e| matches!(e, Entry::Pending(_)))
            .count()
            >= PENDING_LIMIT
        {
            return Lookup::Loading;
        }
        if cache.entries.len() >= CACHE_LIMIT {
            let Some(oldest) = cache
                .entries
                .iter()
                .filter_map(|(k, v)| {
                    if let Entry::Ready(_, used) = v {
                        Some((*k, *used))
                    } else {
                        None
                    }
                })
                .min_by_key(|(_, used)| *used)
                .map(|(k, _)| k)
            else {
                return Lookup::Loading;
            };
            cache.entries.remove(&oldest);
        }
        cache.entries.insert(key, Entry::Pending(now));
        drop(cache);
        let source = Arc::clone(self);
        let request = now;
        self.runtime.spawn_blocking(move || {
            let result = (|| -> anyhow::Result<Column> {
                let loaded = match &source.path {
                    Some(path) => {
                        read_column(path, &source.registry, source.dimension, x, z)?
                    }
                    None => None,
                };
                let (chunk, light) = match loaded {
                    Some(column) => column,
                    None => {
                        if let Some(terrain) = &source.terrain {
                            let generated = terrain.generate_dimension(source.dimension, x, z)?;
                            (generated.chunk, generated.light)
                        } else {
                            let mut flat = source.flat.lock().unwrap();
                            if let Some(column) = flat.get(&biome) {
                                column.clone()
                            } else {
                                let column = (
                                    flat_chunk(&source.registry, biome, source.dimension)?,                                            LightData {
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
                let packet = encode_column_packet(x, z, &chunk, &light)?;
                Ok(Column {
                    chunk,
                    light,
                    packet,
                })
            })()
            .map_err(|e| format!("{e:#}"));
            let mut cache = source.cache.lock().unwrap();
            cache.clock += 1;
            let now = cache.clock;
            if matches!(cache.entries.get(&key), Some(Entry::Pending(version)) if *version == request)
            {
                cache.entries.insert(key, Entry::Ready(result.map(Box::new), now));
            }
        });
        Lookup::Loading
    }
}
/// Encode the complete `level_chunk_with_light` body (packet id + payload) once
/// so every recipient can share the same bytes.
fn encode_column_packet(
    x: i32,
    z: i32,
    chunk: &Chunk,
    light: &LightData,
) -> anyhow::Result<Arc<[u8]>> {
    let body = encode_chunk_packet(x, z, chunk, light)?;
    let mut packet = Vec::new();
    VarInt(ChunkData::ID).encode(&mut packet);
    packet.extend(body);
    Ok(Arc::from(packet))
}
/// Drop every cached packet for chunk `(x, z)` after a block or light change.
fn invalidate(cache: &mut Cache, stats: &Stats, x: i32, z: i32) {
    let before = cache.entries.len();
    cache.entries.retain(|&(cx, cz, _), _| (cx, cz) != (x, z));
    if cache.entries.len() < before {
        stats.invalidations.fetch_add(1, Ordering::Relaxed);
        tracing::debug!(x, z, "chunk packet cache invalidated");
    }
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

fn flat_chunk(
    registry: &RuntimeRegistry,
    biome: i32,
    dimension: Dimension,
) -> anyhow::Result<Chunk> {
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
        biome as u32,
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
}
