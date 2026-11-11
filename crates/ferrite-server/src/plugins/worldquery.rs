//! World reads that take a heightmap or a light query beyond the native block/biome calls:
//! `World#getHighestBlockYAt(x, z, HeightMap)` and `Block#getLightLevel`. They work for every
//! world the live view knows, runtime worlds included, and read the chunk's stored data.
//! Chunk state (`isChunkLoaded`, `isChunkGenerated`, `loadChunk`) is answered here for every
//! world except the default one, whose loaded set `chunks` tracks.

use ferrite_plugin::{HOST_NULL, Heightmap, HostCall};

/// The reply to a heightmap or light query, or `None` for another call.
pub fn answer(call: &HostCall) -> Option<String> {
    answer_with_view(call, ferrite_plugin::world_view().as_deref())
}

fn answer_with_view(
    call: &HostCall,
    view: Option<&dyn ferrite_plugin::WorldView>,
) -> Option<String> {
    if call.owner != "World" {
        return None;
    }
    let world = uuid::Uuid::parse_str(&call.subject).ok();
    let chunk_call = matches!(
        call.method.as_str(),
        "isChunkLoaded(int,int)" | "isChunkGenerated(int,int)" | "loadChunk(int,int)"
    );
    if chunk_call && world == Some(ferrite_plugin::default_world_id()) {
        return None;
    }
    let reply = match (call.method.as_str(), call.args.as_slice()) {
        ("getHighestBlockYAt(int,int,HeightMap)", [x, z, kind]) => (|| {
            let kind = Heightmap::from_bukkit(kind)?;
            view?
                .heightmap(world?, x.parse().ok()?, z.parse().ok()?, kind)
                .map(|y| y.to_string())
        })(),
        ("getLightLevel(int,int,int)", [x, y, z]) => (|| {
            view?
                .light_level(world?, x.parse().ok()?, y.parse().ok()?, z.parse().ok()?)
                .map(|level| level.to_string())
        })(),
        ("isChunkLoaded(int,int)", [x, z]) => (|| {
            view?
                .chunk_loaded(world?, x.parse().ok()?, z.parse().ok()?)
                .map(|state| state.to_string())
        })(),
        ("isChunkGenerated(int,int)", [x, z]) => (|| {
            view?
                .chunk_generated(world?, x.parse().ok()?, z.parse().ok()?)
                .map(|state| state.to_string())
        })(),
        ("loadChunk(int,int)", [x, z]) => (|| {
            view?
                .load_chunk(world?, x.parse().ok()?, z.parse().ok()?)
                .map(|state| state.to_string())
        })(),
        _ => return None,
    };
    Some(reply.unwrap_or_else(|| HOST_NULL.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{AnvilChunkSource, Dimension, RuntimeWorldCatalog, RuntimeWorldRequest};
    use std::sync::Arc;
    use std::time::Duration;
    use uuid::Uuid;

    const LOAD: Duration = Duration::from_secs(10);

    fn call(world: Uuid, method: &str, args: &[&str]) -> HostCall {
        HostCall {
            owner: "World".into(),
            subject: world.to_string(),
            method: method.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    fn state(name: &str) -> u32 {
        u32::from(ferrite_data::blocks::default_state(name).expect(name))
    }

    fn highest(
        view: &dyn ferrite_plugin::WorldView,
        world: Uuid,
        x: i32,
        z: i32,
        kind: &str,
    ) -> String {
        answer_with_view(
            &call(
                world,
                "getHighestBlockYAt(int,int,HeightMap)",
                &[&x.to_string(), &z.to_string(), kind],
            ),
            Some(view),
        )
        .expect("the call is a world query")
    }

    #[tokio::test]
    async fn heightmap_queries_honour_each_vanilla_heightmap() {
        let world = Uuid::from_u128(0x77);
        let source = AnvilChunkSource::new_memory_dimension(None, Dimension::Overworld);
        assert!(source.ensure_chunk_loaded(0, 0, LOAD));
        // Column (1, 1): water at 65, leaves at 67, a torch on top at 68, over the grass at 64.
        source
            .set_block(1, 65, 1, state("minecraft:water"))
            .unwrap();
        source
            .set_block(1, 67, 1, state("minecraft:oak_leaves"))
            .unwrap();
        source
            .set_block(1, 68, 1, state("minecraft:torch"))
            .unwrap();
        // Column (3, 3): two blocks of water over the grass.
        source
            .set_block(3, 65, 3, state("minecraft:water"))
            .unwrap();
        source
            .set_block(3, 66, 3, state("minecraft:water"))
            .unwrap();
        let view = crate::plugins::world_view_for_sources([(world, source)]);
        let view = &*view;

        for (kind, expected) in [
            ("WORLD_SURFACE", "68"),
            ("MOTION_BLOCKING", "67"),
            ("MOTION_BLOCKING_NO_LEAVES", "65"),
            ("OCEAN_FLOOR", "67"),
        ] {
            assert_eq!(
                highest(view, world, 1, 1, kind),
                expected,
                "column A {kind}"
            );
        }
        for (kind, expected) in [
            ("WORLD_SURFACE", "66"),
            ("MOTION_BLOCKING", "66"),
            ("MOTION_BLOCKING_NO_LEAVES", "66"),
            ("OCEAN_FLOOR", "64"),
        ] {
            assert_eq!(
                highest(view, world, 3, 3, kind),
                expected,
                "column B {kind}"
            );
        }
        // An untouched flat column is topped by the grass at 64.
        assert_eq!(highest(view, world, 8, 8, "MOTION_BLOCKING"), "64");
    }

    #[tokio::test]
    async fn heightmap_queries_fail_explicitly_for_bad_input() {
        let world = Uuid::from_u128(0x78);
        let source = AnvilChunkSource::new_memory_dimension(None, Dimension::Overworld);
        let view = crate::plugins::world_view_for_sources([(world, source)]);
        let null = HOST_NULL.to_owned();
        assert_eq!(highest(&*view, world, 0, 0, "NOT_A_HEIGHTMAP"), null);
        assert_eq!(
            highest(&*view, Uuid::from_u128(0x79), 0, 0, "MOTION_BLOCKING"),
            null,
            "unknown world"
        );
        let bad_number = call(
            world,
            "getHighestBlockYAt(int,int,HeightMap)",
            &["x", "0", "MOTION_BLOCKING"],
        );
        assert_eq!(
            answer_with_view(&bad_number, Some(&*view)),
            Some(null.clone())
        );
        let no_view = call(
            world,
            "getHighestBlockYAt(int,int,HeightMap)",
            &["0", "0", "MOTION_BLOCKING"],
        );
        assert_eq!(answer_with_view(&no_view, None), Some(null));
        let other = call(world, "refreshChunk(int,int)", &["0", "0"]);
        assert_eq!(answer_with_view(&other, None), None);
    }

    #[tokio::test]
    async fn light_level_is_the_brighter_of_sky_and_block_light() {
        let world = Uuid::from_u128(0x7a);
        let source = AnvilChunkSource::new_memory_dimension(None, Dimension::Overworld);
        assert!(source.ensure_chunk_loaded(0, 0, LOAD));
        let view = crate::plugins::world_view_for_sources([(world, Arc::clone(&source))]);
        let level = |x: &str, y: &str, z: &str| {
            answer_with_view(
                &call(world, "getLightLevel(int,int,int)", &[x, y, z]),
                Some(&*view),
            )
            .unwrap()
        };
        assert_eq!(
            level("2", "70", "2"),
            "15",
            "open sky above the flat ground"
        );
        assert_eq!(level("2", "60", "2"), "0", "underground has no light");
        assert_eq!(level("zz", "70", "2"), HOST_NULL);
    }

    /// A runtime world's chunk that nobody has loaded is loaded for the read instead of failing.
    #[tokio::test]
    async fn reads_on_an_unloaded_runtime_world_chunk_load_it() {
        let Some(vanilla) = vanilla_root() else {
            return;
        };
        let registry = Arc::new(ferrite_world::RuntimeRegistry::load(vanilla).unwrap());
        let dir = tempfile::tempdir().unwrap();
        let mut catalog = RuntimeWorldCatalog::default();
        let runtime = catalog
            .open_persistent(
                "reader",
                dir.path(),
                registry,
                None,
                RuntimeWorldRequest {
                    dimension: Dimension::Overworld,
                    spawn: [0, 65, 0],
                    seed: 3,
                },
            )
            .unwrap();
        // Far from anything a player would hold; nothing has asked for it yet.
        let (x, z) = (16 * 40 + 5, -16 * 40 + 9);
        assert_eq!(
            runtime.source.block(x, 64, z),
            None,
            "precondition: the chunk is not resident"
        );
        let view =
            crate::plugins::world_view_for_sources([(runtime.id, Arc::clone(&runtime.source))]);

        let ground = view.block(runtime.id, x, 64, z).expect("the chunk loads");
        assert!(ground.starts_with("minecraft:grass_block"), "{ground}");
        assert_eq!(
            view.block(runtime.id, x, 70, z).as_deref(),
            Some("minecraft:air")
        );
        assert!(view.biome(runtime.id, x, 64, z).is_some());
        assert_eq!(highest(&*view, runtime.id, x, z, "MOTION_BLOCKING"), "64");
        assert_eq!(view.sky_light(runtime.id, x, 70, z), Some(15));
        assert_eq!(view.highest_block(runtime.id, x, z), Some(64));

        // A second unloaded chunk answers through the host-call layer too.
        let (x2, z2) = (16 * -33 + 1, 16 * 50 + 2);
        assert_eq!(runtime.source.block(x2, 64, z2), None);
        let light = call(
            runtime.id,
            "getLightLevel(int,int,int)",
            &[&x2.to_string(), "70", &z2.to_string()],
        );
        assert_eq!(
            answer_with_view(&light, Some(&*view)).as_deref(),
            Some("15")
        );
    }

    fn chunk_state(
        view: &dyn ferrite_plugin::WorldView,
        world: Uuid,
        method: &str,
        x: i32,
        z: i32,
    ) -> String {
        answer_with_view(
            &call(world, method, &[&x.to_string(), &z.to_string()]),
            Some(view),
        )
        .expect("a chunk state call")
    }

    #[tokio::test]
    async fn is_chunk_generated_is_false_until_the_chunk_exists() {
        let world = Uuid::from_u128(0x7b);
        let source = AnvilChunkSource::new_memory_dimension(None, Dimension::Overworld);
        let view = crate::plugins::world_view_for_sources([(world, Arc::clone(&source))]);
        let generated = |x, z| chunk_state(&*view, world, "isChunkGenerated(int,int)", x, z);
        assert_eq!(generated(40, -40), "false", "nothing has created it");
        // Asking never creates the chunk.
        assert_eq!(generated(40, -40), "false");
        assert!(!source.chunk_resident(40, -40));
        assert_eq!(
            chunk_state(&*view, world, "loadChunk(int,int)", 40, -40),
            "true"
        );
        assert_eq!(generated(40, -40), "true");
        assert_eq!(generated(41, -40), "false", "a neighbour is still missing");
    }

    #[tokio::test]
    async fn load_chunk_makes_it_loaded_and_its_blocks_readable() {
        let world = Uuid::from_u128(0x7c);
        let source = AnvilChunkSource::new_memory_dimension(None, Dimension::Overworld);
        let view = crate::plugins::world_view_for_sources([(world, Arc::clone(&source))]);
        assert_eq!(
            chunk_state(&*view, world, "isChunkLoaded(int,int)", 7, 9),
            "false"
        );
        assert_eq!(
            chunk_state(&*view, world, "loadChunk(int,int)", 7, 9),
            "true"
        );
        assert_eq!(
            chunk_state(&*view, world, "isChunkLoaded(int,int)", 7, 9),
            "true"
        );
        assert!(
            source.block(7 * 16 + 1, 64, 9 * 16 + 1).is_some(),
            "the block is readable without a further load"
        );
        assert_eq!(
            view.block(world, 7 * 16 + 1, 64, 9 * 16 + 1).as_deref(),
            Some("minecraft:grass_block[snowy=false]")
        );
    }

    #[tokio::test]
    async fn a_chunk_that_was_never_loaded_is_not_loaded() {
        let world = Uuid::from_u128(0x7d);
        let source = AnvilChunkSource::new_memory_dimension(None, Dimension::Overworld);
        assert!(source.ensure_chunk_loaded(0, 0, LOAD));
        let view = crate::plugins::world_view_for_sources([(world, source)]);
        assert_eq!(
            chunk_state(&*view, world, "isChunkLoaded(int,int)", 0, 0),
            "true"
        );
        assert_eq!(
            chunk_state(&*view, world, "isChunkLoaded(int,int)", 1, 0),
            "false"
        );
        assert_eq!(
            chunk_state(&*view, world, "isChunkLoaded(int,int)", -5, 12),
            "false"
        );
        // Unknown world and bad numbers answer null; the default world is left to `chunks`.
        let unknown = call(Uuid::from_u128(0x7e), "isChunkLoaded(int,int)", &["0", "0"]);
        assert_eq!(
            answer_with_view(&unknown, Some(&*view)).as_deref(),
            Some(HOST_NULL)
        );
        let bad = call(world, "isChunkLoaded(int,int)", &["x", "0"]);
        assert_eq!(
            answer_with_view(&bad, Some(&*view)).as_deref(),
            Some(HOST_NULL)
        );
        let default = call(
            ferrite_plugin::default_world_id(),
            "isChunkLoaded(int,int)",
            &["0", "0"],
        );
        assert_eq!(answer_with_view(&default, Some(&*view)), None);
    }

    fn vanilla_root() -> Option<std::path::PathBuf> {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3/generated");
        root.join("reports/blocks.json").exists().then_some(root)
    }
}
