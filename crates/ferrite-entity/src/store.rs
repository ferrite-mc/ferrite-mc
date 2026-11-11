//! Region-local entity storage.
//!
//! An [`EntityStore`] holds the entities of one region. Slots are generational (a removed
//! entity's [`EntityKey`] never resolves again) and map to a dense index; the per-tick hot
//! fields (position, velocity, bounding box and flags) live in parallel dense columns so
//! physics and tracking passes walk contiguous memory, while everything else sits in a dense
//! [`EntityData`] column beside them. Removal swaps the last entity into the hole, so the
//! columns never have gaps.
//!
//! Every entity is also listed in the [`SectionIndex`] under the chunk section that contains
//! its position, which [`EntityStore::query`] uses for range and collision queries.
//!
//! Folia rule: a store is only touched by the region that owns it. Entities move between
//! regions by value as [`EntityRecord`]s: [`EntityStore::split`] and [`EntityStore::merge`]
//! follow the regionizer, and [`EntityStore::take_crossing`] removes entities that walked into
//! another region section so the caller can hand them to the owner of their new chunk.

use crate::arrow::ArrowState;
use crate::effect::ActiveEffects;
use crate::experience::OrbState;
use crate::fishing::FishingState;
use crate::id::EntityKey;
use crate::index::{Cell, SectionIndex};
use crate::item::ItemState;
use crate::metadata::Metadata;
use crate::mob::MobState;
use crate::tracker::TrackerState;
use crate::villager::VillagerState;
use ferrite_math::{AABB, Vec3};
use std::collections::{BTreeSet, HashMap};
use uuid::Uuid;

/// Chunk shift of a region section, matching `ferrite_region::Regionizer::default()`.
pub const DEFAULT_REGION_SHIFT: u32 = 4;

/// Boolean entity state, kept in the hot `flags` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Flags(pub u16);

impl Flags {
    pub const ON_GROUND: u16 = 1;
    pub const HORIZONTAL_COLLISION: u16 = 1 << 1;
    pub const VERTICAL_COLLISION: u16 = 1 << 2;
    /// Vanilla's `needsSync`: send position and velocity this tick even off-interval.
    pub const NEEDS_SYNC: u16 = 1 << 3;
    pub const NO_GRAVITY: u16 = 1 << 4;
    /// Removed this tick; dropped from the store after trackers have seen it.
    pub const REMOVED: u16 = 1 << 5;
    /// Moved by a teleport; trackers send a teleport instead of a delta move.
    pub const TELEPORTED: u16 = 1 << 6;
    pub const NO_PHYSICS: u16 = 1 << 7;

    pub fn has(self, flag: u16) -> bool {
        self.0 & flag != 0
    }

    pub fn set(&mut self, flag: u16, on: bool) {
        if on {
            self.0 |= flag;
        } else {
            self.0 &= !flag;
        }
    }
}

/// Everything about an entity except its hot columns.
#[derive(Debug, Clone)]
pub struct EntityData {
    /// Network id, from [`crate::allocate_id`].
    pub id: i32,
    pub uuid: Uuid,
    /// Entity type protocol id (`ferrite_data::entity_props`).
    pub kind: u16,
    /// Box width and height in blocks.
    pub width: f64,
    pub height: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub head_yaw: f32,
    /// Ticks lived, vanilla's `tickCount`.
    pub tick_count: u32,
    /// Synched data values.
    pub metadata: Metadata,
    /// Tracker state; travels with the entity between regions.
    pub tracker: TrackerState,
    /// Item entity state, for `minecraft:item`.
    pub item: Option<ItemState>,
    /// The potion stack of a thrown splash or lingering potion.
    pub potion: Option<ferrite_item::ItemStack>,
    /// Arrow state, for `minecraft:arrow`.
    pub arrow: Option<ArrowState>,
    /// Fishing-hook state, for `minecraft:fishing_bobber`.
    pub fishing: Option<FishingState>,
    /// Living state, for the modelled mobs.
    pub mob: Option<MobState>,
    /// Experience-orb state, for `minecraft:experience_orb`.
    pub orb: Option<OrbState>,
    /// Trade and profession state, for `minecraft:villager`.
    pub villager: Option<VillagerState>,
    /// Active mob effects (`LivingEntity.activeEffects`), one per effect id.
    pub effects: ActiveEffects,
    /// Block state carried by a falling block (vanilla `FallingBlockEntity.blockState`).
    pub block_state: Option<u16>,
    /// Remaining fuse ticks of a primed TNT entity (vanilla `PrimedTnt.fuse`).
    pub fuse: Option<i32>,
    /// Scoreboard tags (`/tag`), which entity selectors match with `tag=`.
    pub tags: BTreeSet<String>,
    /// Section the entity is listed under in the index.
    cell: Cell,
    /// Region section (chunk coordinates shifted by the store's shift) the entity was in when
    /// it entered this store.
    home: (i32, i32),
}

