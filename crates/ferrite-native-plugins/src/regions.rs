//! WorldGuard-style 3D AABB region protection engine.
//!
//! Regions are scoped per world with normalized `[min, max]` block coordinates, integer priority,
//! owner/member UUID sets, and optional protection flags. Queries use a per-world chunk-column
//! spatial index for fast lookup while resolving overlapping priorities and member bypasses.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{PluginError, Result};
use crate::storage::{load_json, load_json_or_default, save_json_atomic};

/// Maximum number of chunk columns indexed per region before placing it in the world-wide
/// large-region bucket so massive regions do not bloat the spatial hash.
const MAX_INDEXED_CHUNKS: i64 = 512;

/// Protection flags for a 3D region. `None` means unset (inherit from lower-priority regions).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegionFlags {
    /// General building permission (fallback for `block_break` and `block_place`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<bool>,
    /// Breaking blocks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_break: Option<bool>,
    /// Placing blocks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_place: Option<bool>,
    /// Interacting with containers, doors, buttons, and entities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interact: Option<bool>,
    /// Player-versus-player combat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pvp: Option<bool>,
    /// Explosion block/entity damage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explosions: Option<bool>,
    /// Player entry into the region.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<bool>,
}

/// A named 3D axis-aligned bounding box protection region inside a world.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Region {
    /// Canonical region name (unique within `world`).
    pub name: String,
    /// World name / identifier.
    pub world: String,
    /// Inclusive minimum `[x, y, z]` block corner.
    pub min: [i32; 3],
    /// Inclusive maximum `[x, y, z]` block corner.
    pub max: [i32; 3],
    /// Priority (higher overrides lower).
    pub priority: i32,
    /// Owners who bypass deny flags.
    #[serde(default)]
    pub owners: HashSet<Uuid>,
    /// Members who bypass deny flags.
    #[serde(default)]
    pub members: HashSet<Uuid>,
    /// Region protection flags.
    #[serde(default)]
    pub flags: RegionFlags,
}

impl Region {
    /// Creates a new region with normalized `[min, max]` bounds.
    pub fn new(
        world: impl Into<String>,
        name: impl Into<String>,
        corner_a: [i32; 3],
        corner_b: [i32; 3],
    ) -> Self {
        let min = [
            corner_a[0].min(corner_b[0]),
            corner_a[1].min(corner_b[1]),
            corner_a[2].min(corner_b[2]),
        ];
        let max = [
            corner_a[0].max(corner_b[0]),
            corner_a[1].max(corner_b[1]),
            corner_a[2].max(corner_b[2]),
        ];
        Self {
            name: name.into(),
            world: world.into(),
            min,
            max,
            priority: 0,
            owners: HashSet::new(),
            members: HashSet::new(),
            flags: RegionFlags::default(),
        }
    }

    /// Returns `true` if block coordinate `(x, y, z)` lies inside `[min, max]` inclusively.
    #[inline]
    pub fn contains(&self, x: i32, y: i32, z: i32) -> bool {
        x >= self.min[0]
            && x <= self.max[0]
            && y >= self.min[1]
            && y <= self.max[1]
            && z >= self.min[2]
            && z <= self.max[2]
    }

    /// Returns `true` if `player` is an owner or member of this region.
    #[inline]
    pub fn is_trusted(&self, player: Uuid) -> bool {
        self.owners.contains(&player) || self.members.contains(&player)
    }
}

/// Per-world priority-sorted region list and chunk-column spatial index.
#[derive(Debug, Clone, Default)]
struct WorldSpatialIndex {
    /// Regions sorted by `(priority DESC, name ASC)`.
    sorted: Vec<Region>,
    /// Chunk `(cx, cz)` -> indices into `sorted` (already in priority-descending order).
    chunks: HashMap<(i32, i32), Vec<usize>>,
    /// Indices into `sorted` for regions spanning more than `MAX_INDEXED_CHUNKS` chunk columns.
    large: Vec<usize>,
}

