use ferrite_data::blocks;
use ferrite_fluid::{FluidKind, FluidTicks, FluidWorld, fluid_state};
use std::collections::{HashMap, HashSet};

struct World {
    cells: HashMap<(i32, i32, i32), u32>,
    holes: HashSet<(i32, i32)>,
    ledge: bool,
}
fn id(name: &str) -> u32 {
    u32::from(blocks::default_state(name).unwrap())
}
impl FluidWorld for World {
    fn block(&self, x: i32, y: i32, z: i32) -> u32 {
        *self.cells.get(&(x, y, z)).unwrap_or_else(|| {
            if (y == -1 && !self.holes.contains(&(x, z)) && (!self.ledge || x <= 0)) || y <= -5 {
                &self.cells[&(999, 999, 999)]
            } else {
                &self.cells[&(998, 998, 998)]
            }
        })
    }
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32) {
        self.cells.insert((x, y, z), state);
    }
    fn ultrawarm(&self) -> bool {
        false
    }
}
fn state(name: &str, props: &[(&str, &str)]) -> u32 {
    u32::from(blocks::state_id(name, props).unwrap())
}
fn world() -> World {
    World {
        cells: HashMap::from([((999, 999, 999), id("stone")), ((998, 998, 998), id("air"))]),
        holes: HashSet::new(),
        ledge: false,
    }
}
#[test]
fn water_source_spreads_seven_block_diamond_on_flat_floor() {
    let mut w = world();
    w.set_block(0, 0, 0, id("water"));
    let mut ticks = FluidTicks::default();
    ticks.schedule((0, 0, 0), 0);
    ticks.run_until(&mut w, 300);
    for x in -9i32..=9 {
        for z in -9i32..=9 {
            assert_eq!(
                fluid_state(w.block(x, 0, z)).kind == FluidKind::Water,
                x.abs() + z.abs() <= 7,
                "{x},{z}"
            );
        }
    }
}
#[test]
fn slope_search_prefers_a_hole() {
    let mut w = world();
    w.holes.insert((3, 0));
    w.set_block(0, 0, 0, id("water"));
    let mut ticks = FluidTicks::default();
    ticks.schedule((0, 0, 0), 0);
    ticks.run_until(&mut w, 5);
    assert_eq!(fluid_state(w.block(1, 0, 0)).kind, FluidKind::Water);
    assert_eq!(fluid_state(w.block(-1, 0, 0)).kind, FluidKind::Empty);
}
#[test]
fn water_over_ledge_creates_falling_column() {
    let mut w = world();
    w.ledge = true;
    w.set_block(0, 0, 0, id("water"));
    let mut ticks = FluidTicks::default();
    ticks.schedule((0, 0, 0), 0);
    ticks.run_until(&mut w, 100);
    for y in -4..0 {
        assert!(fluid_state(w.block(1, y, 0)).falling, "y={y}");
    }
}
#[test]
fn lava_on_flat_floor_spreads_three_blocks() {
    let mut w = world();
    w.set_block(0, 0, 0, id("lava"));
    let mut ticks = FluidTicks::default();
    ticks.schedule((0, 0, 0), 0);
    ticks.run_until(&mut w, 300);
    for x in -5i32..=5 {
        for z in -5i32..=5 {
            assert_eq!(
                fluid_state(w.block(x, 0, z)).kind == FluidKind::Lava,
                x.abs() + z.abs() <= 3,
                "{x},{z}"
            );
        }
    }
}

#[test]
fn three_neighboring_sources_allow_sideways_spread_while_falling() {
    let mut w = world();
    w.holes.insert((0, 0));
    for pos in [(0, 0, 0), (-1, 0, 0), (1, 0, 0), (0, 0, -1)] {
        w.set_block(pos.0, pos.1, pos.2, id("water"));
    }
    let mut ticks = FluidTicks::default();
    ticks.schedule((0, 0, 0), 0);
    ticks.run_until(&mut w, 0);
    assert!(fluid_state(w.block(0, -1, 0)).falling);
    assert_eq!(fluid_state(w.block(0, 0, 1)).kind, FluidKind::Water);
}
#[test]
fn scheduled_work_waits_until_due_and_duplicate_positions_are_safe() {
    let mut w = world();
    w.set_block(0, 0, 0, id("water"));
    let mut ticks = FluidTicks::default();
    ticks.schedule((0, 0, 0), 10);
    ticks.schedule((0, 0, 0), 10);
    ticks.run_until(&mut w, 9);
    assert_eq!(fluid_state(w.block(1, 0, 0)).kind, FluidKind::Empty);
    ticks.run_until(&mut w, 10);
    assert_eq!(fluid_state(w.block(1, 0, 0)).kind, FluidKind::Water);
}

