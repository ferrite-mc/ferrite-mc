//! Packet framing: `VarInt length | [VarInt data_length] | body`, with optional zlib compression.
//! Encryption is a byte-stream transform and lives in the network layer below framing.
//!
//! Compression is zlib through the crate's compression backend: libdeflate in
//! the default build, flate2 with `--no-default-features`. The compressor and
//! decompressor objects are created once per `FrameEncoder`/`FrameDecoder` and
//! reused for every packet; the decoder allocates exactly the declared
//! uncompressed size per frame, bounded by [`MAX_UNCOMPRESSED_LEN`].

use std::sync::{Mutex, PoisonError};

use crate::codec::{Decode, DecodeError, Encode, VarInt};
use crate::compress;

/// Vanilla rejects frames larger than 2 MiB (2^21).
pub const MAX_FRAME_LEN: usize = 1 << 21;
/// Vanilla rejects decompressed bodies larger than 8 MiB (2^23).
pub const MAX_UNCOMPRESSED_LEN: usize = 1 << 23;

pub struct FrameEncoder {
    /// `None` = compression disabled, `Some(t)` = compress bodies of at least `t` bytes.
    threshold: Option<usize>,
    /// When true, emit an uncompressed frame (`data_length = 0`) if zlib adds bytes instead of
    /// shrinking the body. Saves egress bandwidth and client CPU on incompressible payloads.
    skip_incompressible: bool,
    // `Mutex` (not `RefCell`) so `&FrameEncoder` stays `Sync`: connection futures
    // that hold one across an await must remain `Send` for `tokio::spawn`.
    state: Mutex<compress::Encoder>,
}

impl Default for FrameEncoder {
    fn default() -> Self {
        Self {
            threshold: None,
            skip_incompressible: false,
            state: Mutex::new(compress::Encoder::default()),
        }
    }
}

impl FrameEncoder {
    pub fn set_compression(&mut self, threshold: Option<usize>, level: u32) {
        self.threshold = threshold;
        self.state
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .set_level(level);
    }

    pub fn set_skip_incompressible(&mut self, skip: bool) {
        self.skip_incompressible = skip;
    }

    /// Frame an already-encoded packet body (ID + fields), appending to `out`.
    pub fn encode(&self, body: &[u8], out: &mut Vec<u8>) {
        let Some(threshold) = self.threshold else {
            VarInt(body.len() as i32).encode(out);
            out.extend_from_slice(body);
            return;
        };
        // `data_length == 0` is the wire marker for an uncompressed body, so an
        // empty body takes that path even when the threshold is zero.
        if body.is_empty() || body.len() < threshold {
            VarInt(body.len() as i32 + 1).encode(out);
            out.push(0); // data_length = 0: uncompressed
            out.extend_from_slice(body);
            return;
        }
        let mut guard = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let compressed = guard.compress(body);
        let data_len = VarInt(body.len() as i32);
        let compressed_frame_len = data_len.len() + compressed.len();
        if self.skip_incompressible && compressed_frame_len > body.len() {
            VarInt(body.len() as i32 + 1).encode(out);
            out.push(0);
            out.extend_from_slice(body);
            return;
        }
        VarInt(compressed_frame_len as i32).encode(out);
        data_len.encode(out);
        out.extend_from_slice(compressed);
    }
}

#[derive(Default)]
pub struct FrameDecoder {
    buf: Vec<u8>,
    threshold: Option<usize>,
    decompressor: compress::Decoder,
}

impl FrameDecoder {
    pub fn set_compression(&mut self, threshold: Option<usize>) {
        self.threshold = threshold;
    }

    /// Feed raw (already decrypted) bytes from the socket.
    pub fn feed(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// Pop the next complete packet body (ID + fields), or `None` if more bytes are needed.
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>, DecodeError> {
        let Some((len, header)) = VarInt::decode_partial(&self.buf)? else {
            return Ok(None);
        };
        let len =
            usize::try_from(len).map_err(|_| DecodeError::Invalid("negative frame length"))?;
        if len > MAX_FRAME_LEN {
            return Err(DecodeError::Invalid("frame too large"));
        }
        if self.buf.len() < header + len {
            return Ok(None);
        }
        let end = header + len;
        if self.threshold.is_none() {
            let frame = self.buf[header..end].to_vec();
            self.buf.drain(..end);
            return Ok(Some(frame));
        }

        // Compressed frame: `[data_length][zlib payload]` inside the frame body.
        let mut body = &self.buf[header..end];
        let data_len = VarInt::decode(&mut body)?.0;
        if data_len == 0 {
            let frame = body.to_vec();
            self.buf.drain(..end);
            return Ok(Some(frame));
        }
        let data_len =
            usize::try_from(data_len).map_err(|_| DecodeError::Invalid("negative data length"))?;
        if data_len > MAX_UNCOMPRESSED_LEN {
            return Err(DecodeError::Invalid("decompressed packet too large"));
        }
        // The declared length sizes the output buffer exactly; the backend
        // rejects streams that do not fit, and the check below rejects streams
        // that end before the declared length.
        let mut out = vec![0u8; data_len];
        let written =
            self.decompressor
                .decompress(body, &mut out)
                .map_err(|error| match error {
                    compress::BackendError::BadData => DecodeError::Invalid("bad zlib data"),
                    compress::BackendError::InsufficientSpace => {
                        DecodeError::Invalid("decompressed length mismatch")
                    }
                })?;
        if written != data_len {
            return Err(DecodeError::Invalid("decompressed length mismatch"));
        }
        self.buf.drain(..end);
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
}
