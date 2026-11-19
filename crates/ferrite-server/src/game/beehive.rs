//! Beehive and bee-nest block entities (vanilla `BeehiveBlockEntity`, `BeehiveBlock`).
//!
//! Vanilla stores up to three occupants in the block entity's `bees` list. Each entry is an
//! `Occupant`: the bee's save NBT under `entity_data`, how long it has been inside
//! (`ticks_in_hive`) and how long it must stay (`min_ticks_in_hive`). `flower_pos` is the hive's
//! remembered flower. The honey level is a block-state property (`honey_level 0..=5`), not block
//! entity data.
//!
//! Occupancy is tile-owned: the block entity lives in the chunk, and a bee becoming an occupant
//! is removed from the owning region's entity store as one complete record. Entering and leaving
//! therefore cannot duplicate a bee across a region split or hand-off. Malformed entries are
//! skipped one by one with a warning; a broken save never takes the whole hive down.
//!
//! This module owns the data shape and the vanilla rules that do not need a region. The region
//! owns day/night entry, cross-region hand-off, crop growth and hive destruction.

use ferrite_entity::mob::MobKind;
use ferrite_nbt::{Compound, List, Tag};
use ferrite_world::BlockEntity;
use std::collections::BTreeMap;

use super::anvil::AnvilChunkSource;

/// The shared block entity type of both the beehive and the bee nest.
pub const BLOCK_ENTITY: &str = "minecraft:beehive";
/// `BeehiveBlockEntity.MAX_OCCUPANTS`.
pub const MAX_OCCUPANTS: usize = 3;
/// `BeehiveBlock.MAX_HONEY_LEVELS`.
pub const MAX_HONEY_LEVEL: u8 = 5;
/// Vanilla `BeehiveBlockEntity.MIN_OCCUPATION_TICKS_NECTAR` (2 minutes).
pub const MIN_OCCUPATION_TICKS_NECTAR: i32 = 2400;
/// Vanilla `BeehiveBlockEntity.MIN_OCCUPATION_TICKS_NECTARLESS` (30 seconds).
pub const MIN_OCCUPATION_TICKS_NECTARLESS: i32 = 600;
/// Vanilla `BeehiveBlockEntity.MIN_TICKS_BEFORE_REENTERING_HIVE`: a disturbed bee stays away.
pub const MIN_TICKS_BEFORE_REENTERING_HIVE: u16 = 400;
/// One stored bee (`BeehiveBlockEntity.Occupant`).
#[derive(Clone, Debug, PartialEq)]
pub struct Occupant {
    /// The entity's full save NBT, exactly as `entity_to_nbt` writes it.
    pub entity_data: Compound,
    /// Ticks spent in this hive.
    pub ticks_in_hive: i32,
    /// Ticks this bee must stay before it may leave.
    pub min_ticks_in_hive: i32,
}

impl Occupant {
    /// `BeehiveBlockEntity.Occupant.of`: capture a live bee. The minimum stay is 2400 ticks for
    /// a nectar-carrying bee and 600 otherwise.
    pub fn of(record: &ferrite_entity::EntityRecord) -> Option<Self> {
        if record.data.mob.as_ref().map(|mob| mob.kind) != Some(MobKind::Bee) {
            return None;
        }
        let entity_data = super::entity_save::entity_to_nbt(record)?;
        let has_nectar = record.data.mob.as_ref().is_some_and(|mob| mob.has_nectar);
        Some(Self {
            entity_data,
            ticks_in_hive: 0,
            min_ticks_in_hive: if has_nectar {
                MIN_OCCUPATION_TICKS_NECTAR
            } else {
                MIN_OCCUPATION_TICKS_NECTARLESS
            },
        })
    }

    /// `BeeData.hasNectar` reads `HasNectar` from the stored entity NBT.
    pub fn has_nectar(&self) -> bool {
        matches!(
            self.entity_data.get("HasNectar"),
            Some(Tag::Byte(value)) if *value != 0
        )
    }

    /// `BeeData.tick`: one tick inside, and whether the minimum stay is over.
    pub fn tick(&mut self) -> bool {
        self.ticks_in_hive = self.ticks_in_hive.saturating_add(1);
        self.ticks_in_hive > self.min_ticks_in_hive
    }

