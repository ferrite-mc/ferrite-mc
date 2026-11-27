use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("unexpected end of input")]
    Eof,
    #[error("VarInt too long")]
    VarIntTooLong,
    #[error("string length {0} exceeds max {1}")]
    StringTooLong(usize, usize),
    #[error("invalid UTF-8")]
    InvalidUtf8,
    #[error("invalid value: {0}")]
    Invalid(&'static str),
    #[error("nbt: {0}")]
    Nbt(String),
}

/// Writes a value in protocol form. Encoding into a `Vec` cannot fail.
pub trait Encode {
    fn encode(&self, out: &mut Vec<u8>);
}

/// Reads a value in protocol form, advancing `buf` past it.
pub trait Decode: Sized {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError>;
}

pub fn take<'a>(buf: &mut &'a [u8], n: usize) -> Result<&'a [u8], DecodeError> {
    if buf.len() < n {
        return Err(DecodeError::Eof);
    }
    let (head, tail) = buf.split_at(n);
    *buf = tail;
    Ok(head)
}

macro_rules! impl_be {
    ($($t:ty),*) => {$(
        impl Encode for $t {
            fn encode(&self, out: &mut Vec<u8>) { out.extend_from_slice(&self.to_be_bytes()); }
        }
        impl Decode for $t {
            fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
                let b = take(buf, size_of::<$t>())?;
                Ok(<$t>::from_be_bytes(b.try_into().unwrap()))
            }
        }
    )*};
}
impl_be!(u8, i8, u16, i16, u32, i32, u64, i64, u128, f32, f64);

impl Encode for bool {
    fn encode(&self, out: &mut Vec<u8>) {
        out.push(*self as u8);
    }
}
impl Decode for bool {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        match u8::decode(buf)? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(DecodeError::Invalid("bool")),
        }
    }
}

/// Exact encoded VarInt byte length indexed by `(value as u32).leading_zeros()` (0..=32).
const VARINT_EXACT_BYTE_LENGTHS: [u8; 33] = [
    5, 5, 5, 5, 4, 4, 4, 4, 4, 4, 4, 3, 3, 3, 3, 3, 3, 3, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1,
    1,
];