impl EntityData {
    pub fn new(id: i32, uuid: Uuid, kind: u16, width: f64, height: f64) -> Self {
        Self {
            id,
            uuid,
            kind,
            width,
            height,
            yaw: 0.0,
            pitch: 0.0,
            head_yaw: 0.0,
            tick_count: 0,
            metadata: Metadata::default(),
            tracker: TrackerState::default(),
            item: None,
            potion: None,
            arrow: None,
            fishing: None,
            mob: None,
            orb: None,
            villager: None,
            effects: ActiveEffects::default(),
            block_state: None,
            fuse: None,
            tags: BTreeSet::new(),
            cell: Cell::new(0, 0, 0),
            home: (0, 0),
        }
    }

    /// The bounding box of an entity of this size standing at `pos`.
    pub fn bounding_box(&self, pos: Vec3) -> AABB {
        let half = self.width / 2.0;
        AABB::new(
            pos.x - half,
            pos.y,
            pos.z - half,
            pos.x + half,
            pos.y + self.height,
            pos.z + half,
        )
    }
}

/// One entity by value: what moves between stores.
#[derive(Debug, Clone)]
pub struct EntityRecord {
    pub pos: Vec3,
    pub vel: Vec3,
    pub flags: Flags,
    pub data: EntityData,
}

impl EntityRecord {
    /// The chunk the entity stands in.
    pub fn chunk(&self) -> (i32, i32) {
        chunk_of(self.pos)
    }
}

/// The chunk containing a position.
pub fn chunk_of(pos: Vec3) -> (i32, i32) {
    ((pos.x.floor() as i32) >> 4, (pos.z.floor() as i32) >> 4)
}

#[derive(Debug, Clone, Copy)]
struct Slot {
    generation: u32,
    /// Dense index, or [`FREE`] when the slot is unused.
    dense: u32,
}

const FREE: u32 = u32::MAX;

/// The entities of one region; see the module documentation.
#[derive(Debug)]
pub struct EntityStore {
    shift: u32,
    slots: Vec<Slot>,
    free: Vec<u32>,
    keys: Vec<EntityKey>,
    pos: Vec<Vec3>,
    vel: Vec<Vec3>,
    bbox: Vec<AABB>,
    flags: Vec<Flags>,
    data: Vec<EntityData>,
    by_id: HashMap<i32, EntityKey>,
    index: SectionIndex,
}

impl Default for EntityStore {
    fn default() -> Self {
        Self::new(DEFAULT_REGION_SHIFT)
    }
}

