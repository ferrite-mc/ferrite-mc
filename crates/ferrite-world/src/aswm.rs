//! ASWM / Slime-style single-blob compressed world storage (`FSLM` format).
//!
//! Stores all chunks of a world/dimension and its spawn point in one compressed binary blob
//! loaded wholly into RAM on startup, eliminating per-chunk region file seeks and locks.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;

const MAGIC: &[u8; 4] = b"FSLM";
const VERSION: u16 = 1;
/// Hard ceiling on uncompressed ASWM blob size (512 MiB) to bound decompression bombs.
pub const MAX_ASWM_UNCOMPRESSED_BYTES: usize = 512 * 1024 * 1024;
const MIN_CHUNK_ENTRY_BYTES: usize = 12;

/// Compression algorithm applied to the ASWM world payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum AswmCompression {
    None = 0,
    #[default]
    Lz4 = 1,
    Zlib = 2,
    Gzip = 3,
}

impl AswmCompression {
    pub fn from_u8(v: u8) -> Result<Self, AswmError> {
        match v {
            0 => Ok(Self::None),
            1 => Ok(Self::Lz4),
            2 => Ok(Self::Zlib),
            3 => Ok(Self::Gzip),
            other => Err(AswmError::UnsupportedCompression(other)),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AswmError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("invalid ASWM magic header")]
    BadMagic,
    #[error("unsupported ASWM format version: {0}")]
    UnsupportedVersion(u16),
    #[error("unsupported ASWM compression id: {0}")]
    UnsupportedCompression(u8),
    #[error("truncated or corrupt ASWM blob")]
    Truncated,
    #[error("ASWM payload exceeds maximum uncompressed size ({max} bytes)")]
    PayloadTooLarge { max: usize },
}

/// An ASWM single-blob world loaded wholly into memory.
#[derive(Debug, Clone, Default)]
pub struct AswmWorld {
    pub spawn: [i32; 3],
    pub chunks: HashMap<(i32, i32), Vec<u8>>,
}

impl AswmWorld {
    pub fn new(spawn: [i32; 3]) -> Self {
        Self {
            spawn,
            chunks: HashMap::new(),
        }
    }

    /// Returns an independent in-memory clone of the world (e.g. for minigame/instance cloning).
    pub fn clone_independent(&self) -> Self {
        self.clone()
    }

    /// Encodes the entire world into a single compressed ASWM blob.
    pub fn encode_to_bytes(&self, compression: AswmCompression) -> Result<Vec<u8>, AswmError> {
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

        let compressed = compress_payload(compression, &raw)?;
        let mut out = Vec::with_capacity(7 + compressed.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_be_bytes());
        out.push(compression as u8);
        out.extend_from_slice(&compressed);
        Ok(out)
    }

    /// Decodes a single compressed ASWM blob into memory using [`MAX_ASWM_UNCOMPRESSED_BYTES`].
    pub fn decode_from_bytes(data: &[u8]) -> Result<Self, AswmError> {
        Self::decode_from_bytes_with_limit(data, MAX_ASWM_UNCOMPRESSED_BYTES)
    }

    /// Decodes a single compressed ASWM blob while capping the decompressed payload size.
    pub fn decode_from_bytes_with_limit(
        data: &[u8],
        max_uncompressed_bytes: usize,
    ) -> Result<Self, AswmError> {
        if data.len() < 7 {
            return Err(AswmError::Truncated);
        }
        if &data[0..4] != MAGIC {
            return Err(AswmError::BadMagic);
        }
        let version = u16::from_be_bytes([data[4], data[5]]);
        if version != VERSION {
            return Err(AswmError::UnsupportedVersion(version));
        }
        let compression = AswmCompression::from_u8(data[6])?;
        let raw = decompress_payload(compression, &data[7..], max_uncompressed_bytes)?;

        let mut cursor = 0usize;
        let read_i32 = |buf: &[u8], pos: &mut usize| -> Result<i32, AswmError> {
            let end = pos.checked_add(4).ok_or(AswmError::Truncated)?;
            let slice = buf.get(*pos..end).ok_or(AswmError::Truncated)?;
            *pos = end;
            Ok(i32::from_be_bytes(slice.try_into().unwrap()))
        };
        let read_u32 = |buf: &[u8], pos: &mut usize| -> Result<u32, AswmError> {
            let end = pos.checked_add(4).ok_or(AswmError::Truncated)?;
            let slice = buf.get(*pos..end).ok_or(AswmError::Truncated)?;
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
            return Err(AswmError::Truncated);
        }
        let mut chunks = HashMap::with_capacity(count);
        for _ in 0..count {
            let x = read_i32(&raw, &mut cursor)?;
            let z = read_i32(&raw, &mut cursor)?;
            let len = read_u32(&raw, &mut cursor)? as usize;
            let end = cursor.checked_add(len).ok_or(AswmError::Truncated)?;
            let bytes = raw.get(cursor..end).ok_or(AswmError::Truncated)?.to_vec();
            cursor = end;
            chunks.insert((x, z), bytes);
        }
        Ok(Self { spawn, chunks })
    }

    pub fn read_file(path: impl AsRef<Path>) -> Result<Self, AswmError> {
        let bytes = std::fs::read(path)?;
        Self::decode_from_bytes(&bytes)
    }

