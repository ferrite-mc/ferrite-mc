//! Shoulder parrots (vanilla `Player.setEntityOnShoulder`).
//!
//! A parrot that a player picks up stops being a world entity: its entity data moves into the
//! player's `ShoulderEntityLeft` or `ShoulderEntityRight` compound. The region owns that state
//! while the player is online (the server keeps it keyed by the player UUID, so it travels with
//! the player across region hand-offs and dimension changes); this module is the pure state and
//! NBT form, and [`ShoulderParrot::to_record`] turns it back into a real parrot when it is
//! released.
//!
//! The 26.3 client is told about the parrot through player metadata indices
//! [`LEFT_INDEX`]/[`RIGHT_INDEX`] as an optional varint variant, so a server-side implementation
//! can display the parrot without keeping a hidden entity alive.

use crate::mob::{self, MobKind};
use crate::store::{EntityData, EntityRecord};
use crate::{Rng, tame};
use ferrite_math::Vec3;
use ferrite_nbt::{Compound, Tag};
use std::sync::OnceLock;
use uuid::Uuid;

/// Player metadata index of the left shoulder (`Player.DATA_ID_SHOULDER_LEFT`).
pub const LEFT_INDEX: u8 = 19;
/// Player metadata index of the right shoulder (`Player.DATA_ID_SHOULDER_RIGHT`).
pub const RIGHT_INDEX: u8 = 20;
/// `Parrot.Variant` has five colours in 26.3.
pub const MAX_VARIANTS: i32 = 5;
/// The vanilla save keys of the two shoulder compounds.
pub const LEFT_KEY: &str = "ShoulderEntityLeft";
pub const RIGHT_KEY: &str = "ShoulderEntityRight";

/// One shoulder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShoulderSide {
    Left,
    Right,
}

impl ShoulderSide {
    pub fn metadata_index(self) -> u8 {
        match self {
            Self::Left => LEFT_INDEX,
            Self::Right => RIGHT_INDEX,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Left => LEFT_KEY,
            Self::Right => RIGHT_KEY,
        }
    }
}

/// A parrot that sits on a player's shoulder, with the subset of its entity data the server
/// needs to recreate it. The client only needs the colour variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShoulderParrot {
    pub uuid: Uuid,
    pub variant: i32,
    pub owner: Option<Uuid>,
}

fn parrot_type() -> u16 {
    static KIND: OnceLock<u16> = OnceLock::new();
    *KIND.get_or_init(|| {
        ferrite_data::entity_props::by_name("parrot")
            .expect("vanilla has a parrot entity type")
            .id
    })
}

impl ShoulderParrot {
    /// Captures a live, tamed parrot owned by `owner`; anything else cannot sit on a shoulder.
    pub fn from_data(data: &EntityData, owner: Uuid) -> Option<Self> {
        if data.kind != parrot_type() {
            return None;
        }
        let state = data.mob.as_ref()?;
        if state.kind != MobKind::Parrot || state.is_dead() || state.owner != Some(owner) {
            return None;
        }
        Some(Self {
            uuid: data.uuid,
            variant: 0,
            owner: state.owner,
        })
    }

    /// Recreates the parrot entity where a shoulder is released.
    pub fn to_record(self, pos: Vec3, rng: &mut Rng) -> EntityRecord {
        let mut record = mob::new_mob(MobKind::Parrot, pos, rng);
        record.data.uuid = self.uuid;
        let mut state = *record
            .data
            .mob
            .as_ref()
            .expect("a new parrot has mob state");
        state.owner = self.owner;
        state.sitting = true;
        record.data.mob = Some(state);
        tame::define_metadata(&mut record.data.metadata, &state);
        record
    }

    /// The vanilla-shaped shoulder compound. 26.3 stores the colour as an entity component, but
    /// this sidecar is Ferrite's own and keeps the variant as an int.
    pub fn to_nbt(self) -> Compound {
        let mut c = Compound::new();
        c.insert("id".into(), Tag::String("minecraft:parrot".into()));
        c.insert("UUID".into(), Tag::IntArray(uuid_words(self.uuid)));
        c.insert(
            "Variant".into(),
            Tag::Int(self.variant.clamp(0, MAX_VARIANTS - 1)),
        );
        if let Some(owner) = self.owner {
            c.insert("Owner".into(), Tag::IntArray(uuid_words(owner)));
        }
        c
    }

    /// Reads [`Self::to_nbt`]; a malformed compound is ignored, like any other corrupt save.
    pub fn from_nbt(c: &Compound) -> Option<Self> {
        let uuid = uuid_field(c, "UUID")?;
        let variant = match c.get("Variant") {
            Some(Tag::Int(variant)) => (*variant).clamp(0, MAX_VARIANTS - 1),
            _ => 0,
        };
        Some(Self {
            uuid,
            variant,
            owner: uuid_field(c, "Owner"),
        })
    }
}

/// The two shoulders of one player.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Shoulders {
    pub left: Option<ShoulderParrot>,
    pub right: Option<ShoulderParrot>,
}

impl Shoulders {
    pub fn is_empty(&self) -> bool {
        self.left.is_none() && self.right.is_none()
    }

    /// Puts `parrot` on the first free shoulder, left first like vanilla.
    pub fn put(&mut self, parrot: ShoulderParrot) -> Option<ShoulderSide> {
        if self.left.is_none() {
            self.left = Some(parrot);
            Some(ShoulderSide::Left)
        } else if self.right.is_none() {
            self.right = Some(parrot);
            Some(ShoulderSide::Right)
        } else {
            None
        }
    }

