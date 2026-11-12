//! Saved maps (vanilla `MapItemSavedData`) and the id allocator (`MapIndex`).
//!
//! # Storage layout (26.3)
//!
//! Vanilla keeps maps in the *server-level* saved-data storage, not in a dimension's: the
//! overworld, nether and end share one id space. Each map is a gzip NBT file
//! `data/minecraft/maps/<id>.dat` shaped `{ data: {...}, DataVersion }`, and the allocator is
//! `data/minecraft/maps/last_id.dat` shaped `{ data: { map: <last id> }, DataVersion }` where a
//! missing counter means `-1`, so the first id handed out is `0`. The field names below were
//! read off the 26.3 server's codecs (`dimension`, `xCenter`, `zCenter`, `scale`, `colors`,
//! `trackingPosition`, `unlimitedTracking`, `locked`, `banners`, `frames`).
//!
//! # World namespaces
//!
//! * The three vanilla dimensions share one store, rooted at the primary world directory.
//! * A runtime world (a Bukkit `createWorld`) also uses that same server-wide store, so a map item
//!   that travels between worlds never resolves to a different map and ids never collide. What
//!   differs per world is the map's own `dimension` field, which records the world the map
//!   was made for (`minecraft:overworld`, ... or the runtime world's resource key). The
//!   store never rewrites it: a clone, zoom-out or lock keeps the source's world.
//! * When no server root is registered (tests, embedded use) a source falls back to its own
//!   directory.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, OnceLock},
};

use ferrite_nbt::{Compound, List, Tag, binary};

/// Pixels per side of a map.
pub const MAP_SIZE: usize = 128;
/// Bytes of a map's colour array.
pub const MAP_COLORS: usize = MAP_SIZE * MAP_SIZE;
/// Largest zoom level (vanilla `MapItemSavedData.MAX_SCALE`).
pub const MAX_SCALE: u8 = 4;

/// Why a map could not be read or written.
#[derive(Debug)]
pub enum MapError {
    /// The filesystem refused.
    Io(std::io::Error),
    /// The file exists but is not a valid map or counter. It is left untouched on disk.
    Malformed(String),
    /// Every `i32` id is taken.
    Exhausted,
}

impl std::fmt::Display for MapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "map storage I/O failed: {error}"),
            Self::Malformed(why) => write!(f, "malformed map data: {why}"),
            Self::Exhausted => f.write_str("no free map id"),
        }
    }
}

impl std::error::Error for MapError {}

impl From<std::io::Error> for MapError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// A banner marker on a map (vanilla `MapBanner`).
#[derive(Clone, Debug, PartialEq)]
pub struct MapBanner {
    pub pos: [i32; 3],
    /// Dye colour name; vanilla defaults a missing value to white.
    pub color: String,
    /// The banner's custom name as a text-component tag.
    pub name: Option<Tag>,
}

/// An item frame holding the map (vanilla `MapFrame`).
#[derive(Clone, Debug, PartialEq)]
pub struct MapFrame {
    pub pos: [i32; 3],
    pub rotation: i32,
    pub entity_id: i32,
}

/// One saved map.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedMap {
    /// The world the map belongs to: a dimension key or a runtime world's resource key.
    pub dimension: String,
    pub x_center: i32,
    pub z_center: i32,
    pub scale: u8,
    pub tracking_position: bool,
    pub unlimited_tracking: bool,
    pub locked: bool,
    /// [`MAP_COLORS`] palette indices, row-major; 0 is transparent.
    pub colors: Vec<u8>,
    pub banners: Vec<MapBanner>,
    pub frames: Vec<MapFrame>,
}

impl SavedMap {
    /// Vanilla `MapItemSavedData.createFresh`: the centre snaps to the scale's grid.
    pub fn fresh(
        x: f64,
        z: f64,
        scale: u8,
        tracking_position: bool,
        unlimited_tracking: bool,
        dimension: &str,
    ) -> Self {
        let scale = scale.min(MAX_SCALE);
        let span = 128 * (1i32 << scale);
        let cell = |value: f64| ((value + 64.0) / f64::from(span)).floor() as i32;
        Self {
            dimension: dimension.to_owned(),
            x_center: cell(x) * span + span / 2 - 64,
            z_center: cell(z) * span + span / 2 - 64,
            scale,
            tracking_position,
            unlimited_tracking,
            locked: false,
            colors: vec![0; MAP_COLORS],
            banners: Vec::new(),
            frames: Vec::new(),
        }
    }