    pub fn write_file_atomic(
        &self,
        path: impl AsRef<Path>,
        compression: AswmCompression,
    ) -> Result<(), AswmError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let encoded = self.encode_to_bytes(compression)?;
        let tmp = path.with_extension("aswm.tmp");
        std::fs::write(&tmp, encoded)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

fn compress_payload(compression: AswmCompression, raw: &[u8]) -> Result<Vec<u8>, AswmError> {
    match compression {
        AswmCompression::None => Ok(raw.to_vec()),
        AswmCompression::Lz4 => Ok(lz4_flex::compress_prepend_size(raw)),
        AswmCompression::Zlib => {
            let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
            enc.write_all(raw)?;
            Ok(enc.finish()?)
        }
        AswmCompression::Gzip => {
            let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            enc.write_all(raw)?;
            Ok(enc.finish()?)
        }
    }
}

fn decompress_payload(
    compression: AswmCompression,
    data: &[u8],
    max_uncompressed_bytes: usize,
) -> Result<Vec<u8>, AswmError> {
    match compression {
        AswmCompression::None => {
            if data.len() > max_uncompressed_bytes {
                return Err(AswmError::PayloadTooLarge {
                    max: max_uncompressed_bytes,
                });
            }
            Ok(data.to_vec())
        }
        AswmCompression::Lz4 => {
            let size_bytes: [u8; 4] = data
                .get(..4)
                .and_then(|s| s.try_into().ok())
                .ok_or(AswmError::Truncated)?;
            let declared = u32::from_le_bytes(size_bytes) as usize;
            if declared > max_uncompressed_bytes {
                return Err(AswmError::PayloadTooLarge {
                    max: max_uncompressed_bytes,
                });
            }
            let out =
                lz4_flex::decompress_size_prepended(data).map_err(|_| AswmError::Truncated)?;
            if out.len() > max_uncompressed_bytes {
                return Err(AswmError::PayloadTooLarge {
                    max: max_uncompressed_bytes,
                });
            }
            Ok(out)
        }
        AswmCompression::Zlib => {
            read_bounded_stream(flate2::read::ZlibDecoder::new(data), max_uncompressed_bytes)
        }
        AswmCompression::Gzip => {
            read_bounded_stream(flate2::read::GzDecoder::new(data), max_uncompressed_bytes)
        }
    }
}

fn read_bounded_stream<R: Read>(reader: R, max_bytes: usize) -> Result<Vec<u8>, AswmError> {
    let cap = u64::try_from(max_bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut out = Vec::new();
    reader.take(cap).read_to_end(&mut out)?;
    if out.len() > max_bytes {
        return Err(AswmError::PayloadTooLarge { max: max_bytes });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_all_compression_modes() {
        let mut world = AswmWorld::new([12, 70, -8]);
        world.chunks.insert((0, 0), vec![1, 2, 3, 4, 5]);
        world.chunks.insert((-3, 7), vec![42; 512]);

        for mode in [
            AswmCompression::None,
            AswmCompression::Lz4,
            AswmCompression::Zlib,
            AswmCompression::Gzip,
        ] {
            let encoded = world.encode_to_bytes(mode).unwrap();
            let decoded = AswmWorld::decode_from_bytes(&encoded).unwrap();
            assert_eq!(decoded.spawn, world.spawn);
            assert_eq!(decoded.chunks, world.chunks);
        }
    }

    #[test]
    fn rejects_bad_magic_and_preserves_independent_clone() {
        assert!(matches!(
            AswmWorld::decode_from_bytes(b"NOPE1234"),
            Err(AswmError::BadMagic)
        ));
        let mut w1 = AswmWorld::new([0, 64, 0]);
        w1.chunks.insert((1, 2), vec![9, 9]);
        let mut w2 = w1.clone_independent();
        w2.chunks.insert((1, 2), vec![7, 7]);
        assert_eq!(w1.chunks.get(&(1, 2)).unwrap(), &[9, 9]);
        assert_eq!(w2.chunks.get(&(1, 2)).unwrap(), &[7, 7]);
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
        blob.push(AswmCompression::None as u8);
        blob.extend_from_slice(&raw);

        assert!(matches!(
            AswmWorld::decode_from_bytes(&blob),
            Err(AswmError::Truncated)
        ));
    }

    #[test]
    fn rejects_decompression_bombs_beyond_limit() {
        let mut world = AswmWorld::new([0, 64, 0]);
        world.chunks.insert((0, 0), vec![0xAA; 4096]);

        for mode in [
            AswmCompression::None,
            AswmCompression::Lz4,
            AswmCompression::Zlib,
            AswmCompression::Gzip,
        ] {
            let encoded = world.encode_to_bytes(mode).unwrap();
            assert!(matches!(
                AswmWorld::decode_from_bytes_with_limit(&encoded, 1024),
                Err(AswmError::PayloadTooLarge { max: 1024 })
            ));
        }

        // Crafted LZ4 header claiming 2 GiB uncompressed payload must be rejected before allocation.
        let mut forged_lz4 = Vec::new();
        forged_lz4.extend_from_slice(MAGIC);
        forged_lz4.extend_from_slice(&VERSION.to_be_bytes());
        forged_lz4.push(AswmCompression::Lz4 as u8);
        forged_lz4.extend_from_slice(&u32::MAX.to_le_bytes());
        forged_lz4.extend_from_slice(&[0u8; 8]);
        assert!(matches!(
            AswmWorld::decode_from_bytes(&forged_lz4),
            Err(AswmError::PayloadTooLarge {
                max: MAX_ASWM_UNCOMPRESSED_BYTES
            })
        ));
    }
}
