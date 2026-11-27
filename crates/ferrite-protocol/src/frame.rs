//! Packet framing: `VarInt length | [VarInt data_length] | body`, with optional zlib compression.
//! Encryption is a byte-stream transform and lives in the network layer below framing.
//!
//! Compression is zlib through the crate's compression backend: libdeflate in
//! the default build, flate2 with `--no-default-features`. The compressor and
//! decompressor objects are created once per `FrameEncoder`/`FrameDecoder` and
//! reused for every packet; the decoder allocates exactly the declared
//! uncompressed size per frame, bounded by [`MAX_UNCOMPRESSED_LEN`].

use std::cell::RefCell;

use crate::codec::{Decode, DecodeError, Encode, VarInt};
use crate::compress;

thread_local! {
    static TLS_ENCODER: RefCell<compress::Encoder> = RefCell::new(compress::Encoder::default());
}

/// Vanilla rejects frames larger than 2 MiB (2^21).
pub const MAX_FRAME_LEN: usize = 1 << 21;
/// Vanilla rejects decompressed bodies larger than 8 MiB (2^23).
pub const MAX_UNCOMPRESSED_LEN: usize = 1 << 23;

/// DEFLATE expands its input by at most 1032:1 (a 258-byte match can be coded in two
/// bits). A frame declaring more than this for its compressed length is not a packet.
const ZLIB_MAX_EXPANSION: usize = 1032;
/// zlib header/adler overhead and block headers, so small frames are not judged too strictly.
const ZLIB_BOUND_SLACK: usize = 64;
/// Decompression scratch up to this size is kept between frames. A larger buffer is released
/// as soon as a smaller frame is decoded, so one oversized packet cannot pin memory.
const RETAINED_SCRATCH_LEN: usize = 256 * 1024;

#[derive(Clone, Default)]
pub struct FrameEncoder {
    /// `None` = compression disabled, `Some(t)` = compress bodies of at least `t` bytes.
    threshold: Option<usize>,
    /// When true, emit an uncompressed frame (`data_length = 0`) if zlib adds bytes instead of
    /// shrinking the body. Saves egress bandwidth and client CPU on incompressible payloads.
    skip_incompressible: bool,
    level: u32,
}

impl FrameEncoder {
    /// Creates a [`FrameEncoder`] pre-configured with compression threshold, zlib level, and skip policy.
    pub fn new_compressed(threshold: Option<usize>, level: u32, skip_incompressible: bool) -> Self {
        let mut enc = Self::default();
        enc.set_compression(threshold, level);
        enc.set_skip_incompressible(skip_incompressible);
        enc
    }

    pub fn set_compression(&mut self, threshold: Option<usize>, level: u32) {
        self.threshold = threshold;
        self.level = level;
    }

    pub fn set_skip_incompressible(&mut self, skip: bool) {
        self.skip_incompressible = skip;
    }

    /// Returns `(threshold, level, skip_incompressible)` for matching pre-compressed shared frames.
    pub fn compression_config(&self) -> (Option<usize>, u32, bool) {
        (self.threshold, self.level, self.skip_incompressible)
    }

    /// Encode and compress a packet body into a newly allocated wire frame buffer.
    pub fn encode_to_vec(&self, body: &[u8]) -> Vec<u8> {
        let Some(threshold) = self.threshold else {
            let vi = VarInt(body.len() as i32);
            let mut out = Vec::with_capacity(vi.len() + body.len());
            vi.encode(&mut out);
            out.extend_from_slice(body);
            return out;
        };
        if body.is_empty() || body.len() < threshold {
            let vi = VarInt(body.len() as i32 + 1);
            let mut out = Vec::with_capacity(vi.len() + 1 + body.len());
            vi.encode(&mut out);
            out.push(0);
            out.extend_from_slice(body);
            return out;
        }
        TLS_ENCODER.with(|cell| {
            let mut guard = cell.borrow_mut();
            guard.set_level(self.level);
            let compressed = guard.compress(body);
            let data_len = VarInt(body.len() as i32);
            let compressed_frame_len = data_len.len() + compressed.len();
            if self.skip_incompressible && compressed_frame_len > body.len() {
                let vi = VarInt(body.len() as i32 + 1);
                let mut out = Vec::with_capacity(vi.len() + 1 + body.len());
                vi.encode(&mut out);
                out.push(0);
                out.extend_from_slice(body);
                return out;
            }
            let frame_vi = VarInt(compressed_frame_len as i32);
            let mut out = Vec::with_capacity(frame_vi.len() + compressed_frame_len);
            frame_vi.encode(&mut out);
            data_len.encode(&mut out);
            out.extend_from_slice(compressed);
            out
        })
    }

