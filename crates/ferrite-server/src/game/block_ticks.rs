//! Scheduled block ticks for blocks whose vanilla behavior has no ticker of its own:
//! `ComposterBlock.addLayer` schedules its ready step 20 ticks later, and
//! `FrogspawnBlock.onPlace` schedules the hatch 3600..12000 ticks later.
//!
//! Like the fire and button queues this one is region-local: a split moves the entries whose
//! block belongs to the new region, a merge appends the other region's entries, and a tick owns
//! only the chunks it holds.

use std::collections::HashSet;

type Pos = [i32; 3];

/// The scheduled block behaviors this queue knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum BlockTick {
    /// Vanilla `ComposterBlock.addLayer`: a composter at level 7 becomes ready at level 8.
    ComposterReady,
    /// Vanilla `FrogspawnBlock.tick`: hatch tadpoles and remove the frogspawn.
    FrogspawnHatch,
}

#[derive(Debug, Default)]
pub(super) struct BlockTicks {
    clock: u32,
    due: Vec<(Pos, u32, BlockTick)>,
    scheduled: HashSet<(Pos, BlockTick)>,
}

impl BlockTicks {
    /// `Level.scheduleTick`: a second schedule for the same block and behavior keeps the
    /// original due time, as vanilla's tick queue does.
    pub(super) fn schedule(&mut self, pos: Pos, kind: BlockTick, delay: u32) {
        if self.scheduled.insert((pos, kind)) {
            self.due
                .push((pos, self.clock.wrapping_add(delay.max(1)), kind));
        }
    }

    /// Moves the entries whose block lies in `keep`'s sections into a new region.
    pub(super) fn split(&mut self, keep: impl Fn(i32, i32) -> bool) -> Self {
        let mut result = Self {
            clock: self.clock,
            due: Vec::new(),
            scheduled: HashSet::new(),
        };
        let mut moving = Vec::new();
        self.due.retain(|&(pos, due, kind)| {
            if keep(pos[0], pos[2]) {
                moving.push((pos, due, kind));
                false
            } else {
                true
            }
        });
        for (pos, due, kind) in moving {
            self.scheduled.remove(&(pos, kind));
            result.scheduled.insert((pos, kind));
            result.due.push((pos, due, kind));
        }
        result
    }

    /// Absorbs another region's pending ticks (`RegionData::merge`).
    pub(super) fn merge(&mut self, other: Self) {
        for (pos, due, kind) in other.due {
            if self.scheduled.insert((pos, kind)) {
                self.due.push((pos, due, kind));
            }
        }
    }

    /// Advances the queue by one tick and returns the entries that are due, in schedule order.
    pub(super) fn take_due(&mut self) -> Vec<(Pos, BlockTick)> {
        self.clock = self.clock.wrapping_add(1);
        let mut due = Vec::new();
        let mut index = 0;
        while index < self.due.len() {
            let (pos, at, kind) = self.due[index];
            if at > self.clock {
                index += 1;
                continue;
            }
            self.due.swap_remove(index);
            self.scheduled.remove(&(pos, kind));
            due.push((pos, kind));
        }
        due.sort_by_key(|&(pos, kind)| (self.clock, pos, kind as u8));
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_fire_only_once_their_delay_has_elapsed() {
        let mut ticks = BlockTicks::default();
        ticks.schedule([0, 64, 0], BlockTick::ComposterReady, 20);
        for _ in 0..19 {
            assert!(ticks.take_due().is_empty());
        }
        assert_eq!(
            ticks.take_due(),
            vec![([0, 64, 0], BlockTick::ComposterReady)]
        );
        assert!(ticks.take_due().is_empty(), "a due tick is removed");
    }

    #[test]
    fn a_rescheduled_block_keeps_its_first_due_time() {
        let mut ticks = BlockTicks::default();
        ticks.schedule([1, 64, 1], BlockTick::FrogspawnHatch, 2);
        ticks.schedule([1, 64, 1], BlockTick::FrogspawnHatch, 100);
        assert!(ticks.take_due().is_empty());
        assert_eq!(
            ticks.take_due(),
            vec![([1, 64, 1], BlockTick::FrogspawnHatch)]
        );
    }

    #[test]
    fn split_and_merge_follow_the_block_sections() {
        let mut ticks = BlockTicks::default();
        ticks.schedule([4, 64, 4], BlockTick::ComposterReady, 1);
        ticks.schedule([516, 64, 4], BlockTick::FrogspawnHatch, 1);
        let mut moved = ticks.split(|x, _| x >= 16);
        assert_eq!(
            ticks.take_due(),
            vec![([4, 64, 4], BlockTick::ComposterReady)]
        );
        assert_eq!(
            moved.take_due(),
            vec![([516, 64, 4], BlockTick::FrogspawnHatch)]
        );
        ticks.merge(moved);
        ticks.schedule([516, 64, 4], BlockTick::FrogspawnHatch, 1);
        assert_eq!(
            ticks.take_due(),
            vec![([516, 64, 4], BlockTick::FrogspawnHatch)]
        );
    }
}
