//! zlib backend used by packet framing.
//!
//! The default build compresses with libdeflate (`libdeflater`). A build with
//! `--no-default-features` serves the same [`Encoder`]/[`Decoder`] API from
//! flate2, so `frame.rs` never knows which one is compiled in. Both key types
//! keep their zlib state: one encoder and one decoder live inside a
//! connection's `FrameEncoder`/`FrameDecoder` and are reused for every packet
//! instead of being constructed per frame.

/// Why a zlib stream could not be decompressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackendError {
    /// The input is not a valid zlib stream.
    BadData,
    /// The output buffer is smaller than the decompressed stream.
    InsufficientSpace,
}

#[cfg(feature = "libdeflate")]
mod backend {
    use libdeflater::{CompressionLvl, Compressor, DecompressionError, Decompressor};

    use super::BackendError;

    /// Reusable zlib compressor. libdeflate keeps the level in the compressor,
    /// so switching levels replaces the object rather than reconfiguring it.
    #[derive(Default)]
    pub(crate) struct Encoder {
        level: u32,
        compressor: Option<Compressor>,
        /// Reused output buffer, grown to the largest bound seen.
        scratch: Vec<u8>,
    }

    impl Encoder {
        pub(crate) fn set_level(&mut self, level: u32) {
            if level != self.level {
                self.level = level;
                self.compressor = None;
            }
        }

        pub(crate) fn level(&self) -> u32 {
            self.level
        }

        /// Compress `body` into the reusable scratch buffer and return the bytes.
        pub(crate) fn compress(&mut self, body: &[u8]) -> &[u8] {
            let level = self.level;
            let compressor = self.compressor.get_or_insert_with(|| {
                // libdeflate accepts 1..=12; levels outside that (including the
                // flate2-style 0) are clamped.
                let level = CompressionLvl::new(level.clamp(1, 12) as i32)
                    .expect("level clamped to libdeflate range");
                Compressor::new(level)
            });
            let bound = compressor.zlib_compress_bound(body.len());
            if self.scratch.len() < bound {
                self.scratch.resize(bound, 0);
            }
            let written = compressor
                .zlib_compress(body, &mut self.scratch[..bound])
                .expect("libdeflate compress bound is sufficient");
            &self.scratch[..written]
        }
    }

    /// Reusable zlib decompressor; `libdeflate` writes into a caller slice.
    #[derive(Default)]
    pub(crate) struct Decoder {
        decompressor: Decompressor,
    }

    impl Decoder {
        pub(crate) fn decompress(
            &mut self,
            input: &[u8],
            out: &mut [u8],
        ) -> Result<usize, BackendError> {
            self.decompressor
                .zlib_decompress(input, out)
                .map_err(|error| match error {
                    DecompressionError::BadData => BackendError::BadData,
                    DecompressionError::InsufficientSpace => BackendError::InsufficientSpace,
                })
        }
    }
}

#[cfg(not(feature = "libdeflate"))]
mod backend {
    use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status};

    use super::BackendError;

    /// `zlib` writes a 6-byte zlib header plus at most 5 bytes per 16 KiB
    /// stored block; the extra 64 bytes keep the loop below to one call.
    fn zlib_bound(len: usize) -> usize {
        len + len / 16_384 * 5 + 6 + 64
    }

    /// Reusable zlib compressor (flate2 fallback).
    #[derive(Default)]
    pub(crate) struct Encoder {
        level: u32,
        stream: Option<Compress>,
        /// Reused output buffer, grown to the largest bound seen.
        scratch: Vec<u8>,
    }

    impl Encoder {
        pub(crate) fn set_level(&mut self, level: u32) {
            if level != self.level {
                self.level = level;
                self.stream = None;
            }
        }

        pub(crate) fn level(&self) -> u32 {
            self.level
        }

        /// Compress `body` into the reusable scratch buffer and return the bytes.
        pub(crate) fn compress(&mut self, body: &[u8]) -> &[u8] {
            let level = self.level.min(9);
            let stream = self
                .stream
                .get_or_insert_with(|| Compress::new(Compression::new(level), true));
            let bound = zlib_bound(body.len());
            if self.scratch.len() < bound {
                self.scratch.resize(bound, 0);
            }
            let mut consumed = 0;
            let mut written = 0;
            let status = loop {
                let status = stream
                    .compress(
                        &body[consumed..],
                        &mut self.scratch[written..bound],
                        FlushCompress::Finish,
                    )
                    .expect("compress into a caller slice");
                let new_consumed = stream.total_in() as usize;
                let new_written = stream.total_out() as usize;
                let progress = new_consumed != consumed || new_written != written;
                consumed = new_consumed;
                written = new_written;
                if matches!(status, Status::StreamEnd) || !progress {
                    break status;
                }
            };
            stream.reset();
            assert!(
                matches!(status, Status::StreamEnd),
                "flate2 zlib output bound is sufficient"
            );
            &self.scratch[..written]
        }
    }

    /// Reusable zlib decompressor (flate2 fallback).
    #[derive(Default)]
    pub(crate) struct Decoder {
        stream: Option<Decompress>,
    }

    impl Decoder {
        pub(crate) fn decompress(
            &mut self,
            input: &[u8],
            out: &mut [u8],
        ) -> Result<usize, BackendError> {
            let stream = self.stream.get_or_insert_with(|| Decompress::new(true));
            let mut consumed = 0;
            let status = loop {
                let before_out = stream.total_out();
                let status = match stream.decompress(
                    &input[consumed..],
                    &mut out[before_out as usize..],
                    FlushDecompress::Finish,
                ) {
                    Ok(status) => status,
                    Err(_) => {
                        // Leave the decoder usable for the next stream.
                        stream.reset(true);
                        return Err(BackendError::BadData);
                    }
                };
                consumed = stream.total_in() as usize;
                if matches!(status, Status::StreamEnd)
                    || before_out == stream.total_out()
                    || stream.total_out() as usize == out.len()
                {
                    break status;
                }
            };
            let written = stream.total_out() as usize;
            stream.reset(true);
            match status {
                Status::StreamEnd => Ok(written),
                _ if written == out.len() => Err(BackendError::InsufficientSpace),
                _ => Err(BackendError::BadData),
            }
        }
    }
}

pub(crate) use backend::{Decoder, Encoder};