/// Exact encoded VarLong byte length indexed by `(value as u64).leading_zeros()` (0..=64).
const VARLONG_EXACT_BYTE_LENGTHS: [u8; 65] = [
    10, 9, 9, 9, 9, 9, 9, 9, 8, 8, 8, 8, 8, 8, 8, 7, 7, 7, 7, 7, 7, 7, 6, 6, 6, 6, 6, 6, 6, 5, 5,
    5, 5, 5, 5, 5, 4, 4, 4, 4, 4, 4, 4, 3, 3, 3, 3, 3, 3, 3, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1,
    1, 1,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct VarInt(pub i32);

impl VarInt {
    pub const MAX_LEN: usize = 5;

    /// Encoded length in bytes (1..=5).
    #[allow(clippy::len_without_is_empty)]
    #[inline]
    pub fn len(self) -> usize {
        usize::from(VARINT_EXACT_BYTE_LENGTHS[(self.0 as u32).leading_zeros() as usize])
    }

    /// Decode from a partial buffer: `Ok(None)` means more bytes are needed.
    #[inline]
    pub fn decode_partial(buf: &[u8]) -> Result<Option<(i32, usize)>, DecodeError> {
        // One- and two-byte values dominate packet data and exit with one test.
        if let Some(&first) = buf.first() {
            if first & 0x80 == 0 {
                return Ok(Some((i32::from(first), 1)));
            }
            if let Some(&second) = buf.get(1)
                && second & 0x80 == 0
            {
                let value = i32::from(first & 0x7f) | i32::from(second & 0x7f) << 7;
                return Ok(Some((value, 2)));
            }
        }
        let mut value = 0u32;
        for i in 0..Self::MAX_LEN {
            let Some(&b) = buf.get(i) else {
                return Ok(None);
            };
            value |= ((b & 0x7f) as u32) << (7 * i);
            if b & 0x80 == 0 {
                return Ok(Some((value as i32, i + 1)));
            }
        }
        Err(DecodeError::VarIntTooLong)
    }
}

impl Encode for VarInt {
    #[inline]
    fn encode(&self, out: &mut Vec<u8>) {
        let mut v = self.0 as u32;
        if v < 0x80 {
            out.push(v as u8);
            return;
        }
        if v < 0x4000 {
            out.extend_from_slice(&[(v as u8 & 0x7f) | 0x80, (v >> 7) as u8]);
            return;
        }
        let mut buf = [0u8; Self::MAX_LEN];
        let mut n = 0;
        loop {
            if v & !0x7f == 0 {
                buf[n] = v as u8;
                out.extend_from_slice(&buf[..=n]);
                return;
            }
            buf[n] = (v as u8 & 0x7f) | 0x80;
            n += 1;
            v >>= 7;
        }
    }
}
impl Decode for VarInt {
    #[inline]
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        match Self::decode_partial(buf)? {
            Some((v, n)) => {
                *buf = &buf[n..];
                Ok(VarInt(v))
            }
            None => Err(DecodeError::Eof),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct VarLong(pub i64);

impl VarLong {
    pub const MAX_LEN: usize = 10;

    /// Encoded length in bytes (1..=10).
    #[allow(clippy::len_without_is_empty)]
    #[inline]
    pub fn len(self) -> usize {
        usize::from(VARLONG_EXACT_BYTE_LENGTHS[(self.0 as u64).leading_zeros() as usize])
    }
}

impl Encode for VarLong {
    #[inline]
    fn encode(&self, out: &mut Vec<u8>) {
        let mut v = self.0 as u64;
        if v < 0x80 {
            out.push(v as u8);
            return;
        }
        let mut buf = [0u8; Self::MAX_LEN];
        let mut n = 0;
        loop {
            if v & !0x7f == 0 {
                buf[n] = v as u8;
                out.extend_from_slice(&buf[..=n]);
                return;
            }
            buf[n] = (v as u8 & 0x7f) | 0x80;
            n += 1;
            v >>= 7;
        }
    }
}
impl Decode for VarLong {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let mut value = 0u64;
        for i in 0..10 {
            let b = u8::decode(buf)?;
            value |= ((b & 0x7f) as u64) << (7 * i);
            if b & 0x80 == 0 {
                return Ok(VarLong(value as i64));
            }
        }
        Err(DecodeError::VarIntTooLong)
    }
}

/// Default max string length in characters.
pub const MAX_STRING: usize = 32767;

impl Encode for str {
    fn encode(&self, out: &mut Vec<u8>) {
        VarInt(self.len() as i32).encode(out);
        out.extend_from_slice(self.as_bytes());
    }
}
impl Encode for String {
    fn encode(&self, out: &mut Vec<u8>) {
        self.as_str().encode(out)
    }
}
impl Decode for String {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        decode_string(buf, MAX_STRING)
    }
}

pub fn decode_string(buf: &mut &[u8], max_chars: usize) -> Result<String, DecodeError> {
    let len = VarInt::decode(buf)?.0;
    let len = usize::try_from(len).map_err(|_| DecodeError::Invalid("negative string length"))?;
    let wire_cap = max_chars.saturating_mul(3);
    if len > wire_cap {
        return Err(DecodeError::StringTooLong(len, wire_cap));
    }
    let s = std::str::from_utf8(take(buf, len)?).map_err(|_| DecodeError::InvalidUtf8)?;
    // Every character is at least one byte, so a byte length within the limit already
    // proves the character count is: skip the second pass over ASCII payloads.
    if len > max_chars {
        let chars = s.chars().count();
        if chars > max_chars {
            return Err(DecodeError::StringTooLong(chars, max_chars));
        }
    }
    Ok(s.to_owned())
}

impl Encode for Uuid {
    fn encode(&self, out: &mut Vec<u8>) {
        self.as_u128().encode(out)
    }
}
impl Decode for Uuid {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        Ok(Uuid::from_u128(u128::decode(buf)?))
    }
}

impl<T: Encode> Encode for Option<T> {
    fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Some(v) => {
                true.encode(out);
                v.encode(out);
            }
            None => false.encode(out),
        }
    }
}
impl<T: Decode> Decode for Option<T> {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        if bool::decode(buf)? {
            Ok(Some(T::decode(buf)?))
        } else {
            Ok(None)
        }
    }
}

/// Length-prefixed (VarInt) array.
impl<T: Encode> Encode for Vec<T> {
    fn encode(&self, out: &mut Vec<u8>) {
        VarInt(self.len() as i32).encode(out);
        for v in self {
            v.encode(out);
        }
    }
}
impl<T: Decode> Decode for Vec<T> {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let len = VarInt::decode(buf)?.0;
        let len =
            usize::try_from(len).map_err(|_| DecodeError::Invalid("negative array length"))?;
        // Every element is at least one byte, so cap preallocation by remaining input.
        let mut v = Vec::with_capacity(len.min(buf.len()));
        for _ in 0..len {
            v.push(T::decode(buf)?);
        }
        Ok(v)
    }
}

