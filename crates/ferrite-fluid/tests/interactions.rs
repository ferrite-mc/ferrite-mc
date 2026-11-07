use ferrite_data::blocks;
use ferrite_fluid::{
    FluidKind, FluidTicks, FluidWorld, fluid_state, get_new_liquid, pickup_water, place_water,
};
use std::collections::HashMap;
struct World {
    cells: HashMap<(i32, i32, i32), u32>,
    lava_conversion: bool,
    water_conversion: bool,
    ultrawarm: bool,
}
fn id(s: &str) -> u32 {
    u32::from(blocks::default_state(s).unwrap())
}
fn state(name: &str, props: &[(&str, &str)]) -> u32 {
    u32::from(blocks::state_id(name, props).unwrap())
}
impl FluidWorld for World {
    fn block(&self, x: i32, y: i32, z: i32) -> u32 {
        *self.cells.get(&(x, y, z)).unwrap_or(&id("air"))
    }
    fn set_block(&mut self, x: i32, y: i32, z: i32, s: u32) {
        self.cells.insert((x, y, z), s);
    }
    fn ultrawarm(&self) -> bool {
        self.ultrawarm
    }
    fn lava_conversion(&self) -> bool {
        self.lava_conversion
    }
    fn water_conversion(&self) -> bool {
        self.water_conversion
    }
}
fn world() -> World {
    World {
        cells: HashMap::new(),
        lava_conversion: false,
        water_conversion: true,
        ultrawarm: false,
    }
}
fn tick(w: &mut World, p: (i32, i32, i32)) {
    let mut t = FluidTicks::default();
    t.schedule(p, 0);
    t.run_until(w, 0);
}
#[test]
fn lava_next_to_water_solidifies_by_source_status() {
    for (level, product) in [("0", "obsidian"), ("1", "cobblestone")] {
        let mut w = world();
        w.set_block(0, 0, 0, state("lava", &[("level", level)]));
        w.set_block(1, 0, 0, id("water"));
        tick(&mut w, (0, 0, 0));
        assert_eq!(w.block(0, 0, 0), id(product));
    }
}
#[test]
fn downward_lava_entering_water_makes_stone() {
    let mut w = world();
    w.set_block(0, 0, 0, id("lava"));
    w.set_block(0, -1, 0, id("water"));
    tick(&mut w, (0, 0, 0));
    assert_eq!(w.block(0, -1, 0), id("stone"));
}
#[test]
fn lava_over_soul_soil_next_to_blue_ice_makes_basalt() {
    let mut w = world();
    w.set_block(0, 0, 0, id("lava"));
    w.set_block(0, -1, 0, id("soul_soil"));
    w.set_block(1, 0, 0, id("blue_ice"));
    tick(&mut w, (0, 0, 0));
    assert_eq!(w.block(0, 0, 0), id("basalt"));
}
#[test]
fn scheduled_lava_respects_conversion_rule() {
    let mut w = world();
    w.lava_conversion = true;
    w.set_block(-1, 0, 0, id("lava"));
    w.set_block(1, 0, 0, id("lava"));
    w.set_block(0, -1, 0, id("stone"));
    w.set_block(0, 0, 0, state("lava", &[("level", "1")]));
    tick(&mut w, (0, 0, 0));
    assert!(fluid_state(w.block(0, 0, 0)).source);
}

#[test]
fn lava_source_conversion_stays_disabled_when_rule_is_off() {
    for enabled in [false, true] {
        let mut w = world();
        w.lava_conversion = enabled;
        w.set_block(-1, 0, 0, id("lava"));
        w.set_block(1, 0, 0, id("lava"));
        w.set_block(0, -1, 0, id("stone"));
        w.set_block(0, 0, 0, state("lava", &[("level", "1")]));
        tick(&mut w, (0, 0, 0));
        assert_eq!(fluid_state(w.block(0, 0, 0)).source, enabled);
    }
}

#[test]
fn water_source_conversion_can_be_disabled() {
    for enabled in [false, true] {
        let mut w = world();
        w.water_conversion = enabled;
        w.set_block(-1, 0, 0, id("water"));
        w.set_block(1, 0, 0, id("water"));
        w.set_block(0, -1, 0, id("stone"));
        w.set_block(0, 0, 0, state("water", &[("level", "1")]));
        tick(&mut w, (0, 0, 0));
        assert_eq!(fluid_state(w.block(0, 0, 0)).source, enabled);
    }
}

