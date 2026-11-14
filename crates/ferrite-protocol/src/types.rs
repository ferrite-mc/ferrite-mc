//! Composite protocol types shared across packets.

use crate::codec::{Decode, DecodeError, Encode};

/// Block position packed as `x:26 | z:26 | y:12` in a big-endian i64.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    pub fn pack(self) -> i64 {
        ((self.x as i64 & 0x3ff_ffff) << 38)
            | ((self.z as i64 & 0x3ff_ffff) << 12)
            | (self.y as i64 & 0xfff)
    }

    pub fn unpack(v: i64) -> Self {
        Self {
            x: (v >> 38) as i32,
            y: (v << 52 >> 52) as i32,
            z: (v << 26 >> 38) as i32,
        }
    }
}

impl Encode for BlockPos {
    fn encode(&self, out: &mut Vec<u8>) {
        self.pack().encode(out)
    }
}
impl Decode for BlockPos {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        Ok(Self::unpack(i64::decode(buf)?))
    }
}

/// Namespaced identifier such as `minecraft:overworld`. Stored in full form.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Ident(pub String);

impl Ident {
    pub fn new(s: impl Into<String>) -> Self {
        let s = s.into();
        if s.contains(':') {
            Self(s)
        } else {
            Self(format!("minecraft:{s}"))
        }
    }
}

impl Encode for Ident {
    fn encode(&self, out: &mut Vec<u8>) {
        self.0.encode(out)
    }
}
impl Decode for Ident {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        Ok(Self::new(crate::codec::decode_string(buf, 32767)?))
    }
}

/// Low-precision vector (vanilla `LpVec3`, `Vec3.LP_STREAM_CODEC`) used for entity velocities.
///
/// One zero byte encodes a vector shorter than [`LpVec3::ABS_MIN`]. Otherwise the vector is
/// scaled by the ceiling of its largest component; each component is quantized to 15 bits and
/// packed with a 2-bit scale and a continuation flag into 6 bytes, and scales above 3 continue
/// in a trailing VarInt.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LpVec3(pub [f64; 3]);

impl LpVec3 {
    pub const ZERO: Self = Self([0.0; 3]);
    pub const ABS_MAX: f64 = 1.7179869183E10;
    pub const ABS_MIN: f64 = 3.051944088384301E-5;

    fn sanitize(v: f64) -> f64 {
        if v.is_nan() {
            0.0
        } else {
            v.clamp(-Self::ABS_MAX, Self::ABS_MAX)
        }
    }

    fn pack(v: f64) -> i64 {
        // Java's Math.round: round half up.
        ((v * 0.5 + 0.5) * 32766.0 + 0.5).floor() as i64
    }

    fn unpack(v: i64) -> f64 {
        ((v & 32767) as f64).min(32766.0) * 2.0 / 32766.0 - 1.0
    }
}

impl Encode for LpVec3 {
    fn encode(&self, out: &mut Vec<u8>) {
        let [x, y, z] = self.0.map(Self::sanitize);
        let chessboard = x.abs().max(y.abs()).max(z.abs());
        if chessboard < Self::ABS_MIN {
            out.push(0);
            return;
        }
        let scale = chessboard.ceil() as i64;
        let partial = scale & 3 != scale;
        let markers = if partial { scale & 3 | 4 } else { scale };
        let buffer = markers
            | Self::pack(x / scale as f64) << 3
            | Self::pack(y / scale as f64) << 18
            | Self::pack(z / scale as f64) << 33;
        out.push(buffer as u8);
        out.push((buffer >> 8) as u8);
        ((buffer >> 16) as i32).encode(out);
        if partial {
            crate::VarInt((scale >> 2) as i32).encode(out);
        }
    }
}

impl Decode for LpVec3 {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let lowest = i64::from(u8::decode(buf)?);
        if lowest == 0 {
            return Ok(Self::ZERO);
        }
        let middle = i64::from(u8::decode(buf)?);
        let highest = i64::from(u32::decode(buf)?);
        let buffer = highest << 16 | middle << 8 | lowest;
        let mut scale = lowest & 3;
        if lowest & 4 != 0 {
            scale |= (i64::from(crate::VarInt::decode(buf)?.0) & 0xFFFF_FFFF) << 2;
        }
        let scale = scale as f64;
        Ok(Self([
            Self::unpack(buffer >> 3) * scale,
            Self::unpack(buffer >> 18) * scale,
            Self::unpack(buffer >> 33) * scale,
        ]))
    }
}

