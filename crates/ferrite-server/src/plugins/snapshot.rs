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
        _ => None,
    }
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
}
