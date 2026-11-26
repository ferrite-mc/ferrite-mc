//! FAWE/WorldEdit-style cuboid selection, pure batch edit planners, clipboard, and undo/redo.
//!
//! All batch planners (`plan_set`, `plan_replace`, `plan_walls`, `copy_clipboard`, `plan_paste`)
//! are pure over a generic `Fn(i32, i32, i32) -> Option<u32>` block-reader closure so region
//! threads or tests can compute exact block-write batches without coupling directly to chunk
//! storage.

use std::collections::{HashMap, VecDeque};

use uuid::Uuid;

use crate::error::{PluginError, Result};

/// Default safety limit on blocks touched by a single operation.
pub const DEFAULT_MAX_BLOCKS: usize = 1_000_000;

/// Default maximum undo/redo history entries retained per player.
pub const DEFAULT_MAX_HISTORY: usize = 64;

/// A single block write (`([x, y, z], block_state)`).
pub type BlockEdit = ([i32; 3], u32);

/// Per-player cuboid selection endpoints.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CuboidSelection {
    /// First selected block corner.
    pub pos1: Option<[i32; 3]>,
    /// Second selected block corner.
    pub pos2: Option<[i32; 3]>,
}

impl CuboidSelection {
    /// Returns normalized `(min, max)` bounds if both `pos1` and `pos2` are set.
    pub fn bounds(&self) -> Option<([i32; 3], [i32; 3])> {
        let p1 = self.pos1?;
        let p2 = self.pos2?;
        let min = [p1[0].min(p2[0]), p1[1].min(p2[1]), p1[2].min(p2[2])];
        let max = [p1[0].max(p2[0]), p1[1].max(p2[1]), p1[2].max(p2[2])];
        Some((min, max))
    }

    /// Returns the total block volume of the selection if both corners are set (overflow-safe).
    pub fn volume(&self) -> Option<u64> {
        let (min, max) = self.bounds()?;
        let dx = (i128::from(max[0]) - i128::from(min[0]) + 1) as u128;
        let dy = (i128::from(max[1]) - i128::from(min[1]) + 1) as u128;
        let dz = (i128::from(max[2]) - i128::from(min[2]) + 1) as u128;
        let total = dx.saturating_mul(dy).saturating_mul(dz);
        Some(u64::try_from(total).unwrap_or(u64::MAX))
    }
}

/// Copied block schematic stored as relative `[dx, dy, dz]` offsets from its copy origin.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Clipboard {
    /// Relative offset `(dx, dy, dz)` and captured block state ID.
    pub blocks: Vec<([i32; 3], u32)>,
}

/// Paired previous/new block snapshots for undo and redo.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HistoryEntry {
    /// Previous block states (`([x, y, z], old_state)`) to restore on undo.
    previous: Vec<BlockEdit>,
    /// Applied block states (`([x, y, z], new_state)`) to re-apply on redo.
    applied: Vec<BlockEdit>,
}

#[derive(Debug, Clone, Default)]
struct PlayerEditState {
    selection: CuboidSelection,
    clipboard: Option<Clipboard>,
    undo_stack: VecDeque<HistoryEntry>,
    redo_stack: VecDeque<HistoryEntry>,
}

/// FAWE/WorldEdit-style cuboid selection, clipboard, batch edit planner, and undo/redo engine.
#[derive(Debug, Clone)]
pub struct WorldEditEngine {
    max_blocks: usize,
    max_history: usize,
    players: HashMap<Uuid, PlayerEditState>,
}

impl Default for WorldEditEngine {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_BLOCKS)
    }
}

impl WorldEditEngine {
    /// Creates an engine with the given `max_blocks` safety cap per operation.
    pub fn new(max_blocks: usize) -> Self {
        Self {
            max_blocks,
            max_history: DEFAULT_MAX_HISTORY,
            players: HashMap::new(),
        }
    }

    /// Returns the configured maximum block limit per operation.
    pub fn max_blocks(&self) -> usize {
        self.max_blocks
    }

    /// Updates the maximum block limit per operation.
    pub fn set_max_blocks(&mut self, max_blocks: usize) {
        self.max_blocks = max_blocks;
    }

