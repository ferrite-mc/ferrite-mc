//! Vanilla fire simulation, region-local: `FireBlock`, `BaseFireBlock` and `SoulFireBlock`.
//!
//! Fire is driven by scheduled block ticks (vanilla `FireBlock.onPlace`/`tick` schedule
//! `getFireTickDelay` = 30 + `random.nextInt(10)`), not by random ticks. A tick ages the fire,
//! lets rain put it out, spreads it into flammable neighbours (`FireBlock.FLAMMABLES`, whose
//! membership comes from the generated block tags) and removes fire without support.
//!
//! Only blocks in chunks this region holds are read or written; a fire next to a region border
//! never ignites the neighbour's blocks.

use super::{Dimension, anvil::AnvilChunkSource, fluid::RegionFluidWorld};
use ferrite_data::{block_props, blocks, tags};
use ferrite_entity::Rng;
use ferrite_fluid::FluidWorld;
use std::collections::HashSet;

/// Vanilla `FireBlock.MAX_AGE`.
pub const MAX_AGE: u8 = 15;

/// The RNG operations `FireBlock` uses, implemented by both the server's entity RNG and the
/// random-tick `RandomSource`.
pub trait FireRng {
    fn next_int(&mut self, bound: i32) -> i32;
    fn next_f32(&mut self) -> f32;
}

impl FireRng for Rng {
    fn next_int(&mut self, bound: i32) -> i32 {
        Rng::next_int(self, bound)
    }
    fn next_f32(&mut self) -> f32 {
        Rng::next_f32(self)
    }
}

/// Vanilla `FireBlock.getFireTickDelay`.
pub fn tick_delay(rng: &mut impl FireRng) -> u32 {
    30 + rng.next_int(10) as u32
}

/// Block access for fire. Unloaded chunks are `None`, never loaded by a fire tick.
pub trait FireWorld {
    fn block(&self, x: i32, y: i32, z: i32) -> Option<u16>;
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: u16);
    /// Whether this region owns the chunk containing `(x, z)`.
    fn owns(&self, x: i32, z: i32) -> bool;
}

impl FireWorld for RegionFluidWorld<'_> {
    fn block(&self, x: i32, y: i32, z: i32) -> Option<u16> {
        if !self.owns(x, z) {
            return None;
        }
        u16::try_from(FluidWorld::block(self, x, y, z)).ok()
    }
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: u16) {
        if self.owns(x, z) {
            FluidWorld::set_block(self, x, y, z, u32::from(state));
        }
    }
    fn owns(&self, x: i32, z: i32) -> bool {
        RegionFluidWorld::owns(self, x, z)
    }
}

impl FireWorld for AnvilChunkSource {
    fn block(&self, x: i32, y: i32, z: i32) -> Option<u16> {
        AnvilChunkSource::block(self, x, y, z).and_then(|state| u16::try_from(state).ok())
    }
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: u16) {
        let _ = AnvilChunkSource::set_block(self, x, y, z, u32::from(state));
    }
    fn owns(&self, _: i32, _: i32) -> bool {
        true
    }
}

fn name(state: u16) -> Option<&'static str> {
    ferrite_data::blocks::state(state).map(|(name, _)| name)
}

fn short(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

fn property(state: u16, key: &str) -> Option<&'static str> {
    ferrite_data::blocks::state(state)?
        .1
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, value)| *value)
}

fn air() -> u16 {
    blocks::default_state("air").expect("vanilla air")
}

pub fn is_fire(state: u16) -> bool {
    name(state).is_some_and(|name| tags::contains("block", "fire", name))
}

pub fn is_soul_fire(state: u16) -> bool {
    name(state) == Some("minecraft:soul_fire")
}

