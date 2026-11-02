//! Brewing stand block entities (vanilla `BrewingStandBlockEntity`).
//!
//! The five slots (three bottles, the ingredient and the blaze-powder fuel) and the two timers
//! live in the block entity's NBT (`Items`, `BrewTime`, `Fuel`), like every other container in
//! this server, so they are saved with the chunk and visible to `/data merge block`. A menu
//! ticks the stand while a player has it open; closing writes the state back.

use ferrite_inventory::BrewingState;
use ferrite_item::ItemStack;
use ferrite_nbt::{Compound, List, Tag};
use ferrite_world::BlockEntity;

use super::anvil::AnvilChunkSource;

/// Slots of a brewing stand: three bottles, the ingredient, the fuel.
pub const SLOTS: usize = 5;

/// The block entity type name of a brewing stand.
pub const BLOCK: &str = "minecraft:brewing_stand";

/// The block entity state of a brewing stand.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BrewingData {
    pub items: Vec<ItemStack>,
    pub state: BrewingState,
}

fn number(entity: Option<&BlockEntity>, key: &str) -> i32 {
    match entity.and_then(|entity| entity.data.get(key)) {
        Some(Tag::Byte(value)) => i32::from(*value),
        Some(Tag::Short(value)) => i32::from(*value),
        Some(Tag::Int(value)) => *value,
        _ => 0,
    }
}

/// Vanilla `ContainerHelper.loadAllItems` for the five brewing slots.
fn load_items(entity: Option<&BlockEntity>) -> Vec<ItemStack> {
    let mut items = vec![ItemStack::empty(); SLOTS];
    let Some(Tag::List(List::Compound(list))) = entity.and_then(|e| e.data.get("Items")) else {
        return items;
    };
    for entry in list {
        let Some(Tag::Byte(slot)) = entry.get("Slot") else {
            continue;
        };
        let Some(target) = usize::try_from(*slot).ok().and_then(|s| items.get_mut(s)) else {
            continue;
        };
        match ItemStack::from_nbt(entry) {
            Ok(stack) => *target = stack,
            Err(error) => tracing::warn!(%error, "ignoring an unreadable brewing stand item"),
        }
    }
    items
}

/// Vanilla `ContainerHelper.saveAllItems`: non-empty slots with their `Slot` byte.
fn save_items(items: &[ItemStack]) -> Tag {
    let list = items
        .iter()
        .enumerate()
        .filter(|(_, stack)| !stack.is_empty())
        .filter_map(|(slot, stack)| {
            let mut entry = stack.to_nbt().ok()?;
            entry.insert("Slot".into(), Tag::Byte(slot as i8));
            Some(entry)
        })
        .collect();
    Tag::List(List::Compound(list))
}

fn block_entity_type() -> Option<u32> {
    ferrite_data::registries::id("minecraft:block_entity_type", BLOCK)
        .and_then(|id| u32::try_from(id).ok())
}

/// Reads a brewing stand's items and timers.
pub fn load(source: &AnvilChunkSource, [x, y, z]: [i32; 3]) -> BrewingData {
    let entity = source.block_entity(x, y, z);
    let entity = entity.as_ref();
    BrewingData {
        items: load_items(entity),
        state: BrewingState {
            brew_time: number(entity, "BrewTime"),
            fuel: number(entity, "Fuel"),
        },
    }
}

/// Writes the items and timers back, creating the block entity when one is missing.
pub fn store(source: &AnvilChunkSource, [x, y, z]: [i32; 3], data: &BrewingData) {
    let Some(type_id) = block_entity_type() else {
        return;
    };
    let result = source.update_block_entity(x, y, z, |entity| {
        let current = entity.as_ref().map(|entity| BrewingData {
            items: load_items(Some(entity)),
            state: BrewingState {
                brew_time: number(Some(entity), "BrewTime"),
                fuel: number(Some(entity), "Fuel"),
            },
        });
        if current.as_ref() == Some(data) {
            return (false, ());
        }
        let entity = entity.get_or_insert_with(|| {
            let mut nbt = Compound::new();
            nbt.insert("id".into(), Tag::String(BLOCK.into()));
            nbt.insert("x".into(), Tag::Int(x));
            nbt.insert("y".into(), Tag::Int(y));
            nbt.insert("z".into(), Tag::Int(z));
            BlockEntity { type_id, data: nbt }
        });
        entity.data.insert("Items".into(), save_items(&data.items));
        // Vanilla saves the two timers as a short and a byte.
        entity
            .data
            .insert("BrewTime".into(), Tag::Short(data.state.brew_time as i16));
        entity
            .data
            .insert("Fuel".into(), Tag::Byte(data.state.fuel as i8));
        (true, ())
    });
    if let Err(error) = result {
        tracing::warn!(x, y, z, %error, "could not store brewing stand state");
    }
}

