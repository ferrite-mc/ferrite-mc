use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
    time::{SystemTime, UNIX_EPOCH},
};
const SECTOR: usize = 4096;
const LIMIT: usize = 64 * 1024 * 1024;
static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>> = OnceLock::new();
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum RegionCompression {
    Gzip = 1,
    Zlib = 2,
    None = 3,
    Lz4 = 4,
}
#[derive(Debug, thiserror::Error)]
pub enum RegionError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("invalid region file: {0}")]
    Invalid(&'static str),
    #[error("unsupported region compression: {0}")]
    Compression(u8),
    #[error("chunk outside region")]
    Coordinates,
}
#[derive(Debug, Clone)]
pub struct RegionFile {
    path: PathBuf,
    rx: i32,
    rz: i32,
    lock: Arc<Mutex<()>>,
}
impl RegionFile {
    pub fn open(path: impl AsRef<Path>, rx: i32, rz: i32) -> Result<Self, RegionError> {
        let path = path.as_ref();
        let parent = path.parent().unwrap_or(Path::new("."));
        let parent = std::fs::canonicalize(parent)?;
        let path = parent.join(
            path.file_name()
                .ok_or(RegionError::Invalid("missing filename"))?,
        );
        let mut locks = LOCKS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .map_err(|_| RegionError::Invalid("poisoned lock"))?;
        let lock = locks.get(&path).and_then(Weak::upgrade).unwrap_or_else(|| {
            let lock = Arc::new(Mutex::new(()));
            locks.insert(path.clone(), Arc::downgrade(&lock));
            lock
        });
        Ok(Self { path, rx, rz, lock })
    }
    fn index(&self, x: i32, z: i32) -> Result<usize, RegionError> {
        if x.div_euclid(32) != self.rx || z.div_euclid(32) != self.rz {
            return Err(RegionError::Coordinates);
        }
        Ok((x.rem_euclid(32) + 32 * z.rem_euclid(32)) as usize)
    }
    fn sidecar(&self, x: i32, z: i32) -> PathBuf {
        self.path.with_file_name(format!("c.{x}.{z}.mcc"))
    }
    /// Returns all `(chunk_x, chunk_z)` coordinates present in this region file's header.
    pub fn present_chunks(&self) -> Result<Vec<(i32, i32)>, RegionError> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| RegionError::Invalid("poisoned lock"))?;
        let mut file = match File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        if file.metadata()?.len() < SECTOR as u64 {
            return Ok(Vec::new());
        }
        let mut locations = [0u8; SECTOR];
        file.read_exact(&mut locations)?;
        let mut out = Vec::new();
        for lz in 0..32i32 {
            for lx in 0..32i32 {
                let index = (lx + 32 * lz) as usize;
                let loc =
                    u32::from_be_bytes(locations[index * 4..index * 4 + 4].try_into().unwrap());
                if loc != 0 {
                    out.push((self.rx * 32 + lx, self.rz * 32 + lz));
                }
            }
        }
        Ok(out)
    }

    pub fn read_chunk(&self, x: i32, z: i32) -> Result<Option<Vec<u8>>, RegionError> {
        let index = self.index(x, z)?;
        let _guard = self
            .lock
            .lock()
            .map_err(|_| RegionError::Invalid("poisoned lock"))?;
        let mut file = match File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let mut header = [0u8; SECTOR * 2];
        file.read_exact(&mut header)?;
        let location = u32::from_be_bytes(header[index * 4..index * 4 + 4].try_into().unwrap());
        if location == 0 {
            return Ok(None);
        }
        let sector = (location >> 8) as u64;
        let sectors = (location & 255) as usize;
        if sector < 2
            || sectors == 0
            || sector * SECTOR as u64 + sectors as u64 * SECTOR as u64 > file.metadata()?.len()
        {
            return Err(RegionError::Invalid("invalid sector location"));
        }
        file.seek(SeekFrom::Start(sector * SECTOR as u64))?;
        let mut length = [0; 4];
        file.read_exact(&mut length)?;
        let length = u32::from_be_bytes(length) as usize;
        if length == 0 || length + 4 > sectors * SECTOR {
            return Err(RegionError::Invalid("invalid chunk length"));
        }
        let mut kind = [0];
        file.read_exact(&mut kind)?;
        let external = kind[0] & 0x80 != 0;
        let kind = kind[0] & 0x7f;
        let compressed = if external {
            if length != 1 {
                return Err(RegionError::Invalid("external chunk has inline payload"));
            }
            let mut bytes = Vec::new();
            File::open(self.sidecar(x, z))?
                .take((LIMIT + 1) as u64)
                .read_to_end(&mut bytes)?;
            bytes
        } else {
            let mut bytes = vec![0; length - 1];
            file.read_exact(&mut bytes)?;
            bytes
        };
        if compressed.len() > LIMIT {
            return Err(RegionError::Invalid("compressed size limit"));
        }
        Ok(Some(decompress(kind, &compressed)?))
    }
    pub fn write_chunk(
        &self,
        x: i32,
        z: i32,
        bytes: &[u8],
        compression: RegionCompression,
    ) -> Result<(), RegionError> {
        let index = self.index(x, z)?;
        if bytes.len() > LIMIT {
            return Err(RegionError::Invalid("chunk size limit"));
        }
        let data = compress(compression, bytes)?;
        let _guard = self
            .lock
            .lock()
            .map_err(|_| RegionError::Invalid("poisoned lock"))?;
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&self.path)?;
        if file.metadata()?.len() == 0 {
            file.write_all(&[0u8; SECTOR * 2])?;
        }
        let mut header = [0u8; SECTOR * 2];
        file.seek(SeekFrom::Start(0))?;
        file.read_exact(&mut header)?;
        let external = (data.len() + 5).div_ceil(SECTOR) > 255;
        let sectors = if external {
            1
        } else {
            (data.len() + 5).div_ceil(SECTOR)
        };
        let total = (file.metadata()?.len() as usize).div_ceil(SECTOR);
        let mut used = vec![false; total.max(2)];
        used[0] = true;
        used[1] = true;
        for slot in 0..1024 {
            let loc = u32::from_be_bytes(header[slot * 4..slot * 4 + 4].try_into().unwrap());
            if loc == 0 {
                continue;
            }
            let start = (loc >> 8) as usize;
            let count = (loc & 255) as usize;
            if start < 2 || count == 0 || start.checked_add(count).is_none_or(|end| end > total) {
                return Err(RegionError::Invalid("invalid location table"));
            }
            for bit in &mut used[start..start + count] {
                if *bit {
                    return Err(RegionError::Invalid("overlapping locations"));
                }
                *bit = true;
            }
        }
        let start = (2..=used.len())
            .find(|&i| i + sectors <= used.len() && used[i..i + sectors].iter().all(|v| !v))
            .unwrap_or(used.len());
        if start >= 1 << 24 {
            return Err(RegionError::Invalid("region too large"));
        }
        if external {
            std::fs::write(self.sidecar(x, z), &data)?;
        }
        file.seek(SeekFrom::Start((start * SECTOR) as u64))?;
        file.write_all(
            &(if external {
                1u32
            } else {
                (data.len() + 1) as u32
            })
            .to_be_bytes(),
        )?;
        file.write_all(&[(compression as u8) | if external { 0x80 } else { 0 }])?;
        if !external {
            file.write_all(&data)?;
        }
        let written = if external { 5 } else { data.len() + 5 };
        file.write_all(&vec![0; sectors * SECTOR - written])?;
        let location = ((start as u32) << 8) | sectors as u32;
        header[index * 4..index * 4 + 4].copy_from_slice(&location.to_be_bytes());
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as u32;
        header[SECTOR + index * 4..SECTOR + index * 4 + 4]
            .copy_from_slice(&timestamp.to_be_bytes());
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&header)?;
        file.flush()?;
        if !external {
            match std::fs::remove_file(self.sidecar(x, z)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
}
fn decompress(kind: u8, bytes: &[u8]) -> Result<Vec<u8>, RegionError> {
    if kind == 4 {
        return lz4_read(bytes);
    }
    let r: Box<dyn Read + '_> = match kind {
        1 => Box::new(flate2::read::GzDecoder::new(bytes)),
        2 => Box::new(flate2::read::ZlibDecoder::new(bytes)),
        3 => Box::new(bytes),
        n => return Err(RegionError::Compression(n)),
    };
    let mut out = Vec::new();
    r.take((LIMIT + 1) as u64).read_to_end(&mut out)?;
    if out.len() > LIMIT {
        return Err(RegionError::Invalid("decompressed size limit"));
    }
    Ok(out)
}
fn compress(kind: RegionCompression, bytes: &[u8]) -> Result<Vec<u8>, RegionError> {
    match kind {
        RegionCompression::None => Ok(bytes.to_vec()),
        RegionCompression::Lz4 => Ok(lz4_write(bytes)),
        RegionCompression::Gzip => {
            let mut w = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            w.write_all(bytes)?;
            Ok(w.finish()?)
        }
        RegionCompression::Zlib => {
            let mut w = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            w.write_all(bytes)?;
            Ok(w.finish()?)
        }
    }
}
fn lz4_write(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for block in bytes.chunks(65536) {
        let compressed = lz4_flex::block::compress(block);
        let (method, data) = if compressed.len() < block.len() {
            (0x20, compressed.as_slice())
        } else {
            (0x10, block)
        };
        out.extend_from_slice(b"LZ4Block");
        out.push(method | 6);
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(block.len() as u32).to_le_bytes());
        out.extend_from_slice(&xxhash_rust::xxh32::xxh32(block, 0x9747b28c).to_le_bytes());
        out.extend_from_slice(data);
    }
    out.extend_from_slice(b"LZ4Block");
    out.push(0x16);
    out.extend_from_slice(&[0; 12]);
    out
}
fn lz4_read(mut bytes: &[u8]) -> Result<Vec<u8>, RegionError> {
    let mut out = Vec::new();
    loop {
        if bytes.len() < 21 || &bytes[..8] != b"LZ4Block" {
            return Err(RegionError::Invalid("invalid LZ4 header"));
        }
        let token = bytes[8];
        let compressed = u32::from_le_bytes(bytes[9..13].try_into().unwrap()) as usize;
        let original = u32::from_le_bytes(bytes[13..17].try_into().unwrap()) as usize;
        let hash = u32::from_le_bytes(bytes[17..21].try_into().unwrap());
        bytes = &bytes[21..];
        if original == 0 && compressed == 0 && hash == 0 {
            break;
        }
        if original > (1usize << (10 + (token & 15)))
            || compressed > bytes.len()
            || out.len().checked_add(original).is_none_or(|n| n > LIMIT)
        {
            return Err(RegionError::Invalid("invalid LZ4 size"));
        }
        let block = match token & 0xf0 {
            0x10 if compressed == original => bytes[..compressed].to_vec(),
            0x20 => lz4_flex::block::decompress(&bytes[..compressed], original)
                .map_err(|_| RegionError::Invalid("invalid LZ4 block"))?,
            _ => return Err(RegionError::Invalid("invalid LZ4 method")),
        };
        if block.len() != original || xxhash_rust::xxh32::xxh32(&block, 0x9747b28c) != hash {
            return Err(RegionError::Invalid("invalid LZ4 checksum"));
        }
        out.extend(block);
        bytes = &bytes[compressed..];
    }
    Ok(out)
}
