//! Scheduled fluid simulation, based on `FlowingFluid.java#tick`, `#spread`,
//! `#getSpread` and `#getSlopeDistance`.
use crate::{
    Direction, FluidKind, FluidState, FluidWorld, block_for, can_hold_fluid, can_pass_at,
    fluid_state, get_new_liquid_with_rules, tick_delay,
};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

pub type Position = (i32, i32, i32);
const SIDES: [(i32, i32, i32); 4] = [(0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)];
const NEIGHBORS: [(i32, i32, i32); 6] = [
    (0, 0, -1),
    (0, 0, 1),
    (-1, 0, 0),
    (1, 0, 0),
    (0, -1, 0),
    (0, 1, 0),
];
fn offset(p: Position, d: Position) -> Option<Position> {
    Some((
        p.0.checked_add(d.0)?,
        p.1.checked_add(d.1)?,
        p.2.checked_add(d.2)?,
    ))
}
fn direction(from: Position, to: Position) -> Direction {
    if to.0 < from.0 {
        Direction::West
    } else if to.0 > from.0 {
        Direction::East
    } else if to.1 < from.1 {
        Direction::Down
    } else if to.1 > from.1 {
        Direction::Up
    } else if to.2 < from.2 {
        Direction::North
    } else {
        Direction::South
    }
}
fn block(w: &impl FluidWorld, p: Position) -> u32 {
    w.block(p.0, p.1, p.2)
}