/// A particle and its option data (`ParticleTypes.STREAM_CODEC`: the registry id, then whatever
/// that particle type's codec writes - a colour, a block state, an item...). The server builds
/// `data` itself, so encoding is the id followed by the bytes; decoding only understands the
/// option-less particles, because the layout of the rest depends on the registry.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParticleOptions {
    pub id: i32,
    pub data: Vec<u8>,
}
impl ParticleOptions {
    pub fn new(id: i32) -> Self {
        Self {
            id,
            data: Vec::new(),
        }
    }
    pub fn with_data(id: i32, data: Vec<u8>) -> Self {
        Self { id, data }
    }
}
impl Encode for ParticleOptions {
    fn encode(&self, out: &mut Vec<u8>) {
        crate::codec::VarInt(self.id).encode(out);
        out.extend_from_slice(&self.data);
    }
}
impl Decode for ParticleOptions {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        Ok(Self::new(crate::codec::VarInt::decode(buf)?.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lp_vec3_matches_vanilla_quantization() {
        let mut out = Vec::new();
        LpVec3([1e-6, 0.0, -1e-6]).encode(&mut out);
        assert_eq!(out, [0]);
        for v in [
            [0.0, 0.2, 0.0],
            [0.05, -0.04, -0.1],
            [3.0, -2.5, 1.0],
            [7.9, 0.0, -12.3],
            [1000.0, 1.0, -0.5],
        ] {
            let mut out = Vec::new();
            LpVec3(v).encode(&mut out);
            let back = LpVec3::decode(&mut out.as_slice()).unwrap().0;
            let scale = v.iter().fold(0.0f64, |m, c| m.max(c.abs())).ceil();
            for (a, b) in v.iter().zip(back) {
                assert!(
                    (a - b).abs() <= scale / 32766.0 + 1e-12,
                    "{v:?} -> {back:?}"
                );
            }
        }
        // Scale 1: lowest byte carries markers 01 and no continuation.
        let mut out = Vec::new();
        LpVec3([0.0, 0.2, 0.0]).encode(&mut out);
        assert_eq!(out.len(), 6);
        assert_eq!(out[0] & 7, 1);
        // Scale 13 needs the VarInt continuation.
        let mut out = Vec::new();
        LpVec3([0.0, 0.0, -12.3]).encode(&mut out);
        assert_eq!(out.len(), 7);
        assert_eq!(out[0] & 7, 4 | (13 & 3));
        assert_eq!(out[6], 13 >> 2);
    }

    #[test]
    #[allow(clippy::unusual_byte_groupings)] // grouped as x / z / y fields
    fn block_pos_packs() {
        for p in [
            BlockPos::new(0, 0, 0),
            BlockPos::new(-1, -64, 1),
            BlockPos::new(30_000_000, 319, -30_000_000),
        ] {
            assert_eq!(BlockPos::unpack(p.pack()), p);
        }
        // Known value from the protocol docs: (18357644, 831, -20882616).
        let p = BlockPos::new(18357644, 831, -20882616);
        assert_eq!(
            p.pack() as u64,
            0b01000110000001110110001100_10110000010101101101001000_001100111111
        );
    }
}

/// A translatable text component, `{translate: key}` as network NBT (the title of vanilla
/// containers such as `container.crafting`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Translatable(pub String);
impl Encode for Translatable {
    fn encode(&self, out: &mut Vec<u8>) {
        10u8.encode(out);
        let mut key = Vec::new();
        PlainText("translate".into()).encode(&mut key);
        // A named compound entry: the tag type, the name, then the string payload.
        out.extend_from_slice(&key);
        let mut value = Vec::new();
        PlainText(self.0.clone()).encode(&mut value);
        out.extend_from_slice(&value[1..]);
        0u8.encode(out);
    }
}
impl Decode for Translatable {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        if u8::decode(buf)? != 10 {
            return Err(DecodeError::Invalid("expected a text compound"));
        }
        if PlainText::decode(buf)?.0 != "translate" {
            return Err(DecodeError::Invalid("expected a translate key"));
        }
        // The value is a bare string payload: restore its tag byte for PlainText.
        let len = u16::decode(&mut &buf[..])? as usize;
        let mut tagged = vec![8u8];
        tagged.extend_from_slice(crate::codec::take(buf, 2 + len)?);
        let key = PlainText::decode(&mut tagged.as_slice())?.0;
        if u8::decode(buf)? != 0 {
            return Err(DecodeError::Invalid("unexpected extra text fields"));
        }
        Ok(Self(key))
    }
}

/// A network NBT text component: a plain string, or a full component compound (styled text,
/// translations, `extra` children) as a nameless network tag.
#[derive(Debug, Clone, PartialEq)]
pub enum NetworkText {
    Plain(String),
    Rich(ferrite_nbt::Tag),
}
impl Encode for NetworkText {
    fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Self::Plain(text) => PlainText(text.clone()).encode(out),
            Self::Rich(tag) => out.extend(
                ferrite_nbt::binary::write_network(tag).expect("component NBT is encodable"),
            ),
        }
    }
}
impl Decode for NetworkText {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        if buf.first() == Some(&8) {
            return Ok(Self::Plain(PlainText::decode(buf)?.0));
        }
        let (tag, used) = ferrite_nbt::binary::read_network_prefix(buf)
            .map_err(|_| DecodeError::Invalid("invalid component NBT"))?;
        *buf = &buf[used..];
        Ok(Self::Rich(tag))
    }
}