/// Creates the block entity of a freshly placed brewing stand, so `/data merge block` and the
/// chunk save have somewhere to put the state before the first menu opens.
pub fn ensure(source: &AnvilChunkSource, pos: [i32; 3]) {
    store(source, pos, &BrewingData::default());
}

/// Removes the block entity of a broken brewing stand and returns its items.
pub fn take_contents(source: &AnvilChunkSource, [x, y, z]: [i32; 3]) -> Vec<ItemStack> {
    source
        .update_block_entity(x, y, z, |entity| {
            entity.take().map_or((false, Vec::new()), |removed| {
                (true, load_items(Some(&removed)))
            })
        })
        .unwrap_or_default()
        .into_iter()
        .filter(|stack| !stack.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::commands_data::{BlockData, apply_block};
    use ferrite_nbt::snbt;

    async fn loaded() -> std::sync::Arc<AnvilChunkSource> {
        let source = AnvilChunkSource::new_memory();
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        source
    }

    fn water_bottle() -> ItemStack {
        let mut stack = ItemStack::from_name("potion").unwrap();
        stack.components.insert(
            ferrite_item::component_ids::POTION_CONTENTS,
            ferrite_item::Component::PotionContents(ferrite_item::PotionContents {
                potion: Some("minecraft:water".into()),
                ..Default::default()
            }),
        );
        stack
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn brewing_state_round_trips_and_survives_a_data_merge() {
        let source = loaded().await;
        let state =
            u32::from(ferrite_data::blocks::state_id("brewing_stand", &[]).expect("vanilla block"));
        source.set_block(8, 64, 8, state).unwrap();
        ensure(&source, [8, 64, 8]);
        assert_eq!(
            source.block_entity(8, 64, 8).unwrap().data.get("id"),
            Some(&Tag::String(BLOCK.into()))
        );
        let mut data = BrewingData {
            items: vec![ItemStack::empty(); SLOTS],
            state: BrewingState {
                brew_time: 123,
                fuel: 7,
            },
        };
        data.items[0] = water_bottle();
        data.items[3] = ItemStack::from_name("nether_wart")
            .unwrap()
            .copy_with_count(2);
        data.items[4] = ItemStack::from_name("blaze_powder").unwrap();
        store(&source, [8, 64, 8], &data);
        assert_eq!(load(&source, [8, 64, 8]), data);
        // Storing unchanged state does not mark the chunk modified again.
        store(&source, [8, 64, 8], &data);
        let entity = source.block_entity(8, 64, 8).unwrap();
        assert_eq!(entity.data.get("BrewTime"), Some(&Tag::Short(123)));
        assert_eq!(entity.data.get("Fuel"), Some(&Tag::Byte(7)));

        // `/data merge block` reaches the same keys the loader reads.
        let merged = snbt::parse(r#"{BrewTime:5s,Fuel:3b}"#).unwrap();
        apply_block(
            Some(source.as_ref()),
            &BlockData::Merge {
                pos: [8, 64, 8],
                data: merged,
            },
        )
        .expect("the block entity exists");
        let after = load(&source, [8, 64, 8]);
        assert_eq!(
            after.state,
            BrewingState {
                brew_time: 5,
                fuel: 3
            }
        );
        assert_eq!(after.items, data.items);

        let spilled = take_contents(&source, [8, 64, 8]);
        assert_eq!(
            spilled,
            vec![
                data.items[0].clone(),
                data.items[3].clone(),
                data.items[4].clone()
            ]
        );
        assert!(source.block_entity(8, 64, 8).is_none());
    }
}
