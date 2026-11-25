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
    let mut w = Vec::with_capacity(256);
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
/// Encodes a nameless network `TAG_Compound` straight into `out`, omitting any top-level keys in `skip`.
pub fn write_network_compound_into_skipping(
    out: &mut Vec<u8>,
    compound: &Compound,
    skip: &[&str],
) -> Result<()> {
    out.push(10);
    for (name, value) in compound {
        if skip.contains(&name.as_str()) {
            continue;
        }
        out.push(value.id());
        write_string(out, name)?;
        write_tag(out, value, 1)?;
    }
    out.push(0);
    if out.len() > MAX_BYTES {
        return Err(Error::Invalid("size limit"));
    }
    Ok(())
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
                let byte_len = n.checked_mul(4).ok_or(Error::Invalid("truncated array"))?;
                if byte_len > self.bytes.len() - self.pos {
                    return Err(Error::Invalid("truncated array"));
                }
                let raw = &self.bytes[self.pos..self.pos + byte_len];
                self.pos += byte_len;
                let a: Vec<i32> = raw
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| i32::from_be_bytes(*b))
                    .collect();
                Tag::IntArray(a)
            }
            12 => {
                let n = self.len()?;
                let byte_len = n.checked_mul(8).ok_or(Error::Invalid("truncated array"))?;
                if byte_len > self.bytes.len() - self.pos {
                    return Err(Error::Invalid("truncated array"));
                }
                let raw = &self.bytes[self.pos..self.pos + byte_len];
                self.pos += byte_len;
                let a: Vec<i64> = raw
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .map(|b| i64::from_be_bytes(*b))
                    .collect();
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
    // Fast path: pure non-null ASCII is valid UTF-8 and valid MUTF-8 without UTF-16 transcoding.
    if bytes.iter().all(|&b| (1..=0x7f).contains(&b)) {
        return Ok(std::str::from_utf8(bytes)
            .expect("verified ASCII bytes")
            .to_owned());
    }
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
    // Fast path: non-null ASCII strings encode identically in UTF-8 and MUTF-8 with zero allocation.
    if s.bytes().all(|b| (1..=0x7f).contains(&b)) {
        let n = u16::try_from(s.len()).map_err(|_| Error::Invalid("string too long"))?;
        out.extend_from_slice(&n.to_be_bytes());
        out.extend_from_slice(s.as_bytes());
        return Ok(());
    }
    let len_pos = out.len();
    out.extend_from_slice(&[0, 0]);
    for u in s.encode_utf16() {
        if u != 0 && u <= 0x7f {
            out.push(u as u8);
        } else if u <= 0x7ff {
            out.push(0xc0 | (u >> 6) as u8);
            out.push(0x80 | (u & 63) as u8);
        } else {
            out.push(0xe0 | (u >> 12) as u8);
            out.push(0x80 | ((u >> 6) & 63) as u8);
            out.push(0x80 | (u & 63) as u8);
        }
    }
    let byte_len = out.len() - len_pos - 2;
    let Ok(n) = u16::try_from(byte_len) else {
        out.truncate(len_pos);
        return Err(Error::Invalid("string too long"));
    };
    let be = n.to_be_bytes();
    out[len_pos] = be[0];
    out[len_pos + 1] = be[1];
    Ok(())
}
fn write_len(out: &mut Vec<u8>, n: usize) -> Result<()> {
    let n = i32::try_from(n).map_err(|_| Error::Invalid("array too long"))?;
    out.extend_from_slice(&n.to_be_bytes());
    Ok(())
}
fn write_byte_array(out: &mut Vec<u8>, v: &[i8]) -> Result<()> {
    write_len(out, v.len())?;
    if out.len().saturating_add(v.len()) > MAX_BYTES {
        return Err(Error::Invalid("size limit"));
    }
    // SAFETY: `i8` and `u8` have identical size (1), alignment (1), and bit validity.
    let bytes = unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), v.len()) };
    out.extend_from_slice(bytes);
    Ok(())
}
fn write_int_array(out: &mut Vec<u8>, v: &[i32]) -> Result<()> {
    write_len(out, v.len())?;
    let byte_len = v.len().checked_mul(4).ok_or(Error::Invalid("size limit"))?;
    if out.len().saturating_add(byte_len) > MAX_BYTES {
        return Err(Error::Invalid("size limit"));
    }
    out.reserve(byte_len);
    let start = out.len();
    let spare = &mut out.spare_capacity_mut()[..byte_len];
    for (dst, &x) in spare.as_chunks_mut::<4>().0.iter_mut().zip(v) {
        let be = x.to_be_bytes();
        dst[0].write(be[0]);
        dst[1].write(be[1]);
        dst[2].write(be[2]);
        dst[3].write(be[3]);
    }
    // SAFETY: `reserve(byte_len)` succeeded and all `byte_len` bytes in `spare` were initialized.
    unsafe {
        out.set_len(start + byte_len);
    }
    Ok(())
}
fn write_long_array(out: &mut Vec<u8>, v: &[i64]) -> Result<()> {
    write_len(out, v.len())?;
    let byte_len = v.len().checked_mul(8).ok_or(Error::Invalid("size limit"))?;
    if out.len().saturating_add(byte_len) > MAX_BYTES {
        return Err(Error::Invalid("size limit"));
    }
    out.reserve(byte_len);
    let start = out.len();
    let spare = &mut out.spare_capacity_mut()[..byte_len];
    for (dst, &x) in spare.as_chunks_mut::<8>().0.iter_mut().zip(v) {
        let be = x.to_be_bytes();
        dst[0].write(be[0]);
        dst[1].write(be[1]);
        dst[2].write(be[2]);
        dst[3].write(be[3]);
        dst[4].write(be[4]);
        dst[5].write(be[5]);
        dst[6].write(be[6]);
        dst[7].write(be[7]);
    }
    // SAFETY: `reserve(byte_len)` succeeded and all `byte_len` bytes in `spare` were initialized.
    unsafe {
        out.set_len(start + byte_len);
    }
    Ok(())
}
fn write_compound(out: &mut Vec<u8>, v: &Compound, depth: usize) -> Result<()> {
    if depth > MAX_DEPTH || out.len() > MAX_BYTES {
        return Err(Error::Invalid("size or depth limit"));
    }
    for (name, value) in v {
        out.push(value.id());
        write_string(out, name)?;
        write_tag(out, value, depth + 1)?;
    }
    out.push(0);
    if out.len() > MAX_BYTES {
        return Err(Error::Invalid("size limit"));
    }
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
        Tag::ByteArray(v) => write_byte_array(out, v)?,
        Tag::IntArray(v) => write_int_array(out, v)?,
        Tag::LongArray(v) => write_long_array(out, v)?,
        Tag::Compound(v) => write_compound(out, v, depth)?,
        Tag::List(v) => write_list(out, v, depth + 1)?,
    }
    if out.len() > MAX_BYTES {
        return Err(Error::Invalid("size limit"));
    }
    Ok(())
}
fn check_list_header(out: &mut Vec<u8>, len: usize, elem_bytes: usize, depth: usize) -> Result<()> {
    if !len.is_multiple_of(1) {
        unreachable!();
    }
    if len > 0 && (depth > MAX_DEPTH || out.len() > MAX_BYTES) {
        return Err(Error::Invalid("size or depth limit"));
    }
    if elem_bytes > 0 {
        let byte_len = len
            .checked_mul(elem_bytes)
            .ok_or(Error::Invalid("size limit"))?;
        if out.len().saturating_add(byte_len) > MAX_BYTES {
            return Err(Error::Invalid("size limit"));
        }
        out.reserve(byte_len);
    }
    Ok(())
}
fn write_list(out: &mut Vec<u8>, list: &List, depth: usize) -> Result<()> {
    match list {
        List::End => {
            out.push(0);
            write_len(out, 0)?;
        }
        List::Byte(v) => {
            out.push(1);
            write_len(out, v.len())?;
            check_list_header(out, v.len(), 1, depth)?;
            // SAFETY: `i8` and `u8` have identical size (1), alignment (1), and bit validity.
            let bytes = unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), v.len()) };
            out.extend_from_slice(bytes);
        }
        List::Short(v) => {
            out.push(2);
            write_len(out, v.len())?;
            check_list_header(out, v.len(), 2, depth)?;
            for &x in v {
                out.extend_from_slice(&x.to_be_bytes());
            }
        }
        List::Int(v) => {
            out.push(3);
            write_len(out, v.len())?;
            check_list_header(out, v.len(), 4, depth)?;
            for &x in v {
                out.extend_from_slice(&x.to_be_bytes());
            }
        }
        List::Long(v) => {
            out.push(4);
            write_len(out, v.len())?;
            check_list_header(out, v.len(), 8, depth)?;
            for &x in v {
                out.extend_from_slice(&x.to_be_bytes());
            }
        }
        List::Float(v) => {
            out.push(5);
            write_len(out, v.len())?;
            check_list_header(out, v.len(), 4, depth)?;
            for &x in v {
                out.extend_from_slice(&x.to_be_bytes());
            }
        }
        List::Double(v) => {
            out.push(6);
            write_len(out, v.len())?;
            check_list_header(out, v.len(), 8, depth)?;
            for &x in v {
                out.extend_from_slice(&x.to_be_bytes());
            }
        }
        List::ByteArray(v) => {
            out.push(7);
            write_len(out, v.len())?;
            check_list_header(out, v.len(), 0, depth)?;
            for arr in v {
                write_byte_array(out, arr)?;
            }
        }
        List::String(v) => {
            out.push(8);
            write_len(out, v.len())?;
            check_list_header(out, v.len(), 0, depth)?;
            for s in v {
                if out.len() > MAX_BYTES {
                    return Err(Error::Invalid("size or depth limit"));
                }
                write_string(out, s)?;
                if out.len() > MAX_BYTES {
                    return Err(Error::Invalid("size limit"));
                }
            }
        }
        List::List(v) => {
            out.push(9);
            write_len(out, v.len())?;
            check_list_header(out, v.len(), 0, depth)?;
            for sub in v {
                if out.len() > MAX_BYTES {
                    return Err(Error::Invalid("size or depth limit"));
                }
                write_list(out, sub, depth + 1)?;
                if out.len() > MAX_BYTES {
                    return Err(Error::Invalid("size limit"));
                }
            }
        }
        List::Compound(v) => {
            out.push(10);
            write_len(out, v.len())?;
            for c in v {
                write_compound(out, c, depth)?;
            }
        }
        List::IntArray(v) => {
            out.push(11);
            write_len(out, v.len())?;
            check_list_header(out, v.len(), 0, depth)?;
            for arr in v {
                write_int_array(out, arr)?;
            }
        }
        List::LongArray(v) => {
            out.push(12);
            write_len(out, v.len())?;
            check_list_header(out, v.len(), 0, depth)?;
            for arr in v {
                write_long_array(out, arr)?;
            }
        }
    }
    Ok(())
}