    fn to_nbt(&self) -> Compound {
        let mut tag = Compound::new();
        tag.insert(
            "entity_data".into(),
            Tag::Compound(self.entity_data.clone()),
        );
        tag.insert("ticks_in_hive".into(), Tag::Int(self.ticks_in_hive));
        tag.insert("min_ticks_in_hive".into(), Tag::Int(self.min_ticks_in_hive));
        tag
    }

    fn from_nbt(tag: &Compound) -> Option<Self> {
        let Some(Tag::Compound(entity_data)) = tag.get("entity_data") else {
            return None;
        };
        match entity_data.get("id") {
            Some(Tag::String(id)) if id == "minecraft:bee" => {}
            _ => return None,
        }
        let ticks_in_hive = int(tag, "ticks_in_hive").unwrap_or(0).max(0);
        let min_ticks_in_hive = int(tag, "min_ticks_in_hive").unwrap_or(0).max(0);
        Some(Self {
            entity_data: entity_data.clone(),
            ticks_in_hive,
            min_ticks_in_hive,
        })
    }

    /// Creates the live bee at `at`, restoring its saved fields. A saved entity without a
    /// position is placed at the hive exit instead of being dropped.
    pub fn record_at(&self, at: [f64; 3]) -> Option<ferrite_entity::EntityRecord> {
        let mut data = self.entity_data.clone();
        data.insert("Pos".into(), position_tag(at));
        if !data.contains_key("Motion") {
            data.insert(
                "Motion".into(),
                Tag::List(List::Double(vec![0.0, 0.0, 0.0])),
            );
        }
        let mut record = super::entity_save::entity_from_nbt(&data)?;
        if record.data.mob.as_ref().map(|mob| mob.kind) != Some(MobKind::Bee) {
            return None;
        }
        record.pos = ferrite_math::Vec3::new(at[0], at[1], at[2]);
        record.vel = ferrite_math::Vec3::ZERO;
        if let Some(mob) = record.data.mob.as_mut() {
            mob.stay_out_of_hive = 0;
        }
        Some(record)
    }
}

/// The data of one hive block entity.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HiveData {
    pub occupants: Vec<Occupant>,
    pub flower_pos: Option<[i32; 3]>,
}

impl HiveData {
    /// `BeehiveBlockEntity.addOccupant`: refuses when the hive is full.
    pub fn add(&mut self, record: &ferrite_entity::EntityRecord) -> bool {
        if self.occupants.len() >= MAX_OCCUPANTS {
            return false;
        }
        let Some(occupant) = Occupant::of(record) else {
            return false;
        };
        if self.flower_pos.is_none()
            && let Some(flower) = record.data.mob.as_ref().and_then(|mob| mob.flower_pos)
        {
            self.flower_pos = Some(flower);
        }
        self.occupants.push(occupant);
        true
    }

    /// The `minecraft:bees` component value for a broken hive (vanilla `Bees`). Empty hives
    /// omit the key, matching the item default.
    pub fn bees_tag(&self) -> Option<Tag> {
        (!self.occupants.is_empty()).then(|| {
            Tag::List(List::Compound(
                self.occupants.iter().map(Occupant::to_nbt).collect(),
            ))
        })
    }
}

fn int(tag: &Compound, key: &str) -> Option<i32> {
    match tag.get(key) {
        Some(Tag::Int(value)) => Some(*value),
        Some(Tag::Short(value)) => Some(i32::from(*value)),
        Some(Tag::Byte(value)) => Some(i32::from(*value)),
        _ => None,
    }
}

fn position_tag([x, y, z]: [f64; 3]) -> Tag {
    Tag::List(List::Double(vec![x, y, z]))
}

/// A block position in vanilla's `BlockPos.CODEC` form: a list of three ints. The older
/// `[I;x,y,z]` and `{X,Y,Z}` shapes are accepted when reading.
fn block_pos_tag([x, y, z]: [i32; 3]) -> Tag {
    Tag::List(List::Int(vec![x, y, z]))
}

fn block_pos(tag: &Compound, key: &str) -> Option<[i32; 3]> {
    match tag.get(key)? {
        Tag::List(List::Int(values)) if values.len() == 3 => {
            Some([values[0], values[1], values[2]])
        }
        Tag::IntArray(values) if values.len() == 3 => Some([values[0], values[1], values[2]]),
        Tag::Compound(pos) => Some([int(pos, "X")?, int(pos, "Y")?, int(pos, "Z")?]),
        _ => None,
    }
}

fn hive_type_id() -> Option<u32> {
    ferrite_data::registries::id("minecraft:block_entity_type", BLOCK_ENTITY)
        .and_then(|id| u32::try_from(id).ok())
}