/// A plain-text network NBT component encoded as a nameless TAG_String.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlainText(pub String);
impl Encode for PlainText {
    fn encode(&self, out: &mut Vec<u8>) {
        let mut bytes = Vec::new();
        for c in self.0.encode_utf16() {
            if (1..=0x7f).contains(&c) {
                bytes.push(c as u8);
            } else if c <= 0x7ff {
                bytes.push((0xc0 | (c >> 6)) as u8);
                bytes.push((0x80 | (c & 0x3f)) as u8);
            } else {
                bytes.push((0xe0 | (c >> 12)) as u8);
                bytes.push((0x80 | ((c >> 6) & 0x3f)) as u8);
                bytes.push((0x80 | (c & 0x3f)) as u8);
            }
        }
        8u8.encode(out);
        u16::try_from(bytes.len())
            .expect("NBT string exceeds 65535 bytes")
            .encode(out);
        out.extend_from_slice(&bytes);
    }
}
impl Decode for PlainText {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        if u8::decode(buf)? != 8 {
            return Err(DecodeError::Invalid("expected NBT string"));
        }
        let n = u16::decode(buf)? as usize;
        let b = crate::codec::take(buf, n)?;
        let mut units = Vec::new();
        let mut i = 0;
        while i < b.len() {
            let x = b[i];
            let (value, width) = if (1..=0x7f).contains(&x) {
                (x as u16, 1)
            } else if x & 0xe0 == 0xc0 && i + 1 < b.len() && b[i + 1] & 0xc0 == 0x80 {
                ((((x & 31) as u16) << 6) | ((b[i + 1] & 63) as u16), 2)
            } else if x & 0xf0 == 0xe0
                && i + 2 < b.len()
                && b[i + 1] & 0xc0 == 0x80
                && b[i + 2] & 0xc0 == 0x80
            {
                (
                    (((x & 15) as u16) << 12)
                        | (((b[i + 1] & 63) as u16) << 6)
                        | ((b[i + 2] & 63) as u16),
                    3,
                )
            } else {
                return Err(DecodeError::Invalid("invalid modified UTF-8"));
            };
            units.push(value);
            i += width;
        }
        Ok(Self(String::from_utf16(&units).map_err(|_| {
            DecodeError::Invalid("invalid modified UTF-8")
        })?))
    }
}
