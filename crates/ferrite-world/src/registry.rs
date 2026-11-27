use crate::{BlockFlags, BlockInfo};
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
};
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlockState {
    pub name: String,
    pub properties: BTreeMap<String, String>,
}
pub trait BlockStateRegistry: BlockInfo {
    fn block_id(&self, name: &str, properties: &BTreeMap<String, String>) -> Option<u32>;
    fn block_state(&self, id: u32) -> Option<&BlockState>;
    fn biome_id(&self, name: &str) -> Option<u32>;
    fn biome_name(&self, id: u32) -> Option<&str>;
    fn entity_id(&self, name: &str) -> Option<u32>;
    fn entity_name(&self, id: u32) -> Option<&str>;
    /// Global palette width for block states: `9..=32`. Decoders reject other widths.
    fn block_bits(&self) -> u8;
    /// Global palette width for biomes: `4..=32`. Decoders reject other widths.
    fn biome_bits(&self) -> u8;
}
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("invalid registry data: {0}")]
    Invalid(&'static str),
}
#[derive(Debug, Clone)]
pub struct RuntimeRegistry {
    states: Vec<Option<BlockState>>,
    lookup: HashMap<BlockState, u32>,
    defaults: HashMap<String, u32>,
    biomes: Vec<String>,
    biome_ids: HashMap<String, u32>,
    entities: HashMap<String, u32>,
    entity_names: HashMap<u32, String>,
}
impl RuntimeRegistry {
    pub fn load(base: impl AsRef<Path>) -> Result<Self, RegistryError> {
        #[derive(serde::Deserialize)]
        struct RawBlock {
            states: Vec<RawState>,
        }
        #[derive(serde::Deserialize)]
        struct RawState {
            id: u32,
            #[serde(default)]
            properties: BTreeMap<String, String>,
            #[serde(default)]
            default: bool,
        }
        #[derive(serde::Deserialize)]
        struct RawRegistries {
            #[serde(rename = "minecraft:block_entity_type")]
            block_entity_type: RawRegistrySection,
        }
        #[derive(serde::Deserialize)]
        struct RawRegistrySection {
            entries: HashMap<String, RawRegistryEntry>,
        }
        #[derive(serde::Deserialize)]
        struct RawRegistryEntry {
            protocol_id: u32,
        }

        let base = base.as_ref();
        let raw_blocks = std::fs::read(base.join("reports/blocks.json"))?;
        let blocks: HashMap<String, RawBlock> = serde_json::from_slice(&raw_blocks)?;
        let mut states = Vec::with_capacity(32_768);
        let mut lookup = HashMap::with_capacity(32_768);
        let mut defaults = HashMap::with_capacity(blocks.len());
        for (name, block) in blocks {
            for state in block.states {
                let id = state.id;
                let entry = BlockState {
                    name: name.clone(),
                    properties: state.properties,
                };
                if states.len() <= id as usize {
                    states.resize(id as usize + 1, None);
                }
                if states[id as usize].replace(entry.clone()).is_some() {
                    return Err(RegistryError::Invalid("duplicate state"));
                }
                lookup.insert(entry, id);
                if state.default {
                    defaults.insert(name.clone(), id);
                }
            }
        }
        let mut biomes: Vec<String> =
            std::fs::read_dir(base.join("data/minecraft/worldgen/biome"))?
                .map(|e| {
                    let e = e?;
                    Ok(format!(
                        "minecraft:{}",
                        e.path()
                            .file_stem()
                            .ok_or(RegistryError::Invalid("biome filename"))?
                            .to_string_lossy()
                    ))
                })
                .collect::<Result<_, RegistryError>>()?;
        biomes.sort();
        let biome_ids = biomes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.clone(), i as u32))
            .collect();
        let raw_registries = std::fs::read(base.join("reports/registries.json"))?;
        let registries: RawRegistries = serde_json::from_slice(&raw_registries)?;
        let mut entities = HashMap::with_capacity(registries.block_entity_type.entries.len());
        let mut entity_names = HashMap::with_capacity(registries.block_entity_type.entries.len());
        for (name, entry) in registries.block_entity_type.entries {
            entity_names.insert(entry.protocol_id, name.clone());
            entities.insert(name, entry.protocol_id);
        }
        Ok(Self {
            states,
            lookup,
            defaults,
            biomes,
            biome_ids,
            entities,
            entity_names,
        })
    }
}
impl BlockStateRegistry for RuntimeRegistry {
    fn block_id(&self, name: &str, properties: &BTreeMap<String, String>) -> Option<u32> {
        if properties.is_empty() {
            self.defaults.get(name).copied()
        } else {
            self.lookup
                .get(&BlockState {
                    name: name.into(),
                    properties: properties.clone(),
                })
                .copied()
        }
    }
    fn block_state(&self, id: u32) -> Option<&BlockState> {
        self.states.get(id as usize)?.as_ref()
    }
    fn biome_id(&self, name: &str) -> Option<u32> {
        self.biome_ids.get(name).copied()
    }
    fn biome_name(&self, id: u32) -> Option<&str> {
        self.biomes.get(id as usize).map(String::as_str)
    }
    fn entity_id(&self, name: &str) -> Option<u32> {
        self.entities.get(name).copied()
    }
    fn entity_name(&self, id: u32) -> Option<&str> {
        self.entity_names.get(&id).map(String::as_str)
    }
    fn block_bits(&self) -> u8 {
        ((usize::BITS - self.states.len().saturating_sub(1).leading_zeros()) as u8).max(9)
    }
    fn biome_bits(&self) -> u8 {
        ((usize::BITS - self.biomes.len().saturating_sub(1).leading_zeros()) as u8).max(4)
    }
}
/// Vanilla state properties, indexed by the global state ID.
impl BlockInfo for RuntimeRegistry {
    fn randomly_ticking(&self, id: u32) -> bool {
        u16::try_from(id)
            .ok()
            .filter(|&id| {
                usize::from(id) < ferrite_data::block_props::STATE_COUNT
                    && self.block_state(u32::from(id)).is_some()
            })
            .is_some_and(|id| ferrite_data::block_props::get(id).randomly_ticking)
    }
    fn fluid_randomly_ticking(&self, id: u32) -> bool {
        // The 26.3 server jar overrides Fluid.isRandomlyTicking only in LavaFluid.
        use ferrite_data::block_props::FluidKind;
        u16::try_from(id)
            .ok()
            .filter(|&id| {
                usize::from(id) < ferrite_data::block_props::STATE_COUNT
                    && self.block_state(u32::from(id)).is_some()
            })
            .is_some_and(|id| {
                matches!(
                    ferrite_data::block_props::get(id).fluid,
                    Some((FluidKind::Lava | FluidKind::FlowingLava, _, _))
                )
            })
    }
    fn flags(&self, id: u32) -> BlockFlags {
        let Ok(id) = u16::try_from(id) else {
            return BlockFlags::default();
        };
        if usize::from(id) >= ferrite_data::block_props::STATE_COUNT
            || self.block_state(u32::from(id)).is_none()
        {
            return BlockFlags::default();
        }
        let props = ferrite_data::block_props::get(id);
        BlockFlags {
            air: props.is_air,
            fluid: props.fluid.is_some(),
            motion_blocking: props.heightmaps & ferrite_data::block_props::OCEAN_FLOOR != 0,
            leaves: props.heightmaps & ferrite_data::block_props::MOTION_BLOCKING != 0
                && props.heightmaps & ferrite_data::block_props::MOTION_BLOCKING_NO_LEAVES == 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_vanilla_heightmap_predicate_is_representable() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3/generated");
        let registry = RuntimeRegistry::load(path).unwrap();
        for id in 0..ferrite_data::block_props::STATE_COUNT as u32 {
            let p = ferrite_data::block_props::get(id as u16);
            let f = registry.flags(id);
            let mask = u8::from(!f.air)
                | (u8::from(f.motion_blocking) << 1)
                | (u8::from(f.motion_blocking || f.fluid) << 2)
                | (u8::from((f.motion_blocking || f.fluid) && !f.leaves) << 3);
            assert_eq!(mask, p.heightmaps, "state {id}");
            assert_eq!(f.fluid, p.fluid.is_some(), "state {id}");
        }
    }
}
