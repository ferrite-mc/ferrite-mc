//! Ferrite single-blob compressed world storage (`.fworld` / `FWRD` v3 format).
//!
//! Stores all chunks of a world/dimension and its spawn point in one integrity-checked binary
//! blob loaded wholly into RAM on startup, eliminating per-chunk region file seeks and locks.
//!
//! Format (`FWRD` v3, big-endian, with v2 decode compatibility):
//! - `0..4`: magic `b"FWRD"`
//! - `4..6`: format version (`3u16`, or `2u16` legacy whole-blob)
//! - `6`: [`BlobCompression`] id (`0=None, 1=Lz4, 2=Zlib, 3=Gzip`)
//! - `7`: header flags (`FLAG_CHECKSUM_XXH32 = 1`)
//! - `8..20`: `spawn[0..3]` (`3 * i32`)
//! - `20..24`: `chunk_count` (`u32`)
//! - `24..28`: `total_raw_len` (`u32`, pre-checked against limit before any allocation)
//! - `28..`: `chunk_count` independent compressed chunk records:
//!   `[x: i32, z: i32, raw_len: u32, stored_len: u32, xxh32: u32, payload: [u8; stored_len]]`
//!
//! Storing per-chunk compressed records enables multi-core parallel encode/decode, clean-chunk
//! zero-recompression incremental flushes, and cold compressed storage in RAM.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;

use xxhash_rust::xxh32::xxh32;

const MAGIC: &[u8; 4] = b"FWRD";
const VERSION_V2: u16 = 2;
const VERSION_V3: u16 = 3;
const HEADER_LEN_V2: usize = 16;
const HEADER_LEN_V3: usize = 28;
const RECORD_HEADER_LEN_V3: usize = 20;
const FLAG_CHECKSUM_XXH32: u8 = 0x01;
const MIN_CHUNK_ENTRY_BYTES: usize = 12;
const PARALLEL_CHUNK_THRESHOLD: usize = 16;

/// Hard ceiling on uncompressed `.fworld` payload size (512 MiB) to bound decompression bombs.
pub const MAX_FWORLD_UNCOMPRESSED_BYTES: usize = 512 * 1024 * 1024;

/// Compression algorithm applied to `.fworld` chunk payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum BlobCompression {
    None = 0,
    #[default]
    Lz4 = 1,
    Zlib = 2,
    Gzip = 3,
}

impl BlobCompression {
    pub fn from_u8(v: u8) -> Result<Self, BlobError> {
        match v {
            0 => Ok(Self::None),
            1 => Ok(Self::Lz4),
            2 => Ok(Self::Zlib),
            3 => Ok(Self::Gzip),
            other => Err(BlobError::UnsupportedCompression(other)),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BlobError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("invalid .fworld magic header")]
    BadMagic,
    #[error("unsupported .fworld format version: {0}")]
    UnsupportedVersion(u16),
    #[error("unsupported .fworld compression id: {0}")]
    UnsupportedCompression(u8),
    #[error("truncated or corrupt .fworld blob")]
    Truncated,
    #[error(".fworld payload exceeds maximum uncompressed size ({max} bytes)")]
    PayloadTooLarge { max: usize },
    #[error(".fworld integrity check failed (expected xxh32 {expected:#010x}, got {actual:#010x})")]
    ChecksumMismatch { expected: u32, actual: u32 },
}

/// Already-compressed single-chunk record kept cold in RAM for zero-recompression flushes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompressedChunkRecord {
    pub raw_len: u32,
    pub checksum: u32,
    pub stored: Vec<u8>,
}

impl CompressedChunkRecord {
    pub fn compress(raw: &[u8], compression: BlobCompression) -> Result<Self, BlobError> {
        let raw_len = u32::try_from(raw.len()).map_err(|_| BlobError::PayloadTooLarge {
            max: MAX_FWORLD_UNCOMPRESSED_BYTES,
        })?;
        let checksum = xxh32(raw, 0);
        let stored = compress_payload(compression, raw)?;
        Ok(Self {
            raw_len,
            checksum,
            stored,
        })
    }

    pub fn decompress(
        &self,
        compression: BlobCompression,
        max_uncompressed_bytes: usize,
    ) -> Result<Vec<u8>, BlobError> {
        let raw_len = self.raw_len as usize;
        if raw_len > max_uncompressed_bytes {
            return Err(BlobError::PayloadTooLarge {
                max: max_uncompressed_bytes,
            });
        }
        let raw = decompress_payload(compression, &self.stored, raw_len, max_uncompressed_bytes)?;
        if raw.len() != raw_len {
            return Err(BlobError::Truncated);
        }
        let actual_crc = xxh32(&raw, 0);
        if actual_crc != self.checksum {
            return Err(BlobError::ChecksumMismatch {
                expected: self.checksum,
                actual: actual_crc,
            });
        }
        Ok(raw)
    }
}