    /// Encode and compress a packet body directly into a reference-counted slice without
    /// over-allocating `body.len()` capacity on compressed frames.
    pub fn encode_to_arc(&self, body: &[u8]) -> std::sync::Arc<[u8]> {
        std::sync::Arc::from(self.encode_to_vec(body))
    }

    /// Frame an already-encoded packet body (ID + fields), appending to `out`.
    pub fn encode(&self, body: &[u8], out: &mut Vec<u8>) {
        let Some(threshold) = self.threshold else {
            let vi = VarInt(body.len() as i32);
            out.reserve(vi.len() + body.len());
            vi.encode(out);
            out.extend_from_slice(body);
            return;
        };
        // `data_length == 0` is the wire marker for an uncompressed body, so an
        // empty body takes that path even when the threshold is zero.
        if body.is_empty() || body.len() < threshold {
            let vi = VarInt(body.len() as i32 + 1);
            out.reserve(vi.len() + 1 + body.len());
            vi.encode(out);
            out.push(0); // data_length = 0: uncompressed
            out.extend_from_slice(body);
            return;
        }
        TLS_ENCODER.with(|cell| {
            let mut guard = cell.borrow_mut();
            guard.set_level(self.level);
            let compressed = guard.compress(body);
            let data_len = VarInt(body.len() as i32);
            let compressed_frame_len = data_len.len() + compressed.len();
            if self.skip_incompressible && compressed_frame_len > body.len() {
                let vi = VarInt(body.len() as i32 + 1);
                out.reserve(vi.len() + 1 + body.len());
                vi.encode(out);
                out.push(0);
                out.extend_from_slice(body);
                return;
            }
            let frame_vi = VarInt(compressed_frame_len as i32);
            out.reserve(frame_vi.len() + compressed_frame_len);
            frame_vi.encode(out);
            data_len.encode(out);
            out.extend_from_slice(compressed);
        });
    }
}

#[derive(Default)]
pub struct FrameDecoder {
    buf: Vec<u8>,
    cursor: usize,
    threshold: Option<usize>,
    decompressor: compress::Decoder,
    scratch: Vec<u8>,
}

impl FrameDecoder {
    pub fn set_compression(&mut self, threshold: Option<usize>) {
        self.threshold = threshold;
    }

    /// Prepares the internal buffer for an async socket read directly into spare capacity,
    /// avoiding an intermediate stack buffer and extra copy.
    pub fn read_buf_mut(&mut self) -> &mut Vec<u8> {
        self.read_buf_mut_with_spare(4096)
    }

    /// Prepares the internal buffer ensuring at least `min_spare` bytes of spare capacity.
    pub fn read_buf_mut_with_spare(&mut self, min_spare: usize) -> &mut Vec<u8> {
        if self.cursor == self.buf.len() {
            self.buf.clear();
            self.cursor = 0;
        } else if self.cursor >= 4096 {
            let remaining = self.buf.len() - self.cursor;
            self.buf.copy_within(self.cursor.., 0);
            self.buf.truncate(remaining);
            self.cursor = 0;
        }
        if self.buf.capacity() - self.buf.len() < min_spare {
            self.buf.reserve(min_spare);
        }
        &mut self.buf
    }

