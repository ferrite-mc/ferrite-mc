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
static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Weak<Mutex<RegionInner>>>>> = OnceLock::new();

#[derive(Default)]
struct RegionInner {
    file: Option<File>,
    writable: bool,
    header: Option<Box<[u8; SECTOR * 2]>>,
    file_len: u64,
    write_buf: Vec<u8>,
}

impl std::fmt::Debug for RegionInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegionInner")
            .field("writable", &self.writable)
            .field("has_header", &self.header.is_some())
            .field("file_len", &self.file_len)
            .finish()
    }
}

impl RegionInner {
    fn ensure_read(&mut self, path: &Path) -> Result<bool, RegionError> {
        if self.file.is_none() {
            let mut file = match File::open(path) {
                Ok(f) => f,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(e) => return Err(e.into()),
            };
            let len = file.metadata()?.len();
            self.file_len = len;
            self.writable = false;
            if len >= (SECTOR * 2) as u64 {
                let mut hdr = Box::new([0u8; SECTOR * 2]);
                file.read_exact(&mut *hdr)?;
                self.header = Some(hdr);
            } else {
                self.header = None;
            }
            self.file = Some(file);
        }
        Ok(true)
    }

    fn ensure_write(&mut self, path: &Path) -> Result<(), RegionError> {
        if !self.writable || self.file.is_none() {
            self.file = None;
            let mut file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(path)?;
            let mut len = file.metadata()?.len();
            if len == 0 {
                let hdr = Box::new([0u8; SECTOR * 2]);
                file.write_all(&*hdr)?;
                len = (SECTOR * 2) as u64;
                self.header = Some(hdr);
            } else if self.header.is_none() {
                let mut hdr = Box::new([0u8; SECTOR * 2]);
                file.seek(SeekFrom::Start(0))?;
                file.read_exact(&mut *hdr)?;
                self.header = Some(hdr);
            }
            self.file_len = len;
            self.writable = true;
            self.file = Some(file);
        } else if self.header.is_none()
            && let Some(file) = self.file.as_mut()
        {
            let mut hdr = Box::new([0u8; SECTOR * 2]);
            file.seek(SeekFrom::Start(0))?;
            file.read_exact(&mut *hdr)?;
            self.header = Some(hdr);
        }
        Ok(())
    }
}

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
    lock: Arc<Mutex<RegionInner>>,
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
            let lock = Arc::new(Mutex::new(RegionInner::default()));
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
    /// Compresses a raw chunk payload so callers can parallelize compression across cores before
    /// acquiring the per-region file lock.
    pub fn compress_chunk(
        bytes: &[u8],
        compression: RegionCompression,
    ) -> Result<Vec<u8>, RegionError> {
        if bytes.len() > LIMIT {
            return Err(RegionError::Invalid("chunk size limit"));
        }
        compress(compression, bytes)
    }
    /// Returns all `(chunk_x, chunk_z)` coordinates present in this region file's header.
    pub fn present_chunks(&self) -> Result<Vec<(i32, i32)>, RegionError> {
        let mut inner = self
            .lock
            .lock()
            .map_err(|_| RegionError::Invalid("poisoned lock"))?;
        if !inner.ensure_read(&self.path)? {
            return Ok(Vec::new());
        }
        if inner.file_len < SECTOR as u64 {
            return Ok(Vec::new());
        }
        let mut locations_buf;
        let locations: &[u8] = if let Some(hdr) = &inner.header {
            &hdr[..SECTOR]
        } else {
            locations_buf = [0u8; SECTOR];
            let file = inner.file.as_mut().unwrap();
            file.seek(SeekFrom::Start(0))?;
            file.read_exact(&mut locations_buf)?;
            &locations_buf
        };
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
        let mut inner = self
            .lock
            .lock()
            .map_err(|_| RegionError::Invalid("poisoned lock"))?;
        if !inner.ensure_read(&self.path)? {
            return Ok(None);
        }
        let Some(header) = inner.header.as_ref() else {
            return Err(RegionError::Io(std::io::ErrorKind::UnexpectedEof.into()));
        };
        let location = u32::from_be_bytes(header[index * 4..index * 4 + 4].try_into().unwrap());
        if location == 0 {
            return Ok(None);
        }
        let sector = (location >> 8) as u64;
        let sectors = (location & 255) as usize;
        if sector < 2
            || sectors == 0
            || sector * SECTOR as u64 + sectors as u64 * SECTOR as u64 > inner.file_len
        {
            return Err(RegionError::Invalid("invalid sector location"));
        }
        let file = inner.file.as_mut().unwrap();
        file.seek(SeekFrom::Start(sector * SECTOR as u64))?;
        let mut head5 = [0u8; 5];
        file.read_exact(&mut head5)?;
        let length = u32::from_be_bytes([head5[0], head5[1], head5[2], head5[3]]) as usize;
        if length == 0 || length + 4 > sectors * SECTOR {
            return Err(RegionError::Invalid("invalid chunk length"));
        }
        let external = head5[4] & 0x80 != 0;
        let kind = head5[4] & 0x7f;
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
        drop(inner);
        Ok(Some(decompress(kind, &compressed)?))
    }
    pub fn write_chunk(
        &self,
        x: i32,
        z: i32,
        bytes: &[u8],
        compression: RegionCompression,
    ) -> Result<(), RegionError> {
        let _ = self.index(x, z)?;
        let data = Self::compress_chunk(bytes, compression)?;
        self.write_compressed_batch(&[(x, z, data)], compression)
    }
    /// Writes a batch of pre-compressed chunks into this region file with a single header commit.
    pub fn write_compressed_batch(
        &self,
        chunks: &[(i32, i32, Vec<u8>)],
        compression: RegionCompression,
    ) -> Result<(), RegionError> {
        if chunks.is_empty() {
            return Ok(());
        }
        let mut indexed = Vec::with_capacity(chunks.len());
        for (x, z, data) in chunks {
            indexed.push((self.index(*x, *z)?, *x, *z, data.as_slice()));
        }
        let mut inner = self
            .lock
            .lock()
            .map_err(|_| RegionError::Invalid("poisoned lock"))?;
        inner.ensure_write(&self.path)?;
        let total = (inner.file_len as usize).div_ceil(SECTOR);
        let mut used = vec![false; total.max(2)];
        used[0] = true;
        used[1] = true;
        {
            let header = inner.header.as_ref().unwrap();
            for slot in 0..1024 {
                let loc = u32::from_be_bytes(header[slot * 4..slot * 4 + 4].try_into().unwrap());
                if loc == 0 {
                    continue;
                }
                let start = (loc >> 8) as usize;
                let count = (loc & 255) as usize;
                if start < 2 || count == 0 || start.checked_add(count).is_none_or(|end| end > total)
                {
                    return Err(RegionError::Invalid("invalid location table"));
                }
                for bit in &mut used[start..start + count] {
                    if *bit {
                        return Err(RegionError::Invalid("overlapping locations"));
                    }
                    *bit = true;
                }
            }
        }

        let timestamp_be = (SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as u32)
            .to_be_bytes();
        let mut sidecars_to_clean = Vec::new();

        for (index, x, z, data) in indexed {
            let external = (data.len() + 5).div_ceil(SECTOR) > 255;
            let sectors = if external {
                1
            } else {
                (data.len() + 5).div_ceil(SECTOR)
            };
            let start = (2..=used.len())
                .find(|&i| i + sectors <= used.len() && used[i..i + sectors].iter().all(|v| !v))
                .unwrap_or(used.len());
            if start >= 1 << 24 {
                return Err(RegionError::Invalid("region too large"));
            }
            if start + sectors > used.len() {
                used.resize(start + sectors, false);
            }
            used[start..start + sectors].fill(true);

            if external {
                std::fs::write(self.sidecar(x, z), data)?;
            }

            let total_bytes = sectors * SECTOR;
            let RegionInner {
                file,
                header,
                file_len,
                write_buf,
                ..
            } = &mut *inner;
            write_buf.clear();
            write_buf.reserve(total_bytes);
            let len_u32 = if external {
                1u32
            } else {
                (data.len() + 1) as u32
            };
            write_buf.extend_from_slice(&len_u32.to_be_bytes());
            write_buf.push((compression as u8) | if external { 0x80 } else { 0 });
            if !external {
                write_buf.extend_from_slice(data);
            }
            write_buf.resize(total_bytes, 0);

            let file = file.as_mut().unwrap();
            file.seek(SeekFrom::Start((start * SECTOR) as u64))?;
            file.write_all(write_buf)?;
            let end_bytes = ((start + sectors) * SECTOR) as u64;
            if end_bytes > *file_len {
                *file_len = end_bytes;
            }

            let header = header.as_mut().unwrap();
            let old_loc = u32::from_be_bytes(header[index * 4..index * 4 + 4].try_into().unwrap());
            let location = ((start as u32) << 8) | sectors as u32;
            header[index * 4..index * 4 + 4].copy_from_slice(&location.to_be_bytes());
            header[SECTOR + index * 4..SECTOR + index * 4 + 4].copy_from_slice(&timestamp_be);
            if !external && (old_loc & 255) == 1 {
                sidecars_to_clean.push((x, z));
            }
        }

        {
            let RegionInner { file, header, .. } = &mut *inner;
            let header_bytes: &[u8; SECTOR * 2] = header.as_ref().unwrap();
            let file = file.as_mut().unwrap();
            file.seek(SeekFrom::Start(0))?;
            file.write_all(header_bytes)?;
            file.flush()?;
        }

        for (x, z) in sidecars_to_clean {
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