impl EntityStore {
    /// An empty store for regions made of `2^shift` x `2^shift` chunk sections.
    pub fn new(shift: u32) -> Self {
        Self {
            shift,
            slots: Vec::new(),
            free: Vec::new(),
            keys: Vec::new(),
            pos: Vec::new(),
            vel: Vec::new(),
            bbox: Vec::new(),
            flags: Vec::new(),
            data: Vec::new(),
            by_id: HashMap::new(),
            index: SectionIndex::default(),
        }
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    fn region_section(&self, pos: Vec3) -> (i32, i32) {
        let (x, z) = chunk_of(pos);
        (x >> self.shift, z >> self.shift)
    }

    /// Adds an entity and returns its key.
    pub fn insert(&mut self, record: EntityRecord) -> EntityKey {
        let EntityRecord {
            pos,
            vel,
            flags,
            mut data,
        } = record;
        let dense = self.keys.len() as u32;
        let key = match self.free.pop() {
            Some(index) => {
                let slot = &mut self.slots[index as usize];
                slot.dense = dense;
                EntityKey {
                    index,
                    generation: slot.generation,
                }
            }
            None => {
                self.slots.push(Slot {
                    generation: 0,
                    dense,
                });
                EntityKey {
                    index: self.slots.len() as u32 - 1,
                    generation: 0,
                }
            }
        };
        data.cell = Cell::containing(pos);
        data.home = self.region_section(pos);
        self.index.insert(data.cell, key);
        self.by_id.insert(data.id, key);
        self.keys.push(key);
        self.bbox.push(data.bounding_box(pos));
        self.pos.push(pos);
        self.vel.push(vel);
        self.flags.push(flags);
        self.data.push(data);
        key
    }

    /// Dense column index of a live entity.
    pub fn dense(&self, key: EntityKey) -> Option<usize> {
        let slot = self.slots.get(key.index as usize)?;
        (slot.generation == key.generation && slot.dense != FREE).then_some(slot.dense as usize)
    }

    pub fn contains(&self, key: EntityKey) -> bool {
        self.dense(key).is_some()
    }

    /// Key of the entity with a network id.
    pub fn key_of(&self, id: i32) -> Option<EntityKey> {
        self.by_id.get(&id).copied()
    }

    /// Removes an entity and returns it by value.
    pub fn remove(&mut self, key: EntityKey) -> Option<EntityRecord> {
        let dense = self.dense(key)?;
        let slot = &mut self.slots[key.index as usize];
        slot.dense = FREE;
        slot.generation = slot.generation.wrapping_add(1);
        self.free.push(key.index);
        self.keys.swap_remove(dense);
        if let Some(moved) = self.keys.get(dense) {
            self.slots[moved.index as usize].dense = dense as u32;
        }
        self.bbox.swap_remove(dense);
        let data = self.data.swap_remove(dense);
        self.index.remove(data.cell, key);
        self.by_id.remove(&data.id);
        Some(EntityRecord {
            pos: self.pos.swap_remove(dense),
            vel: self.vel.swap_remove(dense),
            flags: self.flags.swap_remove(dense),
            data,
        })
    }

    /// Keys in dense order. Collect them before mutating the store while iterating.
    pub fn keys(&self) -> &[EntityKey] {
        &self.keys
    }

    pub fn positions(&self) -> &[Vec3] {
        &self.pos
    }

    pub fn velocities(&self) -> &[Vec3] {
        &self.vel
    }

    pub fn boxes(&self) -> &[AABB] {
        &self.bbox
    }

    pub fn all_flags(&self) -> &[Flags] {
        &self.flags
    }

    pub fn pos(&self, key: EntityKey) -> Option<Vec3> {
        self.dense(key).map(|i| self.pos[i])
    }

    pub fn vel(&self, key: EntityKey) -> Option<Vec3> {
        self.dense(key).map(|i| self.vel[i])
    }

    pub fn bbox(&self, key: EntityKey) -> Option<AABB> {
        self.dense(key).map(|i| self.bbox[i])
    }

    pub fn flags(&self, key: EntityKey) -> Option<Flags> {
        self.dense(key).map(|i| self.flags[i])
    }

    pub fn flags_mut(&mut self, key: EntityKey) -> Option<&mut Flags> {
        self.dense(key).map(|i| &mut self.flags[i])
    }

    pub fn data(&self, key: EntityKey) -> Option<&EntityData> {
        self.dense(key).map(|i| &self.data[i])
    }

    pub fn data_mut(&mut self, key: EntityKey) -> Option<&mut EntityData> {
        self.dense(key).map(|i| &mut self.data[i])
    }

    pub fn set_vel(&mut self, key: EntityKey, vel: Vec3) {
        if let Some(i) = self.dense(key) {
            self.vel[i] = vel;
        }
    }

    /// Moves an entity, keeping its box and section listing in step.
    pub fn set_pos(&mut self, key: EntityKey, pos: Vec3) {
        let Some(i) = self.dense(key) else { return };
        self.pos[i] = pos;
        self.bbox[i] = self.data[i].bounding_box(pos);
        let cell = Cell::containing(pos);
        if cell != self.data[i].cell {
            self.index.remove(self.data[i].cell, key);
            self.index.insert(cell, key);
            self.data[i].cell = cell;
        }
    }

    /// Calls `f` for every entity whose box intersects `area`.
    pub fn query(&self, area: AABB, mut f: impl FnMut(EntityKey)) {
        self.index.for_each_near(area, |key| {
            if let Some(i) = self.dense(key)
                && self.bbox[i].intersects(area)
            {
                f(key);
            }
        });
    }

    /// Entities whose box intersects `area`.
    pub fn query_vec(&self, area: AABB) -> Vec<EntityKey> {
        let mut out = Vec::new();
        self.query(area, |key| out.push(key));
        out
    }

    /// The section index, for callers that want raw section lists.
    pub fn index(&self) -> &SectionIndex {
        &self.index
    }

    /// Removes and returns every entity standing in a chunk for which `take` is true; used by
    /// `RegionData::split`.
    pub fn split(&mut self, take: impl Fn((i32, i32)) -> bool) -> Self {
        let mut out = Self::new(self.shift);
        let leaving: Vec<_> = self
            .keys
            .iter()
            .zip(&self.pos)
            .filter(|(_, pos)| take(chunk_of(**pos)))
            .map(|(key, _)| *key)
            .collect();
        for key in leaving {
            if let Some(record) = self.remove(key) {
                out.insert(record);
            }
        }
        out
    }

    /// Moves every entity of `other` into this store; used by `RegionData::merge`. The moved
    /// entities get new keys.
    pub fn merge(&mut self, mut other: Self) {
        while let Some(&key) = other.keys.last() {
            if let Some(record) = other.remove(key) {
                self.insert(record);
            }
        }
    }

    /// Removes and returns the entities that moved into a different region section since they
    /// entered this store. The new section may belong to another region, so the caller hands
    /// each record to the owner of [`EntityRecord::chunk`] (Folia: via that region's task
    /// queue); re-inserting a record that came back to this region is harmless.
    pub fn take_crossing(&mut self) -> Vec<EntityRecord> {
        let leaving: Vec<_> = (0..self.keys.len())
            .filter(|&i| self.region_section(self.pos[i]) != self.data[i].home)
            .map(|i| self.keys[i])
            .collect();
        leaving
            .into_iter()
            .filter_map(|key| self.remove(key))
            .collect()
    }

    /// Removes and returns entities flagged [`Flags::REMOVED`].
    pub fn sweep_removed(&mut self) -> Vec<EntityRecord> {
        let dead: Vec<_> = (0..self.keys.len())
            .filter(|&i| self.flags[i].has(Flags::REMOVED))
            .map(|i| self.keys[i])
            .collect();
        dead.into_iter()
            .filter_map(|key| self.remove(key))
            .collect()
    }

    /// Checks that slots, columns, the id map and the section index agree.
    pub fn check_invariants(&self) -> Result<(), String> {
        let n = self.keys.len();
        if [
            self.pos.len(),
            self.vel.len(),
            self.bbox.len(),
            self.flags.len(),
            self.data.len(),
        ]
        .iter()
        .any(|len| *len != n)
        {
            return Err("column lengths differ".into());
        }
        if self.by_id.len() != n {
            return Err("id map size differs from entity count".into());
        }
        if self.index.total() != n {
            return Err("section index lists stale entities".into());
        }
        for (i, key) in self.keys.iter().enumerate() {
            if self.dense(*key) != Some(i) {
                return Err(format!("slot of dense entity {i} is stale"));
            }
            let data = &self.data[i];
            if data.cell != Cell::containing(self.pos[i]) {
                return Err(format!("entity {} listed in the wrong section", data.id));
            }
            if !self.index.get(data.cell).contains(key) {
                return Err(format!("entity {} missing from its section", data.id));
            }
            if self.by_id.get(&data.id) != Some(key) {
                return Err(format!("entity {} missing from the id map", data.id));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::{allocate_id, random_uuid};

    fn record(x: f64, y: f64, z: f64) -> EntityRecord {
        EntityRecord {
            pos: Vec3::new(x, y, z),
            vel: Vec3::ZERO,
            flags: Flags::default(),
            data: EntityData::new(allocate_id(), random_uuid(), 71, 0.25, 0.25),
        }
    }

    #[test]
    fn removed_keys_never_resolve_again() {
        let mut store = EntityStore::default();
        let a = store.insert(record(0.5, 64.0, 0.5));
        let b = store.insert(record(1.5, 64.0, 0.5));
        let id_b = store.data(b).unwrap().id;
        assert!(store.remove(a).is_some());
        assert!(store.remove(a).is_none());
        // `b` was swapped into the hole and still resolves.
        assert_eq!(store.data(b).unwrap().id, id_b);
        let c = store.insert(record(2.5, 64.0, 0.5));
        assert_ne!(a, c);
        assert!(!store.contains(a));
        assert_eq!(store.key_of(id_b), Some(b));
        store.check_invariants().unwrap();
    }

    #[test]
    fn moving_entities_updates_sections_and_queries() {
        let mut store = EntityStore::default();
        let a = store.insert(record(0.5, 64.0, 0.5));
        let far = store.insert(record(100.5, 64.0, 100.5));
        let near = AABB::new(0.0, 63.0, 0.0, 1.0, 65.0, 1.0);
        assert_eq!(store.query_vec(near), vec![a]);
        store.set_pos(a, Vec3::new(-20.5, 10.0, 40.0));
        assert!(store.query_vec(near).is_empty());
        let moved = AABB::new(-21.0, 9.0, 39.5, -20.0, 11.0, 40.5);
        assert_eq!(store.query_vec(moved), vec![a]);
        // An entity listed in the neighbouring section is still found through the margin.
        store.set_pos(far, Vec3::new(16.05, 64.0, 0.5));
        let edge = AABB::new(15.8, 64.0, 0.0, 15.95, 64.2, 1.0);
        assert_eq!(store.query_vec(edge), vec![far]);
        store.check_invariants().unwrap();
    }

    #[test]
    fn split_and_merge_follow_chunks() {
        let mut store = EntityStore::default();
        for x in 0..20 {
            store.insert(record(f64::from(x) * 40.0, 64.0, 0.0));
        }
        let east = store.split(|(cx, _)| (cx >> 4) >= 2);
        assert!(east.positions().iter().all(|p| p.x >= 512.0));
        assert!(store.positions().iter().all(|p| p.x < 512.0));
        assert_eq!(store.len() + east.len(), 20);
        store.check_invariants().unwrap();
        east.check_invariants().unwrap();
        store.merge(east);
        assert_eq!(store.len(), 20);
        store.check_invariants().unwrap();
    }

    #[test]
    fn crossing_a_region_section_hands_the_entity_out() {
        let mut store = EntityStore::default();
        let key = store.insert(record(250.0, 64.0, 0.0));
        let stay = store.insert(record(10.0, 64.0, 0.0));
        store.set_pos(key, Vec3::new(255.9, 64.0, 0.0));
        assert!(store.take_crossing().is_empty());
        store.set_pos(key, Vec3::new(256.1, 64.0, 0.0));
        let out = store.take_crossing();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].chunk(), (16, 0));
        assert!(store.contains(stay));
        assert!(!store.contains(key));
        // The receiving store records the new section as home.
        let mut other = EntityStore::default();
        other.insert(out.into_iter().next().unwrap());
        assert!(other.take_crossing().is_empty());
        store.check_invariants().unwrap();
    }

    #[test]
    fn removed_flag_is_swept() {
        let mut store = EntityStore::default();
        let a = store.insert(record(0.0, 64.0, 0.0));
        store.insert(record(1.0, 64.0, 0.0));
        store.flags_mut(a).unwrap().set(Flags::REMOVED, true);
        assert_eq!(store.sweep_removed().len(), 1);
        assert_eq!(store.len(), 1);
        store.check_invariants().unwrap();
    }
}