impl WorldSpatialIndex {
    fn rebuild(&mut self, regions: impl IntoIterator<Item = Region>) {
        self.sorted.clear();
        self.chunks.clear();
        self.large.clear();

        self.sorted.extend(regions);
        self.sorted.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| a.name.cmp(&b.name))
        });

        for (idx, region) in self.sorted.iter().enumerate() {
            let min_cx = region.min[0] >> 4;
            let max_cx = region.max[0] >> 4;
            let min_cz = region.min[2] >> 4;
            let max_cz = region.max[2] >> 4;

            let span_x = i64::from(max_cx) - i64::from(min_cx) + 1;
            let span_z = i64::from(max_cz) - i64::from(min_cz) + 1;
            if span_x.saturating_mul(span_z) > MAX_INDEXED_CHUNKS {
                self.large.push(idx);
            } else {
                for cx in min_cx..=max_cx {
                    for cz in min_cz..=max_cz {
                        self.chunks.entry((cx, cz)).or_default().push(idx);
                    }
                }
            }
        }
    }

    /// Collects all regions covering `(x, y, z)` in `(priority DESC, name ASC)` order.
    fn query_at(&self, x: i32, y: i32, z: i32) -> Vec<&Region> {
        let cx = x >> 4;
        let cz = z >> 4;
        let chunk_hits = self.chunks.get(&(cx, cz)).map(Vec::as_slice).unwrap_or(&[]);

        // Merge two sorted slices (`chunk_hits` and `self.large` are both sorted by index asc,
        // which corresponds to priority descending).
        let mut out = Vec::new();
        let mut i = 0;
        let mut j = 0;
        while i < chunk_hits.len() || j < self.large.len() {
            let idx = match (chunk_hits.get(i), self.large.get(j)) {
                (Some(&a), Some(&b)) => match a.cmp(&b) {
                    Ordering::Less => {
                        i += 1;
                        a
                    }
                    Ordering::Greater => {
                        j += 1;
                        b
                    }
                    Ordering::Equal => {
                        i += 1;
                        j += 1;
                        a
                    }
                },
                (Some(&a), None) => {
                    i += 1;
                    a
                }
                (None, Some(&b)) => {
                    j += 1;
                    b
                }
                (None, None) => break,
            };

            let region = &self.sorted[idx];
            if region.contains(x, y, z) {
                out.push(region);
            }
        }
        out
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct RegionEngineSerde {
    #[serde(default)]
    worlds: BTreeMap<String, BTreeMap<String, Region>>,
}

impl From<RegionEngineSerde> for RegionEngine {
    fn from(raw: RegionEngineSerde) -> Self {
        let mut engine = Self {
            worlds: raw.worlds,
            indices: HashMap::new(),
        };
        engine.rebuild_all_indices();
        engine
    }
}

/// WorldGuard-style 3D region protection engine.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(from = "RegionEngineSerde")]
pub struct RegionEngine {
    /// Regions grouped by `world` -> `region_name`.
    #[serde(default)]
    worlds: BTreeMap<String, BTreeMap<String, Region>>,
    /// Non-serialized spatial lookup indices per world.
    #[serde(skip)]
    indices: HashMap<String, WorldSpatialIndex>,
}

impl PartialEq for RegionEngine {
    fn eq(&self, other: &Self) -> bool {
        self.worlds == other.worlds
    }
}

impl Eq for RegionEngine {}

/// Selects which player-facing flag check to run.
#[derive(Debug, Clone, Copy)]
enum PlayerAction {
    Break,
    Place,
    Interact,
    Entry,
}

impl RegionEngine {
    /// Creates an empty region protection engine.
    pub fn new() -> Self {
        Self::default()
    }

    fn normalize_world(world: &str) -> Result<String> {
        let w = world.trim().to_owned();
        if w.is_empty() {
            return Err(PluginError::InvalidIdentifier(world.to_owned()));
        }
        Ok(w)
    }

    fn normalize_name(name: &str) -> Result<String> {
        let n = name.trim().to_ascii_lowercase();
        if n.is_empty() || n.chars().any(char::is_whitespace) {
            return Err(PluginError::InvalidIdentifier(name.to_owned()));
        }
        Ok(n)
    }

    fn rebuild_world_index(&mut self, world: &str) {
        if let Some(regions) = self.worlds.get(world) {
            let mut idx = WorldSpatialIndex::default();
            idx.rebuild(regions.values().cloned());
            self.indices.insert(world.to_owned(), idx);
        } else {
            self.indices.remove(world);
        }
    }

    fn rebuild_all_indices(&mut self) {
        self.indices.clear();
        let worlds: Vec<String> = self.worlds.keys().cloned().collect();
        for w in worlds {
            self.rebuild_world_index(&w);
        }
    }

    /// Defines a new region in `world` with bounds normalized from `corner_a` and `corner_b`.
    pub fn define_region(
        &mut self,
        world: &str,
        name: &str,
        corner_a: [i32; 3],
        corner_b: [i32; 3],
    ) -> Result<()> {
        let world_key = Self::normalize_world(world)?;
        let name_key = Self::normalize_name(name)?;
        let world_map = self.worlds.entry(world_key.clone()).or_default();
        if world_map.contains_key(&name_key) {
            return Err(PluginError::RegionAlreadyExists {
                world: world_key,
                name: name_key,
            });
        }
        let region = Region::new(world_key.clone(), name_key.clone(), corner_a, corner_b);
        world_map.insert(name_key, region);
        self.rebuild_world_index(&world_key);
        Ok(())
    }

    /// Inserts or replaces a region directly, rebuilding that world's spatial index.
    pub fn upsert_region(&mut self, mut region: Region) -> Result<()> {
        region.world = Self::normalize_world(&region.world)?;
        region.name = Self::normalize_name(&region.name)?;
        let min = [
            region.min[0].min(region.max[0]),
            region.min[1].min(region.max[1]),
            region.min[2].min(region.max[2]),
        ];
        let max = [
            region.min[0].max(region.max[0]),
            region.min[1].max(region.max[1]),
            region.min[2].max(region.max[2]),
        ];
        region.min = min;
        region.max = max;

        let world_key = region.world.clone();
        self.worlds
            .entry(world_key.clone())
            .or_default()
            .insert(region.name.clone(), region);
        self.rebuild_world_index(&world_key);
        Ok(())
    }

    /// Deletes a region from `world`.
    pub fn remove_region(&mut self, world: &str, name: &str) -> Result<bool> {
        let world_key = Self::normalize_world(world)?;
        let name_key = Self::normalize_name(name)?;
        let removed = self
            .worlds
            .get_mut(&world_key)
            .is_some_and(|m| m.remove(&name_key).is_some());
        if removed {
            if self.worlds.get(&world_key).is_some_and(BTreeMap::is_empty) {
                self.worlds.remove(&world_key);
            }
            self.rebuild_world_index(&world_key);
        }
        Ok(removed)
    }

    /// Returns a reference to a region by world and name.
    pub fn get_region(&self, world: &str, name: &str) -> Option<&Region> {
        let name_key = Self::normalize_name(name).ok()?;
        self.worlds.get(world.trim())?.get(&name_key)
    }

    /// Lists all region names in `world` in alphabetical order.
    pub fn list_regions(&self, world: &str) -> Vec<String> {
        self.worlds
            .get(world.trim())
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Sets the priority of an existing region and updates the spatial index.
    pub fn set_priority(&mut self, world: &str, name: &str, priority: i32) -> Result<()> {
        let world_key = Self::normalize_world(world)?;
        let name_key = Self::normalize_name(name)?;
        let region = self
            .worlds
            .get_mut(&world_key)
            .and_then(|m| m.get_mut(&name_key))
            .ok_or_else(|| PluginError::RegionNotFound {
                world: world_key.clone(),
                name: name_key,
            })?;
        region.priority = priority;
        self.rebuild_world_index(&world_key);
        Ok(())
    }

    /// Updates a region's flags via a closure and syncs the spatial index.
    pub fn update_flags(
        &mut self,
        world: &str,
        name: &str,
        f: impl FnOnce(&mut RegionFlags),
    ) -> Result<()> {
        let world_key = Self::normalize_world(world)?;
        let name_key = Self::normalize_name(name)?;
        let region = self
            .worlds
            .get_mut(&world_key)
            .and_then(|m| m.get_mut(&name_key))
            .ok_or_else(|| PluginError::RegionNotFound {
                world: world_key.clone(),
                name: name_key,
            })?;
        f(&mut region.flags);
        self.rebuild_world_index(&world_key);
        Ok(())
    }

    /// Adds an owner UUID to a region.
    pub fn add_owner(&mut self, world: &str, name: &str, owner: Uuid) -> Result<bool> {
        let world_key = Self::normalize_world(world)?;
        let name_key = Self::normalize_name(name)?;
        let region = self
            .worlds
            .get_mut(&world_key)
            .and_then(|m| m.get_mut(&name_key))
            .ok_or_else(|| PluginError::RegionNotFound {
                world: world_key.clone(),
                name: name_key,
            })?;
        let inserted = region.owners.insert(owner);
        self.rebuild_world_index(&world_key);
        Ok(inserted)
    }

    /// Removes an owner UUID from a region.
    pub fn remove_owner(&mut self, world: &str, name: &str, owner: Uuid) -> Result<bool> {
        let world_key = Self::normalize_world(world)?;
        let name_key = Self::normalize_name(name)?;
        let region = self
            .worlds
            .get_mut(&world_key)
            .and_then(|m| m.get_mut(&name_key))
            .ok_or_else(|| PluginError::RegionNotFound {
                world: world_key.clone(),
                name: name_key,
            })?;
        let removed = region.owners.remove(&owner);
        self.rebuild_world_index(&world_key);
        Ok(removed)
    }

    /// Adds a member UUID to a region.
    pub fn add_member(&mut self, world: &str, name: &str, member: Uuid) -> Result<bool> {
        let world_key = Self::normalize_world(world)?;
        let name_key = Self::normalize_name(name)?;
        let region = self
            .worlds
            .get_mut(&world_key)
            .and_then(|m| m.get_mut(&name_key))
            .ok_or_else(|| PluginError::RegionNotFound {
                world: world_key.clone(),
                name: name_key,
            })?;
        let inserted = region.members.insert(member);
        self.rebuild_world_index(&world_key);
        Ok(inserted)
    }

    /// Removes a member UUID from a region.
    pub fn remove_member(&mut self, world: &str, name: &str, member: Uuid) -> Result<bool> {
        let world_key = Self::normalize_world(world)?;
        let name_key = Self::normalize_name(name)?;
        let region = self
            .worlds
            .get_mut(&world_key)
            .and_then(|m| m.get_mut(&name_key))
            .ok_or_else(|| PluginError::RegionNotFound {
                world: world_key.clone(),
                name: name_key,
            })?;
        let removed = region.members.remove(&member);
        self.rebuild_world_index(&world_key);
        Ok(removed)
    }

    /// Returns all overlapping regions at `(world, x, y, z)` sorted by priority descending.
    pub fn regions_at(&self, world: &str, x: i32, y: i32, z: i32) -> Vec<&Region> {
        self.indices
            .get(world.trim())
            .map(|idx| idx.query_at(x, y, z))
            .unwrap_or_default()
    }

    /// Returns the highest-priority region covering `(world, x, y, z)`, if any.
    pub fn highest_priority_region(&self, world: &str, x: i32, y: i32, z: i32) -> Option<&Region> {
        self.regions_at(world, x, y, z).into_iter().next()
    }

    /// Resolves a player action across overlapping regions ordered by priority descending.
    fn check_player_action(
        &self,
        world: &str,
        x: i32,
        y: i32,
        z: i32,
        player: Uuid,
        action: PlayerAction,
    ) -> bool {
        let hits = self.regions_at(world, x, y, z);
        if hits.is_empty() {
            return true;
        }

        let mut higher_tier_trusted = false;
        let mut idx = 0;

        while idx < hits.len() {
            let tier_priority = hits[idx].priority;
            let mut tier_end = idx + 1;
            while tier_end < hits.len() && hits[tier_end].priority == tier_priority {
                tier_end += 1;
            }
            let tier = &hits[idx..tier_end];

            // 1) Specific flag on this priority tier.
            let mut saw_specific = false;
            let mut specific_allowed = true;
            for region in tier {
                let flag = match action {
                    PlayerAction::Break => region.flags.block_break,
                    PlayerAction::Place => region.flags.block_place,
                    PlayerAction::Interact => region.flags.interact,
                    PlayerAction::Entry => region.flags.entry,
                };
                if let Some(val) = flag {
                    saw_specific = true;
                    if !val && !higher_tier_trusted && !region.is_trusted(player) {
                        specific_allowed = false;
                    }
                }
            }
            if saw_specific {
                return specific_allowed;
            }

            // 2) For break/place, fall back to `build` on this priority tier.
            if matches!(action, PlayerAction::Break | PlayerAction::Place) {
                let mut saw_build = false;
                let mut build_allowed = true;
                for region in tier {
                    if let Some(val) = region.flags.build {
                        saw_build = true;
                        if !val && !higher_tier_trusted && !region.is_trusted(player) {
                            build_allowed = false;
                        }
                    }
                }
                if saw_build {
                    return build_allowed;
                }
            }

            if tier.iter().any(|r| r.is_trusted(player)) {
                higher_tier_trusted = true;
            }
            idx = tier_end;
        }

        true
    }

    /// Resolves a non-player flag across overlapping regions sorted by priority descending.
    fn check_world_flag(
        &self,
        world: &str,
        x: i32,
        y: i32,
        z: i32,
        getter: impl Fn(&RegionFlags) -> Option<bool>,
    ) -> bool {
        let hits = self.regions_at(world, x, y, z);
        let mut idx = 0;
        while idx < hits.len() {
            let tier_priority = hits[idx].priority;
            let mut tier_end = idx + 1;
            while tier_end < hits.len() && hits[tier_end].priority == tier_priority {
                tier_end += 1;
            }
            let mut saw = false;
            let mut allowed = true;
            for region in &hits[idx..tier_end] {
                if let Some(val) = getter(&region.flags) {
                    saw = true;
                    if !val {
                        allowed = false;
                    }
                }
            }
            if saw {
                return allowed;
            }
            idx = tier_end;
        }
        true
    }

    /// Returns `true` if `player` is allowed to break the block at `(world, x, y, z)`.
    pub fn can_break(&self, world: &str, x: i32, y: i32, z: i32, player: Uuid) -> bool {
        self.check_player_action(world, x, y, z, player, PlayerAction::Break)
    }

    /// Returns `true` if `player` is allowed to place a block at `(world, x, y, z)`.
    pub fn can_place(&self, world: &str, x: i32, y: i32, z: i32, player: Uuid) -> bool {
        self.check_player_action(world, x, y, z, player, PlayerAction::Place)
    }

    /// Returns `true` if `player` is allowed to interact at `(world, x, y, z)`.
    pub fn can_interact(&self, world: &str, x: i32, y: i32, z: i32, player: Uuid) -> bool {
        self.check_player_action(world, x, y, z, player, PlayerAction::Interact)
    }

    /// Returns `true` if `player` is allowed to enter `(world, x, y, z)`.
    pub fn can_enter(&self, world: &str, x: i32, y: i32, z: i32, player: Uuid) -> bool {
        self.check_player_action(world, x, y, z, player, PlayerAction::Entry)
    }

    /// Returns `true` if PvP combat is allowed at `(world, x, y, z)`.
    pub fn can_pvp(&self, world: &str, x: i32, y: i32, z: i32) -> bool {
        self.check_world_flag(world, x, y, z, |f| f.pvp)
    }

    /// Returns `true` if explosions are allowed at `(world, x, y, z)`.
    pub fn can_explode(&self, world: &str, x: i32, y: i32, z: i32) -> bool {
        self.check_world_flag(world, x, y, z, |f| f.explosions)
    }

    /// Saves all regions atomically to a JSON file.
    pub fn save_to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        save_json_atomic(path.as_ref(), self)
    }

    /// Loads regions from a JSON file and rebuilds the spatial index.
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        load_json(path.as_ref())
    }

    /// Loads regions from a JSON file, or returns an empty engine if missing.
    pub fn load_or_default(path: impl AsRef<Path>) -> Result<Self> {
        load_json_or_default(path.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn priority_override_and_member_bypass() {
        let mut rg = RegionEngine::new();
        let owner = Uuid::new_v4();
        let member = Uuid::new_v4();
        let stranger = Uuid::new_v4();

        rg.define_region("world", "spawn", [50, 0, 50], [-50, 255, -50])
            .unwrap();
        rg.set_priority("world", "spawn", 10).unwrap();
        rg.add_owner("world", "spawn", owner).unwrap();
        rg.update_flags("world", "spawn", |f| {
            f.build = Some(false);
            f.interact = Some(false);
            f.pvp = Some(false);
            f.explosions = Some(false);
            f.entry = Some(true);
        })
        .unwrap();

        // Nested PvP arena inside spawn with higher priority.
        rg.define_region("world", "arena", [-10, 60, -10], [10, 80, 10])
            .unwrap();
        rg.set_priority("world", "arena", 20).unwrap();
        rg.add_member("world", "arena", member).unwrap();
        rg.update_flags("world", "arena", |f| {
            f.pvp = Some(true);
            f.block_place = Some(false);
        })
        .unwrap();

        // Outside regions: everything allowed.
        assert!(rg.can_break("world", 100, 64, 100, stranger));
        assert!(rg.can_pvp("world", 100, 64, 100));
        assert!(rg.can_explode("world", 100, 64, 100));

        // Inside spawn (outside arena): stranger blocked, owner bypasses.
        assert!(!rg.can_break("world", 30, 64, 30, stranger));
        assert!(!rg.can_place("world", 30, 64, 30, stranger));
        assert!(!rg.can_interact("world", 30, 64, 30, stranger));
        assert!(rg.can_enter("world", 30, 64, 30, stranger));
        assert!(!rg.can_pvp("world", 30, 64, 30));
        assert!(!rg.can_explode("world", 30, 64, 30));
        assert!(rg.can_break("world", 30, 64, 30, owner));

        // Inside arena: PvP allowed by higher-priority arena, build still denied to strangers,
        // while arena member bypasses into building/placing inside arena.
        assert!(rg.can_pvp("world", 0, 64, 0));
        assert!(!rg.can_explode("world", 0, 64, 0));
        assert!(!rg.can_place("world", 0, 64, 0, stranger));
        assert!(!rg.can_break("world", 0, 64, 0, stranger));
        assert!(rg.can_place("world", 0, 64, 0, member));
        assert!(rg.can_break("world", 0, 64, 0, member));
        // Owner of spawn (priority 10) is NOT trusted in arena (priority 20) where block_place=false.
        assert!(!rg.can_place("world", 0, 64, 0, owner));
    }

    #[test]
    fn large_region_and_json_round_trip() {
        let mut rg = RegionEngine::new();
        let stranger = Uuid::new_v4();

        // Huge region (> 512 chunks) uses the large-region bucket.
        rg.define_region(
            "world_nether",
            "warzone",
            [-2000, 0, -2000],
            [2000, 128, 2000],
        )
        .unwrap();
        rg.update_flags("world_nether", "warzone", |f| {
            f.explosions = Some(false);
            f.entry = Some(false);
        })
        .unwrap();

        let file = NamedTempFile::new().unwrap();
        rg.save_to_file(file.path()).unwrap();
        let loaded = RegionEngine::load_from_file(file.path()).unwrap();

        assert_eq!(rg, loaded);
        assert!(!loaded.can_explode("world_nether", 500, 64, -500));
        assert!(!loaded.can_enter("world_nether", -1500, 10, 1500, stranger));
        assert_eq!(
            loaded
                .highest_priority_region("world_nether", 0, 64, 0)
                .map(|r| r.name.as_str()),
            Some("warzone")
        );
    }

    #[test]
    fn specific_flag_overrides_build_and_equal_priority_denies() {
        let mut rg = RegionEngine::new();
        let stranger = Uuid::new_v4();
        let helper = Uuid::new_v4();

        rg.define_region("world", "farm", [0, 60, 0], [10, 70, 10])
            .unwrap();
        rg.update_flags("world", "farm", |f| {
            f.build = Some(false);
            f.block_break = Some(true);
        })
        .unwrap();

        // Specific `block_break = true` overrides `build = false` in the same region,
        // while `block_place` falls back to `build = false`.
        assert!(rg.can_break("world", 5, 65, 5, stranger));
        assert!(!rg.can_place("world", 5, 65, 5, stranger));

        // Overlapping equal-priority region denying `block_break` fails closed for non-members.
        rg.define_region("world", "protected_core", [4, 64, 4], [6, 66, 6])
            .unwrap();
        rg.add_member("world", "protected_core", helper).unwrap();
        rg.update_flags("world", "protected_core", |f| {
            f.block_break = Some(false);
        })
        .unwrap();

        assert!(!rg.can_break("world", 5, 65, 5, stranger));
        assert!(rg.can_break("world", 5, 65, 5, helper));
        assert!(rg.remove_region("world", "protected_core").unwrap());
        assert!(rg.can_break("world", 5, 65, 5, stranger));
    }
}
