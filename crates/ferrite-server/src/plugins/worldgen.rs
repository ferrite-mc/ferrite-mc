//! Plugin calls answered by the vanilla world generation host: biome and structure queries.
//!
//! Only the overworld has a console target for plugins, so these accept the default world.
//! Without a vanilla generator (flat test world) every call returns `None`, which the plugin
//! sees as "not implemented" rather than a made-up answer.

use super::is_resource_key;
use crate::game::{ChunkGenerator, Dimension};
use ferrite_plugin::{HOST_NULL, HostCall};

/// A search radius in blocks; vanilla's `/locate` uses 6,400 for biomes and 100 chunks for structures.
const MAX_RADIUS: i32 = 12_800;

fn target(text: &str) -> Option<&str> {
    let key = text.strip_prefix('#').unwrap_or(text);
    is_resource_key(key).then_some(text)
}

fn position((x, y, z): (i32, i32, i32)) -> String {
    format!("{x} {y} {z}")
}

/// The reply for a `World#...` biome or structure call, or `None` when it is not one of them or
/// the generator cannot answer. An empty reply means "searched, nothing found".
pub fn world_query(terrain: &dyn ChunkGenerator, call: &HostCall) -> Option<String> {
    if call.owner != "World" {
        return None;
    }
    let world = uuid::Uuid::parse_str(&call.subject).ok()?;
    if world != ferrite_plugin::default_world_id() {
        return None;
    }
    let arg = |index: usize| {
        call.args
            .get(index)
            .map(String::as_str)
            .filter(|a| *a != HOST_NULL)
    };
    let int = |index: usize| arg(index)?.parse::<i32>().ok();
    let radius = |index: usize| int(index).filter(|r| (1..=MAX_RADIUS).contains(r));
    match call.method.as_str() {
        "locateBiome(String,int,int,int)" => {
            let found = terrain
                .locate_biome(
                    Dimension::Overworld,
                    target(arg(0)?)?,
                    int(2)?,
                    int(3)?,
                    radius(1)?,
                )
                .ok()?;
            Some(found.map(position).unwrap_or_default())
        }
        "locateStructure(String,int,int,int)" => {
            let found = terrain
                .locate_structure(
                    Dimension::Overworld,
                    target(arg(0)?)?,
                    int(2)?,
                    int(3)?,
                    radius(1)?,
                )
                .ok()?;
            Some(found.map(position).unwrap_or_default())
        }
        "hasStructureAt(String,int,int,int)" => {
            let key = arg(0).filter(|key| is_resource_key(key))?;
            let inside = terrain
                .structure_at(Dimension::Overworld, key, int(1)?, int(2)?, int(3)?)
                .ok()?;
            Some(inside.to_string())
        }
        "getBiomeKey(int,int,int)" => terrain
            .biome_at(Dimension::Overworld, int(0)?, int(1)?, int(2)?)
            .ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Generated;

    struct Fake;
    impl ChunkGenerator for Fake {
        fn generate(&self, _: Dimension, _: i32, _: i32) -> anyhow::Result<Generated> {
            anyhow::bail!("unused")
        }
        fn spawn_height(&self, _: Dimension, _: i32, _: i32) -> anyhow::Result<i32> {
            Ok(64)
        }
        fn locate_biome(
            &self,
            _: Dimension,
            target: &str,
            x: i32,
            z: i32,
            radius: i32,
        ) -> anyhow::Result<Option<(i32, i32, i32)>> {
            Ok((target == "minecraft:plains").then_some((x + radius, 70, z)))
        }
        fn biome_at(&self, _: Dimension, _: i32, y: i32, _: i32) -> anyhow::Result<String> {
            Ok(if y < 0 {
                "minecraft:deep_dark"
            } else {
                "minecraft:plains"
            }
            .into())
        }
        fn structure_at(
            &self,
            _: Dimension,
            id: &str,
            _: i32,
            _: i32,
            _: i32,
        ) -> anyhow::Result<bool> {
            Ok(id == "minecraft:village_plains")
        }
    }

    fn call(method: &str, args: &[&str]) -> HostCall {
        HostCall {
            owner: "World".into(),
            subject: ferrite_plugin::default_world_id().to_string(),
            method: method.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    #[test]
    fn biome_and_structure_calls_reach_the_generator() {
        assert_eq!(
            world_query(
                &Fake,
                &call(
                    "locateBiome(String,int,int,int)",
                    &["minecraft:plains", "100", "5", "7"]
                )
            ),
            Some("105 70 7".to_owned())
        );
        // Searched and not found is an empty reply, not "unsupported".
        assert_eq!(
            world_query(
                &Fake,
                &call(
                    "locateBiome(String,int,int,int)",
                    &["minecraft:desert", "100", "0", "0"]
                )
            ),
            Some(String::new())
        );
        assert_eq!(
            world_query(
                &Fake,
                &call(
                    "hasStructureAt(String,int,int,int)",
                    &["minecraft:village_plains", "0", "64", "0"]
                )
            ),
            Some("true".to_owned())
        );
        assert_eq!(
            world_query(&Fake, &call("getBiomeKey(int,int,int)", &["0", "-10", "0"])),
            Some("minecraft:deep_dark".to_owned())
        );
    }

    #[test]
    fn bad_input_and_other_worlds_are_not_answered() {
        let locate = "locateBiome(String,int,int,int)";
        assert_eq!(
            world_query(
                &Fake,
                &call(locate, &["minecraft:plains; stop", "100", "0", "0"])
            ),
            None
        );
        assert_eq!(
            world_query(&Fake, &call(locate, &["minecraft:plains", "0", "0", "0"])),
            None
        );
        assert_eq!(
            world_query(
                &Fake,
                &call(locate, &["minecraft:plains", "999999", "0", "0"])
            ),
            None
        );
        let mut other = call(locate, &["minecraft:plains", "10", "0", "0"]);
        other.subject = uuid::Uuid::from_u128(99).to_string();
        assert_eq!(world_query(&Fake, &other), None);
        let mut player = call("getBiomeKey(int,int,int)", &["0", "0", "0"]);
        player.owner = "Player".into();
        assert_eq!(world_query(&Fake, &player), None);
    }
}