fn is_hive_entity(entity: &BlockEntity) -> bool {
    match entity.data.get("id") {
        Some(Tag::String(id)) => id == BLOCK_ENTITY,
        _ => hive_type_id() == Some(entity.type_id),
    }
}

/// Reads a hive's occupants and remembered flower. A malformed entry is skipped; the rest of
/// the hive loads.
pub fn load(source: &AnvilChunkSource, [x, y, z]: [i32; 3]) -> HiveData {
    let Some(entity) = source.block_entity(x, y, z).filter(is_hive_entity) else {
        return HiveData::default();
    };
    let mut occupants = Vec::new();
    if let Some(Tag::List(List::Compound(entries))) = entity.data.get("bees") {
        for entry in entries {
            match Occupant::from_nbt(entry) {
                Some(occupant) if occupants.len() < MAX_OCCUPANTS => occupants.push(occupant),
                Some(_) => {}
                None => tracing::warn!(x, y, z, "ignoring an unreadable beehive occupant"),
            }
        }
    } else if entity.data.contains_key("bees") {
        tracing::warn!(
            x,
            y,
            z,
            "ignoring a beehive whose bees list is not compounds"
        );
    }
    HiveData {
        occupants,
        flower_pos: block_pos(&entity.data, "flower_pos"),
    }
}

/// Writes occupants and `flower_pos` back, creating the block entity when one is missing.
/// Nothing is written when the parsed data is unchanged, so a quiet hive does not dirty its
/// chunk every tick.
pub fn store(source: &AnvilChunkSource, [x, y, z]: [i32; 3], data: &HiveData) {
    let Some(type_id) = hive_type_id() else {
        return;
    };
    let result = source.update_block_entity(x, y, z, |entity| {
        if entity
            .as_ref()
            .filter(|entity| is_hive_entity(entity))
            .is_some_and(|entity| {
                load_occupants(&entity.data) == data.occupants
                    && block_pos(&entity.data, "flower_pos") == data.flower_pos
            })
        {
            return (false, ());
        }
        let entity = entity.get_or_insert_with(|| {
            let mut nbt = Compound::new();
            nbt.insert("id".into(), Tag::String(BLOCK_ENTITY.into()));
            nbt.insert("x".into(), Tag::Int(x));
            nbt.insert("y".into(), Tag::Int(y));
            nbt.insert("z".into(), Tag::Int(z));
            BlockEntity { type_id, data: nbt }
        });
        entity.type_id = type_id;
        entity
            .data
            .insert("id".into(), Tag::String(BLOCK_ENTITY.into()));
        let bees = data
            .occupants
            .iter()
            .map(Occupant::to_nbt)
            .collect::<Vec<_>>();
        entity
            .data
            .insert("bees".into(), Tag::List(List::Compound(bees)));
        match data.flower_pos {
            Some(pos) => {
                entity.data.insert("flower_pos".into(), block_pos_tag(pos));
            }
            None => {
                entity.data.shift_remove("flower_pos");
            }
        }
        (true, ())
    });
    if let Err(error) = result {
        tracing::warn!(x, y, z, %error, "could not store beehive state");
    }
}

fn load_occupants(data: &Compound) -> Vec<Occupant> {
    let Some(Tag::List(List::Compound(entries))) = data.get("bees") else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(Occupant::from_nbt)
        .take(MAX_OCCUPANTS)
        .collect()
}

/// Creates the block entity of a freshly placed hive.
pub fn ensure(source: &AnvilChunkSource, pos: [i32; 3]) {
    store(source, pos, &HiveData::default());
}

/// Removes the hive block entity and returns both the item components a silk-touch break should
/// copy and the occupants that have to be released.
pub fn take(source: &AnvilChunkSource, [x, y, z]: [i32; 3]) -> (BTreeMap<String, Tag>, HiveData) {
    let removed = source
        .update_block_entity(x, y, z, |entity| match entity.take() {
            Some(removed) => (true, Some(removed.data)),
            None => (false, None),
        })
        .ok()
        .flatten();
    let Some(data) = removed else {
        return (BTreeMap::new(), HiveData::default());
    };
    let occupants = load_occupants(&data);
    let hive = HiveData {
        occupants,
        flower_pos: block_pos(&data, "flower_pos"),
    };
    let mut components = BTreeMap::new();
    if let Some(bees) = hive.bees_tag() {
        components.insert("minecraft:bees".to_owned(), bees);
    }
    (components, hive)
}