    /// Pops the next complete uncompressed frame's byte range `(start, end)` inside
    /// [`Self::raw_buf`] without allocating a `Vec<u8>`. Returns `None` if incomplete or if
    /// compression is enabled (use [`Self::next_frame_ref`] instead).
    pub fn next_uncompressed_range(&mut self) -> Result<Option<(usize, usize)>, DecodeError> {
        if self.threshold.is_some() {
            return Ok(None);
        }
        let unread = &self.buf[self.cursor..];
        let Some((len, header)) = VarInt::decode_partial(unread)? else {
            return Ok(None);
        };
        let len =
            usize::try_from(len).map_err(|_| DecodeError::Invalid("negative frame length"))?;
        if len > MAX_FRAME_LEN {
            return Err(DecodeError::Invalid("frame too large"));
        }
        if unread.len() < header + len {
            return Ok(None);
        }
        let abs_start = self.cursor + header;
        let abs_end = self.cursor + header + len;
        self.cursor += header + len;
        Ok(Some((abs_start, abs_end)))
    }

    /// Returns the underlying staging buffer slice for ranges returned by
    /// [`Self::next_uncompressed_range`].
    pub fn raw_buf(&self) -> &[u8] {
        &self.buf
    }

    /// Capacity of the decompression scratch buffer (test-only observation).
    #[cfg(test)]
    fn scratch_capacity(&self) -> usize {
        self.scratch.capacity()
    }

    /// Feed raw (already decrypted) bytes from the socket.
    pub fn feed(&mut self, data: &[u8]) {
        if self.cursor == self.buf.len() {
            self.buf.clear();
            self.cursor = 0;
        } else if self.cursor >= 4096 {
            let remaining = self.buf.len() - self.cursor;
            self.buf.copy_within(self.cursor.., 0);
            self.buf.truncate(remaining);
            self.cursor = 0;
        }
        self.buf.extend_from_slice(data);
    }

    fn consume(&mut self, end: usize) {
        self.cursor += end;
        if self.cursor >= self.buf.len() {
            self.buf.clear();
            self.cursor = 0;
        }
    }

    /// Pop the next complete packet body (ID + fields) as a borrowed slice without allocating
    /// a new `Vec<u8>` per frame.
    pub fn next_frame_ref(&mut self) -> Result<Option<&[u8]>, DecodeError> {
        let unread = &self.buf[self.cursor..];
        let Some((len, header)) = VarInt::decode_partial(unread)? else {
            return Ok(None);
        };
        let len =
            usize::try_from(len).map_err(|_| DecodeError::Invalid("negative frame length"))?;
        if len > MAX_FRAME_LEN {
            return Err(DecodeError::Invalid("frame too large"));
        }
        if unread.len() < header + len {
            return Ok(None);
        }
        let end = header + len;
        let abs_start = self.cursor + header;
        let abs_end = self.cursor + end;
        if self.threshold.is_none() {
            self.cursor += end;
            return Ok(Some(&self.buf[abs_start..abs_end]));
        }

        // Compressed frame: `[data_length][zlib payload]` inside the frame body.
        let mut body = &self.buf[abs_start..abs_end];
        let before_len = body.len();
        let data_len = VarInt::decode(&mut body)?.0;
        let varint_bytes = before_len - body.len();
        if data_len == 0 {
            self.cursor += end;
            return Ok(Some(&self.buf[abs_start + varint_bytes..abs_end]));
        }
        let data_len =
            usize::try_from(data_len).map_err(|_| DecodeError::Invalid("negative data length"))?;
        if data_len > MAX_UNCOMPRESSED_LEN {
            return Err(DecodeError::Invalid("decompressed packet too large"));
        }
        // Reject an impossible declared size before allocating it: a six-byte frame
        // must not be able to reserve megabytes of this connection's memory.
        if data_len > body.len().saturating_mul(ZLIB_MAX_EXPANSION) + ZLIB_BOUND_SLACK {
            return Err(DecodeError::Invalid(
                "decompressed length beyond zlib bound",
            ));
        }
        // Grow only: shrinking here would zero the buffer again on the next large frame.
        // The borrow of the previous frame's slice has ended by now, so an oversized
        // buffer can be released once normal-sized frames arrive again.
        if data_len <= RETAINED_SCRATCH_LEN && self.scratch.len() > RETAINED_SCRATCH_LEN {
            self.scratch.truncate(RETAINED_SCRATCH_LEN);
            self.scratch.shrink_to(RETAINED_SCRATCH_LEN);
        }
        if self.scratch.len() < data_len {
            self.scratch.resize(data_len, 0);
        }
        let written = self
            .decompressor
            .decompress(body, &mut self.scratch[..data_len])
            .map_err(|error| match error {
                compress::BackendError::BadData => DecodeError::Invalid("bad zlib data"),
                compress::BackendError::InsufficientSpace => {
                    DecodeError::Invalid("decompressed length mismatch")
                }
            })?;
        if written != data_len {
            return Err(DecodeError::Invalid("decompressed length mismatch"));
        }
        self.consume(end);
        Ok(Some(&self.scratch[..data_len]))
    }

