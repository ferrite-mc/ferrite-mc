//! Block entity data for plugins that capture structures: the NBT of the block entities in a box
//! as hex, and (through `/data merge block`, see `extra.rs`) writing it back.

use ferrite_plugin::{HOST_ITEM_SEPARATOR, HOST_NULL, HostCall};

/// The reply to `World#ferriteBlockEntities(x1, y1, z1, x2, y2, z2)`: one item per block entity,
/// `x,y,z,<hex of binary NBT>`, in a list.
pub fn answer(call: &HostCall) -> Option<String> {
    answer_with_view(call, ferrite_plugin::world_view().as_deref())
}

fn answer_with_view(
    call: &HostCall,
    view: Option<&dyn ferrite_plugin::WorldView>,
) -> Option<String> {
    if call.owner != "World" || call.method != "ferriteBlockEntities(int,int,int,int,int,int)" {
        return None;
    }
    let world = uuid::Uuid::parse_str(&call.subject).ok()?;
    let n: Vec<i32> = call
        .args
        .iter()
        .map(|a| a.parse().ok())
        .collect::<Option<_>>()?;
    let [x1, y1, z1, x2, y2, z2] = n[..] else {
        return None;
    };
    let Some(view) = view else {
        return Some(HOST_NULL.to_owned());
    };
    let Some(entities) = view.block_entities(world, [x1, y1, z1], [x2, y2, z2]) else {
        return Some(HOST_NULL.to_owned());
    };
    let items: Vec<String> = entities
        .into_iter()
        .map(|(at, bytes)| format!("{},{},{},{}", at[0], at[1], at[2], hex(&bytes)))
        .collect();
    Some(items.join(&HOST_ITEM_SEPARATOR.to_string()))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{AnvilChunkSource, BlockData, Dimension, apply_block};
    use ferrite_nbt::{Compound, Tag, binary};
    use std::sync::Arc;

    fn call(world: uuid::Uuid, min: [i32; 3], max: [i32; 3]) -> HostCall {
        HostCall {
            owner: "World".into(),
            subject: world.to_string(),
            method: "ferriteBlockEntities(int,int,int,int,int,int)".into(),
            args: [min[0], min[1], min[2], max[0], max[1], max[2]]
                .map(|value| value.to_string())
                .to_vec(),
        }
    }

    async fn ready(source: &Arc<AnvilChunkSource>) {
        for _ in 0..2_000 {
            if source.get(0, 0, 0).is_some() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("chunk did not load");
    }

    fn put_entity(source: &AnvilChunkSource, [x, y, z]: [i32; 3], marker: &str) {
        let type_id =
            ferrite_data::registries::id("minecraft:block_entity_type", "minecraft:chest")
                .and_then(|id| u32::try_from(id).ok())
                .expect("chest block entity type");
        let mut data = Compound::new();
        data.insert("id".into(), Tag::String("minecraft:chest".into()));
        data.insert("x".into(), Tag::Int(x));
        data.insert("y".into(), Tag::Int(y));
        data.insert("z".into(), Tag::Int(z));
        data.insert("marker".into(), Tag::String(marker.into()));
        source
            .update_block_entity(x, y, z, |entity| {
                *entity = Some(ferrite_world::BlockEntity { type_id, data });
                (true, ())
            })
            .unwrap();
    }

    fn decoded(reply: &str) -> Vec<([i32; 3], String)> {
        if reply.is_empty() || reply == HOST_NULL {
            return Vec::new();
        }
        reply
            .split(HOST_ITEM_SEPARATOR)
            .map(|item| {
                let mut parts = item.splitn(4, ',');
                let x = parts.next().unwrap().parse::<i32>().unwrap();
                let y = parts.next().unwrap().parse::<i32>().unwrap();
                let z = parts.next().unwrap().parse::<i32>().unwrap();
                let at = [x, y, z];
                let hex = parts.next().unwrap();
                let bytes: Vec<u8> = (0..hex.len())
                    .step_by(2)
                    .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).expect("hex byte"))
                    .collect();
                let (_, tag) = binary::read_named(&bytes).expect("named NBT");
                let Tag::Compound(data) = tag else {
                    panic!("block entity NBT is a compound");
                };
                let Some(Tag::String(marker)) = data.get("marker") else {
                    panic!("block entity has a marker");
                };
                (at, marker.clone())
            })
            .collect()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn block_entities_are_read_per_dimension_and_missing_worlds_are_explicit() {
        let overworld = AnvilChunkSource::new_memory_dimension(None, Dimension::Overworld);
        let nether = AnvilChunkSource::new_memory_dimension(None, Dimension::Nether);
        ready(&overworld).await;
        ready(&nether).await;
        put_entity(&overworld, [1, 64, 1], "overworld");
        put_entity(&nether, [2, 64, 2], "nether");

        let view = crate::plugins::world_view_for_sources([
            (
                crate::plugins::world_id(Dimension::Overworld),
                Arc::clone(&overworld),
            ),
            (
                crate::plugins::world_id(Dimension::Nether),
                Arc::clone(&nether),
            ),
        ]);
        let over = answer_with_view(
            &call(
                crate::plugins::world_id(Dimension::Overworld),
                [0, 0, 0],
                [10, 100, 10],
            ),
            Some(view.as_ref()),
        )
        .unwrap();
        assert_eq!(decoded(&over), vec![([1, 64, 1], "overworld".into())]);

        let nether_reply = answer_with_view(
            &call(
                crate::plugins::world_id(Dimension::Nether),
                [0, 0, 0],
                [10, 100, 10],
            ),
            Some(view.as_ref()),
        )
        .unwrap();
        assert_eq!(decoded(&nether_reply), vec![([2, 64, 2], "nether".into())]);

        let missing = uuid::Uuid::from_u128(999);
        assert_eq!(
            answer_with_view(&call(missing, [0; 3], [1; 3]), Some(view.as_ref())).as_deref(),
            Some(HOST_NULL)
        );
        assert_eq!(
            answer_with_view(
                &call(
                    crate::plugins::world_id(Dimension::Overworld),
                    [0; 3],
                    [1; 3]
                ),
                None
            )
            .as_deref(),
            Some(HOST_NULL)
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn block_entity_writes_round_trip_in_the_nether_without_touching_the_overworld() {
        let overworld = AnvilChunkSource::new_memory_dimension(None, Dimension::Overworld);
        let nether = AnvilChunkSource::new_memory_dimension(None, Dimension::Nether);
        ready(&overworld).await;
        ready(&nether).await;
        put_entity(&overworld, [1, 64, 1], "overworld");
        put_entity(&nether, [2, 64, 2], "nether");

        let mut merge = Compound::new();
        merge.insert("marker".into(), Tag::String("merged".into()));
        apply_block(
            Some(&nether),
            &BlockData::Merge {
                pos: [2, 64, 2],
                data: Tag::Compound(merge),
            },
        )
        .expect("merge nether block entity");

        let view = crate::plugins::world_view_for_sources([
            (
                crate::plugins::world_id(Dimension::Overworld),
                Arc::clone(&overworld),
            ),
            (
                crate::plugins::world_id(Dimension::Nether),
                Arc::clone(&nether),
            ),
        ]);
        let nether_reply = answer_with_view(
            &call(
                crate::plugins::world_id(Dimension::Nether),
                [2, 64, 2],
                [2, 64, 2],
            ),
            Some(view.as_ref()),
        )
        .unwrap();
        assert_eq!(decoded(&nether_reply), vec![([2, 64, 2], "merged".into())]);

        let over_reply = answer_with_view(
            &call(
                crate::plugins::world_id(Dimension::Overworld),
                [1, 64, 1],
                [1, 64, 1],
            ),
            Some(view.as_ref()),
        )
        .unwrap();
        assert_eq!(decoded(&over_reply), vec![([1, 64, 1], "overworld".into())]);
        assert_eq!(hex(&[0, 10, 255]), "000aff");
    }
}
