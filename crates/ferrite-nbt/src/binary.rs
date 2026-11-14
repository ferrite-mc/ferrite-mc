//! Bounded Java Edition binary NBT encoding and decoding.
use crate::{Compound, List, Tag};
use std::io::{Read, Write};

const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_DEPTH: usize = 512;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid NBT: {0}")]
    Invalid(&'static str),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
type Result<T> = std::result::Result<T, Error>;

pub fn read_named(bytes: &[u8]) -> Result<(String, Tag)> {
    let mut r = Reader::new(bytes)?;
    let id = r.byte()?;
    let name = r.string()?;
    let tag = r.tag(id, 0)?;
    if r.pos != bytes.len() {
        return Err(Error::Invalid("trailing data"));
    }
    Ok((name, tag))
}
pub fn read_network(bytes: &[u8]) -> Result<Tag> {
    let mut r = Reader::new(bytes)?;
    let id = r.byte()?;
    let tag = r.tag(id, 0)?;
    if r.pos != bytes.len() {
        return Err(Error::Invalid("trailing data"));
    }
    Ok(tag)
}
/// Reads one nameless network tag from the front of `bytes` and reports how many bytes it used.
pub fn read_network_prefix(bytes: &[u8]) -> Result<(Tag, usize)> {
    let mut r = Reader::new(bytes)?;
    let id = r.byte()?;
    let tag = r.tag(id, 0)?;
    Ok((tag, r.pos))
}
pub fn write_named(name: &str, tag: &Tag) -> Result<Vec<u8>> {
    let mut w = Vec::new();
    w.push(tag.id());
    write_string(&mut w, name)?;
    write_tag(&mut w, tag, 0)?;
    Ok(w)
}
pub fn write_network(tag: &Tag) -> Result<Vec<u8>> {
    let mut w = vec![tag.id()];
    write_tag(&mut w, tag, 0)?;
    Ok(w)
}
#[derive(Debug, Clone, Copy)]
pub enum Compression {
    None,
    Gzip,
    Zlib,
}
pub fn read_compressed(bytes: &[u8], compression: Compression) -> Result<(String, Tag)> {
    let source: Box<dyn Read + '_> = match compression {
        Compression::None => Box::new(bytes),
        Compression::Gzip => Box::new(flate2::read::GzDecoder::new(bytes)),
        Compression::Zlib => Box::new(flate2::read::ZlibDecoder::new(bytes)),
    };
    let mut out = Vec::new();
    source.take((MAX_BYTES + 1) as u64).read_to_end(&mut out)?;
    read_named(&out)
}
pub fn write_compressed(name: &str, tag: &Tag, compression: Compression) -> Result<Vec<u8>> {
    let bytes = write_named(name, tag)?;
    match compression {
        Compression::None => Ok(bytes),
        Compression::Gzip => {
            let mut w = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            w.write_all(&bytes)?;
            Ok(w.finish()?)
        }
        Compression::Zlib => {
            let mut w = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            w.write_all(&bytes)?;
            Ok(w.finish()?)
        }
    }
}
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
    remaining_tags: usize,
}
impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Result<Self> {
        if bytes.len() > MAX_BYTES {
            return Err(Error::Invalid("size limit"));
        }
        Ok(Self {
            bytes,
            pos: 0,
            remaining_tags: 1_000_000,
        })
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or(Error::Invalid("size overflow"))?;
        let b = self
            .bytes
            .get(self.pos..end)
            .ok_or(Error::Invalid("truncated"))?;
        self.pos = end;
        Ok(b)
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn number<const N: usize>(&mut self) -> Result<[u8; N]> {
        self.take(N)?
            .try_into()
            .map_err(|_| Error::Invalid("truncated"))
    }
    fn len(&mut self) -> Result<usize> {
        let n = i32::from_be_bytes(self.number()?);
        if n < 0 || n as usize > self.bytes.len() - self.pos {
            return Err(Error::Invalid("invalid length"));
        }
        Ok(n as usize)
    }
    fn string(&mut self) -> Result<String> {
        let n = u16::from_be_bytes(self.number()?) as usize;
        decode_mutf8(self.take(n)?)
    }
    fn tag(&mut self, id: u8, depth: usize) -> Result<Tag> {
        if depth > MAX_DEPTH {
            return Err(Error::Invalid("depth limit"));
        }
        if self.remaining_tags == 0 {
            return Err(Error::Invalid("tag count limit"));
        }
        self.remaining_tags -= 1;
        Ok(match id {
            1 => Tag::Byte(self.byte()? as i8),
            2 => Tag::Short(i16::from_be_bytes(self.number()?)),
            3 => Tag::Int(i32::from_be_bytes(self.number()?)),
            4 => Tag::Long(i64::from_be_bytes(self.number()?)),
            5 => Tag::Float(f32::from_be_bytes(self.number()?)),
            6 => Tag::Double(f64::from_be_bytes(self.number()?)),
            7 => {
                let n = self.len()?;
                Tag::ByteArray(self.take(n)?.iter().map(|b| *b as i8).collect())
            }
            8 => Tag::String(self.string()?),
            9 => {
                let kind = self.byte()?;
                let n = self.len()?;
                Tag::List(self.list(kind, n, depth + 1)?)
            }
            10 => {
                let mut c = Compound::new();
                loop {
                    let kind = self.byte()?;
                    if kind == 0 {
                        break;
                    }
                    let name = self.string()?;
                    let value = self.tag(kind, depth + 1)?;
                    c.insert(name, value);
                }
                Tag::Compound(c)
            }
            11 => {
                let n = self.len()?;
                if n > (self.bytes.len() - self.pos) / 4 {
                    return Err(Error::Invalid("truncated array"));
                }
                let mut a = Vec::with_capacity(n);
                for _ in 0..n {
                    a.push(i32::from_be_bytes(self.number()?));
                }
                Tag::IntArray(a)
            }
            12 => {
                let n = self.len()?;
                if n > (self.bytes.len() - self.pos) / 8 {
                    return Err(Error::Invalid("truncated array"));
                }
                let mut a = Vec::with_capacity(n);
                for _ in 0..n {
                    a.push(i64::from_be_bytes(self.number()?));
                }
                Tag::LongArray(a)
            }
            _ => return Err(Error::Invalid("invalid tag id")),
        })
    }
    fn list(&mut self, id: u8, n: usize, depth: usize) -> Result<List> {
        if n > self.remaining_tags {
            return Err(Error::Invalid("tag count limit"));
        }
        macro_rules! collect {
            ($tag:ident, $list:ident) => {{
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    if let Tag::$tag(x) = self.tag(id, depth)? {
                        v.push(x);
                    } else {
                        unreachable!()
                    }
                }
                List::$list(v)
            }};
        }
        Ok(match id {
            0 if n == 0 => List::End,
            1 => collect!(Byte, Byte),
            2 => collect!(Short, Short),
            3 => collect!(Int, Int),
            4 => collect!(Long, Long),
            5 => collect!(Float, Float),
            6 => collect!(Double, Double),
            7 => collect!(ByteArray, ByteArray),
            8 => collect!(String, String),
            9 => collect!(List, List),
            10 => collect!(Compound, Compound),
            11 => collect!(IntArray, IntArray),
            12 => collect!(LongArray, LongArray),
            _ => return Err(Error::Invalid("invalid list type")),
        })
    }
}
fn decode_mutf8(bytes: &[u8]) -> Result<String> {
    let mut units = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let a = bytes[i];
        let (u, n) = if (1..=0x7f).contains(&a) {
            (u16::from(a), 1)
        } else if a & 0xe0 == 0xc0 && i + 1 < bytes.len() && bytes[i + 1] & 0xc0 == 0x80 {
            let u = (u16::from(a & 31) << 6) | u16::from(bytes[i + 1] & 63);
            if u != 0 && u < 0x80 {
                return Err(Error::Invalid("overlong string"));
            }
            (u, 2)
        } else if a & 0xf0 == 0xe0
            && i + 2 < bytes.len()
            && bytes[i + 1] & 0xc0 == 0x80
            && bytes[i + 2] & 0xc0 == 0x80
        {
            let u = (u16::from(a & 15) << 12)
                | (u16::from(bytes[i + 1] & 63) << 6)
                | u16::from(bytes[i + 2] & 63);
            if u < 0x800 {
                return Err(Error::Invalid("overlong string"));
            }
            (u, 3)
        } else {
            return Err(Error::Invalid("invalid modified UTF-8"));
        };
        units.push(u);
        i += n;
    }
    String::from_utf16(&units).map_err(|_| Error::Invalid("invalid surrogate"))
}
fn write_string(out: &mut Vec<u8>, s: &str) -> Result<()> {
    let mut b = Vec::new();
    for u in s.encode_utf16() {
        if u != 0 && u <= 0x7f {
            b.push(u as u8);
        } else if u <= 0x7ff {
            b.push(0xc0 | (u >> 6) as u8);
            b.push(0x80 | (u & 63) as u8);
        } else {
            b.push(0xe0 | (u >> 12) as u8);
            b.push(0x80 | ((u >> 6) & 63) as u8);
            b.push(0x80 | (u & 63) as u8);
        }
    }
    let n = u16::try_from(b.len()).map_err(|_| Error::Invalid("string too long"))?;
    out.extend_from_slice(&n.to_be_bytes());
    out.extend(b);
    Ok(())
}
fn write_len(out: &mut Vec<u8>, n: usize) -> Result<()> {
    let n = i32::try_from(n).map_err(|_| Error::Invalid("array too long"))?;
    out.extend_from_slice(&n.to_be_bytes());
    Ok(())
}
fn write_tag(out: &mut Vec<u8>, tag: &Tag, depth: usize) -> Result<()> {
    if depth > MAX_DEPTH || out.len() > MAX_BYTES {
        return Err(Error::Invalid("size or depth limit"));
    }
    match tag {
        Tag::Byte(v) => out.push(*v as u8),
        Tag::Short(v) => out.extend_from_slice(&v.to_be_bytes()),
        Tag::Int(v) => out.extend_from_slice(&v.to_be_bytes()),
        Tag::Long(v) => out.extend_from_slice(&v.to_be_bytes()),
        Tag::Float(v) => out.extend_from_slice(&v.to_be_bytes()),
        Tag::Double(v) => out.extend_from_slice(&v.to_be_bytes()),
        Tag::String(v) => write_string(out, v)?,
        Tag::ByteArray(v) => {
            write_len(out, v.len())?;
            out.extend(v.iter().map(|x| *x as u8));
        }
        Tag::IntArray(v) => {
            write_len(out, v.len())?;
            for x in v {
                out.extend_from_slice(&x.to_be_bytes());
            }
        }
        Tag::LongArray(v) => {
            write_len(out, v.len())?;
            for x in v {
                out.extend_from_slice(&x.to_be_bytes());
            }
        }
        Tag::Compound(v) => {
            for (name, value) in v {
                out.push(value.id());
                write_string(out, name)?;
                write_tag(out, value, depth + 1)?;
            }
            out.push(0);
        }
        Tag::List(v) => write_list(out, v, depth + 1)?,
    }
    if out.len() > MAX_BYTES {
        return Err(Error::Invalid("size limit"));
    }
    Ok(())
}
fn write_list(out: &mut Vec<u8>, list: &List, depth: usize) -> Result<()> {
    macro_rules! list {
        ($values:expr, $id:expr, $tag:ident) => {{
            out.push($id);
            write_len(out, $values.len())?;
            for v in $values {
                write_tag(out, &Tag::$tag(v.clone()), depth)?;
            }
        }};
    }
    match list {
        List::End => {
            out.push(0);
            write_len(out, 0)?;
        }
        List::Byte(v) => list!(v, 1, Byte),
        List::Short(v) => list!(v, 2, Short),
        List::Int(v) => list!(v, 3, Int),
        List::Long(v) => list!(v, 4, Long),
        List::Float(v) => list!(v, 5, Float),
        List::Double(v) => list!(v, 6, Double),
        List::ByteArray(v) => list!(v, 7, ByteArray),
        List::String(v) => list!(v, 8, String),
        List::List(v) => list!(v, 9, List),
        List::Compound(v) => list!(v, 10, Compound),
        List::IntArray(v) => list!(v, 11, IntArray),
        List::LongArray(v) => list!(v, 12, LongArray),
    }
    Ok(())
}