    /// Vanilla `scaled()`: a blank map one zoom level out around the same centre.
    pub fn scaled(&self) -> Self {
        Self::fresh(
            f64::from(self.x_center),
            f64::from(self.z_center),
            (self.scale + 1).min(MAX_SCALE),
            self.tracking_position,
            self.unlimited_tracking,
            &self.dimension,
        )
    }

    /// Vanilla `locked()`: a copy that keeps the colours and banners but no longer updates.
    /// Item frames are not carried over.
    pub fn locked_copy(&self) -> Self {
        Self {
            locked: true,
            frames: Vec::new(),
            ..self.clone()
        }
    }

    /// The `data` compound of the map file.
    pub fn to_compound(&self) -> Compound {
        let mut data = Compound::new();
        data.insert("dimension".into(), Tag::String(self.dimension.clone()));
        data.insert("xCenter".into(), Tag::Int(self.x_center));
        data.insert("zCenter".into(), Tag::Int(self.z_center));
        data.insert("scale".into(), Tag::Byte(self.scale as i8));
        data.insert(
            "colors".into(),
            Tag::ByteArray(self.colors.iter().map(|&c| c as i8).collect()),
        );
        data.insert(
            "trackingPosition".into(),
            Tag::Byte(i8::from(self.tracking_position)),
        );
        data.insert(
            "unlimitedTracking".into(),
            Tag::Byte(i8::from(self.unlimited_tracking)),
        );
        data.insert("locked".into(), Tag::Byte(i8::from(self.locked)));
        let banners = self
            .banners
            .iter()
            .map(|banner| {
                let mut entry = Compound::new();
                entry.insert("pos".into(), Tag::IntArray(banner.pos.to_vec()));
                entry.insert("color".into(), Tag::String(banner.color.clone()));
                if let Some(name) = &banner.name {
                    entry.insert("name".into(), name.clone());
                }
                entry
            })
            .collect::<Vec<_>>();
        data.insert("banners".into(), compound_list(banners));
        let frames = self
            .frames
            .iter()
            .map(|frame| {
                let mut entry = Compound::new();
                entry.insert("pos".into(), Tag::IntArray(frame.pos.to_vec()));
                entry.insert("rotation".into(), Tag::Int(frame.rotation));
                entry.insert("entity_id".into(), Tag::Int(frame.entity_id));
                entry
            })
            .collect::<Vec<_>>();
        data.insert("frames".into(), compound_list(frames));
        data
    }

    /// The whole file root: `{ data, DataVersion }`.
    pub fn to_nbt(&self, data_version: i32) -> Tag {
        let mut root = Compound::new();
        root.insert("data".into(), Tag::Compound(self.to_compound()));
        root.insert("DataVersion".into(), Tag::Int(data_version));
        Tag::Compound(root)
    }

