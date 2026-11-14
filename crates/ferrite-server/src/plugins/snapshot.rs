//! Entity snapshots for Bukkit's `EntityFactory#createEntitySnapshot` and
//! `Entity#createSnapshot`.
//!
//! The host is the only side that can parse SNBT, so a factory request is validated and reduced
//! to the plain values Bukkit exposes: type, position, UUID, health, velocity and custom name.
//! A malformed snapshot is answered with [`INVALID`] instead of a fake entity.

use ferrite_nbt::{List, Tag};
use ferrite_plugin::{HOST_ITEM_SEPARATOR, HOST_NULL, HostCall};
use uuid::Uuid;

/// The reply for SNBT that does not describe a known entity; Java turns it into
/// `IllegalArgumentException`, the exception Bukkit declares for a bad snapshot string.
pub(super) const INVALID: &str = "\u{1b}invalid";

/// The host's answer to an entity-snapshot request.
pub fn answer(call: &HostCall) -> Option<String> {
    match (call.owner.as_str(), call.method.as_str()) {
        ("EntityFactory", "createEntitySnapshot(String)") => Some(match call.args.first() {
            Some(input) => from_snbt(input),
            None => INVALID.to_owned(),
        }),
        ("Entity", "getEntitySnapshot()") => super::facts::entity_snapshot(&call.subject),
        ("World", "ferriteEntities(int,int,int,int)") => world_entities(call),
        _ => None,
    }
}

/// Separates the fields of one record in a [`world_entities`] reply; records are separated by
/// [`HOST_ITEM_SEPARATOR`] like every list reply.
pub(super) const FIELD: char = '\u{1e}';

/// The most chunks one `ferriteEntities` request may cover. A structure is at most 48 blocks per
/// side (4 x 4 chunks); this leaves room for padding without letting a plugin walk the world.
const MAX_CHUNKS: i64 = 1024;

/// `World#ferriteEntities(minChunkX, minChunkZ, maxChunkX, maxChunkZ)`: every non-player entity of
/// a runtime world whose position lies in the inclusive chunk range, one record per entity as
/// `type, x, y, z, yaw, pitch, uuid, health, vx, vy, vz, name, nbt` separated by [`FIELD`].
///
/// `health` and `name` use [`HOST_NULL`], `nbt` is the base64 SNBT of the entity's saved form.
/// The default world answers too (its NBT is at most a second old); the nether and the end are not
/// addressable by plugins, so they have no records to give.
fn world_entities(call: &HostCall) -> Option<String> {
    let world = Uuid::parse_str(&call.subject).ok()?;
    if call.args.len() != 4 {
        return None;
    }
    let bounds: Vec<i32> = call
        .args
        .iter()
        .map(|arg| arg.parse().ok())
        .collect::<Option<_>>()?;
    let (min_x, min_z, max_x, max_z) = (bounds[0], bounds[1], bounds[2], bounds[3]);
    let width = i64::from(max_x) - i64::from(min_x) + 1;
    let depth = i64::from(max_z) - i64::from(min_z) + 1;
    if width < 1 || depth < 1 || width * depth > MAX_CHUNKS {
        return None;
    }
    Some(super::facts::runtime_snapshots(
        world,
        (min_x, min_z),
        (max_x, max_z),
    ))
}

/// Encodes one entity's NBT as SNBT, for the snapshot a plugin can turn back into an entity.
pub(crate) fn entity_snbt(record: &ferrite_entity::EntityRecord) -> Option<String> {
    let Tag::Compound(root) = crate::game::entities_to_nbt(std::slice::from_ref(record)) else {
        return None;
    };
    let Some(Tag::List(List::Compound(entities))) = root.get("Entities") else {
        return None;
    };
    let entity = entities.first()?;
    Some(ferrite_nbt::snbt::format(&Tag::Compound(entity.clone())))
}

/// A live entity record for `snbt`, placed at `pos` facing `yaw` and carrying `uuid`.
///
/// The UUID and position come from the caller so a snapshot placed twice never produces two
/// entities with the same identity. `None` when the SNBT is not a known, loadable entity.
pub(crate) fn record_from_snbt(
    snbt: &str,
    uuid: Uuid,
    pos: [f64; 3],
    yaw: f32,
) -> Option<ferrite_entity::EntityRecord> {
    let Ok(Tag::Compound(entity)) = ferrite_nbt::snbt::parse(snbt) else {
        return None;
    };
    let mut root = ferrite_nbt::Compound::new();
    root.insert("Entities".into(), Tag::List(List::Compound(vec![entity])));
    let mut record = crate::game::entities_from_nbt(&Tag::Compound(root))
        .into_iter()
        .next()?;
    record.pos = ferrite_math::Vec3::new(pos[0], pos[1], pos[2]);
    record.data.uuid = uuid;
    record.data.yaw = yaw;
    Some(record)
}