/// Whether a block name is a beehive or bee nest (block tag `#minecraft:beehives`).
pub fn is_hive(block: &str) -> bool {
    ferrite_data::tags::contains("block", "beehives", block)
}

/// The `honey_level` property of a hive state.
pub fn honey_level(state: u16) -> Option<u8> {
    let (_, props) = ferrite_data::blocks::state(state)?;
    props
        .iter()
        .find(|(key, _)| *key == "honey_level")
        .and_then(|(_, value)| value.parse().ok())
}

/// A copy of `state` with a new `honey_level`.
pub fn with_honey_level(state: u16, level: u8) -> Option<u16> {
    let (name, props) = ferrite_data::blocks::state(state)?;
    let level = level.min(MAX_HONEY_LEVEL);
    let value = level.to_string();
    let properties: Vec<(&str, &str)> = props
        .iter()
        .map(|&(key, old)| {
            if key == "honey_level" {
                (key, value.as_str())
            } else {
                (key, old)
            }
        })
        .collect();
    ferrite_data::blocks::state_id(name, &properties)
}

/// `CampfireBlock.isSmokeyPos`: a lit campfire in the five blocks below calms the hive.
pub fn is_sedated(source: &AnvilChunkSource, [x, y, z]: [i32; 3]) -> bool {
    (1..=5).any(|below| {
        let Some(state) = source
            .block(x, y - below, z)
            .and_then(|state| u16::try_from(state).ok())
        else {
            return false;
        };
        let Some((name, props)) = ferrite_data::blocks::state(state) else {
            return false;
        };
        ferrite_data::tags::contains("block", "campfires", name)
            && props
                .iter()
                .any(|(key, value)| *key == "lit" && *value == "true")
    })
}

/// A fenced direction of the hive's `facing` property.
fn facing_offset(state: u16) -> Option<(i32, i32)> {
    let (_, props) = ferrite_data::blocks::state(state)?;
    let facing = props
        .iter()
        .find(|(key, _)| *key == "facing")
        .map(|(_, value)| *value)?;
    Some(match facing {
        "north" => (0, -1),
        "south" => (0, 1),
        "west" => (-1, 0),
        "east" => (1, 0),
        _ => return None,
    })
}

/// Where a released bee appears. Vanilla `releaseOccupant`: a blocked exit is refused unless the
/// release is an emergency; an emergency release with a blocked exit spawns at the hive centre.
pub fn exit_position(
    source: &AnvilChunkSource,
    [x, y, z]: [i32; 3],
    state: u16,
    width: f64,
    height: f64,
    emergency: bool,
) -> Option<[f64; 3]> {
    let (dx, dz) = facing_offset(state).unwrap_or((0, 0));
    let exit = [x + dx, y, z + dz];
    let blocked = source
        .block(exit[0], exit[1], exit[2])
        .and_then(|block| u16::try_from(block).ok())
        .is_none_or(|block| {
            let props = ferrite_data::block_props::get(block);
            !props.is_air && props.can_occlude
        });
    if blocked && !emergency {
        return None;
    }
    let spread = if blocked { 0.0 } else { 0.55 + width / 2.0 };
    Some([
        f64::from(x) + 0.5 + f64::from(dx) * spread,
        f64::from(y) + 0.5 - height / 2.0,
        f64::from(z) + 0.5 + f64::from(dz) * spread,
    ])
}

/// Builds the live bee for an occupant at the hive's exit.
pub fn release(
    source: &AnvilChunkSource,
    pos: [i32; 3],
    state: u16,
    occupant: &Occupant,
    emergency: bool,
) -> Option<ferrite_entity::EntityRecord> {
    let props = MobKind::Bee.props();
    let at = exit_position(
        source,
        pos,
        state,
        f64::from(props.width),
        f64::from(props.height),
        emergency,
    )?;
    occupant.record_at(at)
}

/// Rewrites a released bee's synched flag byte after the region changed its nectar state.
pub fn sync_record(record: &mut ferrite_entity::EntityRecord) {
    let flags = record
        .data
        .mob
        .as_ref()
        .map(ferrite_entity::bee::metadata_flags)
        .unwrap_or(0);
    record.data.metadata.set(
        ferrite_entity::metadata::bee::FLAGS,
        ferrite_protocol::packets::play::MetaValue::Byte(flags),
    );
}