    /// Updates the maximum undo/redo history depth retained per player.
    pub fn set_max_history(&mut self, max_history: usize) {
        self.max_history = max_history;
        for state in self.players.values_mut() {
            while state.undo_stack.len() > max_history {
                state.undo_stack.pop_front();
            }
            while state.redo_stack.len() > max_history {
                state.redo_stack.pop_front();
            }
        }
    }

    /// Sets corner 1 (`//pos1`) for `player`.
    pub fn set_pos1(&mut self, player: Uuid, pos: [i32; 3]) {
        self.players.entry(player).or_default().selection.pos1 = Some(pos);
    }

    /// Sets corner 2 (`//pos2`) for `player`.
    pub fn set_pos2(&mut self, player: Uuid, pos: [i32; 3]) {
        self.players.entry(player).or_default().selection.pos2 = Some(pos);
    }

    /// Clears `player`'s selection.
    pub fn clear_selection(&mut self, player: Uuid) {
        if let Some(state) = self.players.get_mut(&player) {
            state.selection = CuboidSelection::default();
        }
    }

    /// Returns `player`'s selection if present.
    pub fn selection(&self, player: Uuid) -> Option<CuboidSelection> {
        self.players.get(&player).map(|s| s.selection)
    }

    /// Returns normalized `(min, max)` bounds for `player`'s selection if both corners are set.
    pub fn bounds(&self, player: Uuid) -> Option<([i32; 3], [i32; 3])> {
        self.players.get(&player)?.selection.bounds()
    }

    /// Returns the block volume of `player`'s selection if both corners are set.
    pub fn volume(&self, player: Uuid) -> Option<u64> {
        self.players.get(&player)?.selection.volume()
    }

    fn checked_selection_bounds(&self, player: Uuid) -> Result<([i32; 3], [i32; 3])> {
        let sel = self
            .players
            .get(&player)
            .map(|s| s.selection)
            .unwrap_or_default();
        let bounds = sel.bounds().ok_or(PluginError::IncompleteSelection)?;
        let vol = sel.volume().ok_or(PluginError::IncompleteSelection)?;
        if vol > self.max_blocks as u64 {
            return Err(PluginError::SelectionTooLarge {
                blocks: vol,
                max_blocks: self.max_blocks,
            });
        }
        Ok(bounds)
    }

    fn record_history(&mut self, player: Uuid, previous: Vec<BlockEdit>, applied: Vec<BlockEdit>) {
        if applied.is_empty() || self.max_history == 0 {
            return;
        }
        let state = self.players.entry(player).or_default();
        state.redo_stack.clear();
        if state.undo_stack.len() >= self.max_history {
            state.undo_stack.pop_front();
        }
        state
            .undo_stack
            .push_back(HistoryEntry { previous, applied });
    }

    /// Plans a `//set <block_state>` over `player`'s cuboid selection.
    ///
    /// Skips blocks where `read_block` returns `None` (unloaded/out-of-bounds) or where the
    /// existing state already equals `block_state`, and pushes an undo snapshot when non-empty.
    pub fn plan_set<F>(
        &mut self,
        player: Uuid,
        block_state: u32,
        read_block: F,
    ) -> Result<Vec<BlockEdit>>
    where
        F: Fn(i32, i32, i32) -> Option<u32>,
    {
        let (min, max) = self.checked_selection_bounds(player)?;
        let mut previous = Vec::new();
        let mut applied = Vec::new();

        for y in min[1]..=max[1] {
            for z in min[2]..=max[2] {
                for x in min[0]..=max[0] {
                    if let Some(old) = read_block(x, y, z)
                        && old != block_state
                    {
                        previous.push(([x, y, z], old));
                        applied.push(([x, y, z], block_state));
                    }
                }
            }
        }

        self.record_history(player, previous, applied.clone());
        Ok(applied)
    }

    /// Plans a `//replace <from_state> <to_state>` over `player`'s cuboid selection.
    pub fn plan_replace<F>(
        &mut self,
        player: Uuid,
        from_state: u32,
        to_state: u32,
        read_block: F,
    ) -> Result<Vec<BlockEdit>>
    where
        F: Fn(i32, i32, i32) -> Option<u32>,
    {
        let (min, max) = self.checked_selection_bounds(player)?;
        let mut previous = Vec::new();
        let mut applied = Vec::new();

        if from_state == to_state {
            return Ok(applied);
        }

        for y in min[1]..=max[1] {
            for z in min[2]..=max[2] {
                for x in min[0]..=max[0] {
                    if let Some(old) = read_block(x, y, z)
                        && old == from_state
                    {
                        previous.push(([x, y, z], old));
                        applied.push(([x, y, z], to_state));
                    }
                }
            }
        }

        self.record_history(player, previous, applied.clone());
        Ok(applied)
    }