/// Vanilla `FireBlock.FLAMMABLES` burn/flame odds, with membership taken from the generated
/// block tags (planks, logs that burn, leaves, wool, wooden stairs/slabs/fences/doors/...).
pub fn flammable(state: u16) -> Option<(i32, i32)> {
    let name = name(state)?;
    let short_name = short(name);
    if tags::contains("block", "planks", name)
        || tags::contains("block", "wooden_stairs", name)
        || tags::contains("block", "wooden_slabs", name)
        || tags::contains("block", "wooden_fences", name)
        || tags::contains("block", "wooden_doors", name)
        || tags::contains("block", "wooden_trapdoors", name)
        || short_name.ends_with("_fence_gate")
    {
        return Some((5, 20));
    }
    if tags::contains("block", "logs_that_burn", name) {
        return Some((5, 5));
    }
    if tags::contains("block", "leaves", name) || tags::contains("block", "wool", name) {
        return Some((30, 60));
    }
    Some(match short_name {
        "hay_block" => (60, 20),
        "vine" => (15, 100),
        "coal_block" => (5, 10),
        "bookshelf" => (30, 20),
        "crafting_table" => (20, 20),
        "bamboo" | "scaffolding" => (60, 60),
        _ => return None,
    })
}

fn solid_below(world: &impl FireWorld, pos: [i32; 3]) -> bool {
    let Some(below) = world.block(pos[0], pos[1] - 1, pos[2]) else {
        return false;
    };
    // `BlockState.isFaceSturdy`: the top face of the block below is a support.
    !block_props::face_occlusion(below, 3).is_empty()
}

fn soul_base(world: &impl FireWorld, pos: [i32; 3]) -> bool {
    world
        .block(pos[0], pos[1] - 1, pos[2])
        .and_then(name)
        .is_some_and(|name| matches!(short(name), "soul_sand" | "soul_soil"))
}

const NEIGHBOURS: [[i32; 3]; 6] = [
    [-1, 0, 0],
    [1, 0, 0],
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
];

fn neighbour_fire_or_fuel(world: &impl FireWorld, pos: [i32; 3]) -> bool {
    NEIGHBOURS.iter().any(|offset| {
        let target = [pos[0] + offset[0], pos[1] + offset[1], pos[2] + offset[2]];
        world
            .block(target[0], target[1], target[2])
            .is_some_and(|state| is_fire(state) || flammable(state).is_some())
    })
}

/// Vanilla `FireBlock.canSurvive` / `BaseFireBlock.isValidFireLocation`: fire needs a sturdy
/// floor, another fire, or a flammable block beside it. Soul fire only survives on soul soil
/// or soul sand (`SoulFireBlock.canSurvive`).
pub fn can_survive(world: &impl FireWorld, pos: [i32; 3], state: u16) -> bool {
    if is_soul_fire(state) {
        return soul_base(world, pos);
    }
    // A region border counts as support: never break another region's fire.
    world.block(pos[0], pos[1] - 1, pos[2]).is_none()
        || solid_below(world, pos)
        || neighbour_fire_or_fuel(world, pos)
}

/// Vanilla `BaseFireBlock.canBePlacedAt`: the target is air (or replaceable) over a support.
pub fn can_be_placed_at(world: &impl FireWorld, pos: [i32; 3]) -> bool {
    let Some(state) = world.block(pos[0], pos[1], pos[2]) else {
        return false;
    };
    let props = block_props::get(state);
    (props.is_air || props.replaceable) && (solid_below(world, pos) || soul_base(world, pos))
}

/// Vanilla `BaseFireBlock.getState` / `SoulFireBlock`: soul fire on soul sand or soul soil.
pub fn placement_state(world: &impl FireWorld, pos: [i32; 3]) -> Option<u16> {
    if !can_be_placed_at(world, pos) {
        return None;
    }
    if soul_base(world, pos) {
        blocks::default_state("soul_fire")
    } else {
        blocks::default_state("fire")
    }
}

fn with_age(state: u16, age: u8) -> Option<u16> {
    let (name, props) = blocks::state(state)?;
    let mut properties = props.to_vec();
    let age_text = age.to_string();
    for (key, value) in &mut properties {
        if *key == "age" {
            *value = &age_text;
        }
    }
    blocks::state_id(name, &properties)
}

/// The state fire has at `pos`: `SoulFireBlock` over soul sand/soil, plain fire otherwise.
fn fire_state(world: &impl FireWorld, pos: [i32; 3]) -> Option<u16> {
    if soul_base(world, pos) {
        blocks::default_state("soul_fire")
    } else {
        blocks::default_state("fire")
    }
}