/// `releaseOccupant`'s honey bump: usually one level, with a 5% chance of two, never above five.
pub fn delivered_honey(level: u8, rng: &mut ferrite_entity::Rng) -> u8 {
    if level >= MAX_HONEY_LEVEL {
        return MAX_HONEY_LEVEL;
    }
    let mut amount = if rng.next_int(100) == 0 { 2 } else { 1 };
    if level + amount > MAX_HONEY_LEVEL {
        amount -= 1;
    }
    (level + amount).min(MAX_HONEY_LEVEL)
}

/// `BeeGrowCropGoal`'s age growth for a `#bee_growables` block: one stage, never past the
/// crop's maximum. `None` means the block is not growable or is already mature.
pub fn grow_crop(state: u16) -> Option<u16> {
    let (name, props) = ferrite_data::blocks::state(state)?;
    if !ferrite_data::tags::contains("block", "bee_growables", name) {
        return None;
    }
    let short = name.strip_prefix("minecraft:").unwrap_or(name);
    // Pitcher crop's upper half follows its lower half; growing it alone would desynchronize
    // the two-block plant, so the lower half (or an unsplit block) is the only target.
    if short == "pitcher_crop"
        && props
            .iter()
            .any(|(key, value)| *key == "half" && *value == "upper")
    {
        return None;
    }
    let current: u8 = props
        .iter()
        .find(|(key, _)| *key == "age")
        .and_then(|(_, value)| value.parse().ok())?;
    let max = max_age(short)?;
    if current >= max {
        return None;
    }
    let next = (current + 1).to_string();
    let properties: Vec<(&str, &str)> = props
        .iter()
        .map(|&(key, old)| {
            if key == "age" {
                (key, next.as_str())
            } else {
                (key, old)
            }
        })
        .collect();
    ferrite_data::blocks::state_id(name, &properties)
}