    /// Plans a `//walls <block_state>` along the 4 vertical perimeter faces of `player`'s selection.
    pub fn plan_walls<F>(
        &mut self,
        player: Uuid,
        block_state: u32,
        read_block: F,
    ) -> Result<Vec<BlockEdit>>
    where
        F: Fn(i32, i32, i32) -> Option<u32>,
    {
        let (min, max) = self.checked_selection_bounds(player)?;
        let mut previous = Vec::new();
        let mut applied = Vec::new();

        for y in min[1]..=max[1] {
            for z in min[2]..=max[2] {
                for x in min[0]..=max[0] {
                    let on_wall = x == min[0] || x == max[0] || z == min[2] || z == max[2];
                    if on_wall
                        && let Some(old) = read_block(x, y, z)
                        && old != block_state
                    {
                        previous.push(([x, y, z], old));
                        applied.push(([x, y, z], block_state));
                    }
                }
            }
        }

        self.record_history(player, previous, applied.clone());
        Ok(applied)
    }

    /// Copies `player`'s selection into their clipboard relative to `bounds.min`, returning the
    /// number of captured blocks.
    pub fn copy_clipboard<F>(&mut self, player: Uuid, read_block: F) -> Result<usize>
    where
        F: Fn(i32, i32, i32) -> Option<u32>,
    {
        let (min, _) = self.checked_selection_bounds(player)?;
        self.copy_clipboard_from(player, min, read_block)
    }

    /// Copies `player`'s selection into their clipboard relative to an explicit `origin`.
    pub fn copy_clipboard_from<F>(
        &mut self,
        player: Uuid,
        origin: [i32; 3],
        read_block: F,
    ) -> Result<usize>
    where
        F: Fn(i32, i32, i32) -> Option<u32>,
    {
        let (min, max) = self.checked_selection_bounds(player)?;
        let mut blocks = Vec::new();

        for y in min[1]..=max[1] {
            for z in min[2]..=max[2] {
                for x in min[0]..=max[0] {
                    if let Some(state) = read_block(x, y, z) {
                        blocks.push(([x - origin[0], y - origin[1], z - origin[2]], state));
                    }
                }
            }
        }

        let count = blocks.len();
        self.players.entry(player).or_default().clipboard = Some(Clipboard { blocks });
        Ok(count)
    }

    /// Returns a reference to `player`'s clipboard, if populated.
    pub fn clipboard(&self, player: Uuid) -> Option<&Clipboard> {
        self.players.get(&player)?.clipboard.as_ref()
    }

    /// Plans a `//paste` of `player`'s clipboard anchored at `origin`.
    pub fn plan_paste<F>(
        &mut self,
        player: Uuid,
        origin: [i32; 3],
        read_block: F,
    ) -> Result<Vec<BlockEdit>>
    where
        F: Fn(i32, i32, i32) -> Option<u32>,
    {
        let blocks = self
            .players
            .get(&player)
            .and_then(|s| s.clipboard.as_ref())
            .filter(|c| !c.blocks.is_empty())
            .map(|c| c.blocks.clone())
            .ok_or(PluginError::EmptyClipboard)?;

        if blocks.len() > self.max_blocks {
            return Err(PluginError::SelectionTooLarge {
                blocks: blocks.len() as u64,
                max_blocks: self.max_blocks,
            });
        }

        let mut previous = Vec::new();
        let mut applied = Vec::new();

        for (offset, new_state) in blocks {
            let Some(wx) = origin[0].checked_add(offset[0]) else {
                continue;
            };
            let Some(wy) = origin[1].checked_add(offset[1]) else {
                continue;
            };
            let Some(wz) = origin[2].checked_add(offset[2]) else {
                continue;
            };

            if let Some(old) = read_block(wx, wy, wz)
                && old != new_state
            {
                previous.push(([wx, wy, wz], old));
                applied.push(([wx, wy, wz], new_state));
            }
        }

        self.record_history(player, previous, applied.clone());
        Ok(applied)
    }