/// `World#ferriteSpawnSnapshot(snbt, uuid, x, y, z, yaw)` on a runtime world: puts the entity the
/// SNBT describes into the world at the given spot under the given UUID.
pub fn command(call: &HostCall) -> Option<ferrite_plugin::PlayerCommand> {
    if call.owner != "World"
        || call.method != "ferriteSpawnSnapshot(String,String,double,double,double,double)"
    {
        return None;
    }
    let world = Uuid::parse_str(&call.subject).ok()?;
    if call.args.len() != 6 {
        return None;
    }
    let uuid = Uuid::parse_str(&call.args[1]).ok()?;
    let mut numbers = [0.0f64; 4];
    for (slot, arg) in numbers.iter_mut().zip(&call.args[2..]) {
        *slot = arg.parse().ok().filter(|value: &f64| value.is_finite())?;
    }
    let pos = [numbers[0], numbers[1], numbers[2]];
    if pos.iter().any(|value| value.abs() > 30_000_000.0) {
        return None;
    }
    // Validate here so a bad snapshot is refused with an error reply instead of vanishing later.
    record_from_snbt(&call.args[0], uuid, pos, numbers[3] as f32)?;
    Some(ferrite_plugin::PlayerCommand::SpawnSnapshot {
        world,
        snbt: call.args[0].clone(),
        uuid,
        pos,
        yaw: numbers[3] as f32,
    })
}

/// Parses one entity's SNBT and encodes what Bukkit's snapshot can carry.
pub fn from_snbt(input: &str) -> String {
    let Ok(Tag::Compound(root)) = ferrite_nbt::snbt::parse(input) else {
        return INVALID.to_owned();
    };
    let Some(Tag::String(id)) = root.get("id") else {
        return INVALID.to_owned();
    };
    let Some(kind) = ferrite_data::entity_props::by_name(id) else {
        return INVALID.to_owned();
    };
    let pos = numbers(root.get("Pos"))
        .filter(|values| values.len() == 3)
        .map_or([0.0; 3], |values| [values[0], values[1], values[2]]);
    let rotation = numbers(root.get("Rotation"))
        .filter(|values| values.len() == 2)
        .map_or([0.0; 2], |values| [values[0], values[1]]);
    let velocity = numbers(root.get("Motion"))
        .filter(|values| values.len() == 3)
        .map_or([0.0; 3], |values| [values[0], values[1], values[2]]);
    let uuid = match root.get("UUID") {
        Some(Tag::IntArray(words)) => uuid_from_words(words),
        _ => None,
    };
    let health = number(root.get("Health")).filter(|health| health.is_finite() && *health >= 0.0);
    let name = match root.get("CustomName") {
        Some(Tag::String(name)) => Some(name.as_str()),
        _ => None,
    };
    wire(kind.key, pos, rotation, uuid, health, velocity, name)
}

/// Encodes the snapshot as `type, pos, rotation, uuid, health, velocity, name`.
///
/// Empty values use [`HOST_NULL`]; the custom name is base64 so it can never collide with the
/// field separator.
pub(super) fn wire(
    kind: &str,
    pos: [f64; 3],
    rotation: [f64; 2],
    uuid: Option<Uuid>,
    health: Option<f64>,
    velocity: [f64; 3],
    name: Option<&str>,
) -> String {
    use base64::Engine as _;
    let sep = HOST_ITEM_SEPARATOR;
    let uuid = uuid.map_or_else(|| HOST_NULL.to_owned(), |uuid| uuid.to_string());
    let health = health
        .filter(|health| health.is_finite())
        .map_or_else(|| HOST_NULL.to_owned(), |health| health.to_string());
    let name = name.map_or_else(
        || HOST_NULL.to_owned(),
        |name| base64::engine::general_purpose::STANDARD.encode(name),
    );
    format!(
        "{kind}{sep}{}{sep}{}{sep}{}{sep}{}{sep}{}{sep}{uuid}{sep}{health}{sep}{}{sep}{}{sep}{}{sep}{name}",
        pos[0], pos[1], pos[2], rotation[0], rotation[1], velocity[0], velocity[1], velocity[2]
    )
}