    /// Pop the next complete packet body (ID + fields), or `None` if more bytes are needed.
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>, DecodeError> {
        let Some(slice) = self.next_frame_ref()? else {
            return Ok(None);
        };
        let out = slice.to_vec();
        if self.cursor >= self.buf.len() {
            self.buf.clear();
            self.cursor = 0;
        }
        Ok(Some(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use flate2::Compression;
    use flate2::read::ZlibDecoder;
    use flate2::write::ZlibEncoder;
    use std::io::{Read, Write};

    /// Deterministic pseudo-random bytes: no structure for zlib to exploit.
    fn incompressible(len: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(len);
        let mut state = 0x1234_5678u32;
        for _ in 0..len {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            out.push((state >> 24) as u8);
        }
        out
    }

    /// Chunk-packet-shaped bytes: long runs, palettes and per-block noise.
    fn chunk_like(len: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(len);
        let mut state = 0x9e37_79b9u32;
        for i in 0..len {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            out.push(if i % 7 == 0 {
                (state >> 24) as u8
            } else {
                (i % 251) as u8
            });
        }
        out
    }

    /// The payload classes framing has to handle end to end.
    fn payloads() -> Vec<(&'static str, Vec<u8>)> {
        vec![
            ("empty", Vec::new()),
            ("tiny", vec![0x07]),
            ("small", b"hello, ferrite".to_vec()),
            ("chunk-sized", chunk_like(256 * 1024)),
            ("incompressible", incompressible(64 * 1024)),
        ]
    }

    /// Wrap already-compressed bytes in `[frame length][data length][payload]`.
    fn wire_frame(declared: i32, compressed: &[u8]) -> Vec<u8> {
        let mut frame = Vec::new();
        VarInt(declared).encode(&mut frame);
        frame.extend_from_slice(compressed);
        let mut wire = Vec::new();
        VarInt(frame.len() as i32).encode(&mut wire);
        wire.extend_from_slice(&frame);
        wire
    }

    fn roundtrip(threshold: Option<usize>, body: &[u8], label: &str) {
        let mut enc = FrameEncoder::default();
        enc.set_compression(threshold, 6);
        let mut dec = FrameDecoder::default();
        dec.set_compression(threshold);
        let mut wire = Vec::new();
        enc.encode(body, &mut wire);
        enc.encode(body, &mut wire);
        // Feed byte-by-byte to exercise partial frames.
        let mut got = Vec::new();
        for b in wire {
            dec.feed(&[b]);
            while let Some(f) = dec.next_frame().unwrap() {
                got.push(f);
            }
        }
        assert_eq!(got.len(), 2, "{label}: two frames");
        assert_eq!(got[0], body, "{label}: first frame");
        assert_eq!(got[1], body, "{label}: second frame");
    }

    #[test]
    fn frames_roundtrip() {
        for (label, body) in payloads() {
            roundtrip(None, &body, label);
            roundtrip(Some(0), &body, label);
            roundtrip(Some(256), &body, label);
        }
    }

    #[test]
    fn empty_body_compresses_as_the_uncompressed_marker() {
        // data_length == 0 is the wire marker for "not compressed", so an empty
        // body must never be followed by a zlib stream, even at threshold zero.
        let mut enc = FrameEncoder::default();
        enc.set_compression(Some(0), 6);
        let mut wire = Vec::new();
        enc.encode(b"", &mut wire);
        assert_eq!(wire, [0x01, 0x00]);

        let mut dec = FrameDecoder::default();
        dec.set_compression(Some(0));
        dec.feed(&wire);
        assert_eq!(dec.next_frame().unwrap().unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn frame_len_above_max_rejected() {
        let mut dec = FrameDecoder::default();
        let mut wire = Vec::new();
        VarInt(MAX_FRAME_LEN as i32 + 1).encode(&mut wire);
        dec.feed(&wire);
        assert!(matches!(
            dec.next_frame(),
            Err(DecodeError::Invalid("frame too large"))
        ));

        // The limit itself is accepted; only the body is still missing.
        let mut dec = FrameDecoder::default();
        let mut wire = Vec::new();
        VarInt(MAX_FRAME_LEN as i32).encode(&mut wire);
        dec.feed(&wire);
        assert!(dec.next_frame().unwrap().is_none());
    }

    #[test]
    fn compressed_data_len_above_max_rejected() {
        let mut frame = Vec::new();
        VarInt(MAX_UNCOMPRESSED_LEN as i32 + 1).encode(&mut frame);
        frame.extend_from_slice(&[0; 8]);

        let mut dec = FrameDecoder::default();
        dec.set_compression(Some(0));
        let mut wire = Vec::new();
        VarInt(frame.len() as i32).encode(&mut wire);
        wire.extend_from_slice(&frame);
        dec.feed(&wire);
        assert!(matches!(
            dec.next_frame(),
            Err(DecodeError::Invalid("decompressed packet too large"))
        ));
    }

    #[test]
    fn corrupt_zlib_payload_errors() {
        let mut frame = Vec::new();
        VarInt(64).encode(&mut frame);
        // Invalid zlib header (CM nibble is 14, not 8).
        frame.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef, 0x00, 0x11, 0x22, 0x33]);

        let mut dec = FrameDecoder::default();
        dec.set_compression(Some(0));
        let mut wire = Vec::new();
        VarInt(frame.len() as i32).encode(&mut wire);
        wire.extend_from_slice(&frame);
        dec.feed(&wire);
        assert!(matches!(dec.next_frame(), Err(DecodeError::Invalid(_))));
    }

    #[test]
    fn truncated_zlib_payload_errors() {
        let body = vec![7u8; 4096];
        let mut z = ZlibEncoder::new(Vec::new(), Compression::new(6));
        z.write_all(&body).unwrap();
        let mut compressed = z.finish().unwrap();
        compressed.truncate(compressed.len() / 2);

        let mut dec = FrameDecoder::default();
        dec.set_compression(Some(0));
        dec.feed(&wire_frame(body.len() as i32, &compressed));
        assert!(matches!(dec.next_frame(), Err(DecodeError::Invalid(_))));
    }

    /// Split a wire buffer into `(declared frame len, frame body)`.
    fn split_frame(wire: &[u8]) -> (usize, &[u8]) {
        let mut s = wire;
        let len = VarInt::decode(&mut s).unwrap().0 as usize;
        assert_eq!(s.len(), len);
        (len, s)
    }

    /// Byte-level check one way: frames our encoder writes are valid zlib for
    /// flate2 and carry the payload unchanged.
    #[test]
    fn backend_frames_decode_with_flate2() {
        for (label, body) in payloads() {
            let mut enc = FrameEncoder::default();
            enc.set_compression(Some(0), 9);
            let mut wire = Vec::new();
            enc.encode(&body, &mut wire);

            let (_, frame) = split_frame(&wire);
            let mut s = frame;
            let data_len = VarInt::decode(&mut s).unwrap().0;
            if body.is_empty() {
                assert_eq!(data_len, 0, "{label}: empty body uses the marker");
                assert!(s.is_empty(), "{label}: marker carries no payload");
                continue;
            }
            assert_eq!(data_len as usize, body.len(), "{label}: declared length");
            let mut decoded = Vec::new();
            ZlibDecoder::new(s)
                .read_to_end(&mut decoded)
                .unwrap_or_else(|error| panic!("{label}: flate2 rejected the stream: {error}"));
            assert_eq!(decoded, body, "{label}: flate2 output");
        }
    }

    /// Byte-level check the other way: zlib streams written by flate2 decode
    /// through the active backend.
    #[test]
    fn flate2_frames_decode_with_backend() {
        for (label, body) in payloads() {
            if body.is_empty() {
                // Empty bodies travel as the uncompressed marker; data_length 0
                // cannot describe a zlib stream.
                continue;
            }
            let mut z = ZlibEncoder::new(Vec::new(), Compression::new(9));
            z.write_all(&body).unwrap();
            let compressed = z.finish().unwrap();

            let mut dec = FrameDecoder::default();
            dec.set_compression(Some(0));
            dec.feed(&wire_frame(body.len() as i32, &compressed));
            assert_eq!(dec.next_frame().unwrap().unwrap(), body, "{label}");
        }
    }

    #[test]
    fn incompressible_payload_roundtrips() {
        let body = incompressible(64 * 1024);
        let mut enc = FrameEncoder::default();
        enc.set_compression(Some(0), 6);
        let mut wire = Vec::new();
        enc.encode(&body, &mut wire);

        let (frame_len, frame) = split_frame(&wire);
        let mut s = frame;
        let data_len = VarInt::decode(&mut s).unwrap().0 as usize;
        assert_eq!(data_len, body.len());
        assert_eq!(frame_len, VarInt(data_len as i32).len() + s.len());
        assert!(
            s.len() >= body.len(),
            "random bytes should not compress: {} < {}",
            s.len(),
            body.len()
        );

        let mut dec = FrameDecoder::default();
        dec.set_compression(Some(0));
        dec.feed(&wire);
        assert_eq!(dec.next_frame().unwrap().unwrap(), body);
    }

    #[test]
    fn compressed_declared_length_mismatch_rejected() {
        let body = b"hello, ferrite";
        let mut z = ZlibEncoder::new(Vec::new(), Compression::new(6));
        z.write_all(body).unwrap();
        let compressed = z.finish().unwrap();

        for declared in [body.len() as i32 - 1, body.len() as i32 + 1] {
            let mut frame = Vec::new();
            VarInt(declared).encode(&mut frame);
            frame.extend_from_slice(&compressed);
            let mut wire = Vec::new();
            VarInt(frame.len() as i32).encode(&mut wire);
            wire.extend_from_slice(&frame);

            let mut dec = FrameDecoder::default();
            dec.set_compression(Some(0));
            dec.feed(&wire);
            assert!(matches!(
                dec.next_frame(),
                Err(DecodeError::Invalid("decompressed length mismatch"))
            ));
        }
    }

    #[test]
    fn rejects_varint_overflow_oversized_frames_and_compression_bombs() {
        // 6 continuation bytes = VarInt overflow.
        let mut dec = FrameDecoder::default();
        dec.feed(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x01]);
        assert!(matches!(dec.next_frame(), Err(DecodeError::VarIntTooLong)));

        // Negative frame length.
        let mut dec = FrameDecoder::default();
        let mut neg = Vec::new();
        VarInt(-1).encode(&mut neg);
        dec.feed(&neg);
        assert!(matches!(
            dec.next_frame(),
            Err(DecodeError::Invalid("negative frame length"))
        ));

        // Frame > MAX_FRAME_LEN.
        let mut dec = FrameDecoder::default();
        let mut big = Vec::new();
        VarInt((MAX_FRAME_LEN as i32) + 1).encode(&mut big);
        dec.feed(&big);
        assert!(matches!(
            dec.next_frame(),
            Err(DecodeError::Invalid("frame too large"))
        ));

        // Compression bomb: declared data_len > MAX_UNCOMPRESSED_LEN.
        let mut dec = FrameDecoder::default();
        dec.set_compression(Some(256));
        let mut body = Vec::new();
        VarInt((MAX_UNCOMPRESSED_LEN as i32) + 1).encode(&mut body);
        body.push(0x78); // dummy zlib byte
        let mut wire = Vec::new();
        VarInt(body.len() as i32).encode(&mut wire);
        wire.extend_from_slice(&body);
        dec.feed(&wire);
        assert!(matches!(
            dec.next_frame(),
            Err(DecodeError::Invalid("decompressed packet too large"))
        ));

        // Truncated packet returns Ok(None) cleanly.
        let mut dec = FrameDecoder::default();
        dec.feed(&[10, 1, 2, 3]);
        assert_eq!(dec.next_frame().unwrap(), None);
    }

    #[test]
    fn skip_incompressible_falls_back_to_uncompressed_marker() {
        let body = incompressible(16 * 1024);
        let mut enc = FrameEncoder::default();
        enc.set_compression(Some(0), 6);
        enc.set_skip_incompressible(true);

        let mut wire = Vec::new();
        enc.encode(&body, &mut wire);

        let (frame_len, frame) = split_frame(&wire);
        let mut s = frame;
        let data_len = VarInt::decode(&mut s).unwrap().0;
        assert_eq!(data_len, 0, "expected uncompressed marker data_length = 0");
        assert_eq!(frame_len, 1 + body.len());

        let mut dec = FrameDecoder::default();
        dec.set_compression(Some(0));
        dec.feed(&wire);
        assert_eq!(dec.next_frame().unwrap().unwrap(), body);
    }

    #[test]
    fn compressed_bomb_length_is_rejected_before_allocating() {
        // A 12-byte frame declaring 8 MiB - 1 bytes: no valid zlib stream can
        // expand that far, so the decoder must reject it instead of reserving
        // the declared size first.
        let mut body = Vec::new();
        VarInt(MAX_UNCOMPRESSED_LEN as i32 - 1).encode(&mut body);
        body.extend_from_slice(&[0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let mut wire = Vec::new();
        VarInt(body.len() as i32).encode(&mut wire);
        wire.extend_from_slice(&body);

        let mut dec = FrameDecoder::default();
        dec.set_compression(Some(0));
        dec.feed(&wire);
        assert!(matches!(
            dec.next_frame(),
            Err(DecodeError::Invalid(
                "decompressed length beyond zlib bound"
            ))
        ));
        assert!(
            dec.scratch_capacity() < 64 * 1024,
            "bomb declared 8 MiB and the decoder allocated {} bytes",
            dec.scratch_capacity()
        );
    }

    #[test]
    fn highly_compressible_body_within_the_deflate_ratio_is_accepted() {
        // The expansion bound must not reject legitimate runs of identical bytes.
        let body = vec![0u8; 1 << 20];
        let mut enc = FrameEncoder::default();
        enc.set_compression(Some(0), 6);
        let mut wire = Vec::new();
        enc.encode(&body, &mut wire);

        let mut dec = FrameDecoder::default();
        dec.set_compression(Some(0));
        dec.feed(&wire);
        assert_eq!(dec.next_frame().unwrap().unwrap(), body);
    }

    #[test]
    fn decoder_reuses_scratch_across_frame_sizes() {
        let bodies = [
            chunk_like(64 * 1024),
            vec![7u8; 4096],
            chunk_like(64 * 1024),
        ];
        let mut enc = FrameEncoder::default();
        enc.set_compression(Some(64), 6);
        let mut dec = FrameDecoder::default();
        dec.set_compression(Some(64));

        let mut settled = 0;
        for (i, body) in bodies.iter().enumerate() {
            let mut wire = Vec::new();
            enc.encode(body, &mut wire);
            dec.feed(&wire);
            assert_eq!(dec.next_frame().unwrap().unwrap(), *body);
            if i == 0 {
                settled = dec.scratch_capacity();
                assert!(settled >= body.len());
            } else {
                // A small frame between large ones must not release and re-zero the buffer.
                assert_eq!(
                    dec.scratch_capacity(),
                    settled,
                    "scratch churned at frame {i}"
                );
            }
        }
    }

    #[test]
    fn encode_to_arc_matches_encode_across_compression_modes() {
        for threshold in [None, Some(256), Some(0)] {
            for skip in [false, true] {
                let mut enc = FrameEncoder::default();
                enc.set_compression(threshold, 6);
                enc.set_skip_incompressible(skip);
                for payload in [
                    Vec::new(),
                    vec![7u8; 64],
                    vec![42u8; 8192],
                    incompressible(4096),
                ] {
                    let mut expected = Vec::new();
                    enc.encode(&payload, &mut expected);
                    let arc = enc.encode_to_arc(&payload);
                    assert_eq!(&*arc, expected.as_slice());

                    let mut dec = FrameDecoder::default();
                    dec.set_compression(threshold);
                    dec.feed(&arc);
                    assert_eq!(dec.next_frame().unwrap().unwrap(), payload);
                }
            }
        }
    }
}
