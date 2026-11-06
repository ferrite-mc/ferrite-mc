//! The record in a jukebox, kept in the block entity (`RecordItem`) like vanilla.

use super::anvil::AnvilChunkSource;
use ferrite_item::ItemStack;
use ferrite_nbt::{Compound, Tag};
use ferrite_world::BlockEntity;

const KEY: &str = "RecordItem";

/// Puts a record into the jukebox at `pos`. `false` when the chunk is not loaded.
pub fn insert(source: &AnvilChunkSource, [x, y, z]: [i32; 3], record: &ItemStack) -> bool {
    let Some(type_id) =
        ferrite_data::registries::id("minecraft:block_entity_type", "minecraft:jukebox")
            .and_then(|id| u32::try_from(id).ok())
    else {
        return false;
    };
    let Ok(stack) = record.to_nbt() else {
        return false;
    };
    source
        .update_block_entity(x, y, z, |entity| {
            let entity = entity.get_or_insert_with(|| {
                let mut data = Compound::new();
                data.insert("id".into(), Tag::String("minecraft:jukebox".into()));
                data.insert("x".into(), Tag::Int(x));
                data.insert("y".into(), Tag::Int(y));
                data.insert("z".into(), Tag::Int(z));
                BlockEntity { type_id, data }
            });
            entity.data.insert(KEY.into(), Tag::Compound(stack));
            (true, ())
        })
        .is_ok()
}

/// Takes the record out of the jukebox at `pos`, leaving the block entity without one.
pub fn take(source: &AnvilChunkSource, [x, y, z]: [i32; 3]) -> Option<ItemStack> {
    source
        .update_block_entity(x, y, z, |entity| {
            let Some(entity) = entity.as_mut() else {
                return (false, None);
            };
            match entity.data.shift_remove(KEY) {
                Some(Tag::Compound(stack)) => (true, ItemStack::from_nbt(&stack).ok()),
                _ => (false, None),
            }
        })
        .ok()
        .flatten()
        .filter(|stack| !stack.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_record_goes_in_and_comes_out_once() {
        let source = AnvilChunkSource::new_memory();
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let disc = ItemStack::from_name("minecraft:music_disc_13").unwrap();
        assert!(take(&source, [3, 70, 3]).is_none());
        assert!(insert(&source, [3, 70, 3], &disc));
        let out = take(&source, [3, 70, 3]).expect("the record");
        assert_eq!(out.name(), Some("minecraft:music_disc_13"));
        assert!(take(&source, [3, 70, 3]).is_none());
    }
}