/// Replaces a burning flammable block with fire (vanilla `FireBlock.checkBurn`), which does
/// not need an air target.
fn burn_block(world: &mut impl FireWorld, pos: [i32; 3]) -> bool {
    let Some(state) = fire_state(world, pos) else {
        return false;
    };
    if world.block(pos[0], pos[1], pos[2]) == Some(state) {
        return false;
    }
    world.set_block(pos[0], pos[1], pos[2], state);
    true
}

fn set_fire(world: &mut impl FireWorld, pos: [i32; 3]) -> bool {
    let Some(state) = placement_state(world, pos) else {
        return false;
    };
    if world.block(pos[0], pos[1], pos[2]) == Some(state) {
        return false;
    }
    world.set_block(pos[0], pos[1], pos[2], state);
    true
}

/// One `FireBlock.tick` (without the reschedule). Returns whether the fire block itself
/// changed, and pushes fires it placed into `spawned` so the caller schedules them.
pub fn tick_one(
    world: &mut impl FireWorld,
    pos: [i32; 3],
    state: u16,
    rng: &mut impl FireRng,
    raining: bool,
    infiniburn: bool,
    spawned: &mut Vec<[i32; 3]>,
) -> bool {
    if !can_survive(world, pos, state) {
        world.set_block(pos[0], pos[1], pos[2], air());
        return true;
    }
    // `SoulFireBlock` never ages or spreads; it only disappears without its base.
    if is_soul_fire(state) {
        return false;
    }
    let age = property(state, "age")
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or(0);
    // Vanilla rain extinguish odds: `0.2 + age * 0.03`.
    if !infiniburn && raining && rng.next_f32() < 0.2 + f32::from(age) * 0.03 {
        world.set_block(pos[0], pos[1], pos[2], air());
        return true;
    }
    let mut changed = false;
    // `FireBlock.checkBurn`: a flammable neighbour catches at 1/burnOdds, and the air around
    // a flammable block catches at 1/flameOdds.
    for offset in NEIGHBOURS {
        let target = [pos[0] + offset[0], pos[1] + offset[1], pos[2] + offset[2]];
        let Some(neighbour) = world.block(target[0], target[1], target[2]) else {
            continue;
        };
        if let Some((burn, _)) = flammable(neighbour) {
            if rng.next_int(burn.max(1)) == 0 && burn_block(world, target) {
                spawned.push(target);
            }
            continue;
        }
        let props = block_props::get(neighbour);
        if !props.is_air && !props.replaceable {
            continue;
        }
        let flame = NEIGHBOURS.iter().find_map(|near| {
            let around = [
                target[0] + near[0],
                target[1] + near[1],
                target[2] + near[2],
            ];
            world
                .block(around[0], around[1], around[2])
                .and_then(flammable)
                .map(|(_, flame)| flame)
        });
        if let Some(flame) = flame
            && rng.next_int(flame.max(1)) == 0
            && set_fire(world, target)
        {
            spawned.push(target);
        }
    }
    // `FireBlock.tick` ages fire by `random.nextInt(3) / 2`; infinity-burning floors (the
    // dimension's `infiniburn` tag) keep their age.
    let next_age = if infiniburn {
        age
    } else {
        (age + (rng.next_int(3) / 2) as u8).min(MAX_AGE)
    };
    if next_age != age
        && let Some(state) = with_age(state, next_age)
    {
        world.set_block(pos[0], pos[1], pos[2], state);
        changed = true;
    }
    // Fire without fuel and without a sturdy floor burns out once it is older than 3.
    if !infiniburn
        && next_age > 3
        && !neighbour_fire_or_fuel(world, pos)
        && !solid_below(world, pos)
    {
        world.set_block(pos[0], pos[1], pos[2], air());
        changed = true;
    }
    changed
}

/// Scheduled fire ticks for one region (vanilla `Level.scheduleTick` for `FireBlock`).
#[derive(Debug, Default)]
pub struct FireTicks {
    clock: u32,
    due: Vec<([i32; 3], u32)>,
    scheduled: HashSet<[i32; 3]>,
}

