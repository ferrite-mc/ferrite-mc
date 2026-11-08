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
    let world = match uuid::Uuid::parse_str(&call.subject) {
        Ok(world) => world,
        Err(_) => return Some(HOST_NULL.to_owned()),
    };
    let Some(n) = call
        .args
        .iter()
        .map(|a| a.parse().ok())
        .collect::<Option<Vec<i32>>>()
    else {
        return Some(HOST_NULL.to_owned());
    };
    let Ok([x1, y1, z1, x2, y2, z2]) = <[i32; 6]>::try_from(n) else {
        return Some(HOST_NULL.to_owned());
    };
    let (min, max) = ([x1, y1, z1], [x2, y2, z2]);
    if (0..3).any(|axis| min[axis] > max[axis]) {
        return Some(HOST_NULL.to_owned());
    }
    let Some(view) = view else {
        return Some(HOST_NULL.to_owned());
    };
    let entities = match view.block_entities(world, min, max) {
        Ok(entities) => entities,
        Err(error) => {
            tracing::debug!(?error, %world, "plugin block entity query failed");
            return Some(HOST_NULL.to_owned());
        }
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
            match source.get(0, 0, 0) {
                Some(Ok(_)) => return,
                Some(Err(error)) => panic!("chunk load failed: {error}"),
                None => tokio::time::sleep(std::time::Duration::from_millis(5)).await,
            }
        }
        panic!("chunk did not load");
    }

    fn chest_type() -> u32 {
        ferrite_data::registries::id("minecraft:block_entity_type", "minecraft:chest")
            .and_then(|id| u32::try_from(id).ok())
            .expect("chest block entity type")
    }

    fn vanilla_root() -> Option<std::path::PathBuf> {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3/generated");
        root.join("reports/blocks.json").exists().then_some(root)
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ferrite-plugins-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn put_entity(source: &AnvilChunkSource, [x, y, z]: [i32; 3], marker: &str) {
        let type_id = chest_type();
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

    struct UnreadableView;

    impl ferrite_plugin::WorldView for UnreadableView {
        fn block(&self, _: uuid::Uuid, _: i32, _: i32, _: i32) -> Option<String> {
            None
        }
        fn sky_light(&self, _: uuid::Uuid, _: i32, _: i32, _: i32) -> Option<u8> {
            None
        }
        fn highest_block(&self, _: uuid::Uuid, _: i32, _: i32) -> Option<i32> {
            None
        }
        fn biome(&self, _: uuid::Uuid, _: i32, _: i32, _: i32) -> Option<String> {
            None
        }
        fn block_entities(
            &self,
            _: uuid::Uuid,
            _: [i32; 3],
            _: [i32; 3],
        ) -> Result<Vec<ferrite_plugin::BlockEntityData>, ferrite_plugin::BlockEntityError>
        {
            Err(ferrite_plugin::BlockEntityError::Unreadable)
        }
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn block_entity_query_failures_are_not_empty_successes() {
        let overworld = AnvilChunkSource::new_memory_dimension(None, Dimension::Overworld);
        let world = crate::plugins::world_id(Dimension::Overworld);
        let view = crate::plugins::world_view_for_sources([(world, Arc::clone(&overworld))]);

        // Chunk (0, 0) has not been requested yet, so the query is unanswerable.
        assert_eq!(
            answer_with_view(&call(world, [0; 3], [15, 319, 15]), Some(view.as_ref())).as_deref(),
            Some(HOST_NULL)
        );

        ready(&overworld).await;
        put_entity(&overworld, [1, 64, 1], "ok");

        // A loaded chunk with no block entities in the box is a real empty result.
        assert_eq!(
            answer_with_view(&call(world, [8, 64, 8], [10, 70, 10]), Some(view.as_ref()))
                .as_deref(),
            Some("")
        ); // The adapter reports an unreadable block entity as a failed query, not as no entities.
        let unreadable: &dyn ferrite_plugin::WorldView = &UnreadableView;
        assert_eq!(
            answer_with_view(&call(world, [0; 3], [15, 319, 15]), Some(unreadable)).as_deref(),
            Some(HOST_NULL)
        );

        // Invalid subjects, argument counts and boxes are failed queries, not panics.
        let mut invalid_subject = call(world, [0; 3], [1; 3]);
        invalid_subject.subject = "not-a-uuid".into();
        assert_eq!(
            answer_with_view(&invalid_subject, Some(view.as_ref())).as_deref(),
            Some(HOST_NULL)
        );
        let mut short = call(world, [0; 3], [1; 3]);
        short.args.truncate(3);
        assert_eq!(
            answer_with_view(&short, Some(view.as_ref())).as_deref(),
            Some(HOST_NULL)
        );
        assert_eq!(
            answer_with_view(&call(world, [9; 3], [8; 3]), Some(view.as_ref())).as_deref(),
            Some(HOST_NULL)
        );

        // A box larger than the host's chunk budget is refused before any chunk is inspected.
        assert_eq!(
            answer_with_view(
                &call(world, [0; 3], [1_000_000, 319, 16]),
                Some(view.as_ref())
            )
            .as_deref(),
            Some(HOST_NULL)
        );

        // A merge into a block with no block entity is an error, not a no-op.
        assert!(
            apply_block(
                Some(&overworld),
                &BlockData::Merge {
                    pos: [9, 64, 9],
                    data: Tag::Compound(Compound::new()),
                }
            )
            .is_err()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn block_entity_edits_survive_reload_in_two_dimensions() {
        let Some(root) = vanilla_root() else {
            eprintln!("SKIP block entity persistence test: vanilla/26.3/generated is absent");
            return;
        };
        let dir = temp_dir("multiworld-block-entities");
        let registry = Arc::new(ferrite_world::RuntimeRegistry::load(&root).unwrap());
        let overworld = AnvilChunkSource::with_dimension(
            dir.clone(),
            Arc::clone(&registry),
            None,
            Dimension::Overworld,
        );
        let nether = AnvilChunkSource::with_dimension(
            dir.clone(),
            Arc::clone(&registry),
            None,
            Dimension::Nether,
        );
        ready(&overworld).await;
        ready(&nether).await;
        let chest = u32::from(
            ferrite_data::blocks::default_state("chest").expect("vanilla chest block state"),
        );
        for (source, marker) in [(&overworld, "overworld"), (&nether, "nether")] {
            source.set_block(4, 64, 4, chest).unwrap();
            put_entity(source, [4, 64, 4], marker);
        }

        // The plugin merge names one world; the same coordinates in the other world stay put.
        for (source, marker) in [(&overworld, "overworld-edited"), (&nether, "nether-edited")] {
            let mut merge = Compound::new();
            merge.insert("marker".into(), Tag::String(marker.into()));
            apply_block(
                Some(source.as_ref()),
                &BlockData::Merge {
                    pos: [4, 64, 4],
                    data: Tag::Compound(merge),
                },
            )
            .expect("merge block entity in its own world");
        }
        overworld.flush_chunk(0, 0).await.unwrap();
        nether.flush_chunk(0, 0).await.unwrap();
        drop(overworld);
        drop(nether);

        // "Restart": reopen the same world root and read each dimension's own storage tree.
        let registry = Arc::new(ferrite_world::RuntimeRegistry::load(&root).unwrap());
        let overworld = AnvilChunkSource::with_dimension(
            dir.clone(),
            Arc::clone(&registry),
            None,
            Dimension::Overworld,
        );
        let nether =
            AnvilChunkSource::with_dimension(dir.clone(), registry, None, Dimension::Nether);
        ready(&overworld).await;
        ready(&nether).await;
        let view = crate::plugins::world_view_for_sources([
            (crate::plugins::world_id(Dimension::Overworld), overworld),
            (crate::plugins::world_id(Dimension::Nether), nether),
        ]);
        let read = |dimension: Dimension| {
            let world = crate::plugins::world_id(dimension);
            let reply = answer_with_view(&call(world, [4, 64, 4], [4, 64, 4]), Some(view.as_ref()))
                .unwrap();
            decoded(&reply)
        };
        assert_eq!(
            read(Dimension::Overworld),
            vec![([4, 64, 4], "overworld-edited".into())]
        );
        assert_eq!(
            read(Dimension::Nether),
            vec![([4, 64, 4], "nether-edited".into())]
        );
        drop(view);
        let _ = std::fs::remove_dir_all(dir);
    }
}