impl<T: Encode + ?Sized> Encode for &T {
    fn encode(&self, out: &mut Vec<u8>) {
        (**self).encode(out)
    }
}

/// Raw bytes that consume the rest of the packet.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RestBytes(pub Vec<u8>);

impl Encode for RestBytes {
    fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.0)
    }
}
impl Decode for RestBytes {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let v = buf.to_vec();
        *buf = &[];
        Ok(RestBytes(v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip<T: Encode + Decode + PartialEq + std::fmt::Debug>(v: T) -> Vec<u8> {
        let mut out = Vec::new();
        v.encode(&mut out);
        let mut s = out.as_slice();
        assert_eq!(T::decode(&mut s).unwrap(), v);
        assert!(s.is_empty());
        out
    }

    #[test]
    fn varint_known_values() {
        assert_eq!(roundtrip(VarInt(0)), [0x00]);
        assert_eq!(roundtrip(VarInt(127)), [0x7f]);
        assert_eq!(roundtrip(VarInt(128)), [0x80, 0x01]);
        assert_eq!(roundtrip(VarInt(25565)), [0xdd, 0xc7, 0x01]);
        assert_eq!(roundtrip(VarInt(i32::MAX)), [0xff, 0xff, 0xff, 0xff, 0x07]);
        assert_eq!(roundtrip(VarInt(-1)), [0xff, 0xff, 0xff, 0xff, 0x0f]);
        for v in [0, 1, 300, -300, i32::MIN, i32::MAX] {
            assert_eq!(VarInt(v).len(), roundtrip(VarInt(v)).len());
        }
    }

    #[test]
    fn varlong_known_values() {
        let mut expected = vec![0xff; 9];
        expected.push(0x01);
        assert_eq!(roundtrip(VarLong(-1)), expected);
        roundtrip(VarLong(i64::MIN));
    }

    #[test]
    fn strings_options_arrays_uuids() {
        roundtrip("héllo".to_string());
        roundtrip(Some(5i32));
        roundtrip(None::<i32>);
        roundtrip(vec![1u8, 2, 3]);
        roundtrip(Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef));
    }

    #[test]
    fn varint_decode_fast_path_consumes_exactly_one_or_two_bytes() {
        let tail = [0xab, 0xcd];
        for (value, encoded) in [
            (127i32, &[0x7f][..]),
            (128, &[0x80, 0x01]),
            (16_383, &[0xff, 0x7f]),
            (i32::MAX, &[0xff, 0xff, 0xff, 0xff, 0x07]),
        ] {
            let mut bytes = encoded.to_vec();
            bytes.extend_from_slice(&tail);
            let mut s = bytes.as_slice();
            assert_eq!(VarInt::decode(&mut s).unwrap(), VarInt(value));
            assert_eq!(s, &tail, "{value} should consume {} bytes", encoded.len());
            assert_eq!(
                VarInt::decode_partial(&bytes).unwrap(),
                Some((value, encoded.len()))
            );
        }
    }

    #[test]
    fn partial_varint() {
        assert_eq!(VarInt::decode_partial(&[0x80]).unwrap(), None);
        assert!(VarInt::decode_partial(&[0x80; 5]).is_err());
    }

    #[test]
    fn decode_partial_every_prefix_of_five_byte_varint() {
        for v in [i32::MIN, -1, i32::MAX, 0x1234_5678] {
            let mut bytes = Vec::new();
            VarInt(v).encode(&mut bytes);
            assert_eq!(bytes.len(), VarInt::MAX_LEN, "{v} should encode to 5 bytes");
            for n in 0..bytes.len() {
                assert_eq!(
                    VarInt::decode_partial(&bytes[..n]).unwrap(),
                    None,
                    "prefix of {n} bytes of {v} should need more input"
                );
            }
            assert_eq!(VarInt::decode_partial(&bytes).unwrap(), Some((v, 5)));
        }
    }

    #[test]
    fn varlong_max_len_and_overlong() {
        for v in [-1, i64::MIN] {
            let mut bytes = Vec::new();
            VarLong(v).encode(&mut bytes);
            assert_eq!(bytes.len(), 10, "{v} should encode to 10 bytes");
            let mut s = bytes.as_slice();
            assert_eq!(VarLong::decode(&mut s).unwrap(), VarLong(v));
            assert!(s.is_empty());
        }

        // 11 continuation bytes: the decoder must stop after 10 and error.
        let overlong = [0xffu8; 11];
        let mut s = overlong.as_slice();
        assert!(matches!(
            VarLong::decode(&mut s),
            Err(DecodeError::VarIntTooLong)
        ));
    }

    #[test]
    fn decode_string_rejects_negative_and_oversized_lengths() {
        let mut bytes = Vec::new();
        VarInt(-1).encode(&mut bytes);
        bytes.extend_from_slice(b"abc");
        let mut s = bytes.as_slice();
        assert!(matches!(
            decode_string(&mut s, 16),
            Err(DecodeError::Invalid("negative string length"))
        ));

        let mut bytes = Vec::new();
        VarInt(100).encode(&mut bytes);
        let mut s = bytes.as_slice();
        // 100 bytes is above the wire cap of 3 * max_chars = 30.
        assert!(matches!(
            decode_string(&mut s, 10),
            Err(DecodeError::StringTooLong(100, 30))
        ));
    }

    #[test]
    fn decode_string_limits_characters_not_only_bytes() {
        // Byte length above the cap is allowed when the character count is not.
        let mut bytes = Vec::new();
        "ééé".to_string().encode(&mut bytes);
        let mut s = bytes.as_slice();
        assert_eq!(decode_string(&mut s, 4).unwrap(), "ééé");

        // ... and rejected when it is.
        let mut bytes = Vec::new();
        "ééé".to_string().encode(&mut bytes);
        let mut s = bytes.as_slice();
        assert!(matches!(
            decode_string(&mut s, 2),
            Err(DecodeError::StringTooLong(3, 2))
        ));

        // ASCII at the character cap passes without needing the byte cap.
        let mut bytes = Vec::new();
        "abcd".to_string().encode(&mut bytes);
        let mut s = bytes.as_slice();
        assert_eq!(decode_string(&mut s, 4).unwrap(), "abcd");
    }

    #[test]
    fn vec_decode_huge_length_short_input_is_eof() {
        // One-byte elements: the capacity cap means only the 3 buffered bytes are
        // read before the fourth element hits end of input.
        let mut bytes = Vec::new();
        VarInt(i32::MAX).encode(&mut bytes);
        bytes.extend_from_slice(&[1, 2, 3]);
        let mut s = bytes.as_slice();
        assert!(matches!(Vec::<u8>::decode(&mut s), Err(DecodeError::Eof)));

        // Multi-byte elements: the first element already runs out of input.
        let mut bytes = Vec::new();
        VarInt(i32::MAX).encode(&mut bytes);
        bytes.extend_from_slice(&[1, 2, 3]);
        let mut s = bytes.as_slice();
        assert!(matches!(Vec::<u32>::decode(&mut s), Err(DecodeError::Eof)));
    }

    #[test]
    fn bool_decode_rejects_non_boolean() {
        let buf = [2u8];
        let mut s = buf.as_slice();
        assert!(matches!(
            bool::decode(&mut s),
            Err(DecodeError::Invalid("bool"))
        ));
    }

    #[test]
    fn varint_and_varlong_fast_len_and_encode_match_reference() {
        for shift in 0..32u32 {
            for delta in [-1i32, 0, 1] {
                let v = (1u32 << shift).wrapping_add_signed(delta) as i32;
                let bytes = roundtrip(VarInt(v));
                assert_eq!(VarInt(v).len(), bytes.len(), "VarInt({v}) len mismatch");
                let mut slice = bytes.as_slice();
                assert_eq!(VarInt::decode(&mut slice).unwrap(), VarInt(v));
                assert!(slice.is_empty());
            }
        }
        for shift in 0..64u32 {
            for delta in [-1i64, 0, 1] {
                let v = (1u64 << shift).wrapping_add_signed(delta) as i64;
                let bytes = roundtrip(VarLong(v));
                assert_eq!(VarLong(v).len(), bytes.len(), "VarLong({v}) len mismatch");
                let mut slice = bytes.as_slice();
                assert_eq!(VarLong::decode(&mut slice).unwrap(), VarLong(v));
                assert!(slice.is_empty());
            }
        }
        for v in [0i64, -1, i64::MIN, i64::MAX] {
            let bytes = roundtrip(VarLong(v));
            assert_eq!(VarLong(v).len(), bytes.len());
            let mut slice = bytes.as_slice();
            assert_eq!(VarLong::decode(&mut slice).unwrap(), VarLong(v));
        }
    }
}
