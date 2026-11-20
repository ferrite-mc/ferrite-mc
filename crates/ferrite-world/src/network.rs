use crate::{Chunk, HeightmapKind, put_varint};
use ferrite_nbt::{Tag, binary};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LightSection {
    Unknown,
    Empty,
    Data(Box<[u8; 2048]>),
}
#[derive(Debug, Clone, Default)]
pub struct LightData {
    pub sky: Vec<LightSection>,
    pub block: Vec<LightSection>,
}
#[derive(Debug, thiserror::Error)]
pub enum NetworkError {
    #[error("light section count does not match dimension")]
    LightCount,
    #[error("block entity Y out of range")]
    EntityY,
    #[error(transparent)]
    Nbt(#[from] binary::Error),
}
/// Encode the complete packet body, including coordinates and light masks.
pub fn encode_chunk_packet(
    x: i32,
    z: i32,
    chunk: &Chunk,
    light: &LightData,
) -> Result<Vec<u8>, NetworkError> {
    let count = chunk.sections().len() + 2;
    if light.sky.len() != count || light.block.len() != count {
        return Err(NetworkError::LightCount);
    }
    let mut out = Vec::new();
    out.extend_from_slice(&x.to_be_bytes());
    out.extend_from_slice(&z.to_be_bytes());
    put_varint(&mut out, HeightmapKind::ALL.len() as u32);
    let bits = (u32::BITS - chunk.height_blocks().leading_zeros()) as usize;
    let per = 64 / bits;
    let word_len = 256usize.div_ceil(per);
    let mut words = vec![0u64; word_len];
    for kind in HeightmapKind::ALL {
        put_varint(&mut out, kind as u32);
        words.fill(0);
        for (i, &value) in chunk.heights(kind).iter().enumerate() {
            words[i / per] |= u64::from(value) << ((i % per) * bits);
        }
        put_varint(&mut out, word_len as u32);
        for &word in &words {
            out.extend_from_slice(&word.to_be_bytes());
        }
    }
    let mut sections = Vec::with_capacity(chunk.sections().len() * 64);
    for section in chunk.sections() {
        section.encode_network_into(&mut sections);
    }
    put_varint(&mut out, sections.len() as u32);
    out.extend(sections);
    put_varint(&mut out, chunk.block_entities.len() as u32);
    for (&(x, y, z), entity) in &chunk.block_entities {
        if x >= 16 || z >= 16 {
            return Err(NetworkError::EntityY);
        }
        out.push((x << 4) | z);
        out.extend_from_slice(
            &i16::try_from(y)
                .map_err(|_| NetworkError::EntityY)?
                .to_be_bytes(),
        );
        put_varint(&mut out, entity.type_id);
        let mut data = entity.data.clone();
        data.shift_remove("x");
        data.shift_remove("y");
        data.shift_remove("z");
        out.extend(binary::write_network(&Tag::Compound(data))?);
    }
    write_mask(&mut out, &light.sky, false);
    write_mask(&mut out, &light.block, false);
    write_mask(&mut out, &light.sky, true);
    write_mask(&mut out, &light.block, true);
    write_arrays(&mut out, &light.sky);
    write_arrays(&mut out, &light.block);
    Ok(out)
}
fn write_mask(out: &mut Vec<u8>, sections: &[LightSection], empty: bool) {
    let word_count = sections.len().div_ceil(64);
    let mut stack = [0u64; 4];
    let mut heap = Vec::new();
    let words: &mut [u64] = if word_count <= stack.len() {
        &mut stack[..word_count]
    } else {
        heap.resize(word_count, 0);
        &mut heap
    };
    for (i, section) in sections.iter().enumerate() {
        if if empty {
            matches!(section, LightSection::Empty)
        } else {
            matches!(section, LightSection::Data(_))
        } {
            words[i / 64] |= 1u64 << (i % 64);
        }
    }
    // 26.3 reads a light mask (`java.util.BitSet`) as a VarInt-prefixed byte array of
    // `BitSet.toByteArray()`: little-endian bytes without trailing zero bytes.
    let mut trimmed_len = words.len() * 8;
    while trimmed_len > 0 {
        let word = words[(trimmed_len - 1) / 8];
        let byte = ((word >> (((trimmed_len - 1) % 8) * 8)) & 0xff) as u8;
        if byte != 0 {
            break;
        }
        trimmed_len -= 1;
    }
    put_varint(out, trimmed_len as u32);
    for (idx, &word) in words.iter().enumerate() {
        let start = idx * 8;
        if start >= trimmed_len {
            break;
        }
        let take = (trimmed_len - start).min(8);
        out.extend_from_slice(&word.to_le_bytes()[..take]);
    }
}
fn write_arrays(out: &mut Vec<u8>, sections: &[LightSection]) {
    put_varint(
        out,
        sections
            .iter()
            .filter(|s| matches!(s, LightSection::Data(_)))
            .count() as u32,
    );
    for section in sections {
        if let LightSection::Data(data) = section {
            put_varint(out, 2048);
            out.extend_from_slice(&data[..]);
        }
    }
}