    /// Reads a map file root. Anything that does not match the codec is an error, so a damaged
    /// file is reported instead of being treated as a blank map.
    pub fn from_nbt(root: &Tag) -> Result<Self, MapError> {
        let bad = |why: &str| MapError::Malformed(why.to_owned());
        let Tag::Compound(root) = root else {
            return Err(bad("root is not a compound"));
        };
        let Some(Tag::Compound(data)) = root.get("data") else {
            return Err(bad("missing data compound"));
        };
        let Some(Tag::String(dimension)) = data.get("dimension") else {
            return Err(bad("missing dimension"));
        };
        let int = |name: &str| match data.get(name) {
            Some(Tag::Int(value)) => Ok(*value),
            _ => Err(MapError::Malformed(format!("missing int {name}"))),
        };
        let flag = |name: &str, default: bool| match data.get(name) {
            None => Ok(default),
            Some(Tag::Byte(value)) => Ok(*value != 0),
            Some(_) => Err(MapError::Malformed(format!("{name} is not a byte"))),
        };
        let scale = match data.get("scale") {
            None => 0,
            Some(Tag::Byte(value)) if (0..=MAX_SCALE as i8).contains(value) => *value as u8,
            Some(_) => return Err(bad("scale out of range")),
        };
        let colors = match data.get("colors") {
            Some(Tag::ByteArray(bytes)) if bytes.len() == MAP_COLORS => {
                bytes.iter().map(|&b| b as u8).collect()
            }
            Some(Tag::ByteArray(bytes)) => {
                return Err(MapError::Malformed(format!(
                    "colors has {} bytes, expected {MAP_COLORS}",
                    bytes.len()
                )));
            }
            _ => return Err(bad("missing colors")),
        };
        let pos = |entry: &Compound| match entry.get("pos") {
            Some(Tag::IntArray(values)) if values.len() == 3 => {
                Ok([values[0], values[1], values[2]])
            }
            _ => Err(MapError::Malformed("entry pos is not 3 ints".into())),
        };
        let mut banners = Vec::new();
        match data.get("banners") {
            None | Some(Tag::List(List::End)) => {}
            Some(Tag::List(List::Compound(entries))) => {
                for entry in entries {
                    let color = match entry.get("color") {
                        Some(Tag::String(color)) => color.clone(),
                        None => "white".to_owned(),
                        Some(_) => return Err(bad("banner color is not a string")),
                    };
                    banners.push(MapBanner {
                        pos: pos(entry)?,
                        color,
                        name: entry.get("name").cloned(),
                    });
                }
            }
            Some(_) => return Err(bad("banners is not a compound list")),
        }
        let mut frames = Vec::new();
        match data.get("frames") {
            None | Some(Tag::List(List::End)) => {}
            Some(Tag::List(List::Compound(entries))) => {
                for entry in entries {
                    let (Some(Tag::Int(rotation)), Some(Tag::Int(entity_id))) =
                        (entry.get("rotation"), entry.get("entity_id"))
                    else {
                        return Err(bad("frame without rotation and entity_id"));
                    };
                    frames.push(MapFrame {
                        pos: pos(entry)?,
                        rotation: *rotation,
                        entity_id: *entity_id,
                    });
                }
            }
            Some(_) => return Err(bad("frames is not a compound list")),
        }
        Ok(Self {
            dimension: dimension.clone(),
            x_center: int("xCenter")?,
            z_center: int("zCenter")?,
            scale,
            tracking_position: flag("trackingPosition", true)?,
            unlimited_tracking: flag("unlimitedTracking", false)?,
            locked: flag("locked", false)?,
            colors,
            banners,
            frames,
        })
    }
}

fn compound_list(entries: Vec<Compound>) -> Tag {
    Tag::List(if entries.is_empty() {
        List::End
    } else {
        List::Compound(entries)
    })
}

struct Inner {
    last_id: i32,
    cache: HashMap<i32, Arc<SavedMap>>,
}

/// The map files of one server and their id allocator. Cheap to share: every method takes
/// `&self` and holds one short mutex, never a world or region lock.
pub struct MapStore {
    dir: PathBuf,
    data_version: i32,
    inner: Mutex<Inner>,
}

const COUNTER: &str = "last_id.dat";

