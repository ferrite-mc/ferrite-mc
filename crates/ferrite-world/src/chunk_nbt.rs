use crate::{
    BlockEntity, BlockStateRegistry, Chunk, ChunkError, ChunkSection, HeightmapKind, PaletteKind,
    PalettedContainer,
};
use ferrite_nbt::{Compound, List, Tag};
use std::collections::BTreeMap;
/// Data version from the 26.3 version.json world_version.
pub const DATA_VERSION: i32 = 5023;
#[derive(Debug, thiserror::Error)]
pub enum ChunkNbtError {
    #[error("invalid chunk NBT: {0}")]
    Invalid(&'static str),
    #[error("unknown registry entry: {0}")]
    Unknown(String),
    #[error(transparent)]
    Chunk(#[from] ChunkError),
    #[error(transparent)]
    Palette(#[from] crate::PaletteError),
}
fn compound(tag: &Tag) -> Result<&Compound, ChunkNbtError> {
    if let Tag::Compound(c) = tag {
        Ok(c)
    } else {
        Err(ChunkNbtError::Invalid("expected compound"))
    }
}
fn string(tag: &Tag) -> Result<&str, ChunkNbtError> {
    if let Tag::String(s) = tag {
        Ok(s)
    } else {
        Err(ChunkNbtError::Invalid("expected string"))
    }
}
fn ints(c: &Compound, name: &str) -> Result<i32, ChunkNbtError> {
    if let Some(Tag::Int(v)) = c.get(name) {
        Ok(*v)
    } else {
        Err(ChunkNbtError::Invalid("expected integer"))
    }
}
fn state_id_from_tag(
    tag: &Tag,
    registry: &impl BlockStateRegistry,
    props: &mut BTreeMap<String, String>,
) -> Result<u32, ChunkNbtError> {
    props.clear();
    let name = match tag {
        Tag::String(s) => s.as_str(),
        Tag::Compound(c) => {
            let name = string(
                c.get("id")
                    .ok_or(ChunkNbtError::Invalid("missing state id"))?,
            )?;
            if let Some(p) = c.get("properties") {
                for (k, v) in compound(p)? {
                    props.insert(k.clone(), string(v)?.to_owned());
                }
            }
            name
        }
        _ => return Err(ChunkNbtError::Invalid("invalid state")),
    };
    registry
        .block_id(name, props)
        .ok_or_else(|| ChunkNbtError::Unknown(name.to_owned()))
}
fn decode_container(
    c: &Compound,
    kind: PaletteKind,
    registry: &impl BlockStateRegistry,
) -> Result<PalettedContainer, ChunkNbtError> {
    let palette_tag = c
        .get("palette")
        .ok_or(ChunkNbtError::Invalid("missing palette"))?;
    let ids: Vec<u32> = match palette_tag {
        Tag::List(List::String(v)) => {
            if v.is_empty() || v.len() > kind.len() {
                return Err(ChunkNbtError::Invalid("palette size"));
            }
            v.iter()
                .map(|name| {
                    if kind == PaletteKind::Blocks {
                        static EMPTY_PROPS: BTreeMap<String, String> = BTreeMap::new();
                        registry
                            .block_id(name, &EMPTY_PROPS)
                            .ok_or_else(|| ChunkNbtError::Unknown(name.clone()))
                    } else {
                        registry
                            .biome_id(name)
                            .ok_or_else(|| ChunkNbtError::Unknown(name.clone()))
                    }
                })
                .collect::<Result<_, _>>()?
        }
        Tag::List(List::Compound(v)) => {
            if v.is_empty() || v.len() > kind.len() {
                return Err(ChunkNbtError::Invalid("palette size"));
            }
            let mut props = BTreeMap::new();
            let mut out = Vec::with_capacity(v.len());
            for entry in v {
                if let Some(inner) = entry.get("") {
                    if kind == PaletteKind::Blocks {
                        out.push(state_id_from_tag(inner, registry, &mut props)?);
                    } else {
                        let name = string(inner)?;
                        out.push(
                            registry
                                .biome_id(name)
                                .ok_or_else(|| ChunkNbtError::Unknown(name.to_owned()))?,
                        );
                    }
                } else if kind == PaletteKind::Blocks {
                    let name = string(
                        entry
                            .get("id")
                            .ok_or(ChunkNbtError::Invalid("missing state id"))?,
                    )?;
                    props.clear();
                    if let Some(p) = entry.get("properties") {
                        for (k, v) in compound(p)? {
                            props.insert(k.clone(), string(v)?.to_owned());
                        }
                    }
                    out.push(
                        registry
                            .block_id(name, &props)
                            .ok_or_else(|| ChunkNbtError::Unknown(name.to_owned()))?,
                    );
                } else {
                    return Err(ChunkNbtError::Invalid("expected string"));
                }
            }
            out
        }
        _ => return Err(ChunkNbtError::Invalid("invalid palette")),
    };
    let global_bits = if kind == PaletteKind::Blocks {
        registry.block_bits()
    } else {
        registry.biome_bits()
    };
    if ids.len() == 1 {
        return Ok(PalettedContainer::new(kind, ids[0], global_bits)?);
    }
    let bits = ((usize::BITS - (ids.len() - 1).leading_zeros()) as usize)
        .max(if kind == PaletteKind::Blocks { 4 } else { 1 });
    let per = 64 / bits;
    let words = match c.get("data") {
        Some(Tag::LongArray(v)) => v,
        _ => return Err(ChunkNbtError::Invalid("missing palette data")),
    };
    if words.len() != kind.len().div_ceil(per) {
        return Err(ChunkNbtError::Invalid("palette data length"));
    }
    let max_val = 1u64 << global_bits;
    if ids.iter().any(|&id| u64::from(id) >= max_val) {
        return Err(ChunkNbtError::Palette(crate::PaletteError::Value));
    }

    let mask = (1u64 << bits) - 1;
    let max_indirect = usize::from(kind.max_bits());
    if bits <= max_indirect {
        let unique_ids = {
            let mut sorted = ids.clone();
            sorted.sort_unstable();
            sorted.windows(2).all(|w| w[0] != w[1])
        };
        if unique_ids {
            let mut freq = vec![0u16; ids.len()];
            let mut rem = kind.len();
            for &word in words {
                let mut w = word as u64;
                let n = rem.min(per);
                for _ in 0..n {
                    let idx = (w & mask) as usize;
                    let Some(slot) = freq.get_mut(idx) else {
                        return Err(ChunkNbtError::Invalid("palette index"));
                    };
                    *slot += 1;
                    w >>= bits;
                }
                rem -= n;
            }
            let mut counts: Vec<(u32, u16)> = ids
                .iter()
                .copied()
                .zip(freq)
                .filter(|&(_, c)| c > 0)
                .collect();
            counts.sort_unstable_by_key(|&(k, _)| k);
            if counts.len() == 1 {
                return Ok(PalettedContainer::new(kind, counts[0].0, global_bits)?);
            }
            let expected_bits = ((usize::BITS - (counts.len() - 1).leading_zeros()) as usize)
                .max(if kind == PaletteKind::Blocks { 4 } else { 1 });
            if expected_bits == bits {
                let data: Vec<u64> = words.iter().map(|&w| w as u64).collect();
                return Ok(PalettedContainer::from_nbt_parts(
                    kind,
                    global_bits,
                    bits as u8,
                    ids,
                    counts,
                    data,
                ));
            }
        }
    }

    let mut values = Vec::with_capacity(kind.len());
    let mut rem = kind.len();
    for &word in words {
        let mut w = word as u64;
        let n = rem.min(per);
        for _ in 0..n {
            let idx = (w & mask) as usize;
            let id = *ids
                .get(idx)
                .ok_or(ChunkNbtError::Invalid("palette index"))?;
            values.push(id);
            w >>= bits;
        }
        rem -= n;
    }
    Ok(PalettedContainer::from_values_u32(
        kind,
        &values,
        global_bits,
    )?)
}

fn strip_source_nbt(root: &Compound, min_sec: i32, max_sec: i32) -> Option<Compound> {
    let mut out = Compound::new();
    for (k, v) in root {
        match k.as_str() {
            "DataVersion" | "Heightmaps" | "block_entities" => continue,
            "sections" => {
                if let Tag::List(List::Compound(sections)) = v {
                    let mut kept = Vec::new();
                    for sec in sections {
                        let sy = match sec.get("Y") {
                            Some(Tag::Byte(b)) => i32::from(*b),
                            _ => {
                                kept.push(sec.clone());
                                continue;
                            }
                        };
                        if sy < min_sec || sy >= max_sec {
                            kept.push(sec.clone());
                        } else if sec
                            .keys()
                            .any(|sk| !matches!(sk.as_str(), "Y" | "block_states" | "biomes"))
                        {
                            let mut slim = Compound::new();
                            for (sk, sv) in sec {
                                if !matches!(sk.as_str(), "block_states" | "biomes") {
                                    slim.insert(sk.clone(), sv.clone());
                                }
                            }
                            kept.push(slim);
                        }
                    }
                    if !kept.is_empty() {
                        out.insert(k.clone(), Tag::List(List::Compound(kept)));
                    }
                }
            }
            _ => {
                out.insert(k.clone(), v.clone());
            }
        }
    }
    if out.is_empty() { None } else { Some(out) }
}
pub fn chunk_from_nbt(
    tag: &Tag,
    min_y: i32,
    height: u32,
    registry: &impl BlockStateRegistry,
) -> Result<Chunk, ChunkNbtError> {
    let root = compound(tag)?;
    if ints(root, "DataVersion")? != DATA_VERSION {
        return Err(ChunkNbtError::Invalid("unsupported data version"));
    }
    let air = registry
        .block_id("minecraft:air", &BTreeMap::new())
        .ok_or(ChunkNbtError::Invalid("missing air"))?;
    let biome = registry
        .biome_id("minecraft:plains")
        .ok_or(ChunkNbtError::Invalid("missing plains"))?;
    let mut chunk = Chunk::new(
        min_y,
        height,
        air,
        biome,
        registry.block_bits(),
        registry.biome_bits(),
    )?;
    let sections = match root.get("sections") {
        Some(Tag::List(List::Compound(v))) => v,
        _ => return Err(ChunkNbtError::Invalid("missing sections")),
    };
    for section in sections {
        let sy = match section.get("Y") {
            Some(Tag::Byte(v)) => i32::from(*v),
            _ => return Err(ChunkNbtError::Invalid("section Y")),
        };
        let idx = sy - min_y.div_euclid(16);
        if idx < 0 || idx >= chunk.sections.len() as i32 {
            continue;
        }
        let blocks = decode_container(
            compound(
                section
                    .get("block_states")
                    .ok_or(ChunkNbtError::Invalid("missing block states"))?,
            )?,
            PaletteKind::Blocks,
            registry,
        )?;
        let biomes = decode_container(
            compound(
                section
                    .get("biomes")
                    .ok_or(ChunkNbtError::Invalid("missing biomes"))?,
            )?,
            PaletteKind::Biomes,
            registry,
        )?;
        let (mut count, mut fluid_count, mut random_blocks, mut random_fluids) =
            (0u16, 0u16, 0u16, 0u16);
        for (state, occurrences) in blocks.value_counts() {
            let occurrences = occurrences as u16;
            let flags = registry.flags(state);
            if !flags.air {
                count += occurrences;
            }
            if flags.fluid {
                fluid_count += occurrences;
            }
            if registry.randomly_ticking(state) {
                random_blocks += occurrences;
            }
            if registry.fluid_randomly_ticking(state) {
                random_fluids += occurrences;
            }
        }
        chunk.sections[idx as usize] = ChunkSection {
            blocks,
            biomes,
            block_count: count,
            fluid_count,
            random_blocks,
            random_fluids,
        };
    }
    if let Some(Tag::List(List::Compound(entities))) = root.get("block_entities") {
        for entity in entities {
            // A broken block entity must not take the whole chunk down with it: skip it and keep
            // loading the rest, like vanilla's erroring block-entity codec.
            let (Some(Tag::Int(x)), Some(Tag::Int(y)), Some(Tag::Int(z))) =
                (entity.get("x"), entity.get("y"), entity.get("z"))
            else {
                tracing::warn!("skipping a block entity without integer x/y/z coordinates");
                continue;
            };
            let name = match entity.get("id") {
                Some(Tag::String(name)) => name,
                _ => {
                    tracing::warn!(x, y, z, "skipping a block entity without an id");
                    continue;
                }
            };
            let Some(type_id) = registry.entity_id(name) else {
                tracing::warn!(id = %name, "skipping a block entity of an unknown type");
                continue;
            };
            chunk.block_entities.insert(
                (x.rem_euclid(16) as u8, *y, z.rem_euclid(16) as u8),
                BlockEntity {
                    type_id,
                    data: entity.clone(),
                },
            );
        }
    }
    let min_sec = min_y.div_euclid(16);
    let max_sec = (min_y + height as i32).div_euclid(16);
    chunk.source = strip_source_nbt(root, min_sec, max_sec);
    chunk.recompute_heightmaps(registry);
    Ok(chunk)
}
fn state_tag(id: u32, registry: &impl BlockStateRegistry) -> Result<Tag, ChunkNbtError> {
    let state = registry
        .block_state(id)
        .ok_or_else(|| ChunkNbtError::Unknown(format!("block state {id}")))?;
    if state.properties.is_empty() || registry.block_id(&state.name, &BTreeMap::new()) == Some(id) {
        return Ok(Tag::String(state.name.clone()));
    }
    let mut p = Compound::new();
    for (k, v) in &state.properties {
        p.insert(k.clone(), Tag::String(v.clone()));
    }
    let mut c = Compound::new();
    c.insert("id".into(), Tag::String(state.name.clone()));
    c.insert("properties".into(), Tag::Compound(p));
    Ok(Tag::Compound(c))
}
fn encode_container(
    container: &PalettedContainer,
    kind: PaletteKind,
    registry: &impl BlockStateRegistry,
) -> Result<Compound, ChunkNbtError> {
    let palette = container.palette();
    let mut out = Compound::new();
    if kind == PaletteKind::Biomes {
        let mut names = Vec::with_capacity(palette.len());
        for &id in palette {
            let name = registry
                .biome_name(id)
                .ok_or_else(|| ChunkNbtError::Unknown(format!("biome {id}")))?;
            names.push(name.to_owned());
        }
        out.insert("palette".into(), Tag::List(List::String(names)));
    } else {
        let tags: Vec<Tag> = palette
            .iter()
            .map(|&id| state_tag(id, registry))
            .collect::<Result<_, _>>()?;
        if tags.iter().all(|t| matches!(t, Tag::String(_))) {
            out.insert(
                "palette".into(),
                Tag::List(List::String(
                    tags.into_iter()
                        .map(|t| {
                            if let Tag::String(s) = t {
                                s
                            } else {
                                unreachable!()
                            }
                        })
                        .collect(),
                )),
            );
        } else {
            out.insert(
                "palette".into(),
                Tag::List(List::Compound(
                    tags.into_iter()
                        .map(|t| {
                            if let Tag::Compound(c) = t {
                                c
                            } else {
                                let mut c = Compound::new();
                                c.insert(String::new(), t);
                                c
                            }
                        })
                        .collect(),
                )),
            );
        }
    }
    if palette.len() > 1 {
        let bits = ((usize::BITS - (palette.len() - 1).leading_zeros()) as usize)
            .max(if kind == PaletteKind::Blocks { 4 } else { 1 });
        let per = 64 / bits;
        let expected_words = kind.len().div_ceil(per);
        // Fast path: when the container already uses an indirect palette of this exact bit width
        // and its packed word array matches the target length, copy the packed u64 words directly
        // instead of re-querying all 4,096 blocks.
        if usize::from(container.bits_per_entry()) == bits
            && container.packed_data().len() == expected_words
        {
            out.insert(
                "data".into(),
                Tag::LongArray(container.packed_data().iter().map(|&n| n as i64).collect()),
            );
        } else {
            let mut words = vec![0i64; expected_words];
            let is_sorted = palette.windows(2).all(|w| w[0] < w[1]);
            let mut word_idx = 0usize;
            let mut lane = 0usize;
            let mut word = 0u64;
            let mut mru_id = u32::MAX;
            let mut mru_idx = 0u64;
            for i in 0..kind.len() {
                let id = container.get(i).unwrap();
                let index = if id == mru_id {
                    mru_idx
                } else {
                    let pos = if is_sorted && palette.len() > 16 {
                        palette.binary_search(&id).ok()
                    } else {
                        palette.iter().position(|&p| p == id)
                    }
                    .ok_or(ChunkNbtError::Invalid("missing palette value"))?
                        as u64;
                    mru_id = id;
                    mru_idx = pos;
                    pos
                };
                word |= index << (lane * bits);
                lane += 1;
                if lane == per {
                    words[word_idx] = word as i64;
                    word_idx += 1;
                    word = 0;
                    lane = 0;
                }
            }
            if lane > 0 {
                words[word_idx] = word as i64;
            }
            out.insert("data".into(), Tag::LongArray(words));
        }
    }
    Ok(out)
}
pub fn chunk_to_nbt(
    chunk: &Chunk,
    registry: &impl BlockStateRegistry,
) -> Result<Tag, ChunkNbtError> {
    let mut root = chunk.source.clone().unwrap_or_default();
    root.insert("DataVersion".into(), Tag::Int(DATA_VERSION));
    let old_sections = match root.swap_remove("sections") {
        Some(Tag::List(List::Compound(v))) => v,
        _ => Vec::new(),
    };
    let min_sec = chunk.min_y.div_euclid(16);
    let max_sec = (chunk.min_y + chunk.height as i32).div_euclid(16);
    let mut in_range_old: Vec<Option<Compound>> = vec![None; chunk.sections.len()];
    let mut out_of_range_old = Vec::new();
    for sec in old_sections {
        if let Some(Tag::Byte(y)) = sec.get("Y") {
            let yi = i32::from(*y);
            if yi >= min_sec && yi < max_sec {
                let slot = (yi - min_sec) as usize;
                if slot < in_range_old.len() {
                    in_range_old[slot] = Some(sec);
                    continue;
                }
            }
            out_of_range_old.push(sec);
        }
    }
    let mut sections = Vec::with_capacity(chunk.sections.len() + out_of_range_old.len());
    for (i, (section, old_sec)) in chunk.sections.iter().zip(in_range_old).enumerate() {
        let y = min_sec + i as i32;
        let y8 = i8::try_from(y).map_err(|_| ChunkNbtError::Invalid("section y out of range"))?;
        let mut c = old_sec.unwrap_or_default();
        c.insert("Y".into(), Tag::Byte(y8));
        c.insert(
            "block_states".into(),
            Tag::Compound(encode_container(
                &section.blocks,
                PaletteKind::Blocks,
                registry,
            )?),
        );
        c.insert(
            "biomes".into(),
            Tag::Compound(encode_container(
                &section.biomes,
                PaletteKind::Biomes,
                registry,
            )?),
        );
        sections.push(c);
    }
    sections.extend(out_of_range_old);
    root.insert("sections".into(), Tag::List(List::Compound(sections)));
    let mut heights = Compound::new();
    let bits = (u32::BITS - chunk.height.leading_zeros()) as usize;
    let per = 64 / bits;
    let word_len = 256usize.div_ceil(per);
    let mut words_buf = [0i64; 64];
    for (name, kind) in [
        ("WORLD_SURFACE_WG", HeightmapKind::WorldSurfaceWg),
        ("WORLD_SURFACE", HeightmapKind::WorldSurface),
        ("OCEAN_FLOOR_WG", HeightmapKind::OceanFloorWg),
        ("OCEAN_FLOOR", HeightmapKind::OceanFloor),
        ("MOTION_BLOCKING", HeightmapKind::MotionBlocking),
        (
            "MOTION_BLOCKING_NO_LEAVES",
            HeightmapKind::MotionBlockingNoLeaves,
        ),
    ] {
        let words = &mut words_buf[..word_len];
        words.fill(0);
        let mut word_idx = 0usize;
        let mut shift = 0usize;
        for &value in chunk.heights(kind) {
            words[word_idx] |= (u64::from(value) << shift) as i64;
            shift += bits;
            if shift + bits > 64 {
                shift = 0;
                word_idx += 1;
            }
        }
        heights.insert(name.into(), Tag::LongArray(words.to_vec()));
    }
    root.insert("Heightmaps".into(), Tag::Compound(heights));
    let cx = match root.get("xPos") {
        Some(Tag::Int(n)) => *n,
        _ => 0,
    };
    let cz = match root.get("zPos") {
        Some(Tag::Int(n)) => *n,
        _ => 0,
    };
    let entities = chunk
        .block_entities
        .iter()
        .filter_map(|(&(x, y, z), entity)| {
            // Vanilla's block-entity codec drops a type it cannot write instead of failing the
            // whole chunk; the rest of the chunk must still be saved.
            let Some(name) = registry.entity_name(entity.type_id) else {
                tracing::warn!(
                    type_id = entity.type_id,
                    "not saving a block entity of an unknown type"
                );
                return None;
            };
            let mut c = entity.data.clone();
            c.insert("id".into(), Tag::String(name.into()));
            c.insert("x".into(), Tag::Int(cx * 16 + i32::from(x)));
            c.insert("y".into(), Tag::Int(y));
            c.insert("z".into(), Tag::Int(cz * 16 + i32::from(z)));
            Some(c)
        })
        .collect::<Vec<_>>();
    root.insert("block_entities".into(), Tag::List(List::Compound(entities)));
    Ok(Tag::Compound(root))
}
