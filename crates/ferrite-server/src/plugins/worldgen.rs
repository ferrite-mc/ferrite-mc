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
        "getStructureStarts(int,int)" => {
            let starts = terrain
                .structure_starts(Dimension::Overworld, int(0)?, int(1)?)
                .ok()?;
            let lines: Vec<String> = starts
                .iter()
                .filter(|(id, _)| is_resource_key(id))
                .map(|(id, b)| {
                    format!("{id} {} {} {} {} {} {}", b[0], b[1], b[2], b[3], b[4], b[5])
                })
                .collect();
            Some(lines.join("\n"))
        }
        "getBiomeKey(int,int,int)" => terrain
            .biome_at(Dimension::Overworld, int(0)?, int(1)?, int(2)?)
            .ok(),
        _ => None,
    }
}

/// `setblock` lines for a state id, keeping its properties (`minecraft:oak_log[axis=y]`).
fn state_text(state: u16) -> Option<String> {
    let (name, props) = ferrite_data::blocks::state(state)?;
    let mut text = name.to_owned();
    if !props.is_empty() {
        let list: Vec<String> = props.iter().map(|(k, v)| format!("{k}={v}")).collect();
        text.push_str(&format!("[{}]", list.join(",")));
    }
    Some(text)
}

/// `World#generateTree`: runs the vanilla feature in the worldgen host and returns the console
/// lines that apply the changed blocks with their full states, plus the reply (`true` only when
/// the host really changed something). `None` when this is not that call or there is no host.
pub fn place_feature(
    terrain: &dyn ChunkGenerator,
    call: &HostCall,
) -> Option<(Vec<String>, String)> {
    if call.owner != "World" || call.method != "placeFeature(Location,String)" {
        return None;
    }
    let world = uuid::Uuid::parse_str(&call.subject).ok()?;
    let dimension = super::world_dimension(world)?;
    let (x, y, z, feature) = feature_request(call)?;
    let blocks = match terrain.place(dimension, &format!("feature;{feature};{x};{y};{z}")) {
        Ok(blocks) => blocks,
        Err(_) => return Some((Vec::new(), "false".into())),
    };
    // The console only addresses the overworld directly; the nether and end need an explicit
    // dimension, otherwise a tree grown there would land in the overworld.
    let prefix = if dimension == Dimension::Overworld {
        String::new()
    } else {
        format!("execute in {} run ", dimension.key())
    };
    let lines: Vec<String> = blocks
        .iter()
        .filter_map(|([bx, by, bz], state)| {
            Some(format!(
                "{prefix}setblock {bx} {by} {bz} {}",
                state_text(*state)?
            ))
        })
        .collect();
    let reply = if lines.is_empty() { "false" } else { "true" };
    Some((lines, reply.into()))
}

/// `World#generateTree(Location, TreeType, BlockChangeDelegate)`: the blocks the feature would
/// change as `x,y,z,state` lines, without touching the world, so Java can replay them into the
/// delegate. `None` for another call or a world the console/generator cannot address.
pub fn place_feature_blocks(terrain: &dyn ChunkGenerator, call: &HostCall) -> Option<String> {
    if call.owner != "World" || call.method != "featureBlocks(Location,String)" {
        return None;
    }
    let world = uuid::Uuid::parse_str(&call.subject).ok()?;
    let dimension = super::world_dimension(world)?;
    let (x, y, z, feature) = feature_request(call)?;
    let blocks = terrain
        .place(dimension, &format!("feature;{feature};{x};{y};{z}"))
        .ok()?;
    Some(
        blocks
            .iter()
            .filter_map(|([bx, by, bz], state)| {
                Some(format!("{bx},{by},{bz},{}", state_text(*state)?))
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// The `(x, y, z, feature)` a `placeFeature`/`featureBlocks` call carries.
fn feature_request(call: &HostCall) -> Option<(i32, i32, i32, String)> {
    let mut parts = call
        .args
        .first()?
        .split(ferrite_plugin::HOST_ITEM_SEPARATOR);
    parts.next()?;
    let mut axis = || {
        let v: f64 = parts.next()?.parse().ok()?;
        (v.is_finite() && v.abs() < 3.0e7).then(|| v.floor() as i32)
    };
    let (x, y, z) = (axis()?, axis()?, axis()?);
    let feature = call.args.get(1)?.to_ascii_lowercase();
    is_resource_key(&feature).then_some((x, y, z, feature))
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
        fn structure_starts(
            &self,
            _: Dimension,
            x: i32,
            z: i32,
        ) -> anyhow::Result<Vec<(String, [i32; 6])>> {
            Ok(if (x, z) == (0, 0) {
                vec![("minecraft:village_plains".into(), [0, 60, 0, 20, 75, 20])]
            } else {
                Vec::new()
            })
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
        fn place(&self, _: Dimension, argument: &str) -> anyhow::Result<crate::game::Placed> {
            if argument.starts_with("feature;minecraft:oak;") {
                let log = ferrite_data::blocks::state_id("minecraft:oak_log", &[("axis", "y")]);
                Ok(crate::game::Placed {
                    blocks: vec![([1, 65, -2], log.unwrap())],
                    entities: Vec::new(),
                })
            } else if argument.starts_with("feature;minecraft:empty;") {
                Ok(crate::game::Placed::default())
            } else {
                anyhow::bail!("no such feature")
            }
        }
    }

    #[test]
    fn generate_tree_applies_the_host_blocks_with_their_states() {
        let at = format!(
            "w{0}1.5{0}64{0}-2{0}0{0}0",
            ferrite_plugin::HOST_ITEM_SEPARATOR
        );
        let place = |feature: &str| {
            place_feature(
                &Fake,
                &call("placeFeature(Location,String)", &[&at, feature]),
            )
        };
        let (lines, reply) = place("minecraft:oak").unwrap();
        assert_eq!(lines, vec!["setblock 1 65 -2 minecraft:oak_log[axis=y]"]);
        assert_eq!(reply, "true");
        // A tree in the nether names its dimension, so it cannot land in the overworld.
        let mut nether = call("placeFeature(Location,String)", &[&at, "minecraft:oak"]);
        nether.subject = crate::plugins::world_id(Dimension::Nether).to_string();
        let (lines, reply) = place_feature(&Fake, &nether).unwrap();
        assert_eq!(
            lines,
            vec!["execute in minecraft:the_nether run setblock 1 65 -2 minecraft:oak_log[axis=y]"]
        );
        assert_eq!(reply, "true");
        // The delegate overload needs the blocks without the world changing.
        assert_eq!(
            place_feature_blocks(
                &Fake,
                &call("featureBlocks(Location,String)", &[&at, "minecraft:oak"])
            ),
            Some("1,65,-2,minecraft:oak_log[axis=y]".to_owned())
        );
        assert_eq!(
            place("minecraft:empty").unwrap(),
            (Vec::new(), "false".into())
        );
        assert_eq!(
            place("minecraft:nope").unwrap(),
            (Vec::new(), "false".into())
        );
        assert!(place("bad key").is_none());
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
            world_query(&Fake, &call("getStructureStarts(int,int)", &["0", "0"])),
            Some("minecraft:village_plains 0 60 0 20 75 20".to_owned())
        );
        assert_eq!(
            world_query(&Fake, &call("getStructureStarts(int,int)", &["5", "5"])),
            Some(String::new())
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
