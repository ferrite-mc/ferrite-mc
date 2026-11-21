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
fn palette_entries(tag: &Tag) -> Result<Vec<Tag>, ChunkNbtError> {
    match tag {
        Tag::List(List::String(v)) => Ok(v.iter().cloned().map(Tag::String).collect()),
        Tag::List(List::Compound(v)) => Ok(v
            .iter()
            .map(|c| {
                c.get("")
                    .cloned()
                    .unwrap_or_else(|| Tag::Compound(c.clone()))
            })
            .collect()),
        _ => Err(ChunkNbtError::Invalid("invalid palette")),
    }
}
fn state_id(tag: &Tag, registry: &impl BlockStateRegistry) -> Result<u32, ChunkNbtError> {
    let (name, properties) = match tag {
        Tag::String(s) => (s.as_str(), BTreeMap::new()),
        Tag::Compound(c) => {
            let name = string(
                c.get("id")
                    .ok_or(ChunkNbtError::Invalid("missing state id"))?,
            )?;
            let mut props = BTreeMap::new();
            if let Some(p) = c.get("properties") {
                for (k, v) in compound(p)? {
                    props.insert(k.clone(), string(v)?.to_owned());
                }
            }
            (name, props)
        }
        _ => return Err(ChunkNbtError::Invalid("invalid state")),
    };
    registry
        .block_id(name, &properties)
        .ok_or_else(|| ChunkNbtError::Unknown(name.to_owned()))
}
fn decode_container(
    c: &Compound,
    kind: PaletteKind,
    registry: &impl BlockStateRegistry,
) -> Result<PalettedContainer, ChunkNbtError> {
    let palette = palette_entries(
        c.get("palette")
            .ok_or(ChunkNbtError::Invalid("missing palette"))?,
    )?;
    if palette.is_empty() || palette.len() > kind.len() {
        return Err(ChunkNbtError::Invalid("palette size"));
    }
    let ids: Vec<u32> = palette
        .iter()
        .map(|v| {
            if kind == PaletteKind::Blocks {
                state_id(v, registry)
            } else {
                let name = string(v)?;
                registry
                    .biome_id(name)
                    .ok_or_else(|| ChunkNbtError::Unknown(name.to_owned()))
            }
        })
        .collect::<Result<_, _>>()?;
    let mut values = vec![ids[0]; kind.len()];
    if ids.len() > 1 {
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
        for (i, value) in values.iter_mut().enumerate() {
            let index =
                ((words[i / per] as u64 >> ((i % per) * bits)) & ((1u64 << bits) - 1)) as usize;
            *value = *ids
                .get(index)
                .ok_or(ChunkNbtError::Invalid("palette index"))?;
        }
    }
    Ok(PalettedContainer::from_values(
        kind,
        &values,
        if kind == PaletteKind::Blocks {
            registry.block_bits()
        } else {
            registry.biome_bits()
        },
    )?)
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
    chunk.source = Some(root.clone());
    chunk.recompute_heightmaps(registry);
    Ok(chunk)
}
fn state_tag(id: u32, registry: &impl BlockStateRegistry) -> Result<Tag, ChunkNbtError> {
    let state = registry
        .block_state(id)
        .ok_or_else(|| ChunkNbtError::Unknown(format!("block state {id}")))?;
    if registry.block_id(&state.name, &BTreeMap::new()) == Some(id) {
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
    let tags: Vec<Tag> = palette
        .iter()
        .map(|&id| {
            if kind == PaletteKind::Blocks {
                state_tag(id, registry)
            } else {
                registry
                    .biome_name(id)
                    .map(|s| Tag::String(s.into()))
                    .ok_or_else(|| ChunkNbtError::Unknown(format!("biome {id}")))
            }
        })
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
            let mut words = vec![0u64; expected_words];
            let is_sorted = palette.windows(2).all(|w| w[0] < w[1]);
            for i in 0..kind.len() {
                let id = container.get(i).unwrap();
                let index = if is_sorted {
                    palette.binary_search(&id).ok()
                } else {
                    palette.iter().position(|&p| p == id)
                }
                .ok_or(ChunkNbtError::Invalid("missing palette value"))?;
                words[i / per] |= (index as u64) << ((i % per) * bits);
            }
            out.insert(
                "data".into(),
                Tag::LongArray(words.into_iter().map(|n| n as i64).collect()),
            );
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
    let old_sections = match root.get("sections") {
        Some(Tag::List(List::Compound(v))) => v.clone(),
        _ => Vec::new(),
    };
    let mut sections = Vec::new();
    for (i, section) in chunk.sections.iter().enumerate() {
        let y = chunk.min_y.div_euclid(16) + i as i32;
        let y8 = i8::try_from(y).map_err(|_| ChunkNbtError::Invalid("section y out of range"))?;
        let mut c = old_sections
            .iter()
            .find(|v| v.get("Y") == Some(&Tag::Byte(y8)))
            .cloned()
            .unwrap_or_default();
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
    for section in old_sections {
        if let Some(Tag::Byte(y)) = section.get("Y")
            && (i32::from(*y) < chunk.min_y.div_euclid(16)
                || i32::from(*y) >= (chunk.min_y + chunk.height as i32).div_euclid(16))
        {
            sections.push(section);
        }
    }
    root.insert("sections".into(), Tag::List(List::Compound(sections)));
    let mut heights = Compound::new();
    let bits = (u32::BITS - chunk.height.leading_zeros()) as usize;
    let per = 64 / bits;
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
        let mut words = vec![0u64; 256usize.div_ceil(per)];
        for (i, &value) in chunk.heights(kind).iter().enumerate() {
            words[i / per] |= u64::from(value) << ((i % per) * bits);
        }
        heights.insert(
            name.into(),
            Tag::LongArray(words.into_iter().map(|v| v as i64).collect()),
        );
    }
    root.insert("Heightmaps".into(), Tag::Compound(heights));
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
            let cx = match root.get("xPos") {
                Some(Tag::Int(n)) => *n,
                _ => 0,
            };
            let cz = match root.get("zPos") {
                Some(Tag::Int(n)) => *n,
                _ => 0,
            };
            c.insert("x".into(), Tag::Int(cx * 16 + i32::from(x)));
            c.insert("y".into(), Tag::Int(y));
            c.insert("z".into(), Tag::Int(cz * 16 + i32::from(z)));
            Some(c)
        })
        .collect::<Vec<_>>();
    root.insert("block_entities".into(), Tag::List(List::Compound(entities)));
    Ok(Tag::Compound(root))
}
