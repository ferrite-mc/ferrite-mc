//! Ferrite single-blob compressed world storage (`.fworld` / `FWRD` v2 format).
//!
//! Stores all chunks of a world/dimension and its spawn point in one integrity-checked binary
//! blob loaded wholly into RAM on startup, eliminating per-chunk region file seeks and locks.
//!
//! Format (`FWRD` v2, big-endian):
//! - `0..4`: magic `b"FWRD"`
//! - `4..6`: format version (`2u16`)
//! - `6`: [`BlobCompression`] id (`0=None, 1=Lz4, 2=Zlib, 3=Gzip`)
//! - `7`: header flags (`FLAG_CHECKSUM_XXH32 = 1`)
//! - `8..12`: uncompressed payload length (`u32`, pre-checked before decompression allocation)
//! - `12..16`: `xxh32` checksum of the uncompressed payload (verified on every read)
//! - `16..`: compressed (or raw) payload bytes `[spawn_xyz: 3*i32, chunk_count: u32, chunks...]`

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;

use xxhash_rust::xxh32::xxh32;

const MAGIC: &[u8; 4] = b"FWRD";
const VERSION: u16 = 2;
const HEADER_LEN: usize = 16;
const FLAG_CHECKSUM_XXH32: u8 = 0x01;
const MIN_CHUNK_ENTRY_BYTES: usize = 12;

/// Hard ceiling on uncompressed `.fworld` blob size (512 MiB) to bound decompression bombs.
pub const MAX_FWORLD_UNCOMPRESSED_BYTES: usize = 512 * 1024 * 1024;

/// Compression algorithm applied to the `.fworld` payload.
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

/// A Ferrite single-blob world (`.fworld`) loaded wholly into memory.
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

    /// Encodes the entire world into a single integrity-checked `.fworld` byte buffer.
    pub fn encode_to_bytes(&self, compression: BlobCompression) -> Result<Vec<u8>, BlobError> {
        let mut entries: Vec<(&(i32, i32), &Vec<u8>)> = self.chunks.iter().collect();
        entries.sort_by_key(|&(&(x, z), _)| (z, x));

        let payload_est: usize = 16 + entries.iter().map(|(_, v)| 12 + v.len()).sum::<usize>();
        let mut raw = Vec::with_capacity(payload_est);
        for coord in self.spawn {
            raw.extend_from_slice(&coord.to_be_bytes());
        }
        raw.extend_from_slice(&(entries.len() as u32).to_be_bytes());
        for (&(x, z), bytes) in entries {
            raw.extend_from_slice(&x.to_be_bytes());
            raw.extend_from_slice(&z.to_be_bytes());
            raw.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
            raw.extend_from_slice(bytes);
        }

        let raw_len = u32::try_from(raw.len()).map_err(|_| BlobError::PayloadTooLarge {
            max: MAX_FWORLD_UNCOMPRESSED_BYTES,
        })?;
        let checksum = xxh32(&raw, 0);
        let compressed = compress_payload(compression, &raw)?;

        let mut out = Vec::with_capacity(HEADER_LEN + compressed.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_be_bytes());
        out.push(compression as u8);
        out.push(FLAG_CHECKSUM_XXH32);
        out.extend_from_slice(&raw_len.to_be_bytes());
        out.extend_from_slice(&checksum.to_be_bytes());
        out.extend_from_slice(&compressed);
        Ok(out)
    }

    /// Decodes a `.fworld` blob into memory using [`MAX_FWORLD_UNCOMPRESSED_BYTES`].
    pub fn decode_from_bytes(data: &[u8]) -> Result<Self, BlobError> {
        Self::decode_from_bytes_with_limit(data, MAX_FWORLD_UNCOMPRESSED_BYTES)
    }

    /// Decodes a `.fworld` blob while capping the decompressed payload size and verifying `xxh32`.
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
        if data.len() < HEADER_LEN {
            return Err(BlobError::Truncated);
        }
        let version = u16::from_be_bytes([data[4], data[5]]);
        if version != VERSION {
            return Err(BlobError::UnsupportedVersion(version));
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
            &data[HEADER_LEN..],
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
        Ok(Self { spawn, chunks })
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
    fn roundtrips_all_compression_modes() {
        let mut world = Fworld::new([12, 70, -8]);
        world.chunks.insert((0, 0), vec![1, 2, 3, 4, 5]);
        world.chunks.insert((-3, 7), vec![42; 512]);

        for mode in [
            BlobCompression::None,
            BlobCompression::Lz4,
            BlobCompression::Zlib,
            BlobCompression::Gzip,
        ] {
            let encoded = world.encode_to_bytes(mode).unwrap();
            assert_eq!(&encoded[0..4], b"FWRD");
            let decoded = Fworld::decode_from_bytes(&encoded).unwrap();
            assert_eq!(decoded.spawn, world.spawn);
            assert_eq!(decoded.chunks, world.chunks);
        }
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
        let mut raw = Vec::new();
        raw.extend_from_slice(&0i32.to_be_bytes());
        raw.extend_from_slice(&64i32.to_be_bytes());
        raw.extend_from_slice(&0i32.to_be_bytes());
        raw.extend_from_slice(&u32::MAX.to_be_bytes());

        let mut blob = Vec::new();
        blob.extend_from_slice(MAGIC);
        blob.extend_from_slice(&VERSION.to_be_bytes());
        blob.push(BlobCompression::None as u8);
        blob.push(FLAG_CHECKSUM_XXH32);
        blob.extend_from_slice(&(raw.len() as u32).to_be_bytes());
        blob.extend_from_slice(&xxh32(&raw, 0).to_be_bytes());
        blob.extend_from_slice(&raw);

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

        // Crafted header claiming u32::MAX uncompressed payload must fail before allocation.
        let mut forged = Vec::new();
        forged.extend_from_slice(MAGIC);
        forged.extend_from_slice(&VERSION.to_be_bytes());
        forged.push(BlobCompression::Lz4 as u8);
        forged.push(FLAG_CHECKSUM_XXH32);
        forged.extend_from_slice(&u32::MAX.to_be_bytes());
        forged.extend_from_slice(&0u32.to_be_bytes());
        forged.extend_from_slice(&[0u8; 8]);
        assert!(matches!(
            Fworld::decode_from_bytes(&forged),
            Err(BlobError::PayloadTooLarge {
                max: MAX_FWORLD_UNCOMPRESSED_BYTES
            })
        ));
    }
}
