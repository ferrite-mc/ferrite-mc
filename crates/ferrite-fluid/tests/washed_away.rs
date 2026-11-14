//! `FlowingFluid.canHoldAnyFluid` / `canHoldSpecificFluid`: what flowing fluid may enter and what
//! the slope search may walk through (checked against the 26.3 bytecode).
use ferrite_data::blocks;
use ferrite_fluid::{
    FluidKind, FluidTicks, FluidWorld, can_hold_fluid, can_hold_fluid_as, fluid_state,
};
use std::collections::{HashMap, HashSet};

struct World {
    cells: HashMap<(i32, i32, i32), u32>,
    holes: HashSet<(i32, i32)>,
}
fn id(name: &str) -> u32 {
    u32::from(blocks::default_state(name).unwrap())
}
impl FluidWorld for World {
    fn block(&self, x: i32, y: i32, z: i32) -> u32 {
        *self
            .cells
            .get(&(x, y, z))
            .unwrap_or(&if y <= -1 && !self.holes.contains(&(x, z)) {
                id("stone")
            } else {
                id("air")
            })
    }
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32) {
        self.cells.insert((x, y, z), state);
    }
    fn ultrawarm(&self) -> bool {
        false
    }
}
fn world() -> World {
    World {
        cells: HashMap::new(),
        holes: HashSet::new(),
    }
}
fn run(w: &mut World, ticks: u64) {
    let mut t = FluidTicks::default();
    t.schedule((0, 0, 0), 0);
    t.run_until(w, ticks);
}

#[test]
fn the_washed_away_tag_decides_what_water_can_replace() {
    // Plants, buttons, torches and carpets are in `washed_away_by_fluids`; a plain stone block,
    // and `structure_void` (replaceable, but not in the tag), are not.
    for name in [
        "oak_sapling",
        "torch",
        "stone_button",
        "white_carpet",
        "air",
        "short_grass",
    ] {
        assert!(can_hold_fluid(id(name), FluidKind::Water), "{name}");
        assert!(can_hold_fluid(id(name), FluidKind::Lava), "{name}");
    }
    for name in ["stone", "structure_void", "dirt"] {
        assert!(!can_hold_fluid(id(name), FluidKind::Water), "{name}");
    }
}

#[test]
fn a_waterloggable_block_only_takes_the_source_type_of_water() {
    let slab = u32::from(
        blocks::state_id(
            "stone_slab",
            &[("type", "bottom"), ("waterlogged", "false")],
        )
        .unwrap(),
    );
    // `SimpleWaterloggedBlock.canPlaceLiquid` is `fluid == Fluids.WATER`: the flowing type and
    // lava do not fit.
    assert!(can_hold_fluid_as(slab, FluidKind::Water, true));
    assert!(!can_hold_fluid_as(slab, FluidKind::Water, false));
    assert!(!can_hold_fluid_as(slab, FluidKind::Lava, true));
    // Kelp and seagrass are containers that never take liquid.
    assert!(!can_hold_fluid_as(id("seagrass"), FluidKind::Water, true));
}

#[test]
fn water_flows_into_and_washes_away_a_sapling() {
    let mut w = world();
    w.set_block(1, 0, 0, id("oak_sapling"));
    w.set_block(0, 0, 0, id("water"));
    run(&mut w, 10);
    assert_eq!(
        fluid_state(w.block(1, 0, 0)).kind,
        FluidKind::Water,
        "the sapling is in the washed-away tag"
    );
}

#[test]
fn the_slope_search_does_not_walk_through_a_waterloggable_block() {
    let mut w = world();
    // A hole two blocks east, behind a slab. The slab cannot take flowing water, so it is
    // neither a candidate nor a path: the water goes round it (north and south both reach the
    // hole in three steps) instead of being stuck waiting for the slab to fill.
    w.holes.insert((2, 0));
    let slab = u32::from(
        blocks::state_id(
            "stone_slab",
            &[("type", "bottom"), ("waterlogged", "false")],
        )
        .unwrap(),
    );
    w.set_block(1, 0, 0, slab);
    w.set_block(0, 0, 0, id("water"));
    run(&mut w, 10);
    assert_eq!(fluid_state(w.block(0, 0, 1)).kind, FluidKind::Water);
    assert_eq!(fluid_state(w.block(0, 0, -1)).kind, FluidKind::Water);
    assert_eq!(
        fluid_state(w.block(1, 0, 0)).kind,
        FluidKind::Empty,
        "the slab stays dry"
    );
}