/// Cold compressed in-memory `.fworld` representation: stores pre-compressed chunks to minimize
/// resident RAM and skips re-compressing unchanged chunks during atomic disk flushes.
#[derive(Debug, Clone, Default)]
pub struct FworldColdStore {
    pub spawn: [i32; 3],
    pub compression: BlobCompression,
    pub chunks: HashMap<(i32, i32), CompressedChunkRecord>,
}

impl FworldColdStore {
    pub fn new(spawn: [i32; 3], compression: BlobCompression) -> Self {
        Self {
            spawn,
            compression,
            chunks: HashMap::new(),
        }
    }

    pub fn insert_raw(&mut self, x: i32, z: i32, raw: &[u8]) -> Result<(), BlobError> {
        let rec = CompressedChunkRecord::compress(raw, self.compression)?;
        self.chunks.insert((x, z), rec);
        Ok(())
    }

    /// Compresses a batch of raw chunk payloads in parallel across available cores and stores
    /// their compressed records in `self.chunks`.
    pub fn insert_raw_batch(&mut self, entries: &[((i32, i32), Vec<u8>)]) -> Result<(), BlobError> {
        if entries.is_empty() {
            return Ok(());
        }
        let compression = self.compression;
        let workers = parallel_workers(entries.len(), compression);
        if workers == 1 {
            for ((x, z), raw) in entries {
                self.insert_raw(*x, *z, raw)?;
            }
            return Ok(());
        }
        type ColdBatchResult = Result<Vec<((i32, i32), CompressedChunkRecord)>, BlobError>;
        let chunk_size = entries.len().div_ceil(workers);
        let mut results: Vec<ColdBatchResult> = (0..entries.len().div_ceil(chunk_size))
            .map(|_| Ok(Vec::new()))
            .collect();
        std::thread::scope(|s| {
            for (slot, slice) in results.iter_mut().zip(entries.chunks(chunk_size)) {
                s.spawn(move || {
                    let mut local = Vec::with_capacity(slice.len());
                    for ((x, z), raw) in slice {
                        match CompressedChunkRecord::compress(raw, compression) {
                            Ok(rec) => local.push(((*x, *z), rec)),
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
        for res in results {
            for (k, rec) in res? {
                self.chunks.insert(k, rec);
            }
        }
        Ok(())
    }

    pub fn get_raw(&self, x: i32, z: i32) -> Result<Option<Vec<u8>>, BlobError> {
        self.chunks
            .get(&(x, z))
            .map(|rec| rec.decompress(self.compression, MAX_FWORLD_UNCOMPRESSED_BYTES))
            .transpose()
    }

    pub fn read_file(
        path: impl AsRef<Path>,
        target_compression: BlobCompression,
    ) -> Result<Self, BlobError> {
        let mut reader = std::io::BufReader::new(std::fs::File::open(path.as_ref())?);
        let mut hdr = [0u8; HEADER_LEN_V3];
        let mut n = 0usize;
        while n < HEADER_LEN_V3 {
            let read = reader.read(&mut hdr[n..])?;
            if read == 0 {
                break;
            }
            n += read;
        }
        if n == HEADER_LEN_V3
            && &hdr[0..4] == MAGIC
            && u16::from_be_bytes([hdr[4], hdr[5]]) == VERSION_V3
        {
            let file_compression = BlobCompression::from_u8(hdr[6])?;
            if file_compression == target_compression {
                return Self::decode_v3_cold_stream(&hdr, reader, MAX_FWORLD_UNCOMPRESSED_BYTES);
            }
        }
        let mut data = Vec::with_capacity(n);
        data.extend_from_slice(&hdr[..n]);
        reader.read_to_end(&mut data)?;
        let world = Fworld::decode_from_bytes(&data)?;
        let mut store = Self::new(world.spawn, target_compression);
        let entries: Vec<_> = world.chunks.into_iter().collect();
        let workers = parallel_workers(entries.len(), target_compression);
        if workers == 1 {
            for ((x, z), raw) in entries {
                store.insert_raw(x, z, &raw)?;
            }
        } else {
            type ColdBatchResult = Result<Vec<((i32, i32), CompressedChunkRecord)>, BlobError>;
            let chunk_size = entries.len().div_ceil(workers);
            let mut results: Vec<ColdBatchResult> = (0..entries.len().div_ceil(chunk_size))
                .map(|_| Ok(Vec::new()))
                .collect();
            std::thread::scope(|s| {
                for (slot, slice) in results.iter_mut().zip(entries.chunks(chunk_size)) {
                    s.spawn(move || {
                        let mut local = Vec::with_capacity(slice.len());
                        for ((x, z), raw) in slice {
                            match CompressedChunkRecord::compress(raw, target_compression) {
                                Ok(rec) => local.push(((*x, *z), rec)),
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
            for res in results {
                for (k, rec) in res? {
                    store.chunks.insert(k, rec);
                }
            }
        }
        Ok(store)
    }

    fn decode_v3_cold_stream(
        hdr: &[u8; HEADER_LEN_V3],
        mut reader: std::io::BufReader<std::fs::File>,
        max_uncompressed_bytes: usize,
    ) -> Result<Self, BlobError> {
        let compression = BlobCompression::from_u8(hdr[6])?;
        let spawn = [
            i32::from_be_bytes(hdr[8..12].try_into().unwrap()),
            i32::from_be_bytes(hdr[12..16].try_into().unwrap()),
            i32::from_be_bytes(hdr[16..20].try_into().unwrap()),
        ];
        let count = u32::from_be_bytes(hdr[20..24].try_into().unwrap()) as usize;
        let declared_total = u32::from_be_bytes(hdr[24..28].try_into().unwrap()) as usize;
        if declared_total > max_uncompressed_bytes {
            return Err(BlobError::PayloadTooLarge {
                max: max_uncompressed_bytes,
            });
        }
        if let Ok(meta) = reader.get_ref().metadata() {
            let remaining = meta.len().saturating_sub(HEADER_LEN_V3 as u64);
            if (count as u64) > remaining / (RECORD_HEADER_LEN_V3 as u64) {
                return Err(BlobError::Truncated);
            }
        }
        let map_io = |err: std::io::Error| {
            if err.kind() == std::io::ErrorKind::UnexpectedEof {
                BlobError::Truncated
            } else {
                BlobError::Io(err)
            }
        };
        let mut total_raw = 0usize;
        let mut chunks = HashMap::with_capacity(count.min(65_536));
        for _ in 0..count {
            let mut rec_hdr = [0u8; RECORD_HEADER_LEN_V3];
            reader.read_exact(&mut rec_hdr).map_err(map_io)?;
            let x = i32::from_be_bytes(rec_hdr[0..4].try_into().unwrap());
            let z = i32::from_be_bytes(rec_hdr[4..8].try_into().unwrap());
            let raw_len = u32::from_be_bytes(rec_hdr[8..12].try_into().unwrap());
            let stored_len = u32::from_be_bytes(rec_hdr[12..16].try_into().unwrap()) as usize;
            let checksum = u32::from_be_bytes(rec_hdr[16..20].try_into().unwrap());
            total_raw =
                total_raw
                    .checked_add(raw_len as usize)
                    .ok_or(BlobError::PayloadTooLarge {
                        max: max_uncompressed_bytes,
                    })?;
            if total_raw > max_uncompressed_bytes {
                return Err(BlobError::PayloadTooLarge {
                    max: max_uncompressed_bytes,
                });
            }
            if stored_len > max_uncompressed_bytes {
                return Err(BlobError::Truncated);
            }
            let mut stored = vec![0u8; stored_len];
            reader.read_exact(&mut stored).map_err(map_io)?;
            chunks.insert(
                (x, z),
                CompressedChunkRecord {
                    raw_len,
                    checksum,
                    stored,
                },
            );
        }
        Ok(Self {
            spawn,
            compression,
            chunks,
        })
    }

    pub fn encode_to_bytes(&self) -> Result<Vec<u8>, BlobError> {
        let mut entries: Vec<(&(i32, i32), &CompressedChunkRecord)> = self.chunks.iter().collect();
        entries.sort_by_key(|&(&(x, z), _)| (z, x));
        let mut total_raw = 0usize;
        let mut payload_bytes = 0usize;
        for (_, rec) in &entries {
            total_raw =
                total_raw
                    .checked_add(rec.raw_len as usize)
                    .ok_or(BlobError::PayloadTooLarge {
                        max: MAX_FWORLD_UNCOMPRESSED_BYTES,
                    })?;
            payload_bytes += RECORD_HEADER_LEN_V3 + rec.stored.len();
        }
        let total_raw_u32 = u32::try_from(total_raw).map_err(|_| BlobError::PayloadTooLarge {
            max: MAX_FWORLD_UNCOMPRESSED_BYTES,
        })?;
        let mut out = Vec::with_capacity(HEADER_LEN_V3 + payload_bytes);
        write_v3_header(
            &mut out,
            self.compression,
            self.spawn,
            entries.len() as u32,
            total_raw_u32,
        );
        for (&(x, z), rec) in entries {
            out.extend_from_slice(&x.to_be_bytes());
            out.extend_from_slice(&z.to_be_bytes());
            out.extend_from_slice(&rec.raw_len.to_be_bytes());
            out.extend_from_slice(&(rec.stored.len() as u32).to_be_bytes());
            out.extend_from_slice(&rec.checksum.to_be_bytes());
            out.extend_from_slice(&rec.stored);
        }
        Ok(out)
    }

    pub fn write_file_atomic(&self, path: impl AsRef<Path>) -> Result<(), BlobError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let encoded = self.encode_to_bytes()?;
        let tmp = path.with_extension("fworld.tmp");
        std::fs::write(&tmp, encoded)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Writes a timestamped `.fworld` snapshot into `backup_dir` and rotates old snapshots,
    /// retaining at most `max_keep` newest snapshots matching `prefix` (`0` = keep all).
    pub fn write_snapshot_to_dir(
        &self,
        backup_dir: impl AsRef<Path>,
        prefix: &str,
        max_keep: usize,
    ) -> Result<std::path::PathBuf, BlobError> {
        let dir = backup_dir.as_ref();
        std::fs::create_dir_all(dir)?;
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let safe_prefix = if prefix.is_empty() { "world" } else { prefix };
        let mut target = dir.join(format!("{safe_prefix}-{millis}.fworld"));
        let mut seq = 1u32;
        while target.exists() {
            target = dir.join(format!("{safe_prefix}-{millis}-{seq}.fworld"));
            seq = seq.saturating_add(1);
        }
        self.write_file_atomic(&target)?;

        if max_keep > 0
            && let Ok(entries) = std::fs::read_dir(dir)
        {
            let mut snapshots: Vec<std::path::PathBuf> = entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.is_file()
                        && p.file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n.starts_with(safe_prefix) && n.ends_with(".fworld"))
                })
                .collect();
            snapshots.sort();
            while snapshots.len() > max_keep {
                let oldest = snapshots.remove(0);
                let _ = std::fs::remove_file(oldest);
            }
        }
        Ok(target)
    }
}

/// A Ferrite single-blob world (`.fworld`) decoded in memory.
#[derive(Debug, Clone, Default)]
pub struct Fworld {
    pub spawn: [i32; 3],
    pub chunks: HashMap<(i32, i32), Vec<u8>>,
}

impl Fworld {
    pub fn new(spawn: [i32; 3]) -> Self {
        Self {
            spawn,
            chunks: HashMap::new(),
        }
    }

    /// Returns an independent in-memory clone of the world (e.g. for instancing or rollback).
    pub fn clone_independent(&self) -> Self {
        self.clone()
    }

    /// Encodes the entire world into a parallel-compressed, integrity-checked `FWRD` v3 `.fworld` blob.
    pub fn encode_to_bytes(&self, compression: BlobCompression) -> Result<Vec<u8>, BlobError> {
        let mut entries: Vec<(&(i32, i32), &Vec<u8>)> = self.chunks.iter().collect();
        entries.sort_by_key(|&(&(x, z), _)| (z, x));

        let mut total_raw = 0usize;
        for (_, raw) in &entries {
            total_raw = total_raw
                .checked_add(raw.len())
                .ok_or(BlobError::PayloadTooLarge {
                    max: MAX_FWORLD_UNCOMPRESSED_BYTES,
                })?;
        }
        let total_raw_u32 = u32::try_from(total_raw).map_err(|_| BlobError::PayloadTooLarge {
            max: MAX_FWORLD_UNCOMPRESSED_BYTES,
        })?;

        let records = compress_entries_parallel(&entries, compression)?;
        let payload_bytes: usize = records
            .iter()
            .map(|(_, _, _, stored)| RECORD_HEADER_LEN_V3 + stored.len())
            .sum();

        let mut out = Vec::with_capacity(HEADER_LEN_V3 + payload_bytes);
        write_v3_header(
            &mut out,
            compression,
            self.spawn,
            records.len() as u32,
            total_raw_u32,
        );
        for ((x, z), raw_len, checksum, stored) in records {
            out.extend_from_slice(&x.to_be_bytes());
            out.extend_from_slice(&z.to_be_bytes());
            out.extend_from_slice(&raw_len.to_be_bytes());
            out.extend_from_slice(&(stored.len() as u32).to_be_bytes());
            out.extend_from_slice(&checksum.to_be_bytes());
            out.extend_from_slice(&stored);
        }
        Ok(out)
    }

    /// Decodes a `.fworld` blob (`FWRD` v3 or legacy v2) into memory using [`MAX_FWORLD_UNCOMPRESSED_BYTES`].
    pub fn decode_from_bytes(data: &[u8]) -> Result<Self, BlobError> {
        Self::decode_from_bytes_with_limit(data, MAX_FWORLD_UNCOMPRESSED_BYTES)
    }

    /// Decodes a `.fworld` blob while capping total decompressed payload size and verifying `xxh32`.
    pub fn decode_from_bytes_with_limit(
        data: &[u8],
        max_uncompressed_bytes: usize,
    ) -> Result<Self, BlobError> {
        if data.len() < 4 {
            return Err(BlobError::Truncated);
        }
        if &data[0..4] != MAGIC {
            return Err(BlobError::BadMagic);
        }
        if data.len() < 6 {
            return Err(BlobError::Truncated);
        }
        let version = u16::from_be_bytes([data[4], data[5]]);
        match version {
            VERSION_V3 => decode_v3(data, max_uncompressed_bytes),
            VERSION_V2 => decode_v2(data, max_uncompressed_bytes),
            other => Err(BlobError::UnsupportedVersion(other)),
        }
    }

    pub fn read_file(path: impl AsRef<Path>) -> Result<Self, BlobError> {
        let bytes = std::fs::read(path)?;
        Self::decode_from_bytes(&bytes)
    }

    pub fn write_file_atomic(
        &self,
        path: impl AsRef<Path>,
        compression: BlobCompression,
    ) -> Result<(), BlobError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let encoded = self.encode_to_bytes(compression)?;
        let tmp = path.with_extension("fworld.tmp");
        std::fs::write(&tmp, encoded)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

struct HeaderV3 {
    compression: BlobCompression,
    spawn: [i32; 3],
    count: usize,
}

fn write_v3_header(
    out: &mut Vec<u8>,
    compression: BlobCompression,
    spawn: [i32; 3],
    count: u32,
    total_raw_len: u32,
) {
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION_V3.to_be_bytes());
    out.push(compression as u8);
    out.push(FLAG_CHECKSUM_XXH32);
    for coord in spawn {
        out.extend_from_slice(&coord.to_be_bytes());
    }
    out.extend_from_slice(&count.to_be_bytes());
    out.extend_from_slice(&total_raw_len.to_be_bytes());
}

fn parse_v3_header(data: &[u8], max_uncompressed_bytes: usize) -> Result<HeaderV3, BlobError> {
    if data.len() < HEADER_LEN_V3 {
        return Err(BlobError::Truncated);
    }
    let compression = BlobCompression::from_u8(data[6])?;
    let spawn = [
        i32::from_be_bytes(data[8..12].try_into().unwrap()),
        i32::from_be_bytes(data[12..16].try_into().unwrap()),
        i32::from_be_bytes(data[16..20].try_into().unwrap()),
    ];
    let count = u32::from_be_bytes(data[20..24].try_into().unwrap()) as usize;
    let declared_total = u32::from_be_bytes(data[24..28].try_into().unwrap()) as usize;
    if declared_total > max_uncompressed_bytes {
        return Err(BlobError::PayloadTooLarge {
            max: max_uncompressed_bytes,
        });
    }
    let remaining_bytes = data.len().saturating_sub(HEADER_LEN_V3);
    if count > remaining_bytes / RECORD_HEADER_LEN_V3 {
        return Err(BlobError::Truncated);
    }
    Ok(HeaderV3 {
        compression,
        spawn,
        count,
    })
}

fn parallel_workers(len: usize, compression: BlobCompression) -> usize {
    if len < PARALLEL_CHUNK_THRESHOLD || compression == BlobCompression::None {
        return 1;
    }
    std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(8)
        .min(len)
}

type CompressedTuple = ((i32, i32), u32, u32, Vec<u8>);

fn compress_entries_parallel(
    entries: &[(&(i32, i32), &Vec<u8>)],
    compression: BlobCompression,
) -> Result<Vec<CompressedTuple>, BlobError> {
    let workers = parallel_workers(entries.len(), compression);
    if workers == 1 {
        let mut out = Vec::with_capacity(entries.len());
        for &(&(x, z), bytes) in entries {
            let rec = CompressedChunkRecord::compress(bytes, compression)?;
            out.push(((x, z), rec.raw_len, rec.checksum, rec.stored));
        }
        return Ok(out);
    }
    let batch = entries.len().div_ceil(workers);
    let mut parts: Vec<Result<Vec<CompressedTuple>, BlobError>> =
        (0..entries.len().div_ceil(batch))
            .map(|_| Ok(Vec::new()))
            .collect();
    std::thread::scope(|scope| {
        for (slot, slice) in parts.iter_mut().zip(entries.chunks(batch)) {
            scope.spawn(move || {
                let mut local = Vec::with_capacity(slice.len());
                for &(&(x, z), bytes) in slice {
                    match CompressedChunkRecord::compress(bytes, compression) {
                        Ok(rec) => local.push(((x, z), rec.raw_len, rec.checksum, rec.stored)),
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
    let mut out = Vec::with_capacity(entries.len());
    for part in parts {
        out.extend(part?);
    }
    Ok(out)
}

type DecodeJob<'a> = ((i32, i32), usize, u32, &'a [u8]);
type DecodedChunkBatch = Result<Vec<((i32, i32), Vec<u8>)>, BlobError>;

fn decode_v3(data: &[u8], max_uncompressed_bytes: usize) -> Result<Fworld, BlobError> {
    let header = parse_v3_header(data, max_uncompressed_bytes)?;
    let check_crc = (data[7] & FLAG_CHECKSUM_XXH32) != 0;
    let mut cursor = HEADER_LEN_V3;
    let mut total_raw = 0usize;
    let mut jobs: Vec<DecodeJob<'_>> = Vec::with_capacity(header.count);
    for _ in 0..header.count {
        let rec_hdr = data
            .get(cursor..cursor + RECORD_HEADER_LEN_V3)
            .ok_or(BlobError::Truncated)?;
        cursor += RECORD_HEADER_LEN_V3;
        let x = i32::from_be_bytes(rec_hdr[0..4].try_into().unwrap());
        let z = i32::from_be_bytes(rec_hdr[4..8].try_into().unwrap());
        let raw_len = u32::from_be_bytes(rec_hdr[8..12].try_into().unwrap()) as usize;
        let stored_len = u32::from_be_bytes(rec_hdr[12..16].try_into().unwrap()) as usize;
        let crc = u32::from_be_bytes(rec_hdr[16..20].try_into().unwrap());
        total_raw = total_raw
            .checked_add(raw_len)
            .ok_or(BlobError::PayloadTooLarge {
                max: max_uncompressed_bytes,
            })?;
        if total_raw > max_uncompressed_bytes {
            return Err(BlobError::PayloadTooLarge {
                max: max_uncompressed_bytes,
            });
        }
        let stored = data
            .get(cursor..cursor + stored_len)
            .ok_or(BlobError::Truncated)?;
        cursor += stored_len;
        jobs.push(((x, z), raw_len, crc, stored));
    }

    let workers = parallel_workers(jobs.len(), header.compression);
    let mut chunks = HashMap::with_capacity(jobs.len());
    if workers == 1 {
        for ((x, z), raw_len, crc, stored) in jobs {
            let raw = decompress_payload(header.compression, stored, raw_len, raw_len)?;
            if raw.len() != raw_len {
                return Err(BlobError::Truncated);
            }
            if check_crc {
                let actual = xxh32(&raw, 0);
                if actual != crc {
                    return Err(BlobError::ChecksumMismatch {
                        expected: crc,
                        actual,
                    });
                }
            }
            chunks.insert((x, z), raw);
        }
    } else {
        let batch = jobs.len().div_ceil(workers);
        let mut results: Vec<DecodedChunkBatch> = (0..jobs.len().div_ceil(batch))
            .map(|_| Ok(Vec::new()))
            .collect();
        std::thread::scope(|scope| {
            let comp = header.compression;
            for (slot, slice) in results.iter_mut().zip(jobs.chunks(batch)) {
                scope.spawn(move || {
                    let mut local = Vec::with_capacity(slice.len());
                    for &((x, z), raw_len, crc, stored) in slice {
                        let raw = match decompress_payload(comp, stored, raw_len, raw_len) {
                            Ok(r) => r,
                            Err(e) => {
                                *slot = Err(e);
                                return;
                            }
                        };
                        if raw.len() != raw_len {
                            *slot = Err(BlobError::Truncated);
                            return;
                        }
                        if check_crc {
                            let actual = xxh32(&raw, 0);
                            if actual != crc {
                                *slot = Err(BlobError::ChecksumMismatch {
                                    expected: crc,
                                    actual,
                                });
                                return;
                            }
                        }
                        local.push(((x, z), raw));
                    }
                    *slot = Ok(local);
                });
            }
        });
        for res in results {
            for (k, raw) in res? {
                chunks.insert(k, raw);
            }
        }
    }
    Ok(Fworld {
        spawn: header.spawn,
        chunks,
    })
}

fn decode_v2(data: &[u8], max_uncompressed_bytes: usize) -> Result<Fworld, BlobError> {
    if data.len() < HEADER_LEN_V2 {
        return Err(BlobError::Truncated);
    }
    let compression = BlobCompression::from_u8(data[6])?;
    let flags = data[7];
    let declared_len = u32::from_be_bytes(data[8..12].try_into().unwrap()) as usize;
    let expected_crc = u32::from_be_bytes(data[12..16].try_into().unwrap());
    if declared_len > max_uncompressed_bytes {
        return Err(BlobError::PayloadTooLarge {
            max: max_uncompressed_bytes,
        });
    }
    let raw = decompress_payload(
        compression,
        &data[HEADER_LEN_V2..],
        declared_len,
        max_uncompressed_bytes,
    )?;
    if raw.len() != declared_len {
        return Err(BlobError::Truncated);
    }
    if (flags & FLAG_CHECKSUM_XXH32) != 0 {
        let actual_crc = xxh32(&raw, 0);
        if actual_crc != expected_crc {
            return Err(BlobError::ChecksumMismatch {
                expected: expected_crc,
                actual: actual_crc,
            });
        }
    }

    let mut cursor = 0usize;
    let read_i32 = |buf: &[u8], pos: &mut usize| -> Result<i32, BlobError> {
        let end = pos.checked_add(4).ok_or(BlobError::Truncated)?;
        let slice = buf.get(*pos..end).ok_or(BlobError::Truncated)?;
        *pos = end;
        Ok(i32::from_be_bytes(slice.try_into().unwrap()))
    };
    let read_u32 = |buf: &[u8], pos: &mut usize| -> Result<u32, BlobError> {
        let end = pos.checked_add(4).ok_or(BlobError::Truncated)?;
        let slice = buf.get(*pos..end).ok_or(BlobError::Truncated)?;
        *pos = end;
        Ok(u32::from_be_bytes(slice.try_into().unwrap()))
    };

    let spawn = [
        read_i32(&raw, &mut cursor)?,
        read_i32(&raw, &mut cursor)?,
        read_i32(&raw, &mut cursor)?,
    ];
    let count = read_u32(&raw, &mut cursor)? as usize;
    let remaining = raw.len().saturating_sub(cursor);
    if count > remaining / MIN_CHUNK_ENTRY_BYTES {
        return Err(BlobError::Truncated);
    }
    let mut chunks = HashMap::with_capacity(count);
    for _ in 0..count {
        let x = read_i32(&raw, &mut cursor)?;
        let z = read_i32(&raw, &mut cursor)?;
        let len = read_u32(&raw, &mut cursor)? as usize;
        let end = cursor.checked_add(len).ok_or(BlobError::Truncated)?;
        let bytes = raw.get(cursor..end).ok_or(BlobError::Truncated)?.to_vec();
        cursor = end;
        chunks.insert((x, z), bytes);
    }
    Ok(Fworld { spawn, chunks })
}

fn compress_payload(compression: BlobCompression, raw: &[u8]) -> Result<Vec<u8>, BlobError> {
    match compression {
        BlobCompression::None => Ok(raw.to_vec()),
        BlobCompression::Lz4 => Ok(lz4_flex::compress_prepend_size(raw)),
        BlobCompression::Zlib => {
            let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
            enc.write_all(raw)?;
            Ok(enc.finish()?)
        }
        BlobCompression::Gzip => {
            let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            enc.write_all(raw)?;
            Ok(enc.finish()?)
        }
    }
}

fn decompress_payload(
    compression: BlobCompression,
    data: &[u8],
    declared_len: usize,
    max_uncompressed_bytes: usize,
) -> Result<Vec<u8>, BlobError> {
    match compression {
        BlobCompression::None => {
            if data.len() > max_uncompressed_bytes {
                return Err(BlobError::PayloadTooLarge {
                    max: max_uncompressed_bytes,
                });
            }
            Ok(data.to_vec())
        }
        BlobCompression::Lz4 => {
            let size_bytes: [u8; 4] = data
                .get(..4)
                .and_then(|s| s.try_into().ok())
                .ok_or(BlobError::Truncated)?;
            let declared = u32::from_le_bytes(size_bytes) as usize;
            if declared > max_uncompressed_bytes {
                return Err(BlobError::PayloadTooLarge {
                    max: max_uncompressed_bytes,
                });
            }
            let out =
                lz4_flex::decompress_size_prepended(data).map_err(|_| BlobError::Truncated)?;
            if out.len() > max_uncompressed_bytes {
                return Err(BlobError::PayloadTooLarge {
                    max: max_uncompressed_bytes,
                });
            }
            Ok(out)
        }
        BlobCompression::Zlib => read_bounded_stream(
            flate2::read::ZlibDecoder::new(data),
            declared_len,
            max_uncompressed_bytes,
        ),
        BlobCompression::Gzip => read_bounded_stream(
            flate2::read::GzDecoder::new(data),
            declared_len,
            max_uncompressed_bytes,
        ),
    }
}

fn read_bounded_stream<R: Read>(
    reader: R,
    declared_len: usize,
    max_bytes: usize,
) -> Result<Vec<u8>, BlobError> {
    let cap = u64::try_from(max_bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut out = Vec::with_capacity(declared_len.min(max_bytes));
    reader.take(cap).read_to_end(&mut out)?;
    if out.len() > max_bytes {
        return Err(BlobError::PayloadTooLarge { max: max_bytes });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_all_compression_modes_single_and_parallel() {
        let mut world = Fworld::new([12, 70, -8]);
        // Exceed PARALLEL_CHUNK_THRESHOLD to exercise multi-core parallel compression & decode.
        for x in 0..6 {
            for z in 0..6 {
                world.chunks.insert((x, z), vec![(x * 7 + z) as u8; 512]);
            }
        }

        for mode in [
            BlobCompression::None,
            BlobCompression::Lz4,
            BlobCompression::Zlib,
            BlobCompression::Gzip,
        ] {
            let encoded = world.encode_to_bytes(mode).unwrap();
            assert_eq!(&encoded[0..4], b"FWRD");
            assert_eq!(u16::from_be_bytes([encoded[4], encoded[5]]), VERSION_V3);
            let decoded = Fworld::decode_from_bytes(&encoded).unwrap();
            assert_eq!(decoded.spawn, world.spawn);
            assert_eq!(decoded.chunks, world.chunks);
        }
    }

    #[test]
    fn cold_store_keeps_chunks_compressed_and_roundtrips_cleanly() {
        let path =
            std::env::temp_dir().join(format!("ferrite_fworld_cold_{}.fworld", std::process::id()));
        let mut cold = FworldColdStore::new([0, 64, 0], BlobCompression::Lz4);
        cold.insert_raw(1, -2, &vec![0x5Au8; 4096]).unwrap();
        // Compressed size in RAM must be far smaller than uncompressed 4 KiB.
        assert!(cold.chunks.get(&(1, -2)).unwrap().stored.len() < 256);
        cold.write_file_atomic(&path).unwrap();

        let reloaded = FworldColdStore::read_file(&path, BlobCompression::Lz4).unwrap();
        assert_eq!(
            reloaded.get_raw(1, -2).unwrap().unwrap(),
            vec![0x5Au8; 4096]
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_bad_magic_corrupt_checksum_and_preserves_independent_clone() {
        assert!(matches!(
            Fworld::decode_from_bytes(b"NOPE1234"),
            Err(BlobError::BadMagic)
        ));
        let mut w1 = Fworld::new([0, 64, 0]);
        w1.chunks.insert((1, 2), vec![9, 9]);
        let mut w2 = w1.clone_independent();
        w2.chunks.insert((1, 2), vec![7, 7]);
        assert_eq!(w1.chunks.get(&(1, 2)).unwrap(), &[9, 9]);
        assert_eq!(w2.chunks.get(&(1, 2)).unwrap(), &[7, 7]);

        // Bit-flip in payload must trip the xxh32 integrity check.
        let mut raw_blob = w1.encode_to_bytes(BlobCompression::None).unwrap();
        let last = raw_blob.len() - 1;
        raw_blob[last] ^= 0xFF;
        assert!(matches!(
            Fworld::decode_from_bytes(&raw_blob),
            Err(BlobError::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn rejects_forged_chunk_count_header() {
        let mut blob = Vec::new();
        write_v3_header(&mut blob, BlobCompression::None, [0, 64, 0], u32::MAX, 128);
        blob.extend_from_slice(&[0u8; 32]);

        assert!(matches!(
            Fworld::decode_from_bytes(&blob),
            Err(BlobError::Truncated)
        ));
    }

    #[test]
    fn rejects_decompression_bombs_beyond_limit() {
        let mut world = Fworld::new([0, 64, 0]);
        world.chunks.insert((0, 0), vec![0xAA; 4096]);

        for mode in [
            BlobCompression::None,
            BlobCompression::Lz4,
            BlobCompression::Zlib,
            BlobCompression::Gzip,
        ] {
            let encoded = world.encode_to_bytes(mode).unwrap();
            assert!(matches!(
                Fworld::decode_from_bytes_with_limit(&encoded, 1024),
                Err(BlobError::PayloadTooLarge { max: 1024 })
            ));
        }

        // Crafted v3 header claiming u32::MAX uncompressed payload fails O(1) before allocation.
        let mut forged = Vec::new();
        write_v3_header(&mut forged, BlobCompression::Lz4, [0, 64, 0], 1, u32::MAX);
        assert!(matches!(
            Fworld::decode_from_bytes(&forged),
            Err(BlobError::PayloadTooLarge {
                max: MAX_FWORLD_UNCOMPRESSED_BYTES
            })
        ));
    }

    #[test]
    fn write_snapshot_to_dir_creates_valid_backups_and_rotates() {
        let mut store = FworldColdStore::new([0, 64, 0], BlobCompression::Lz4);
        store.insert_raw(0, 0, &[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        let dir = std::env::temp_dir().join(format!("ferrite-fworld-snap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let s1 = store.write_snapshot_to_dir(&dir, "overworld", 2).unwrap();
        let s2 = store.write_snapshot_to_dir(&dir, "overworld", 2).unwrap();
        let s3 = store.write_snapshot_to_dir(&dir, "overworld", 2).unwrap();
        assert!(s3.exists());
        let remaining: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(remaining.len(), 2);
        let decoded = Fworld::read_file(&s3).unwrap();
        assert_eq!(
            decoded.chunks.get(&(0, 0)).map(Vec::as_slice),
            Some(&[1, 2, 3, 4, 5, 6, 7, 8][..])
        );
        let _ = (s1, s2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
