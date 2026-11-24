//! Per chunk-section entity lists, the spatial index behind range and collision queries.
//!
//! Like vanilla's `EntitySectionStorage` (and Lithium's section-list lookups), every entity is
//! listed in the 16x16x16 section that contains its position, and a box query visits only the
//! sections the box can reach. Entities are indexed by position, not by box, so a query widens
//! its section range by [`QUERY_MARGIN`] blocks to find entities whose box pokes into it from
//! a neighbouring section.

use crate::id::EntityKey;
use ferrite_math::{AABB, Vec3};
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

#[derive(Default)]
struct FxHasher(u64);

impl Hasher for FxHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut h = self.0;
        for &b in bytes {
            h = (h.rotate_left(5) ^ u64::from(b)).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
        self.0 = h;
    }
    #[inline]
    fn write_i64(&mut self, i: i64) {
        self.0 = (self.0.rotate_left(5) ^ (i as u64)).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

type FastMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;

/// Blocks a query is widened by, so entities up to this far outside their own section are
/// still found; vanilla's `EntitySectionStorage.getEntities` uses the same 2 blocks.
pub const QUERY_MARGIN: f64 = 2.0;

/// A chunk section position, packed like vanilla's `SectionPos.asLong`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Cell(i64);

impl Cell {
    pub fn new(x: i32, y: i32, z: i32) -> Self {
        Self(
            ((i64::from(x) & 0x3F_FFFF) << 42)
                | (i64::from(y) & 0xF_FFFF)
                | ((i64::from(z) & 0x3F_FFFF) << 20),
        )
    }

    /// The section containing a position.
    pub fn containing(pos: Vec3) -> Self {
        Self::new(
            (pos.x.floor() as i32) >> 4,
            (pos.y.floor() as i32) >> 4,
            (pos.z.floor() as i32) >> 4,
        )
    }

    pub fn x(self) -> i32 {
        (self.0 >> 42) as i32
    }

    pub fn y(self) -> i32 {
        ((self.0 << 44) >> 44) as i32
    }

    pub fn z(self) -> i32 {
        ((self.0 << 22) >> 42) as i32
    }
}

/// Section lists of the entities in one store.
#[derive(Debug, Default)]
pub struct SectionIndex {
    cells: FastMap<Cell, Vec<EntityKey>>,
    free: Vec<Vec<EntityKey>>,
}

impl SectionIndex {
    pub fn insert(&mut self, cell: Cell, key: EntityKey) {
        match self.cells.entry(cell) {
            std::collections::hash_map::Entry::Occupied(mut e) => e.get_mut().push(key),
            std::collections::hash_map::Entry::Vacant(e) => {
                let mut list = self.free.pop().unwrap_or_else(|| Vec::with_capacity(4));
                list.push(key);
                e.insert(list);
            }
        }
    }

    pub fn remove(&mut self, cell: Cell, key: EntityKey) {
        if let Some(list) = self.cells.get_mut(&cell) {
            if let Some(i) = list.iter().position(|k| *k == key) {
                list.swap_remove(i);
            }
            if list.is_empty()
                && let Some(empty) = self.cells.remove(&cell)
                && self.free.len() < 64
            {
                self.free.push(empty);
            }
        }
    }

    /// Entities listed in `cell`.
    pub fn get(&self, cell: Cell) -> &[EntityKey] {
        self.cells.get(&cell).map_or(&[], Vec::as_slice)
    }

    /// Number of non-empty sections.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Number of listed entities over all sections.
    pub fn total(&self) -> usize {
        self.cells.values().map(Vec::len).sum()
    }

    /// Calls `f` for every entity listed in a section that `area`, widened by
    /// [`QUERY_MARGIN`], touches. Callers filter by the entity's own box.
    pub fn for_each_near(&self, area: AABB, mut f: impl FnMut(EntityKey)) {
        let min = |v: f64| ((v - QUERY_MARGIN).floor() as i32) >> 4;
        let max = |v: f64| ((v + QUERY_MARGIN).floor() as i32) >> 4;
        let (x0, x1) = (min(area.min.x), max(area.max.x));
        let (y0, y1) = (min(area.min.y), max(area.max.y));
        let (z0, z1) = (min(area.min.z), max(area.max.z));
        let span = |a: i32, b: i32| i64::from(b) - i64::from(a) + 1;
        if span(x0, x1) * span(y0, y1) * span(z0, z1) > self.cells.len() as i64 {
            // A huge box: walking the occupied sections is cheaper than walking the box.
            for (cell, keys) in &self.cells {
                if (x0..=x1).contains(&cell.x())
                    && (y0..=y1).contains(&cell.y())
                    && (z0..=z1).contains(&cell.z())
                {
                    keys.iter().copied().for_each(&mut f);
                }
            }
            return;
        }
        for x in x0..=x1 {
            for z in z0..=z1 {
                for y in y0..=y1 {
                    self.get(Cell::new(x, y, z))
                        .iter()
                        .copied()
                        .for_each(&mut f);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_packs_negative_coordinates() {
        for (x, y, z) in [
            (0, 0, 0),
            (-1, -4, -1),
            (1_875_000, 19, -1_875_000),
            (5, -1, 9),
        ] {
            let cell = Cell::new(x, y, z);
            assert_eq!((cell.x(), cell.y(), cell.z()), (x, y, z));
        }
        assert_eq!(
            Cell::containing(Vec3::new(-0.5, 64.0, 31.9)),
            Cell::new(-1, 4, 1)
        );
    }
}