impl FireTicks {
    pub fn schedule(&mut self, pos: [i32; 3], delay: u32) {
        if self.scheduled.insert(pos) {
            self.due.push((pos, self.clock.wrapping_add(delay.max(1))));
        }
    }

    pub fn split(&mut self, keep: impl Fn(i32, i32) -> bool) -> Self {
        let mut out = Self {
            clock: self.clock,
            due: Vec::new(),
            scheduled: HashSet::new(),
        };
        let mut moved = Vec::new();
        self.due.retain(|&(pos, due)| {
            if keep(pos[0], pos[2]) {
                moved.push((pos, due));
                false
            } else {
                true
            }
        });
        for (pos, due) in moved {
            self.scheduled.remove(&pos);
            out.scheduled.insert(pos);
            out.due.push((pos, due));
        }
        out
    }

    pub fn merge(&mut self, other: Self) {
        for (pos, due) in other.due {
            if self.scheduled.insert(pos) {
                self.due.push((pos, due));
            }
        }
    }

    /// Runs every due fire tick. `changed` receives blocks whose state a neighbour update must
    /// see (removed fire, new fire, a new age).
    pub fn tick(
        &mut self,
        world: &mut impl FireWorld,
        dimension: Dimension,
        raining: bool,
        rng: &mut impl FireRng,
        changed: &mut Vec<[i32; 3]>,
    ) {
        self.clock = self.clock.wrapping_add(1);
        let mut index = 0;
        while index < self.due.len() {
            if self.due[index].1 > self.clock {
                index += 1;
                continue;
            }
            let (pos, _) = self.due.swap_remove(index);
            self.scheduled.remove(&pos);
            if !world.owns(pos[0], pos[2]) {
                continue;
            }
            let Some(state) = world.block(pos[0], pos[1], pos[2]) else {
                continue;
            };
            if !is_fire(state) {
                continue;
            }
            // Vanilla `FireBlock.tick` checks the dimension's infiniburn tag on the floor.
            let infiniburn = world
                .block(pos[0], pos[1] - 1, pos[2])
                .and_then(name)
                .is_some_and(|below| tags::contains("block", dimension.infiniburn_tag(), below));
            let mut spawned = Vec::new();
            if tick_one(world, pos, state, rng, raining, infiniburn, &mut spawned) {
                changed.push(pos);
            }
            for spawned in spawned {
                self.schedule(spawned, tick_delay(rng));
                changed.push(spawned);
            }
            if world.block(pos[0], pos[1], pos[2]).is_some_and(is_fire) {
                self.schedule(pos, tick_delay(rng));
            }
        }
    }
}