    pub fn get(&self, side: ShoulderSide) -> Option<ShoulderParrot> {
        match side {
            ShoulderSide::Left => self.left,
            ShoulderSide::Right => self.right,
        }
    }

    pub fn set(&mut self, side: ShoulderSide, parrot: Option<ShoulderParrot>) {
        match side {
            ShoulderSide::Left => self.left = parrot,
            ShoulderSide::Right => self.right = parrot,
        }
    }

    /// Takes the parrot off `side`.
    pub fn release(&mut self, side: ShoulderSide) -> Option<ShoulderParrot> {
        match side {
            ShoulderSide::Left => self.left.take(),
            ShoulderSide::Right => self.right.take(),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (ShoulderSide, ShoulderParrot)> {
        [
            (ShoulderSide::Left, self.left),
            (ShoulderSide::Right, self.right),
        ]
        .into_iter()
        .filter_map(|(side, parrot)| parrot.map(|parrot| (side, parrot)))
    }

    /// The two vanilla save keys, omitted when a shoulder is empty.
    pub fn to_nbt(self) -> Tag {
        let mut c = Compound::new();
        if let Some(left) = self.left {
            c.insert(LEFT_KEY.into(), Tag::Compound(left.to_nbt()));
        }
        if let Some(right) = self.right {
            c.insert(RIGHT_KEY.into(), Tag::Compound(right.to_nbt()));
        }
        Tag::Compound(c)
    }

    pub fn from_nbt(tag: &Tag) -> Self {
        let Tag::Compound(c) = tag else {
            return Self::default();
        };
        let read = |key: &str| match c.get(key) {
            Some(Tag::Compound(parrot)) => ShoulderParrot::from_nbt(parrot),
            _ => None,
        };
        Self {
            left: read(LEFT_KEY),
            right: read(RIGHT_KEY),
        }
    }
}

fn uuid_words(uuid: Uuid) -> Vec<i32> {
    let n = uuid.as_u128();
    vec![
        (n >> 96) as u32 as i32,
        (n >> 64) as u32 as i32,
        (n >> 32) as u32 as i32,
        n as u32 as i32,
    ]
}

fn uuid_field(c: &Compound, key: &str) -> Option<Uuid> {
    let Tag::IntArray(words) = c.get(key)? else {
        return None;
    };
    if words.len() != 4 {
        return None;
    }
    let mut n: u128 = 0;
    for word in words {
        n = (n << 32) | u128::from(*word as u32);
    }
    Some(Uuid::from_u128(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parrot(uuid: u128, owner: u128) -> ShoulderParrot {
        ShoulderParrot {
            uuid: Uuid::from_u128(uuid),
            variant: 2,
            owner: Some(Uuid::from_u128(owner)),
        }
    }

    #[test]
    fn shoulders_fill_left_first_and_refuse_a_third_parrot() {
        let mut shoulders = Shoulders::default();
        assert!(shoulders.is_empty());
        assert_eq!(
            shoulders.put(parrot(1, 9)),
            Some(ShoulderSide::Left),
            "the first parrot takes the left shoulder"
        );
        assert_eq!(shoulders.put(parrot(2, 9)), Some(ShoulderSide::Right));
        assert_eq!(shoulders.put(parrot(3, 9)), None, "both shoulders are full");
        assert_eq!(shoulders.left.unwrap().uuid, Uuid::from_u128(1));
        assert_eq!(
            shoulders.release(ShoulderSide::Left).unwrap().uuid,
            Uuid::from_u128(1)
        );
        assert_eq!(shoulders.put(parrot(4, 9)), Some(ShoulderSide::Left));
        assert!(shoulders.get(ShoulderSide::Right).is_some());
        assert_eq!(shoulders.iter().count(), 2);
    }

    #[test]
    fn a_tamed_parrot_can_be_captured_and_restored() {
        let owner = Uuid::from_u128(9);
        let mut rng = Rng::new(3);
        let record = mob::new_mob(MobKind::Parrot, Vec3::new(0.5, 64.0, 0.5), &mut rng);
        let uuid = record.data.uuid;
        assert!(
            ShoulderParrot::from_data(&record.data, owner).is_none(),
            "a wild parrot cannot sit on a shoulder"
        );
        let mut tamed = record.data.clone();
        tamed.mob.as_mut().unwrap().owner = Some(owner);
        let parrot = ShoulderParrot::from_data(&tamed, owner).expect("tamed parrot");
        assert_eq!(parrot.uuid, uuid);
        let restored = parrot.to_record(Vec3::new(4.5, 64.0, 4.5), &mut rng);
        assert_eq!(restored.data.uuid, uuid);
        assert_eq!(restored.data.kind, parrot_type());
        let state = restored.data.mob.as_ref().unwrap();
        assert_eq!(state.kind, MobKind::Parrot);
        assert_eq!(state.owner, Some(owner));
        assert!(state.sitting);
    }

    #[test]
    fn shoulder_nbt_round_trips_both_sides() {
        let mut shoulders = Shoulders::default();
        shoulders.put(parrot(0xaa, 9));
        shoulders.put(parrot(0xbb, 9));
        let tag = shoulders.to_nbt();
        let loaded = Shoulders::from_nbt(&tag);
        assert_eq!(loaded, shoulders);
        let Tag::Compound(c) = &tag else {
            unreachable!()
        };
        assert!(c.contains_key(LEFT_KEY) && c.contains_key(RIGHT_KEY));
        assert_eq!(
            Shoulders::from_nbt(&Tag::Compound(Compound::new())),
            Shoulders::default(),
            "an empty save has no shoulders"
        );
    }
}