/// The doubles of an NBT number list (float, double or integer lists all count).
fn numbers(tag: Option<&Tag>) -> Option<Vec<f64>> {
    Some(match tag? {
        Tag::List(List::Double(values)) => values.clone(),
        Tag::List(List::Float(values)) => values.iter().map(|value| f64::from(*value)).collect(),
        Tag::List(List::Int(values)) => values.iter().map(|value| f64::from(*value)).collect(),
        Tag::List(List::Long(values)) => values.iter().map(|value| *value as f64).collect(),
        _ => return None,
    })
}

fn number(tag: Option<&Tag>) -> Option<f64> {
    Some(match tag? {
        Tag::Byte(value) => f64::from(*value),
        Tag::Short(value) => f64::from(*value),
        Tag::Int(value) => f64::from(*value),
        Tag::Long(value) => *value as f64,
        Tag::Float(value) => f64::from(*value),
        Tag::Double(value) => *value,
        _ => return None,
    })
}

/// Vanilla stores a UUID as four big-endian ints.
fn uuid_from_words(words: &[i32]) -> Option<Uuid> {
    if words.len() != 4 {
        return None;
    }
    let mut bytes = [0u8; 16];
    for (index, word) in words.iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&(*word as u32).to_be_bytes());
    }
    Some(Uuid::from_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(wire: &str) -> Vec<&str> {
        wire.split(HOST_ITEM_SEPARATOR).collect()
    }

    #[test]
    fn snbt_values_become_the_snapshot_wire() {
        let wire = from_snbt(
            r#"{id:"minecraft:pig", Pos:[1.5d, 64.0d, 2.5d], Rotation:[90.0f, 10.0f],
                UUID:[I;0, 0, 0, 9], Health:16.0f, Motion:[0.0d, 0.5d, 0.0d], CustomName:"Piggy"}"#,
        );
        let fields = parts(&wire);
        assert_eq!(fields.len(), 12, "{wire}");
        assert_eq!(fields[0], "minecraft:pig");
        assert_eq!(&fields[1..4], &["1.5", "64", "2.5"]);
        assert_eq!(&fields[4..6], &["90", "10"]);
        assert_eq!(fields[6], "00000000-0000-0000-0000-000000000009", "{wire}");
        assert_eq!(fields[7], "16");
        assert_eq!(&fields[8..11], &["0", "0.5", "0"]);
        use base64::Engine as _;
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(fields[11])
                .unwrap(),
            b"Piggy"
        );
    }

    #[test]
    fn missing_optional_values_use_the_null_marker() {
        let wire = from_snbt(r#"{id:"minecraft:armor_stand"}"#);
        let fields = parts(&wire);
        assert_eq!(fields[0], "minecraft:armor_stand");
        assert_eq!(fields[6], HOST_NULL);
        assert_eq!(fields[7], HOST_NULL);
        assert_eq!(fields[11], HOST_NULL);
    }

    #[test]
    fn malformed_or_unknown_snapshots_are_rejected() {
        assert_eq!(from_snbt("{}"), INVALID);
        assert_eq!(from_snbt("not nbt"), INVALID);
        assert_eq!(from_snbt(r#"{id:"minecraft:not_a_mob"}"#), INVALID);
        assert_eq!(from_snbt(r#"{id:3}"#), INVALID);
    }

    #[test]
    fn the_bridge_answers_both_snapshot_routes() {
        let factory = HostCall {
            owner: "EntityFactory".into(),
            subject: String::new(),
            method: "createEntitySnapshot(String)".into(),
            args: vec![r#"{id:"minecraft:pig"}"#.into()],
        };
        assert!(answer(&factory).is_some_and(|reply| reply.starts_with("minecraft:pig")));
        let missing = HostCall {
            owner: "EntityFactory".into(),
            subject: String::new(),
            method: "createEntitySnapshot(String)".into(),
            args: vec![],
        };
        assert_eq!(answer(&missing).as_deref(), Some(INVALID));
        let gone = HostCall {
            owner: "Entity".into(),
            subject: Uuid::from_u128(123).to_string(),
            method: "getEntitySnapshot()".into(),
            args: vec![],
        };
        assert_eq!(answer(&gone), None);
    }

    use crate::game::{Dimension, WorldId};
    use ferrite_entity::{EntityStore, Rng, mob::MobKind};
    use ferrite_math::Vec3;

    fn zombie(uuid: Uuid) -> ferrite_entity::EntityRecord {
        let mut record = ferrite_entity::mob::new_mob(
            MobKind::Zombie,
            Vec3::new(12.5, 64.0, -3.5),
            &mut Rng::default(),
        );
        record.data.uuid = uuid;
        record.data.yaw = 45.0;
        record
    }

    fn ask(world: Uuid, args: [&str; 4]) -> Option<String> {
        answer(&HostCall {
            owner: "World".into(),
            subject: world.to_string(),
            method: "ferriteEntities(int,int,int,int)".into(),
            args: args.map(str::to_owned).to_vec(),
        })
    }

    #[test]
    fn a_runtime_world_lists_its_entities_with_nbt_inside_the_chunk_range() {
        let world = Uuid::new_v4();
        let uuid = Uuid::new_v4();
        let mut store = EntityStore::new(3);
        store.insert(zombie(uuid));
        let mut owned = std::collections::HashSet::new();
        super::super::facts::publish_entities(&mut owned, &store, WorldId::Runtime(world));

        let reply = ask(world, ["0", "-1", "0", "-1"]).expect("runtime worlds answer");
        let records: Vec<_> = reply.split(HOST_ITEM_SEPARATOR).collect();
        assert_eq!(records.len(), 1, "{reply:?}");
        let fields: Vec<_> = records[0].split(FIELD).collect();
        assert_eq!(fields.len(), 13, "{fields:?}");
        assert_eq!(fields[0], "minecraft:zombie");
        assert_eq!(&fields[1..4], &["12.5", "64", "-3.5"]);
        assert_eq!(fields[4], "45");
        assert_eq!(fields[6], uuid.to_string());
        assert_ne!(fields[7], HOST_NULL, "a zombie has health");
        use base64::Engine as _;
        let nbt = base64::engine::general_purpose::STANDARD
            .decode(fields[12])
            .unwrap();
        let Ok(Tag::Compound(root)) = ferrite_nbt::snbt::parse(std::str::from_utf8(&nbt).unwrap())
        else {
            panic!("the record carries SNBT");
        };
        assert_eq!(
            root.get("id"),
            Some(&Tag::String("minecraft:zombie".to_owned()))
        );

        // Outside the range, in another world, or for an unsupported request: nothing.
        assert_eq!(ask(world, ["5", "5", "6", "6"]).as_deref(), Some(""));
        assert_eq!(
            ask(Uuid::new_v4(), ["0", "-1", "0", "-1"]).as_deref(),
            Some("")
        );
        assert_eq!(ask(world, ["0", "0", "100", "100"]), None, "range cap");
        assert_eq!(ask(world, ["1", "0", "0", "0"]), None, "inverted range");

        let empty = EntityStore::new(3);
        super::super::facts::publish_entities(&mut owned, &empty, WorldId::Runtime(world));
        assert_eq!(ask(world, ["0", "-1", "0", "-1"]).as_deref(), Some(""));
    }

    #[test]
    fn overworld_entities_stay_out_of_runtime_world_listings() {
        let world = Uuid::new_v4();
        let mut store = EntityStore::new(3);
        store.insert(zombie(Uuid::new_v4()));
        let mut owned = std::collections::HashSet::new();
        super::super::facts::publish_entities(
            &mut owned,
            &store,
            WorldId::Vanilla(Dimension::Overworld),
        );
        assert_eq!(ask(world, ["0", "-1", "0", "-1"]).as_deref(), Some(""));
        let empty = EntityStore::new(3);
        super::super::facts::publish_entities(&mut owned, &empty, Dimension::Overworld);
    }

    #[test]
    fn nether_and_end_entities_are_listed_under_their_own_world_ids() {
        for dimension in [Dimension::Nether, Dimension::End] {
            let uuid = Uuid::new_v4();
            let mut store = EntityStore::new(3);
            store.insert(zombie(uuid));
            let mut owned = std::collections::HashSet::new();
            super::super::facts::publish_entities(&mut owned, &store, dimension);
            let reply = ask(crate::plugins::world_id(dimension), ["0", "-1", "0", "-1"])
                .unwrap_or_else(|| panic!("{dimension:?} answers"));
            let fields = reply
                .split(HOST_ITEM_SEPARATOR)
                .map(|record| record.split(FIELD).map(str::to_owned).collect::<Vec<_>>())
                .find(|fields| fields.get(6) == Some(&uuid.to_string()))
                .unwrap_or_else(|| panic!("{dimension:?} lists its entity: {reply:?}"));
            assert_eq!(fields.len(), 13, "{fields:?}");
            assert_ne!(
                fields[12], HOST_NULL,
                "{dimension:?} has saved NBT for capture"
            );
            let empty = EntityStore::new(3);
            super::super::facts::publish_entities(&mut owned, &empty, dimension);
            assert!(
                !ask(crate::plugins::world_id(dimension), ["0", "-1", "0", "-1"])
                    .expect("still answers")
                    .contains(&uuid.to_string())
            );
        }
    }

    #[test]
    fn the_default_world_lists_its_entities_with_cached_nbt() {
        let uuid = Uuid::new_v4();
        let mut store = EntityStore::new(3);
        store.insert(zombie(uuid));
        let mut owned = std::collections::HashSet::new();
        let overworld = WorldId::Vanilla(Dimension::Overworld);
        // Other tests publish to the shared default world too, so look the entity up by UUID.
        let find = || {
            ask(ferrite_plugin::default_world_id(), ["0", "-1", "0", "-1"])
                .expect("the default world answers")
                .split(HOST_ITEM_SEPARATOR)
                .map(|record| record.split(FIELD).map(str::to_owned).collect::<Vec<_>>())
                .find(|fields| fields.get(6) == Some(&uuid.to_string()))
        };
        super::super::facts::publish_entities(&mut owned, &store, overworld);
        let fields = find().expect("listed");
        assert_eq!(fields.len(), 13, "{fields:?}");
        assert_ne!(fields[12], HOST_NULL, "NBT is present");
        let first_nbt = fields[12].clone();
        super::super::facts::publish_entities(&mut owned, &store, overworld);
        assert_eq!(find().expect("still listed")[12], first_nbt);
        let empty = EntityStore::new(3);
        super::super::facts::publish_entities(&mut owned, &empty, overworld);
        assert!(find().is_none());
    }

    #[test]
    fn a_snapshot_becomes_an_entity_under_the_requested_identity() {
        let original = zombie(Uuid::new_v4());
        let snbt = entity_snbt(&original).expect("a zombie serializes");
        let fresh = Uuid::new_v4();
        let record = record_from_snbt(&snbt, fresh, [100.5, 70.0, -20.5], 90.0).expect("loads");
        assert_eq!(record.data.uuid, fresh);
        assert_ne!(record.data.uuid, original.data.uuid);
        assert_eq!(record.pos, Vec3::new(100.5, 70.0, -20.5));
        assert_eq!(record.data.yaw, 90.0);
        assert_eq!(record.data.kind, original.data.kind);
        assert!(record_from_snbt("{id:\"minecraft:not_a_mob\"}", fresh, [0.0; 3], 0.0).is_none());
        assert!(record_from_snbt("garbage", fresh, [0.0; 3], 0.0).is_none());
    }

    #[test]
    fn the_spawn_op_is_a_validated_command() {
        let world = Uuid::new_v4();
        let uuid = Uuid::new_v4();
        let snbt = entity_snbt(&zombie(Uuid::new_v4())).unwrap();
        let call = |snbt: &str, x: &str| HostCall {
            owner: "World".into(),
            subject: world.to_string(),
            method: "ferriteSpawnSnapshot(String,String,double,double,double,double)".into(),
            args: vec![
                snbt.into(),
                uuid.to_string(),
                x.into(),
                "70".into(),
                "2".into(),
                "90".into(),
            ],
        };
        assert_eq!(
            command(&call(&snbt, "1.5")),
            Some(ferrite_plugin::PlayerCommand::SpawnSnapshot {
                world,
                snbt: snbt.clone(),
                uuid,
                pos: [1.5, 70.0, 2.0],
                yaw: 90.0,
            })
        );
        assert_eq!(command(&call(&snbt, "NaN")), None);
        assert_eq!(command(&call("{id:\"minecraft:nope\"}", "1")), None);
    }
}