impl MapStore {
    /// Opens (creating on first write) the store under `world_root/data/minecraft/maps`.
    ///
    /// The counter is the larger of `last_id.dat` and the highest `<id>.dat` on disk, so a
    /// lost or damaged counter can never hand out an id whose file exists.
    pub fn open(world_root: &Path, data_version: i32) -> Result<Self, MapError> {
        let dir = world_root.join("data/minecraft/maps");
        let mut last = -1i32;
        match std::fs::read(dir.join(COUNTER)) {
            Ok(bytes) => match read_counter(&bytes) {
                Ok(value) => last = value,
                Err(error) => {
                    tracing::warn!("ignoring unreadable map counter, using the map files: {error}");
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        match std::fs::read_dir(&dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    if let Some(id) = name
                        .to_str()
                        .and_then(|name| name.strip_suffix(".dat"))
                        .and_then(|stem| stem.parse::<i32>().ok())
                    {
                        last = last.max(id);
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(Self {
            dir,
            data_version,
            inner: Mutex::new(Inner {
                last_id: last,
                cache: HashMap::new(),
            }),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn path(&self, id: i32) -> PathBuf {
        self.dir.join(format!("{id}.dat"))
    }

    /// The highest id handed out so far, or -1.
    pub fn last_id(&self) -> i32 {
        self.lock().last_id
    }

    /// Stores `map` under a new id and returns it. The map file is created exclusively, so an
    /// existing file is skipped instead of overwritten.
    pub fn allocate(&self, map: SavedMap) -> Result<i32, MapError> {
        let mut inner = self.lock();
        std::fs::create_dir_all(&self.dir)?;
        let bytes = encode(&map, self.data_version)?;
        let mut id = inner.last_id;
        loop {
            id = id.checked_add(1).ok_or(MapError::Exhausted)?;
            match write_new(&self.path(id), &bytes) {
                Ok(()) => break,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        inner.last_id = id;
        // The map exists even if this fails; the next open recovers the counter from the files.
        write_atomic(
            &self.dir.join(COUNTER),
            &counter_bytes(id, self.data_version)?,
        )?;
        inner.cache.insert(id, Arc::new(map));
        Ok(id)
    }

    /// The map with `id`: `Ok(None)` when no such map exists, an error when its file is damaged.
    pub fn get(&self, id: i32) -> Result<Option<Arc<SavedMap>>, MapError> {
        let mut inner = self.lock();
        if let Some(map) = inner.cache.get(&id) {
            return Ok(Some(map.clone()));
        }
        let bytes = match std::fs::read(self.path(id)) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let (_, tag) = binary::read_compressed(&bytes, binary::Compression::Gzip)
            .map_err(|error| MapError::Malformed(error.to_string()))?;
        let map = Arc::new(SavedMap::from_nbt(&tag)?);
        inner.cache.insert(id, map.clone());
        Ok(Some(map))
    }

    /// Replaces the saved data of an existing map.
    pub fn save(&self, id: i32, map: SavedMap) -> Result<(), MapError> {
        let mut inner = self.lock();
        if id > inner.last_id || id < 0 {
            return Err(MapError::Malformed(format!("map {id} was never allocated")));
        }
        std::fs::create_dir_all(&self.dir)?;
        write_atomic(&self.path(id), &encode(&map, self.data_version)?)?;
        inner.cache.insert(id, Arc::new(map));
        Ok(())
    }
}

fn encode(map: &SavedMap, data_version: i32) -> Result<Vec<u8>, MapError> {
    binary::write_compressed("", &map.to_nbt(data_version), binary::Compression::Gzip)
        .map_err(|error| MapError::Malformed(error.to_string()))
}

fn counter_bytes(last: i32, data_version: i32) -> Result<Vec<u8>, MapError> {
    let mut data = Compound::new();
    data.insert("map".into(), Tag::Int(last));
    let mut root = Compound::new();
    root.insert("data".into(), Tag::Compound(data));
    root.insert("DataVersion".into(), Tag::Int(data_version));
    binary::write_compressed("", &Tag::Compound(root), binary::Compression::Gzip)
        .map_err(|error| MapError::Malformed(error.to_string()))
}

fn read_counter(bytes: &[u8]) -> Result<i32, MapError> {
    let (_, tag) = binary::read_compressed(bytes, binary::Compression::Gzip)
        .map_err(|error| MapError::Malformed(error.to_string()))?;
    let Tag::Compound(root) = tag else {
        return Err(MapError::Malformed("counter root".into()));
    };
    match root.get("data") {
        Some(Tag::Compound(data)) => match data.get("map") {
            Some(Tag::Int(value)) if *value >= -1 => Ok(*value),
            None => Ok(-1),
            _ => Err(MapError::Malformed("counter map is not an int".into())),
        },
        _ => Err(MapError::Malformed("counter without data".into())),
    }
}

fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temp = path.with_extension(format!("dat.{}.{unique}.tmp", std::process::id()));
    std::fs::write(&temp, bytes)?;
    std::fs::rename(&temp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

/// Where the server's maps live when a source is not the primary world: set once at startup.
fn server_root() -> &'static Mutex<Option<PathBuf>> {
    static ROOT: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    ROOT.get_or_init(|| Mutex::new(None))
}

/// Registers the primary world directory every world's maps are stored under.
pub fn set_server_root(root: Option<PathBuf>) {
    *server_root()
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = root;
}

/// The shared store for the world rooted at `own_root`, opened once per directory. A registered
/// server root takes precedence, which is what puts runtime worlds in the primary store.
pub fn store_for(own_root: &Path, data_version: i32) -> Result<Arc<MapStore>, MapError> {
    static STORES: OnceLock<Mutex<HashMap<PathBuf, Arc<MapStore>>>> = OnceLock::new();
    let root = server_root()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
        .unwrap_or_else(|| own_root.to_path_buf());
    let mut stores = STORES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(store) = stores.get(&root) {
        return Ok(store.clone());
    }
    let store = Arc::new(MapStore::open(&root, data_version)?);
    stores.insert(root, store.clone());
    Ok(store)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SavedMap {
        let mut map = SavedMap::fresh(1000.0, -300.0, 2, true, false, "minecraft:the_nether");
        map.colors[0] = 4;
        map.colors[MAP_COLORS - 1] = 0x7f;
        map.banners.push(MapBanner {
            pos: [1, 64, -2],
            color: "red".into(),
            name: Some(Tag::String("{\"text\":\"Base\"}".into())),
        });
        map.frames.push(MapFrame {
            pos: [3, 70, 4],
            rotation: 2,
            entity_id: 17,
        });
        map
    }

    #[test]
    fn fresh_maps_snap_their_centre_to_the_scale_grid() {
        // Vanilla createFresh: scale 0 spans 128, so x=1000 lands in cell floor(1064/128)=8,
        // centred at 8*128+64-64=1024, and z=-300 in cell floor(-236/128)=-2, centred at -256.
        let map = SavedMap::fresh(1000.0, -300.0, 0, true, false, "minecraft:overworld");
        assert_eq!((map.x_center, map.z_center), (1024, -256));
        let big = SavedMap::fresh(0.0, 0.0, 4, true, false, "minecraft:overworld");
        assert_eq!((big.x_center, big.z_center), (1024 - 64, 1024 - 64));
        assert_eq!(big.colors.len(), MAP_COLORS);
    }

    #[test]
    fn scaled_is_blank_and_one_level_out_and_caps_at_four() {
        let map = sample();
        let out = map.scaled();
        assert_eq!(out.scale, 3);
        assert!(out.colors.iter().all(|&c| c == 0));
        assert!(out.banners.is_empty() && !out.locked);
        assert_eq!(out.dimension, "minecraft:the_nether");
        let mut max = sample();
        max.scale = MAX_SCALE;
        assert_eq!(max.scaled().scale, MAX_SCALE);
    }

    #[test]
    fn locked_copy_keeps_pixels_and_banners_but_not_frames() {
        let locked = sample().locked_copy();
        assert!(locked.locked);
        assert_eq!(locked.colors[0], 4);
        assert_eq!(locked.banners.len(), 1);
        assert!(locked.frames.is_empty());
    }

    #[test]
    fn saved_map_nbt_round_trips_through_gzip() {
        let map = sample();
        let bytes =
            binary::write_compressed("", &map.to_nbt(4000), binary::Compression::Gzip).unwrap();
        let (_, tag) = binary::read_compressed(&bytes, binary::Compression::Gzip).unwrap();
        assert_eq!(SavedMap::from_nbt(&tag).unwrap(), map);
        let Tag::Compound(root) = tag else { panic!() };
        assert_eq!(root.get("DataVersion"), Some(&Tag::Int(4000)));
    }

    #[test]
    fn nbt_uses_the_vanilla_field_names_and_types() {
        let data = sample().to_compound();
        assert!(matches!(data.get("dimension"), Some(Tag::String(_))));
        assert!(matches!(data.get("xCenter"), Some(Tag::Int(_))));
        assert!(matches!(data.get("scale"), Some(Tag::Byte(2))));
        assert!(matches!(data.get("locked"), Some(Tag::Byte(0))));
        assert!(matches!(data.get("trackingPosition"), Some(Tag::Byte(1))));
        assert!(matches!(data.get("colors"), Some(Tag::ByteArray(c)) if c.len() == MAP_COLORS));
        assert!(matches!(
            data.get("banners"),
            Some(Tag::List(List::Compound(_)))
        ));
        assert!(matches!(
            data.get("frames"),
            Some(Tag::List(List::Compound(_)))
        ));
    }

    #[test]
    fn missing_optional_fields_take_the_vanilla_defaults() {
        let mut data = sample().to_compound();
        for key in [
            "scale",
            "trackingPosition",
            "unlimitedTracking",
            "locked",
            "banners",
            "frames",
        ] {
            data.shift_remove(key);
        }
        let mut root = Compound::new();
        root.insert("data".into(), Tag::Compound(data));
        let map = SavedMap::from_nbt(&Tag::Compound(root)).unwrap();
        assert_eq!(map.scale, 0);
        assert!(map.tracking_position && !map.unlimited_tracking && !map.locked);
        assert!(map.banners.is_empty() && map.frames.is_empty());
    }

    #[test]
    fn malformed_maps_are_rejected_not_defaulted() {
        let good = sample().to_compound();
        let wrap = |data: Compound| {
            let mut root = Compound::new();
            root.insert("data".into(), Tag::Compound(data));
            Tag::Compound(root)
        };
        assert!(SavedMap::from_nbt(&Tag::Int(1)).is_err());
        assert!(SavedMap::from_nbt(&Tag::Compound(Compound::new())).is_err());
        let mut short = good.clone();
        short.insert("colors".into(), Tag::ByteArray(vec![0; 10]));
        assert!(SavedMap::from_nbt(&wrap(short)).is_err());
        let mut scale = good.clone();
        scale.insert("scale".into(), Tag::Byte(9));
        assert!(SavedMap::from_nbt(&wrap(scale)).is_err());
        let mut no_dimension = good.clone();
        no_dimension.shift_remove("dimension");
        assert!(SavedMap::from_nbt(&wrap(no_dimension)).is_err());
        let mut wrong_type = good;
        wrong_type.insert("xCenter".into(), Tag::String("1".into()));
        assert!(SavedMap::from_nbt(&wrap(wrong_type)).is_err());
    }

    #[test]
    fn ids_start_at_zero_and_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let last = {
            let store = MapStore::open(dir.path(), 4000).unwrap();
            assert_eq!(store.last_id(), -1);
            let a = store.allocate(sample()).unwrap();
            let b = store.allocate(sample().scaled()).unwrap();
            assert_eq!((a, b), (0, 1));
            b
        };
        let store = MapStore::open(dir.path(), 4000).unwrap();
        assert_eq!(store.last_id(), last);
        assert_eq!(store.allocate(sample()).unwrap(), 2);
        assert_eq!(*store.get(0).unwrap().unwrap(), sample());
        assert_eq!(store.get(1).unwrap().unwrap().scale, 3);
        assert!(store.get(99).unwrap().is_none());
        assert!(dir.path().join("data/minecraft/maps/last_id.dat").is_file());
        assert!(dir.path().join("data/minecraft/maps/2.dat").is_file());
    }

    #[test]
    fn counter_file_is_vanilla_shaped() {
        let dir = tempfile::tempdir().unwrap();
        let store = MapStore::open(dir.path(), 4000).unwrap();
        store.allocate(sample()).unwrap();
        let bytes = std::fs::read(dir.path().join("data/minecraft/maps/last_id.dat")).unwrap();
        assert_eq!(read_counter(&bytes).unwrap(), 0);
    }

    #[test]
    fn a_lost_or_damaged_counter_never_reuses_an_existing_map() {
        let dir = tempfile::tempdir().unwrap();
        {
            let store = MapStore::open(dir.path(), 4000).unwrap();
            for _ in 0..3 {
                store.allocate(sample()).unwrap();
            }
        }
        let counter = dir.path().join("data/minecraft/maps/last_id.dat");
        std::fs::remove_file(&counter).unwrap();
        let store = MapStore::open(dir.path(), 4000).unwrap();
        assert_eq!(store.last_id(), 2);
        assert_eq!(store.allocate(sample()).unwrap(), 3);
        std::fs::write(&counter, b"not gzip").unwrap();
        let store = MapStore::open(dir.path(), 4000).unwrap();
        assert_eq!(store.allocate(sample()).unwrap(), 4);
    }

    #[test]
    fn allocation_skips_an_id_whose_file_appeared_behind_the_counter() {
        let dir = tempfile::tempdir().unwrap();
        let store = MapStore::open(dir.path(), 4000).unwrap();
        store.allocate(sample()).unwrap();
        let squatter = dir.path().join("data/minecraft/maps/1.dat");
        std::fs::write(&squatter, b"someone else's map").unwrap();
        assert_eq!(store.allocate(sample()).unwrap(), 2);
        assert_eq!(std::fs::read(&squatter).unwrap(), b"someone else's map");
    }

    #[test]
    fn a_damaged_map_file_is_an_error_and_stays_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let store = MapStore::open(dir.path(), 4000).unwrap();
        let id = store.allocate(sample()).unwrap();
        let path = dir.path().join(format!("data/minecraft/maps/{id}.dat"));
        std::fs::write(&path, b"garbage").unwrap();
        let fresh = MapStore::open(dir.path(), 4000).unwrap();
        assert!(matches!(fresh.get(id), Err(MapError::Malformed(_))));
        assert_eq!(std::fs::read(&path).unwrap(), b"garbage");
        assert_eq!(fresh.allocate(sample()).unwrap(), id + 1);
    }

    #[test]
    fn save_rewrites_only_allocated_maps() {
        let dir = tempfile::tempdir().unwrap();
        let store = MapStore::open(dir.path(), 4000).unwrap();
        let id = store.allocate(sample()).unwrap();
        let mut changed = sample();
        changed.colors[5] = 9;
        store.save(id, changed.clone()).unwrap();
        let reopened = MapStore::open(dir.path(), 4000).unwrap();
        assert_eq!(*reopened.get(id).unwrap().unwrap(), changed);
        assert!(store.save(id + 5, changed).is_err());
    }

    #[test]
    fn concurrent_allocation_hands_out_distinct_ids() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(MapStore::open(dir.path(), 4000).unwrap());
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let store = store.clone();
                std::thread::spawn(move || {
                    (0..10)
                        .map(|_| store.allocate(sample()).unwrap())
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        let mut ids: Vec<i32> = handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap())
            .collect();
        ids.sort_unstable();
        assert_eq!(ids, (0..80).collect::<Vec<_>>());
        let reopened = MapStore::open(dir.path(), 4000).unwrap();
        assert_eq!(reopened.last_id(), 79);
    }

    #[test]
    fn two_stores_on_one_directory_do_not_collide() {
        let dir = tempfile::tempdir().unwrap();
        let a = Arc::new(MapStore::open(dir.path(), 4000).unwrap());
        let b = Arc::new(MapStore::open(dir.path(), 4000).unwrap());
        let handles: Vec<_> = [a, b]
            .into_iter()
            .map(|store| {
                std::thread::spawn(move || {
                    (0..20)
                        .map(|_| store.allocate(sample()).unwrap())
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        let mut ids: Vec<i32> = handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 40, "no id may be handed out twice");
    }

    #[test]
    fn without_a_server_root_each_directory_has_its_own_shared_store() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let first = store_for(a.path(), 4000).unwrap();
        assert!(Arc::ptr_eq(&first, &store_for(a.path(), 4000).unwrap()));
        assert!(!Arc::ptr_eq(&first, &store_for(b.path(), 4000).unwrap()));
    }

    #[test]
    fn a_runtime_world_keeps_its_own_dimension_identity() {
        let dir = tempfile::tempdir().unwrap();
        let store = MapStore::open(dir.path(), 4000).unwrap();
        let overworld = SavedMap::fresh(0.0, 0.0, 0, true, false, "minecraft:overworld");
        let runtime = SavedMap::fresh(0.0, 0.0, 0, true, false, "ferrite:arena");
        let a = store.allocate(overworld).unwrap();
        let b = store.allocate(runtime).unwrap();
        assert_ne!(a, b);
        assert_eq!(
            store.get(a).unwrap().unwrap().dimension,
            "minecraft:overworld"
        );
        let copy = store.get(b).unwrap().unwrap().locked_copy();
        assert_eq!(copy.dimension, "ferrite:arena");
        assert_eq!(copy.scaled().dimension, "ferrite:arena");
    }
}