    /// Pops the latest edit from `player`'s undo stack, pushes it onto the redo stack, and
    /// returns the previous `Vec<([i32; 3], u32)>` block snapshot to restore.
    pub fn undo(&mut self, player: Uuid) -> Result<Vec<BlockEdit>> {
        let state = self
            .players
            .get_mut(&player)
            .ok_or(PluginError::NothingToUndo)?;
        let entry = state
            .undo_stack
            .pop_back()
            .ok_or(PluginError::NothingToUndo)?;
        let restore = entry.previous.clone();
        state.redo_stack.push_back(entry);
        Ok(restore)
    }

    /// Pops the latest undone edit from `player`'s redo stack, pushes it back onto the undo stack,
    /// and returns the block writes to re-apply.
    pub fn redo(&mut self, player: Uuid) -> Result<Vec<BlockEdit>> {
        let state = self
            .players
            .get_mut(&player)
            .ok_or(PluginError::NothingToRedo)?;
        let entry = state
            .redo_stack
            .pop_back()
            .ok_or(PluginError::NothingToRedo)?;
        let reapply = entry.applied.clone();
        state.undo_stack.push_back(entry);
        Ok(reapply)
    }

    /// Returns the number of undo entries available for `player`.
    pub fn undo_depth(&self, player: Uuid) -> usize {
        self.players.get(&player).map_or(0, |s| s.undo_stack.len())
    }

    /// Returns the number of redo entries available for `player`.
    pub fn redo_depth(&self, player: Uuid) -> usize {
        self.players.get(&player).map_or(0, |s| s.redo_stack.len())
    }