/// Vanilla `LavaFluid.randomTick` fire part: a lava block with flammable blocks around it
/// (or above it) starts a fire on a nearby supported air block. Returns the fire position.
pub fn lava_ignites(
    world: &mut impl FireWorld,
    pos: [i32; 3],
    rng: &mut impl FireRng,
) -> Option<[i32; 3]> {
    if rng.next_int(3) != 0 {
        return None;
    }
    let mut fuel_nearby = false;
    for x in -1..=1 {
        for y in -1..=1 {
            for z in -1..=1 {
                if let Some(state) = world.block(pos[0] + x, pos[1] + y, pos[2] + z)
                    && flammable(state).is_some()
                {
                    fuel_nearby = true;
                }
            }
        }
    }
    if !fuel_nearby {
        return None;
    }
    for offset in NEIGHBOURS {
        let target = [pos[0] + offset[0], pos[1] + offset[1], pos[2] + offset[2]];
        if can_be_placed_at(world, target) && set_fire(world, target) {
            return Some(target);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct Grid {
        blocks: HashMap<[i32; 3], u16>,
    }

    impl Grid {
        fn new() -> Self {
            Self {
                blocks: HashMap::new(),
            }
        }
        fn set(&mut self, pos: [i32; 3], state: u16) {
            self.blocks.insert(pos, state);
        }
        fn get(&self, pos: [i32; 3]) -> u16 {
            *self.blocks.get(&pos).unwrap_or(&0)
        }
    }

    impl FireWorld for Grid {
        fn block(&self, x: i32, y: i32, z: i32) -> Option<u16> {
            Some(self.get([x, y, z]))
        }
        fn set_block(&mut self, x: i32, y: i32, z: i32, state: u16) {
            self.blocks.insert([x, y, z], state);
        }
        fn owns(&self, _: i32, _: i32) -> bool {
            true
        }
    }

    fn state(name: &str) -> u16 {
        blocks::default_state(name).unwrap()
    }

    #[test]
    fn flammability_comes_from_the_generated_tags() {
        assert_eq!(flammable(state("oak_planks")), Some((5, 20)));
        assert_eq!(flammable(state("spruce_stairs")), Some((5, 20)));
        assert_eq!(flammable(state("oak_log")), Some((5, 5)));
        assert_eq!(flammable(state("oak_leaves")), Some((30, 60)));
        assert_eq!(flammable(state("white_wool")), Some((30, 60)));
        assert_eq!(flammable(state("hay_block")), Some((60, 20)));
        assert_eq!(flammable(state("stone")), None);
        assert_eq!(flammable(state("water")), None);
    }

    #[test]
    fn soul_fire_needs_soul_soil_and_fire_needs_support() {
        let mut grid = Grid::new();
        grid.set([0, 63, 0], state("soul_soil"));
        grid.set([0, 64, 0], state("soul_fire"));
        assert!(can_survive(&grid, [0, 64, 0], state("soul_fire")));
        assert_eq!(placement_state(&grid, [0, 64, 0]), Some(state("soul_fire")));
        grid.set([0, 63, 0], state("stone"));
        assert!(!can_survive(&grid, [0, 64, 0], state("soul_fire")));
        grid.set([0, 64, 0], state("fire"));
        assert!(can_survive(&grid, [0, 64, 0], state("fire")));
        grid.set([0, 63, 0], state("air"));
        assert!(!can_survive(&grid, [0, 64, 0], state("fire")));
        // Fire beside planks survives even without a floor.
        grid.set([1, 64, 0], state("oak_planks"));
        assert!(can_survive(&grid, [0, 64, 0], state("fire")));
    }

    #[test]
    fn fire_ages_burns_planks_and_rain_extinguishes() {
        let mut grid = Grid::new();
        grid.set([0, 63, 0], state("stone"));
        grid.set([0, 64, 0], state("fire"));
        grid.set([1, 64, 0], state("oak_planks"));
        let mut rng = Rng::new(3);
        let mut spawned = Vec::new();
        let mut burned = false;
        for _ in 0..40 {
            spawned.clear();
            let current = grid.get([0, 64, 0]);
            tick_one(
                &mut grid,
                [0, 64, 0],
                current,
                &mut rng,
                false,
                false,
                &mut spawned,
            );
            if grid.get([1, 64, 0]) == state("fire") {
                burned = true;
                break;
            }
        }
        assert!(burned, "fire never caught the planks");
        // A rainy tick at a high age puts it out (odds 0.2 + age*0.03).
        grid.set([0, 64, 0], super::with_age(state("fire"), 15).unwrap());
        let mut rng = Rng::new(1);
        let mut removed = false;
        for _ in 0..50 {
            let current = grid.get([0, 64, 0]);
            if tick_one(
                &mut grid,
                [0, 64, 0],
                current,
                &mut rng,
                true,
                false,
                &mut spawned,
            ) && grid.get([0, 64, 0]) == 0
            {
                removed = true;
                break;
            }
        }
        assert!(removed, "rain never extinguished the fire");
    }

    #[test]
    fn scheduled_fire_reschedules_itself() {
        let mut grid = Grid::new();
        grid.set([0, 63, 0], state("stone"));
        grid.set([0, 64, 0], state("fire"));
        let mut ticks = FireTicks::default();
        ticks.schedule([0, 64, 0], 2);
        let mut rng = Rng::new(5);
        let mut changed = Vec::new();
        for _ in 0..10 {
            ticks.tick(
                &mut grid,
                Dimension::Overworld,
                false,
                &mut rng,
                &mut changed,
            );
        }
        assert!(ticks.due.iter().any(|(pos, _)| *pos == [0, 64, 0]));
        assert!(is_fire(grid.get([0, 64, 0])));
    }
}