#[test]
fn duplicate_scheduling_keeps_first_due_time() {
    let mut w = world();
    w.set_block(0, 0, 0, id("water"));
    let mut ticks = FluidTicks::default();
    ticks.schedule((0, 0, 0), 10);
    ticks.schedule((0, 0, 0), 1);
    ticks.run_until(&mut w, 1);
    assert_eq!(fluid_state(w.block(1, 0, 0)).kind, FluidKind::Empty);
    ticks.run_until(&mut w, 10);
    assert_eq!(fluid_state(w.block(1, 0, 0)).kind, FluidKind::Water);
}

#[test]
fn slope_search_can_look_through_existing_flowing_fluid() {
    let mut w = world();
    w.holes.insert((2, 0));
    w.holes.insert((-3, 0));
    w.set_block(0, 0, 0, id("water"));
    w.set_block(
        1,
        0,
        0,
        u32::from(blocks::state_id("water", &[("level", "1")]).unwrap()),
    );
    let mut ticks = FluidTicks::default();
    ticks.schedule((0, 0, 0), 0);
    ticks.run_until(&mut w, 0);
    assert_eq!(fluid_state(w.block(-1, 0, 0)).kind, FluidKind::Empty);
}

#[test]
fn moving_pending_ticks_preserves_remaining_delay() {
    let mut w = world();
    w.set_block(0, 0, 0, id("water"));
    let mut from = FluidTicks::default();
    from.schedule((0, 0, 0), 10);
    from.run_until(&mut w, 4);
    let mut to = FluidTicks::default();
    to.run_until(&mut w, 50);
    from.transfer_to(&mut to, |_| true);
    to.run_until(&mut w, 55);
    assert_eq!(fluid_state(w.block(1, 0, 0)).kind, FluidKind::Empty);
    to.run_until(&mut w, 56);
    assert_eq!(fluid_state(w.block(1, 0, 0)).kind, FluidKind::Water);
}

/// Vanilla waterlogged blocks carry a source fluid state but are not `LiquidBlock`s, so the
/// fluid manager never schedules or spreads them. A waterlogged slab above a hole stays dry.
#[test]
fn waterlogged_block_never_ticks_as_water() {
    let mut w = world();
    w.holes.insert((0, 0));
    let wet = state("stone_slab", &[("type", "bottom"), ("waterlogged", "true")]);
    w.set_block(0, 0, 0, wet);
    let mut ticks = FluidTicks::default();
    ticks.schedule((0, 0, 0), 0);
    ticks.on_block_change(&mut w, (0, 0, 0));
    ticks.run_until(&mut w, 40);
    assert_eq!(w.block(0, 0, 0), wet);
    for pos in [(0, -1, 0), (1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1)] {
        assert_eq!(
            fluid_state(w.block(pos.0, pos.1, pos.2)).kind,
            FluidKind::Empty,
            "water spread to {pos:?}"
        );
    }
}

/// A waterlogged block still counts as water for `LiquidBlock.neighborChanged` lava rules.
#[test]
fn waterlogged_block_beside_lava_still_solidifies_it() {
    let mut w = world();
    w.set_block(
        0,
        0,
        0,
        state("stone_slab", &[("type", "bottom"), ("waterlogged", "true")]),
    );
    w.set_block(1, 0, 0, id("lava"));
    let mut ticks = FluidTicks::default();
    ticks.on_block_change(&mut w, (0, 0, 0));
    assert_eq!(w.block(1, 0, 0), id("obsidian"));
}

#[test]
fn advance_processes_one_relative_tick() {
    let mut w = world();
    w.set_block(0, 0, 0, id("water"));
    let mut ticks = FluidTicks::default();
    ticks.schedule((0, 0, 0), 2);
    ticks.advance(&mut w);
    assert_eq!(fluid_state(w.block(1, 0, 0)).kind, FluidKind::Empty);
    ticks.advance(&mut w);
    assert_eq!(fluid_state(w.block(1, 0, 0)).kind, FluidKind::Water);
}