/// A tick-ordered queue; duplicate positions retain the first due tick.
#[derive(Default)]
pub struct FluidTicks {
    now: u64,
    seq: u64,
    queue: BinaryHeap<Reverse<(u64, u64, Position)>>,
    pending: HashMap<Position, (u64, u64)>,
}
impl FluidTicks {
    pub fn schedule(&mut self, pos: Position, delay: u64) {
        let due = self.now.saturating_add(delay);
        if self.pending.contains_key(&pos) {
            return;
        }
        let seq = self.seq;
        self.seq += 1;
        self.pending.insert(pos, (due, seq));
        self.queue.push(Reverse((due, seq, pos)));
    }
    /// Transfer selected positions while preserving remaining delay and order.
    /// Existing destination entries retain their original due time.
    pub fn transfer_to(&mut self, destination: &mut Self, select: impl Fn(Position) -> bool) {
        let mut entries: Vec<_> = self
            .pending
            .iter()
            .filter(|(pos, _)| select(**pos))
            .map(|(&pos, &(due, seq))| (due, seq, pos))
            .collect();
        entries.sort_unstable();
        for (due, _, pos) in entries {
            self.pending.remove(&pos);
            destination.schedule(pos, due.saturating_sub(self.now));
        }
    }
    /// Advance this region's clock by one game tick.
    pub fn advance(&mut self, world: &mut impl FluidWorld) {
        self.run_until(world, self.now.saturating_add(1));
    }
    /// Processes due work in due-time then insertion order; later work remains queued.
    pub fn run_until(&mut self, world: &mut impl FluidWorld, tick: u64) {
        assert!(tick >= self.now, "cannot rewind fluid ticks");
        while let Some(&Reverse((due, seq, pos))) = self.queue.peek() {
            if due > tick {
                break;
            }
            self.queue.pop();
            if self.pending.get(&pos) != Some(&(due, seq)) {
                continue;
            }
            self.pending.remove(&pos);
            self.now = due;
            self.tick(world, pos);
        }
        self.now = tick;
    }
    /// Notify the scheduler after a block placement, removal or neighbor change.
    pub fn on_block_change(&mut self, world: &mut impl FluidWorld, pos: Position) {
        let kind = fluid_state(block(world, pos)).kind;
        self.changed(world, pos, kind);
    }
    fn changed(&mut self, world: &mut impl FluidWorld, pos: Position, kind: FluidKind) {
        let delay = u64::from(tick_delay(kind, world.ultrawarm()));
        if delay > 0 {
            self.schedule(pos, delay);
        }
        for d in NEIGHBORS {
            if let Some(n) = offset(pos, d) {
                let f = fluid_state(block(world, n));
                if f.kind != FluidKind::Empty {
                    self.schedule(n, u64::from(tick_delay(f.kind, world.ultrawarm())));
                }
            }
        }
        for n in std::iter::once(Some(pos))
            .chain(NEIGHBORS.into_iter().map(|d| offset(pos, d)))
            .flatten()
        {
            let f = fluid_state(block(world, n));
            if f.kind == FluidKind::Lava {
                self.lava_reaction(world, n, f);
            }
        }
    }
    fn put(&mut self, world: &mut impl FluidWorld, pos: Position, state: FluidState) {
        let old = block(world, pos);
        // `SimpleWaterloggedBlock.placeLiquid` accepts source water only.
        let new = if state.kind == FluidKind::Water && fluid_state(old).kind == FluidKind::Empty {
            if let Some(wet) = crate::waterlogged_variant(old, true) {
                if !state.source {
                    return;
                }
                wet
            } else {
                block_for(state)
            }
        } else {
            block_for(state)
        };
        if old != new {
            world.set_block(pos.0, pos.1, pos.2, new);
            self.changed(world, pos, state.kind);
        }
    }
    fn tick(&mut self, world: &mut impl FluidWorld, pos: Position) {
        let mut current = fluid_state(block(world, pos));
        if current.kind == FluidKind::Empty {
            return;
        }
        // `LiquidBlock.java#shouldSpreadLiquid` reacts on the lava block.
        if current.kind == FluidKind::Lava && self.lava_reaction(world, pos, current) {
            return;
        }
        if !current.source {
            let next = get_new_liquid_with_rules(
                world,
                pos.0,
                pos.1,
                pos.2,
                current.kind,
                world.water_conversion(),
                world.lava_conversion(),
            );
            if next != current {
                self.put(world, pos, next);
                current = next;
            }
        }
        if current.kind == FluidKind::Empty {
            return;
        }
        let Some(down) = offset(pos, (0, -1, 0)) else {
            return;
        };
        // `LavaFluid.java#spreadTo` creates stone when flowing downward into
        // a standalone water block (waterlogged containers are not replaced).
        if current.kind == FluidKind::Lava
            && fluid_state(block(world, down)).kind == FluidKind::Water
            && can_pass_at(world, pos, down, Direction::Down)
        {
            let target = block(world, down);
            let standalone = u16::try_from(target)
                .ok()
                .and_then(ferrite_data::blocks::state)
                .is_some_and(|(name, _)| name == "minecraft:water" || name == "water");
            if standalone {
                world.set_block(
                    down.0,
                    down.1,
                    down.2,
                    u32::from(ferrite_data::blocks::default_state("stone").unwrap()),
                );
                self.changed(world, down, FluidKind::Empty);
            }
            return;
        }
        let mut spread_down = false;
        if self.can_enter(world, pos, down, current.kind) {
            let next = get_new_liquid_with_rules(
                world,
                down.0,
                down.1,
                down.2,
                current.kind,
                world.water_conversion(),
                world.lava_conversion(),
            );
            if next.kind != FluidKind::Empty {
                self.put(world, down, next);
                spread_down = true;
            }
        }
        if spread_down {
            let sources = SIDES
                .iter()
                .filter_map(|&d| offset(pos, d))
                .filter(|&n| {
                    let f = fluid_state(block(world, n));
                    f.kind == current.kind && f.source
                })
                .count();
            if sources < 3 {
                return;
            }
        } else if !current.source && self.hole(world, pos, current.kind) {
            return;
        }
        let drop = if current.kind == FluidKind::Lava && !world.ultrawarm() {
            2
        } else {
            1
        };
        if !current.falling && current.amount <= drop {
            return;
        }
        let limit = if current.kind == FluidKind::Water || world.ultrawarm() {
            4
        } else {
            2
        };
        let mut choices = Vec::new();
        let mut best = u32::MAX;
        for d in SIDES {
            let Some(n) = offset(pos, d) else { continue };
            if !self.can_explore(world, pos, n, current.kind) {
                continue;
            }
            let score = if self.hole(world, n, current.kind) {
                0
            } else {
                self.slope(world, n, pos, current.kind, 1, limit)
            };
            if score < best {
                choices.clear();
                best = score;
            }
            if score == best && self.can_enter(world, pos, n, current.kind) {
                choices.push(n);
            }
        }
        for n in choices {
            let next = get_new_liquid_with_rules(
                world,
                n.0,
                n.1,
                n.2,
                current.kind,
                world.water_conversion(),
                world.lava_conversion(),
            );
            if next.kind != FluidKind::Empty {
                self.put(world, n, next);
            }
        }
    }
    fn lava_reaction(
        &mut self,
        w: &mut impl FluidWorld,
        pos: Position,
        current: FluidState,
    ) -> bool {
        let soul = offset(pos, (0, -1, 0)).is_some_and(|p| {
            block(w, p) == u32::from(ferrite_data::blocks::default_state("soul_soil").unwrap())
        });
        for d in [(0, 1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)] {
            let Some(n) = offset(pos, d) else { continue };
            let neighbor = block(w, n);
            let result = if fluid_state(neighbor).kind == FluidKind::Water {
                Some(if current.source {
                    "obsidian"
                } else {
                    "cobblestone"
                })
            } else if soul
                && neighbor == u32::from(ferrite_data::blocks::default_state("blue_ice").unwrap())
            {
                Some("basalt")
            } else {
                None
            };
            if let Some(result) = result {
                w.set_block(
                    pos.0,
                    pos.1,
                    pos.2,
                    u32::from(ferrite_data::blocks::default_state(result).unwrap()),
                );
                self.changed(w, pos, FluidKind::Empty);
                return true;
            }
        }
        false
    }
    fn can_enter(
        &self,
        w: &impl FluidWorld,
        from: Position,
        to: Position,
        kind: FluidKind,
    ) -> bool {
        let target = block(w, to);
        fluid_state(target).kind == FluidKind::Empty
            && can_hold_fluid(target, kind)
            && can_pass_at(w, from, to, direction(from, to))
    }
    fn can_explore(
        &self,
        w: &impl FluidWorld,
        from: Position,
        to: Position,
        kind: FluidKind,
    ) -> bool {
        let target = block(w, to);
        let fluid = fluid_state(target);
        (fluid.kind == FluidKind::Empty || (fluid.kind == kind && !fluid.source))
            && can_hold_fluid(target, kind)
            && can_pass_at(w, from, to, direction(from, to))
    }
    fn hole(&self, w: &impl FluidWorld, pos: Position, kind: FluidKind) -> bool {
        let Some(below) = offset(pos, (0, -1, 0)) else {
            return false;
        };
        let target = block(w, below);
        can_pass_at(w, pos, below, Direction::Down)
            && (fluid_state(target).kind == kind || can_hold_fluid(target, kind))
    }
    fn slope(
        &self,
        w: &impl FluidWorld,
        pos: Position,
        prev: Position,
        kind: FluidKind,
        depth: u32,
        limit: u32,
    ) -> u32 {
        let mut best = u32::MAX;
        for d in SIDES {
            let Some(n) = offset(pos, d) else { continue };
            if n == prev || !self.can_explore(w, pos, n, kind) {
                continue;
            }
            if self.hole(w, n, kind) {
                return depth;
            }
            if depth < limit {
                best = best.min(self.slope(w, n, pos, kind, depth + 1, limit));
            }
        }
        best
    }
}