    /// Peeks at the previous block snapshot on top of `player`'s undo stack without popping it.
    pub fn peek_undo_snapshot(&self, player: Uuid) -> Option<&[BlockEdit]> {
        self.players
            .get(&player)?
            .undo_stack
            .back()
            .map(|e| e.previous.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply_edits(world: &mut HashMap<[i32; 3], u32>, edits: &[BlockEdit]) {
        for &(pos, state) in edits {
            world.insert(pos, state);
        }
    }

    #[test]
    fn selection_bounds_volume_and_safety_cap() {
        let mut we = WorldEditEngine::new(100);
        let p = Uuid::new_v4();

        assert!(matches!(
            we.plan_set(p, 1, |_, _, _| Some(0)),
            Err(PluginError::IncompleteSelection)
        ));

        we.set_pos1(p, [10, 64, 10]);
        we.set_pos2(p, [8, 66, 9]);
        assert_eq!(we.bounds(p), Some(([8, 64, 9], [10, 66, 10])));
        assert_eq!(we.volume(p), Some(3 * 3 * 2));

        // Exceeding max_blocks is rejected cleanly without overflow.
        we.set_pos1(p, [i32::MIN, 0, 0]);
        we.set_pos2(p, [i32::MAX, 10, 10]);
        assert!(matches!(
            we.plan_set(p, 1, |_, _, _| Some(0)),
            Err(PluginError::SelectionTooLarge { .. })
        ));
    }

    #[test]
    fn set_replace_walls_and_undo_redo() {
        let mut we = WorldEditEngine::new(1_000);
        let p = Uuid::new_v4();
        let mut world: HashMap<[i32; 3], u32> = HashMap::new();
        for y in 0..3 {
            for z in 0..3 {
                for x in 0..3 {
                    world.insert([x, y, z], if x == 1 && y == 1 && z == 1 { 2 } else { 0 });
                }
            }
        }

        we.set_pos1(p, [0, 0, 0]);
        we.set_pos2(p, [2, 2, 2]);

        // Walls of a 3x3x3 cube = 27 - 3 center column blocks = 24 blocks.
        let wall_edits = we
            .plan_walls(p, 5, |x, y, z| world.get(&[x, y, z]).copied())
            .unwrap();
        assert_eq!(wall_edits.len(), 24);
        apply_edits(&mut world, &wall_edits);
        assert_eq!(world[&[0, 1, 0]], 5);
        assert_eq!(world[&[1, 1, 1]], 2);

        // Replace state 2 with 9.
        let replace_edits = we
            .plan_replace(p, 2, 9, |x, y, z| world.get(&[x, y, z]).copied())
            .unwrap();
        assert_eq!(replace_edits, vec![([1, 1, 1], 9)]);
        apply_edits(&mut world, &replace_edits);
        assert_eq!(world[&[1, 1, 1]], 9);

        // Undo replace -> restores 2.
        let undo1 = we.undo(p).unwrap();
        apply_edits(&mut world, &undo1);
        assert_eq!(world[&[1, 1, 1]], 2);

        // Redo replace -> applies 9.
        let redo1 = we.redo(p).unwrap();
        apply_edits(&mut world, &redo1);
        assert_eq!(world[&[1, 1, 1]], 9);

        // New edit clears redo stack.
        let set_edits = we
            .plan_set(p, 7, |x, y, z| world.get(&[x, y, z]).copied())
            .unwrap();
        assert_eq!(set_edits.len(), 27);
        assert_eq!(we.redo_depth(p), 0);
        assert!(matches!(we.redo(p), Err(PluginError::NothingToRedo)));
    }

    #[test]
    fn copy_and_paste_relative_to_origin() {
        let mut we = WorldEditEngine::new(1_000);
        let p = Uuid::new_v4();
        let mut world: HashMap<[i32; 3], u32> = HashMap::new();
        for x in 0..=1 {
            for z in 0..=1 {
                world.insert([x, 10, z], (x * 10 + z + 1) as u32);
                world.insert([x + 20, 10, z + 20], 0);
            }
        }

        we.set_pos1(p, [0, 10, 0]);
        we.set_pos2(p, [1, 10, 1]);
        let copied = we
            .copy_clipboard(p, |x, y, z| world.get(&[x, y, z]).copied())
            .unwrap();
        assert_eq!(copied, 4);

        let paste_edits = we
            .plan_paste(p, [20, 10, 20], |x, y, z| world.get(&[x, y, z]).copied())
            .unwrap();
        assert_eq!(paste_edits.len(), 4);
        apply_edits(&mut world, &paste_edits);
        assert_eq!(world[&[20, 10, 20]], 1);
        assert_eq!(world[&[21, 10, 21]], 12);

        let undo = we.undo(p).unwrap();
        apply_edits(&mut world, &undo);
        assert_eq!(world[&[20, 10, 20]], 0);
        assert_eq!(world[&[21, 10, 21]], 0);
    }

    #[test]
    fn unloaded_blocks_skipped_custom_origin_and_history_cap() {
        let mut we = WorldEditEngine::new(100);
        we.set_max_history(2);
        let p = Uuid::new_v4();

        // Empty clipboard returns EmptyClipboard.
        assert!(matches!(
            we.plan_paste(p, [0, 0, 0], |_, _, _| Some(0)),
            Err(PluginError::EmptyClipboard)
        ));

        we.set_pos1(p, [0, 0, 0]);
        we.set_pos2(p, [1, 0, 0]);
        // Only x == 0 is loaded; x == 1 returns None and is skipped cleanly.
        let edits = we
            .plan_set(p, 4, |x, _, _| if x == 0 { Some(1) } else { None })
            .unwrap();
        assert_eq!(edits, vec![([0, 0, 0], 4)]);
        assert_eq!(we.peek_undo_snapshot(p), Some(&[([0, 0, 0], 1)][..]));

        // Copy relative to custom origin [1, 0, 0].
        let copied = we
            .copy_clipboard_from(p, [1, 0, 0], |x, _, _| if x == 0 { Some(9) } else { None })
            .unwrap();
        assert_eq!(copied, 1);
        let pasted = we.plan_paste(p, [10, 5, 5], |_, _, _| Some(0)).unwrap();
        assert_eq!(pasted, vec![([9, 5, 5], 9)]);

        // Push a third edit; max_history=2 evicts oldest.
        we.plan_set(p, 5, |x, _, _| if x == 0 { Some(4) } else { None })
            .unwrap();
        assert_eq!(we.undo_depth(p), 2);
        we.undo(p).unwrap();
        we.undo(p).unwrap();
        assert!(matches!(we.undo(p), Err(PluginError::NothingToUndo)));
    }
}
