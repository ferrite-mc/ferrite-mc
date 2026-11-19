//! Cat and wolf variants (`minecraft:cat_variant`, `minecraft:wolf_variant`), kept on the mob as
//! an index into these name tables.
//!
//! The variant is not sent to the client yet: the 26.3 data accessors (`Cat.DATA_VARIANT_ID` and
//! `Wolf.DATA_VARIANT_ID`) use holder serializers that `MetaValue` does not model, so a client
//! still draws the default coat. Not verified with a real client.

use crate::metadata::animal_variant;
use crate::mob::MobKind;
use crate::{Metadata, Rng};
use ferrite_protocol::packets::play::MetaValue;

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

/// Every 26.3 cow sound variant, in registry order (the order the synched serializer uses).
pub const COW_SOUND: &[&str] = &["minecraft:classic", "minecraft:moody"];
/// Every 26.3 chicken sound variant, in registry order.
pub const CHICKEN_SOUND: &[&str] = &["minecraft:classic", "minecraft:picky"];
/// Every 26.3 pig sound variant, in registry order (`0=big, 1=classic, 2=mini`).
pub const PIG_SOUND: &[&str] = &["minecraft:big", "minecraft:classic", "minecraft:mini"];
/// Every 26.3 wolf sound variant, in registry order.
pub const WOLF_SOUND: &[&str] = &[
    "minecraft:angry",
    "minecraft:big",
    "minecraft:classic",
    "minecraft:cute",
    "minecraft:grumpy",
    "minecraft:puglin",
    "minecraft:sad",
];
/// Every 26.3 cat sound variant, in registry order.
pub const CAT_SOUND: &[&str] = &["minecraft:classic", "minecraft:royal"];

/// The sound variants of a kind, in registry order.
pub fn sound_names(kind: MobKind) -> Option<&'static [&'static str]> {
    match kind {
        MobKind::Cow => Some(COW_SOUND),
        MobKind::Chicken => Some(CHICKEN_SOUND),
        MobKind::Pig => Some(PIG_SOUND),
        MobKind::Wolf => Some(WOLF_SOUND),
        MobKind::Cat => Some(CAT_SOUND),
        _ => None,
    }
}

/// The registry name of a sound-variant index.
pub fn sound_name(kind: MobKind, index: u8) -> Option<&'static str> {
    sound_names(kind)?.get(usize::from(index)).copied()
}

/// The index of a sound-variant registry name (`minecraft:` is optional).
pub fn sound_index(kind: MobKind, name: &str) -> Option<u8> {
    let short = name.strip_prefix("minecraft:").unwrap_or(name);
    sound_names(kind)?
        .iter()
        .position(|candidate| candidate.strip_prefix("minecraft:") == Some(short))
        .and_then(|i| u8::try_from(i).ok())
}

/// The default sound variant of a kind (`minecraft:classic` in all but the pig and wolf).
pub fn default_sound_index(kind: MobKind) -> Option<u8> {
    match kind {
        MobKind::Cow | MobKind::Chicken | MobKind::Cat => {
            Some(animal_variant::DEFAULT_CLASSIC_ZERO as u8)
        }
        MobKind::Pig => Some(animal_variant::DEFAULT_PIG_SOUND_CLASSIC as u8),
        MobKind::Wolf => Some(animal_variant::DEFAULT_WOLF_SOUND_CLASSIC as u8),
        _ => None,
    }
}

/// Picks a sound variant uniformly from the kind's 26.3 registry.
pub fn random_sound_index(kind: MobKind, rng: &mut Rng) -> u8 {
    let len = sound_names(kind).map_or(0, <[&str]>::len);
    if len == 0 {
        return 0;
    }
    rng.next_int(len as i32) as u8
}

/// The sound variant a bred baby gets. `Wolf.getBreedOffspring` picks a fresh random sound
/// variant; the other animals create the baby without `finalizeSpawn`, so it keeps the
/// default (`classic`).
pub fn offspring_sound_index(kind: MobKind, rng: &mut Rng) -> Option<u8> {
    (kind == MobKind::Wolf).then(|| random_sound_index(kind, rng))
}

/// Writes the selected sound-variant holder (or the kind's default) to synched data.
pub fn sync_sound_metadata(metadata: &mut Metadata, kind: MobKind, index: Option<u8>) {
    let Some(names) = sound_names(kind) else {
        return;
    };
    let default = default_sound_index(kind).unwrap_or(0);
    let index = index
        .filter(|index| usize::from(*index) < names.len())
        .unwrap_or(default);
    let id = i32::from(index);
    match kind {
        MobKind::Cow => metadata.set(
            animal_variant::COW_SOUND_VARIANT,
            MetaValue::CowSoundVariant(id),
        ),
        MobKind::Chicken => metadata.set(
            animal_variant::CHICKEN_SOUND_VARIANT,
            MetaValue::ChickenSoundVariant(id),
        ),
        MobKind::Pig => metadata.set(
            animal_variant::PIG_SOUND_VARIANT,
            MetaValue::PigSoundVariant(id),
        ),
        MobKind::Wolf => metadata.set(
            animal_variant::WOLF_SOUND_VARIANT,
            MetaValue::WolfSoundVariant(id),
        ),
        MobKind::Cat => metadata.set(
            animal_variant::CAT_SOUND_VARIANT,
            MetaValue::CatSoundVariant(id),
        ),
        _ => {}
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

    #[test]
    fn sound_variant_names_indexes_and_defaults_round_trip() {
        for kind in [
            MobKind::Cow,
            MobKind::Chicken,
            MobKind::Pig,
            MobKind::Wolf,
            MobKind::Cat,
        ] {
            for (i, wanted) in sound_names(kind).unwrap().iter().enumerate() {
                let i = u8::try_from(i).unwrap();
                assert_eq!(sound_name(kind, i), Some(*wanted));
                assert_eq!(sound_index(kind, wanted), Some(i));
            }
            let default = default_sound_index(kind).unwrap();
            assert_eq!(sound_index(kind, "classic"), Some(default));
        }
        assert_eq!(sound_index(MobKind::Pig, "minecraft:classic"), Some(1));
        assert_eq!(sound_index(MobKind::Wolf, "minecraft:classic"), Some(2));
        assert_eq!(sound_index(MobKind::Cow, "minecraft:royal"), None);
        assert_eq!(sound_name(MobKind::Sheep, 0), None);

        let mut metadata = Metadata::default();
        metadata.define_mob(MobKind::Pig);
        sync_sound_metadata(&mut metadata, MobKind::Pig, Some(0));
        assert_eq!(
            metadata.get(animal_variant::PIG_SOUND_VARIANT),
            Some(&MetaValue::PigSoundVariant(0))
        );
        sync_sound_metadata(&mut metadata, MobKind::Pig, None);
        assert_eq!(
            metadata.get(animal_variant::PIG_SOUND_VARIANT),
            Some(&MetaValue::PigSoundVariant(1))
        );
    }

    #[test]
    fn bred_wolves_roll_a_sound_variant_and_other_animals_keep_the_default() {
        let mut rng = Rng::new(7);
        for _ in 0..32 {
            assert!(offspring_sound_index(MobKind::Wolf, &mut rng).is_some());
        }
        for kind in [MobKind::Cow, MobKind::Chicken, MobKind::Pig, MobKind::Cat] {
            assert_eq!(offspring_sound_index(kind, &mut rng), None);
        }
    }
}