#[test]
fn ultrawarm_lava_uses_its_dimension_flow_rules() {
    let mut overworld = world();
    let mut nether = world();
    overworld.set_block(1, 0, 0, id("lava"));
    nether.set_block(1, 0, 0, id("lava"));
    overworld.set_block(0, -1, 0, id("stone"));
    nether.set_block(0, -1, 0, id("stone"));
    nether.ultrawarm = true;
    overworld.lava_conversion = false;
    nether.lava_conversion = false;
    let normal = get_new_liquid(&overworld, 0, 0, 0, FluidKind::Lava, false);
    let fast = get_new_liquid(&nether, 0, 0, 0, FluidKind::Lava, false);
    assert_eq!(normal.amount, 6);
    assert_eq!(fast.amount, 7);
    assert_eq!(
        ferrite_fluid::tick_delay(FluidKind::Lava, overworld.ultrawarm()),
        30
    );
    assert_eq!(
        ferrite_fluid::tick_delay(FluidKind::Lava, nether.ultrawarm()),
        10
    );
}
#[test]
fn water_bucket_preserves_waterloggable_blocks() {
    let mut w = world();
    let dry = state(
        "stone_slab",
        &[("type", "bottom"), ("waterlogged", "false")],
    );
    let wet = state("stone_slab", &[("type", "bottom"), ("waterlogged", "true")]);
    w.set_block(0, 0, 0, dry);
    assert!(place_water(&mut w, (0, 0, 0)));
    assert_eq!(w.block(0, 0, 0), wet);
    assert_eq!(fluid_state(w.block(0, 0, 0)).kind, FluidKind::Water);
    assert!(pickup_water(&mut w, (0, 0, 0)));
    assert_eq!(w.block(0, 0, 0), dry);
    assert!(!pickup_water(&mut w, (0, 0, 0)));
}

#[test]
fn water_bucket_fills_cauldron_and_only_full_cauldron_can_be_picked_up() {
    let mut w = world();
    let empty = id("cauldron");
    w.set_block(0, 0, 0, empty);
    assert!(place_water(&mut w, (0, 0, 0)));
    assert_eq!(w.block(0, 0, 0), state("water_cauldron", &[("level", "3")]));
    w.set_block(0, 0, 0, state("water_cauldron", &[("level", "2")]));
    assert!(!pickup_water(&mut w, (0, 0, 0)));
    w.set_block(0, 0, 0, state("water_cauldron", &[("level", "3")]));
    assert!(pickup_water(&mut w, (0, 0, 0)));
    assert_eq!(w.block(0, 0, 0), empty);
}

#[test]
fn flowing_water_does_not_fill_slab_but_source_water_does() {
    let mut w = world();
    let dry = state(
        "stone_slab",
        &[("type", "bottom"), ("waterlogged", "false")],
    );
    let wet = state("stone_slab", &[("type", "bottom"), ("waterlogged", "true")]);
    w.set_block(1, 0, 0, dry);
    w.set_block(0, 0, 0, id("water"));
    w.set_block(0, -1, 0, id("stone"));
    w.set_block(1, -1, 0, id("stone"));
    for (x, z) in [(-1, 0), (0, -1), (0, 1)] {
        w.set_block(x, 0, z, id("stone"));
    }
    tick(&mut w, (0, 0, 0));
    assert_eq!(w.block(1, 0, 0), dry);
    w.set_block(2, 0, 0, id("water"));
    tick(&mut w, (0, 0, 0));
    assert_eq!(w.block(1, 0, 0), wet);
}

#[test]
fn neighbor_change_reacts_immediately_without_waiting_for_tick() {
    let mut w = world();
    w.set_block(0, 0, 0, id("lava"));
    w.set_block(1, 0, 0, id("water"));
    let mut ticks = FluidTicks::default();
    ticks.on_block_change(&mut w, (1, 0, 0));
    assert_eq!(w.block(0, 0, 0), id("obsidian"));
}