/// Vanilla `CropBlock.getMaxAge` / `StemBlock` / sweet-berry and cave-vine ranges.
fn max_age(name: &str) -> Option<u8> {
    Some(match name {
        "wheat" | "carrots" | "potatoes" | "melon_stem" | "pumpkin_stem" => 7,
        "beetroots" | "sweet_berry_bush" => 3,
        "torchflower_crop" => 1,
        "pitcher_crop" => 4,
        "cave_vines" => 25,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_entity::{EntityRecord, mob};
    use ferrite_math::Vec3;

    async fn loaded() -> std::sync::Arc<AnvilChunkSource> {
        let source = AnvilChunkSource::new_memory();
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        source
    }

    fn hive_state(honey: u8) -> u32 {
        u32::from(
            ferrite_data::blocks::state_id(
                "minecraft:beehive",
                &[("facing", "north"), ("honey_level", &honey.to_string())],
            )
            .expect("a vanilla beehive"),
        )
    }

    fn bee_record(at: Vec3) -> EntityRecord {
        let mut rng = ferrite_entity::Rng::new(7);
        mob::new_mob(MobKind::Bee, at, &mut rng)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn occupants_round_trip_cap_at_three_and_keep_flower_pos() {
        let source = loaded().await;
        source.set_block(4, 70, 4, hive_state(0)).unwrap();
        ensure(&source, [4, 70, 4]);
        let mut data = load(&source, [4, 70, 4]);
        assert!(data.occupants.is_empty());
        data.flower_pos = Some([9, 65, -2]);

        let mut bee = bee_record(Vec3::new(4.5, 70.0, 4.5));
        bee.data.mob.as_mut().unwrap().has_nectar = true;
        bee.data.mob.as_mut().unwrap().flower_pos = Some([5, 65, 5]);
        assert!(data.add(&bee));
        assert_eq!(
            data.occupants[0].min_ticks_in_hive,
            MIN_OCCUPATION_TICKS_NECTAR
        );
        assert!(data.occupants[0].has_nectar());
        for _ in 0..(MAX_OCCUPANTS - 1) {
            assert!(data.add(&bee));
        }
        assert_eq!(
            data.occupants.len(),
            MAX_OCCUPANTS,
            "the fourth bee is refused"
        );
        data.occupants[0].ticks_in_hive = 41;
        store(&source, [4, 70, 4], &data);

        let loaded = load(&source, [4, 70, 4]);
        assert_eq!(loaded, data);
        let entity = source.block_entity(4, 70, 4).unwrap();
        assert_eq!(
            entity.data.get("id"),
            Some(&Tag::String(BLOCK_ENTITY.into()))
        );
        assert!(
            matches!(entity.data.get("bees"), Some(Tag::List(List::Compound(list))) if list.len() == 3)
        );
        assert_eq!(loaded.flower_pos, Some([9, 65, -2]));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn malformed_occupants_are_skipped_and_honey_levels_are_bounded() {
        let source = loaded().await;
        source.set_block(6, 70, 6, hive_state(5)).unwrap();
        ensure(&source, [6, 70, 6]);
        let type_id = hive_type_id().unwrap();
        let mut bad = Compound::new();
        bad.insert("ticks_in_hive".into(), Tag::Int(12));
        let mut not_bee = Compound::new();
        let mut entity_data = Compound::new();
        entity_data.insert("id".into(), Tag::String("minecraft:pig".into()));
        not_bee.insert("entity_data".into(), Tag::Compound(entity_data));
        source
            .update_block_entity(6, 70, 6, |entity| {
                let entity = entity.get_or_insert_with(|| BlockEntity {
                    type_id,
                    data: Compound::new(),
                });
                entity.data.insert(
                    "bees".into(),
                    Tag::List(List::Compound(vec![bad.clone(), not_bee.clone()])),
                );
                entity
                    .data
                    .insert("flower_pos".into(), Tag::List(List::Int(vec![1, 2, 3])));
                (true, ())
            })
            .unwrap();
        let data = load(&source, [6, 70, 6]);
        assert!(
            data.occupants.is_empty(),
            "non-bee and incomplete occupants are ignored"
        );
        assert_eq!(data.flower_pos, Some([1, 2, 3]));

        let five = u16::try_from(hive_state(5)).unwrap();
        let zero = u16::try_from(hive_state(0)).unwrap();
        assert_eq!(honey_level(five), Some(5));
        assert_eq!(with_honey_level(five, 0), Some(zero));
        assert_eq!(with_honey_level(zero, 99), Some(five));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn breaking_a_hive_returns_the_bees_component_and_its_occupants() {
        let source = loaded().await;
        source.set_block(8, 70, 8, hive_state(5)).unwrap();
        ensure(&source, [8, 70, 8]);
        let mut data = load(&source, [8, 70, 8]);
        let bee = bee_record(Vec3::new(8.5, 70.0, 8.5));
        assert!(data.add(&bee));
        store(&source, [8, 70, 8], &data);

        let (components, taken) = take(&source, [8, 70, 8]);
        assert_eq!(taken.occupants.len(), 1);
        assert!(source.block_entity(8, 70, 8).is_none());
        assert!(matches!(
            components.get("minecraft:bees"),
            Some(Tag::List(List::Compound(list))) if list.len() == 1
        ));

        let released = release(
            &source,
            [8, 70, 8],
            u16::try_from(hive_state(5)).unwrap(),
            &taken.occupants[0],
            true,
        )
        .expect("the bee comes back at the exit");
        assert_eq!(
            released.data.mob.as_ref().map(|mob| mob.kind),
            Some(MobKind::Bee)
        );
        assert!(released.pos.z < 8.0, "north-facing hive releases north");
    }

    #[test]
    fn bee_growables_advance_one_age_and_stop_at_max() {
        let wheat = ferrite_data::blocks::default_state("minecraft:wheat").unwrap();
        let young = grow_crop(wheat).expect("wheat is growable");
        assert_eq!(honey_level(young), None, "not a hive");
        let (_, props) = ferrite_data::blocks::state(young).unwrap();
        assert_eq!(
            props.iter().find(|(key, _)| *key == "age").map(|(_, v)| *v),
            Some("1")
        );
        let mature = ferrite_data::blocks::state_id("minecraft:wheat", &[("age", "7")]).unwrap();
        assert_eq!(grow_crop(mature), None, "mature crops do not grow");

        let berries =
            ferrite_data::blocks::state_id("minecraft:sweet_berry_bush", &[("age", "3")]).unwrap();
        assert_eq!(grow_crop(berries), None);
        let stone = ferrite_data::blocks::default_state("minecraft:stone").unwrap();
        assert_eq!(grow_crop(stone), None);

        let mut rng = ferrite_entity::Rng::new(3);
        assert!(delivered_honey(4, &mut rng) <= MAX_HONEY_LEVEL);
        assert_eq!(delivered_honey(MAX_HONEY_LEVEL, &mut rng), MAX_HONEY_LEVEL);
    }
}
