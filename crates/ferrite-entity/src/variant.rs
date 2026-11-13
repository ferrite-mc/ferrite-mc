//! Cat and wolf variants (`minecraft:cat_variant`, `minecraft:wolf_variant`), kept on the mob as
//! an index into these name tables.
//!
//! The variant is not sent to the client yet: the 26.3 data accessors (`Cat.DATA_VARIANT_ID` and
//! `Wolf.DATA_VARIANT_ID`) use holder serializers that `MetaValue` does not model, so a client
//! still draws the default coat. Not verified with a real client.

use crate::mob::MobKind;

/// Every cat variant of the 26.3 `cat_variant` registry.
pub const CAT: &[&str] = &[
    "minecraft:all_black",
    "minecraft:black",
    "minecraft:british_shorthair",
    "minecraft:calico",
    "minecraft:jellie",
    "minecraft:persian",
    "minecraft:ragdoll",
    "minecraft:red",
    "minecraft:siamese",
    "minecraft:tabby",
    "minecraft:white",
];

/// Every wolf variant of the 26.3 `wolf_variant` registry.
pub const WOLF: &[&str] = &[
    "minecraft:ashen",
    "minecraft:black",
    "minecraft:chestnut",
    "minecraft:pale",
    "minecraft:rusty",
    "minecraft:snowy",
    "minecraft:spotted",
    "minecraft:striped",
    "minecraft:woods",
];

/// The variant names of a kind that has variants.
pub fn names(kind: MobKind) -> Option<&'static [&'static str]> {
    match kind {
        MobKind::Cat => Some(CAT),
        MobKind::Wolf => Some(WOLF),
        _ => None,
    }
}

/// The variant a stored index names.
pub fn name(kind: MobKind, index: u8) -> Option<&'static str> {
    names(kind)?.get(usize::from(index)).copied()
}

/// The index of a variant name (`minecraft:` is optional).
pub fn index(kind: MobKind, name: &str) -> Option<u8> {
    let short = name.strip_prefix("minecraft:").unwrap_or(name);
    names(kind)?
        .iter()
        .position(|candidate| candidate.strip_prefix("minecraft:") == Some(short))
        .and_then(|i| u8::try_from(i).ok())
}

/// The registry key of the variant component a criterion predicate names
/// (`minecraft:cat/variant`).
pub fn component(kind: MobKind) -> Option<&'static str> {
    match kind {
        MobKind::Cat => Some("minecraft:cat/variant"),
        MobKind::Wolf => Some("minecraft:wolf/variant"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_indexes_round_trip() {
        for kind in [MobKind::Cat, MobKind::Wolf] {
            for (i, wanted) in names(kind).unwrap().iter().enumerate() {
                let i = u8::try_from(i).unwrap();
                assert_eq!(name(kind, i), Some(*wanted));
                assert_eq!(index(kind, wanted), Some(i));
            }
        }
        assert_eq!(index(MobKind::Cat, "tabby"), Some(9));
        assert_eq!(index(MobKind::Wolf, "minecraft:tabby"), None);
        assert_eq!(name(MobKind::Cow, 0), None);
    }
}
